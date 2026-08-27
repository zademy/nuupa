//! Capa de comandos: la interfaz real de la app hacia la UI.
//!
//! Cada `#[tauri::command]` es un wrapper delgado sobre un núcleo que
//! recibe sus dependencias por parámetro (tabla de gestores, directorio de
//! configuración, bandera de parada): los núcleos se prueban por su
//! interfaz con gestores de juguete, sin Tauri ni entorno.

mod bun;
mod cola;
mod exclusiones;
mod kernel;
mod npm;
mod pnpm;

use cola::{EventoCola, Resumen};
use kernel::{Runner, Snapshot, UpdateOutcome};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// Un gestor soportado: comando visible, verbo de instalación y cómo se
/// descubren sus runners y su espacio global. El verbo vive AQUÍ, una sola
/// vez por gestor: la UI lo recibe por la seam (Snapshot), no lo duplica.
/// Añadir un gestor = añadir una entrada.
pub(crate) struct DefinicionGestor {
    pub(crate) nombre: &'static str,
    /// Comando visible para la UI (tooltip): "npm i -g".
    pub(crate) comando: &'static str,
    /// Argumento de instalación (install/add) que kernel::instalar corre.
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
        .ok_or_else(|| format!("gestor no soportado: {gestor}"))
}

fn validar_gestor(gestor: &str) -> Result<(), String> {
    def_gestor_en(GESTORES, gestor).map(|_| ())
}

// ---- Núcleos (testeables por su interfaz, sin Tauri) ----

/// Descubre el runner y produce la foto del espacio global, con el comando
/// visible del gestor ya ensamblado.
fn correr_snapshot(def: &DefinicionGestor) -> Result<Snapshot, String> {
    let runner = (def.runner)().map_err(|e| e.to_string())?;
    let espacio = (def.snapshot)(runner.as_ref()).map_err(|e| e.to_string())?;
    Ok(espacio.con_comando(def.comando))
}

/// Actualiza un paquete con el verbo del gestor; cada línea de salida sale
/// por `on_line` (el wrapper la streamea como evento `pm-output`).
fn correr_update(
    def: &DefinicionGestor,
    name: &str,
    on_line: &mut dyn FnMut(&str),
) -> Result<UpdateOutcome, String> {
    let runner = (def.runner)().map_err(|e| e.to_string())?;
    kernel::instalar(runner.as_ref(), def.verbo, name, on_line).map_err(|e| e.to_string())
}

/// Excluidos de un gestor; la migración del formato legado corre aquí, una
/// sola vez, cuando el archivo todavía lo es.
fn nucleo_get_excluded(dir: &Path, gestor: &str) -> Result<Vec<String>, String> {
    let (mapa, era_legado) = exclusiones::cargar(dir);
    if era_legado {
        exclusiones::guardar(dir, &mapa).map_err(|e| e.to_string())?;
    }
    Ok(mapa.get(gestor).cloned().unwrap_or_default())
}

fn nucleo_set_excluded(dir: &Path, gestor: &str, nombres: Vec<String>) -> Result<(), String> {
    let (mut mapa, _) = exclusiones::cargar(dir);
    mapa.insert(gestor.to_string(), nombres);
    exclusiones::guardar(dir, &mapa).map_err(|e| e.to_string())
}

// ---- Comandos (wrappers delgados sobre los núcleos) ----

/// Dependencias de los comandos, resueltas una vez en el arranque.
struct Contexto {
    dir_config: PathBuf,
    /// Bandera de "Detener": compartida por la cola activa. Una sola cola
    /// razonable a la vez (los paneles detienen la suya al desmontarse).
    parar: Arc<AtomicBool>,
}

/// Pestañas disponibles: gestores soportados E instalados en esta máquina
/// (chequeo de presencia, sin spawns).
#[tauri::command]
fn gestores_instalados() -> Vec<String> {
    GESTORES
        .iter()
        .filter(|g| (g.instalado)())
        .map(|g| g.nombre.to_string())
        .collect()
}

/// Lista los paquetes globales del espacio del gestor.
/// `outdated` tarda segundos (consulta el registro): corre bloqueado en un
/// hilo del pool para no congelar la IPC.
#[tauri::command]
async fn list_globals(gestor: String) -> Result<Snapshot, String> {
    let def = def_gestor_en(GESTORES, &gestor)?;
    tauri::async_runtime::spawn_blocking(move || correr_snapshot(def))
        .await
        .map_err(|e| e.to_string())?
}

/// Línea de salida de un gestor streameada durante una actualización.
#[derive(Clone, Serialize)]
struct OutputLine {
    gestor: String,
    package: String,
    line: String,
}

/// Actualiza un paquete global a su última versión; cada línea de salida
/// llega como evento `pm-output` para el panel de log.
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

#[tauri::command]
fn set_excluded(
    gestor: String,
    nombres: Vec<String>,
    estado: tauri::State<Contexto>,
) -> Result<(), String> {
    validar_gestor(&gestor)?;
    nucleo_set_excluded(&estado.dir_config, &gestor, nombres)
}

/// «Actualizar todo»: cola secuencial en Rust. Progreso vía `pm-cola`
/// (empieza/resultado por paquete) y `pm-output` (líneas de log); al
/// terminar devuelve resumen + snapshot final (un solo refresco).
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
    let parar = estado.parar.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (resumen, snapshot) = cola::correr(def, &dir, &parar, &mut |ev| match ev {
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
            EventoCola::Resultado {
                paquete,
                exito,
                salida,
            } => {
                let _ = app.emit(
                    "pm-cola",
                    EventoPaquete::resultado(&gestor, paquete, *exito, salida),
                );
            }
        })?;
        Ok(ResultadoActualizarTodo { resumen, snapshot })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Acontecimiento de la cola para una fila de la tabla.
#[derive(Clone, Serialize)]
struct EventoPaquete {
    gestor: String,
    tipo: &'static str, // "empieza" | "resultado"
    paquete: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    exito: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    salida: Option<String>,
}

impl EventoPaquete {
    fn empieza(gestor: &str, paquete: &str) -> Self {
        Self {
            gestor: gestor.into(),
            tipo: "empieza",
            paquete: paquete.into(),
            exito: None,
            salida: None,
        }
    }

    fn resultado(gestor: &str, paquete: &str, exito: bool, salida: &str) -> Self {
        Self {
            gestor: gestor.into(),
            tipo: "resultado",
            paquete: paquete.into(),
            exito: Some(exito),
            salida: Some(salida.to_string()),
        }
    }
}

/// Detiene la cola tras el paquete en curso (graceful).
#[tauri::command]
fn detener_actualizar_todo(estado: tauri::State<Contexto>) {
    estado.parar.store(true, Ordering::Relaxed);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir_config = app
                .path()
                .app_config_dir()
                .map_err(|e| format!("sin directorio de configuración: {e}"))?;
            app.manage(Contexto {
                dir_config,
                parar: Arc::new(AtomicBool::new(false)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_globals,
            update_package,
            get_excluded,
            set_excluded,
            gestores_instalados,
            actualizar_todo,
            detener_actualizar_todo
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::testutil::FakeRunner;

    // ---- dispatch sobre la tabla ----

    #[test]
    fn npm_pnpm_y_bun_soportados() {
        assert!(validar_gestor("npm").is_ok());
        assert!(validar_gestor("pnpm").is_ok());
        assert!(validar_gestor("bun").is_ok());
    }

    #[test]
    fn gestores_no_soportados_rechazados() {
        for g in ["yarn", "", "--force"] {
            assert!(validar_gestor(g).is_err(), "{g} no debe pasar");
        }
    }

    // Tabla de juguete: mismo protocolo que npm, runner falso.
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
        let snap = correr_snapshot(&def_falsa()).expect("snapshot válido");
        assert_eq!(snap.comando_actualizar, "falso i -g"); // verbo: fuente única
        assert_eq!(snap.espacio.version_gestor, "1.0.0");
        assert!(snap.espacio.packages.iter().any(|p| p.outdated));
    }

    #[test]
    fn correr_update_usa_el_verbo_del_def_y_streamea_lineas() {
        let mut lineas = Vec::new();
        let out = correr_update(&def_falsa(), "hunkdiff", &mut |l| {
            lineas.push(l.to_string())
        })
        .expect("update válido");
        assert!(out.success);
        assert_eq!(lineas, vec!["added 1 package in 2s"]);
    }

    #[test]
    fn correr_update_con_gestor_roto_propaga_el_error() {
        let def = DefinicionGestor {
            runner: || {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no hay binario",
                ))
            },
            ..def_falsa()
        };
        let err = correr_update(&def, "hunkdiff", &mut |_| {}).unwrap_err();
        assert!(err.contains("no hay binario"));
    }

    // ---- exclusiones por su núcleo ----

    #[test]
    fn exclusiones_roundtrip_por_gestor_y_migracion_legada_una_vez() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("exclusiones.json"), r#"["hunkdiff"]"#).unwrap();
        // primera lectura: legado → migrado y devuelto como npm
        assert_eq!(
            nucleo_get_excluded(dir.path(), "npm").unwrap(),
            ["hunkdiff"]
        );
        // el archivo ya reescrito en formato mapa
        assert_eq!(
            nucleo_get_excluded(dir.path(), "pnpm").unwrap(),
            Vec::<String>::new()
        );
        nucleo_set_excluded(dir.path(), "pnpm", vec!["cowsay".into()]).unwrap();
        assert_eq!(nucleo_get_excluded(dir.path(), "pnpm").unwrap(), ["cowsay"]);
        assert_eq!(
            nucleo_get_excluded(dir.path(), "npm").unwrap(),
            ["hunkdiff"]
        );
    }

    #[test]
    fn exclusiones_gestor_desconocido_rechazado_por_el_wrapper() {
        assert!(validar_gestor("yarn").is_err());
    }
}
