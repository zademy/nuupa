//! Paquetes globales npm de la versión activa de node.
//!
//! Seam de testing: [`Runner`] es el único punto por donde el proceso npm
//! real entra al sistema; todo lo demás (fusión, detección de desactualizado)
//! se prueba por encima con un runner falso.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Paquete global npm: nombre, versión instalada, última versión conocida y
/// si está desactualizado (calculado en Rust: la UI no re-deriva).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalPackage {
    pub name: String,
    pub installed: String,
    pub latest: Option<String>,
    pub outdated: bool,
}

/// Foto del estado global: versión activa de node + paquetes globales.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: String,
    pub packages: Vec<GlobalPackage>,
}

/// Salida de un comando npm: stdout, stderr y código de salida.
///
/// Contrato: `npm outdated` termina con código 1 cuando HAY paquetes
/// desactualizados — es un resultado válido, no un error. Solo el fallo de
/// spawn (o un fallo duro de npm sin salida útil) se propaga como `Err`.
pub struct RunnerOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Seam: único punto por donde el proceso npm real entra al sistema.
pub trait Runner {
    fn version(&self) -> String;
    fn run(&self, args: &[&str]) -> std::io::Result<RunnerOutput>;

    /// Como `run`, pero streamea cada línea de salida a `on_line` conforme
    /// llega. Por defecto corre `run` y emite las líneas ya completas — el
    /// runner real lo sobreescribe con pipes.
    fn run_streaming(
        &self,
        args: &[&str],
        on_line: &mut dyn FnMut(&str),
    ) -> std::io::Result<RunnerOutput> {
        let out = self.run(args)?;
        for line in out.stdout.lines().chain(out.stderr.lines()) {
            on_line(line);
        }
        Ok(out)
    }
}

/// Parsea `outdated --json` (shape compartido por npm y pnpm:
/// { paquete: { latest, ... } } → mapa nombre→última). Solo lista los
/// paquetes con actualización disponible: los ausentes están al día.
pub(crate) fn parse_outdated(json: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::Value::Null);
    if let serde_json::Value::Object(obj) = parsed {
        for (name, fields) in obj {
            if let Some(latest) = fields.get("latest").and_then(|v| v.as_str()) {
                map.insert(name, latest.to_string());
            }
        }
    }
    map
}

/// Arma la lista final de paquetes: (nombre, instalada) + outdated →
/// latest heredada si está al día. Compartido por los gestores.
pub(crate) fn armar(
    pares: Vec<(String, String)>,
    outdated: &BTreeMap<String, String>,
) -> Vec<GlobalPackage> {
    let mut packages = pares
        .into_iter()
        .map(|(name, installed)| {
            let latest = outdated
                .get(&name)
                .cloned()
                .unwrap_or_else(|| installed.clone());
            let outdated_flag = latest != installed;
            GlobalPackage {
                name,
                installed,
                latest: Some(latest),
                outdated: outdated_flag,
            }
        })
        .collect::<Vec<_>>();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    packages
}

/// Fusiona la salida de `npm ls -g --depth=0 --json` con la de
/// `npm outdated -g --json`.
pub fn merge(ls_json: &str, outdated_json: &str) -> Vec<GlobalPackage> {
    let ls: serde_json::Value = serde_json::from_str(ls_json).unwrap_or(serde_json::Value::Null);
    let pares = ls
        .get("dependencies")
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
        .unwrap_or_default();
    armar(pares, &parse_outdated(outdated_json))
}

/// Resultado de actualizar un paquete global.
#[derive(Debug, Serialize)]
pub struct UpdateOutcome {
    pub success: bool,
    pub output: String,
}

/// Valida un nombre de paquete: nunca un flag disfrazado (viaja como
/// argumento del proceso, sin shell, pero igual).
pub(crate) fn validar_nombre(name: &str) -> std::io::Result<()> {
    if name.is_empty() || name.starts_with('-') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("nombre de paquete inválido: {name:?}"),
        ));
    }
    Ok(())
}

/// Instala la última versión de un paquete global con el verbo del gestor
/// (npm: install · pnpm: add), streameando cada línea a `on_line`.
pub(crate) fn instalar(
    runner: &dyn Runner,
    verbo: &str,
    name: &str,
    on_line: &mut dyn FnMut(&str),
) -> std::io::Result<UpdateOutcome> {
    validar_nombre(name)?;
    let spec = format!("{name}@latest");
    let out = runner.run_streaming(&[verbo, "-g", &spec], on_line)?;
    let mut output = out.stdout;
    if !out.stderr.trim().is_empty() {
        output.push('\n');
        output.push_str(&out.stderr);
    }
    Ok(UpdateOutcome {
        success: out.exit_code == 0,
        output,
    })
}

/// Actualiza un paquete global a su última versión (`npm install -g
/// <paquete>@latest`).
pub fn update(
    runner: &dyn Runner,
    name: &str,
    on_line: &mut dyn FnMut(&str),
) -> std::io::Result<UpdateOutcome> {
    instalar(runner, "install", name, on_line)
}

/// Fallo duro de un comando JSON: código de error y la salida no empieza
/// con el carácter esperado. Compartido por los gestores.
pub(crate) fn guard_json(
    out: &RunnerOutput,
    gestor: &str,
    comando: &str,
    abre: char,
) -> std::io::Result<()> {
    if out.exit_code != 0 && !out.stdout.trim_start().starts_with(abre) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "{gestor} {comando} falló (exit {}): {}",
                out.exit_code,
                out.stderr.trim()
            ),
        ));
    }
    Ok(())
}

/// Corre un comando con pipes y streamea cada línea de stdout a `on_line`
/// conforme llega; stderr se lee en un hilo aparte (no se streamea: el
/// progreso de los gestores va contaminado de secuencias de control).
/// Compartido por los runners reales.
pub(crate) fn correr_streaming(
    mut cmd: std::process::Command,
    on_line: &mut dyn FnMut(&str),
) -> std::io::Result<RunnerOutput> {
    use std::io::{BufRead, BufReader};
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        for line in BufReader::new(stderr).lines().flatten() {
            buf.push_str(&line);
            buf.push('\n');
        }
        buf
    });

    let mut stdout_str = String::new();
    for line in BufReader::new(stdout).lines() {
        let line = line?;
        on_line(&line);
        stdout_str.push_str(&line);
        stdout_str.push('\n');
    }
    let stderr_str = stderr_handle.join().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::Other, "hilo de stderr murió")
    })?;
    let status = child.wait()?;
    Ok(RunnerOutput {
        stdout: stdout_str,
        stderr: stderr_str,
        exit_code: status.code().unwrap_or(-1),
    })
}

/// Ejecuta ls + outdated con el runner dado y fusiona en un [`Snapshot`].
pub fn snapshot(runner: &dyn Runner) -> std::io::Result<Snapshot> {
    let ls = runner.run(&["ls", "-g", "--depth=0", "--json"])?;
    guard_json(&ls, "npm", "ls", '{')?;
    let outdated = runner.run(&["outdated", "-g", "--json"])?;
    // exit != 0 sin JSON utilizable es fallo real (red/registro) — NO
    // "ninguno desactualizado", que es exit 0 con stdout vacío.
    guard_json(&outdated, "npm", "outdated", '{')?;
    Ok(Snapshot {
        version: runner.version(),
        packages: merge(&ls.stdout, &outdated.stdout),
    })
}

/// Resuelve el directorio bin de node/npm de la versión activa de nvm:
/// alias `default` (siguiendo cadenas tipo `node` → `lts/iron`) si termina
/// en una versión instalada, si no la versión mayor más reciente.
pub fn resolve_nvm_bin_dir(nvm_dir: &Path) -> Option<PathBuf> {
    let versions_dir = nvm_dir.join("versions").join("node");
    let mut versions: Vec<String> = std::fs::read_dir(&versions_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .map(|name| strip_v(&name).to_string())
        .collect();
    if versions.is_empty() {
        return None;
    }
    versions.sort_by(|a, b| version_key(a).cmp(&version_key(b)));

    let chosen = resolve_alias(nvm_dir, "default", 5)
        .filter(|d| versions.contains(d))
        .unwrap_or_else(|| versions.pop().expect("versions no vacío"));
    Some(versions_dir.join(format!("v{chosen}")).join("bin"))
}

/// Sigue una cadena de alias de nvm (`default` → `node` → `lts/iron` →
/// versión) hasta dar con un número de versión.
fn resolve_alias(nvm_dir: &Path, alias: &str, depth: u32) -> Option<String> {
    if depth == 0 {
        return None;
    }
    let raw = std::fs::read_to_string(nvm_dir.join("alias").join(alias)).ok()?;
    let raw = strip_v(raw.trim());
    if raw.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        Some(raw.to_string())
    } else {
        resolve_alias(nvm_dir, raw, depth - 1)
    }
}

fn strip_v(s: &str) -> &str {
    s.trim().trim_start_matches('v')
}

fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Runner real: ejecuta el npm de la versión activa de node.
pub struct RealRunner {
    bin_dir: PathBuf,
    version: String,
}

impl RealRunner {
    /// Descubre el npm de la versión activa de node. nvm es la fuente
    /// autoritativa: el PATH heredado puede apuntar al npm de OTRO node (p.ej.
    /// el de Homebrew en /usr/local/bin, visible incluso desde Finder); el
    /// PATH queda como fallback cuando no hay nvm.
    pub fn discover() -> std::io::Result<Self> {
        let nvm = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".nvm"));
        if let Some(bin_dir) = nvm.as_deref().and_then(resolve_nvm_bin_dir) {
            return Ok(Self::from_bin_dir(bin_dir));
        }
        find_in_path("npm")
            .map(Self::from_bin_dir)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "npm no encontrado: ni en ~/.nvm ni en PATH",
                )
            })
    }

    fn from_bin_dir(bin_dir: PathBuf) -> Self {
        Self {
            version: probe_version(&bin_dir),
            bin_dir,
        }
    }
}

impl RealRunner {
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
    fn version(&self) -> String {
        self.version.clone()
    }

    fn run(&self, args: &[&str]) -> std::io::Result<RunnerOutput> {
        let out = self.command(args).output()?;
        Ok(RunnerOutput {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            exit_code: out.status.code().unwrap_or(-1),
        })
    }

    fn run_streaming(
        &self,
        args: &[&str],
        on_line: &mut dyn FnMut(&str),
    ) -> std::io::Result<RunnerOutput> {
        correr_streaming(self.command(args), on_line)
    }
}

/// Antepone el bin de node de la versión activa de nvm al PATH del
/// comando: los shims (npm, pnpm) llevan `#!/usr/bin/env node` y una app
/// GUI abierta desde Finder no hereda el PATH del shell.
pub(crate) fn guardar_path_nvm(cmd: &mut std::process::Command) {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(bin_dir) = home.and_then(|h| resolve_nvm_bin_dir(&h.join(".nvm"))) {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut nueva = bin_dir.into_os_string();
        nueva.push(":");
        nueva.push(&path);
        cmd.env("PATH", nueva);
    }
}

pub(crate) fn find_in_path(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|candidate| candidate.is_file())
}

fn probe_version(bin_dir: &Path) -> String {
    std::process::Command::new(bin_dir.join("node"))
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| strip_v(String::from_utf8_lossy(&o.stdout).trim()).to_string())
        .unwrap_or_else(|| "desconocida".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    struct FakeRunner {
        node: String,
        ls: String,
        ls_exit: i32,
        outdated: String,
        outdated_exit: i32,
        install_exit: i32,
        install_out: String,
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl FakeRunner {
        fn new(ls: &str, outdated: &str) -> Self {
            Self {
                node: "26.2.0".into(),
                ls: ls.into(),
                ls_exit: 0,
                outdated: outdated.into(),
                outdated_exit: 0,
                install_exit: 0,
                install_out: "added 1 package in 2s".into(),
                calls: std::cell::RefCell::new(Vec::new()),
            }
        }

        fn se_llamo_a(&self, cmd: &str) -> bool {
            self.calls.borrow().iter().any(|c| c == cmd)
        }
    }

    impl Runner for FakeRunner {
        fn version(&self) -> String {
            self.node.clone()
        }
        fn run(&self, args: &[&str]) -> std::io::Result<RunnerOutput> {
            self.calls.borrow_mut().push(args.join(" "));
            match args.first() {
                Some(&"ls") => Ok(RunnerOutput {
                    stdout: self.ls.clone(),
                    stderr: String::new(),
                    exit_code: self.ls_exit,
                }),
                Some(&"outdated") => Ok(RunnerOutput {
                    stdout: self.outdated.clone(),
                    stderr: String::new(),
                    exit_code: self.outdated_exit,
                }),
                Some(&"install") => Ok(RunnerOutput {
                    stdout: self.install_out.clone(),
                    stderr: String::new(),
                    exit_code: self.install_exit,
                }),
                _ => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "comando inesperado",
                )),
            }
        }
    }

    #[test]
    fn merge_detecta_desactualizados_y_al_dia() {
        let pkgs = merge(LS_JSON, OUTDATED_JSON);
        assert_eq!(pkgs.len(), 3);
        // Orden alfabético, scoped incluido
        assert_eq!(pkgs[0].name, "@alibaba-group/open-code-review");
        assert!(!pkgs[0].outdated);
        assert_eq!(pkgs[0].latest.as_deref(), Some("1.10.2"));
        assert!(pkgs[1].outdated);
        assert_eq!(pkgs[1].name, "context-mode");
        assert_eq!(pkgs[1].latest.as_deref(), Some("1.0.170"));
        assert!(pkgs[2].outdated);
        assert_eq!(pkgs[2].latest.as_deref(), Some("0.18.0"));
    }

    #[test]
    fn merge_con_outdated_vacio_deja_todo_al_dia() {
        let pkgs = merge(LS_JSON, "");
        assert_eq!(pkgs.len(), 3);
        assert!(pkgs.iter().all(|p| !p.outdated));
        assert!(pkgs.iter().all(|p| p.latest == Some(p.installed.clone())));
    }

    #[test]
    fn merge_ignora_entradas_sin_version() {
        let ls = r#"{"dependencies": {"roto": {"missing": true}, "sano": {"version": "1.0.0"}}}"#;
        let pkgs = merge(ls, "");
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "sano");
    }

    #[test]
    fn snapshot_trata_exit_1_de_outdated_como_valido() {
        let mut runner = FakeRunner::new(LS_JSON, OUTDATED_JSON);
        runner.outdated_exit = 1; // npm outdated: 1 = hay desactualizados
        let snap = snapshot(&runner).expect("exit 1 no es error");
        assert_eq!(snap.version, "26.2.0");
        assert_eq!(snap.packages.len(), 3);
        assert!(snap.packages[1].outdated);
    }

    #[test]
    fn snapshot_falla_si_ls_no_produce_json() {
        let mut runner = FakeRunner::new("ENOTFOUND registry", "");
        runner.ls_exit = 1;
        assert!(snapshot(&runner).is_err());
    }

    #[test]
    fn snapshot_falla_si_outdated_falla_sin_json() {
        // exit 1 + stdout vacío = fallo de red, NO "ninguno desactualizado"
        // (que es exit 0 + stdout vacío).
        let mut runner = FakeRunner::new(LS_JSON, "");
        runner.outdated_exit = 1;
        assert!(snapshot(&runner).is_err());
    }

    #[test]
    fn snapshot_propaga_fallo_de_spawn() {
        struct Roto;
        impl Runner for Roto {
            fn version(&self) -> String {
                "26.2.0".into()
            }
            fn run(&self, _args: &[&str]) -> std::io::Result<RunnerOutput> {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "spawn"))
            }
        }
        assert!(snapshot(&Roto).is_err());
    }

    fn nvm_con(dir: &std::path::Path, versiones: &[&str]) {
        for v in versiones {
            std::fs::create_dir_all(dir.join("versions/node").join(v).join("bin")).unwrap();
        }
    }

    #[test]
    fn update_instala_la_ultima_version_y_streamea() {
        let runner = FakeRunner::new(LS_JSON, OUTDATED_JSON);
        let mut lineas = Vec::new();
        let out = update(&runner, "hunkdiff", &mut |l| lineas.push(l.to_string()))
            .expect("update válido");
        assert!(runner.se_llamo_a("install -g hunkdiff@latest"));
        assert!(out.success);
        assert_eq!(lineas, vec!["added 1 package in 2s"]);
    }

    #[test]
    fn update_fallido_con_exit_distinto_de_cero() {
        let mut runner = FakeRunner::new(LS_JSON, OUTDATED_JSON);
        runner.install_exit = 1;
        let out = update(&runner, "hunkdiff", &mut |_| {}).unwrap();
        assert!(!out.success);
    }

    #[test]
    fn update_rechaza_nombres_invalidos() {
        let runner = FakeRunner::new(LS_JSON, OUTDATED_JSON);
        assert!(update(&runner, "", &mut |_| {}).is_err());
        assert!(update(&runner, "--force", &mut |_| {}).is_err());
    }

    #[test]
    fn update_funciona_con_paquetes_scoped() {
        let runner = FakeRunner::new(LS_JSON, OUTDATED_JSON);
        let out = update(&runner, "@alibaba-group/open-code-review", &mut |_| {})
            .expect("scoped válido");
        assert!(runner
            .se_llamo_a("install -g @alibaba-group/open-code-review@latest"));
        assert!(out.success);
    }

    #[test]
    fn resolver_nvm_prefiere_alias_default() {
        let dir = tempfile::tempdir().unwrap();
        let nvm = dir.path().join(".nvm");
        nvm_con(&nvm, &["v24.1.0", "v26.2.0"]);
        std::fs::create_dir_all(nvm.join("alias")).unwrap();
        std::fs::write(nvm.join("alias/default"), "24.1.0").unwrap();
        let bin = resolve_nvm_bin_dir(&nvm).unwrap();
        assert_eq!(bin, nvm.join("versions/node/v24.1.0/bin"));
    }

    #[test]
    fn resolver_nvm_sigue_cadena_de_alias() {
        let dir = tempfile::tempdir().unwrap();
        let nvm = dir.path().join(".nvm");
        nvm_con(&nvm, &["v24.1.0", "v26.2.0"]);
        std::fs::create_dir_all(nvm.join("alias/lts")).unwrap();
        std::fs::write(nvm.join("alias/default"), "node").unwrap();
        std::fs::write(nvm.join("alias/node"), "lts/iron").unwrap();
        std::fs::write(nvm.join("alias/lts/iron"), "24.1.0").unwrap();
        let bin = resolve_nvm_bin_dir(&nvm).unwrap();
        assert_eq!(bin, nvm.join("versions/node/v24.1.0/bin"));
    }

    #[test]
    fn resolver_nvm_sin_alias_toma_la_mayor() {
        let dir = tempfile::tempdir().unwrap();
        let nvm = dir.path().join(".nvm");
        nvm_con(&nvm, &["v24.1.0", "v26.2.0"]);
        let bin = resolve_nvm_bin_dir(&nvm).unwrap();
        assert_eq!(bin, nvm.join("versions/node/v26.2.0/bin"));
    }
}
