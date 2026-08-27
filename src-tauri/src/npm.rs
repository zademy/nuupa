//! Adapter del gestor npm: cómo descubrir su runner y cómo parsear sus
//! salidas JSON. La ejecución, validación e instalación viven en
//! [`crate::kernel`]; el verbo (`install`) y el comando visible
//! (`npm i -g`) viven en la tabla de gestores de [`crate`].
//!
//! npm corre sobre node: su espacio global pertenece a la versión activa
//! de nvm, y el runner resuelve esa versión (el PATH heredado puede
//! apuntar al npm de OTRO node, p.ej. el de Homebrew).

use crate::kernel::{
    armar, correr, correr_streaming, find_in_path, guardar_path_nvm, resolve_nvm_bin_dir,
    version_de, EspacioGlobal, Runner, RunnerOutput,
};
use std::path::PathBuf;

/// ¿Hay npm en esta máquina? Chequeo de presencia (sin spawn): nvm con al
/// menos una versión, o npm en el PATH. Alimenta qué pestañas existen.
pub fn instalado() -> bool {
    let nvm = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".nvm"));
    nvm.as_deref().and_then(resolve_nvm_bin_dir).is_some() || find_in_path("npm").is_some()
}

/// Runner real: ejecuta el npm de la versión activa de node.
pub struct RealRunner {
    bin_dir: PathBuf,
    npm_version: String,
    node_version: Option<String>,
}

impl RealRunner {
    /// Descubre el npm de la versión activa de node. nvm es la fuente
    /// autoritativa; el PATH queda como fallback cuando no hay nvm.
    pub fn discover() -> std::io::Result<Self> {
        let nvm = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".nvm"));
        let bin_dir = nvm
            .as_deref()
            .and_then(resolve_nvm_bin_dir)
            .map(Ok)
            .unwrap_or_else(|| {
                find_in_path("npm")
                    .map(|p| p.parent().map(PathBuf::from).unwrap_or(p))
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "npm no encontrado: ni en ~/.nvm ni en PATH",
                        )
                    })
            })?;
        Ok(Self::de_bin_dir(bin_dir))
    }

    fn de_bin_dir(bin_dir: PathBuf) -> Self {
        // El espacio global de npm lo define la versión de NODE activa; la
        // versión del gestor es el otro hecho que la statusbar muestra.
        let node_version =
            version_de(std::process::Command::new(bin_dir.join("node")).arg("--version"))
                .map(|v| crate::kernel::sin_v(&v).to_string());
        let npm_version =
            version_de(std::process::Command::new(bin_dir.join("npm")).arg("--version"))
                .unwrap_or_else(|| "desconocida".to_string());
        Self {
            bin_dir,
            npm_version,
            node_version,
        }
    }

    /// Comando npm listo con el PATH de la versión resuelta antepuesto:
    /// el shim de npm lleva shebang `#!/usr/bin/env node`, así encuentra su
    /// node aunque el PATH heredado no lo tenga (app abierta desde Finder).
    fn command(&self, args: &[&str]) -> std::process::Command {
        let mut cmd = std::process::Command::new(self.bin_dir.join("npm"));
        cmd.args(args);
        guardar_path_nvm(&mut cmd);
        cmd
    }
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
