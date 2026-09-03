/** Preview público — publishes the Showroom's local URL through a
 *  Cloudflare quick tunnel. Same Channel<T> + invoke shape as
 *  preview.svelte.ts, one active tunnel at a time (mirrors the
 *  single-slot backend state in deploy.rs). Never takes a URL as input:
 *  the backend only ever tunnels whatever the Showroom is already
 *  serving — see public_preview_start's own doc comment. */
import { Channel, invoke } from "@tauri-apps/api/core";
import type { PublicPreviewEvent, PublicPreviewStatus } from "$lib/types";

class PublicPreviewStore {
  running = $state(false);
  localUrl: string | null = $state(null);
  url: string | null = $state(null);
  starting = $state(false);
  stopping = $state(false);
  error: string | null = $state(null);
  /** True while IntentOS is downloading + verifying cloudflared itself —
   *  only happens once, the first time this is ever used. */
  preparing = $state(false);

  async start(): Promise<void> {
    if (this.starting || this.running) return;
    this.starting = true;
    this.preparing = false;
    this.error = null;
    const onEvent = new Channel<PublicPreviewEvent>();
    onEvent.onmessage = (event) => this.accept(event);
    try {
      const status = await invoke<PublicPreviewStatus>("public_preview_start", { onEvent });
      this.running = status.running;
      this.localUrl = status.localUrl;
      this.url = status.url;
    } catch (e) {
      this.error = String(e);
      this.running = false;
    } finally {
      this.starting = false;
      this.preparing = false;
    }
  }

  async stop(): Promise<void> {
    if (this.stopping || (!this.running && !this.starting)) return;
    this.stopping = true;
    try {
      const status = await invoke<PublicPreviewStatus>("public_preview_stop");
      this.running = status.running;
      this.localUrl = status.localUrl;
      this.url = status.url;
    } finally {
      this.stopping = false;
    }
  }

  private accept(event: PublicPreviewEvent): void {
    if (event.kind === "preparing") this.preparing = true;
    else if (event.kind === "starting") this.preparing = false;
    else if (event.kind === "ready") { this.url = event.url; this.running = true; }
    else if (event.kind === "stopped") { this.running = false; this.url = null; }
    else if (event.kind === "failed") { this.error = event.reason; this.running = false; }
  }
}

export const publicPreview = new PublicPreviewStore();
