//! Local knowledge library — indexes reference PDFs and Markdown (standards,
//! curricula, technical books, and original synthesis notes the user drops
//! into one or more folders) into an in-memory, keyword-scored (BM25) index
//! so IntentOS's stage prompts can ground architecture/development decisions
//! in real citations instead of relying solely on the model's own
//! recollection.
//!
//! Deliberately NOT a vector/embeddings RAG: IntentOS's local inference
//! path is "sovereign" (loopback-only, no network — see `local_model.rs`),
//! and adding an embeddings model would either require a second local
//! model download or a network call, breaking that guarantee for a
//! reference-lookup feature that doesn't need semantic search to be useful.
//! BM25 term-frequency scoring runs entirely offline with no extra model.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Cache file name under `AppState.app_data_dir`. Shared by `state.rs`
/// (best-effort load at startup) and `commands::knowledge` (write after
/// every reindex).
pub const CACHE_FILE: &str = "knowledge-index.json";

const MAX_CHUNK_CHARS: usize = 1100;
const MIN_CHUNK_CHARS: usize = 200;
const MAX_SNIPPET_CHARS: usize = 420;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeChunk {
    pub source_title: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedChunk {
    pub source_title: String,
    pub snippet: String,
    pub score: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeStatus {
    pub source_count: usize,
    pub chunk_count: usize,
    pub indexed_at: Option<String>,
    pub sources: Vec<String>,
}

/// The built index. Cheap to serialize (plain JSON) and small enough
/// (thousands of chunks, not millions) that a hand-rolled inverted index
/// beats pulling in a full-text-search engine dependency for this.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KnowledgeIndex {
    chunks: Vec<KnowledgeChunk>,
    /// term -> list of (chunk index, term frequency in that chunk).
    postings: HashMap<String, Vec<(u32, u32)>>,
    avg_chunk_len: f32,
    indexed_at: Option<String>,
}

impl KnowledgeIndex {
    pub fn status(&self) -> KnowledgeStatus {
        let mut titles: Vec<String> = self.chunks.iter().map(|c| c.source_title.clone()).collect();
        titles.sort();
        titles.dedup();
        KnowledgeStatus {
            source_count: titles.len(),
            chunk_count: self.chunks.len(),
            indexed_at: self.indexed_at.clone(),
            sources: titles,
        }
    }

    /// BM25-ranked top-`k` chunks for `query`. Empty when the index has
    /// nothing indexed yet or the query has no scorable terms — callers
    /// treat an empty result as "no grounding available", never an error.
    pub fn retrieve(&self, query: &str, k: usize) -> Vec<RetrievedChunk> {
        if self.chunks.is_empty() {
            return Vec::new();
        }
        let terms = tokenize(query);
        if terms.is_empty() {
            return Vec::new();
        }
        const K1: f32 = 1.5;
        const B: f32 = 0.75;
        let n = self.chunks.len() as f32;
        let mut scores = vec![0f32; self.chunks.len()];
        for term in &terms {
            let Some(postings) = self.postings.get(term) else {
                continue;
            };
            let df = postings.len() as f32;
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln().max(0.0);
            for &(chunk_idx, tf) in postings {
                let chunk_len = self.chunks[chunk_idx as usize].text.len() as f32;
                let tf = tf as f32;
                let denom = tf + K1 * (1.0 - B + B * chunk_len / self.avg_chunk_len.max(1.0));
                scores[chunk_idx as usize] += idf * (tf * (K1 + 1.0)) / denom.max(0.0001);
            }
        }
        let mut ranked: Vec<(usize, f32)> = scores
            .into_iter()
            .enumerate()
            .filter(|&(_, s)| s > 0.0)
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
            .into_iter()
            .take(k)
            .map(|(idx, score)| {
                let c = &self.chunks[idx];
                RetrievedChunk {
                    source_title: c.source_title.clone(),
                    snippet: truncate_chars(&c.text, MAX_SNIPPET_CHARS).to_string(),
                    score,
                }
            })
            .collect()
    }

    /// Absorb one new source directly into the live index — no rescan of
    /// any folder, no knowledge of which directories built the rest of the
    /// index. This is how a run's own outcome (e.g. "QA failed here and was
    /// fixed") gets folded into the shared memory the whole agent network
    /// reads from, without requiring the caller to know or replay the
    /// dirs/settings that produced everything already in `chunks`.
    pub fn append(&mut self, source_title: String, text: &str) {
        for chunk in chunk_text(text) {
            let idx = self.chunks.len() as u32;
            let mut tf: HashMap<String, u32> = HashMap::new();
            for term in tokenize(&chunk) {
                *tf.entry(term).or_insert(0) += 1;
            }
            for (term, freq) in tf {
                self.postings.entry(term).or_default().push((idx, freq));
            }
            self.chunks.push(KnowledgeChunk {
                source_title: source_title.clone(),
                text: chunk,
            });
        }
        let total_len: usize = self.chunks.iter().map(|c| c.text.len()).sum();
        self.avg_chunk_len = if self.chunks.is_empty() {
            0.0
        } else {
            total_len as f32 / self.chunks.len() as f32
        };
        self.indexed_at = Some(chrono::Utc::now().to_rfc3339());
    }
}

fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|w| w.chars().count() > 2)
        .map(|w| w.to_string())
        .collect()
}

/// Paragraph-aware chunking: keeps paragraphs intact up to `MAX_CHUNK_CHARS`,
/// merging short ones and hard-splitting the rare paragraph that is itself
/// longer than that (some PDFs extract as one giant run with no breaks).
fn chunk_text(raw: &str) -> Vec<String> {
    let paragraphs: Vec<String> = raw
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .collect();
    let mut chunks = Vec::new();
    let mut buf = String::new();
    for p in paragraphs {
        if buf.len() + p.len() + 1 > MAX_CHUNK_CHARS && buf.len() >= MIN_CHUNK_CHARS {
            chunks.push(std::mem::take(&mut buf));
        }
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(&p);
        while buf.len() > MAX_CHUNK_CHARS * 2 {
            let split_at = (0..=MAX_CHUNK_CHARS.min(buf.len()))
                .rev()
                .find(|&i| buf.is_char_boundary(i))
                .unwrap_or(0);
            let tail = buf.split_off(split_at);
            chunks.push(std::mem::replace(&mut buf, tail));
        }
    }
    if !buf.trim().is_empty() {
        chunks.push(buf);
    }
    chunks
}

fn is_supported_doc(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pdf") || e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

fn collect_docs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_docs(&path, out);
        } else if is_supported_doc(&path) {
            out.push(path);
        }
    }
}

pub fn scan_docs(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in dirs {
        collect_docs(dir, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("documento")
        .to_string()
}

/// Extract raw text from one source file. PDF extraction is CPU-bound and
/// `pdf_extract` panics on some malformed/corrupt PDFs instead of returning
/// `Err` (observed on a real book in the wild: "missing object reference"),
/// so it runs behind `catch_unwind` — a panic is treated exactly like an
/// extraction error. Markdown is read as-is (no stripping of headings/links:
/// they're short, readable, and still useful as retrieval context).
fn extract_text(path: &Path) -> Result<String, String> {
    let is_markdown = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false);
    if is_markdown {
        return std::fs::read_to_string(path).map_err(|e| e.to_string());
    }
    match std::panic::catch_unwind(|| pdf_extract::extract_text(path)) {
        Ok(Ok(t)) => Ok(t),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("panicked during extraction".to_string()),
    }
}

/// Build the index synchronously — text extraction is CPU/IO-bound, so
/// callers run this inside `spawn_blocking`. A source that fails to extract
/// (corrupt PDF, scanned-image-only, unreadable file) is skipped and logged
/// rather than failing the whole reindex; only "zero readable sources across
/// the whole corpus" is a hard error, since that means the configured
/// folders are wrong.
pub fn build_index(dirs: &[PathBuf]) -> Result<KnowledgeIndex, AppError> {
    let paths = scan_docs(dirs);
    let mut chunks = Vec::new();
    let mut skipped = 0usize;
    for path in &paths {
        let text = match extract_text(path) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("knowledge: skipping unreadable source {}: {e}", path.display());
                skipped += 1;
                continue;
            }
        };
        let title = title_from_path(path);
        for chunk in chunk_text(&text) {
            chunks.push(KnowledgeChunk {
                source_title: title.clone(),
                text: chunk,
            });
        }
    }
    if chunks.is_empty() && !paths.is_empty() {
        return Err(AppError::Internal {
            message: format!(
                "could not extract text from any of {} source(s) ({skipped} skipped)",
                paths.len()
            ),
        });
    }

    let mut postings: HashMap<String, Vec<(u32, u32)>> = HashMap::new();
    let mut total_len = 0usize;
    for (idx, chunk) in chunks.iter().enumerate() {
        total_len += chunk.text.len();
        let mut tf: HashMap<String, u32> = HashMap::new();
        for term in tokenize(&chunk.text) {
            *tf.entry(term).or_insert(0) += 1;
        }
        for (term, freq) in tf {
            postings.entry(term).or_default().push((idx as u32, freq));
        }
    }
    let avg_chunk_len = if chunks.is_empty() {
        0.0
    } else {
        total_len as f32 / chunks.len() as f32
    };

    Ok(KnowledgeIndex {
        chunks,
        postings,
        avg_chunk_len,
        indexed_at: Some(chrono::Utc::now().to_rfc3339()),
    })
}

// =====================================================================
// Tauri integration — default corpus + startup seed
// =====================================================================

/// Folders always included alongside whatever the user registers in
/// Settings > Biblioteca, so the repo's own curated corpus
/// (`knowledge-base/` at the workspace root — SWEBOK, CS2023, OWASP ASVS,
/// original synthesis notes) grounds stage prompts without anyone having to
/// find and register that folder by hand.
///
/// Resolved from `CARGO_MANIFEST_DIR` (this crate lives in `<repo>/src-tauri`,
/// so its parent is the workspace root) — this only resolves in a source
/// checkout. A bundled/installed build has no `knowledge-base/` next to it;
/// there is no packaging step yet that ships this corpus with a built app,
/// so on such a build this contributes nothing (an unreadable/missing dir is
/// already handled as "zero docs found there", not an error, by
/// `collect_docs`).
pub fn default_dirs() -> Vec<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|repo_root| vec![repo_root.join("knowledge-base")])
        .unwrap_or_default()
}

/// Where the network's own learnings live — one Markdown file per recorded
/// outcome (see `record_learning`), always under the per-install app data
/// dir so it survives reindexes/updates and needs no source checkout,
/// unlike `default_dirs()`. Included in every reindex alongside the
/// curated corpus and whatever the user registers.
pub fn learnings_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("knowledge-learnings")
}

fn slugify(s: &str) -> String {
    let slug: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "learning".to_string()
    } else {
        slug.chars().take(60).collect()
    }
}

/// Folds a run's own outcome into the shared knowledge every agent reads
/// from — the "the network learns from what actually happened" loop.
/// Writes a durable Markdown file under `learnings_dir()` (so a cold
/// restart / full reindex picks it up too, via the dirs merge in
/// `commands::knowledge::knowledge_index` and `spawn_default_index_seed`)
/// AND appends it directly into whatever index is already cached, so the
/// lesson is retrievable immediately, in the same run, without needing a
/// full rescan of every registered folder.
pub async fn record_learning(
    app_data_dir: &Path,
    cache: &std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<KnowledgeIndex>>>>,
    source_title: &str,
    text: String,
) -> Result<(), AppError> {
    let dir = learnings_dir(app_data_dir);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| AppError::Io {
        message: format!("could not create {}: {e}", dir.display()),
    })?;
    let fname = format!(
        "{}-{}.md",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
        slugify(source_title)
    );
    let content = format!("# {source_title}\n\n{text}\n");
    crate::util::fs::atomic_write(&dir.join(fname), content.as_bytes()).await?;

    let mut guard = cache.lock().await;
    let mut idx = match guard.take() {
        Some(arc) => (*arc).clone(),
        None => KnowledgeIndex::default(),
    };
    idx.append(source_title.to_string(), &text);
    if let Ok(bytes) = serde_json::to_vec(&idx) {
        let cache_path = app_data_dir.join(CACHE_FILE);
        let _ = crate::util::fs::atomic_write(&cache_path, &bytes).await;
    }
    *guard = Some(std::sync::Arc::new(idx));
    Ok(())
}

/// Best-effort startup seed: if no index is cached yet (first launch, or a
/// prior run never reindexed), build one from `default_dirs()` alone so
/// architecture/development stage prompts have real grounding from day one
/// — no dependency on the user ever opening Settings > Biblioteca. A user
/// who later registers their own folders there gets a full reindex that
/// still includes this default corpus (`commands::knowledge::knowledge_index`
/// merges `default_dirs()` with whatever they pass).
///
/// PDF/Markdown extraction is too slow to block app startup, so this runs
/// detached via `tauri::async_runtime::spawn`; failures are logged and just
/// leave the cache at `None`, the same "no grounding available" degrade
/// used everywhere else in this module.
pub fn spawn_default_index_seed<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        let state: tauri::State<crate::state::AppState> = app.state();
        {
            let guard = state.knowledge_cache.lock().await;
            if guard.is_some() {
                return;
            }
        }
        let mut dirs = default_dirs();
        dirs.push(learnings_dir(&state.app_data_dir));
        let built = match tokio::task::spawn_blocking(move || build_index(&dirs)).await {
            Ok(Ok(idx)) => idx,
            Ok(Err(e)) => {
                tracing::warn!("knowledge: default corpus seed skipped: {e}");
                return;
            }
            Err(e) => {
                tracing::warn!("knowledge: default corpus seed task panicked: {e}");
                return;
            }
        };
        if let Ok(bytes) = serde_json::to_vec(&built) {
            let cache_path = state.app_data_dir.join(CACHE_FILE);
            let _ = crate::util::fs::atomic_write(&cache_path, &bytes).await;
        }
        let source_count = built.status().source_count;
        *state.knowledge_cache.lock().await = Some(std::sync::Arc::new(built));
        tracing::info!("knowledge: seeded default corpus index at startup ({source_count} source(s))");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_from_texts(texts: &[(&str, &str)]) -> KnowledgeIndex {
        let mut chunks = Vec::new();
        for (title, text) in texts {
            for chunk in chunk_text(text) {
                chunks.push(KnowledgeChunk {
                    source_title: (*title).to_string(),
                    text: chunk,
                });
            }
        }
        let mut postings: HashMap<String, Vec<(u32, u32)>> = HashMap::new();
        let mut total_len = 0usize;
        for (idx, chunk) in chunks.iter().enumerate() {
            total_len += chunk.text.len();
            let mut tf: HashMap<String, u32> = HashMap::new();
            for term in tokenize(&chunk.text) {
                *tf.entry(term).or_insert(0) += 1;
            }
            for (term, freq) in tf {
                postings.entry(term).or_default().push((idx as u32, freq));
            }
        }
        let avg_chunk_len = if chunks.is_empty() {
            0.0
        } else {
            total_len as f32 / chunks.len() as f32
        };
        KnowledgeIndex {
            chunks,
            postings,
            avg_chunk_len,
            indexed_at: Some("2026-01-01T00:00:00Z".into()),
        }
    }

    #[test]
    fn retrieve_ranks_the_chunk_that_actually_mentions_the_query_terms_first() {
        let idx = index_from_texts(&[
            (
                "OWASP-ASVS",
                "Authentication verification requirements cover password policy, session tokens, and multi-factor enforcement across every login flow.",
            ),
            (
                "SWEBOK",
                "Software design principles cover modularity, coupling and cohesion as the foundation of maintainable architecture.",
            ),
        ]);
        let results = idx.retrieve("session token authentication requirements", 2);
        assert!(!results.is_empty());
        assert_eq!(results[0].source_title, "OWASP-ASVS");
    }

    #[test]
    fn retrieve_returns_empty_when_the_index_has_nothing() {
        let idx = KnowledgeIndex::default();
        assert!(idx.retrieve("anything", 3).is_empty());
    }

    #[test]
    fn retrieve_returns_empty_for_a_query_with_no_scorable_terms() {
        let idx = index_from_texts(&[("Book", "Some real paragraph with real words in it.")]);
        assert!(idx.retrieve("a an is", 3).is_empty());
    }

    #[test]
    fn chunk_text_merges_short_paragraphs_and_splits_a_giant_one() {
        let short = "One.\n\nTwo.\n\nThree.";
        let chunks = chunk_text(short);
        assert_eq!(chunks.len(), 1, "short paragraphs should merge into one chunk");

        let giant = "word ".repeat(1000);
        let chunks = chunk_text(&giant);
        assert!(chunks.len() > 1, "a paragraph far past MAX_CHUNK_CHARS must hard-split");
        for c in &chunks {
            assert!(c.len() <= MAX_CHUNK_CHARS * 2);
        }
    }

    #[test]
    fn status_deduplicates_and_sorts_source_titles() {
        let idx = index_from_texts(&[
            ("Zeta Book", "content about zeta topics here for indexing."),
            ("Alpha Book", "content about alpha topics here for indexing."),
            ("Zeta Book", "more zeta content, a second chunk from the same source."),
        ]);
        let status = idx.status();
        assert_eq!(status.sources, vec!["Alpha Book".to_string(), "Zeta Book".to_string()]);
        assert_eq!(status.source_count, 2);
    }

    /// Verifies `default_dirs()` actually resolves to the repo's own
    /// `knowledge-base/` and that the corpus there (including the
    /// `synthesis/*.md` content, not just the `canonical/*.pdf` sources)
    /// gets indexed. Ignored by default like the smoke test below — it
    /// depends on the checked-in corpus, not portable to every checkout
    /// state. Run explicitly:
    ///   cargo test --lib knowledge::tests::default_dirs_indexes_the_repo_corpus_including_markdown -- --ignored --nocapture
    #[test]
    #[ignore]
    fn default_dirs_indexes_the_repo_corpus_including_markdown() {
        let dirs = default_dirs();
        assert_eq!(dirs.len(), 1, "expected exactly the repo's knowledge-base/ dir");
        let idx = build_index(&dirs).expect("build_index over the repo's own knowledge-base");
        let status = idx.status();
        println!("sources: {:?}", status.sources);
        assert!(
            status.sources.iter().any(|s| s.contains("mental-models")),
            "expected the synthesis/*.md source to be indexed alongside the canonical PDFs"
        );
        assert!(status.source_count >= 4, "expected the 3 canonical PDFs plus at least one synthesis doc");
    }

    /// Real-world smoke test against the actual configured libraries on
    /// this dev machine (not portable to CI/other checkouts — ignored by
    /// default). Run explicitly:
    ///   cargo test --lib knowledge::tests::real_libraries_index_and_retrieve_something_relevant -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_libraries_index_and_retrieve_something_relevant() {
        let dirs = vec![
            PathBuf::from("../knowledge-base/canonical"),
            PathBuf::from(r"C:\Users\fitap\Desktop\intentOS\libros"),
        ];
        let idx = build_index(&dirs).expect("build_index over the real libraries");
        let status = idx.status();
        println!("sources: {:?}", status.sources);
        println!("chunk_count: {}", status.chunk_count);
        assert!(status.chunk_count > 100, "expected a real corpus, got {} chunks", status.chunk_count);
        assert!(status.source_count >= 5, "expected most of the 10 configured PDFs to extract");

        for query in [
            "authentication session token security requirements",
            "database schema design normalization",
            "docker container deployment pipeline",
        ] {
            let hits = idx.retrieve(query, 3);
            println!("\nquery: {query}");
            for h in &hits {
                println!("  [{:.2}] {} :: {}", h.score, h.source_title, &h.snippet[..h.snippet.len().min(120)]);
            }
            assert!(!hits.is_empty(), "expected at least one hit for {query:?}");
        }
    }

    #[test]
    fn scan_docs_recurses_dedupes_and_accepts_pdf_and_markdown() {
        let dir = std::env::temp_dir().join(format!("knowledge-scan-test-{}", uuid::Uuid::new_v4()));
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.join("a.pdf"), b"x").unwrap();
        std::fs::write(nested.join("b.PDF"), b"x").unwrap();
        std::fs::write(nested.join("c.md"), b"x").unwrap();
        std::fs::write(dir.join("ignore.txt"), b"x").unwrap();
        std::fs::write(dir.join("ignore.yml"), b"x").unwrap();

        let found = scan_docs(&[dir.clone(), dir.clone()]);
        assert_eq!(found.len(), 3, "duplicate input dirs must not duplicate results");
        assert!(found.iter().all(|p| is_supported_doc(p)));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn build_index_indexes_markdown_sources_not_just_pdf() {
        let dir = std::env::temp_dir().join(format!("knowledge-md-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mental-models.md"),
            "Kahneman's System 1 and System 2 describe fast intuitive judgment versus slow deliberate reasoning, directly relevant to how an agent should decide when to escalate instead of guessing.",
        )
        .unwrap();

        let idx = build_index(&[dir.clone()]).expect("markdown-only corpus should index");
        assert_eq!(idx.status().source_count, 1);
        let hits = idx.retrieve("intuitive judgment escalate guessing", 3);
        assert!(!hits.is_empty(), "expected the markdown source to be retrievable");
        assert_eq!(hits[0].source_title, "mental-models");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_makes_a_new_source_immediately_retrievable() {
        let mut idx = index_from_texts(&[("OWASP-ASVS", "Authentication requirements for session tokens.")]);
        assert_eq!(idx.status().source_count, 1);

        idx.append(
            "Remediación QA — run abc123".to_string(),
            "La etapa QA no pasó por token de sesión ausente en el header; Development lo agregó y QA aprobó.",
        );

        let status = idx.status();
        assert_eq!(status.source_count, 2, "the appended source must show up without rebuilding from disk");
        let hits = idx.retrieve("token de sesión ausente header", 3);
        assert!(!hits.is_empty(), "the just-appended learning must be retrievable in the same process");
        assert_eq!(hits[0].source_title, "Remediación QA — run abc123");
    }

    #[tokio::test]
    async fn record_learning_persists_to_disk_and_updates_the_live_cache() {
        let app_data = std::env::temp_dir().join(format!("knowledge-learning-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&app_data).unwrap();
        let cache: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<KnowledgeIndex>>>> =
            std::sync::Arc::new(tokio::sync::Mutex::new(None));

        record_learning(
            &app_data,
            &cache,
            "Remediación QA — run xyz",
            "El endpoint de exportación devolvía 500 por un campo nulo; Development lo validó y QA aprobó.".to_string(),
        )
        .await
        .expect("recording a learning with no prior cache should succeed");

        // Persisted to disk under learnings_dir(), so a cold restart's full
        // reindex (default_dirs() + learnings_dir()) would pick it up too.
        let files: Vec<_> = std::fs::read_dir(learnings_dir(&app_data)).unwrap().collect();
        assert_eq!(files.len(), 1, "expected exactly one learning file on disk");

        // Retrievable immediately from the live cache, no reindex needed.
        let guard = cache.lock().await;
        let idx = guard.as_ref().expect("cache must be populated after recording a learning");
        let hits = idx.retrieve("endpoint exportación campo nulo", 3);
        assert!(!hits.is_empty(), "the recorded learning must be retrievable from the live cache");
        drop(guard);

        std::fs::remove_dir_all(&app_data).ok();
    }
}
