mod bun;
mod exclusiones;
mod npm;
mod pnpm;

use npm::{RealRunner, Runner, Snapshot, UpdateOutcome};
use serde::Serialize;
use tauri::{Emitter, Manager};

/// Un gestor soportado: cómo se descubren sus runners, su snapshot y su
/// actualización. Añadir un gestor = añadir una entrada (bun en su ticket).
struct DefinicionGestor {
    nombre: &'static str,
    instalado: fn() -> bool,
    runner: fn() -> std::io::Result<Box<dyn Runner>>,
    snapshot: fn(&dyn Runner) -> std::io::Result<Snapshot>,
    update: fn(&dyn Runner, &str, &mut dyn FnMut(&str)) -> std::io::Result<UpdateOutcome>,
}

fn runner_npm() -> std::io::Result<Box<dyn Runner>> {
    Ok(Box::new(RealRunner::discover()?))
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
        instalado: || runner_npm().is_ok(),
        runner: runner_npm,
        snapshot: npm::snapshot,
        update: npm::update,
    },
    DefinicionGestor {
        nombre: "pnpm",
        instalado: pnpm::instalado,
        runner: runner_pnpm,
        snapshot: pnpm::snapshot,
        update: pnpm::update,
    },
    DefinicionGestor {
        nombre: "bun",
        instalado: bun::instalado,
        runner: runner_bun,
        snapshot: bun::snapshot,
        update: bun::update,
    },
];

fn def_gestor(gestor: &str) -> Result<&'static DefinicionGestor, String> {
    GESTORES
        .iter()
        .find(|g| g.nombre == gestor)
        .ok_or_else(|| format!("gestor no soportado: {gestor}"))
}

fn validar_gestor(gestor: &str) -> Result<(), String> {
    def_gestor(gestor).map(|_| ())
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
    let def = def_gestor(&gestor)?;
    tauri::async_runtime::spawn_blocking(move || {
        let runner = (def.runner)().map_err(|e| e.to_string())?;
        (def.snapshot)(runner.as_ref()).map_err(|e| e.to_string())
    })
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
    let def = def_gestor(&gestor)?;
    tauri::async_runtime::spawn_blocking(move || {
        let runner = (def.runner)().map_err(|e| e.to_string())?;
        (def.update)(runner.as_ref(), &name, &mut |line| {
            let _ = app.emit("pm-output", OutputLine {
                gestor: gestor.clone(),
                package: name.clone(),
                line: line.to_string(),
            });
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn dir_config(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|e| format!("sin directorio de configuración: {e}"))
}

#[cfg(test)]
mod tests {
    use super::validar_gestor;

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
}

#[tauri::command]
fn get_excluded(gestor: String, app: tauri::AppHandle) -> Result<Vec<String>, String> {
    validar_gestor(&gestor)?;
    let dir = dir_config(&app)?;
    let (mapa, era_legado) = exclusiones::cargar(&dir);
    if era_legado {
        // Migración un-shot: el formato v1 (lista plana) se reescribe como
        // mapa por gestor en cuanto se lee por primera vez.
        exclusiones::guardar(&dir, &mapa).map_err(|e| e.to_string())?;
    }
    Ok(mapa.get(&gestor).cloned().unwrap_or_default())
}

#[tauri::command]
fn set_excluded(
    gestor: String,
    nombres: Vec<String>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    validar_gestor(&gestor)?;
    let dir = dir_config(&app)?;
    let (mut mapa, _) = exclusiones::cargar(&dir);
    mapa.insert(gestor, nombres);
    exclusiones::guardar(&dir, &mapa).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_globals,
            update_package,
            get_excluded,
            set_excluded,
            gestores_instalados
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
