//! Command layer: the app's real interface towards the UI.
//!
//! Each `#[tauri::command]` is a thin wrapper over a core that receives
//! its dependencies by parameter (manager table, config directory, stop
//! flag): the cores are tested through their interface with toy managers,
//! no Tauri, no environment.

mod brew;
mod bun;
mod cola;
mod exclusiones;
mod kernel;
mod npm;
mod plazo;
mod pnpm;
mod sondeo;

use cola::{EventoCola, Motivo, ResultadoCola, Resumen};
use kernel::{Runner, Snapshot, UpdateOutcome};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

/// Command-line arguments for ONE package, built from its name and
/// (brew-only) type: the shape `args_update` and `args_uninstall`
/// share, so brew's flags (#46) drop in unchanged.
pub(crate) type ArgsPorPaquete = fn(&str, Option<&str>) -> Vec<String>;

/// A supported manager: visible command and how to build ONE package's
/// update arguments. Both live HERE, once per manager: the UI receives
/// the visible one through the seam (Snapshot), it never duplicates it.
/// Adding a manager is ONE entry here plus its adapter module; shared
/// behavior (kernel, queue, exclusions) already knows how to treat it.
#[derive(Clone)]
pub(crate) struct DefinicionGestor {
    pub(crate) nombre: &'static str,
    /// Visible command for the UI (tooltip): "npm i -g".
    pub(crate) comando: &'static str,
    /// Visible uninstall command for the UI (tooltip): "npm uninstall -g".
    /// None while the manager has no uninstall flow (brew, #46).
    pub(crate) comando_desinstalar: Option<&'static str>,
    /// The WHOLE update command line for ONE package, built from its
    /// name and (brew-only) type: single source of the verb — node
    /// managers differ in the verb, brew in a per-row flag.
    pub(crate) args_update: ArgsPorPaquete,
    /// The WHOLE uninstall command line for ONE package, when the
    /// manager supports it (node managers, #44); the shape mirrors
    /// `args_update` so brew's flags (#46) drop in unchanged.
    pub(crate) args_uninstall: Option<ArgsPorPaquete>,
    pub(crate) instalado: fn() -> bool,
    pub(crate) runner: fn() -> std::io::Result<Box<dyn Runner>>,
    pub(crate) snapshot: fn(&dyn Runner) -> std::io::Result<kernel::EspacioGlobal>,
}

/// npm's update line: `npm i -g pkg@latest`.
fn args_npm(name: &str, _tipo: Option<&str>) -> Vec<String> {
    vec!["install".into(), "-g".into(), format!("{name}@latest")]
}

/// pnpm/bun's update line: `<gestor> add -g pkg@latest`.
fn args_add(name: &str, _tipo: Option<&str>) -> Vec<String> {
    vec!["add".into(), "-g".into(), format!("{name}@latest")]
}

/// npm's uninstall line: `npm uninstall -g pkg` (#44).
fn args_uninstall(name: &str, _tipo: Option<&str>) -> Vec<String> {
    vec!["uninstall".into(), "-g".into(), name.into()]
}

/// pnpm/bun's uninstall line: `<gestor> rm -g pkg` (#44).
fn args_rm(name: &str, _tipo: Option<&str>) -> Vec<String> {
    vec!["rm".into(), "-g".into(), name.into()]
}

fn runner_npm() -> std::io::Result<Box<dyn Runner>> {
    Ok(Box::new(npm::RealRunner::discover()?))
}

fn runner_pnpm() -> std::io::Result<Box<dyn Runner>> {
    Ok(Box::new(pnpm::RealPnpmRunner::discover()?))
}

fn runner_bun() -> std::io::Result<Box<dyn Runner>> {
    Ok(Box::new(bun::RealBunRunner::discover()?))
}

fn runner_brew() -> std::io::Result<Box<dyn Runner>> {
    Ok(Box::new(brew::RealBrewRunner::discover()?))
}

/// brew's update line: the row's type picks the flag — always explicit,
/// collision or not (`brew upgrade --formula wget`).
fn args_brew(name: &str, tipo: Option<&str>) -> Vec<String> {
    let flag = match tipo {
        Some(brew::TIPO_CASK) => "--cask",
        _ => "--formula",
    };
    vec!["upgrade".into(), flag.into(), name.into()]
}

const GESTORES: &[DefinicionGestor] = &[
    DefinicionGestor {
        nombre: "npm",
        comando: "npm i -g",
        comando_desinstalar: Some("npm uninstall -g"),
        args_update: args_npm,
        args_uninstall: Some(args_uninstall),
        instalado: npm::instalado,
        runner: runner_npm,
        snapshot: npm::snapshot,
    },
    DefinicionGestor {
        nombre: "pnpm",
        comando: "pnpm add -g",
        comando_desinstalar: Some("pnpm rm -g"),
        args_update: args_add,
        args_uninstall: Some(args_rm),
        instalado: pnpm::instalado,
        runner: runner_pnpm,
        snapshot: pnpm::snapshot,
    },
    DefinicionGestor {
        nombre: "bun",
        comando: "bun add -g",
        comando_desinstalar: Some("bun rm -g"),
        args_update: args_add,
        args_uninstall: Some(args_rm),
        instalado: bun::instalado,
        runner: runner_bun,
        snapshot: bun::snapshot,
    },
    DefinicionGestor {
        nombre: "brew",
        comando: "brew upgrade",
        comando_desinstalar: None, // la papelera de brew llega en #46
        args_update: args_brew,
        args_uninstall: None,
        instalado: brew::instalado,
        runner: runner_brew,
        snapshot: brew::snapshot,
    },
];

fn def_gestor_en<'a>(
    tabla: &'a [DefinicionGestor],
    gestor: &str,
) -> Result<&'a DefinicionGestor, String> {
    tabla
        .iter()
        .find(|g| g.nombre == gestor)
        .ok_or_else(|| format!("unsupported manager: {gestor}"))
}

fn validar_gestor(gestor: &str) -> Result<(), String> {
    def_gestor_en(GESTORES, gestor).map(|_| ())
}

// ---- Cores (testable through their interface, no Tauri) ----

/// Discovers the runner and produces the photo of the global space, with
/// the manager's visible command already assembled.
fn correr_snapshot(def: &DefinicionGestor) -> Result<Snapshot, String> {
    let runner = (def.runner)().map_err(|e| e.to_string())?;
    let espacio = (def.snapshot)(runner.as_ref()).map_err(|e| e.to_string())?;
    Ok(espacio.con_comando(def.comando, def.comando_desinstalar))
}

/// Updates a package with the manager's update line (built per package:
/// verb and — for brew — the row's type flag); each output line leaves
/// through `on_line` (the wrapper streams it as a `pm-output` event).
fn correr_update(
    def: &DefinicionGestor,
    name: &str,
    tipo: Option<&str>,
    on_line: &mut dyn FnMut(&str),
) -> Result<UpdateOutcome, String> {
    kernel::validar_nombre(name).map_err(|e| e.to_string())?;
    let runner = (def.runner)().map_err(|e| e.to_string())?;
    let args = (def.args_update)(name, tipo);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    // An individual update has no Stop: a flag that never fires.
    let sin_parar = kernel::sin_parar();
    kernel::instalar(runner.as_ref(), &refs, on_line, &sin_parar).map_err(|e| e.to_string())
}

/// Uninstalls a package with the manager's uninstall line (#44): a
/// mirror of `correr_update` — name validation, blocking run under
/// the update Plazo, line streaming. Node managers only: a gestor
/// without `args_uninstall` (brew, #46) is refused here.
fn correr_uninstall(
    def: &DefinicionGestor,
    candado: &Mutex<()>,
    dir: &Path,
    gestor: &str,
    name: &str,
    on_line: &mut dyn FnMut(&str),
) -> Result<UpdateOutcome, String> {
    kernel::validar_nombre(name).map_err(|e| e.to_string())?;
    let args_uninstall = def
        .args_uninstall
        .ok_or_else(|| format!("{} no admite Desinstalar todavía", def.nombre))?;
    let runner = (def.runner)().map_err(|e| e.to_string())?;
    let args = args_uninstall(name, None);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    // An individual uninstall has no Stop: a flag that never fires.
    let sin_parar = kernel::sin_parar();
    let salida =
        kernel::instalar(runner.as_ref(), &refs, on_line, &sin_parar).map_err(|e| e.to_string())?;
    if salida.success {
        // The Excluded record dies with its package (#44); the single
        // writer keeps a neighbor's and another gestor's records.
        nucleo_quitar(candado, dir, gestor, name)?;
    }
    Ok(salida)
}

/// The exclusions file's state for the UI (#17): "corrupto" BLOCKS all
/// writes until the user resolves; "ilegible" shows why it cannot read.
#[derive(Serialize)]
struct EstadoExclusiones {
    estado: &'static str, // "ok" | "corrupto" | "ilegible"
    #[serde(skip_serializing_if = "Option::is_none")]
    detalle: Option<String>,
    /// This gestor's excluded names — only meaningful with estado "ok".
    nombres: Vec<String>,
}

/// A gestor's excluded packages AND the file's state (#17): a corrupt
/// file is preserved as evidence and never silently treated as empty.
fn nucleo_get_excluded(dir: &Path, gestor: &str) -> Result<EstadoExclusiones, String> {
    use exclusiones::Lectura;
    Ok(match exclusiones::leer(dir) {
        Lectura::Cargado { mapa, era_legado } => {
            // the legacy-format migration runs here, once
            if era_legado {
                exclusiones::guardar(dir, &mapa).map_err(|e| e.to_string())?;
            }
            EstadoExclusiones {
                estado: "ok",
                detalle: None,
                nombres: mapa.get(gestor).cloned().unwrap_or_default(),
            }
        }
        Lectura::Inexistente => EstadoExclusiones {
            estado: "ok",
            detalle: None,
            nombres: Vec::new(),
        },
        Lectura::Corrupto => {
            // evidence FIRST: the damaged original is preserved; the
            // writes below refuse until the user resolves (#17). A failed
            // copy leaves a trace instead of silence.
            let detalle = exclusiones::resguardar(dir)
                .err()
                .map(|e| format!("(no se pudo conservar la copia .corrupt: {e})"));
            EstadoExclusiones {
                estado: "corrupto",
                detalle,
                nombres: Vec::new(),
            }
        }
        Lectura::Ilegible(e) => EstadoExclusiones {
            estado: "ilegible",
            detalle: Some(e.to_string()),
            nombres: Vec::new(),
        },
    })
}

/// The map a granular op works on, or WHY writing is refused (#17): a
/// file we cannot understand (corrupt) or cannot read is never
/// overwritten by us.
fn mapa_escriturable(
    dir: &Path,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, String> {
    use exclusiones::Lectura;
    match exclusiones::leer(dir) {
        Lectura::Cargado { mapa, .. } => Ok(mapa),
        Lectura::Inexistente => Ok(std::collections::BTreeMap::new()),
        Lectura::Corrupto => Err(
            "el archivo de exclusiones está dañado (conservado como .corrupt): resuélvelo antes de escribir"
                .to_string(),
        ),
        Lectura::Ilegible(e) => Err(format!("no se puede leer el archivo de exclusiones: {e}")),
    }
}

/// Excludes ONE package for ONE gestor (#14): the backend is the single
/// writer, so concurrent toggles are idempotent granular ops — never a
/// full-list replace that can lose a neighbor's exclusion. The lock
/// serializes the file's read-modify-write: it must NOT depend on Tauri
/// running sync commands on one thread.
fn nucleo_excluir(
    candado: &Mutex<()>,
    dir: &Path,
    gestor: &str,
    paquete: &str,
) -> Result<(), String> {
    let _candado = candado.lock().unwrap();
    let mut mapa = mapa_escriturable(dir)?;
    exclusiones::excluir(&mut mapa, gestor, paquete);
    exclusiones::guardar(dir, &mapa).map_err(|e| e.to_string())
}

/// Removes ONE package's exclusion. Idempotent; a legacy file migrates to
/// the map format on the way (guardar always writes the map).
fn nucleo_quitar(
    candado: &Mutex<()>,
    dir: &Path,
    gestor: &str,
    paquete: &str,
) -> Result<(), String> {
    let _candado = candado.lock().unwrap();
    let mut mapa = mapa_escriturable(dir)?;
    exclusiones::quitar(&mut mapa, gestor, paquete);
    exclusiones::guardar(dir, &mapa).map_err(|e| e.to_string())
}

/// Resolves the corrupt emergency by starting clean (#17): evidence
/// (.corrupt) first, then a valid empty map. Only ever with the user's
/// explicit choice.
fn nucleo_empezar_de_cero(candado: &Mutex<()>, dir: &Path) -> Result<(), String> {
    let _candado = candado.lock().unwrap();
    exclusiones::empezar_de_cero(dir).map_err(|e| e.to_string())
}

// ---- Commands (thin wrappers over the cores) ----

/// The machine facts a diagnostics copy needs (#21): version, OS and the
/// installed gestores, plus the user's home (to redact it on the way out).
#[derive(Serialize)]
struct Diagnostico {
    version: &'static str,
    so: String,
    gestores: Vec<String>,
    home: Option<String>,
}

fn nucleo_diagnostico(gestores: Vec<String>) -> Diagnostico {
    Diagnostico {
        version: env!("CARGO_PKG_VERSION"),
        so: format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH),
        gestores,
        home: kernel::home().map(|h| h.display().to_string()),
    }
}

/// Copy-diagnostics facts (#21): nothing else leaves the machine.
#[tauri::command]
fn diagnostico() -> Diagnostico {
    nucleo_diagnostico(gestores_instalados())
}

/// Command dependencies, resolved once at startup.
struct Contexto {
    dir_config: PathBuf,
    /// The queue's shared flags: Stop (cuts, #16), graceful abandonment
    /// (the panel went away) and the ONE-active-queue guard (#12).
    banderas: cola::Banderas,
    /// Serializes the exclusions file's read-modify-write (#14): two
    /// granular commands (even from different gestores' stores) must
    /// never interleave cargar/guardar — a lost update loses an exclusion.
    candado_exclusiones: Arc<Mutex<()>>,
}

/// Available tabs: supported AND installed managers on this machine
/// (presence check, no spawns).
#[tauri::command]
fn gestores_instalados() -> Vec<String> {
    GESTORES
        .iter()
        .filter(|g| (g.instalado)())
        .map(|g| g.nombre.to_string())
        .collect()
}

/// Lists the global packages of the manager's space.
/// `outdated` takes seconds (registry query): it runs blocking on a pool
/// thread so the IPC does not freeze.
#[tauri::command]
async fn list_globals(gestor: String) -> Result<Snapshot, String> {
    let def = def_gestor_en(GESTORES, &gestor)?;
    tauri::async_runtime::spawn_blocking(move || correr_snapshot(def))
        .await
        .map_err(|e| e.to_string())?
}

/// A manager output line streamed during an update.
#[derive(Clone, Serialize)]
struct OutputLine {
    gestor: String,
    package: String,
    line: String,
}

/// Updates a global package to its latest version; each output line
/// arrives as a `pm-output` event for the log panel. `tipo` is the
/// row's type when the manager has more than one kind of package
/// (brew: formula/cask) — it decides the update line's flag.
#[tauri::command]
async fn update_package(
    gestor: String,
    name: String,
    tipo: Option<String>,
    app: tauri::AppHandle,
) -> Result<UpdateOutcome, String> {
    let def = def_gestor_en(GESTORES, &gestor)?;
    tauri::async_runtime::spawn_blocking(move || {
        correr_update(def, &name, tipo.as_deref(), &mut |line| {
            let _ = app.emit(
                "pm-output",
                OutputLine {
                    gestor: gestor.clone(),
                    package: name.clone(),
                    line: line.to_string(),
                },
            );
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Uninstalls a global package from its manager (#44): the mirror of
/// `update_package` — blocking run in the pool, each output line as a
/// `pm-output` event, update Plazo. On success the core already
/// deleted the package's Excluded record (single writer).
#[tauri::command]
async fn uninstall_package(
    gestor: String,
    name: String,
    app: tauri::AppHandle,
    estado: tauri::State<'_, Contexto>,
) -> Result<UpdateOutcome, String> {
    let def = def_gestor_en(GESTORES, &gestor)?;
    let dir = estado.dir_config.clone();
    let candado = Arc::clone(&estado.candado_exclusiones);
    tauri::async_runtime::spawn_blocking(move || {
        correr_uninstall(def, &candado, &dir, &gestor, &name, &mut |line| {
            let _ = app.emit(
                "pm-output",
                OutputLine {
                    gestor: gestor.clone(),
                    package: name.clone(),
                    line: line.to_string(),
                },
            );
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_excluded(
    gestor: String,
    estado: tauri::State<Contexto>,
) -> Result<EstadoExclusiones, String> {
    validar_gestor(&gestor)?;
    nucleo_get_excluded(&estado.dir_config, &gestor)
}

/// Resolves the corrupt-exclusions emergency by starting clean (#17).
#[tauri::command]
fn exclusiones_de_cero(estado: tauri::State<Contexto>) -> Result<(), String> {
    nucleo_empezar_de_cero(&estado.candado_exclusiones, &estado.dir_config)
}

/// Excludes one package from Actualizar todo in its gestor (#14).
#[tauri::command]
fn excluir_paquete(
    gestor: String,
    paquete: String,
    estado: tauri::State<Contexto>,
) -> Result<(), String> {
    validar_gestor(&gestor)?;
    kernel::validar_nombre(&paquete).map_err(|e| e.to_string())?;
    nucleo_excluir(
        &estado.candado_exclusiones,
        &estado.dir_config,
        &gestor,
        &paquete,
    )
}

/// Removes one package's exclusion in its gestor (#14).
#[tauri::command]
fn quitar_exclusion(
    gestor: String,
    paquete: String,
    estado: tauri::State<Contexto>,
) -> Result<(), String> {
    validar_gestor(&gestor)?;
    kernel::validar_nombre(&paquete).map_err(|e| e.to_string())?;
    nucleo_quitar(
        &estado.candado_exclusiones,
        &estado.dir_config,
        &gestor,
        &paquete,
    )
}

/// "Update all": sequential queue in Rust. Progress via `pm-cola`
/// (starts/result per package) and `pm-output` (log lines); on finish it
/// returns summary + final snapshot (a single refresh).
#[derive(Serialize)]
struct ResultadoActualizarTodo {
    resumen: Resumen,
    snapshot: Snapshot,
}

#[tauri::command]
async fn actualizar_todo(
    gestor: String,
    estado: tauri::State<'_, Contexto>,
    app: tauri::AppHandle,
) -> Result<ResultadoActualizarTodo, String> {
    let def = def_gestor_en(GESTORES, &gestor)?;
    let dir = estado.dir_config.clone();
    let banderas = estado.banderas.compartidas();
    tauri::async_runtime::spawn_blocking(move || {
        let (resumen, snapshot) = cola::correr(def, &dir, &banderas, &mut |ev| match ev {
            EventoCola::Linea { paquete, linea } => {
                let _ = app.emit(
                    "pm-output",
                    OutputLine {
                        gestor: gestor.clone(),
                        package: paquete.clone(),
                        line: linea.clone(),
                    },
                );
            }
            EventoCola::Empieza { paquete } => {
                let _ = app.emit("pm-cola", EventoPaquete::empieza(&gestor, paquete));
            }
            EventoCola::Resultado(r) => {
                let _ = app.emit("pm-cola", EventoPaquete::resultado(&gestor, r));
            }
        })?;
        Ok(ResultadoActualizarTodo { resumen, snapshot })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A queue event for a table row. Success is derivable: `motivo === "ok"`.
#[derive(Clone, Serialize)]
struct EventoPaquete {
    gestor: String,
    tipo: &'static str, // "empieza" | "resultado"
    paquete: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    salida: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    motivo: Option<Motivo>,
}

impl EventoPaquete {
    fn empieza(gestor: &str, paquete: &str) -> Self {
        Self {
            gestor: gestor.into(),
            tipo: "empieza",
            paquete: paquete.into(),
            salida: None,
            motivo: None,
        }
    }

    fn resultado(gestor: &str, r: &ResultadoCola) -> Self {
        Self {
            gestor: gestor.into(),
            tipo: "resultado",
            paquete: r.paquete.clone(),
            salida: Some(r.salida.clone()),
            motivo: Some(r.motivo),
        }
    }
}

/// Stops the queue: the in-flight package is CUT (same escalation as the
/// deadline, #16) and the pending ones never start.
#[tauri::command]
fn detener_actualizar_todo(estado: tauri::State<Contexto>) {
    estado.banderas.detener();
}

/// The panel went away: the in-flight package FINISHES and the pending
/// ones never start — nothing is cut.
#[tauri::command]
fn abandonar_actualizar_todo(estado: tauri::State<Contexto>) {
    estado.banderas.abandonar();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir_config = app
                .path()
                .app_config_dir()
                .map_err(|e| format!("no config directory: {e}"))?;
            let banderas = cola::Banderas::nuevas();
            app.manage(Contexto {
                dir_config,
                banderas,
                candado_exclusiones: Arc::new(Mutex::new(())),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_globals,
            update_package,
            uninstall_package,
            get_excluded,
            excluir_paquete,
            quitar_exclusion,
            exclusiones_de_cero,
            gestores_instalados,
            actualizar_todo,
            detener_actualizar_todo,
            abandonar_actualizar_todo,
            diagnostico
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::testutil::FakeRunner;

    // ---- dispatch over the table ----

    #[test]
    fn npm_pnpm_bun_y_brew_soportados() {
        assert!(validar_gestor("npm").is_ok());
        assert!(validar_gestor("pnpm").is_ok());
        assert!(validar_gestor("bun").is_ok());
        assert!(validar_gestor("brew").is_ok());
    }

    #[test]
    fn la_tabla_de_gestores_es_los_cuatro_en_orden() {
        // The table drives tabs AND the diagnostics copy: brew closes
        // the list.
        let nombres: Vec<&str> = GESTORES.iter().map(|g| g.nombre).collect();
        assert_eq!(nombres, vec!["npm", "pnpm", "bun", "brew"]);
    }

    #[test]
    fn gestores_no_soportados_rechazados() {
        for g in ["yarn", "", "--force"] {
            assert!(validar_gestor(g).is_err(), "{g} must not pass");
        }
    }

    // Toy manager table: same protocol as npm, fake runner.
    const LS_JSON: &str = r#"{"dependencies": {"hunkdiff": {"version": "0.17.2"}}}"#;
    const OUTDATED_JSON: &str = r#"{"hunkdiff": {"latest": "0.18.0"}}"#;

    fn def_falsa() -> DefinicionGestor {
        DefinicionGestor {
            nombre: "falso",
            comando: "falso i -g",
            comando_desinstalar: Some("falso uninstall -g"),
            args_update: args_npm,
            args_uninstall: Some(args_uninstall),
            instalado: || true,
            runner: || {
                Ok(Box::new(
                    FakeRunner::new("1.0.0")
                        .respuesta("ls", LS_JSON, 0)
                        .respuesta("outdated", OUTDATED_JSON, 0)
                        .respuesta("install", "added 1 package in 2s", 0)
                        .respuesta_exacta("uninstall -g hunkdiff", "removed 1 package in 1s", 0),
                ) as Box<dyn Runner>)
            },
            snapshot: npm::snapshot,
        }
    }

    #[test]
    fn correr_snapshot_ensambla_el_comando_visible_del_def() {
        let snap = correr_snapshot(&def_falsa()).expect("valid snapshot");
        assert_eq!(snap.comando_actualizar, "falso i -g"); // verb: single source
        assert_eq!(snap.espacio.version_gestor, "1.0.0");
        assert!(snap.espacio.packages.iter().any(|p| p.outdated));
    }

    #[test]
    fn correr_update_usa_el_verbo_del_def_y_streamea_lineas() {
        let mut lineas = Vec::new();
        let out = correr_update(&def_falsa(), "hunkdiff", None, &mut |l| {
            lineas.push(l.to_string())
        })
        .expect("valid update");
        assert!(out.success);
        assert_eq!(lineas, vec!["added 1 package in 2s"]);
    }

    #[test]
    fn correr_update_arma_los_args_por_paquete() {
        // A brew-shaped def: the args builder owns the WHOLE command line
        // and varies it by the row's type — the seam brew needs (#36).
        // The exact-line answers make a wrong command FAIL: only the
        // built line succeeds.
        fn args_brew(name: &str, tipo: Option<&str>) -> Vec<String> {
            let flag = match tipo {
                Some("cask") => "--cask",
                _ => "--formula",
            };
            vec!["upgrade".into(), flag.into(), name.into()]
        }
        fn runner_brew() -> std::io::Result<Box<dyn Runner>> {
            Ok(Box::new(
                FakeRunner::new("7.0.4")
                    .respuesta_exacta("upgrade --formula wget", "Upgrading wget", 0)
                    .respuesta_exacta("upgrade --cask wine", "Upgrading wine", 0),
            ) as Box<dyn Runner>)
        }
        let def = DefinicionGestor {
            nombre: "brew",
            comando: "brew upgrade",
            comando_desinstalar: None,
            args_update: args_brew,
            args_uninstall: None,
            instalado: || true,
            runner: runner_brew,
            snapshot: npm::snapshot,
        };
        correr_update(&def, "wget", None, &mut |_| {}).expect("formula line");
        correr_update(&def, "wine", Some("cask"), &mut |_| {}).expect("cask line");
    }

    #[test]
    fn correr_update_rechaza_nombres_invalidos() {
        for nombre in ["", "--force"] {
            assert!(
                correr_update(&def_falsa(), nombre, None, &mut |_| {}).is_err(),
                "{nombre} must not pass"
            );
        }
    }

    #[test]
    fn correr_update_con_gestor_roto_propaga_el_error() {
        let def = DefinicionGestor {
            runner: || {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no binary",
                ))
            },
            ..def_falsa()
        };
        let err = correr_update(&def, "hunkdiff", None, &mut |_| {}).unwrap_err();
        assert!(err.contains("no binary"));
    }

    // ---- Desinstalar (#44): the table owns the verb per manager ----

    #[test]
    fn args_de_desinstalacion_por_gestor_en_la_tabla() {
        // Node managers only — brew's row arrives with its pineada
        // flow (#46). npm: `uninstall -g`; pnpm/bun: `rm -g`.
        let lineas: Vec<(&str, Option<String>)> = GESTORES
            .iter()
            .map(|g| {
                (
                    g.nombre,
                    g.args_uninstall.map(|f| f("cowsay", None).join(" ")),
                )
            })
            .collect();
        assert_eq!(
            lineas,
            vec![
                ("npm", Some("uninstall -g cowsay".into())),
                ("pnpm", Some("rm -g cowsay".into())),
                ("bun", Some("rm -g cowsay".into())),
                ("brew", None),
            ]
        );
    }

    #[test]
    fn comando_visible_de_desinstalacion_por_gestor_en_la_tabla() {
        let visibles: Vec<(&str, Option<&str>)> = GESTORES
            .iter()
            .map(|g| (g.nombre, g.comando_desinstalar))
            .collect();
        assert_eq!(
            visibles,
            vec![
                ("npm", Some("npm uninstall -g")),
                ("pnpm", Some("pnpm rm -g")),
                ("bun", Some("bun rm -g")),
                ("brew", None),
            ]
        );
    }

    #[test]
    fn correr_uninstall_usa_el_verbo_del_def_y_streamea_lineas() {
        let mut lineas = Vec::new();
        let dir = tempfile::tempdir().unwrap();
        let out = correr_uninstall(
            &def_falsa(),
            &Mutex::new(()),
            dir.path(),
            "falso",
            "hunkdiff",
            &mut |l| lineas.push(l.to_string()),
        )
        .expect("valid uninstall");
        assert!(out.success);
        assert_eq!(lineas, vec!["removed 1 package in 1s"]);
    }

    #[test]
    fn correr_uninstall_arma_los_args_por_gestor() {
        // The exact-line answers make a wrong verb FAIL: only the
        // line each manager's table entry builds succeeds.
        fn runner_tres_verbos() -> std::io::Result<Box<dyn Runner>> {
            Ok(Box::new(
                FakeRunner::new("1.0.0")
                    .respuesta_exacta("uninstall -g hunkdiff", "removed", 0)
                    .respuesta_exacta("rm -g hunkdiff", "removed", 0),
            ) as Box<dyn Runner>)
        }
        for gestor in ["npm", "pnpm", "bun"] {
            let def = DefinicionGestor {
                runner: runner_tres_verbos,
                ..def_gestor_en(GESTORES, gestor).unwrap().clone()
            };
            let dir = tempfile::tempdir().unwrap();
            correr_uninstall(
                &def,
                &Mutex::new(()),
                dir.path(),
                gestor,
                "hunkdiff",
                &mut |_| {},
            )
            .unwrap_or_else(|e| panic!("{gestor}: {e}"));
        }
    }

    #[test]
    fn correr_uninstall_rechaza_nombres_invalidos() {
        let dir = tempfile::tempdir().unwrap();
        for nombre in ["", "--force"] {
            assert!(
                correr_uninstall(
                    &def_falsa(),
                    &Mutex::new(()),
                    dir.path(),
                    "falso",
                    nombre,
                    &mut |_| {},
                )
                .is_err(),
                "{nombre} must not pass"
            );
        }
    }

    #[test]
    fn correr_uninstall_rechaza_un_gestor_sin_desinstalacion() {
        // brew's row has no trash yet: its flow (with pineadas) is #46.
        let def = DefinicionGestor {
            args_uninstall: None,
            ..def_falsa()
        };
        let dir = tempfile::tempdir().unwrap();
        let err = correr_uninstall(
            &def,
            &Mutex::new(()),
            dir.path(),
            "falso",
            "hunkdiff",
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("no admite Desinstalar"), "{err}");
    }

    #[test]
    fn desinstalar_con_exito_borra_la_exclusion_del_paquete() {
        let dir = tempfile::tempdir().unwrap();
        let candado = Mutex::new(());
        nucleo_excluir(&candado, dir.path(), "falso", "hunkdiff").unwrap();
        nucleo_excluir(&candado, dir.path(), "falso", "vecino").unwrap();
        nucleo_excluir(&candado, dir.path(), "otro", "hunkdiff").unwrap();
        let out = correr_uninstall(
            &def_falsa(),
            &candado,
            dir.path(),
            "falso",
            "hunkdiff",
            &mut |_| {},
        )
        .expect("valid uninstall");
        assert!(out.success);
        // only the uninstalled package's Excluded record is gone:
        // a neighbor's and another gestor's same-name record survive
        assert_eq!(
            nucleo_get_excluded(dir.path(), "falso").unwrap().nombres,
            ["vecino"]
        );
        assert_eq!(
            nucleo_get_excluded(dir.path(), "otro").unwrap().nombres,
            ["hunkdiff"]
        );
    }

    #[test]
    fn desinstalar_con_fallo_conserva_la_exclusion() {
        let dir = tempfile::tempdir().unwrap();
        let candado = Mutex::new(());
        nucleo_excluir(&candado, dir.path(), "falso", "hunkdiff").unwrap();
        let def = DefinicionGestor {
            runner: || {
                Ok(Box::new(FakeRunner::new("1.0.0").respuesta_exacta(
                    "uninstall -g hunkdiff",
                    "EACCES: permission denied",
                    1,
                )) as Box<dyn Runner>)
            },
            ..def_falsa()
        };
        let out = correr_uninstall(&def, &candado, dir.path(), "falso", "hunkdiff", &mut |_| {})
            .expect("a gestor failure is a result, not an error");
        assert!(!out.success);
        assert_eq!(
            nucleo_get_excluded(dir.path(), "falso").unwrap().nombres,
            ["hunkdiff"]
        );
    }

    #[test]
    fn correr_snapshot_incluye_el_comando_de_desinstalar() {
        let snap = correr_snapshot(&def_falsa()).expect("valid snapshot");
        assert_eq!(snap.comando_actualizar, "falso i -g");
        assert_eq!(
            snap.comando_desinstalar.as_deref(),
            Some("falso uninstall -g")
        );
        // A gestor without the flow yet (brew, #46) crosses as None.
        let sin_desinstalar = DefinicionGestor {
            comando_desinstalar: None,
            ..def_falsa()
        };
        assert_eq!(
            correr_snapshot(&sin_desinstalar)
                .unwrap()
                .comando_desinstalar,
            None
        );
    }

    // ---- exclusions through their core ----

    #[test]
    fn exclusiones_granulares_roundtrip_por_gestor_y_migracion_legada() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("exclusiones.json"), r#"["hunkdiff"]"#).unwrap();
        // legacy → the first read migrates once, as npm's
        assert_eq!(
            nucleo_get_excluded(dir.path(), "npm").unwrap().nombres,
            ["hunkdiff"]
        );
        assert_eq!(
            nucleo_get_excluded(dir.path(), "pnpm").unwrap().nombres,
            Vec::<String>::new()
        );
        // granular, idempotent, per (gestor, paquete)
        let candado = Mutex::new(());
        nucleo_excluir(&candado, dir.path(), "pnpm", "cowsay").unwrap();
        nucleo_excluir(&candado, dir.path(), "pnpm", "cowsay").unwrap(); // no duplicate
        nucleo_quitar(&candado, dir.path(), "npm", "hunkdiff").unwrap();
        nucleo_quitar(&candado, dir.path(), "npm", "hunkdiff").unwrap(); // absent: fine
        assert_eq!(
            nucleo_get_excluded(dir.path(), "pnpm").unwrap().nombres,
            ["cowsay"]
        );
        assert_eq!(
            nucleo_get_excluded(dir.path(), "npm").unwrap().nombres,
            Vec::<String>::new()
        );
    }

    #[test]
    fn exclusiones_gestor_desconocido_rechazado_por_el_wrapper() {
        assert!(validar_gestor("yarn").is_err());
    }

    #[test]
    fn diagnostico_lleva_version_so_y_gestores() {
        let d = nucleo_diagnostico(vec!["npm".to_string(), "pnpm".to_string()]);
        assert_eq!(d.version, env!("CARGO_PKG_VERSION"));
        assert!(d.so.contains(std::env::consts::OS));
        assert_eq!(d.gestores, vec!["npm", "pnpm"]);
    }
}
