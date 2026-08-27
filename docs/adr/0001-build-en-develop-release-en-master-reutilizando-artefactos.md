# 1. Build en develop, release en master reutilizando los artefactos

Estado: supercedido por el ADR-0002 (2026-08-27) en lo de reutilizar
artefactos; el tag por CI y la sanidad de tag nuevo siguen vigentes.

## Contexto

Nuupa publica releases para 5 plataformas (macOS universal, Linux x64/arm64,
Windows x64/arm64): cada build completo cuesta 5 runners y varios minutos.
El flujo original (push manual de tag `v*` → sanidad → build → release)
construía una vez por release, pero obligaba a crear tags a mano y repetía
el build aunque el mismo commit ya se hubiera construido en la rama de
integración. El proyecto trabaja en `develop` y reserva `master` para lo
que se publica.

## Decisión

- **`develop` construye y valida**: sanidad de versiones, tests (Rust y
  frontend) y los 5 builds por plataforma, que se suben como artefactos
  `nuupa-{label}`.
- **`master` publica sin reconstruir**: al mergear, sanidad exige que el
  tag `v{versión}` sea nuevo (el bump se hace en develop), la release
  descarga los artefactos del run de develop que construyó exactamente el
  commit fusionado (HEAD en fast-forward, `HEAD^2` en merge commit), y
  `gh release create --target` crea tag y release de una vez en el SHA de
  master.
- **Los tags solo los crea CI**: el workflow no se dispara por tags (un
  tag creado por la API generaría un evento push de tag y re-lanzaría el
  pipeline en bucle).

Alternativas descartadas: reconstruir en master (duplica el costo y puede
producir binarios distintos de los validados); renombrar `master` → `main`
(se mantiene `master`).

## Consecuencias

- Un push directo a master sin paso por develop falla ruidosamente (no hay
  run de develop del que descargar artefactos) — correcto: todo cambio
  pasa por develop.
- Los merges a master deben ser merge normal o fast-forward; un
  squash-merge rompe la resolución del commit y el job falla.
- Los binarios publicados son exactamente los que develop construyó y
  validó.
- Liberar exige bump de versión (`tauri.conf.json` + `Cargo.toml`) en
  develop antes del merge; sanidad en master lo rechaza si el tag ya
  existe.
