//! Local knowledge library — indexes reference PDFs (standards, curricula,
//! technical books the user drops into one or more folders) into an
//! in-memory, keyword-scored (BM25) index so IntentOS's stage prompts can
//! ground architecture/development decisions in real citations instead of
//! relying solely on the model's own recollection.
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
#[derive(Debug, Default, Serialize, Deserialize)]
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

fn collect_pdfs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_pdfs(&path, out);
        } else if path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
}

pub fn scan_pdfs(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in dirs {
        collect_pdfs(dir, &mut out);
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

/// Build the index synchronously — PDF text extraction is CPU/IO-bound, so
/// callers run this inside `spawn_blocking`. A PDF that fails to extract
/// (corrupt, scanned-image-only) is skipped and logged rather than failing
/// the whole reindex; only "zero readable pages across every source" is a
/// hard error, since that means the configured folders are wrong.
pub fn build_index(dirs: &[PathBuf]) -> Result<KnowledgeIndex, AppError> {
    let paths = scan_pdfs(dirs);
    let mut chunks = Vec::new();
    let mut skipped = 0usize;
    // `pdf_extract` panics on some malformed/corrupt PDFs instead of
    // returning `Err` (observed on a real book in the wild: "missing
    // object reference"). One bad file must not abort indexing every
    // other source, so extraction runs behind `catch_unwind` and a panic
    // is treated exactly like an extraction error — skip and keep going.
    // (Not swapping the global panic hook here: `build_index` runs inside
    // `spawn_blocking` alongside other concurrent async work, and a
    // process-wide hook is the wrong tool to silence one thread's noise.)
    for path in &paths {
        let extracted = std::panic::catch_unwind(|| pdf_extract::extract_text(path));
        let text = match extracted {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => {
                tracing::warn!("knowledge: skipping unreadable PDF {}: {e}", path.display());
                skipped += 1;
                continue;
            }
            Err(_) => {
                tracing::warn!(
                    "knowledge: skipping PDF that panicked during extraction: {}",
                    path.display()
                );
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
                "could not extract text from any of {} PDF(s) ({skipped} skipped)",
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
    fn scan_pdfs_recurses_and_dedupes() {
        let dir = std::env::temp_dir().join(format!("knowledge-scan-test-{}", uuid::Uuid::new_v4()));
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.join("a.pdf"), b"x").unwrap();
        std::fs::write(nested.join("b.PDF"), b"x").unwrap();
        std::fs::write(dir.join("ignore.txt"), b"x").unwrap();

        let found = scan_pdfs(&[dir.clone(), dir.clone()]);
        assert_eq!(found.len(), 2, "duplicate input dirs must not duplicate results");
        assert!(found.iter().all(|p| p.extension().unwrap().eq_ignore_ascii_case("pdf")));

        std::fs::remove_dir_all(&dir).ok();
    }
}
