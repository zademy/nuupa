# 3. La versión del release la deriva CI (estilo Spring Boot)

Estado: aceptado (2026-08-27). Supersede la regla "bump en develop antes
del merge" de los ADR-0001/0002.

## Contexto

El guard "el tag v{versión} debe ser nuevo" exigía un bump manual en
develop antes de cada merge a master. Pero un merge agrega muchos commits:
ninguno debería cargar la versión. Spring Boot (spring-projects/spring-boot)
lo resuelve al revés: `main` lleva un placeholder (`version=4.2.0-SNAPSHOT`
en gradle.properties), los PRs jamás tocan la versión, y el proceso de
release es quien estampa la versión real y taguea. En Nuupa la versión debe
estar en los archivos al CONSTRUIR (va dentro de los instaladores: nombres
de archivos, metadatos de msi/exe, versión del deb), así que el estampado
tiene que ocurrir antes del build — y eso también puede hacerlo CI.

## Decisión

- Los merges (y cualquier commit) dejan de cargar bumps. El guard de tag
  nuevo desaparece.
- En master, `sanidad` deriva la versión: último tag `v*` → patch +1; si
  los archivos traen una versión MAYOR, esa gana (intención explícita de
  minor/major: editar `0.4.0` en develop y mergear).
- CI estampa `tauri.conf.json` + `Cargo.toml` + `Cargo.lock`, commitea
  `chore(release): vN [skip ci]` con pie `Release-of: <merge-SHA>` y lo
  pusha a master. tests/build/publicar corren sobre ese SHA estampado y el
  tag lo crea `gh release create --target` apuntando a él: el tag
  CONTIENE su propia versión. El estampado es idempotente: si el merge ya
  traía la versión derivada (la de los archivos ganó), no hay commit de
  release — el merge commit ES el commit de release.
- Anti-bucle y anti-repetición: los pushes hechos con GITHUB_TOKEN no
  disparan workflows (y `[skip ci]` lo refuerza); si un re-run encuentra
  `Release-of: <SHA>` en la historia de master, no estampa ni publica otra
  vez.

## Consecuencias

- Cero pasos manuales: merge a master = release del consecutivo.
- La versión que vive en develop es asesoría (solo gana si es mayor a la
  del último tag): puede quedar rezagada sin romper nada.
- Master lleva commits de release generados por CI, visibles en el
  historial (`chore(release): vN`).
- Un re-run de un run ya publicado vuelve a validar tests/build pero no
  publica una segunda release.
