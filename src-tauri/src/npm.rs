//! Adapter del gestor npm: cómo descubrir su runner y cómo parsear sus
//! salidas JSON. La ejecución, validación e instalación viven en
//! [`crate::kernel`]; el verbo (`install`) y el comando visible
//! (`npm i -g`) viven en la tabla de gestores de [`crate`].
//!
//! npm corre sobre node: su espacio global pertenece a la versión activa
//! de nvm, y el runner resuelve esa versión (el PATH heredado puede
//! apuntar al npm de OTRO node, p.ej. el de Homebrew).

#[cfg(windows)]
use crate::kernel::program_files;
use crate::kernel::{
    armar, con_extension, correr, correr_streaming, find_in_path, guardar_path_nvm, home,
    no_encontrado, resolve_nvm_bin_dir, version_de, EspacioGlobal, Runner, RunnerOutput,
};
use std::path::{Path, PathBuf};

/// El ejecutable de npm: en POSIX es el shim `npm`; en Windows es
/// `npm.cmd` (un script de cmd que CreateProcess no puede ejecutar
/// directo — ver [`RealRunner`]).
#[cfg(windows)]
const NPM_BIN: &str = "npm.cmd";
#[cfg(not(windows))]
const NPM_BIN: &str = "npm";

/// Dónde puede estar npm: devuelve el directorio bin resuelto (si hay) y
/// las rutas exploradas fuera del PATH (alimentan el error visible).
#[cfg(not(windows))]
fn ubicaciones_npm() -> (Option<PathBuf>, Vec<PathBuf>) {
    let mut buscadas = Vec::new();
    // nvm es la fuente autoritativa en POSIX: el PATH heredado puede
    // apuntar al npm de OTRO node (p.ej. el de Homebrew).
    if let Some(bin_dir) = home().and_then(|h| resolve_nvm_bin_dir(&h.join(".nvm"))) {
        return (Some(bin_dir), buscadas);
    }
    if let Some(h) = home() {
        buscadas.push(h.join(".nvm"));
    }
    (
        find_in_path(NPM_BIN).and_then(|p| p.parent().map(PathBuf::from)),
        buscadas,
    )
}

/// Windows: npm es un shim del instalador de node.js (estándar:
/// `%ProgramFiles%\nodejs`). nvm-windows publica su versión activa por
/// symlink YA en el PATH: sin resolución propia (defer hasta un reporte
/// real de usuario).
#[cfg(windows)]
fn ubicaciones_npm() -> (Option<PathBuf>, Vec<PathBuf>) {
    let mut buscadas = Vec::new();
    if let Some(pf) = program_files() {
        buscadas.push(pf.join("nodejs"));
    }
    let bin = find_in_path(NPM_BIN)
        .or_else(|| crate::kernel::primer_existente(buscadas.clone()).map(|dir| dir.join(NPM_BIN)));
    (bin.and_then(|b| b.parent().map(PathBuf::from)), buscadas)
}

/// ¿Hay npm en esta máquina? Chequeo de presencia (sin spawn). Alimenta
/// qué pestañas existen.
pub fn instalado() -> bool {
    ubicaciones_npm().0.is_some()
}

/// Runner real: ejecuta el npm de la versión activa de node.
pub struct RealRunner {
    bin_dir: PathBuf,
    npm_version: String,
    node_version: Option<String>,
}

impl RealRunner {
    /// Descubre el npm de esta máquina y resuelve sus versiones.
    pub fn discover() -> std::io::Result<Self> {
        let (bin_dir, buscadas) = ubicaciones_npm();
        let bin_dir = bin_dir.ok_or_else(|| no_encontrado("npm", &buscadas))?;
        Ok(Self::de_bin_dir(bin_dir))
    }

    fn de_bin_dir(bin_dir: PathBuf) -> Self {
        // El espacio global de npm lo define la versión de NODE activa; la
        // versión del gestor es el otro hecho que la statusbar muestra.
        let node_version = version_de(
            std::process::Command::new(bin_dir.join(con_extension("node"))).arg("--version"),
        )
        .map(|v| crate::kernel::sin_v(&v).to_string());
        let npm_version = version_de(&mut comando_npm(&bin_dir, &["--version"]))
            .unwrap_or_else(|| "desconocida".to_string());
        Self {
            bin_dir,
            npm_version,
            node_version,
        }
    }

    /// Comando npm listo para correr.
    fn command(&self, args: &[&str]) -> std::process::Command {
        comando_npm(&self.bin_dir, args)
    }
}

/// Arma el comando npm sobre un directorio bin resuelto.
///
/// POSIX: el shim `npm` lleva shebang `#!/usr/bin/env node`; se antepone
/// el PATH de la versión de nvm para que encuentre su node aunque la app
/// GUI no herede el PATH del shell.
///
/// Windows: npm es `npm.cmd` y CreateProcess no ejecuta scripts de cmd —
/// se invoca `node.exe npm-cli.js`, exactamente lo que el shim hace
/// por dentro (layout estable del instalador de node.js).
#[cfg(not(windows))]
fn comando_npm(bin_dir: &Path, args: &[&str]) -> std::process::Command {
    let mut cmd = std::process::Command::new(bin_dir.join(NPM_BIN));
    cmd.args(args);
    guardar_path_nvm(&mut cmd);
    cmd
}

#[cfg(windows)]
fn comando_npm(bin_dir: &Path, args: &[&str]) -> std::process::Command {
    let mut cmd = std::process::Command::new(bin_dir.join("node.exe"));
    cmd.arg(bin_dir.join("node_modules/npm/bin/npm-cli.js"));
    cmd.args(args);
    cmd
}

impl Runner for RealRunner {
    fn version_gestor(&self) -> String {
        self.npm_version.clone()
    }

    fn version_node(&self) -> Option<String> {
        self.node_version.clone()
    }

    fn run(&self, args: &[&str]) -> std::io::Result<RunnerOutput> {
        correr(&mut self.command(args))
    }

    fn run_streaming(
        &self,
        args: &[&str],
        on_line: &mut dyn FnMut(&str),
    ) -> std::io::Result<RunnerOutput> {
        correr_streaming(self.command(args), on_line)
    }
}

/// `npm ls -g --depth=0 --json` devuelve un objeto con `dependencies`.
fn parse_ls(json: &str) -> Vec<(String, String)> {
    let ls: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::Value::Null);
    ls.get("dependencies")
        .and_then(|v| v.as_object())
        .map(|deps| {
            deps.iter()
                .filter_map(|(name, fields)| {
                    fields
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|v| (name.clone(), v.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Foto del espacio global de npm (versión activa de node incluida).
pub fn snapshot(runner: &dyn Runner) -> std::io::Result<EspacioGlobal> {
    let ls = runner.run(&["ls", "-g", "--depth=0", "--json"])?;
    crate::kernel::guard_json(&ls, "npm", "ls", '{')?;
    let outdated = runner.run(&["outdated", "-g", "--json"])?;
    // exit != 0 sin JSON utilizable es fallo real (red/registro) — NO
    // "ninguno desactualizado", que es exit 0 con stdout vacío.
    crate::kernel::guard_json(&outdated, "npm", "outdated", '{')?;
    Ok(EspacioGlobal {
        version_gestor: runner.version_gestor(),
        version_node: runner.version_node(),
        packages: armar(
            parse_ls(&ls.stdout),
            &crate::kernel::parse_outdated(&outdated.stdout),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::testutil::FakeRunner;

    // Fixtures capturados de npm reales
    const LS_JSON: &str = r#"{
        "dependencies": {
            "@alibaba-group/open-code-review": {"version": "1.10.2"},
            "context-mode": {"version": "1.0.169"},
            "hunkdiff": {"version": "0.17.2"}
        }
    }"#;
    const OUTDATED_JSON: &str = r#"{
        "context-mode": {"current": "1.0.169", "wanted": "1.0.169", "latest": "1.0.170"},
        "hunkdiff": {"current": "0.17.2", "wanted": "0.17.3", "latest": "0.18.0"}
    }"#;

    fn runner_npm() -> FakeRunner {
        FakeRunner::new("11.4.2")
            .con_node(Some("26.2.0"))
            .respuesta("ls", LS_JSON, 0)
            .respuesta("outdated", OUTDATED_JSON, 0)
            .respuesta("install", "added 1 package in 2s", 0)
    }

    #[test]
    fn parse_ls_toma_las_dependencies_del_objeto() {
        let pares = parse_ls(LS_JSON);
        assert_eq!(pares.len(), 3);
        assert!(pares.contains(&("hunkdiff".to_string(), "0.17.2".to_string())));
    }

    #[test]
    fn parse_ls_ignora_entradas_sin_version() {
        let ls = r#"{"dependencies": {"roto": {"missing": true}, "sano": {"version": "1.0.0"}}}"#;
        let pares = parse_ls(ls);
        assert_eq!(pares, vec![("sano".to_string(), "1.0.0".to_string())]);
    }

    #[test]
    fn snapshot_detecta_desactualizados_y_reporta_amblas_versiones() {
        let snap = snapshot(&runner_npm()).expect("snapshot válido");
        assert_eq!(snap.version_gestor, "11.4.2");
        assert_eq!(snap.version_node.as_deref(), Some("26.2.0"));
        assert_eq!(snap.packages.len(), 3);
        let hunkdiff = snap.packages.iter().find(|p| p.name == "hunkdiff").unwrap();
        assert!(hunkdiff.outdated);
        assert_eq!(hunkdiff.latest.as_deref(), Some("0.18.0"));
    }

    #[test]
    fn snapshot_con_outdated_vacio_deja_todo_al_dia() {
        let runner = FakeRunner::new("11.4.2")
            .respuesta("ls", LS_JSON, 0)
            .respuesta("outdated", "", 0);
        let snap = snapshot(&runner).expect("vacío válido");
        assert_eq!(snap.packages.len(), 3);
        assert!(snap.packages.iter().all(|p| !p.outdated));
    }

    #[test]
    fn snapshot_trata_exit_1_de_outdated_como_valido() {
        let runner = FakeRunner::new("11.4.2")
            .respuesta("ls", LS_JSON, 0)
            .respuesta("outdated", OUTDATED_JSON, 1); // npm outdated: 1 = hay desactualizados
        let snap = snapshot(&runner).expect("exit 1 no es error");
        assert!(snap.packages.iter().any(|p| p.outdated));
    }

    #[test]
    fn snapshot_falla_si_ls_no_produce_json() {
        let runner = FakeRunner::new("11.4.2").respuesta("ls", "ENOTFOUND registry", 1);
        assert!(snapshot(&runner).is_err());
    }

    #[test]
    fn snapshot_falla_si_outdated_falla_sin_json() {
        // exit 1 + stdout vacío = fallo de red, NO "ninguno desactualizado"
        // (que es exit 0 + stdout vacío).
        let runner = FakeRunner::new("11.4.2")
            .respuesta("ls", LS_JSON, 0)
            .respuesta("outdated", "", 1);
        assert!(snapshot(&runner).is_err());
    }

    #[test]
    fn snapshot_propaga_fallo_de_spawn() {
        struct Roto;
        impl Runner for Roto {
            fn version_gestor(&self) -> String {
                "11.4.2".into()
            }
            fn run(&self, _args: &[&str]) -> std::io::Result<RunnerOutput> {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "spawn"))
            }
        }
        assert!(snapshot(&Roto).is_err());
    }
}
