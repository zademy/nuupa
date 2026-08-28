# 2. La release reconstruye en master

Estado: aceptado (2026-08-27). Supersede la parte de "reutilizar artefactos"
del ADR-0001.

## Contexto

El ADR-0001 evitaba reconstruir en master: la release descargaba los
instaladores del run de develop que construyó el commit fusionado. En la
práctica (release v0.3.0) ese paso resultó frágil de diagnosticar: resolver
el commit de develop, buscar el run por SHA y descargar de otro run añadió
complejidad visible, y la publicación completa (subida secuencial de ~177 MB
al storage de releases) se percibió como un atasco. La ventaja ahorrada
(~10-15 min de build) no compensó la pérdida de previsibilidad del flujo
clásico "construye y publica lo que construiste".

## Decisión

- **master reconstruye**: el job de build corre en develop (validación
  temprana) Y en master; `publicar-release` usa los instaladores de SU
  PROPIO run — sin resolución de commits ajenos ni saltos entre runs.
- Se conservan del ADR-0001: sanidad en master exige tag nuevo (bump en
  develop antes del merge), el tag lo crea CI (`gh release create
--target`), y no hay trigger de tags.
- Desaparece la restricción de "merge normal o fast-forward" (ya no hay
  resolución de artefactos ajenos).

## Consecuencias

- Cada release paga los 5 builds de nuevo (~10-15 min) además de la subida
  de assets — costo aceptado a cambio de un flujo predecible.
- Los binarios publicados son los del SHA de master (idéntico contenido al
  de develop si el merge no trae cambios propios).
