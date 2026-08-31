import type { Agent } from "$lib/types";

export interface AgencyDepartment {
  slug: string;
  label: string;
  icon: string;
  color: string;
  sourceCategories: string[];
}

export interface AgencyLens {
  slug: string;
  label: string;
  description: string;
  icon: string;
  color: string;
  departments: string[];
}

/**
 * IntentOS' visible organization. Source categories remain untouched; this
 * projection decides where a person is discovered in the agency workspace.
 */
export const AGENCY_DEPARTMENTS: AgencyDepartment[] = [
  { slug: "research", label: "Investigación y Ciencias Humanas", icon: "BookOpen", color: "#8B5CF6", sourceCategories: ["academic"] },
  { slug: "design-experience", label: "Diseño y Experiencia", icon: "Palette", color: "#EC4899", sourceCategories: ["design"] },
  { slug: "engineering-systems", label: "Ingeniería y Sistemas", icon: "Code", color: "#3B82F6", sourceCategories: ["engineering"] },
  { slug: "strategy-business", label: "Estrategia, Producto y Negocios", icon: "BriefcaseBusiness", color: "#F59E0B", sourceCategories: ["finance", "product", "sales"] },
  { slug: "marketing-growth", label: "Marketing y Crecimiento", icon: "Megaphone", color: "#F43F5E", sourceCategories: ["marketing"] },
  { slug: "paid-acquisition", label: "Adquisición y Medios Pagados", icon: "BadgeDollarSign", color: "#A855F7", sourceCategories: ["paid-media"] },
  { slug: "games-realtime", label: "Videojuegos y Tiempo Real", icon: "Gamepad2", color: "#22C55E", sourceCategories: ["game-development"] },
  { slug: "territorial", label: "Productos Territoriales y Geografía Digital", icon: "Map", color: "#14B8A6", sourceCategories: ["gis"] },
  { slug: "health-rehab", label: "Salud Digital y Rehab Tech", icon: "HeartPulse", color: "#EF4444", sourceCategories: ["healthcare", "health"] },
  { slug: "projects-operations", label: "Dirección de Proyectos y Operaciones", icon: "ClipboardList", color: "#64748B", sourceCategories: ["project-management", "support"] },
  { slug: "security-trust", label: "Seguridad, Privacidad y Confianza", icon: "ShieldCheck", color: "#DC2626", sourceCategories: ["security"] },
  { slug: "creative-immersive", label: "Tecnología Creativa e Inmersiva", icon: "Glasses", color: "#06B6D4", sourceCategories: ["spatial-computing"] },
  { slug: "quality", label: "Calidad e Inspección Independiente", icon: "BadgeCheck", color: "#10B981", sourceCategories: ["testing"] },
  { slug: "sector-specialists", label: "Especialistas Sectoriales", icon: "Shapes", color: "#78716C", sourceCategories: ["specialized"] },
];

export const AGENCY_RAMAS: AgencyLens[] = [
  { slug: "ramo-digital-experience", label: "Frontend y experiencia digital", description: "Webs, aplicaciones, e-commerce e interfaces.", icon: "PanelsTopLeft", color: "#8B5CF6", departments: ["design-experience", "engineering-systems", "quality", "projects-operations"] },
  { slug: "ramo-systems-data", label: "Sistemas, backend y datos", description: "APIs, bases de datos, dashboards y sistemas internos.", icon: "Database", color: "#3B82F6", departments: ["engineering-systems", "security-trust", "quality", "projects-operations"] },
  { slug: "ramo-iot", label: "IoT y sistemas conectados", description: "Sensores, telemetría, alertas y automatización física.", icon: "RadioTower", color: "#14B8A6", departments: ["engineering-systems", "territorial", "security-trust", "quality"] },
  { slug: "ramo-tinyml", label: "TinyML y Edge AI", description: "Modelos locales desplegados en dispositivos.", icon: "Cpu", color: "#06B6D4", departments: ["engineering-systems", "health-rehab", "territorial", "quality"] },
  { slug: "ramo-cybersecurity", label: "Ciberseguridad", description: "Arquitectura, auditoría, identidad, privacidad y hardening.", icon: "ShieldCheck", color: "#DC2626", departments: ["security-trust", "engineering-systems", "quality"] },
  { slug: "ramo-devops-quality", label: "DevOps, cloud y calidad", description: "CI/CD, despliegue, observabilidad, rendimiento y operación.", icon: "CloudCog", color: "#64748B", departments: ["engineering-systems", "security-trust", "quality", "projects-operations"] },
  { slug: "ramo-creative-technology", label: "Tecnología creativa", description: "WebGL, 3D, shaders, creative coding y experiencias inmersivas.", icon: "Sparkles", color: "#EC4899", departments: ["creative-immersive", "design-experience", "games-realtime", "engineering-systems", "quality"] },
  { slug: "ramo-strategy-product", label: "Estrategia de producto y negocio", description: "Investigación, MVP, modelo económico, distribución e ingresos.", icon: "Target", color: "#F59E0B", departments: ["strategy-business", "research", "marketing-growth", "paid-acquisition", "projects-operations"] },
];

export const AGENCY_VERTICALS: AgencyLens[] = [
  { slug: "vertical-rehab-tech", label: "Rehab Tech", description: "Tecnología para rehabilitación, adherencia, medición y coordinación clínica.", icon: "HeartPulse", color: "#EF4444", departments: ["health-rehab", "design-experience", "engineering-systems", "security-trust", "quality"] },
  { slug: "vertical-territorial", label: "Tecnología territorial", description: "Productos para territorio, ambiente, minería, drones y geografía digital.", icon: "Map", color: "#14B8A6", departments: ["territorial", "engineering-systems", "creative-immersive", "quality"] },
  { slug: "vertical-games", label: "Videojuegos y economías virtuales", description: "Experiencias en tiempo real, Roblox, simulación y monetización responsable.", icon: "Gamepad2", color: "#22C55E", departments: ["games-realtime", "creative-immersive", "design-experience", "marketing-growth", "strategy-business", "quality"] },
];

export const AGENCY_LENSES: AgencyLens[] = [...AGENCY_RAMAS, ...AGENCY_VERTICALS];

const explicitAssignments: Record<string, string> = {
  "academic-geographer": "territorial",
  "academic-narratologist": "design-experience",
  "support-finance-tracker": "strategy-business",
  "support-infrastructure-maintainer": "engineering-systems",
  "support-legal-compliance-checker": "security-trust",
  "accounts-payable-agent": "strategy-business",
  "business-strategist": "strategy-business",
  "loan-officer-assistant": "strategy-business",
  "specialized-pricing-analyst": "strategy-business",
  "healthcare-customer-service": "health-rehab",
  "healthcare-marketing-compliance": "health-rehab",
  "medical-billing-coding-specialist": "health-rehab",
  "agentic-identity-trust": "security-trust",
  "automation-governance-architect": "security-trust",
  "identity-graph-operator": "security-trust",
  "zk-steward": "security-trust",
  "lsp-index-engineer": "engineering-systems",
  "specialized-mcp-builder": "engineering-systems",
  "specialized-salesforce-architect": "engineering-systems",
  "specialized-workflow-architect": "engineering-systems",
  "specialized-civil-engineer": "territorial",
  "specialized-cultural-intelligence-strategist": "research",
  "customer-service": "projects-operations",
  "customer-success-manager": "projects-operations",
  "specialized-chief-of-staff": "projects-operations",
  "sales-data-extraction-agent": "strategy-business",
  "sales-outreach": "strategy-business",
  "government-digital-presales-consultant": "strategy-business",
};

const bySourceCategory = new Map(
  AGENCY_DEPARTMENTS.flatMap((department) =>
    department.sourceCategories.map((category) => [category, department.slug] as const),
  ),
);

export function agencyDepartment(slug: string): AgencyDepartment | undefined {
  return AGENCY_DEPARTMENTS.find((department) => department.slug === slug);
}

export function agencyLens(slug: string): AgencyLens | undefined {
  return AGENCY_LENSES.find((lens) => lens.slug === slug);
}

export function departmentSlugForAgent(agent: Pick<Agent, "slug" | "category">): string {
  return explicitAssignments[agent.slug] ?? bySourceCategory.get(agent.category) ?? agent.category;
}

export function agentMatchesOrganization(agent: Pick<Agent, "slug" | "category">, slug: string): boolean {
  const department = departmentSlugForAgent(agent);
  const lens = agencyLens(slug);
  if (lens) return lens.departments.includes(department);
  if (agencyDepartment(slug)) return department === slug;
  // Backward compatibility for navigation state and deep links created before
  // the IntentOS projection existed. Source-category URLs must not go blank.
  return agent.category === slug || department === slug;
}
