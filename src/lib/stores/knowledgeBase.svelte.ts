/**
 * Knowledge library store — the local folders of reference PDFs (standards,
 * curricula, technical books) that ground IntentOS's architecture/
 * development stage prompts. See `src-tauri/src/knowledge.rs` for the
 * indexing/retrieval side; this store only owns which folders are
 * registered (persisted client-side, same as `projects.svelte.ts`) and the
 * last-known index status returned by the backend.
 *
 * Fully local: `knowledge_index` never leaves the machine — it just reads
 * PDFs from disk and BM25-scores them. No embeddings model, no network.
 *
 * Singleton: import `knowledgeBase` everywhere.
 */

import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { i18n } from "$lib/stores/i18n.svelte";
import type { KnowledgeStatus } from "$lib/types";

const STORAGE_KEY = "agency-agents:knowledge-dirs:v1";

class KnowledgeBaseStore {
  dirs: string[] = $state([]);
  status: KnowledgeStatus | null = $state(null);
  indexing = $state(false);
  error: string | null = $state(null);
  private hydrated = false;

  private hydrate(): void {
    if (this.hydrated || typeof window === "undefined") return;
    this.hydrated = true;
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const arr = JSON.parse(raw) as unknown;
        if (Array.isArray(arr)) this.dirs = arr.filter((x): x is string => typeof x === "string");
      }
    } catch {
      /* ignore corrupt entry */
    }
  }

  private persist(): void {
    if (typeof window === "undefined") return;
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.dirs));
    } catch {
      /* private mode / quota */
    }
  }

  /** Load persisted folders + current backend status. Does NOT reindex —
      that only happens on `addFolder`/`reindex`/`removeFolder`, all
      explicit user actions, so opening Settings never triggers PDF
      extraction on its own. */
  async load(): Promise<void> {
    this.hydrate();
    try {
      this.status = await invoke<KnowledgeStatus>("knowledge_status");
    } catch (e) {
      this.error = String(e);
    }
  }

  async addFolder(): Promise<void> {
    const picked = await openDialog({ directory: true, title: i18n.t("knowledge.chooseFolderTitle") });
    const path = typeof picked === "string" ? picked : Array.isArray(picked) ? (picked[0] ?? null) : null;
    if (!path || this.dirs.includes(path)) return;
    this.dirs = [...this.dirs, path];
    this.persist();
    await this.reindex();
  }

  async removeFolder(path: string): Promise<void> {
    this.dirs = this.dirs.filter((d) => d !== path);
    this.persist();
    await this.reindex();
  }

  /** Re-scans every registered folder from scratch and swaps the backend's
      index. `dirs.length === 0` still calls through — that's how a user
      clears every folder and ends up with an empty (inert) library
      instead of a stale one. */
  async reindex(): Promise<void> {
    this.indexing = true;
    this.error = null;
    try {
      this.status = await invoke<KnowledgeStatus>("knowledge_index", { dirs: this.dirs });
    } catch (e) {
      this.error = String(e);
    } finally {
      this.indexing = false;
    }
  }
}

export const knowledgeBase = new KnowledgeBaseStore();
