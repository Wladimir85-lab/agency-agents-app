<script lang="ts">
  /**
   * SettingsSectionKnowledge.svelte — the "Biblioteca" section.
   *
   * Lets the user register one or more folders of reference PDFs
   * (standards, curricula, technical books) that ground IntentOS's
   * architecture/development stage prompts with real citations. Mirrors
   * Projects.svelte's folder-registration UX (native picker, persisted
   * list, remove-per-row) — see `knowledgeBase.svelte.ts` for the store
   * and `src-tauri/src/knowledge.rs` for the local BM25 index this feeds.
   */
  import { onMount } from "svelte";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Folder from "@lucide/svelte/icons/folder";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import BookOpen from "@lucide/svelte/icons/book-open";

  import { knowledgeBase } from "$lib/stores/knowledgeBase.svelte";
  import { i18n } from "$lib/stores/i18n.svelte";

  onMount(() => {
    void knowledgeBase.load();
  });

  function shortDate(iso: string | null | undefined): string {
    if (!iso) return i18n.t("knowledge.neverIndexed");
    const d = new Date(iso);
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
  }
</script>

<div class="section">
  <h2>{i18n.t("knowledge.title")}</h2>
  <p class="subtitle">{i18n.t("knowledge.subtitle")}</p>

  <dl class="meta">
    <div class="row"><dt>{i18n.t("knowledge.sourcesTitle")}</dt><dd>{knowledgeBase.status?.sourceCount ?? 0}</dd></div>
    <div class="row"><dt>{i18n.t("knowledge.fragments")}</dt><dd>{knowledgeBase.status?.chunkCount ?? 0}</dd></div>
    <div class="row"><dt>{i18n.t("knowledge.lastIndexed")}</dt><dd>{shortDate(knowledgeBase.status?.indexedAt)}</dd></div>
  </dl>

  <h3>{i18n.t("knowledge.folders")}</h3>
  {#if knowledgeBase.dirs.length === 0}
    <p class="hint">{i18n.t("knowledge.emptyBody")}</p>
  {:else}
    <ul class="dirs">
      {#each knowledgeBase.dirs as dir (dir)}
        <li class="dir">
          <Folder size={15} />
          <span class="dir-path" title={dir}>{dir}</span>
          <button
            class="danger-ic"
            title={i18n.t("knowledge.removeFolder")}
            aria-label={i18n.t("knowledge.removeFolder")}
            disabled={knowledgeBase.indexing}
            onclick={() => knowledgeBase.removeFolder(dir)}
          >
            <Trash2 size={14} />
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <div class="row-actions">
    <button class="ghost" disabled={knowledgeBase.indexing} onclick={() => knowledgeBase.addFolder()}>
      <FolderPlus size={14} /><span>{i18n.t("knowledge.addFolder")}</span>
    </button>
    <button class="ghost" disabled={knowledgeBase.indexing || knowledgeBase.dirs.length === 0} onclick={() => knowledgeBase.reindex()}>
      <RefreshCw size={14} /><span>{knowledgeBase.indexing ? i18n.t("knowledge.indexing") : i18n.t("knowledge.reindex")}</span>
    </button>
  </div>

  {#if knowledgeBase.error}<p class="err">{knowledgeBase.error}</p>{/if}

  {#if knowledgeBase.status && knowledgeBase.status.sources.length > 0}
    <h3>{i18n.t("knowledge.indexedSources")}</h3>
    <ul class="sources">
      {#each knowledgeBase.status.sources as title (title)}
        <li><BookOpen size={13} /><span>{title}</span></li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .section { display: flex; flex-direction: column; gap: var(--space-4); max-width: 580px; }
  h2 { font-size: var(--text-h1); font-weight: var(--fw-semibold); color: var(--color-text-primary); margin-bottom: var(--space-1); }
  h3 { font-size: var(--text-h2); font-weight: var(--fw-semibold); color: var(--color-text-primary); margin-top: var(--space-2); }
  .subtitle { font-size: var(--text-body-sm); color: var(--color-text-secondary); margin-top: calc(-1 * var(--space-3)); }
  .meta {
    display: flex; flex-direction: column; gap: var(--space-1);
    padding: var(--space-3) var(--space-4); background: var(--color-surface-sunken);
    border: 1px solid var(--color-border); border-radius: var(--radius-md);
  }
  .row { display: grid; grid-template-columns: 140px 1fr; gap: var(--space-3); padding: 4px 0; align-items: baseline; }
  dt { font-size: var(--text-body-sm); color: var(--color-text-muted); font-weight: var(--fw-medium); }
  dd { font-size: var(--text-body); color: var(--color-text-primary); }
  .hint { font-size: var(--text-body-sm); color: var(--color-text-muted); line-height: var(--lh-normal); }
  .dirs { display: flex; flex-direction: column; gap: 6px; list-style: none; margin: 0; padding: 0; }
  .dir {
    display: flex; align-items: center; gap: var(--space-2);
    padding: var(--space-2) var(--space-3); border: 1px solid var(--color-border);
    border-radius: var(--radius-md); background: var(--color-surface-sunken); color: var(--color-text-secondary);
  }
  .dir-path { flex: 1; min-width: 0; font-family: var(--font-mono); font-size: var(--text-mono); color: var(--color-text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .danger-ic { flex: none; padding: 4px; border-radius: var(--radius-sm); color: var(--color-text-muted); cursor: pointer; }
  .danger-ic:hover:not(:disabled) { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 10%, transparent); }
  .danger-ic:disabled { opacity: 0.5; cursor: default; }
  .row-actions { display: flex; gap: var(--space-2); }
  .ghost {
    display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 var(--space-3);
    border: 1px solid var(--color-border); border-radius: var(--radius-md);
    background: transparent; color: var(--color-text-secondary); font-size: var(--text-body-sm); cursor: pointer;
  }
  .ghost:hover:not(:disabled) { color: var(--color-text-primary); background: var(--color-surface-sunken); }
  .ghost:disabled { opacity: 0.5; cursor: default; }
  .err { font-size: var(--text-body-sm); color: var(--color-danger); }
  .sources { display: flex; flex-direction: column; gap: 4px; list-style: none; margin: 0; padding: 0; }
  .sources li { display: flex; align-items: center; gap: 8px; font-size: var(--text-body-sm); color: var(--color-text-secondary); }
</style>
