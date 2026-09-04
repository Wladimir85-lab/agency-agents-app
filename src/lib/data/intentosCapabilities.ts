import { invoke } from "@tauri-apps/api/core";

export interface IntentOSCapability {
  id: string;
  label: string;
  shortLabel: string;
  description: string;
  stageLabels: [string, string, string, string, string];
  agents: [string, string, string, string, string];
  keywords: string[];
}

export interface IntentOSPipelineStage {
  id: string;
  kind: "direction" | "architecture" | "development" | "qa" | "reality";
  label: string;
  agent: string;
  capabilityId: string;
  /** Only ever set on a `creative-technology` stage — which conceptual
   *  family (per CREATIVE_TECH_SPECIALIZATIONS) picked this stage's agent,
   *  for traceability. Absent for every other capability. */
  specializationId?: string;
}

/** Where a run's capability selection actually came from — surfaced in the
 *  proposal so a human (or a later audit like this one) can tell a real
 *  routing decision from a guess. Never hidden from the production brief. */
export interface RoutingTrace {
  source: "deterministic" | "semantic" | "semantic-unavailable";
  confidence: number;
  reason: string;
}

/** The explicit, auditable verdict on whether creative-technology earns its
 *  place in this build — see `evaluateCreativeTechnology`. A capability
 *  existing is never itself justification for using it (requirement #3). */
export interface CreativeTechVerdict {
  justified: boolean;
  reason: string;
  specializations: { id: string; status: CreativeTechSpecializationStatus }[];
}

export type CreativeTechSpecializationStatus = "supported-now" | "known-future";

/** A conceptual family inside `creative-technology`, not a new top-level
 *  IntentOSCapability — see the audit this evolves from. `supported-now`
 *  means IntentOS has both a real matching persona AND a real way to
 *  build/run/verify the result with tools it already has (the npm/web
 *  runtime `preview.rs` drives); `known-future` means the concept is real
 *  and a persona may even exist, but IntentOS cannot yet materialize or
 *  verify it end to end, so it must never be presented as ready. */
export interface CreativeTechSpecialization {
  id: string;
  label: string;
  status: CreativeTechSpecializationStatus;
  keywords: string[];
  /** Overrides creative-technology's default "development" agent
   *  (`xr-immersive-developer`) only when a genuinely better-fit persona
   *  exists AND IntentOS can actually run/verify its output. */
  developmentAgent: string;
  note: string;
}

export interface SolutionProposal {
  title: string;
  projectSlug: string;
  problem: string;
  user: string;
  job: string;
  outcome: string;
  solutionForm: string;
  capabilities: IntentOSCapability[];
  pipeline: IntentOSPipelineStage[];
  experience: string[];
  risks: string[];
  revisionNotes: string[];
  /** Present on every proposal produced by `planSolution`/`planSolutionAsync`
   *  — absent only on proposals built by code that predates this field. */
  routing?: RoutingTrace;
  creativeTechnology?: CreativeTechVerdict | null;
}

export interface CreationCatalogProduct {
  id: string;
  name: string;
  description: string;
  purpose: string;
  includes: string[];
  requiredInputs: string[];
  capabilityIds: string[];
  intentTemplate: string;
}

export interface CreationCatalogArea {
  id: string;
  number: string;
  name: string;
  description: string;
  products: CreationCatalogProduct[];
}

type ProductSeed = [name: string, description: string, capabilityIds?: string[]];

function catalogArea(
  id: string,
  number: string,
  name: string,
  description: string,
  purpose: string,
  includes: string[],
  requiredInputs: string[],
  defaultCapabilityIds: string[],
  seeds: ProductSeed[],
): CreationCatalogArea {
  return {
    id,
    number,
    name,
    description,
    products: seeds.map(([productName, productDescription, capabilityIds]) => ({
      id: `${id}:${productName.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "")}`,
      name: productName,
      description: productDescription,
      purpose,
      includes,
      requiredInputs,
      capabilityIds: capabilityIds ?? defaultCapabilityIds,
      intentTemplate: `Crear ${productName.toLowerCase()}. ${productDescription} El resultado debe quedar operativo, probado y listo para el uso definido por el usuario.`,
    })),
  };
}

/** Public creation offer. Areas organise what the user can request; capability
 * IDs remain the internal, additive requirements used by the runtime. */
export const CREATION_CATALOG: CreationCatalogArea[] = [
  catalogArea("frontend", "01", "Frontend", "Interfaces web y experiencias digitales.", "Presentar, vender, informar o permitir que una persona complete una tarea desde el navegador.", ["Diseño responsive", "Flujos e interacción", "Accesibilidad y rendimiento", "Pruebas visuales y funcionales"], ["Objetivo y público", "Contenido, logo y referencias", "Acción principal esperada", "Dominio o plataforma actual, si existe"], ["digital-experience"], [
    ["Landing pages", "Página enfocada en comunicar una oferta y conseguir una acción concreta."],
    ["Sitios corporativos", "Sitio institucional para explicar una organización, sus servicios y canales de contacto."],
    ["Portafolios profesionales", "Vitrina de trabajos, experiencia y capacidades para generar oportunidades."],
    ["Catálogos digitales", "Colección navegable de productos o servicios sin exigir una compra en línea."],
    ["Tiendas y vitrinas ecommerce", "Experiencia de descubrimiento, carrito y compra de productos.", ["digital-experience", "systems-data", "cybersecurity", "devops-quality"]],
    ["Sistemas de reservas y formularios", "Flujos para solicitar horas, cupos, cotizaciones o registrar información.", ["digital-experience", "systems-data", "cybersecurity", "operations-automation"]],
    ["Dashboards administrativos", "Interfaz para operar información, tareas, métricas y estados.", ["digital-experience", "systems-data", "cybersecurity"]],
    ["Rediseño de sitios existentes", "Renovación visual y funcional conservando lo que ya aporta valor."],
    ["Interfaces responsive", "Adaptación consistente para celular, tablet y escritorio."],
    ["Optimización web", "Mejoras de velocidad, SEO técnico y accesibilidad."],
    ["Implementación desde Figma o referencias", "Construcción fiel a un diseño o dirección visual ya definida."],
    ["Mantenimiento y evolución frontend", "Correcciones y mejoras continuas sobre una interfaz existente."]
  ]),
  catalogArea("backend-db", "02", "Backend / DB", "Lógica, APIs y persistencia de datos.", "Dar reglas, memoria, permisos e integraciones reales a productos digitales.", ["Modelo de datos", "API o acciones de servidor", "Validación y permisos", "Pruebas de integración"], ["Datos que deben guardarse", "Usuarios y permisos", "Reglas del negocio", "Servicios externos involucrados"], ["systems-data", "cybersecurity"], [
    ["Diseño de bases de datos", "Estructura consistente para almacenar y relacionar información."], ["APIs REST y endpoints", "Canales controlados para comunicar sistemas y aplicaciones."], ["Autenticación de usuarios", "Registro, ingreso, recuperación y sesiones seguras."], ["Roles y permisos", "Control de qué puede ver y hacer cada tipo de usuario."], ["Sistemas CRUD", "Alta, consulta, edición y eliminación controlada de registros."], ["Motores de reservas", "Disponibilidad y asignación de cupos con prevención de duplicados.", ["systems-data", "cybersecurity", "operations-automation"]], ["Gestión de clientes y fichas", "Registro central de personas, entidades e historiales."], ["Inventario y stock", "Control de existencias, movimientos y alertas."], ["Pagos e integraciones", "Conexión segura con proveedores y servicios externos.", ["systems-data", "cybersecurity", "devops-quality"]], ["Migración desde planillas", "Transformación de datos dispersos a un sistema consistente."], ["Importación y limpieza de datos", "Normalización, deduplicación y carga verificable."], ["Backups y recuperación", "Copias y procedimientos probados para restaurar información.", ["systems-data", "devops-quality", "cybersecurity"]]
  ]),
  catalogArea("ia-llm", "03", "IA / LLM", "Productos inteligentes basados en modelos y conocimiento.", "Automatizar trabajo cognitivo con límites, fuentes y evaluación explícitos.", ["Arquitectura del sistema IA", "Fuentes y herramientas", "Evaluaciones", "Límites y trazabilidad"], ["Tarea que debe resolver", "Fuentes autorizadas", "Ejemplos buenos y malos", "Decisiones que no puede tomar"], ["ai-agents", "systems-data"], [
    ["Chatbots especializados", "Conversación orientada a un dominio, tarea y público concretos."], ["Asistentes internos", "Apoyo al equipo usando procesos y conocimiento de la organización."], ["RAG sobre documentos", "Respuestas respaldadas por documentos recuperados y citables."], ["Bases de conocimiento inteligentes", "Organización y consulta asistida de información interna."], ["Clasificación automática", "Asignación de categorías o prioridades a textos y registros."], ["Extracción de información", "Conversión de documentos o mensajes en datos estructurados."], ["Resúmenes e informes", "Síntesis verificable de grandes volúmenes de información."], ["Generación de contenido", "Borradores controlados según voz, reglas y objetivos."], ["Transcripción y análisis de audio", "Conversión de voz en texto, temas, acuerdos o señales."], ["Visión computacional", "Análisis de imágenes para detectar, clasificar o extraer información."], ["Agentes con herramientas", "Sistemas que razonan y usan APIs bajo límites de autoridad.", ["ai-agents", "systems-data", "cybersecurity", "operations-automation"]], ["Evaluaciones de IA", "Pruebas repetibles de calidad, seguridad y comportamiento."], ["Automatización de seguimiento", "Acciones y comunicaciones basadas en eventos y criterios.", ["ai-agents", "operations-automation", "systems-data"]], ["IA en aplicaciones existentes", "Integración de capacidades inteligentes sin reemplazar el producto actual."]
  ]),
  catalogArea("devops", "04", "DevOps", "Entrega, infraestructura y operación confiable.", "Poner productos en Internet y mantenerlos disponibles, observables y recuperables.", ["Infraestructura reproducible", "Automatización de entrega", "Monitoreo", "Plan de recuperación"], ["Repositorio y tecnología", "Proveedor o restricciones", "Dominio y ambientes", "Nivel esperado de disponibilidad"], ["devops-quality", "cybersecurity"], [
    ["Deploy en Internet", "Publicación verificable de una aplicación en un entorno real."], ["Dominios, DNS y SSL", "Configuración de dirección pública y conexión cifrada."], ["Ambientes desarrollo y producción", "Separación segura entre cambios y uso real."], ["Contenedores Docker", "Empaquetado reproducible de aplicaciones y dependencias."], ["Pipelines CI/CD", "Pruebas y despliegues automáticos ante cambios aprobados."], ["Gestión de secretos", "Manejo seguro de credenciales y variables sensibles."], ["Monitoreo y alertas", "Detección de caídas, errores y degradación."], ["Backups y recuperación", "Respaldo y restauración probada de servicios y datos."], ["Optimización de costos cloud", "Ajuste de recursos según carga y prioridades."], ["Migración de hosting", "Traslado controlado entre proveedores minimizando interrupciones."], ["Ambientes staging", "Espacio realista para validar antes de publicar."], ["Mantenimiento operativo", "Actualizaciones, observabilidad y respuesta continua."]
  ]),
  catalogArea("cybersecurity", "05", "Ciberseguridad", "Protección práctica de aplicaciones, datos y accesos.", "Reducir riesgos antes y después de publicar un sistema.", ["Revisión basada en riesgo", "Hallazgos con evidencia", "Remediación priorizada", "Comprobación posterior"], ["Activo y alcance autorizado", "Arquitectura y accesos de prueba", "Datos sensibles involucrados", "Criterio de riesgo aceptable"], ["cybersecurity"], [
    ["Auditoría básica de aplicaciones", "Revisión inicial de las superficies de riesgo más relevantes."], ["Revisión de autenticación y sesiones", "Evaluación de ingreso, recuperación, tokens y cierre de sesión."], ["Diseño de roles y accesos", "Aplicación del menor privilegio según responsabilidades."], ["Headers y configuración segura", "Endurecimiento de la exposición web y del navegador."], ["Detección de secretos", "Búsqueda de credenciales expuestas en código y configuración."], ["Auditoría de dependencias", "Identificación y priorización de componentes vulnerables."], ["Seguridad de APIs y formularios", "Validación, autorización y manejo seguro de entradas."], ["Hardening de bases de datos", "Reducción de permisos y exposición de la persistencia."], ["Prueba de backups", "Comprobación de que una restauración realmente funciona."], ["Rate limiting y abuso", "Límites contra automatización maliciosa y saturación."], ["Permisos cloud", "Revisión de identidades y accesos en infraestructura."], ["Checklist pre-lanzamiento", "Gate de seguridad antes de poner un producto en uso."], ["Plan de riesgos", "Registro priorizado con responsables y tratamientos."], ["Monitoreo de seguridad", "Señales y alertas para detectar actividad anómala."]
  ]),
  catalogArea("mobile", "06", "Frontend Mobile", "Experiencias para celular y trabajo en movimiento.", "Llevar flujos de clientes o equipos al dispositivo que usan en terreno.", ["Experiencia táctil", "Estados de red y dispositivo", "Integración con backend", "Pruebas en tamaños reales"], ["Usuarios y dispositivos", "Flujo principal", "Funciones del teléfono necesarias", "Distribución web o tiendas"], ["digital-experience", "systems-data"], [
    ["Aplicaciones multiplataforma", "Una experiencia móvil para más de un sistema operativo."], ["PWA instalables", "Aplicación web que puede instalarse y trabajar como app."], ["Portales móviles de clientes", "Acceso personal a solicitudes, estados, documentos o historial."], ["Apps de reservas", "Búsqueda de disponibilidad, reserva y gestión desde celular.", ["digital-experience", "systems-data", "cybersecurity", "operations-automation"]], ["Apps para personal en terreno", "Flujos rápidos para registrar y consultar trabajo fuera de oficina."], ["Formularios con fotografías", "Captura guiada de datos y evidencia visual."], ["Lectura QR y códigos", "Identificación y consulta mediante la cámara."], ["Geolocalización y mapas", "Experiencias dependientes de ubicación y recorridos."], ["Notificaciones push", "Avisos oportunos vinculados a eventos del sistema.", ["digital-experience", "systems-data", "operations-automation"]], ["Modo offline", "Continuidad básica sin conexión y sincronización posterior."], ["Dashboards móviles", "Indicadores y acciones prioritarias en pantallas pequeñas."], ["Integración cámara y archivos", "Uso controlado de capacidades y documentos del dispositivo."], ["Conversión web a móvil", "Adaptación de un producto web existente a uso móvil."], ["Preparación para tiendas", "Configuración, revisión y materiales para distribución."]
  ]),
  catalogArea("iot", "07", "IoT + Demo Day", "Dispositivos conectados y prototipos demostrables.", "Medir o actuar sobre el mundo físico y demostrar el sistema completo.", ["Firmware o dispositivo", "Comunicación y datos", "Dashboard o control", "Demostración con evidencia"], ["Problema físico", "Hardware disponible", "Variables y frecuencia", "Conectividad y entorno de uso"], ["iot", "systems-data"], [
    ["Prototipos ESP32 / MicroPython", "Dispositivo funcional para validar rápidamente una idea conectada."], ["Monitoreo ambiental", "Medición de temperatura, humedad u otras variables."], ["Sistemas MQTT", "Intercambio ligero de mensajes entre dispositivos y servicios."], ["Dashboards en tiempo real", "Visualización viva de telemetría y estados."], ["Alertas por sensores", "Avisos automáticos cuando una condición requiere atención.", ["iot", "systems-data", "operations-automation"]], ["Control remoto", "Accionamiento autorizado de dispositivos desde una interfaz."], ["Registro de acceso y activos", "Identificación de entradas, objetos o movimientos."], ["Seguimiento de activos", "Ubicación o estado de elementos relevantes."], ["Integración IoT con web", "Unión del dispositivo con portal, datos y usuarios."], ["Edge y nube", "Distribución de procesamiento entre dispositivo y servicios."], ["TinyML", "Inferencia local de patrones en hardware limitado.", ["iot", "tinyml-edge-ai", "systems-data"]], ["Prueba de concepto IoT", "Validación acotada del valor y viabilidad técnica."], ["Demo Day", "Demostración clara del problema, sistema y evidencia obtenida.", ["iot", "creative-technology", "strategy-product"]]
  ]),
  catalogArea("creative-coding", "08", "Creative Coding", "Experiencias digitales expresivas e interactivas.", "Comunicar, explorar o sorprender mediante interacción, datos, imagen, sonido y movimiento.", ["Concepto y dirección visual", "Interacción programada", "Rendimiento", "Demostración navegable"], ["Objetivo y contexto", "Referencias visuales", "Contenido o datos", "Dispositivos y entorno de exhibición"], ["creative-technology", "digital-experience"], [
    ["Webs experimentales", "Sitios donde la interacción forma parte central del mensaje."], ["Experiencias interactivas de marca", "Piezas digitales memorables alineadas con una identidad."], ["Animaciones avanzadas", "Movimiento funcional y expresivo dentro de una interfaz."], ["Visualización creativa de datos", "Representaciones explorables que vuelven legible una historia."], ["Arte generativo", "Sistemas visuales creados mediante reglas y variación."], ["Experiencias 3D / WebGL", "Escenas tridimensionales ejecutadas en el navegador."], ["Simuladores interactivos", "Herramientas para explorar decisiones, fenómenos o escenarios."], ["Configuradores de producto", "Exploración visual de opciones y combinaciones."], ["Experiencias audiovisuales reactivas", "Imagen y movimiento que responden a sonido o interacción."], ["Instalaciones digitales", "Software para espacios físicos, muestras o eventos."], ["Kioscos interactivos", "Experiencias táctiles enfocadas en atención presencial."], ["Mapas narrativos", "Historias guiadas por territorio, tiempo y datos."], ["Demos de campaña", "Prototipos llamativos para validar una activación."], ["Prototipos conceptuales", "Materialización rápida de una idea digital poco convencional."]
  ]),
];

export function findCatalogProduct(productId: string | null | undefined): CreationCatalogProduct | null {
  if (!productId) return null;
  return CREATION_CATALOG.flatMap((area) => area.products).find((product) => product.id === productId) ?? null;
}

export const INTENTOS_CAPABILITIES: IntentOSCapability[] = [
  {
    id: "digital-experience",
    label: "Frontend y experiencia digital",
    shortLabel: "Experiencia digital",
    description: "Webs, e-commerce, aplicaciones e interfaces premium.",
    stageLabels: ["Dirección de proyecto", "UX y dirección creativa", "Desarrollo de experiencia", "QA visual y funcional", "Reality Check"],
    agents: ["project-manager-senior", "design-ux-architect", "engineering-frontend-developer", "testing-evidence-collector", "testing-reality-checker"],
    keywords: ["web", "sitio", "landing", "ecommerce", "e-commerce", "tienda", "frontend", "interfaz", "portal", "aplicación", "aplicacion", "app", "visual", "pantalla", "aplicación móvil", "app móvil"],
  },
  {
    id: "systems-data",
    label: "Sistemas, backend y datos",
    shortLabel: "Sistemas y datos",
    description: "APIs, bases de datos, dashboards, CRM y sistemas internos.",
    stageLabels: ["Dirección de proyecto", "Arquitectura de sistema", "Backend y datos", "QA de API e integración", "Reality Check"],
    agents: ["project-manager-senior", "engineering-software-architect", "engineering-backend-architect", "testing-api-tester", "testing-reality-checker"],
    keywords: ["backend", "api", "base de datos", "database", "dashboard", "crm", "inventario", "sistema interno", "panel administrativo", "datos", "registrar usuarios", "historial", "guardar", "persistencia", "persistente", "respaldo", "respaldos"],
  },
  {
    id: "iot",
    label: "IoT y sistemas conectados",
    shortLabel: "IoT",
    description: "Sensores, dispositivos, telemetría, alertas y automatización física.",
    stageLabels: ["Dirección de proyecto", "Arquitectura IoT", "Implementación IoT full stack", "QA de dispositivo y telemetría", "Reality Check"],
    agents: ["project-manager-senior", "engineering-software-architect", "engineering-rapid-prototyper", "testing-evidence-collector", "testing-reality-checker"],
    keywords: ["iot", "sensor", "sensores", "esp32", "arduino", "mqtt", "telemetría", "dispositivo", "firmware", "temperatura", "humedad"],
  },
  {
    id: "tinyml-edge-ai",
    label: "TinyML y Edge AI",
    shortLabel: "TinyML / Edge AI",
    description: "Modelos locales para imagen, sonido, eventos y anomalías en hardware.",
    stageLabels: ["Dirección de proyecto", "Arquitectura Edge AI", "Modelo y despliegue embebido", "Evaluación del modelo", "Reality Check"],
    agents: ["project-manager-senior", "engineering-autonomous-optimization-architect", "engineering-ai-engineer", "specialized-model-qa", "testing-reality-checker"],
    keywords: ["tinyml", "edge ai", "edge", "modelo local", "anomalía", "microcontrolador", "inferencia", "clasificación", "detección de sonido", "detección de imagen"],
  },
  {
    id: "cybersecurity",
    label: "Ciberseguridad",
    shortLabel: "Seguridad",
    description: "Auditoría, autenticación, permisos, secretos y hardening.",
    stageLabels: ["Dirección de auditoría", "Arquitectura de seguridad", "Revisión y remediación", "Pruebas de seguridad", "Reality Check"],
    agents: ["project-manager-senior", "security-architect", "security-appsec-engineer", "security-penetration-tester", "testing-reality-checker"],
    keywords: ["seguridad", "ciberseguridad", "auditoría", "vulnerabilidad", "pentest", "autenticación", "permisos", "secretos", "privacidad", "hardening"],
  },
  {
    id: "devops-quality",
    label: "DevOps, cloud y calidad",
    shortLabel: "DevOps y calidad",
    description: "Despliegue, CI/CD, observabilidad, rendimiento y operación.",
    stageLabels: ["Dirección de entrega", "Arquitectura cloud y seguridad", "Automatización y despliegue", "QA de rendimiento y operación", "Reality Check"],
    agents: ["project-manager-senior", "security-cloud-security-architect", "engineering-devops-automator", "testing-performance-benchmarker", "testing-reality-checker"],
    keywords: ["devops", "cloud", "deploy", "despliegue", "ci/cd", "cicd", "docker", "observabilidad", "monitoreo", "rendimiento", "infraestructura"],
  },
  {
    id: "operations-automation",
    label: "Procesos, automatización y operaciones",
    shortLabel: "Automatización operativa",
    description: "Sistemas que eliminan trabajo repetitivo, coordinan flujos y convierten eventos en acciones verificables.",
    stageLabels: ["Dirección del trabajo", "Arquitectura del proceso", "Implementación de automatización", "QA del flujo operativo", "Reality Check"],
    agents: ["project-manager-senior", "specialized-workflow-architect", "engineering-rapid-prototyper", "testing-test-automation-engineer", "testing-reality-checker"],
    keywords: ["automatizar", "automatización", "proceso", "flujo de trabajo", "workflow", "tarea repetitiva", "operaciones", "notificación", "integración", "sincronizar", "trabajos realizados", "pendiente", "en proceso", "terminado"],
  },
  {
    id: "data-decisions",
    label: "Datos, modelos y apoyo a decisiones",
    shortLabel: "Datos y decisiones",
    description: "Sistemas para estructurar datos, calcular, proyectar, visualizar escenarios y apoyar decisiones.",
    stageLabels: ["Dirección analítica", "Arquitectura de información", "Construcción del sistema de datos", "Validación de datos y decisiones", "Reality Check"],
    agents: ["project-manager-senior", "engineering-data-engineer", "engineering-data-visualization-engineer", "testing-evidence-collector", "testing-reality-checker"],
    keywords: ["analizar", "análisis", "indicador", "métrica", "proyección", "escenario", "flujo de caja", "financiero", "cálculo", "reporte", "visualizar", "decisión"],
  },
  {
    id: "ai-agents",
    label: "IA, agentes y sistemas de conocimiento",
    shortLabel: "IA y agentes",
    description: "Agentes con memoria, herramientas, conocimiento, límites de autoridad y evaluación verificable.",
    stageLabels: ["Dirección de misión IA", "Arquitectura agentic", "Implementación del agente", "Evaluación del comportamiento", "Reality Check"],
    agents: ["project-manager-senior", "engineering-autonomous-optimization-architect", "engineering-ai-engineer", "specialized-model-qa", "testing-reality-checker"],
    keywords: ["agente", "agentes", "inteligencia artificial", "llm", "rag", "memoria", "herramientas", "asistente", "conocimiento", "modelo"],
  },
  {
    id: "creative-technology",
    label: "Tecnología creativa y experiencias inmersivas",
    shortLabel: "Tecnología creativa",
    description: "3D, WebGL, shaders, visualización y narrativa cinematográfica.",
    stageLabels: ["Producción creativa", "Dirección de experiencia", "Desarrollo inmersivo", "QA visual y rendimiento", "Reality Check"],
    agents: ["project-management-studio-producer", "design-visual-storyteller", "xr-immersive-developer", "testing-performance-benchmarker", "testing-reality-checker"],
    keywords: ["webgl", "shader", "3d", "inmersiva", "inmersivo", "cinematográfica", "cinematográfico", "interactiva", "experiencia visual", "visualización", "motion"],
  },
  {
    id: "strategy-product",
    label: "Estrategia de producto y negocio",
    shortLabel: "Estrategia y producto",
    description: "Investigación, definición de MVP, modelo económico y distribución.",
    stageLabels: ["Dirección estratégica", "Investigación y arquitectura de producto", "Diseño de estrategia", "Validación de supuestos", "Reality Check"],
    agents: ["project-manager-senior", "product-trend-researcher", "business-strategist", "project-management-experiment-tracker", "testing-reality-checker"],
    keywords: ["estrategia", "mercado", "modelo de negocio", "modelo económico", "mvp", "validar idea", "investigación", "roadmap", "producto", "distribución", "escalabilidad"],
  },
];

/** Conceptual families inside `creative-technology` — see the interface doc
 *  comment. Audited against the real corpus (`corpus::ensure_corpus`) and
 *  against what IntentOS's own runtime can actually build/run/verify
 *  (`preview.rs` only ever drives an npm/web dev server — no game-engine
 *  editor, no native XR/AR build toolchain, no hardware). `supported-now`
 *  is deliberately conservative: a persona existing is not enough, and
 *  "the underlying LLM could probably improvise it" is not enough either —
 *  it requires the Naval Studio precedent's shape: a real corpus persona
 *  whose own expertise already covers this, building something the
 *  existing web runtime can genuinely run and Reality Check can genuinely
 *  inspect. Every `developmentAgent` below resolves against the real
 *  corpus (verified by hand this session, same as the audit).
 *
 *  `physical-interaction` deliberately has no override: IntentOS already
 *  has a full first-class `iot` capability (its own personas, its own
 *  domain_verification_guidance in runtime.rs) for exactly this. Routing
 *  physical/IoT work through creative-technology's roster instead would
 *  duplicate, not extend, existing architecture — so it stays
 *  known-future *from creative-technology's own agents' point of view*
 *  and defers to `iot` rather than pretending to own it. */
export const CREATIVE_TECH_SPECIALIZATIONS: CreativeTechSpecialization[] = [
  {
    id: "realtime-web-3d",
    label: "3D en tiempo real para navegador",
    status: "supported-now",
    keywords: ["three.js", "threejs", "webgl", "webgpu", "3d interactivo", "girar", "rotar", "recorrer", "recorrido 360", "explorar en 3d", "visor 3d", "modelo 3d"],
    developmentAgent: "xr-immersive-developer",
    note: "Probado en producción: Naval Studio (three ^0.169.0 + rhino3dm ^8.32.2, viewer3d-client.ts) — ver corpus/spatial-computing/xr-immersive-developer.md.",
  },
  {
    id: "shaders-graphics",
    label: "Shaders y gráficos en tiempo real (web)",
    status: "supported-now",
    keywords: ["shader", "glsl", "webgl", "efecto visual", "post-procesado", "post procesado"],
    developmentAgent: "xr-immersive-developer",
    note: "El mismo persona web (WebXR/Three.js) cubre shader tuning para navegador. Shader authoring específico de motor (Unity Shader Graph, Godot) es known-future: IntentOS no construye ni ejecuta proyectos de esos motores.",
  },
  {
    id: "spatial-xr",
    label: "AR/VR/XR nativo (headset, fuera del navegador)",
    status: "known-future",
    keywords: ["realidad aumentada nativa", "realidad virtual nativa", "vision pro", "visionos", "hololens", "meta quest nativo", "oculus nativo"],
    developmentAgent: "xr-immersive-developer",
    note: "Personas reales existen (corpus/spatial-computing/visionos-spatial-engineer.md, macos-spatial-metal-engineer.md) pero IntentOS no tiene toolchain nativo (Xcode/build/simulador) para construir, ejecutar ni verificar el resultado. XR dentro del navegador (WebXR) es 'realtime-web-3d', no esto.",
  },
  {
    id: "generative-visuals",
    label: "Arte/visuales generativos",
    status: "known-future",
    keywords: ["arte generativo", "generativo", "procedural", "creative coding"],
    developmentAgent: "xr-immersive-developer",
    note: "Sin persona dedicada en el corpus (no hay 'creative coder'/generative-art specialist) ni precedente ejecutado. El generalista WebXR podría intentarlo, pero sin prueba real no se presenta como supported-now.",
  },
  {
    id: "spatial-capture",
    label: "Captura 3D del mundo físico (fotogrametría/LiDAR)",
    status: "known-future",
    keywords: ["lidar", "fotogrametría", "fotogrametria", "escaneo 3d", "nube de puntos", "point cloud"],
    developmentAgent: "xr-immersive-developer",
    note: "La mitad web (visualizar la nube de puntos/malla resultante en el navegador) es 'realtime-web-3d', ya probada. La captura nativa (ARKit/LiDAR en un dispositivo) requiere una app móvil nativa que IntentOS no construye ni despliega hoy.",
  },
  {
    id: "simulation-digital-twin",
    label: "Simulación / gemelo digital",
    status: "known-future",
    keywords: ["gemelo digital", "digital twin", "simulación física", "simulacion fisica"],
    developmentAgent: "xr-immersive-developer",
    note: "Sin persona dedicada ni precedente ejecutado. Marcado explícitamente known-future en vez de improvisar una afirmación de capacidad.",
  },
  {
    id: "audio-reactive",
    label: "Audiovisual reactivo",
    status: "known-future",
    keywords: ["audio reactivo", "audio-reactivo", "reactivo al sonido", "visualizador de audio"],
    developmentAgent: "xr-immersive-developer",
    note: "Web Audio API + Three.js lo haría técnicamente posible, pero sin persona dedicada ni precedente ejecutado no se presenta como supported-now.",
  },
  {
    id: "physical-interaction",
    label: "Instalación física / computación física",
    status: "known-future",
    keywords: ["instalación física", "instalacion fisica", "projection mapping", "proyección mapeada", "kiosco interactivo", "sensor físico"],
    developmentAgent: "xr-immersive-developer",
    note: "IntentOS ya tiene una capability propia y operativa para esto ('iot', con sus propios agentes y su propio domain_verification_guidance en runtime.rs). Un intent predominantemente físico/IoT debe enrutarse ahí, no a creative-technology.",
  },
];

/** Best-effort, deterministic tagging of which conceptual family (or
 *  families) inside creative-technology an intent's text touches. This is
 *  a secondary refinement for *which agent/specialization to record* once
 *  creative-technology has already been selected — it never decides
 *  whether creative-technology itself is justified; see
 *  `evaluateCreativeTechnology` for that. Keyword-based like the existing
 *  fast path, and deliberately so: this only affects agent selection
 *  inside an already-approved creative-technology pipeline, so a miss here
 *  costs nothing beyond falling back to the generalist
 *  `xr-immersive-developer` — unlike the capability-level decision, it
 *  never needs a semantic fallback of its own. */
export function pickCreativeTechSpecializations(text: string): CreativeTechSpecialization[] {
  const normalized = text.toLocaleLowerCase("es");
  const matches = CREATIVE_TECH_SPECIALIZATIONS.filter((spec) => spec.keywords.some((keyword) => normalized.includes(keyword)));
  return matches.length ? matches : [CREATIVE_TECH_SPECIALIZATIONS[0]];
}

export function routeIntent(text: string): { capability: IntentOSCapability; capabilities: IntentOSCapability[]; confidence: number; matched: string[]; ranked: { capability: IntentOSCapability; score: number; matched: string[] }[] } {
  const normalized = text.toLocaleLowerCase("es");
  const ranked = INTENTOS_CAPABILITIES.map((capability) => {
    const matched = capability.keywords.filter((keyword) => normalized.includes(keyword));
    return { capability, matched, score: matched.reduce((sum, keyword) => sum + (keyword.includes(" ") ? 2 : 1), 0) };
  }).sort((a, b) => b.score - a.score);
  const best = ranked[0];
  const confidence = best.score === 0 ? 0 : Math.min(0.95, 0.45 + best.score * 0.1);
  const capability = best.score ? best.capability : INTENTOS_CAPABILITIES[0];
  // A single strong keyword is sufficient to select a primary capability.
  // The previous floor of 2 produced an empty roster whenever best.score was
  // exactly 1 (for example, an intent containing only "aplicación").
  const threshold = Math.max(1, Math.ceil(best.score * 0.4));
  const selected = best.score
    ? ranked.filter((item) => item.score >= threshold).slice(0, 4).map((item) => item.capability)
    : [capability];
  // Some product properties imply a capability even when another capability
  // has many more keyword matches. They are additive requirements, not rival
  // classifications competing for a single winning label.
  const ensureCapability = (id: string) => {
    const required = INTENTOS_CAPABILITIES.find((item) => item.id === id);
    if (required && !selected.some((item) => item.id === id)) selected.push(required);
  };
  if (/\b(aplicaci[oó]n|app|interfaz|pantalla|visual)\b/.test(normalized)) ensureCapability("digital-experience");
  if (/\b(datos?|historial|guardar|persistencia|persistente|respaldo|respaldos|registro|registrar)\b/.test(normalized)) ensureCapability("systems-data");
  if (/\b(flujo de trabajo|workflow|trabajos realizados|pendiente|en proceso|terminado)\b/.test(normalized)) ensureCapability("operations-automation");
  if (selected.length > 4) selected.length = 4;
  const capabilities = selected.length ? selected : [capability];
  return { capability, capabilities, confidence, matched: best.matched, ranked };
}

/** Whether the deterministic fast path above is trustworthy enough to skip
 *  the semantic fallback entirely — the hybrid router's only gate on
 *  calling the LLM at all, so it must stay cheap and conservative in both
 *  directions: never trigger the LLM for a genuinely unambiguous intent
 *  (latency/cost), and never trust a guess that the audit specifically
 *  found unreliable.
 *
 *  Two concrete failure modes drove these two rules, not a general
 *  confidence-threshold guess:
 *  - `routed.confidence === 0`: nothing matched at all — today's behaviour
 *    silently defaults to `INTENTOS_CAPABILITIES[0]` (digital-experience).
 *    That is exactly the "sin coincidencia clara" case that must fall
 *    back, not the "functional intent without exact keywords" case only —
 *    the same rule covers both.
 *  - creative-technology entering the selected set on a single one-word
 *    keyword hit (`matched.length < 2`): the audit's case C — a bare "3d"
 *    or "visual" substring must never alone earn WebGL/3D. A stronger
 *    signal (2+ keyword hits, or one 2-word phrase already worth 2 points)
 *    stays fast-path, same as every other capability. */
function isFastPathConfident(routed: ReturnType<typeof routeIntent>): boolean {
  if (routed.confidence === 0) return false;
  const creativeTech = routed.ranked.find((entry) => entry.capability.id === "creative-technology");
  if (creativeTech && routed.capabilities.includes(creativeTech.capability) && creativeTech.matched.length < 2) {
    return false;
  }
  return true;
}

/** `intentText` is optional and additive: omitting it (every pre-existing
 *  caller) reproduces the exact stages composePipeline always produced —
 *  the specialization lookup only ever *narrows which agent fills
 *  creative-technology's own development slot*, never adds, removes, or
 *  reorders a stage, so the pipeline never grows because of it. */
export function composePipeline(capabilities: IntentOSCapability[], intentText = ""): IntentOSPipelineStage[] {
  const primary = capabilities[0] ?? INTENTOS_CAPABILITIES[0];
  const stages: IntentOSPipelineStage[] = [{ id: "direction", kind: "direction", label: primary.stageLabels[0], agent: primary.agents[0], capabilityId: primary.id }];
  for (const capability of capabilities) stages.push({ id: `${capability.id}:architecture`, kind: "architecture", label: capability.stageLabels[1], agent: capability.agents[1], capabilityId: capability.id });
  for (const capability of capabilities) {
    if (capability.id === "creative-technology") {
      // Reuse the same specialization pick creative-technology already
      // uses for its own note/verdict — one lookup, not a duplicated one —
      // and only ever override the agent for a `supported-now` family; a
      // `known-future` match keeps the generalist and still records which
      // family was recognised, so it stays honest rather than silent.
      const specializations = pickCreativeTechSpecializations(intentText);
      const chosen = specializations.find((spec) => spec.status === "supported-now") ?? specializations[0];
      stages.push({
        id: `${capability.id}:development`,
        kind: "development",
        label: capability.stageLabels[2],
        agent: chosen.status === "supported-now" ? chosen.developmentAgent : capability.agents[2],
        capabilityId: capability.id,
        specializationId: chosen.id,
      });
    } else {
      stages.push({ id: `${capability.id}:development`, kind: "development", label: capability.stageLabels[2], agent: capability.agents[2], capabilityId: capability.id });
    }
  }
  for (const capability of capabilities) stages.push({ id: `${capability.id}:qa`, kind: "qa", label: capability.stageLabels[3], agent: capability.agents[3], capabilityId: capability.id });
  stages.push({ id: "reality-check", kind: "reality", label: primary.stageLabels[4], agent: primary.agents[4], capabilityId: primary.id });
  return stages;
}

function projectSlug(text: string): string {
  const normalized = text.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
  const slug = normalized.replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 54);
  return slug || "nuevo-proyecto";
}

function firstMeaningfulLine(text: string): string {
  return text.split(/\r?\n/).map((line) => line.trim()).find(Boolean) ?? "Nuevo proyecto";
}

function nctoRevisionNotes(text = ""): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.match(/^OBSERVACIÓN NCTO \([^)]*\):\s*(.+)$/i)?.[1]?.trim())
    .filter((line): line is string => Boolean(line));
}

function audienceFromRevision(notes: string[]): string | null {
  for (const note of [...notes].reverse()) {
    const match = note.match(/usuarios?(?:\s+objetivo|\s+principales?)?\s+(?:son|serán|sean|a)\s+([^.!?]+)/i);
    if (match?.[1]) return match[1].trim();
  }
  return null;
}

function audienceFromIntent(lower: string): string {
  if ((lower.includes("landing") || lower.includes("sitio") || lower.includes("web")) && (lower.includes("estudio") || lower.includes("agencia"))) {
    return "Personas y organizaciones que evalúan contratar los servicios del estudio";
  }
  if (lower.includes("ecommerce") || lower.includes("e-commerce") || lower.includes("tienda")) return "Personas que necesitan descubrir, evaluar y comprar los productos ofrecidos";
  if (lower.includes("dashboard") || lower.includes("panel") || lower.includes("sistema interno")) return "Equipo responsable de operar y tomar decisiones con la información del sistema";
  return "Personas directamente afectadas por el problema descrito en la intención";
}

function jobFromIntent(lower: string): string {
  if (lower.includes("landing") || lower.includes("sitio") || lower.includes("web")) return "Comprender la propuesta, explorar su contenido principal y completar la acción de contacto o conversión sin fricción";
  if (lower.includes("automat") || lower.includes("proceso")) return "Completar el proceso con menos trabajo manual, errores y pérdida de trazabilidad";
  return "Resolver de extremo a extremo el trabajo descrito en la intención mediante una experiencia operable";
}

function outcomeFromIntent(lower: string): string {
  if (lower.includes("landing") || lower.includes("sitio") || lower.includes("web")) return "Experiencia web funcional, responsive, accesible y comprobada visualmente en los flujos prometidos";
  return "Solución operable, verificable y correspondiente a la intención y propuesta aprobadas";
}

interface PlanSolutionInput {
  intent: string;
  audience?: string;
  requiredFeatures?: string;
  constraints?: string;
  acceptance?: string;
  requiredCapabilityIds?: string[];
}

function combineInputText(input: PlanSolutionInput): string {
  return [input.intent, input.audience, input.requiredFeatures, input.constraints, input.acceptance].filter(Boolean).join("\n");
}

function dedupeCapabilities(list: IntentOSCapability[]): IntentOSCapability[] {
  return list.filter((capability, index, all) => all.findIndex((item) => item.id === capability.id) === index).slice(0, 4);
}

/** The one, explicit, auditable answer to requirement #3: does creative
 *  technology earn its place in *this* build? `semantic`, when present,
 *  is authoritative (a human/LLM judgement about material improvement
 *  beats a keyword count either way — reject *or* confirm). Absent
 *  semantic input, a real deterministic keyword match still counts as
 *  justified (unchanged behaviour), but a keyword that matched creative-
 *  technology without clearing the selection threshold is recorded as an
 *  explicit rejection rather than silently disappearing — so a reviewer
 *  can always see *why* creative-tech isn't in the roster, not just that
 *  it isn't. Returns null only when creative-technology was never even a
 *  candidate, so unrelated proposals aren't cluttered with this section. */
function creativeTechVerdictFromRouting(
  capabilities: IntentOSCapability[],
  routed: ReturnType<typeof routeIntent>,
  text: string,
  semantic?: SemanticClassification | null,
): CreativeTechVerdict | null {
  const specializations = pickCreativeTechSpecializations(text).map((spec) => ({ id: spec.id, status: spec.status }));
  if (semantic?.creativeTechnologyJustified === false) {
    return { justified: false, reason: semantic.creativeTechnologyReason || "El análisis semántico (Esmeralda/Qwen) determinó que la tecnología creativa no aporta una mejora material a este producto.", specializations };
  }
  if (semantic?.creativeTechnologyJustified === true) {
    return { justified: true, reason: semantic.creativeTechnologyReason || "El análisis semántico (Esmeralda/Qwen) confirmó una mejora material a partir de la intención.", specializations };
  }
  const selected = capabilities.some((capability) => capability.id === "creative-technology");
  const ranked = routed.ranked.find((entry) => entry.capability.id === "creative-technology");
  if (selected) {
    return { justified: true, reason: ranked?.matched.length ? `Coincidencia determinista de palabras clave: ${ranked.matched.join(", ")}.` : "Capability requerida explícitamente por el producto del catálogo.", specializations };
  }
  if (ranked && ranked.score > 0) {
    return { justified: false, reason: `Coincidencia determinista débil (${ranked.matched.join(", ")}) — insuficiente por sí sola para justificar tecnología creativa.`, specializations };
  }
  return null;
}

/** Everything about a proposal that does not depend on *how* the
 *  capability roster was decided — shared, byte-for-byte, by the
 *  deterministic-only `planSolution` and the hybrid `planSolutionAsync`,
 *  so the two paths can never silently drift apart on title/audience/
 *  job/outcome derivation. */
function finishProposal(
  input: PlanSolutionInput,
  combined: string,
  revisionNotes: string[],
  capabilities: IntentOSCapability[],
  routing: RoutingTrace,
  creativeTechnology: CreativeTechVerdict | null,
): SolutionProposal {
  const lower = combined.toLocaleLowerCase("es");
  const titleLine = firstMeaningfulLine(input.intent).replace(/^(crear|construir|desarrollar|necesito)\s+/i, "");
  const title = titleLine.replace(/[.:;].*$/, "").slice(0, 72) || "Nuevo proyecto";
  const solutionForm = lower.includes("iot") || lower.includes("sensor")
    ? "Sistema conectado de extremo a extremo"
    : lower.includes("agente") || lower.includes("inteligencia artificial")
      ? "Sistema inteligente con herramientas y límites verificables"
      : lower.includes("automat") || lower.includes("proceso")
        ? "Sistema de automatización operativa"
        : lower.includes("web") || lower.includes("app") || lower.includes("aplicación") || lower.includes("sistema")
          ? "Producto de software operable"
          : "Solución computacional orientada al resultado";
  const resolvedAudience = input.audience?.trim() || audienceFromRevision(revisionNotes) || audienceFromIntent(lower);
  const resolvedJob = input.requiredFeatures?.trim()
    || (revisionNotes.length ? `Construir la solución incorporando las decisiones NCTO: ${revisionNotes.at(-1)}` : jobFromIntent(lower));
  const experience = [
    resolvedAudience ? `El usuario principal será ${resolvedAudience}.` : "La propuesta validará quién utiliza y decide sobre la solución.",
    revisionNotes.length ? `La revisión NCTO se incorporará explícitamente: ${revisionNotes.at(-1)}` : input.requiredFeatures?.trim() ? `El flujo central cubrirá: ${input.requiredFeatures.trim()}` : "El flujo central se derivará de la intención aprobada.",
    "El resultado se demostrará con una experiencia ejecutable y evidencia, no con funciones simuladas.",
  ];
  const risks = [
    input.constraints?.trim() ? `Restricción declarada: ${input.constraints.trim()}` : "Confirmar límites de alcance antes de producción.",
    "Los datos, integraciones y funciones críticas deberán verificarse en Reality Check.",
  ];
  if (creativeTechnology && !creativeTechnology.justified) {
    risks.push(`Tecnología creativa (3D/WebGL/shaders) evaluada y NO incluida: ${creativeTechnology.reason}`);
  }
  return {
    title,
    projectSlug: projectSlug(title),
    problem: input.intent.trim(),
    user: resolvedAudience,
    job: resolvedJob,
    outcome: input.acceptance?.trim() || outcomeFromIntent(lower),
    solutionForm,
    capabilities,
    pipeline: composePipeline(capabilities, combined),
    experience,
    risks,
    revisionNotes,
    routing,
    creativeTechnology,
  };
}

/** Internal, deterministic proposal projection. It exposes the agency's
 * interpretation for NCTO approval without exposing runbook configuration.
 * Fast path only — see `planSolutionAsync` for the hybrid router that adds
 * a semantic fallback for ambiguous intents. Kept synchronous and
 * unchanged so every existing caller keeps its exact current behaviour. */
export function planSolution(input: PlanSolutionInput): SolutionProposal {
  const revisionNotes = nctoRevisionNotes(input.constraints);
  const combined = combineInputText(input);
  const routed = routeIntent(combined);
  const forcedCapabilities = (input.requiredCapabilityIds ?? [])
    .map((id) => INTENTOS_CAPABILITIES.find((capability) => capability.id === id))
    .filter((capability): capability is IntentOSCapability => Boolean(capability));
  const capabilities = dedupeCapabilities([...forcedCapabilities, ...routed.capabilities]);
  const routing: RoutingTrace = {
    source: "deterministic",
    confidence: routed.confidence,
    reason: routed.matched.length ? `Coincidencia determinista de palabras clave: ${routed.matched.join(", ")}.` : "Sin coincidencias de palabras clave; se usó la capability por defecto.",
  };
  const creativeTechnology = creativeTechVerdictFromRouting(capabilities, routed, combined);
  return finishProposal(input, combined, revisionNotes, capabilities, routing, creativeTechnology);
}

interface SemanticClassification {
  /** Undefined when the model's own `capabilityId` was missing or did not
   *  match `INTENTOS_CAPABILITIES` — see the real-model note below. Kept
   *  independent of `creativeTechnologyJustified` so one hallucinated field
   *  never throws away the other, genuinely useful one. */
  capabilityId?: string;
  confidence: number;
  reason: string;
  creativeTechnologyJustified?: boolean;
  creativeTechnologyReason?: string;
}

function extractJsonObject(text: string): Record<string, unknown> | null {
  const attempt = (candidate: string): Record<string, unknown> | null => {
    try {
      const parsed = JSON.parse(candidate);
      return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : null;
    } catch {
      return null;
    }
  };
  const direct = attempt(text.trim());
  if (direct) return direct;
  const match = text.match(/\{[\s\S]*\}/);
  return match ? attempt(match[0]) : null;
}

/** Semantic fallback — still `local_model_complete` (`local_model.rs`),
 *  not a new inference path, but requested with an explicit
 *  `backend: "deepseek"` override rather than whatever
 *  `INTENTOS_INFERENCE_BACKEND` currently points Esmeralda's own
 *  build/conversation engine at.
 *
 *  This went through two real iterations this session, both against live
 *  engines, not guesses:
 *  1. First tried the local sovereign engine (Qwen2.5-Coder-3B via
 *     Ollama) — probed for real across 5 live classifications, and it was
 *     genuinely unreliable specifically on `creativeTechnologyJustified`:
 *     the same "girar el barco" intent (requirement 7-A) got `true` once
 *     and `false` on a repeat with no change to the input.
 *  2. Then tried Groq (`openai/gpt-oss-120b`) — reliable and fast, but
 *     it's a hosted, closed-weight, non-sovereign backend from a company
 *     other than DeepSeek's, which conflicts with the explicit reason this
 *     project chose a local/open engine in the first place: not paying
 *     Anthropic/OpenAI for one, specific, foreign, closed dependency to
 *     replace it with another. Reverted (see git history) once that
 *     conflict was raised directly.
 *  DeepSeek is the resolution: an open-weight model (DeepSeek-V3, served
 *  via DeepSeek's own hosted API here — genuinely open weights, unlike
 *  Claude/GPT/Gemini/Groq's models, even though this specific call still
 *  leaves the machine over their API) at a real API cost far below Groq's
 *  free-tier constraints, without pulling in Anthropic or OpenAI. It is
 *  NOT sovereign — still an outbound call, still gated by
 *  `network_allowed`/Paranoid Mode exactly like Groq was — but it is the
 *  closest available fit to "free/open, not one of the big incumbents"
 *  for a task (a single small classification call) that has already
 *  proven unreliable on the fully local model. Only ever called when
 *  `isFastPathConfident` says the deterministic router is not enough —
 *  see `resolveCapabilitiesHybrid`. If DeepSeek isn't configured (no
 *  `INTENTOS_DEEPSEEK_API_KEY`, or Paranoid Mode is on) this fails closed
 *  to `null` exactly like any other unavailable engine — see the catch
 *  below; the deterministic router still decides on its own.
 *
 *  Fails closed toward "unavailable", never toward "invent an answer": a
 *  network/model error or a totally non-JSON reply returns `null` outright.
 *  A `capabilityId` outside `INTENTOS_CAPABILITIES` is dropped on its own
 *  (never trusted as the primary capability — the router can never end up
 *  running a capability that does not exist) but does NOT discard the rest
 *  of an otherwise-usable reply — found to matter for real against the
 *  local Qwen probe above, which once returned `"capabilityId":"i3d"` (not
 *  a real id) inside an otherwise well-formed, correctly-reasoned
 *  response. Discarding the whole reply over one bad field would silently
 *  fail requirement 7-A even with a reliable model behind it. */
async function classifyIntentSemantic(text: string): Promise<SemanticClassification | null> {
  const catalog = INTENTOS_CAPABILITIES.map((capability) => `- ${capability.id}: ${capability.description}`).join("\n");
  const prompt = [
    "Eres el clasificador semántico de capacidades internas de IntentOS.",
    "Dada la intención de un producto, elige la capability más adecuada de esta lista cerrada. Nunca inventes un id que no esté en la lista.",
    catalog,
    "",
    "Responde ÚNICAMENTE un objeto JSON (sin texto antes ni después), con esta forma exacta:",
    '{"capabilityId":"<uno de los ids de arriba>","confidence":<numero 0.0-1.0>,"reason":"<una frase>","creativeTechnologyJustified":<true o false>,"creativeTechnologyReason":"<una frase>"}',
    "",
    "creativeTechnologyJustified debe ser true SOLO si el producto requiere, como parte de su función central, al menos una de estas capacidades técnicas concretas: geometría 3D real y navegable (rotar/explorar/recorrer un objeto), renderizado en tiempo real (WebGL/shaders/motion avanzado), captura o visualización espacial, o una interacción que sea literalmente imposible de lograr con imágenes estáticas bien tomadas, CSS y animaciones 2D convencionales.",
    "NO la marques true solo porque el pedido use palabras como \"atractivo\", \"memorable\", \"premium\", \"impactante\", \"profesional\", \"cinematográfico\" o \"que se vea bien\" — esas son metas de diseño normales que cualquier sitio bien hecho ya cumple sin 3D/WebGL. Un catálogo de fotos, un portafolio, una landing page o un sitio de presentación de productos (por ejemplo, mostrar repostería de forma atractiva para conseguir pedidos) NO justifican tecnología creativa solo por querer verse bien o ser memorables; SÍ la justifican si piden explícitamente poder rotar/explorar un objeto en 3D, una visualización que cambie en tiempo real según input del usuario (ej. un configurador de producto), o una experiencia espacial/inmersiva real.",
    "Antes de responder true, pregúntate: ¿este pedido sería literalmente imposible de cumplir con buena fotografía y una landing page normal? Si la respuesta es no (por ejemplo, un dashboard administrativo, un catálogo con fotos bonitas, o un sitio institucional \"impactante\"), responde false.",
    "",
    `INTENCIÓN DEL PRODUCTO:\n${text.slice(0, 4000)}`,
  ].join("\n");
  let content: string;
  try {
    const completion = await invoke<{ content: string }>("local_model_complete", { request: { prompt, maxTokens: 220, backend: "deepseek" } });
    content = completion.content;
  } catch (error) {
    console.warn("[intentosCapabilities] semantic fallback unavailable (DeepSeek not configured, or Paranoid Mode is on):", error);
    return null;
  }
  const parsed = extractJsonObject(content);
  if (!parsed) return null;
  const rawCapabilityId = typeof parsed.capabilityId === "string" ? parsed.capabilityId : undefined;
  const capabilityId = rawCapabilityId && INTENTOS_CAPABILITIES.some((capability) => capability.id === rawCapabilityId) ? rawCapabilityId : undefined;
  if (rawCapabilityId && !capabilityId) {
    console.warn("[intentosCapabilities] semantic fallback returned an unknown capabilityId, ignoring only that field:", rawCapabilityId);
  }
  const creativeTechnologyJustified = typeof parsed.creativeTechnologyJustified === "boolean" ? parsed.creativeTechnologyJustified : undefined;
  // Nothing usable at all (both the primary pick and the creative-tech
  // verdict were absent/invalid) — genuinely equivalent to "unavailable".
  if (!capabilityId && creativeTechnologyJustified === undefined) return null;
  const confidence = typeof parsed.confidence === "number" && Number.isFinite(parsed.confidence) ? Math.max(0, Math.min(1, parsed.confidence)) : 0.5;
  return {
    capabilityId,
    confidence,
    reason: typeof parsed.reason === "string" && parsed.reason.trim() ? parsed.reason.trim() : "Clasificación semántica sin justificación textual.",
    creativeTechnologyJustified,
    creativeTechnologyReason: typeof parsed.creativeTechnologyReason === "string" ? parsed.creativeTechnologyReason : undefined,
  };
}

/** The hybrid router itself: deterministic fast path first, semantic
 *  fallback only when `isFastPathConfident` says the deterministic result
 *  cannot be trusted on its own. `capabilityId`/`confidence`/`reason` are
 *  the required structured shape (requirement #2); `creativeTechnology`
 *  additionally exposes the explicit justify/reject verdict (requirement
 *  #3). Never throws — every failure mode (no ambiguity, semantic
 *  unavailable, semantic invalid) resolves to a usable result. */
async function resolveCapabilitiesHybrid(
  combined: string,
  forcedIds: string[] | undefined,
): Promise<{ capabilities: IntentOSCapability[]; routing: RoutingTrace; creativeTechnology: CreativeTechVerdict | null }> {
  const routed = routeIntent(combined);
  const forcedCapabilities = (forcedIds ?? [])
    .map((id) => INTENTOS_CAPABILITIES.find((capability) => capability.id === id))
    .filter((capability): capability is IntentOSCapability => Boolean(capability));

  if (isFastPathConfident(routed)) {
    const capabilities = dedupeCapabilities([...forcedCapabilities, ...routed.capabilities]);
    const routing: RoutingTrace = {
      source: "deterministic",
      confidence: routed.confidence,
      reason: `Coincidencia determinista de palabras clave: ${routed.matched.join(", ")}.`,
    };
    return { capabilities, routing, creativeTechnology: creativeTechVerdictFromRouting(capabilities, routed, combined) };
  }

  const semantic = await classifyIntentSemantic(combined);
  if (!semantic) {
    const capabilities = dedupeCapabilities([...forcedCapabilities, ...routed.capabilities]);
    const routing: RoutingTrace = {
      source: "semantic-unavailable",
      confidence: routed.confidence,
      reason: "El motor semántico (Esmeralda/Qwen) no estaba disponible; se usó la mejor coincidencia determinista.",
    };
    return { capabilities, routing, creativeTechnology: creativeTechVerdictFromRouting(capabilities, routed, combined) };
  }

  // `semantic.capabilityId` is only ever a validated, known id (or
  // undefined) — see classifyIntentSemantic's own validation — so this
  // never adds a capability that doesn't exist, even on a hallucinated id.
  const semanticCapability = semantic.capabilityId
    ? INTENTOS_CAPABILITIES.find((capability) => capability.id === semantic.capabilityId)
    : undefined;
  // Base roster: the deterministic ranking always contributes at least one
  // capability (routeIntent's own fallback), so this is never empty even
  // when the model's primary pick was unusable.
  let capabilities = dedupeCapabilities([...forcedCapabilities, ...(semanticCapability ? [semanticCapability] : []), ...routed.capabilities]);
  if (semantic.creativeTechnologyJustified === false) {
    capabilities = capabilities.filter((capability) => capability.id !== "creative-technology");
    if (!capabilities.length) capabilities = dedupeCapabilities([...forcedCapabilities, ...(semanticCapability ? [semanticCapability] : [INTENTOS_CAPABILITIES[0]])]);
  } else if (semantic.creativeTechnologyJustified === true && !capabilities.some((capability) => capability.id === "creative-technology")) {
    const creativeTech = INTENTOS_CAPABILITIES.find((capability) => capability.id === "creative-technology")!;
    capabilities = dedupeCapabilities([...capabilities, creativeTech]);
  }
  const routing: RoutingTrace = {
    source: "semantic",
    confidence: semantic.confidence,
    reason: semanticCapability
      ? semantic.reason
      : `${semantic.reason} (el id de capability primaria devuelto por el modelo no es válido; se usó la mejor coincidencia determinista como primaria, conservando el veredicto de tecnología creativa).`,
  };
  return { capabilities, routing, creativeTechnology: creativeTechVerdictFromRouting(capabilities, routed, combined, semantic) };
}

/** Hybrid version of `planSolution`: same deterministic fast path (zero
 *  added latency, zero model calls for an unambiguous intent), falling
 *  back to a real semantic classification — via Esmeralda/Qwen's existing
 *  `local_model_complete` — only when the fast path itself says it is not
 *  confident. Use this from the UI; `planSolution` stays available
 *  unchanged for any synchronous caller (e.g. tests) that specifically
 *  wants the deterministic-only projection. */
export async function planSolutionAsync(input: PlanSolutionInput): Promise<SolutionProposal> {
  const revisionNotes = nctoRevisionNotes(input.constraints);
  const combined = combineInputText(input);
  const { capabilities, routing, creativeTechnology } = await resolveCapabilitiesHybrid(combined, input.requiredCapabilityIds);
  return finishProposal(input, combined, revisionNotes, capabilities, routing, creativeTechnology);
}
