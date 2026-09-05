# Corpus Canónico IntentOS

Esta biblioteca aporta contexto técnico verificable a los agentes. No dicta una
solución única: fundamenta su juicio y mantiene trazabilidad entre fuente,
inferencia, decisión y prueba.

## Política de uso

1. Recuperar únicamente fuentes pertinentes a la intención y al tipo de producto.
2. Diferenciar hechos de la fuente, inferencias del agente y decisiones del proyecto.
3. Citar documento, versión y sección utilizados.
4. No afirmar cumplimiento por haber consultado una fuente: demostrarlo mediante
   pruebas o evidencia del producto real.
5. Conservar la versión consultada y advertir cuando pueda estar desactualizada.
6. No incorporar copias no autorizadas de libros comerciales.

## Ciclo cognitivo

`recuperar -> razonar -> cuestionar -> verificar -> recordar`

## Contenido inicial

| Área | Fuente | Uso dentro de IntentOS |
|---|---|---|
| Ingeniería de software | SWEBOK v4.0a | Índice profesional principal: requisitos, arquitectura, construcción, pruebas, operaciones, seguridad, calidad y gestión. |
| Ciencias de la computación | CS2023 | Fundamentos que se esperan de una formación moderna en computación. |
| Seguridad de aplicaciones | OWASP ASVS v5.0.0 | Requisitos identificables que pueden convertirse en controles y gates de seguridad. |

El archivo `sources.yml` registra procedencia, versión, licencia o condiciones de
uso, hash y estado de incorporación. Los documentos descargados son fuentes
primarias; todavía no están conectados automáticamente al recuperador del runtime.

## Síntesis originales (`synthesis/`)

A diferencia de `canonical/` (PDF de fuente primaria) y `catalog/` (bibliografía
sin contenido), `synthesis/` guarda texto propio — interpretaciones en palabras
originales de conceptos de una obra, no citas — ya troceado en unidades listas
para vectorizar.

- `mental-models-cognitive-science.md` — ocho síntesis de modelos mentales y
  ciencia cognitiva con su aplicación directa a la arquitectura de agentes de
  IntentOS (ver `sources.yml`, `sintesis-modelos-mentales-cognitiva-v1`).
- `esmeralda-operational-manual.md` — Manual Operativo Esmeralda: nueve
  capítulos que cruzan auto-referencia, sesgos cognitivos, aprendizaje de
  fallos, arquitectura multiagente y meta-cognición en un protocolo de
  decisión unificado específico para el motor de Esmeralda (ver `sources.yml`,
  `manual-operativo-esmeralda-v1`).

## Próximas capas

- SEBoK para ingeniería de sistemas y ciclo de vida completo.
- NIST Secure Software Development Framework.
- WCAG y especificaciones W3C para accesibilidad e interfaces web.
- Catálogo bibliográfico de textos universitarios y profesionales, sin almacenar
  copias comerciales no autorizadas.
- Extracción estructurada en principio, condición, alternativas, evidencia y
  límites de aplicación.

