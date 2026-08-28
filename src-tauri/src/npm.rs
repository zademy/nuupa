//! npm adapter: how to discover its runner and how to parse its JSON
//! outputs. Execution, validation and installation live in
//! [`crate::kernel`]; the verb (`install`) and the visible command
//! (`npm i -g`) live in [`crate`]'s manager table.
//!
//! npm runs on node: its global space belongs to nvm's active version,
//! and the runner resolves that version (the inherited PATH may point to
//! ANOTHER node's npm, e.g. Homebrew's).

#[cfg(windows)]
use crate::kernel::program_files;
use crate::kernel::{
    armar, con_extension, correr_consulta, correr_instalacion, find_in_path, guardar_path_nvm,
    home, no_encontrado, resolve_nvm_bin_dir, version_de, EspacioGlobal, Runner, RunnerOutput,
};
use std::path::{Path, PathBuf};

/// npm's executable: on POSIX it is the `npm` shim; on Windows it is
/// `npm.cmd` (a cmd script that CreateProcess cannot run directly — see
/// [`RealRunner`]).
#[cfg(windows)]
const NPM_BIN: &str = "npm.cmd";
#[cfg(not(windows))]
const NPM_BIN: &str = "npm";

/// Where npm can be: returns the resolved bin directory (if any) and the
/// paths searched outside the PATH (they feed the visible error).
#[cfg(not(windows))]
fn ubicaciones_npm() -> (Option<PathBuf>, Vec<PathBuf>) {
    let mut buscadas = Vec::new();
    // nvm is the authoritative source on POSIX: the inherited PATH may
    // point to ANOTHER node's npm (e.g. Homebrew's).
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

/// Windows: npm is a shim of the node.js installer (standard location:
/// `%ProgramFiles%\nodejs`). nvm-windows publishes its active version by
/// symlink ALREADY in the PATH: no own resolution (deferred until a real
/// user report).
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

/// Is npm on this machine? Presence check (no spawn). Feeds which tabs
/// exist.
pub fn instalado() -> bool {
    ubicaciones_npm().0.is_some()
}

/// Real runner: executes the npm of node's active version.
pub struct RealRunner {
    bin_dir: PathBuf,
    npm_version: String,
    node_version: Option<String>,
}

impl RealRunner {
    /// Discovers this machine's npm and resolves its versions.
    pub fn discover() -> std::io::Result<Self> {
        let (bin_dir, buscadas) = ubicaciones_npm();
        let bin_dir = bin_dir.ok_or_else(|| no_encontrado("npm", &buscadas))?;
        Ok(Self::de_bin_dir(bin_dir))
    }

    fn de_bin_dir(bin_dir: PathBuf) -> Self {
        // npm's global space is defined by the active NODE version; the
        // manager version is the other fact the statusbar shows.
        let mut probe_node = std::process::Command::new(bin_dir.join(con_extension("node")));
        probe_node.arg("--version");
        let node_version = version_de(probe_node).map(|v| crate::kernel::sin_v(&v).to_string());
        let npm_version = version_de(comando_npm(&bin_dir, &["--version"]))
            .unwrap_or_else(|| "unknown".to_string());
        Self {
            bin_dir,
            npm_version,
            node_version,
        }
    }

    /// An npm command ready to run.
    fn command(&self, args: &[&str]) -> std::process::Command {
        comando_npm(&self.bin_dir, args)
    }
}

/// Builds the npm command over a resolved bin directory.
///
/// POSIX: the `npm` shim carries the `#!/usr/bin/env node` shebang; nvm's
/// version PATH is prepended so it finds its node even when the GUI app
/// does not inherit the shell's PATH.
///
/// Windows: npm is `npm.cmd` and CreateProcess cannot run cmd scripts —
/// `node.exe npm-cli.js` is invoked, exactly what the shim does
/// internally (stable node.js installer layout).
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
        correr_consulta(self.command(args))
    }

    fn run_streaming(
        &self,
        args: &[&str],
        on_line: &mut dyn FnMut(&str),
        parar: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> std::io::Result<RunnerOutput> {
        correr_instalacion(self.command(args), on_line, parar)
    }
}

/// `npm ls -g --depth=0 --json` returns an object with `dependencies`.
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

/// Photo of npm's global space (active node version included).
pub fn snapshot(runner: &dyn Runner) -> std::io::Result<EspacioGlobal> {
    let ls = runner.run(&["ls", "-g", "--depth=0", "--json"])?;
    crate::kernel::guard_json(&ls, "npm", "ls", '{')?;
    let outdated = runner.run(&["outdated", "-g", "--json"])?;
    // exit != 0 without usable JSON is a real failure (network/registry)
    // — NOT "nothing outdated", which is exit 0 with empty stdout.
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

    // Fixtures captured from real npm
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
        let snap = snapshot(&runner_npm()).expect("valid snapshot");
        assert_eq!(snap.version_gestor, "11.4.2");
        assert_eq!(snap.version_node.as_deref(), Some("26.2.0"));
        assert_eq!(snap.packages.len(), 3);
        let hunkdiff = snap.packages.iter().find(|p| p.name == "hunkdiff").unwrap();
        assert!(hunkdiff.outdated);
        assert_eq!(hunkdiff.latest.as_deref(), Some("0.18.0"));
    }

    #[test]
    fn snapshot_filtra_al_propio_npm_de_la_lista() {
        // npm itself shows up in its own `ls` output: it never reaches
        // the table ("Paquete del gestor" in the glossary).
        let ls = r#"{"dependencies": {"npm": {"version": "11.4.2"}, "hunkdiff": {"version": "0.17.2"}}}"#;
        let runner = FakeRunner::new("11.4.2")
            .respuesta("ls", ls, 0)
            .respuesta("outdated", "", 0);
        let snap = snapshot(&runner).expect("valid snapshot");
        assert_eq!(snap.packages.len(), 1);
        assert_eq!(snap.packages[0].name, "hunkdiff");
    }

    #[test]
    fn snapshot_con_outdated_vacio_deja_todo_al_dia() {
        let runner = FakeRunner::new("11.4.2")
            .respuesta("ls", LS_JSON, 0)
            .respuesta("outdated", "", 0);
        let snap = snapshot(&runner).expect("empty is valid");
        assert_eq!(snap.packages.len(), 3);
        assert!(snap.packages.iter().all(|p| !p.outdated));
    }

    #[test]
    fn snapshot_trata_exit_1_de_outdated_como_valido() {
        let runner = FakeRunner::new("11.4.2")
            .respuesta("ls", LS_JSON, 0)
            .respuesta("outdated", OUTDATED_JSON, 1); // npm outdated: 1 = there are outdated
        let snap = snapshot(&runner).expect("exit 1 is not an error");
        assert!(snap.packages.iter().any(|p| p.outdated));
    }

    #[test]
    fn snapshot_falla_si_ls_no_produce_json() {
        let runner = FakeRunner::new("11.4.2").respuesta("ls", "ENOTFOUND registry", 1);
        assert!(snapshot(&runner).is_err());
    }

    #[test]
    fn snapshot_falla_si_outdated_falla_sin_json() {
        // exit 1 + empty stdout = network failure, NOT "nothing outdated"
        // (which is exit 0 + empty stdout).
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
