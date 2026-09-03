/** Showroom — live preview of the isolated workspace a run is building.
 *  Same Channel<T> + invoke shape as runs.svelte.ts, one active preview at
 *  a time (mirrors the single-slot backend state in preview.rs). */
import { Channel, invoke } from "@tauri-apps/api/core";
import type { PreviewEvent, PreviewStatus } from "$lib/types";

class PreviewStore {
  running = $state(false);
  workspacePath: string | null = $state(null);
  url: string | null = $state(null);
  starting = $state(false);
  stopping = $state(false);
  error: string | null = $state(null);
  logs: string[] = $state([]);

  async start(workspacePath: string): Promise<void> {
    if (this.starting) return;
    if (this.running && this.workspacePath === workspacePath) return;
    this.starting = true;
    this.error = null;
    this.logs = [];
    const onEvent = new Channel<PreviewEvent>();
    onEvent.onmessage = (event) => this.accept(event);
    try {
      const status = await invoke<PreviewStatus>("preview_start", { workspacePath, onEvent });
      this.running = status.running;
      this.workspacePath = status.workspacePath;
      this.url = status.url;
    } catch (e) {
      this.error = String(e);
      this.running = false;
    } finally {
      this.starting = false;
    }
  }

  async stop(): Promise<void> {
    if (this.stopping || (!this.running && !this.starting)) return;
    this.stopping = true;
    try {
      const status = await invoke<PreviewStatus>("preview_stop");
      this.running = status.running;
      this.workspacePath = status.workspacePath;
      this.url = status.url;
    } finally {
      this.stopping = false;
    }
  }

  private accept(event: PreviewEvent): void {
    if (event.kind === "ready") { this.url = event.url; this.running = true; }
    else if (event.kind === "log") this.logs = [...this.logs.slice(-199), event.text];
    else if (event.kind === "stopped") { this.running = false; this.url = null; }
    else if (event.kind === "failed") { this.error = event.reason; this.running = false; }
  }
}

export const preview = new PreviewStore();
