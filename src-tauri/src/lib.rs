//! Command layer: the app's real interface towards the UI.
//!
//! Each `#[tauri::command]` is a thin wrapper over a core that receives
//! its dependencies by parameter (manager table, config directory, stop
//! flag): the cores are tested through their interface with toy managers,
//! no Tauri, no environment.

mod bun;
mod cola;
mod exclusiones;
mod kernel;
mod npm;
mod plazo;
mod pnpm;

use cola::{EventoCola, Motivo, ResultadoCola, Resumen};
use kernel::{Runner, Snapshot, UpdateOutcome};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};

/// A supported manager: visible command, install verb and how its
/// runners and its global space are discovered. The verb lives HERE,
/// once per manager: the UI receives it through the seam (Snapshot), it
/// never duplicates it. Adding a manager = adding one entry.
pub(crate) struct DefinicionGestor {
    pub(crate) nombre: &'static str,
    /// Visible command for the UI (tooltip): "npm i -g".
    pub(crate) comando: &'static str,
    /// Install argument (install/add) that kernel::instalar runs.
    pub(crate) verbo: &'static str,
    pub(crate) instalado: fn() -> bool,
    pub(crate) runner: fn() -> std::io::Result<Box<dyn Runner>>,
    pub(crate) snapshot: fn(&dyn Runner) -> std::io::Result<kernel::EspacioGlobal>,
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

const GESTORES: &[DefinicionGestor] = &[
    DefinicionGestor {
        nombre: "npm",
        comando: "npm i -g",
        verbo: "install",
        instalado: npm::instalado,
        runner: runner_npm,
        snapshot: npm::snapshot,
    },
    DefinicionGestor {
        nombre: "pnpm",
        comando: "pnpm add -g",
        verbo: "add",
        instalado: pnpm::instalado,
        runner: runner_pnpm,
        snapshot: pnpm::snapshot,
    },
    DefinicionGestor {
        nombre: "bun",
        comando: "bun add -g",
        verbo: "add",
        instalado: bun::instalado,
        runner: runner_bun,
        snapshot: bun::snapshot,
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
    Ok(espacio.con_comando(def.comando))
}

/// Updates a package with the manager's verb; each output line leaves
/// through `on_line` (the wrapper streams it as a `pm-output` event).
fn correr_update(
    def: &DefinicionGestor,
    name: &str,
    on_line: &mut dyn FnMut(&str),
) -> Result<UpdateOutcome, String> {
    let runner = (def.runner)().map_err(|e| e.to_string())?;
    // An individual update has no Stop: a flag that never fires.
    let sin_parar = kernel::sin_parar();
    kernel::instalar(runner.as_ref(), def.verbo, name, on_line, &sin_parar)
        .map_err(|e| e.to_string())
}

/// A manager's excluded packages; the legacy-format migration runs here,
/// once, while the file still is legacy.
fn nucleo_get_excluded(dir: &Path, gestor: &str) -> Result<Vec<String>, String> {
    let (mapa, era_legado) = exclusiones::cargar(dir);
    if era_legado {
        exclusiones::guardar(dir, &mapa).map_err(|e| e.to_string())?;
    }
    Ok(mapa.get(gestor).cloned().unwrap_or_default())
}

/// Excludes ONE package for ONE gestor (#14): the backend is the single
/// writer, so concurrent toggles are idempotent granular ops — never a
/// full-list replace that can lose a neighbor's exclusion.
fn nucleo_excluir(dir: &Path, gestor: &str, paquete: &str) -> Result<(), String> {
    let (mut mapa, _) = exclusiones::cargar(dir);
    exclusiones::excluir(&mut mapa, gestor, paquete);
    exclusiones::guardar(dir, &mapa).map_err(|e| e.to_string())
}

/// Removes ONE package's exclusion. Idempotent; a legacy file migrates to
/// the map format on the way (guardar always writes the map).
fn nucleo_quitar(dir: &Path, gestor: &str, paquete: &str) -> Result<(), String> {
    let (mut mapa, _) = exclusiones::cargar(dir);
    exclusiones::quitar(&mut mapa, gestor, paquete);
    exclusiones::guardar(dir, &mapa).map_err(|e| e.to_string())
}

// ---- Commands (thin wrappers over the cores) ----

/// Command dependencies, resolved once at startup.
struct Contexto {
    dir_config: PathBuf,
    /// The queue's shared flags: Stop (cuts, #16), graceful abandonment
    /// (the panel went away) and the ONE-active-queue guard (#12).
    banderas: cola::Banderas,
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
/// arrives as a `pm-output` event for the log panel.
#[tauri::command]
async fn update_package(
    gestor: String,
    name: String,
    app: tauri::AppHandle,
) -> Result<UpdateOutcome, String> {
    let def = def_gestor_en(GESTORES, &gestor)?;
    tauri::async_runtime::spawn_blocking(move || {
        correr_update(def, &name, &mut |line| {
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
fn get_excluded(gestor: String, estado: tauri::State<Contexto>) -> Result<Vec<String>, String> {
    validar_gestor(&gestor)?;
    nucleo_get_excluded(&estado.dir_config, &gestor)
}

/// Excludes one package from "Update all" in its gestor (#14).
#[tauri::command]
fn excluir_paquete(
    gestor: String,
    paquete: String,
    estado: tauri::State<Contexto>,
) -> Result<(), String> {
    validar_gestor(&gestor)?;
    nucleo_excluir(&estado.dir_config, &gestor, &paquete)
}

/// Removes one package's exclusion in its gestor (#14).
#[tauri::command]
fn quitar_exclusion(
    gestor: String,
    paquete: String,
    estado: tauri::State<Contexto>,
) -> Result<(), String> {
    validar_gestor(&gestor)?;
    nucleo_quitar(&estado.dir_config, &gestor, &paquete)
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
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir_config = app
                .path()
                .app_config_dir()
                .map_err(|e| format!("no config directory: {e}"))?;
            app.manage(Contexto {
                dir_config,
                banderas: cola::Banderas::nuevas(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_globals,
            update_package,
            get_excluded,
            excluir_paquete,
            quitar_exclusion,
            gestores_instalados,
            actualizar_todo,
            detener_actualizar_todo,
            abandonar_actualizar_todo
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
    fn npm_pnpm_y_bun_soportados() {
        assert!(validar_gestor("npm").is_ok());
        assert!(validar_gestor("pnpm").is_ok());
        assert!(validar_gestor("bun").is_ok());
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
            verbo: "install",
            instalado: || true,
            runner: || {
                Ok(Box::new(
                    FakeRunner::new("1.0.0")
                        .respuesta("ls", LS_JSON, 0)
                        .respuesta("outdated", OUTDATED_JSON, 0)
                        .respuesta("install", "added 1 package in 2s", 0),
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
        let out = correr_update(&def_falsa(), "hunkdiff", &mut |l| {
            lineas.push(l.to_string())
        })
        .expect("valid update");
        assert!(out.success);
        assert_eq!(lineas, vec!["added 1 package in 2s"]);
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
        let err = correr_update(&def, "hunkdiff", &mut |_| {}).unwrap_err();
        assert!(err.contains("no binary"));
    }

    // ---- exclusions through their core ----

    #[test]
    fn exclusiones_granulares_roundtrip_por_gestor_y_migracion_legada() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("exclusiones.json"), r#"["hunkdiff"]"#).unwrap();
        // legacy → the first read migrates once, as npm's
        assert_eq!(
            nucleo_get_excluded(dir.path(), "npm").unwrap(),
            ["hunkdiff"]
        );
        assert_eq!(
            nucleo_get_excluded(dir.path(), "pnpm").unwrap(),
            Vec::<String>::new()
        );
        // granular, idempotent, per (gestor, paquete)
        nucleo_excluir(dir.path(), "pnpm", "cowsay").unwrap();
        nucleo_excluir(dir.path(), "pnpm", "cowsay").unwrap(); // no duplicate
        nucleo_quitar(dir.path(), "npm", "hunkdiff").unwrap();
        nucleo_quitar(dir.path(), "npm", "hunkdiff").unwrap(); // absent: fine
        assert_eq!(nucleo_get_excluded(dir.path(), "pnpm").unwrap(), ["cowsay"]);
        assert_eq!(
            nucleo_get_excluded(dir.path(), "npm").unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn exclusiones_gestor_desconocido_rechazado_por_el_wrapper() {
        assert!(validar_gestor("yarn").is_err());
    }
}
