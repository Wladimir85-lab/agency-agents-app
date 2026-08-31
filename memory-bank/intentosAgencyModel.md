# IntentOS — mapa operativo de la agencia

**Estado:** inventario verificado, sin ampliar capacidades  
**Fecha del corte:** 2026-08-22  
**Fuente verificable:** `src-tauri/resources/corpus-baseline/` y `src/lib/data/intentosCapabilities.ts`

## 1. Modelo conceptual

IntentOS distingue seis niveles que no deben confundirse:

1. **División:** área amplia del conocimiento disponible en el catálogo.
2. **Profesión:** disciplina reconocible por el usuario (por ejemplo, arquitectura de software).
3. **Especialista:** persona-agente concreta del corpus con instrucciones propias.
4. **Capacidad:** tipo de problema que la agencia sabe ejecutar de extremo a extremo.
5. **Equipo:** cinco especialistas asignados a las cinco etapas de una capacidad.
6. **Workflow:** secuencia verificable que convierte una intención en entregables y evidencia.

Flujo canónico:

`Intención → Capacidad → Equipo de cinco → Workflow → Entregables → Evidencia → Veredicto`

## 2. Inventario profesional incluido

El corpus incluido en este checkout contiene **209 archivos de agentes en 16 divisiones**.
Esta cifra describe la fuente local verificable; no debe sustituirse por cifras de una
fuente remota o histórica sin sincronizar y volver a contar el corpus.

| División del corpus | Especialistas incluidos |
|---|---:|
| Academic | 5 |
| Design | 9 |
| Engineering | 30 |
| Finance | 5 |
| Game Development | 20 |
| Marketing | 36 |
| Paid Media | 7 |
| Product | 5 |
| Project Management | 7 |
| Sales | 9 |
| Security | 10 |
| Spatial Computing | 6 |
| Specialized | 46 |
| Support | 6 |
| Testing | 8 |
| **Total** | **209** |

La carpeta `scripts/` no contiene personas-agente y no se cuenta como división profesional.

## 3. Funciones obligatorias del runtime v0.1

Cada ejecución utiliza exactamente cinco funciones, aunque los especialistas cambien
según la capacidad:

| Etapa | Responsabilidad estable | Gate |
|---|---|---|
| 1. Dirección | Aclarar problema, alcance, plan, riesgos y criterios de éxito | Plan ejecutable |
| 2. Arquitectura | Diseñar la solución y sus decisiones estructurales | Diseño viable |
| 3. Producción | Construir los entregables dentro del proyecto | Implementación real |
| 4. QA | Probar requisitos, funcionamiento y evidencia | PASS/FAIL explícito |
| 5. Reality Check | Contrastar afirmaciones con evidencia y límites | Veredicto final |

Estas son **funciones del workflow**, no cinco profesiones únicas ni agentes permanentes.

## 4. Capacidades actualmente cerradas

| Capacidad | Resultado que produce | Equipo especialista |
|---|---|---|
| Experiencia digital | Webs, aplicaciones, e-commerce e interfaces | Project Manager Senior; UX Architect; Frontend Developer; Evidence Collector; Reality Checker |
| Sistemas y datos | APIs, bases de datos, dashboards y sistemas internos | Project Manager Senior; Software Architect; Backend Architect; API Tester; Reality Checker |
| IoT | Sensores, telemetría, persistencia, alertas, API y dashboard | Project Manager Senior; Software Architect; Rapid Prototyper; Evidence Collector; Reality Checker |
| TinyML / Edge AI | Modelos locales y despliegue embebido | Project Manager Senior; Autonomous Optimization Architect; AI Engineer; Model QA; Reality Checker |
| Ciberseguridad | Auditoría, arquitectura, remediación y pruebas | Project Manager Senior; Security Architect; AppSec Engineer; Penetration Tester; Reality Checker |
| DevOps y calidad | CI/CD, cloud, despliegue, observabilidad y rendimiento | Project Manager Senior; Cloud Security Architect; DevOps Automator; Performance Benchmarker; Reality Checker |
| Tecnología creativa | 3D, WebGL, shaders y experiencias inmersivas | Studio Producer; Visual Storyteller; XR Immersive Developer; Performance Benchmarker; Reality Checker |
| Estrategia y producto | Investigación, MVP, modelo económico y distribución | Project Manager Senior; Trend Researcher; Business Strategist; Experiment Tracker; Reality Checker |

Las ocho capacidades resuelven **24 especialistas únicos**, y los 24 existen en el
corpus incluido. No se requieren perfiles nuevos para mantener el contrato actual.

## 5. Lectura estratégica

- El catálogo de 209 agentes es el **mercado interno de talento** de IntentOS.
- Las ocho capacidades son la **oferta operativa activa**, no un resumen de todo lo que
  el catálogo podría llegar a hacer.
- Una persona del catálogo no se considera operativa solo por existir: necesita una
  capacidad, herramientas, datos, política y una ruta de verificación.
- Las divisiones con muchos perfiles (`Specialized`, `Marketing`, `Engineering`) no
  deben convertirse automáticamente en nuevas capacidades.
- La interfaz debe permitir explorar profesiones sin prometer que todas poseen ya un
  workflow autónomo certificado.

## 6. Deudas verificadas antes de ampliar el sistema

1. La documentación histórica contiene cifras de 210 y 232 agentes; el baseline local
   verificable contiene 209.
2. La sesión web muestra `0 agentes` porque el catálogo activo no está disponible; esto
   impide validar la experiencia real de exploración profesional.
3. Las palabras clave del router son suficientes para v0.1, pero no representan aún una
   taxonomía semántica de profesiones, entregables y límites.
4. Las capacidades describen equipos y etapas, pero todavía no formalizan contratos de
   entrada, entregables y evidencia específicos por profesión.

## 7. Próximo gate

Antes de crear capacidades o profesiones nuevas:

1. restablecer y verificar el catálogo activo en la interfaz;
2. derivar las profesiones visibles desde metadatos reales del corpus;
3. diseñar la navegación `División → Profesión → Especialista`;
4. añadir fichas operativas a las ocho capacidades existentes;
5. validar la comprensión del modelo con ejemplos reales de intención.

## 8. Curaduría profesional aprobada

La curaduría de IntentOS es una capa sobre el catálogo original. `Ocultar`, `mover` o
`fusionar` nunca elimina ni modifica la persona fuente del corpus.

### Investigación y Ciencias Humanas

**Nombre anterior del catálogo:** Academic  
**Estado:** aprobado 2026-08-22

| Especialista | Decisión | Función dentro de IntentOS |
|---|---|---|
| Antropólogo | Mantener | Investigación cultural, usuarios, comunidades y mercados |
| Psicólogo | Mantener | Comportamiento, motivación, UX y producto |
| Estadístico | Mantener | Diseño experimental, análisis de datos y validación de hipótesis |
| Geógrafo | Mover a GIS | Investigación territorial, contexto físico y análisis espacial |
| Narratólogo | Mover a Diseño/Contenido | Narrativa, estructura de historias y comunicación |
| Historiador | Ocultar inicialmente | Disponible para investigación documental y proyectos culturales bajo demanda |

La división visible queda compuesta inicialmente por tres profesiones nucleares:
Antropología, Psicología y Estadística. Geografía y Narratología conservan su valor,
pero aparecen donde el usuario buscaría naturalmente esas competencias.

### Decisión transversal — Visual Storyteller

**Estado:** aprobado 2026-08-22

Visual Storyteller pertenece a Diseño y Experiencia, pero su puesto operativo principal
está integrado en la producción frontend de nuevas experiencias web. Es responsable de
traducir una intención de marca en narrativa visual, composición editorial, ritmo,
jerarquía, dirección de arte y uso deliberado de movimiento o medios mixtos.

Su especialidad incluye la nueva ola de estudios digitales de Japón, Corea y otros
referentes asiáticos: asimetría intencional, tipografía protagonista, maximalismo
controlado, capas, textura, narrativa mediante scroll y microinteracciones con propósito.

No reemplaza al Frontend Developer ni al UX Architect. Trabaja como una tríada:

`UX Architect → Visual Storyteller → Frontend Developer`

- UX Architect preserva comprensión, flujos y accesibilidad.
- Visual Storyteller define la experiencia narrativa y la dirección de arte.
- Frontend Developer convierte esa dirección en una implementación responsive,
  accesible y de buen rendimiento.

WebGL, 3D, shaders o motion se utilizan solo cuando refuerzan el concepto. El objetivo
no es copiar una estética asiática, sino comprender sus principios y producir una
identidad original para cada cliente.

### Negocios, Finanzas y Estrategia

**Nombre anterior del catálogo:** Finance, ampliado con perfiles de Specialized y Support  
**Estado:** aprobado 2026-08-22

Finanzas se integra con Negocios para evaluar una intención como sistema económico, no
solo como presupuesto. El departamento es responsable de oportunidad, modelo de negocio,
pricing, costos, rentabilidad, caja, escenarios y riesgo financiero.

Jerarquía inicial:

- **Residente:** Director de Negocios y Finanzas.
- **Arquitectura económica:** CFO, Business Strategist y FP&A Lead.
- **Especialistas:** Financial Analyst, Pricing Analyst, Tax Strategist e Investment Researcher.
- **Producción administrativa:** Bookkeeper & Controller, Accounts Payable y Finance Tracker.
- **Inspección:** controles financieros, Compliance Auditor y Reality Checker cuando aplique.

El departamento no absorbe las responsabilidades completas de Producto, Marketing o
Ventas. Se relacionan mediante un contrato claro:

`Producto define qué crear → Negocios y Finanzas demuestra viabilidad → Marketing crea demanda → Ventas convierte demanda en ingresos`

Cada propuesta debe explicitar costos, precio, margen, costos recurrentes, condiciones
de pago, escenarios y supuestos antes de presentarse como viable.

#### Fusión con Producto

**Estado:** aprobado 2026-08-22

La división Product deja de operar como departamento independiente y se integra con
Negocios, Finanzas y Estrategia. El departamento resultante se denomina **Estrategia,
Producto y Negocios**.

Su flujo es:

`Investigar problema → comprender usuario → definir producto → validar mercado → diseñar modelo económico → priorizar MVP → decidir si construir`

Composición inicial:

- **Dirección:** Director de Estrategia, Producto y Negocios.
- **Producto:** Product Manager, Trend Researcher, Feedback Synthesizer y Behavioral Design.
- **Negocios:** Business Strategist, Pricing Analyst, Investment Researcher y Experiment Tracker.
- **Finanzas:** CFO, FP&A, Financial Analyst, Tax Strategist y operación financiera.
- **Investigación compartida:** UX Researcher, Antropólogo, Psicólogo y Estadístico.
- **Inspección:** Evidence Collector y Reality Checker.

Sprint Prioritizer se conserva en el corpus, pero la priorización pasa a ser una función
del Product Manager. Behavioral Nudge se mantiene como especialidad transversal con
límites éticos, especialmente para Rehab Tech, educación, videojuegos y onboarding.

### Videojuegos y Experiencias en Tiempo Real

**Nombre anterior del catálogo:** Game Development  
**Estado:** aprobado 2026-08-22

La división se conserva como estudio especializado. Sus perfiles no se limitan a crear
videojuegos: los especialistas compatibles participan también en una célula transversal
de Creative Coding.

Cada especialista conserva una **división base** y puede tener una o más **células de
participación**. Esta doble pertenencia evita duplicar agentes o perder profundidad.

Participan en Creative Coding:

- Technical Artist;
- Game Audio Engineer;
- Blender Add-on Engineer;
- especialistas en shaders de Godot y Unity;
- Unreal Technical Artist y Unreal World Builder;
- Level Designer cuando la experiencia requiera recorrido espacial;
- Narrative Designer cuando exista narrativa interactiva;
- especialistas de motor cuando el prototipo utilice Unity, Unreal, Godot o Roblox.

La célula puede producir WebGL, experiencias 3D, simuladores, visualizaciones,
instalaciones digitales, interfaces inmersivas y XR. Game Designer, Economy Designer,
gameplay y multiplayer permanecen como especialidades principalmente de videojuegos.

La selección sigue el orden:

`Intención → tipo de experiencia → restricciones → tecnología/motor → especialistas`

IntentOS no selecciona primero un motor ni muestra todos los perfiles técnicos al
usuario. Tampoco promete producción completa de videojuegos hasta contar con pipeline,
build reproducible, control de assets y Game QA específico.

### Productos Territoriales y Geografía Digital

**Nombre anterior del catálogo:** GIS  
**Estado:** aprobado 2026-08-22

El departamento construye productos digitales para problemas donde la ubicación, el
territorio o el espacio físico son centrales. Integra geografía, cartografía, datos
espaciales, drones, sensores, aplicaciones, modelos 3D e inteligencia artificial.

Flujo básico:

`Problema territorial → captura de datos → análisis geográfico → mapa/modelo digital → aplicación → decisión o alerta`

Profesiones nucleares:

- analista territorial/geógrafo;
- ingeniero de datos geoespaciales;
- desarrollador de mapas y aplicaciones;
- especialista en captura física mediante drones o sensores;
- inspector GIS.

Se conserva la división completa. Su nicho comercial queda abierto hasta investigar
problemas y usuarios reales. Posibles verticales a validar incluyen ambiente, riesgos,
agricultura, construcción, logística, turismo, infraestructura y planificación urbana.
Geología puede colaborar cuando el problema requiera conocimiento del subsuelo o riesgo
geológico, pero no se confunde con Geografía ni se incorpora sin un perfil competente.

### Salud Digital — Rehab Tech

**Origen del catálogo:** Healthcare, ampliado con perfiles de Specialized  
**Estado:** línea prioritaria de investigación aprobada 2026-08-22

Rehab Tech integra tecnología con procesos de rehabilitación. Puede combinar interfaces
accesibles, seguimiento de ejercicios, sensores, wearables, visión artificial,
gamificación, paneles para profesionales y coordinación entre paciente, familia y equipo
clínico.

Equipo mínimo:

- Healthcare Innovation Strategist;
- Clinical Evidence Agent;
- Sovereign Health Systems Architect;
- profesional humano de rehabilitación correspondiente;
- Product/UX y Accessibility Specialists;
- ingeniería de software, IoT o IA según el producto;
- auditoría de privacidad, seguridad y evidencia.

El software apoya adherencia, medición, educación y coordinación. No diagnostica,
prescribe ni reemplaza la evaluación clínica. Todo producto que influya en decisiones o
resultados clínicos requiere validación profesional, evidencia, consentimiento, límites
de uso y tratamiento seguro de datos sensibles.

### Dirección de Proyectos y Operaciones

**Nombre anterior del catálogo:** Project Management  
**Estado:** área transversal aprobada 2026-08-22

No es una línea comercial independiente. Asigna residentes, coordina cuadrillas,
controla alcance, riesgos, dependencias y entrega, y conecta todas las divisiones de una
obra IntentOS.

Jerarquía:

- **Director de Proyectos y Operaciones:** supervisa la cartera y capacidad de la agencia.
- **Residente:** Senior Project Manager; Studio Producer para obras creativas.
- **Capataz:** Project Shepherd, visible como Delivery Manager.
- **Operación:** Studio Operations y Project Operations Specialist.
- **Asistencia:** meeting notes, documentación y automatización del workflow.

Jira Workflow Steward y Meeting Notes Specialist permanecen en el corpus, pero sus
funciones se integran en Project Operations y Project Coordinator. Experiment Tracker
pertenece a Estrategia, Producto y Negocios.

Relación canónica:

`Mandante humano → Residente → Arquitectos → Líderes/especialistas → Builders → Inspección independiente`

Cada proyecto debe tener un único residente responsable, incluso cuando convoque varias
divisiones. QA y Reality Check no dependen de la cadena constructora para aprobar su
propio trabajo.

### Seguridad, Privacidad y Confianza

**Nombre anterior del catálogo:** Security, ampliado con identidad y privacidad  
**Estado:** área transversal y comercial aprobada 2026-08-22

Protege todas las obras y puede ofrecer auditoría y hardening como servicio. Integra
Security Architecture, Cloud Security, AppSec, Identity & Access, Secrets, Privacy,
SecOps, Threat Detection, Incident Response, Penetration Testing y Compliance.

Su principio profundo no es detectar una entidad llamada IA, sino impedir transiciones
de estado no autorizadas. Controla identidad, datos, canales, permisos, transformaciones
y procedencia. Puede detener procesos, bloquear red, revocar capacidades y poner
artefactos en cuarentena; la eliminación irreversible requiere autorización humana y
preservación previa de evidencia.

Invariante:

> Incluso si un agente es engañado, comprometido o defectuoso, no debe poseer autoridad
> suficiente para causar por sí solo un daño irreversible.

IntentOS adopta un patrón inspirado en sistemas transaccionales bancarios: cada acción
sensible debe ser autorizada, limitada, registrada, reconciliada, verificada y
recuperable. Builder, QA, auditor y aprobador humano conservan separación de funciones.
No se promete que el sistema sea inhackeable ni que un detector pueda identificar con
certeza texto generado por IA.

### Experiencias Espaciales e Inmersivas

**Nombre anterior del catálogo:** Spatial Computing  
**Estado:** célula transversal aprobada 2026-08-22

Spatial Computing deja de operar como departamento independiente y se integra en
Creative Coding y Tecnología Creativa. Conserva XR Interface Architect, XR Immersive
Developer, XR Cockpit Interaction Specialist, visionOS Spatial Engineer y macOS
Spatial/Metal Engineer. Terminal Integration Specialist se reubica en Ingeniería.

La célula participa en Rehab Tech, drones, MiningTech, productos territoriales,
videojuegos, simuladores, gemelos digitales e instalaciones inmersivas. Se activa solo
cuando la comprensión espacial, el entrenamiento, el control remoto, el movimiento
corporal o la inmersión aportan una ventaja verificable frente a una interfaz 2D.

Jerarquía:

- **Dirección:** Director de Tecnología Creativa.
- **Arquitectura:** XR Interface Architect.
- **Especialistas:** Immersive, Cockpit, visionOS y Metal.
- **Inspección:** Accessibility, Performance, especialista sectorial y Reality Check.

### Marketing y Crecimiento

**Nombre anterior del catálogo:** Marketing  
**Estado:** departamento configurado 2026-08-22

Marketing se conserva como departamento comercial responsable de transformar una
propuesta validada en atención, confianza y demanda medible. No decide por sí solo qué
producto construir, no compra medios y no cierra ventas. Su contrato organizacional es:

`Estrategia define mercado y propuesta → Marketing crea demanda → Medios Pagados amplifica → Ventas convierte → Finanzas verifica retorno`

#### Jerarquía operativa

- **Dirección:** Growth Hacker, visible como Director de Marketing y Crecimiento.
- **Arquitectura de campaña:** PR & Communications Manager, Social Media Strategist,
  SEO Specialist y Email Marketing Strategist.
- **Producción:** Content Creator, Multi-Platform Publisher y productores de formatos.
- **Especialistas de distribución:** búsqueda, plataformas sociales, comunidades,
  tiendas de aplicaciones, podcasts y comercio digital.
- **Inspección:** analítica del canal, atribución compartida con Medios Pagados,
  cumplimiento aplicable y Reality Checker.

El equipo base desplegable queda formado por cinco puestos neutrales respecto del canal:

1. Growth Hacker — dirección, hipótesis y métricas;
2. Content Creator — mensaje y activos;
3. SEO Specialist — descubrimiento orgánico;
4. Social Media Strategist — estrategia de distribución social;
5. Email Marketing Strategist — captación propia, nutrición y retención.

Este equipo no equivale todavía a una nueva capacidad del runtime. Es una cuadrilla
desplegable del catálogo y preserva el contrato cerrado de Runtime v0.1.

#### Fusión de los 36 perfiles del corpus

| Función visible | Perfiles fuente | Decisión |
|---|---|---|
| Dirección de Marketing y Crecimiento | Growth Hacker | Mantener como responsable único |
| Comunicación y reputación | PR & Communications Manager | Mantener |
| Estrategia social | Social Media Strategist | Mantener como líder multicanal |
| Contenido y publicación | Content Creator; Multi-Platform Publisher | Mantener como producción central |
| Búsqueda orgánica y descubrimiento por IA | SEO Specialist; AEO Foundations Architect; Agentic Search Optimizer; AI Citation Strategist | Fusionar bajo Search & AI Discovery; conservar especialidades internas |
| Email y audiencias propias | Email Marketing Strategist; Private Domain Operator | Fusionar bajo Lifecycle & Owned Audience |
| Video corto | TikTok Strategist; Short-Video Editing Coach; Video Optimization Specialist | Fusionar estrategia, producción y optimización; activar la plataforma después |
| X/Twitter | Twitter Engager; X/Twitter Intelligence Analyst | Fusionar escucha, inteligencia y operación |
| Podcast | Podcast Strategist; Global Podcast Strategist | Fusionar; Global actúa como especialidad internacional |
| Contenido editorial | Book Co-Author; Carousel Growth Engine; LinkedIn Content Creator | Mantener como formatos activables, no como departamentos |
| Comunidad | Reddit Community Builder | Mantener como especialista de comunidad |
| Instagram | Instagram Curator | Mantener como especialista de canal |
| Distribución móvil | App Store Optimizer | Mantener y compartir con Producto |
| Comercio digital | Cross-Border E-Commerce Specialist; Livestream Commerce Coach | Mantener como célula comercial compartida con Ventas |
| Mercado chino | China Market Localization Strategist; China E-Commerce Operator | Mantener; Localization lidera el ingreso al mercado |
| Plataformas de China | Baidu SEO; Bilibili; Douyin; Kuaishou; WeChat; Weibo; Xiaohongshu; Zhihu | Mantener ocultos por defecto y activar según mercado, audiencia y canal |

No se elimina ningún perfil fuente. La interfaz futura debe mostrar primero la función
visible y permitir desplegar sus especialidades, evitando presentar herramientas o
plataformas como si fueran profesiones jerárquicamente equivalentes.

#### Reglas de activación

1. Ningún canal se selecciona antes de definir usuario, mercado, objetivo y métrica.
2. Los perfiles de China requieren una decisión explícita de entrada a ese mercado y
   revisión cultural, lingüística, regulatoria y operativa.
3. Marketing orgánico y Medios Pagados comparten mensaje y medición, pero mantienen
   responsables y presupuestos separados.
4. Una campaña no pasa a producción sin propuesta, audiencia, conversión esperada,
   instrumentación y criterio de detención.
5. Alcance, impresiones o seguidores no prueban valor comercial por sí solos; el gate
   final exige evidencia conectada con demanda, aprendizaje o ingresos.

### Adquisición y Medios Pagados

**Nombre anterior del catálogo:** Paid Media  
**Estado:** unidad configurada 2026-08-22

Medios Pagados se conserva como unidad especializada, coordinada con Marketing pero con
responsabilidad independiente sobre inversión, instrumentación, optimización y auditoría.
Su propósito no es comprar visibilidad: es adquirir demanda medible dentro de límites
económicos aprobados.

Flujo operativo:

`Propuesta y mensaje → plan de medios → instrumentación → compra → optimización → auditoría → decisión de escalar, corregir o detener`

#### Equipo base desplegable

1. **PPC Campaign Strategist — Residente:** objetivo, canales, presupuesto, pujas y
   coordinación de la campaña.
2. **Ad Creative Strategist — Arquitectura creativa:** hipótesis, formatos, variantes y
   coherencia entre anuncio, audiencia y destino.
3. **Paid Social Strategist — Especialista de distribución:** segmentación y operación
   en plataformas sociales pagadas.
4. **Tracking & Measurement Specialist — Instrumentación:** eventos, conversiones,
   calidad de datos, atribución y reconciliación.
5. **Paid Media Auditor — Inspección independiente:** verifica gasto, configuración,
   resultados, riesgos y afirmaciones antes de escalar.

#### Especialistas activables

- **Search Query Analyst:** se incorpora cuando existe búsqueda pagada; analiza intención,
  términos reales, negativos, desperdicio y oportunidades. Trabaja bajo PPC Strategy.
- **Programmatic & Display Buyer:** se incorpora cuando audiencia, inventario, datos,
  volumen y presupuesto justifican compra programática o display. No se activa por moda.

Los siete perfiles permanecen intactos en el corpus. La cuadrilla base usa cinco para
mantener responsabilidad clara; los otros dos se agregan según el tipo de campaña.

#### Contratos con otras divisiones

- **Marketing** entrega posicionamiento, mensaje, contenido orgánico y activos de marca.
- **Medios Pagados** compra y optimiza distribución; no redefine unilateralmente la marca.
- **Ventas** confirma calidad comercial de leads y resultados posteriores a la conversión.
- **Finanzas** aprueba presupuesto, CAC objetivo, payback y tolerancia de pérdida.
- **Privacidad y Seguridad** revisa consentimiento, datos, píxeles y proveedores.

#### Gates obligatorios

1. No gastar sin objetivo, responsable, presupuesto máximo y criterio de detención.
2. No lanzar sin eventos y conversiones comprobados de extremo a extremo.
3. No escalar basándose solo en métricas de plataforma; reconciliar con datos propios.
4. Separar creación/operación de la auditoría final.
5. Registrar cambios relevantes para distinguir mejora real de fluctuación o atribución.
6. Detener automáticamente una campaña que exceda límites aprobados o pierda trazabilidad.

Medios Pagados es monetizable como servicio gestionado, auditoría y optimización. El
modelo económico debe separar honorarios de agencia e inversión publicitaria del cliente;
IntentOS nunca presenta el presupuesto de medios como ingreso propio.

## 9. Proyección organizacional aplicada al IDE

**Estado:** implementada 2026-08-22  
**Fuente:** `src/lib/data/agencyOrganization.ts`

La interfaz ya no necesita presentar las carpetas del corpus como si fueran la estructura
de la agencia. Cada agente conserva su categoría fuente, mientras una capa local y
reversible determina el departamento IntentOS en el que se descubre y despliega.

La proyección implementa los departamentos aprobados, las fusiones de Finance/Product/
Sales, Project Management/Support y los destinos explícitos de perfiles transversales.
Testing aparece como Calidad e Inspección Independiente; Specialized queda como respaldo
sectorial para los perfiles que todavía no poseen un destino explícito.

Garantías:

- ningún archivo de persona del corpus fue editado o eliminado;
- un agente pertenece a un solo departamento visible principal;
- las selecciones e instalaciones por división utilizan la proyección visible;
- categorías futuras desconocidas permanecen visibles como fallback;
- la proyección no crea capacidades ni modifica el Runtime v0.1.

Verificación técnica: mapeos críticos comprobados, `svelte-check` con cero errores y
cero advertencias, build de producción aprobado y `git diff --check` limpio. La prueba
visual interactiva queda pendiente porque el controlador del navegador integrado no
estuvo disponible en la sesión; no se considera ejecutada por inferencia.

### Tres perspectivas de navegación

La pantalla de Agentes expone tres vistas sobre las mismas personas:

- **Departamentos:** pertenencia profesional principal, sin duplicados.
- **8 ramos:** capacidades multidisciplinarias del bootcamp; una persona puede participar
  en varios ramos y, por ello, los conteos pueden superponerse.
- **Verticales:** mercados de aplicación aprobados inicialmente: Rehab Tech, Tecnología
  territorial y Videojuegos/economías virtuales.

Cambiar de perspectiva no copia, mueve ni instala agentes. Los filtros, conteos y
selección masiva se calculan contra el mismo corpus y la instalación continúa operando
por slug de persona.
