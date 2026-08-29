//! Command layer: the app's real interface towards the UI.
//!
//! Each `#[tauri::command]` is a thin wrapper over a core that receives
//! its dependencies by parameter (manager table, config directory, stop
//! flag): the cores are tested through their interface with toy managers,
//! no Tauri, no environment.

mod bun;
mod cola;
mod exclusiones;
mod habilidades;
mod kernel;
mod npm;
mod plazo;
mod pnpm;

use cola::{EventoCola, Motivo, ResultadoCola, Resumen};
use kernel::{Runner, Snapshot, UpdateOutcome};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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
    /// Same single-writer rule for the skills manifest (#27): an install
    /// is one read-modify-write, never interleaved.
    candado_habilidades: Arc<Mutex<()>>,
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

// ---- Habilidades (#26) ----

/// The skills tab's single data path (#28): scan + per-Gestionada SHA
/// check. The scan is filesystem and network work: blocking on a pool
/// thread like the package queries. A network failure marks ITS row
/// (SinVerificar + reason); it never blocks the rest of the list.
#[tauri::command]
async fn listar_habilidades() -> Result<habilidades::SalidaHabilidades, String> {
    let raiz = habilidades::carpeta_del_usuario()?;
    tauri::async_runtime::spawn_blocking(move || {
        let proveedor = habilidades::ProveedorReal::nuevo()?;
        Ok(habilidades::refrescar(&raiz, &proveedor))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Resolves the corrupt-manifest emergency by starting clean (#17):
/// evidence (.corrupt) first, then a valid empty map — only with the
/// user's explicit choice.
#[tauri::command]
fn habilidades_de_cero() -> Result<(), String> {
    let raiz = habilidades::carpeta_del_usuario()?;
    habilidades::empezar_de_cero(&raiz).map_err(|e| e.to_string())
}

/// Scans ONE origin (a GitHub repo or a direct skill's URL) and reports
/// every skill found: Conforme or Inválida with its reason. NOTHING is
/// activated here — the download lives in a throwaway staging dir.
#[tauri::command]
async fn escanear_origen(
    origen: String,
) -> Result<Vec<habilidades::HabilidadEscaneada>, String> {
    let url = habilidades::parsear_origen(&origen)?;
    tauri::async_runtime::spawn_blocking(move || {
        let proveedor = habilidades::ProveedorReal::nuevo()?;
        let staging = tempfile::tempdir().map_err(|e| e.to_string())?;
        habilidades::escanear(&proveedor, &url, staging.path())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Installs the selected rutas of ONE origin (the add flow's second
/// step): per ruta — validate, fetch its SHA, activate by copy-and-swap,
/// record the Origen. The manifest is written ONCE with every confirmed
/// entry under the manifest's lock (#17: a corrupt one refuses writes).
#[tauri::command]
async fn instalar_habilidades(
    origen: String,
    rutas: Vec<String>,
    estado: tauri::State<'_, Contexto>,
) -> Result<Vec<habilidades::ResultadoInstalacion>, String> {
    let url = habilidades::parsear_origen(&origen)?;
    let candado = estado.candado_habilidades.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _candado = candado.lock().unwrap();
        let raiz = habilidades::carpeta_del_usuario()?;
        let mut mapa = habilidades::mapa_escriturable(&raiz)?;
        let proveedor = habilidades::ProveedorReal::nuevo()?;
        let ahora = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let resultados = habilidades::instalar(&proveedor, &url, &rutas, &raiz, &mut mapa, ahora)?;
        habilidades::guardar(&raiz, &mapa).map_err(|e| e.to_string())?;
        Ok(resultados)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens ONE Habilidad's folder in the system file manager: the name is
/// validated as a single safe component under the skills root — never a
/// path, never a hidden or parent entry.
#[tauri::command]
async fn abrir_habilidad(nombre: String, app: tauri::AppHandle) -> Result<(), String> {
    habilidades::nombre_seguro(&nombre)?;
    let ruta = habilidades::carpeta_del_usuario()?.join(&nombre);
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(ruta.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
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

/// Updates ONE managed Habilidad to the latest content of its Origen
/// (#29): the two-phase install over the SAVED origen — validate the new
/// content before activation, then record the new SHA. Managed-only: a
/// No gestionada has nothing to update FROM. One manifest write on
/// success, under the manifest's lock.
#[tauri::command]
async fn actualizar_habilidad(
    nombre: String,
    estado: tauri::State<'_, Contexto>,
) -> Result<(), String> {
    habilidades::nombre_seguro(&nombre)?;
    let candado = estado.candado_habilidades.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _candado = candado.lock().unwrap();
        let raiz = habilidades::carpeta_del_usuario()?;
        let mut mapa = habilidades::mapa_escriturable(&raiz)?;
        let proveedor = habilidades::ProveedorReal::nuevo()?;
        let ahora = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        habilidades::actualizar(&proveedor, &nombre, &raiz, &mut mapa, ahora)?;
        habilidades::guardar(&raiz, &mapa).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
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
                candado_exclusiones: Arc::new(Mutex::new(())),
                candado_habilidades: Arc::new(Mutex::new(())),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_globals,
            update_package,
            get_excluded,
            excluir_paquete,
            quitar_exclusion,
            exclusiones_de_cero,
            gestores_instalados,
            listar_habilidades,
            habilidades_de_cero,
            abrir_habilidad,
            escanear_origen,
            instalar_habilidades,
            actualizar_habilidad,
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
