//! pnpm adapter: its global space is pnpm's own project (independent of
//! nvm); with no packages there is no manifest and the listing is simply
//! empty — a valid state, not an error. The verb (`add`) and the visible
//! command live in [`crate`]'s manager table.

use crate::kernel::{
    armar, con_extension, correr_consulta, correr_instalacion, find_in_path, guardar_path_nvm,
    home, local_app_data, no_encontrado, parse_outdated, primer_existente, version_de,
    EspacioGlobal, Runner, RunnerOutput,
};
use std::path::{Path, PathBuf};

/// pnpm's standard locations (outside the PATH), in order: the `PNPM_HOME`
/// variable if present, and the per-OS defaults — a GUI app does not
/// inherit the shell's PATH, so the defaults save the day. Zero-config:
/// the variable is a bonus, never a requirement.
fn ubicaciones_pnpm() -> Vec<PathBuf> {
    let mut rutas = Vec::new();
    if let Some(v) = std::env::var_os("PNPM_HOME") {
        rutas.push(PathBuf::from(v).join(con_extension("pnpm")));
    }
    if let Some(lad) = local_app_data() {
        rutas.push(lad.join("pnpm").join(con_extension("pnpm"))); // Windows
    }
    if let Some(h) = home() {
        rutas.push(h.join("Library/pnpm/pnpm")); // macOS (PNPM_HOME)
        rutas.push(h.join(".local/share/pnpm/pnpm")); // Linux
    }
    rutas
}

/// Is there a pnpm binary on this machine? Presence check (no spawn):
/// feeds which tabs exist.
pub fn instalado() -> bool {
    find_in_path(&con_extension("pnpm")).is_some() || primer_existente(ubicaciones_pnpm()).is_some()
}

/// pnpm's real runner: binary from the PATH or from pnpm's standard
/// locations. The shim needs node in the PATH: nvm's active version bin
/// is prepended if needed (app opened from Finder).
pub struct RealPnpmRunner {
    bin: PathBuf,
    pnpm_version: String,
    node_version: Option<String>,
}

impl RealPnpmRunner {
    pub fn discover() -> std::io::Result<Self> {
        let buscadas = ubicaciones_pnpm();
        let bin = find_in_path(&con_extension("pnpm"))
            .or_else(|| primer_existente(buscadas.clone()))
            .ok_or_else(|| no_encontrado("pnpm", &buscadas))?;
        // The probes use the same prepended PATH as run(): without it,
        // from Finder the version would silently read "unknown".
        let pnpm_version = version_de(Self::command(&bin, &["--version"]))
            .unwrap_or_else(|| "unknown".to_string());
        let mut probe = std::process::Command::new("node");
        probe.arg("--version");
        guardar_path_nvm(&mut probe);
        let node_version = version_de(probe).map(|v| crate::kernel::sin_v(&v).to_string());
        Ok(Self {
            bin,
            pnpm_version,
            node_version,
        })
    }

    fn command(bin: &Path, args: &[&str]) -> std::process::Command {
        let mut cmd = std::process::Command::new(bin);
        cmd.args(args);
        guardar_path_nvm(&mut cmd);
        cmd
    }
}

impl Runner for RealPnpmRunner {
    fn version_gestor(&self) -> String {
        self.pnpm_version.clone()
    }

    fn version_node(&self) -> Option<String> {
        self.node_version.clone()
    }

    fn run(&self, args: &[&str]) -> std::io::Result<RunnerOutput> {
        correr_consulta(Self::command(&self.bin, args))
    }

    fn run_streaming(
        &self,
        args: &[&str],
        on_line: &mut dyn FnMut(&str),
        parar: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> std::io::Result<RunnerOutput> {
        correr_instalacion(Self::command(&self.bin, args), on_line, parar)
    }
}

/// `pnpm ls -g --depth=0 --json` returns an ARRAY of importers with
/// `dependencies`; empty stdout also means zero globals.
fn parse_ls(json: &str) -> Vec<(String, String)> {
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::Value::Null);
    let mut pares = Vec::new();
    if let serde_json::Value::Array(importers) = parsed {
        for importer in importers {
            let Some(deps) = importer.get("dependencies").and_then(|v| v.as_object()) else {
                continue;
            };
            for (name, fields) in deps {
                if let Some(version) = fields.get("version").and_then(|v| v.as_str()) {
                    pares.push((name.clone(), version.to_string()));
                }
            }
        }
    }
    pares
}

/// Photo of pnpm's global space.
pub fn snapshot(runner: &dyn Runner) -> std::io::Result<EspacioGlobal> {
    let ls = runner.run(&["ls", "-g", "--depth=0", "--json"])?;
    crate::kernel::guard_json(&ls, "pnpm", "ls", '[')?;
    let pares = parse_ls(&ls.stdout);
    if pares.is_empty() {
        // Without globals the global project's manifest does not exist
        // and `outdated` would fail: an empty space is a valid state.
        return Ok(EspacioGlobal {
            version_gestor: runner.version_gestor(),
            version_node: runner.version_node(),
            packages: Vec::new(),
        });
    }
    let out = runner.run(&["outdated", "-g", "--json"])?;
    // Same as npm: exit != 0 without usable JSON is a real failure, not
    // "nothing outdated" (which is exit 0 with empty stdout).
    crate::kernel::guard_json(&out, "pnpm", "outdated", '{')?;
    Ok(EspacioGlobal {
        version_gestor: runner.version_gestor(),
        version_node: runner.version_node(),
        packages: armar(pares, &parse_outdated(&out.stdout)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::testutil::FakeRunner;

    // Fixtures captured from real pnpm 10.33
    const LS_JSON: &str = r#"[{"path":"/Users/ejemplo/Library/pnpm/global/5","private":false,"dependencies":{"cowsay":{"from":"cowsay","version":"1.0.0","resolved":"https://registry.npmjs.org/"},"@org/paquete":{"version":"2.0.0"}}}]"#;
    const OUTDATED_JSON: &str =
        r#"{"cowsay":{"current":"1.0.0","latest":"1.6.0","wanted":"1.0.0","isDeprecated":false}}"#;

    fn runner_pnpm() -> FakeRunner {
        FakeRunner::new("10.33.0")
            .con_node(Some("26.2.0"))
            .respuesta("ls", LS_JSON, 0)
            .respuesta("outdated", OUTDATED_JSON, 0)
            .respuesta("add", "Done in 1.5s", 0)
    }

    #[test]
    fn parse_ls_lee_el_array_de_importers() {
        let pares = parse_ls(LS_JSON);
        assert_eq!(pares.len(), 2);
        assert!(pares.contains(&("cowsay".to_string(), "1.0.0".to_string())));
        assert!(pares.contains(&("@org/paquete".to_string(), "2.0.0".to_string())));
    }

    #[test]
    fn snapshot_detecta_desactualizados_con_scoped() {
        let snap = snapshot(&runner_pnpm()).expect("valid snapshot");
        assert_eq!(snap.version_gestor, "10.33.0");
        assert_eq!(snap.version_node.as_deref(), Some("26.2.0"));
        assert_eq!(snap.packages.len(), 2);
        let cowsay = snap.packages.iter().find(|p| p.name == "cowsay").unwrap();
        assert!(cowsay.outdated);
        assert_eq!(cowsay.latest.as_deref(), Some("1.6.0"));
        let scoped = snap
            .packages
            .iter()
            .find(|p| p.name == "@org/paquete")
            .unwrap();
        assert!(!scoped.outdated);
    }

    #[test]
    fn snapshot_vacio_no_llama_a_outdated() {
        let runner = FakeRunner::new("10.33.0").respuesta("ls", "[]", 0);
        let snap = snapshot(&runner).expect("empty is valid");
        assert!(snap.packages.is_empty());
        assert!(runner.se_llamo_a("ls -g --depth=0 --json"));
        assert!(!runner.se_llamo_a("outdated -g --json"));
    }

    #[test]
    fn snapshot_trata_exit_1_de_outdated_como_valido() {
        let runner = FakeRunner::new("10.33.0")
            .respuesta("ls", LS_JSON, 0)
            .respuesta("outdated", OUTDATED_JSON, 1); // there are outdated
        let snap = snapshot(&runner).expect("exit 1 is not an error");
        assert!(snap.packages.iter().any(|p| p.outdated));
    }
}
