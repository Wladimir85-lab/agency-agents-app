<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import PlayIcon from "@lucide/svelte/icons/play";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import PaperclipIcon from "@lucide/svelte/icons/paperclip";
  import XIcon from "@lucide/svelte/icons/x";
  import SquareIcon from "@lucide/svelte/icons/square";
  import InstallModal from "./InstallModal.svelte";
  import Button from "./Button.svelte";
  import { corpus } from "$lib/stores/corpus.svelte";
  import { runbooks } from "$lib/stores/runbooks.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { runs } from "$lib/stores/runs.svelte";
  import { toast } from "$lib/stores/toast.svelte";
  import { i18n } from "$lib/stores/i18n.svelte";
  import { INTENTOS_CAPABILITIES, routeIntent } from "$lib/data/intentosCapabilities";
  import type { Agent, Runbook, RunEvent } from "$lib/types";

  const LEGACY_DRAFT_KEY = "intentos.productionBrief.v1";
  const DEFAULT_SIGNATURE = "Dirección creativa inspirada en la nueva ola de estudios digitales de Japón y Corea: tipografía protagonista y cinética, composición editorial con asimetría intencional, maximalismo controlado, capas y texturas, microinteracciones con propósito y narrativa visual mediante scroll. Usar 3D, WebGL o medios mixtos solo cuando refuercen la identidad del producto. El resultado debe sentirse vivo, memorable y propio, nunca como una plantilla SaaS genérica. Mantener siempre jerarquía clara, accesibilidad, respuesta móvil, rendimiento y facilidad de uso.";
  onMount(() => {
    // Security baseline v0.1: production briefs can contain client data and
    // must never live in plaintext WebView storage. Remove the legacy draft
    // once and keep the current brief in memory until an encrypted workspace
    // store exists.
    try { localStorage.removeItem(LEGACY_DRAFT_KEY); } catch { /* best effort */ }
    corpus.ensureLoaded(); runbooks.load(); projects.refresh(); runs.load();
  });
  const bySlug = $derived(new Map(corpus.agents.map((a) => [a.slug, a])));
  const rosterSlugs = (rb: Runbook) => rb.roster.flatMap((g) => g.agents);
  const resolvedSlugs = (rb: Runbook) => rosterSlugs(rb).filter((s) => bySlug.has(s));
  const resolve = (slugs: string[]): { slug: string; agent: Agent | undefined }[] => slugs.map((slug) => ({ slug, agent: bySlug.get(slug) }));
  const title = (rb: Runbook) => i18n.optional(`runbooks.item.${rb.slug}.title`, rb.title);
  const summary = (rb: Runbook) => i18n.optional(`runbooks.item.${rb.slug}.summary`, rb.summary);
  let intent = $state("");
  let audience = $state("");
  let businessGoal = $state("");
  let requiredFeatures = $state("");
  let visualDirection = $state("");
  let references = $state("");
  let availableAssets = $state("");
  let signature = $state(DEFAULT_SIGNATURE);
  let attachments = $state<string[]>([]);
  let constraints = $state("");
  let acceptance = $state("");
  let briefOpen = $state(false);
  let projectPath = $state("");
  let selectedSlug = $state("");
  let selectedCapabilityId = $state("auto");
  let openSlug = $state<string | null>(null);
  let deployRb = $state<Runbook | null>(null);
  let validation = $state("");
  const selected = $derived(runbooks.list.find((r) => r.slug === selectedSlug) ?? null);
  const routed = $derived(routeIntent(`${intent}\n${requiredFeatures}\n${businessGoal}`));
  const activeCapability = $derived(INTENTOS_CAPABILITIES.find((item) => item.id === selectedCapabilityId) ?? routed.capability);
  const capabilityAgents = $derived(activeCapability.agents.map((slug) => bySlug.get(slug)).filter((agent): agent is Agent => Boolean(agent)));
  const teamReady = $derived(capabilityAgents.length === 5);
  const provider = $derived(runs.providers.find((p) => p.available) ?? null);
  const canStart = $derived(Boolean(intent.trim() && projectPath && selected && provider && teamReady && !runs.starting && runs.current?.status !== "running"));
  const output = $derived(runs.events.filter((item) => item.event.kind === "output"));
  const briefCount = $derived([audience, businessGoal, requiredFeatures, visualDirection, signature, references, availableAssets || attachments.length ? "assets" : "", constraints, acceptance].filter((value) => value.trim()).length);

  $effect(() => { if (!selectedSlug && runbooks.list.length) selectedSlug = runbooks.list[0].slug; });
  $effect(() => { if (!projectPath && projects.list.length) projectPath = projects.list[0].path; });
  $effect(() => {
    if (!runs.current) return;
    if (!projectPath || projectPath === projects.list[0]?.path) projectPath = runs.current.projectPath;
    if (runbooks.list.some((rb) => rb.slug === runs.current?.runbookId)) selectedSlug = runs.current.runbookId;
  });

  function productionBrief(): string {
    const sections = [
      ["INTENCIÓN DEL PRODUCTO", intent],
      ["CAPACIDAD INTENTOS ACTIVADA", `${activeCapability.label}\n${activeCapability.description}`],
      ["EQUIPO ESPECIALISTA", activeCapability.agents.map((slug, index) => `${index + 1}. ${activeCapability.stageLabels[index]}: ${bySlug.get(slug)?.name ?? slug}`).join("\n")],
      ["USUARIO OBJETIVO", audience],
      ["OBJETIVO COMERCIAL", businessGoal],
      ["FUNCIONES OBLIGATORIAS", requiredFeatures],
      ["DIRECCIÓN VISUAL", visualDirection],
      ["SELLO PROPIO INTENTOS", signature],
      ["REFERENCIAS VISUALES", references],
      ["DATOS Y ACTIVOS DISPONIBLES", availableAssets],
      ["ARCHIVOS ADJUNTOS AUTORIZADOS", attachments.map((path) => `- ${path}`).join("\n")],
      ["RESTRICCIONES Y FUERA DE ALCANCE", constraints],
      ["CRITERIOS DE APROBACIÓN", acceptance],
    ].filter(([, value]) => value.trim());
    return sections.map(([heading, value]) => `${heading}:\n${value.trim()}`).join("\n\n");
  }

  async function addAttachments() {
    try {
      const picked = await openDialog({ multiple: true, directory: false, title: "Adjuntar activos al brief de producción", filters: [{ name: "Activos de producción", extensions: ["png", "jpg", "jpeg", "webp", "svg", "pdf", "docx", "txt", "md", "csv", "xlsx", "json"] }] });
      const paths = typeof picked === "string" ? [picked] : (picked ?? []);
      attachments = [...new Set([...attachments, ...paths])];
    } catch (e) { toast.error("No se pudieron adjuntar los archivos", String(e)); }
  }

  function fileName(path: string): string { return path.split(/[\\/]/).pop() ?? path; }
  function removeAttachment(path: string) { attachments = attachments.filter((item) => item !== path); }

  function newIntent() {
    if (runs.current?.status === "running" || runs.current?.status === "queued") return;
    intent = "";
    audience = "";
    businessGoal = "";
    requiredFeatures = "";
    visualDirection = "";
    signature = DEFAULT_SIGNATURE;
    references = "";
    availableAssets = "";
    attachments = [];
    constraints = "";
    acceptance = "";
    selectedCapabilityId = "auto";
    validation = "";
    briefOpen = false;
    runs.clearCurrent();
    toast.success("Nueva intención preparada");
  }

  async function start() {
    validation = "";
    if (!intent.trim()) validation = "Describe la intención del producto.";
    else if (!projectPath) validation = "Selecciona un proyecto.";
    else if (!selected) validation = "Selecciona un pipeline.";
    else if (!provider) validation = "No hay un runtime ejecutable disponible.";
    else if (!teamReady) validation = "El catálogo activo no contiene los cinco especialistas requeridos para esta capacidad.";
    if (validation || !selected || !provider) return;
    try { await runs.start({ intent: productionBrief(), projectPath, runbookId: selected.slug, capabilityId: activeCapability.id, stageLabels: [...activeCapability.stageLabels], agentSlugs: [...activeCapability.agents], providerId: provider.id }); }
    catch (e) { toast.error("No se pudo iniciar IntentOS", String(e)); }
  }

  function activationPrompt(rb: Runbook): string {
    const roster = rb.roster.map((g) => `- ${g.group} (${g.activation}): ${g.agents.map((s) => bySlug.get(s)?.name ?? s).join(", ")}`).join("\n");
    return `Activa el pipeline "${title(rb)}" en modo ${rb.mode}.\n${summary(rb)}\n\nEquipo:\n${roster}\n\nVerifica cada etapa con evidencia antes de avanzar.`;
  }
  async function copyPrompt(rb: Runbook) { try { await navigator.clipboard.writeText(activationPrompt(rb)); toast.success("Prompt copiado"); } catch (e) { toast.error("No se pudo copiar", String(e)); } }
  function eventText(event: RunEvent): string {
    if (event.kind === "output") return event.text;
    if (event.kind === "gatePassed") return `Gate aprobado: ${event.stageId}`;
    if (event.kind === "gateFailed") return `Gate fallido: ${event.stageId} — ${event.reason}`;
    return `Estado: ${event.run.status}`;
  }
</script>

<section class="workspace">
  <header class="head"><div><h1>IntentOS Production</h1><p>Escribe una intención y ejecuta una fábrica de agentes con gates verificables.</p></div><div class="head-actions"><Button variant="secondary" onclick={newIntent} disabled={runs.current?.status === "running" || runs.current?.status === "queued"} ariaLabel="Crear nueva intención"><PlusIcon size={14}/> Nueva intención</Button><span class:ok={provider} class="provider">{provider ? `${provider.label} · ${provider.version ?? "disponible"}` : "Runtime no disponible"}</span></div></header>
  <div class="grid">
    <div class="composer">
      <div class="card">
        <label for="intent">¿Qué producto quieres crear?</label>
        <textarea id="intent" bind:value={intent} rows="7" placeholder="Describe el problema, usuario, resultado esperado y restricciones…" aria-describedby="intent-help validation"></textarea>
        <p id="intent-help" class="hint">IntentOS coordinará Project Manager → UX/Arquitectura → Desarrollo ↔ QA → Reality Check.</p>
        <section class="router" aria-labelledby="router-title">
          <div class="router-head"><div><strong id="router-title">Router de intención</strong><small>{selectedCapabilityId === "auto" ? (routed.confidence ? `Recomendación automática · ${Math.round(routed.confidence * 100)}%` : "Describe el encargo para clasificarlo") : "Selección manual"}</small></div><span>{activeCapability.shortLabel}</span></div>
          <label for="capability">Capacidad de la agencia<select id="capability" bind:value={selectedCapabilityId}><option value="auto">Automática — IntentOS decide</option>{#each INTENTOS_CAPABILITIES as capability (capability.id)}<option value={capability.id}>{capability.label}</option>{/each}</select></label>
          <p>{activeCapability.description}</p>
          <ol class="dynamic-team">{#each activeCapability.agents as slug, index (slug)}<li class:missing={!bySlug.has(slug)}><span>{index + 1}</span><div><strong>{activeCapability.stageLabels[index]}</strong><small>{bySlug.get(slug)?.name ?? slug}</small></div></li>{/each}</ol>
          {#if selectedCapabilityId === "auto" && routed.matched.length}<small class="signals">Señales detectadas: {routed.matched.join(", ")}</small>{/if}
        </section>
        <button class="brief-toggle" type="button" onclick={() => briefOpen = !briefOpen} aria-expanded={briefOpen} aria-controls="production-brief"><span><strong>Brief de producción</strong><small>{briefCount}/9 campos complementarios</small></span><svg class:rotated={briefOpen} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg></button>
        {#if briefOpen}
          <div id="production-brief" class="brief">
            <label for="audience">Usuario objetivo<textarea id="audience" bind:value={audience} rows="2" placeholder="Quién usará o comprará el producto"></textarea></label>
            <label for="business-goal">Objetivo comercial<textarea id="business-goal" bind:value={businessGoal} rows="2" placeholder="Qué resultado debe producir para el negocio"></textarea></label>
            <label for="required-features">Funciones obligatorias<textarea id="required-features" bind:value={requiredFeatures} rows="3" placeholder="Flujos y capacidades que no pueden faltar"></textarea></label>
            <label for="visual-direction">Dirección visual<textarea id="visual-direction" bind:value={visualDirection} rows="3" placeholder="Personalidad, colores, estilo, nivel de densidad"></textarea></label>
            <label for="signature">Sello propio IntentOS<textarea id="signature" bind:value={signature} rows="3" placeholder="Rasgos distintivos que deben hacer reconocible este producto; evita interfaces genéricas"></textarea></label>
            <label for="references">Referencias visuales<textarea id="references" bind:value={references} rows="2" placeholder="URLs, productos o patrones de referencia; indica qué tomar de cada uno"></textarea></label>
            <label for="assets">Datos y activos disponibles<textarea id="assets" bind:value={availableAssets} rows="2" placeholder="Logo, fotografías, contenido, datos reales y credenciales autorizadas"></textarea></label>
            <div class="attachments">
              <div><strong>Archivos adjuntos</strong><small>Los agentes recibirán las rutas autorizadas de estos archivos.</small></div>
              <Button variant="secondary" onclick={addAttachments} ariaLabel="Adjuntar archivos"><PaperclipIcon size={14}/> Adjuntar</Button>
              {#if attachments.length}<ul>{#each attachments as path (path)}<li title={path}><span>{fileName(path)}</span><button type="button" onclick={() => removeAttachment(path)} aria-label={`Quitar ${fileName(path)}`}><XIcon size={13}/></button></li>{/each}</ul>{/if}
            </div>
            <label for="constraints">Restricciones y fuera de alcance<textarea id="constraints" bind:value={constraints} rows="2" placeholder="Qué no debe construir o qué debe respetar"></textarea></label>
            <label for="acceptance">Criterios de aprobación<textarea id="acceptance" bind:value={acceptance} rows="3" placeholder="Pruebas, navegadores, tamaños y evidencias requeridas"></textarea></label>
          </div>
        {/if}
        <div class="fields">
          <label for="project">Proyecto<select id="project" bind:value={projectPath}><option value="">Seleccionar proyecto</option>{#each projects.list as p (p.path)}<option value={p.path}>{p.label}</option>{/each}</select></label>
          <Button variant="secondary" onclick={() => projects.addViaPicker()} ariaLabel="Añadir proyecto"><PlusIcon size={14}/> Añadir</Button>
        </div>
        <label for="pipeline">Pipeline<select id="pipeline" bind:value={selectedSlug}>{#each runbooks.list as rb (rb.slug)}<option value={rb.slug}>{title(rb)}</option>{/each}</select></label>
        {#if validation}<p id="validation" class="error" role="alert">{validation}</p>{/if}
        {#if !provider && runs.providers.length}<p class="error">{runs.providers[0].unavailableReason ?? "Instala un Codex CLI independiente y autentícalo."}</p>{/if}
        <Button variant="primary" onclick={start} disabled={!canStart} loading={runs.starting} ariaLabel="Iniciar producción"><PlayIcon size={15}/> Iniciar producción</Button>
      </div>

      <h2>Pipelines disponibles</h2>
      <ul class="recipes">
        {#each runbooks.list as rb (rb.slug)}
          <li>
            <button class="recipe" onclick={() => openSlug = openSlug === rb.slug ? null : rb.slug} aria-expanded={openSlug === rb.slug} aria-controls={`recipe-${rb.slug}`}><ChevronDown size={15}/><span><strong>{title(rb)}</strong><small>{summary(rb)}</small></span></button>
            <div class="recipe-actions"><button onclick={() => copyPrompt(rb)}><CopyIcon size={13}/> Copiar prompt</button><button onclick={() => deployRb = rb}><DownloadIcon size={13}/> Desplegar</button></div>
            {#if openSlug === rb.slug}<div id={`recipe-${rb.slug}`} class="roster">{#each rb.roster as group (group.group)}<section><strong>{group.group}</strong>{#each resolve(group.agents) as item (item.slug)}<span>{item.agent?.emoji ?? "○"} {item.agent?.name ?? item.slug}</span>{/each}</section>{/each}</div>{/if}
          </li>
        {/each}
      </ul>
    </div>

    <aside class="console" aria-live="polite">
      {#if runs.current}
        <div class="run-head"><div><span class="eyebrow">EJECUCIÓN</span><h2>{runs.current.status}</h2><p title={runs.current.projectPath}>{runs.current.projectPath}</p></div>{#if runs.current.status === "running" || runs.current.status === "queued"}<Button variant="danger" onclick={() => runs.cancel()} loading={runs.cancelling}><SquareIcon size={13}/> Cancelar</Button>{/if}</div>
        <ol class="stages">{#each runs.current.stages as stage (stage.id)}<li class:active={stage.status === "running"} aria-current={stage.status === "running" ? "step" : undefined}><span class={`dot ${stage.status}`}></span><div><strong>{stage.label}</strong><small>{stage.agentSlug} · intento {stage.attempt || 1}</small></div><b>{stage.status}</b></li>{/each}</ol>
        <div class="log" role="log" aria-live="polite" aria-relevant="additions">{#if runs.events.length === 0}<p>Esperando actividad del runtime…</p>{:else}{#each runs.events as item (item.id)}<div><time>{new Date(item.at).toLocaleTimeString()}</time><pre>{eventText(item.event)}</pre></div>{/each}{/if}</div>
        {#if runs.current.error}<p class="error run-error">{runs.current.error}</p>{/if}
      {:else}
        <div class="empty"><PlayIcon size={36}/><h2>La fábrica está preparada</h2><p>Selecciona proyecto y pipeline, escribe tu intención e inicia una ejecución.</p></div>
      {/if}
    </aside>
  </div>
</section>

{#if deployRb}<InstallModal title={`Desplegar ${title(deployRb)}`} agentSlugs={resolvedSlugs(deployRb)} onClose={() => deployRb = null}/>{/if}

<style>
  .workspace{height:100%;display:flex;flex-direction:column;min-height:0}.head{padding:var(--space-3) var(--space-4);border-bottom:1px solid var(--color-border);display:flex;justify-content:space-between;gap:16px;align-items:center}.head h1{font-size:var(--text-h2)}.head p,.hint{color:var(--color-text-secondary);font-size:var(--text-body-sm)}.provider{font-size:11px;padding:5px 9px;border-radius:99px;background:color-mix(in srgb,var(--color-danger) 12%,transparent);color:var(--color-danger)}.provider.ok{background:color-mix(in srgb,var(--color-success) 12%,transparent);color:var(--color-success)}.grid{flex:1;min-height:0;overflow:auto;padding:var(--space-3);display:grid;grid-template-columns:minmax(340px,460px) minmax(380px,1fr);gap:var(--space-3)}.composer{display:flex;flex-direction:column;gap:var(--space-3)}.card,.console,.recipes>li{background:var(--color-surface-raised);border:1px solid var(--color-border);border-radius:var(--radius-lg)}.card{padding:var(--space-4);display:flex;flex-direction:column;gap:10px}label{font-size:var(--text-body-sm);font-weight:var(--fw-semibold);display:flex;flex-direction:column;gap:5px}textarea,select{width:100%;border:1px solid var(--color-border);border-radius:var(--radius-md);background:var(--color-surface);color:var(--color-text-primary);padding:9px;font:inherit}textarea{resize:vertical}.brief-toggle{display:flex;align-items:center;justify-content:space-between;gap:10px;width:100%;padding:10px;border:1px solid var(--color-border);border-radius:var(--radius-md);background:var(--color-surface)}.brief-toggle span{display:flex;flex-direction:column;align-items:flex-start}.brief-toggle small{color:var(--color-text-muted);font-size:11px}.brief-toggle svg{transition:transform .15s ease}.brief-toggle svg.rotated{transform:rotate(180deg)}.brief{display:grid;grid-template-columns:1fr 1fr;gap:10px;padding:10px;border:1px solid var(--color-border);border-radius:var(--radius-md);background:var(--color-surface-sunken)}.brief label:nth-child(n+3),.attachments{grid-column:1/-1}.brief textarea{background:var(--color-surface-raised);font-size:12px}.attachments{display:grid;grid-template-columns:1fr auto;gap:8px;align-items:center;padding:9px;border:1px dashed var(--color-border);border-radius:var(--radius-md)}.attachments>div{display:flex;flex-direction:column}.attachments small{font-size:11px;color:var(--color-text-muted)}.attachments ul{grid-column:1/-1;display:flex;flex-direction:column;gap:4px;list-style:none}.attachments li{display:flex;justify-content:space-between;align-items:center;gap:8px;padding:5px 7px;border-radius:var(--radius-sm);background:var(--color-surface-raised);font-size:11px}.attachments li span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.attachments li button{display:flex;color:var(--color-text-muted)}.fields{display:grid;grid-template-columns:1fr auto;align-items:end;gap:8px}.error{font-size:var(--text-body-sm);color:var(--color-danger)}h2{font-size:var(--text-h3)}.recipes{list-style:none;display:flex;flex-direction:column;gap:8px}.recipes>li{padding:9px}.recipe{display:flex;gap:8px;width:100%;text-align:left}.recipe span{display:flex;flex-direction:column;gap:2px}.recipe small{font-size:12px;color:var(--color-text-secondary)}.recipe-actions{display:flex;gap:8px;justify-content:flex-end}.recipe-actions button{display:flex;gap:4px;align-items:center;color:var(--color-brand);font-size:12px}.roster{border-top:1px solid var(--color-border);margin-top:8px;padding-top:8px;display:grid;grid-template-columns:repeat(auto-fit,minmax(160px,1fr));gap:10px}.roster section,.roster span{display:flex;flex-direction:column;font-size:12px;color:var(--color-text-secondary)}.console{min-height:0;padding:var(--space-4);display:flex;flex-direction:column;gap:var(--space-3)}.run-head{display:flex;justify-content:space-between;gap:10px}.run-head p{font-size:11px;color:var(--color-text-muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:440px}.eyebrow{font-size:10px;color:var(--color-brand);letter-spacing:.08em}.stages{list-style:none;display:flex;flex-direction:column;gap:6px}.stages li{display:grid;grid-template-columns:12px 1fr auto;align-items:center;gap:9px;padding:8px;border-radius:var(--radius-md);background:var(--color-surface)}.stages li.active{outline:1px solid var(--color-brand)}.stages small{display:block;color:var(--color-text-muted);font-size:11px}.stages b{font-size:10px;text-transform:uppercase}.dot{width:9px;height:9px;border-radius:50%;background:var(--color-border)}.dot.running{background:var(--color-brand)}.dot.passed{background:var(--color-success)}.dot.failed{background:var(--color-danger)}.log{flex:1;min-height:180px;overflow:auto;background:var(--color-surface-sunken);border-radius:var(--radius-md);padding:10px}.log div{display:grid;grid-template-columns:76px 1fr;gap:8px;border-bottom:1px solid var(--color-border);padding:5px 0}.log time{font:10px var(--font-mono);color:var(--color-text-muted)}.log pre{white-space:pre-wrap;word-break:break-word;font:11px/1.45 var(--font-mono);color:var(--color-text-secondary)}.empty{margin:auto;text-align:center;max-width:360px;color:var(--color-text-secondary)}.empty h2{color:var(--color-text-primary);margin:10px 0 5px}.run-error{padding:8px;background:color-mix(in srgb,var(--color-danger) 10%,transparent);border-radius:var(--radius-md)}button{cursor:pointer;background:none;color:inherit}@media(max-width:820px){.grid{grid-template-columns:1fr}.console{min-height:520px}.head{align-items:flex-start;flex-direction:column}.fields,.brief{grid-template-columns:1fr}.brief label:nth-child(n){grid-column:auto}.recipe-actions{justify-content:flex-start;flex-wrap:wrap}}@media(prefers-reduced-motion:reduce){*{scroll-behavior:auto!important}.brief-toggle svg{transition:none}}
  .router{display:flex;flex-direction:column;gap:8px;padding:10px;border:1px solid color-mix(in srgb,var(--color-brand) 45%,var(--color-border));border-radius:var(--radius-md);background:color-mix(in srgb,var(--color-brand) 5%,var(--color-surface-sunken))}.router-head{display:flex;justify-content:space-between;align-items:flex-start;gap:8px}.router-head>div{display:flex;flex-direction:column}.router-head small,.router p,.signals{color:var(--color-text-muted);font-size:11px}.router-head>span{padding:3px 7px;border-radius:99px;background:color-mix(in srgb,var(--color-brand) 14%,transparent);color:var(--color-brand);font-size:10px;font-weight:var(--fw-semibold)}.dynamic-team{display:grid;grid-template-columns:1fr 1fr;gap:5px;list-style:none}.dynamic-team li{display:grid;grid-template-columns:20px 1fr;gap:6px;align-items:center;padding:5px;border-radius:var(--radius-sm);background:var(--color-surface-raised)}.dynamic-team li>span{display:grid;place-items:center;width:18px;height:18px;border-radius:50%;background:color-mix(in srgb,var(--color-brand) 15%,transparent);color:var(--color-brand);font-size:9px;font-weight:700}.dynamic-team li div{min-width:0;display:flex;flex-direction:column}.dynamic-team strong,.dynamic-team small{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.dynamic-team strong{font-size:10px}.dynamic-team small{font-size:9px;color:var(--color-text-muted)}.dynamic-team li.missing{outline:1px solid var(--color-danger)}@media(max-width:520px){.dynamic-team{grid-template-columns:1fr}}
  .head-actions{display:flex;align-items:center;gap:8px;flex-wrap:wrap}
</style>
