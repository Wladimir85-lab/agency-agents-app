/** IntentOS runtime projection. The Rust backend remains the source of truth. */
import { Channel, invoke } from "@tauri-apps/api/core";
import type { RunEvent, RunSummary, RuntimeProvider, StartRunRequest } from "$lib/types";

export interface TimestampedRunEvent {
  id: number;
  at: string;
  event: RunEvent;
}

class RunsStore {
  providers: RuntimeProvider[] = $state([]);
  list: RunSummary[] = $state([]);
  current: RunSummary | null = $state(null);
  events: TimestampedRunEvent[] = $state([]);
  loading = $state(false);
  starting = $state(false);
  cancelling = $state(false);
  error: string | null = $state(null);
  private sequence = 0;

  async load(projectPath?: string): Promise<void> {
    this.loading = true;
    this.error = null;
    try {
      const [providers, runs] = await Promise.all([
        invoke<RuntimeProvider[]>("runtime_providers"),
        invoke<RunSummary[]>("runtime_list", { projectPath: projectPath ?? null }),
      ]);
      this.providers = providers;
      this.list = runs;
      this.current = runs[0] ?? null;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loading = false;
    }
  }

  async start(request: StartRunRequest): Promise<RunSummary> {
    this.starting = true;
    this.error = null;
    this.events = [];
    this.sequence = 0;
    const onEvent = new Channel<RunEvent>();
    onEvent.onmessage = (event) => this.accept(event);
    try {
      const run = await invoke<RunSummary>("runtime_start", { request, onEvent });
      this.current = run;
      this.upsert(run);
      return run;
    } catch (e) {
      this.error = String(e);
      throw e;
    } finally {
      this.starting = false;
    }
  }

  async refreshCurrent(): Promise<void> {
    if (!this.current) return;
    const run = await invoke<RunSummary>("runtime_get", { runId: this.current.id });
    this.current = run;
    this.upsert(run);
  }

  async cancel(): Promise<void> {
    if (!this.current || this.cancelling) return;
    this.cancelling = true;
    try {
      await invoke("runtime_cancel", { runId: this.current.id });
      await this.refreshCurrent();
    } finally {
      this.cancelling = false;
    }
  }

  /** Clear the UI projection without deleting persisted run evidence. */
  clearCurrent(): void {
    this.current = null;
    this.events = [];
    this.error = null;
    this.sequence = 0;
  }

  private upsert(run: RunSummary): void {
    this.list = [run, ...this.list.filter((item) => item.id !== run.id)];
  }

  private accept(event: RunEvent): void {
    this.events = [...this.events, { id: ++this.sequence, at: new Date().toISOString(), event }];
    if (event.kind === "runUpdated") {
      this.current = event.run;
      this.upsert(event.run);
    }
  }
}

export const runs = new RunsStore();
