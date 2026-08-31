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

export function routeIntent(text: string): { capability: IntentOSCapability; capabilities: IntentOSCapability[]; confidence: number; matched: string[] } {
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
  return { capability, capabilities, confidence, matched: best.matched };
}

export function composePipeline(capabilities: IntentOSCapability[]): IntentOSPipelineStage[] {
  const primary = capabilities[0] ?? INTENTOS_CAPABILITIES[0];
  const stages: IntentOSPipelineStage[] = [{ id: "direction", kind: "direction", label: primary.stageLabels[0], agent: primary.agents[0], capabilityId: primary.id }];
  for (const capability of capabilities) stages.push({ id: `${capability.id}:architecture`, kind: "architecture", label: capability.stageLabels[1], agent: capability.agents[1], capabilityId: capability.id });
  for (const capability of capabilities) stages.push({ id: `${capability.id}:development`, kind: "development", label: capability.stageLabels[2], agent: capability.agents[2], capabilityId: capability.id });
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

/** Internal, deterministic proposal projection. It exposes the agency's
 * interpretation for NCTO approval without exposing runbook configuration. */
export function planSolution(input: {
  intent: string;
  audience?: string;
  requiredFeatures?: string;
  constraints?: string;
  acceptance?: string;
  requiredCapabilityIds?: string[];
}): SolutionProposal {
  const revisionNotes = nctoRevisionNotes(input.constraints);
  const combined = [input.intent, input.audience, input.requiredFeatures, input.constraints, input.acceptance].filter(Boolean).join("\n");
  const routed = routeIntent(combined);
  const forcedCapabilities = (input.requiredCapabilityIds ?? [])
    .map((id) => INTENTOS_CAPABILITIES.find((capability) => capability.id === id))
    .filter((capability): capability is IntentOSCapability => Boolean(capability));
  const capabilities = [...forcedCapabilities, ...routed.capabilities]
    .filter((capability, index, all) => all.findIndex((item) => item.id === capability.id) === index)
    .slice(0, 4);
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
  return {
    title,
    projectSlug: projectSlug(title),
    problem: input.intent.trim(),
    user: resolvedAudience,
    job: resolvedJob,
    outcome: input.acceptance?.trim() || outcomeFromIntent(lower),
    solutionForm,
    capabilities,
    pipeline: composePipeline(capabilities),
    experience,
    risks,
    revisionNotes,
  };
}
