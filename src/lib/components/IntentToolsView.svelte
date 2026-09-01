<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { LocalModelStatus } from "$lib/types";
  import ToolsView from "./ToolsView.svelte";
  import Braces from "@lucide/svelte/icons/braces";
  import Terminal from "@lucide/svelte/icons/terminal";
  import Globe2 from "@lucide/svelte/icons/globe-2";
  import FlaskConical from "@lucide/svelte/icons/flask-conical";
  import Database from "@lucide/svelte/icons/database";
  import Image from "@lucide/svelte/icons/image";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import PackageCheck from "@lucide/svelte/icons/package-check";
  import Cpu from "@lucide/svelte/icons/cpu";

  type Lens = "internal" | "engines";
  let lens: Lens = $state("internal");
  let localModel = $state<LocalModelStatus | null>(null);
  let localModelBusy = $state(false);
  let localModelError = $state("");

  async function refreshLocalModel() {
    localModel = await invoke<LocalModelStatus>("local_model_status");
  }

  async function startLocalModel() {
    localModelBusy = true;
    localModelError = "";
    try {
      localModel = await invoke<LocalModelStatus>("local_model_start");
    } catch (error) {
      localModelError = String(error);
      await refreshLocalModel().catch(() => undefined);
    } finally {
      localModelBusy = false;
    }
  }

  onMount(() => {
    void refreshLocalModel()
      .catch(() => (localModel = null));
  });

  const capabilities = $derived([
    {
      icon: Cpu,
      name: "Motor local soberano",
      detail: localModel?.sovereignReady
        ? `Inferencia ${localModel.executionMode === "nvidia" ? "acelerada por NVIDIA" : "local por CPU"}, sin consumo de proveedores pagados.`
        : localModelError || localModel?.blockers[0] || "Comprobando runner y modelo local…",
      state: localModel?.sovereignReady ? "Operativo" : "No preparado",
    },
    { icon: Braces, name: "Editor y archivos", detail: "Crea, modifica, compara y versiona los artefactos del proyecto.", state: "Activo" },
    { icon: Terminal, name: "Terminal y procesos", detail: "Ejecuta compiladores, servidores y tareas con límites y trazabilidad.", state: "Activo" },
    { icon: Globe2, name: "Navegador y Showroom", detail: "Abre el resultado, comprueba salud y prepara una muestra interactiva compartible.", state: "En construcción" },
    { icon: FlaskConical, name: "Pruebas y Reality Loop", detail: "Observa fallos, repara y vuelve a ejecutar hasta obtener evidencia.", state: "Activo" },
    { icon: Database, name: "Datos", detail: "Inspecciona, valida y deduplica datos mediante capacidades gobernadas.", state: "Activo" },
    { icon: Image, name: "Medios y experiencia", detail: "Produce y verifica recursos visuales, documentos y experiencias interactivas.", state: "Preparado" },
    { icon: ShieldCheck, name: "Seguridad y aislamiento", detail: "Protege el proyecto original y controla archivos, procesos, red y secretos.", state: "Activo" },
    { icon: PackageCheck, name: "Entrega", detail: "Prepara revisión, aplicación, paquete, repositorio y publicación verificable.", state: "Activo" },
  ]);
</script>

<section class="intent-tools">
  <header>
    <div>
      <p class="eyebrow">INTENTOS FABRIC</p>
      <h2>Herramientas propias</h2>
      <p>Capacidades que pertenecen a IntentOS. Los proveedores y modelos se eligen por debajo.</p>
    </div>
    <div class="lens" role="tablist" aria-label="Tipo de herramienta">
      <button class:active={lens === "internal"} role="tab" aria-selected={lens === "internal"} onclick={() => lens = "internal"}>Internas</button>
      <button class:active={lens === "engines"} role="tab" aria-selected={lens === "engines"} onclick={() => lens = "engines"}>Motores externos</button>
    </div>
  </header>

  {#if lens === "internal"}
    <div class="principle">
      <strong>La intención elige el resultado.</strong>
      <span>IntentOS selecciona silenciosamente herramientas, conocimiento y motores.</span>
    </div>
    <div class="grid">
      {#each capabilities as capability, index (capability.name)}
        <article>
          <div class="icon"><capability.icon size={20} /></div>
          <div>
            <div class="card-title"><h3>{capability.name}</h3><span>{capability.state}</span></div>
            <p>{capability.detail}</p>
            {#if index === 0 && localModel && !localModel.sovereignReady}
              <button class="motor-action" disabled={localModelBusy || !localModel.serverExecutable || !localModel.modelPath} onclick={startLocalModel}>
                {localModelBusy ? "Encendiendo…" : "Encender motor local"}
              </button>
            {/if}
          </div>
        </article>
      {/each}
    </div>
  {:else}
    <div class="external-note">
      <strong>Compatibilidad heredada</strong>
      <span>Estos programas pueden recibir capacidades de IntentOS, pero no son herramientas internas ni definen la fábrica.</span>
    </div>
    <div class="legacy"><ToolsView /></div>
  {/if}
</section>

<style>
  .intent-tools{height:100%;min-height:0;display:flex;flex-direction:column;background:var(--color-surface)}
  header{display:flex;align-items:flex-start;justify-content:space-between;gap:24px;padding:24px 28px;border-bottom:1px solid var(--color-border)}
  .eyebrow{margin:0 0 5px;color:var(--color-brand);font:700 10px var(--font-mono);letter-spacing:.16em}
  h2{margin:0;color:var(--color-text-primary);font-size:24px;letter-spacing:-.03em}
  header p:last-child{max-width:650px;margin:6px 0 0;color:var(--color-text-muted);font-size:13px}
  .lens{display:flex;padding:3px;border:1px solid var(--color-border);border-radius:10px;background:var(--color-surface-sunken)}
  .lens button{padding:7px 12px;border-radius:7px;color:var(--color-text-muted);font-size:12px;white-space:nowrap}
  .lens button.active{background:var(--color-surface-raised);color:var(--color-text-primary);box-shadow:var(--shadow-sm)}
  .principle,.external-note{display:flex;gap:10px;align-items:center;margin:20px 28px 0;padding:12px 14px;border:1px solid var(--color-border);border-radius:10px;background:var(--color-surface-raised);font-size:12px}
  .principle strong{color:var(--color-brand)}.principle span,.external-note span{color:var(--color-text-muted)}
  .external-note{margin-bottom:14px}.external-note strong{color:var(--color-text-primary)}
  .grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px;padding:18px 28px 28px;overflow:auto}
  article{display:grid;grid-template-columns:42px 1fr;gap:13px;min-height:110px;padding:17px;border:1px solid var(--color-border);border-radius:12px;background:var(--color-surface-raised)}
  article:hover{border-color:color-mix(in srgb,var(--color-brand) 45%,var(--color-border));transform:translateY(-1px)}
  .icon{width:42px;height:42px;display:grid;place-items:center;border-radius:10px;background:var(--color-brand-subtle);color:var(--color-brand)}
  .card-title{display:flex;align-items:center;justify-content:space-between;gap:10px}.card-title h3{margin:1px 0 7px;color:var(--color-text-primary);font-size:14px}.card-title span{padding:3px 7px;border-radius:999px;background:var(--color-surface-sunken);color:var(--color-text-muted);font:9px var(--font-mono)}
  article p{margin:0;color:var(--color-text-muted);font-size:12px;line-height:1.5}
  .motor-action{margin-top:11px;padding:7px 10px;border:1px solid color-mix(in srgb,var(--color-brand) 55%,var(--color-border));border-radius:8px;background:var(--color-brand-subtle);color:var(--color-brand);font-size:11px;font-weight:650}
  .motor-action:disabled{cursor:not-allowed;opacity:.5}
  .legacy{flex:1;min-height:0;border-top:1px solid var(--color-border)}
  @media(max-width:760px){header{flex-direction:column}.grid{grid-template-columns:1fr}.principle,.external-note{align-items:flex-start;flex-direction:column}}
</style>
