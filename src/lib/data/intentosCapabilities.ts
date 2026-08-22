export interface IntentOSCapability {
  id: string;
  label: string;
  shortLabel: string;
  description: string;
  stageLabels: [string, string, string, string, string];
  agents: [string, string, string, string, string];
  keywords: string[];
}

export const INTENTOS_CAPABILITIES: IntentOSCapability[] = [
  {
    id: "digital-experience",
    label: "Frontend y experiencia digital",
    shortLabel: "Experiencia digital",
    description: "Webs, e-commerce, aplicaciones e interfaces premium.",
    stageLabels: ["Dirección de proyecto", "UX y dirección creativa", "Desarrollo de experiencia", "QA visual y funcional", "Reality Check"],
    agents: ["project-manager-senior", "design-ux-architect", "engineering-frontend-developer", "testing-evidence-collector", "testing-reality-checker"],
    keywords: ["web", "sitio", "landing", "ecommerce", "e-commerce", "tienda", "frontend", "interfaz", "portal", "aplicación móvil", "app móvil"],
  },
  {
    id: "systems-data",
    label: "Sistemas, backend y datos",
    shortLabel: "Sistemas y datos",
    description: "APIs, bases de datos, dashboards, CRM y sistemas internos.",
    stageLabels: ["Dirección de proyecto", "Arquitectura de sistema", "Backend y datos", "QA de API e integración", "Reality Check"],
    agents: ["project-manager-senior", "engineering-software-architect", "engineering-backend-architect", "testing-api-tester", "testing-reality-checker"],
    keywords: ["backend", "api", "base de datos", "database", "dashboard", "crm", "inventario", "sistema interno", "panel administrativo", "datos"],
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

export function routeIntent(text: string): { capability: IntentOSCapability; confidence: number; matched: string[] } {
  const normalized = text.toLocaleLowerCase("es");
  const ranked = INTENTOS_CAPABILITIES.map((capability) => {
    const matched = capability.keywords.filter((keyword) => normalized.includes(keyword));
    return { capability, matched, score: matched.reduce((sum, keyword) => sum + (keyword.includes(" ") ? 2 : 1), 0) };
  }).sort((a, b) => b.score - a.score);
  const best = ranked[0];
  const confidence = best.score === 0 ? 0 : Math.min(0.95, 0.45 + best.score * 0.1);
  return { capability: best.score ? best.capability : INTENTOS_CAPABILITIES[0], confidence, matched: best.matched };
}
