# Plan — brew: identidad completa y objetivo de actualización

**Spec:** [#48 — brew: fórmulas de tap en la tabla y Actualizar todo sobre lo que brew declara desactualizado](https://github.com/zademy/nuupa/issues/48)
**Objetivo:** que todo lo que brew puede actualizar se pueda ver y actualizar desde Nuupa: las fórmulas de un tap de terceros dejan de desaparecer de la tabla, y las dependencias desactualizadas (que no son filas) entran en el Actualizar todo.
**Stack:** Rust (backend Tauri v2) + Vue 3 (frontend) + Vitest.

## Contexto para quien implemente

Dos bugs distintos, ambos medidos en una máquina real con Homebrew 7.0.11:

1. **La identidad de una fila.** Homebrew nombra las fórmulas según la fuente: `brew leaves` y `brew outdated --json=v2` usan el nombre **con tap** (`gentleman-programming/tap/engram`), pero `brew list --formula --versions` usa el nombre **corto** (`engram`). La tabla se armaba desde `list --versions` y se filtraba contra el set de hojas (`hojas.contains(&nombre)`) → toda fórmula de un tap de terceros se descartaba. Con la identidad correcta (el nombre con tap), la fila aparece y su línea de actualización (`brew upgrade --formula gentleman-programming/tap/engram`) es la que brew entiende.
2. **El objetivo del Actualizar todo.** `brew upgrade --formula node` actualiza **solo** node (Homebrew: _"upgrade only the given formula"_), y la cola de Nuupa se armaba con las filas de la tabla → una dependencia desactualizada (`simdjson` 5.0.7 → 5.1.0) quedaba varada: no sale en `brew leaves` y nadie la arrastra.

**Decisiones ya tomadas (no las reabra):**

- El Espacio global de brew **sigue siendo el de brew**: `brew leaves` + todos los casks. Las dependencias **no** son filas.
- La identidad de una fila es el **nombre completo** de brew. En core, el nombre completo _es_ el corto, así que no cambia el aspecto de nada.
- El **objetivo de actualización** (lo que el gestor declara actualizable) es una lista propia que cruza el IPC. npm/pnpm/bun la llenan con sus filas desactualizadas; brew con todo lo que `brew outdated` declara. El botón, el contador y la cola leen **esa misma lista**, así que no pueden discrepar entre sí.
- La frescura no se toca: `brew outdated` y `brew upgrade` ya disparan el auto-update de Homebrew. Nuupa **no** llama a `brew update`.
- Los casks se identifican por **token** (`font-jetbrains-mono`): brew no expone nombre completo para casks.
- Una hoja cuya versión instalada no se puede resolver **no se descarta**: aparece con la celda vacía.

**Vocabulario (GLOSSARY.md, obligatorio):** Gestor, Espacio global, Fórmula hoja, Cask, Desactualizado, Excluido, Pineada, Objetivo de actualización.

## Comandos

```sh
# Frontend
npm test                      # vitest
npm run format:check          # prettier
npm run lint                  # eslint

# Backend
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# La app (verificación manual contra el brew real)
cargo tauri dev               # levanta vite (beforeDevCommand) + la app
```

CI exige, en este orden: `npm run format:check` → `npm run lint` → `npm test` → `cargo fmt --check` → `cargo clippy -- -D warnings` → `cargo test`.

## Mapa de archivos

| Archivo                                        | Qué cambia                                                             |
| ---------------------------------------------- | ---------------------------------------------------------------------- |
| `GLOSSARY.md`                                  | "Fórmula hoja" según brew; entra "Objetivo de actualización"           |
| `src-tauri/src/kernel.rs`                      | Tipo nuevo `PlanGestor`; `Snapshot` lo envuelve; `con_comando` se muda |
| `src-tauri/src/lib.rs`                         | `DefinicionGestor.snapshot` devuelve `PlanGestor`; `correr_snapshot`   |
| `src-tauri/src/cola.rs`                        | La cola se arma con el objetivo, no con las filas                      |
| `src-tauri/src/npm.rs`, `pnpm.rs`, `bun.rs`    | Devuelven `PlanGestor` (objetivo = filas desactualizadas)              |
| `src-tauri/src/brew.rs`                        | Identidad con el nombre completo; el objetivo de brew                  |
| `src/store.js`                                 | El botón y el contador leen `snapshot.objetivo`                        |
| `src/store.test.js`, `src/PanelGestor.test.js` | Los snapshots falsos llevan `objetivo`                                 |

## Tickets

La spec es #48. Cada tarea es un ticket hijo (rama `ticket/<n>-<slug>`):

| Task                                                                  | Ticket                                           | Depende de |
| --------------------------------------------------------------------- | ------------------------------------------------ | ---------- |
| Task 1 — El vocabulario (GLOSSARY.md)                                 | [#49](https://github.com/zademy/nuupa/issues/49) | —          |
| Task 2 — brew: la identidad de una fila es el nombre completo         | [#50](https://github.com/zademy/nuupa/issues/50) | —          |
| Task 3 — El objetivo de actualización cruza el IPC                    | [#51](https://github.com/zademy/nuupa/issues/51) | #50        |
| Task 4 — brew: el objetivo es todo lo que brew declara desactualizado | [#52](https://github.com/zademy/nuupa/issues/52) | #51        |
| Task 5 — Verificación contra el brew real                             | (sin ticket: es la comprobación final)           | #49–#52    |

---

## Task 1 — El vocabulario (GLOSSARY.md)

**Por qué primero:** las tareas siguientes nombran cosas que el GLOSSARY tiene que definir ya.

**Qué cambia** (`GLOSSARY.md`):

1. **"Fórmula hoja"** — la definición actual ("Fórmula que el usuario instaló a propósito (no llegó como dependencia de otra)") no es la de brew, que rechaza toda fórmula que sea dependencia de runtime de otra fórmula **o cask** instalada _antes_ de mirar si se instaló a propósito. La nueva definición:

   > **Fórmula hoja**: Fórmula que brew lista como hoja: no es dependencia de runtime de otra fórmula ni de un cask instalado. Es lo que brew llama _leaf_.

   Añade una nota de que la definición de brew y la de "instalada a propósito" no coinciden (p. ej. `cairo` y `pango`: instaladas a propósito y a la vez dependencias → no son hojas).

2. **"Objetivo de actualización"** (entrada nueva):

   > **Objetivo de actualización**: Los paquetes que un gestor declara actualizables ahora mismo. Para los gestores de node es el subconjunto desactualizado del Espacio global; para brew es todo lo que `brew outdated` declara, dependencias incluidas (las dependencias no son filas, pero sí se pueden actualizar). El botón de Actualizar todo, el contador y la cola leen el objetivo: nunca pueden discrepar entre sí.
   > _Avoid_: lista de actualización, pendientes, desactualizables

**Verificación:** `grep -n "Fórmula hoja\|Objetivo de actualización" GLOSSARY.md` — las dos entradas existen y las dos evitan los sinónimos de la lista.

**Commit:** `docs: GLOSSARY — Fórmula hoja según brew y Objetivo de actualización (#48)`

---

## Task 2 — brew: la identidad de una fila es el nombre completo

**Por qué aquí:** es el bug visible, se arregla entero en `src-tauri/src/brew.rs` y no toca el IPC (así que se puede revisar y mergear solo).

**Test primero.** En `src-tauri/src/brew.rs`, módulo `tests`:Captura de Homebrew 7.0.11 real (2026-10-31):

```rust
const LEAVES_TAP: &str = "gentleman-programming/tap/engram\nwget\n";
const FORMULAE_TAP: &str = "engram 3.6.0\nwget 1.25.0\n";
const OUTDATED_TAP: &str = r#"{
    "formulae": [
        {"name":"gentleman-programming/tap/engram","installed_versions":["3.6.0"],"current_version":"3.7.0","pinned":false,"pinned_version":null}
    ],
    "casks": []
}"#;
```

Tests nuevos (nombre exacto):

- `una_hoja_de_tap_aparece_con_su_nombre_completo` — con `LEAVES_TAP` + `FORMULAE_TAP` + `OUTDATED_TAP`, la fila de `engram` se llama `gentleman-programming/tap/engram`, trae `installed == "3.6.0"`, `latest == "3.7.0"` y `outdated == true`.
- `la_hoja_de_tap_encuentra_su_version_por_el_nombre_corto` — la fila existe aunque `brew list --versions` traiga `engram` (corto) y la hoja traiga `gentleman-programming/tap/engram` (largo): la versión instalada no se pierde.
- `una_hoja_sin_version_resoluble_aparece_con_la_celda_vacia` — una hoja cuyo nombre corto no está en `brew list --versions` produce fila con `installed == ""` (no se descarta).
- `las_lineas_de_brew_llevan_el_nombre_completo` — en `src-tauri/src/lib.rs`: `args_brew("gentleman-programming/tap/engram", Some("formula"))` devuelve `["upgrade", "--formula", "gentleman-programming/tap/engram"]`, y lo mismo `args_uninstall_brew` (`uninstall --formula`) y `args_unpin_brew` (`unpin`).
- Renombrar `parse_versions_unirne_versiones_multiples` → `parse_versions_une_las_versiones_multiples` (el tipo de retorno cambia).

**Qué cambia** (`src-tauri/src/brew.rs`):

1. `parse_versions` pasa a devolver un mapa (el nombre corto → las versiones instaladas), para no escanear la lista por cada hoja:

```rust
/// Parses `brew list --versions` (and `--cask --versions`): `name v1
/// [v2 …]` per line. Multiple installed versions join with ", ". The key
/// is the SHORT name (`engram`); the rows are identified by the full one
/// (`gentleman-programming/tap/engram`) — `nombre_corto` bridges both.
fn parse_versions(stdout: &str) -> BTreeMap<String, String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let mut partes = l.split_whitespace();
            let nombre = partes.next()?;
            Some((nombre.to_string(), partes.collect::<Vec<_>>().join(", ")))
        })
        .collect()
}
```

2. Helper nuevo:

```rust
/// The short name of a brew formula: what comes after the tap prefix.
/// `gentleman-programming/tap/engram` → `engram`. Core formulae have no
/// prefix, so the short name is the full name.
fn nombre_corto(nombre: &str) -> &str {
    nombre.rsplit('/').next().unwrap_or(nombre)
}
```

3. `armar_brew`: las hojas se iteran (ya vienen con el nombre completo) y su versión se busca por el nombre corto. La fila **muestra y usa** el nombre completo; la clave del mapa de desactualizados también (`brew outdated` indexa las fórmulas por nombre completo):

```rust
fn armar_brew(
    version_gestor: String,
    hojas: BTreeSet<String>,
    versiones: BTreeMap<String, String>,
    casks: BTreeMap<String, String>,
    outdated: &BTreeMap<(String, String), Desactualizado>,
) -> EspacioGlobal {
    let mut packages = Vec::new();
    // Only leaves: dependencies update with their parent, they are not
    // the user's packages (they never reach a table).
    for hoja in hojas {
        // brew's leaves print the FULL name (`tap/name`); `brew list
        // --versions` prints the SHORT one. The row keeps the full
        // name — it is the identity brew uses in `outdated` too.
        let installed = versiones.get(nombre_corto(&hoja)).cloned().unwrap_or_default();
        packages.push(paquete(TIPO_FORMULA, hoja, installed, outdated));
    }
    for (token, installed) in casks {
        packages.push(paquete(TIPO_CASK, token, installed, outdated));
    }
    // …el sort por (nombre, tipo_orden) no cambia
}
```

4. `snapshot()`: los cuatro comandos de brew **no cambian** (`outdated --json=v2`, `leaves`, `list --versions`, `list --cask --versions`) — una sola pasada.

5. `paquete()` no cambia: ya busca `outdated.get(&(tipo, nombre))` y, si no está, la fila queda "al día" con la versión instalada.

**Lo que NO cambia:** `args_brew`, `args_uninstall_brew`, `args_unpin_brew` (en `lib.rs`) — reciben el nombre de la fila, que ahora es el completo, y brew lo entiende (verificado: `brew upgrade --dry-run --formula gentleman-programming/tap/engram`, `brew unpin gentleman-programming/tap/engram` sale con 0, y `brew uninstall --formula tap/no-existe` da _"No available formula with the name"_, o sea que la forma parsea).

**Comandos:**

```sh
cargo test --manifest-path src-tauri/Cargo.toml brew
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

**Commit:** `fix: brew — la fila se identifica con el nombre completo de brew (#48)`

---

## Task 3 — El objetivo de actualización cruza el IPC

**Por qué aquí:** es la costura que hace posible el objetivo de brew. Es un **refactor sin cambio de comportamiento**: npm/pnpm/bun llenan el objetivo con sus filas desactualizadas, o sea exactamente lo que la cola y el botón usan hoy.

**Test primero.**

- `src-tauri/src/kernel.rs`: test nuevo `el_objetivo_de_actualizacion_cruza_el_ipc` — serializar un `Snapshot` con `serde_json::to_value` y comprobar que el JSON trae `objetivo` con los paquetes que lleva (y que `packages` sigue ahí).
- `src-tauri/src/cola.rs`: test nuevo `la_cola_se_arma_con_el_objetivo_y_no_con_las_filas` — un `PlanGestor` cuyo `espacio.packages` está al día y cuyo `objetivo` trae un paquete desactualizado → la cola actualiza ese paquete (el `FakeRunner` registra la línea ejecutada).
- `src-tauri/src/cola.rs`: test nuevo `una_excluida_con_nombre_de_tap_no_entra_en_la_cola` — el archivo de Excluidas guarda el nombre **completo** (`gentleman-programming/tap/engram`), que es el mismo string que muestra la fila; el paquete no se actualiza.
- `src/store.test.js`: test nuevo `el_objetivo_es_lo_que_actualiza_todo_toma` — el store cuenta y habilita el botón a partir de `snapshot.objetivo`, no de `packages`.
- `src/PanelGestor.test.js`: el botón de "Actualizar todo" se habilita con el objetivo aunque la fila no exista.

**Qué cambia:**

1. `src-tauri/src/kernel.rs` — tipo nuevo y el `Snapshot` lo envuelve:

```rust
/// A manager's plan: its global space AND the update objective — the
/// packages it declares updatable. For npm/pnpm/bun the objective is
/// the outdated rows; for brew it is everything `brew outdated`
/// declares (dependencies included — they are not rows). The queue,
/// the button and the count all read the objective, so they can never
/// disagree with each other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanGestor {
    #[serde(flatten)]
    pub espacio: EspacioGlobal,
    pub objetivo: Vec<GlobalPackage>,
}

impl PlanGestor {
    /// The plan of a manager whose updatable set IS its outdated rows
    /// (npm, pnpm, bun). Brew has its own: everything `brew outdated`
    /// declares, dependencies included.
    pub fn de_filas(espacio: EspacioGlobal) -> Self {
        let objetivo = espacio.packages.iter().filter(|p| p.outdated).cloned().collect();
        PlanGestor { espacio, objetivo }
    }
}
```

```rust
pub struct Snapshot {
    pub comando_actualizar: String,
    pub comando_desinstalar: Option<String>,
    #[serde(flatten)]
    pub plan: PlanGestor,
}
```

`con_comando` se muda de `impl EspacioGlobal` a `impl PlanGestor` (mismo cuerpo, `plan: self`). El JSON que cruza el IPC queda: `comando_actualizar`, `comando_desinstalar`, `version_gestor`, `version_node`, `packages`, `objetivo`.

Con `Snapshot` envolviendo el plan, el campo `espacio` se renombra a `plan`: hay que tocar los cinco usos de `snap.espacio` — dos asserts en `src-tauri/src/cola.rs`, dos en `src-tauri/src/lib.rs` y el test de `kernel.rs` — que pasan a ser `snap.plan.espacio`.

2. `src-tauri/src/lib.rs`:

```rust
pub(crate) snapshot: fn(&dyn Runner) -> std::io::Result<kernel::PlanGestor>,
```

```rust
fn correr_snapshot(def: &DefinicionGestor) -> Result<Snapshot, String> {
    let runner = (def.runner)().map_err(|e| e.to_string())?;
    let plan = (def.snapshot)(runner.as_ref()).map_err(|e| e.to_string())?;
    Ok(plan.con_comando(def.comando, def.comando_desinstalar))
}
```

3. `src-tauri/src/cola.rs` — la cola se arma con el objetivo (que ya solo contiene lo desactualizado):

```rust
let snap0 = (def.snapshot)(runner.as_ref()).map_err(|e| e.to_string())?;
let pendientes: Vec<GlobalPackage> = snap0
    .objetivo
    .iter()
    .filter(|p| !p.pinned && !excluidos_de(dir_config, def.nombre).contains(&p.name))
    .cloned()
    .collect();
```

(El filtro `p.outdated` desaparece: el objetivo solo trae lo desactualizado. Los pines y las exclusiones se filtran igual que hoy.)

Los helpers de test que fabrican un gestor — `def_brew_con` en `lib.rs` y `cola.rs`, y el `def_con` equivalente — pasan a devolver `PlanGestor` (con `objetivo` incluido), porque `DefinicionGestor.snapshot` cambia de tipo.

4. `src-tauri/src/npm.rs`, `pnpm.rs`, `bun.rs` — el objetivo son las filas desactualizadas. El bloque vive **una vez** en `PlanGestor::de_filas` (kernel), no repetido en los tres adaptadores:

```rust
Ok(PlanGestor::de_filas(EspacioGlobal {
    version_gestor,
    version_node,
    packages,
}))
```

5. `src/store.js` — el botón y el contador leen el objetivo:

```js
// The update objective: what the manager declares updatable. The
// queue (backend), the button and the count all read the SAME list —
// they can never disagree.
const objetivo = computed(() => state.snapshot?.objetivo ?? []);

const desactualizables = computed(() =>
  objetivo.value.filter((p) => !isExcluded(p.name)),
);

const hayDesactualizados = computed(() => desactualizables.value.length > 0);

const conteo = computed(() => {
  const all = state.snapshot?.packages ?? [];
  return {
    total: all.length,
    desactualizados: objetivo.value.length,
    excluidos: excluded.value.length,
  };
});
```

6. `GLOSSARY.md` — la entrada **Cola** dice hoy que actualiza «los paquetes desactualizados no excluidos»; con el objetivo, la cola actualiza **el objetivo menos Pineadas y menos Excluidas**. Esa frase se corrige en esta task para que el glosario no contradiga el código nuevo. (El objetivo **sí** incluye las Pineadas — brew las declara desactualizadas — pero la cola nunca las actualiza.)

7. `src/store.test.js` y `src/PanelGestor.test.js` — los snapshots falsos llevan `objetivo`:
   - `src/store.test.js`: la constante `SNAPSHOT` gana `objetivo` con los dos paquetes desactualizados (`context-mode` y `hunkdiff`). **Regla general**: cualquier test que arme un snapshot a mano tiene que mantener `objetivo` coherente con `packages` — si el test marca filas como desactualizadas, el `objetivo` tiene que traerlas también. Los que lo hacen hoy: `"updateAll sin desactualizados no hace nada"` y `"un excluido desactualizado desactiva 'Actualizar todo' si es el único"`. Si al ejecutar los tests aparece algún otro que dependa del conjunto desactualizado, se actualiza igual: el store ya no lee `packages` para eso.
   - `src/PanelGestor.test.js`: `SNAPSHOT` y `SNAPSHOT_BREW` ganan `objetivo` con sus filas desactualizadas. Los snapshots que se inventan dentro de un test (los de las filas) no lo necesitan: no miran el botón ni el contador.

**Comandos:**

```sh
cargo test --manifest-path src-tauri/Cargo.toml
npm test
npm run format:check && npm run lint
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

**Commit:** `refactor: el objetivo de actualización cruza el IPC (#48)`

---

## Task 4 — brew: el objetivo es todo lo que brew declara desactualizado

**Por qué aquí:** el comportamiento que pedía el usuario. Depende de la Task 3 (el objetivo ya existe; aquí se llena con lo que brew declara).

**Test primero**, en `src-tauri/src/brew.rs`:

```rust
const OUTDATED_CON_DEPENDENCIA: &str = r#"{
    "formulae": [
        {"name":"node","installed_versions":["26.13.0_1"],"current_version":"26.13.0_2","pinned":false,"pinned_version":null},
        {"name":"simdjson","installed_versions":["5.0.6","5.0.7"],"current_version":"5.1.0","pinned":false,"pinned_version":null}
    ],
    "casks": []
}"#;
```

con las hojas `node\n` (simdjson **no** es hoja: es dependencia de node).

- `el_objetivo_de_brew_incluye_una_dependencia_que_no_son_fila` — el objetivo trae `node` **y** `simdjson`; la tabla (`espacio.packages`) trae solo `node`.
- `el_objetivo_de_brew_trae_la_version_instalada_de_cada_paquete` — la entrada de `simdjson` trae `installed == "5.0.6, 5.0.7"` (las dos versiones instaladas, como las de brew) y `latest == "5.1.0"`.
- `una_dependencia_pineada_no_entra_en_la_cola` — una entrada con `pinned: true` va en el objetivo (brew la declara) pero la cola la salta (eso lo verifica `cola.rs`, no brew).

**Qué cambia** (`src-tauri/src/brew.rs`): `armar_brew` devuelve `PlanGestor` y arma el objetivo con el mapa de desactualizados:

```rust
fn armar_brew(
    version_gestor: String,
    hojas: BTreeSet<String>,
    versiones: BTreeMap<String, String>,
    casks: BTreeMap<String, String>,
    outdated: &BTreeMap<(String, String), Desactualizado>,
) -> PlanGestor {
    let mut packages = Vec::new();
    // …las filas, igual que en la Task 2…

    // The update objective: EVERYTHING brew declares outdated, not just
    // the rows. A dependency that brew reports outdated is updatable
    // (`brew upgrade --formula simdjson` works) even though it is not
    // a leaf — and `brew upgrade --formula node` would NOT take it
    // along. Without this, its update is stranded.
    let objetivo = outdated
        .iter()
        .map(|((tipo, nombre), d)| {
            let installed = match tipo.as_str() {
                TIPO_CASK => casks.get(nombre).cloned().unwrap_or_default(),
                _ => versiones.get(nombre_corto(nombre)).cloned().unwrap_or_default(),
            };
            GlobalPackage {
                tipo: Some(tipo.clone()),
                name: nombre.clone(),
                installed,
                latest: Some(d.latest.clone()),
                outdated: true,
                pinned: d.pinned,
            }
        })
        .collect();

    PlanGestor {
        espacio: EspacioGlobal { version_gestor, version_node: None, packages },
        objetivo,
    }
}
```

**Lo que NO cambia:** la tabla (`espacio.packages`) sigue siendo hojas + casks; el frontend no necesita cambios (lee el objetivo genérico); `brew outdated` se llama una sola vez.

**Comandos:**

```sh
cargo test --manifest-path src-tauri/Cargo.toml
npm test
```

**Commit:** `feat: brew — el objetivo de actualización incluye las dependencias (#48)`

---

## Task 5 — Verificación contra el brew real

**Qué hacer:** con el brew de esta máquina (Homebrew 7.0.11, 141 fórmulas, 30 hojas, 5 casks):

```sh
cargo tauri dev
```

- La pestaña de brew muestra `gentleman-programming/tap/engram` como fila (con su versión `3.6.0`), y **no** muestra `simdjson` (no es hoja).
- El estado "desactualizados" del statusbar cuenta 3 (`node`, `node@24`, `simdjson`), no 1.
- "Actualizar todo" arranca y el log muestra `brew/simdjson: …` entre los paquetes.
- Actualizar una fila de un tap de terceros usa la línea con el nombre completo (en el log: `gentleman-programming/tap/engram`).

**Antes de dar esto por bueno**, en el mismo orden que la CI:

```sh
npm run format:check && npm run lint && npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

**Commit:** (ninguno — es verificación; si algo falla, se arregla en la tarea que lo caused)

---

## Fuera de alcance

- Meter las dependencias desactualizadas **como filas** de la tabla: la tabla es la de brew.
- `brew upgrade` a secas para el Actualizar todo: se salta las Excluidas y rompe el Detener fila a fila.
- Llamar a `brew update` desde Nuupa: la frescura ya la gestiona brew.
- Mostrar la línea de comando de cada paquete en el log: el log ya nombra el paquete.
- Los casks que brew omite por diseño (`auto_updates true`, `version :latest`).
- Paquetes instalados fuera de brew (MacPorts, pipx, uv, cargo, apps a mano).
