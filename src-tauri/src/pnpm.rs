//! Adapter del gestor pnpm: su espacio global es un proyecto propio de
//! pnpm (independiente de nvm); sin paquetes no hay manifiesto y el
//! listado simplemente está vacío — estado válido, no error. El verbo
//! (`add`) y el comando visible viven en la tabla de gestores de [`crate`].

use crate::kernel::{
    armar, con_extension, correr, correr_streaming, find_in_path, guardar_path_nvm, home,
    local_app_data, no_encontrado, parse_outdated, primer_existente, version_de, EspacioGlobal,
    Runner, RunnerOutput,
};
use std::path::{Path, PathBuf};

/// Ubicaciones estándar de pnpm (fuera del PATH), en orden: la variable
/// `PNPM_HOME` si existe, y los defaults por SO — una app GUI no hereda el
/// PATH del shell, así que los defaults salvan. Cero-config: la var es
/// bonus, nunca requisito.
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

/// ¿Hay binario de pnpm en esta máquina? Chequeo de presencia (sin spawn):
/// alimenta qué pestañas existen.
pub fn instalado() -> bool {
    find_in_path(&con_extension("pnpm")).is_some() || primer_existente(ubicaciones_pnpm()).is_some()
}

/// Runner real de pnpm: binario del PATH o de las ubicaciones estándar de
/// pnpm. El shim necesita node en el PATH: se antepone el bin de la
/// versión activa de nvm si hace falta (app abierta desde Finder).
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
        // Los probes usan el mismo PATH antepuesto que run(): sin eso,
        // desde Finder la versión saldría "desconocida" en silencio.
        let pnpm_version = version_de(&mut Self::command(&bin, &["--version"]))
            .unwrap_or_else(|| "desconocida".to_string());
        let mut probe = std::process::Command::new("node");
        probe.arg("--version");
        guardar_path_nvm(&mut probe);
        let node_version = version_de(&mut probe).map(|v| crate::kernel::sin_v(&v).to_string());
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
        correr(&mut Self::command(&self.bin, args))
    }

    fn run_streaming(
        &self,
        args: &[&str],
        on_line: &mut dyn FnMut(&str),
    ) -> std::io::Result<RunnerOutput> {
        correr_streaming(Self::command(&self.bin, args), on_line)
    }
}

/// `pnpm ls -g --depth=0 --json` devuelve un ARRAY de importers con
/// `dependencies`; stdout vacío también significa cero globales.
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

/// Foto del espacio global de pnpm.
pub fn snapshot(runner: &dyn Runner) -> std::io::Result<EspacioGlobal> {
    let ls = runner.run(&["ls", "-g", "--depth=0", "--json"])?;
    crate::kernel::guard_json(&ls, "pnpm", "ls", '[')?;
    let pares = parse_ls(&ls.stdout);
    if pares.is_empty() {
        // Sin globales no existe el manifiesto del proyecto global y
        // `outdated` fallaría: espacio vacío es un estado válido.
        return Ok(EspacioGlobal {
            version_gestor: runner.version_gestor(),
            version_node: runner.version_node(),
            packages: Vec::new(),
        });
    }
    let out = runner.run(&["outdated", "-g", "--json"])?;
    // Igual que npm: exit != 0 sin JSON utilizable es fallo real, no
    // "ninguno desactualizado" (que es exit 0 con stdout vacío).
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

    // Fixtures capturados de pnpm 10.33 reales
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
        let snap = snapshot(&runner_pnpm()).expect("snapshot válido");
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
        let snap = snapshot(&runner).expect("vacío es válido");
        assert!(snap.packages.is_empty());
        assert!(runner.se_llamo_a("ls -g --depth=0 --json"));
        assert!(!runner.se_llamo_a("outdated -g --json"));
    }

    #[test]
    fn snapshot_trata_exit_1_de_outdated_como_valido() {
        let runner = FakeRunner::new("10.33.0")
            .respuesta("ls", LS_JSON, 0)
            .respuesta("outdated", OUTDATED_JSON, 1); // hay desactualizados
        let snap = snapshot(&runner).expect("exit 1 no es error");
        assert!(snap.packages.iter().any(|p| p.outdated));
    }
}
