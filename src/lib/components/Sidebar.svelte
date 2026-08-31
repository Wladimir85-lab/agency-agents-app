<script lang="ts">
  import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
  import Bot from "@lucide/svelte/icons/bot";
  import Wrench from "@lucide/svelte/icons/wrench";
  import Users from "@lucide/svelte/icons/users";
  import FolderGit2 from "@lucide/svelte/icons/folder-git-2";
  import Rocket from "@lucide/svelte/icons/rocket";
  import Activity from "@lucide/svelte/icons/activity";
  import PanelLeftClose from "@lucide/svelte/icons/panel-left-close";
  import PanelLeftOpen from "@lucide/svelte/icons/panel-left-open";
  import Layers3 from "@lucide/svelte/icons/layers-3";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";

  import { ui } from "$lib/stores/ui.svelte";
  import { corpus } from "$lib/stores/corpus.svelte";
  import { install } from "$lib/stores/install.svelte";
  import { i18n } from "$lib/stores/i18n.svelte";
  import { shortcut } from "$lib/util/platform";
  import type { SidebarSection } from "$lib/types";
  import { CREATION_CATALOG } from "$lib/data/intentosCapabilities";

  interface NavItem {
    id: SidebarSection;
    shortcut: string;
    icon: typeof Bot;
  }

  // Agency-first navigation. Agents is the home screen and now the UNIFIED
  // surface — it absorbed the former Library, so install state lives there as a
  // filter, not a separate section. Shortcut glyphs adapt per platform
  // (⌘ on macOS, Ctrl elsewhere) since the app ships on macOS/Linux/Windows.
  const nav: NavItem[] = [
    { id: "dashboard", shortcut: shortcut("0"), icon: LayoutDashboard },
    { id: "personas",  shortcut: shortcut("1"), icon: Bot },
    { id: "tools",     shortcut: shortcut("2"), icon: Wrench },
    { id: "teams",     shortcut: shortcut("3"), icon: Users },
    { id: "projects",  shortcut: shortcut("4"), icon: FolderGit2 },
    { id: "runbooks",  shortcut: shortcut("5"), icon: Rocket },
    { id: "activity",  shortcut: shortcut("6"), icon: Activity },
  ];

  function label(id: SidebarSection): string {
    if (id === "dashboard") return i18n.t("nav.dashboard");
    if (id === "personas") return i18n.t("nav.agents");
    if (id === "tools") return i18n.t("nav.tools");
    if (id === "teams") return i18n.t("nav.teams");
    if (id === "projects") return i18n.t("nav.projects");
    if (id === "runbooks") return i18n.t("nav.runbooks");
    return i18n.t("nav.activity");
  }

  function badge(id: SidebarSection): string | null {
    if (id === "personas") {
      // Installed agents with a newer version in the catalog — "updates
      // available". (Matches the "N updates" action on the Agents view. Other
      // non-current states — local edits, untracked, missing — are surfaced
      // per-agent when you drill in, not as a catch-all attention count.)
      const n = install.installed.filter((i) => i.state === "outdated").length;
      return n > 0 ? String(n) : null;
    }
    return null;
  }

  /** Footer: live corpus size — the app's own at-a-glance status. */
  const agentCount = $derived(corpus.agents.length);
  let openCatalogArea = $state<string | null>("frontend");
</script>

<aside
  class="sidebar"
  class:collapsed={ui.sidebarCollapsed}
  style="width: {ui.sidebarCollapsed ? 56 : ui.sidebarWidth}px"
  aria-label={i18n.t("nav.primary")}
>
  <div class="brand-row">
    <button
      type="button"
      class="sidebar-toggle"
      title={ui.sidebarCollapsed ? i18n.t("titlebar.showSidebar") : i18n.t("titlebar.hideSidebar")}
      aria-label={ui.sidebarCollapsed ? i18n.t("titlebar.showSidebar") : i18n.t("titlebar.hideSidebar")}
      aria-pressed={ui.sidebarCollapsed}
      onclick={() => ui.toggleSidebarCollapsed()}
    >
      {#if ui.sidebarCollapsed}
        <PanelLeftOpen size={16} />
      {:else}
        <PanelLeftClose size={16} />
      {/if}
    </button>
  </div>

  <nav>
    <section class="creation-catalog" aria-label="Catálogo de creación">
      {#if ui.sidebarCollapsed}
        <button class="catalog-rail" type="button" title="Abrir Catálogo de Creación" aria-label="Abrir Catálogo de Creación" onclick={() => ui.toggleSidebarCollapsed()}><Layers3 size={17}/></button>
      {:else}
        <div class="catalog-title"><Layers3 size={14}/><span>CATÁLOGO</span></div>
        <p class="catalog-subtitle">¿Qué quieres crear?</p>
        {#each CREATION_CATALOG as area (area.id)}
          {@const expanded = openCatalogArea === area.id}
          <div class="catalog-area">
            <button class="area-trigger" type="button" aria-expanded={expanded} onclick={() => openCatalogArea = expanded ? null : area.id}>
              <b>{area.number}</b><span>{area.name}</span><ChevronRight class={expanded ? "expanded" : undefined} size={13}/>
            </button>
            {#if expanded}
              <ul class="products">
                {#each area.products as product (product.id)}
                  <li><button class:active={ui.catalogProductId === product.id} type="button" title={product.description} onclick={() => ui.openCatalogProduct(product.id)}>{product.name}</button></li>
                {/each}
              </ul>
            {/if}
          </div>
        {/each}
      {/if}
    </section>
    {#if !ui.sidebarCollapsed}<div class="nav-divider"><span>ESPACIOS</span></div>{/if}
    <ul>
      {#each nav as item (item.id)}
        {@const isActive = ui.section === item.id}
        {@const b = badge(item.id)}
        <li>
          <button
            class="nav-item"
            class:active={isActive}
            aria-current={isActive ? "page" : undefined}
            onclick={() => ui.setSection(item.id)}
            title={`${label(item.id)} (${item.shortcut})`}
          >
            <span class="ico" aria-hidden="true"><item.icon size={16} /></span>
            <span class="label">{label(item.id)}</span>
            {#if b}<span class="badge" title={i18n.t("agentUpdates.badgeTitle", { count: Number(b) })}>{b}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
  </nav>

  <footer class="foot">
    <div class="status status-ready" title={i18n.t("nav.catalogStatus", { count: agentCount })}>
      <span class="dot" aria-hidden="true"></span>
      <span class="status-label">{i18n.t("nav.catalogStatus", { count: agentCount })}</span>
    </div>
  </footer>
</aside>

<style>
  .sidebar {
    /* width is set inline from ui.sidebarWidth (or 56px collapsed) so the
       resize handle in +page.svelte can drive it live. */
    flex: none;
    background: var(--color-surface-sunken);
    border-right: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    min-height: 0;
    position: relative;
    z-index: 5;
    transition: width var(--motion-duration-base, 180ms) var(--motion-ease-out, ease);
  }
  @media (prefers-reduced-motion: reduce) {
    .sidebar { transition: none; }
  }

  /* Brand row — identity and sidebar control form one menu-level unit. */
  .brand-row {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    min-height: 48px;
    padding: var(--space-2);
  }
  .sidebar-toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    flex: none;
    border-radius: var(--radius-md);
    color: var(--color-text-muted);
    transition:
      color var(--motion-duration-fast) var(--motion-ease-out),
      background-color var(--motion-duration-fast) var(--motion-ease-out);
  }
  .sidebar-toggle:hover {
    color: var(--color-text-primary);
    background: var(--color-surface-raised);
  }

  nav { flex: 1; padding: var(--space-2); overflow-y: auto; }
  ul { display: flex; flex-direction: column; gap: 1px; }
  .creation-catalog{margin-bottom:8px}.catalog-rail{width:100%;height:34px;display:grid;place-items:center;border-radius:var(--radius-md);color:var(--color-brand)}.catalog-rail:hover{background:var(--color-brand-subtle)}.catalog-title{display:flex;align-items:center;gap:7px;padding:5px 9px 1px;color:var(--color-brand);font-size:10px;font-weight:800;letter-spacing:.12em}.catalog-subtitle{padding:0 9px 7px;color:var(--color-text-muted);font-size:10px}.catalog-area{margin-bottom:1px}.area-trigger{display:grid;grid-template-columns:22px 1fr 14px;align-items:center;gap:5px;width:100%;min-height:30px;padding:5px 8px;border-radius:var(--radius-md);color:var(--color-text-secondary);text-align:left}.area-trigger:hover{background:var(--color-surface-raised);color:var(--color-text-primary)}.area-trigger b{font:9px var(--font-mono);color:var(--color-brand)}.area-trigger span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:11px;font-weight:600}.area-trigger :global(svg){transition:transform 140ms ease}.area-trigger :global(svg.expanded){transform:rotate(90deg)}.products{margin:2px 0 6px 28px!important;padding-left:7px;border-left:1px solid var(--color-border)}.products button{width:100%;padding:5px 6px;border-radius:var(--radius-sm);color:var(--color-text-muted);font-size:10px;line-height:1.25;text-align:left}.products button:hover,.products button.active{background:var(--color-brand-subtle);color:var(--color-cask-on-subtle)}.nav-divider{display:flex;align-items:center;gap:6px;margin:9px 8px 5px;color:var(--color-text-muted);font-size:8px;letter-spacing:.14em}.nav-divider:after{content:"";height:1px;flex:1;background:var(--color-border)}

  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-md);
    color: var(--color-text-secondary);
    font-size: var(--text-body);
    font-weight: var(--fw-medium);
    line-height: 1;
    text-align: left;
    position: relative;
    min-height: 34px;
    transition:
      color var(--motion-duration-fast) var(--motion-ease-out),
      background-color var(--motion-duration-fast) var(--motion-ease-out),
      transform var(--motion-duration-fast) var(--motion-ease-out);
  }
  .nav-item:hover {
    background: var(--color-brand-subtle);
    color: var(--color-cask-on-subtle);
    transform: translateX(2px);
  }
  .nav-item.active {
    background: var(--color-surface-raised);
    color: var(--color-text-primary);
    font-weight: var(--fw-semibold);
  }
  .nav-item.active:hover {
    background: var(--color-brand-subtle);
    color: var(--color-cask-on-subtle);
  }
  .nav-item .label { flex: 1; }
  .ico { display: inline-flex; transition: transform var(--motion-duration-base) var(--motion-ease-spring); }
  .nav-item:hover .ico { transform: scale(1.08); }
  .badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 16px;
    min-width: 16px;
    padding: 0 var(--space-1);
    border-radius: var(--radius-full);
    background: var(--color-brand);
    color: var(--color-text-inverse);
    font-size: var(--text-caption);
    font-weight: var(--fw-semibold);
  }

  .foot {
    border-top: 1px solid var(--color-border);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-caption);
    color: var(--color-text-muted);
    padding: 2px var(--space-1);
    margin: -2px calc(-1 * var(--space-1));
    border-radius: var(--radius-sm);
    background: transparent;
    text-align: left;
    white-space: nowrap;
  }
  .dot {
    width: 8px; height: 8px; border-radius: var(--radius-full);
    background: var(--color-text-muted);
  }
  .status-ready .dot { background: var(--color-success); }

  /* ── Collapsed sidebar (icon-rail mode) ── */
  .sidebar.collapsed { width: 56px; overflow: visible; }
  .sidebar.collapsed .brand-row { justify-content: center; padding: 4px; }
  .sidebar.collapsed .sidebar-toggle { width: 24px; height: 28px; }
  .sidebar.collapsed nav { overflow: visible; }
  .sidebar.collapsed .nav-item {
    justify-content: center;
    padding-left: 0;
    padding-right: 0;
    position: relative;
    border-radius: var(--radius-lg);
  }
  .sidebar.collapsed .nav-item:hover { transform: translateX(0); }
  .sidebar.collapsed .nav-item.active {
    background: var(--color-brand-subtle);
    color: var(--color-cask-on-subtle);
  }
  .sidebar.collapsed .nav-item.active::before {
    content: "";
    position: absolute;
    left: -8px;
    width: 2px;
    height: 17px;
    border-radius: 0 var(--radius-full) var(--radius-full) 0;
    background: var(--color-brand);
  }
  .sidebar.collapsed .nav-item .label {
    display: block;
    position: absolute;
    left: calc(100% + 10px);
    top: 50%;
    z-index: 10;
    width: max-content;
    max-width: 180px;
    padding: 7px 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-overlay);
    color: var(--color-text-primary);
    box-shadow: var(--shadow-sm);
    backdrop-filter: blur(18px);
    opacity: 0;
    pointer-events: none;
    transform: translate(-5px, -50%) scale(0.96);
    transform-origin: left center;
    transition:
      opacity var(--motion-duration-fast) var(--motion-ease-out),
      transform var(--motion-duration-base) var(--motion-ease-spring);
  }
  .sidebar.collapsed .nav-item:hover .label,
  .sidebar.collapsed .nav-item:focus-visible .label {
    opacity: 1;
    transform: translate(0, -50%) scale(1);
  }
  .sidebar.collapsed .nav-item .badge {
    position: absolute;
    top: 2px;
    right: 4px;
    min-width: 14px;
    height: 14px;
    padding: 0 4px;
    font-size: 9px;
    line-height: 1;
  }
  .sidebar.collapsed .foot {
    align-items: center;
    padding-left: var(--space-2);
    padding-right: var(--space-2);
  }
  .sidebar.collapsed .status { justify-content: center; margin: 0; padding: 4px; }
  .sidebar.collapsed .status-label { display: none; }
</style>
