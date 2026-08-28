//! Shared manager kernel: domain types, command execution and environment
//! services (nvm, version probes).
//!
//! The adapters (npm, pnpm, bun) contribute ONLY what varies between
//! managers: how to list their global space and how to parse the output.
//! Everything else — running processes, validating names, installing,
//! assembling the final list — lives here, once.
//!
//! Testing seam: [`Runner`] is the single point where a manager's real
//! process enters the system; everything above it is tested with
//! [`testutil::FakeRunner`].

use crate::plazo::{finalizar, plazo_vencido, Plazo, PLAZO_CONSULTA, PLAZO_INSTALACION};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// Global package: name, installed version, latest known version and
/// whether it is outdated (computed in Rust: the UI never re-derives it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalPackage {
    pub name: String,
    pub installed: String,
    pub latest: Option<String>,
    pub outdated: bool,
}

/// A manager's global space: its packages and versions, in the
/// manager-agnostic shape each adapter produces (vocabulary in
/// CONTEXT.md). [`Snapshot`] adds the visible command when crossing the IPC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EspacioGlobal {
    /// Version of the manager itself (`npm --version`).
    pub version_gestor: String,
    /// Active node version when the manager runs on node (npm, pnpm);
    /// `None` for self-contained managers (bun).
    pub version_node: Option<String>,
    pub packages: Vec<GlobalPackage>,
}

/// The photo that crosses the seam to the UI: the global space plus the
/// visible update command (single source of the verb; the UI never
/// duplicates it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub comando_actualizar: String,
    #[serde(flatten)]
    pub espacio: EspacioGlobal,
}

impl EspacioGlobal {
    /// Completes the global space with its visible command for the IPC.
    pub fn con_comando(self, comando: &str) -> Snapshot {
        Snapshot {
            comando_actualizar: comando.to_string(),
            espacio: self,
        }
    }
}

/// Result of updating a global package.
#[derive(Debug, Serialize)]
pub struct UpdateOutcome {
    pub success: bool,
    pub output: String,
}

/// Output of a manager command: stdout, stderr and exit code.
///
/// Contract: npm/pnpm `outdated` exits with code 1 when there ARE
/// outdated packages — that is a valid result, not an error. Only a spawn
/// failure (or a hard failure with no useful output) propagates as `Err`.
#[derive(Debug)]
pub struct RunnerOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Seam: the single point where a manager's real process enters the
/// system.
pub trait Runner {
    fn version_gestor(&self) -> String;
    fn version_node(&self) -> Option<String> {
        None
    }
    fn run(&self, args: &[&str]) -> std::io::Result<RunnerOutput>;

    /// Like `run`, but streams each output line to `on_line` as it
    /// arrives. By default it runs `run` and emits the already-complete
    /// lines — the real runner overrides it with pipes. `parar` is Stop
    /// (#16): faithful for the flag-already-set case — an immediate
    /// Interrupted, exactly what the engine returns when Stop won.
    fn run_streaming(
        &self,
        args: &[&str],
        on_line: &mut dyn FnMut(&str),
        parar: &std::sync::Arc<AtomicBool>,
    ) -> std::io::Result<RunnerOutput> {
        if parar.load(Ordering::Acquire) {
            return Err(detenido_a_pedido());
        }
        let out = self.run(args)?;
        for line in out.stdout.lines().chain(out.stderr.lines()) {
            on_line(line);
        }
        Ok(out)
    }
}

/// Parses `outdated --json` (shape shared by npm and pnpm:
/// { package: { latest, ... } } → name→latest map). It only lists
/// packages with an update available: absent ones are up to date.
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

/// Manager packages ("Paquete del gestor" in the glossary): npm, pnpm and
/// bun themselves. They update outside Nuupa — npm ships with node, pnpm
/// and bun are installed by their own official installers — so they never
/// appear in any global space.
pub(crate) const PAQUETES_DE_GESTOR: &[&str] = &["npm", "pnpm", "bun"];

/// Assembles the final package list: (name, installed) + outdated →
/// latest inherited when up to date. Shared by all managers; drops the
/// manager packages ([`PAQUETES_DE_GESTOR`]) so they never reach a table.
pub(crate) fn armar(
    pares: Vec<(String, String)>,
    outdated: &BTreeMap<String, String>,
) -> Vec<GlobalPackage> {
    let mut packages = pares
        .into_iter()
        .filter(|(name, _)| !PAQUETES_DE_GESTOR.contains(&name.as_str()))
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
    packages.sort_by_key(|p| p.name.clone());
    packages
}

/// Hard failure of a JSON command: non-zero exit code and the output does
/// not start with the expected character. Shared by the JSON managers.
pub(crate) fn guard_json(
    out: &RunnerOutput,
    gestor: &str,
    comando: &str,
    abre: char,
) -> std::io::Result<()> {
    if out.exit_code != 0 && !out.stdout.trim_start().starts_with(abre) {
        return Err(std::io::Error::other(format!(
            "{gestor} {comando} failed (exit {}): {}",
            out.exit_code,
            out.stderr.trim()
        )));
    }
    Ok(())
}

/// Validates a package name: never a disguised flag (it travels as a
/// process argument, no shell, but still).
pub(crate) fn validar_nombre(name: &str) -> std::io::Result<()> {
    if name.is_empty() || name.starts_with('-') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid package name: {name:?}"),
        ));
    }
    Ok(())
}

/// Installs the latest version of a global package with the manager's
/// verb (npm: install · pnpm/bun: add), streaming each line to `on_line`.
/// The verb comes from the manager definition: single source.
pub fn instalar(
    runner: &dyn Runner,
    verbo: &str,
    name: &str,
    on_line: &mut dyn FnMut(&str),
    parar: &std::sync::Arc<AtomicBool>,
) -> std::io::Result<UpdateOutcome> {
    validar_nombre(name)?;
    let spec = format!("{name}@latest");
    let out = runner.run_streaming(&[verbo, "-g", &spec], on_line, parar)?;
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

/// A Stop flag that never fires: queries and individual updates.
pub fn sin_parar() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

/// Runs a command collecting all of its output (no streaming), under a
/// deadline. Queries are never stopped: no Stop flag travels here.
pub fn correr(cmd: std::process::Command, plazo: Plazo) -> std::io::Result<RunnerOutput> {
    correr_streaming(cmd, &mut |_| {}, plazo, &sin_parar())
}

/// A gestor QUERY (ls/outdated/--version) under the query deadline: the
/// only way adapters run queries, so the query→deadline mapping lives in
/// ONE place.
pub fn correr_consulta(cmd: std::process::Command) -> std::io::Result<RunnerOutput> {
    correr(cmd, PLAZO_CONSULTA)
}

/// A gestor INSTALLATION under the installation deadline, streaming: the
/// only way adapters run installs. `parar` is Stop (#16): it reaches the
/// watchdog, which escalates the child's termination mid-flight.
pub fn correr_instalacion(
    cmd: std::process::Command,
    on_line: &mut dyn FnMut(&str),
    parar: &Arc<AtomicBool>,
) -> std::io::Result<RunnerOutput> {
    correr_streaming(cmd, on_line, PLAZO_INSTALACION, parar)
}

/// Why the watchdog escalated the child's termination: the deadline won
/// or Stop arrived mid-flight (#16).
#[derive(Debug, Clone, Copy)]
enum Fin {
    Plazo,
    Detenido,
}

/// The Stop error: the engine cut the child at the user's request.
fn detenido_a_pedido() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        "detenido a pedido (proceso finalizado)",
    )
}

/// Runs a command with pipes and streams each stdout line to `on_line` as
/// it arrives; stderr is read on a separate thread (not streamed: manager
/// progress output is polluted with control sequences). Under `plazo`: a
/// watchdog thread escalates the child's termination when it expires.
pub fn correr_streaming(
    mut cmd: std::process::Command,
    on_line: &mut dyn FnMut(&str),
    plazo: Plazo,
    parar: &Arc<AtomicBool>,
) -> std::io::Result<RunnerOutput> {
    use std::io::{BufRead, BufReader};
    // Own process group: the escalation can signal the whole family
    // (shims leave grandchildren holding the pipe) and the child does not
    // receive the app's terminal signals.
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let mut lineas = BufReader::new(stderr).lines();
        // A read error here is terminal: the draining stops.
        while let Some(Ok(line)) = lineas.next() {
            buf.push_str(&line);
            buf.push('\n');
        }
        buf
    });

    // The watchdog polls: the child finishing first (channel), Stop
    // (#16) or the deadline — in that order of precedence. The child
    // travels in a mutex because killing it needs `&mut` — the wait
    // below never holds the lock while blocking.
    let hijo = Arc::new(Mutex::new(child));
    let como_termino = Arc::new(Mutex::new(None::<Fin>));
    let (listo_tx, listo_rx) = mpsc::channel::<()>();
    let (vigia_hijo, vigia_fin) = (Arc::clone(&hijo), Arc::clone(&como_termino));
    let vigia_parar = Arc::clone(parar);
    let vigia = std::thread::spawn(move || {
        let inicio = std::time::Instant::now();
        loop {
            if listo_rx.try_recv().is_ok() {
                return; // the child finished in time
            }
            let fin = if vigia_parar.load(Ordering::Acquire) {
                Some(Fin::Detenido)
            } else if inicio.elapsed() >= plazo.total {
                Some(Fin::Plazo)
            } else {
                None
            };
            let Some(fin) = fin else {
                std::thread::sleep(Duration::from_millis(15));
                continue;
            };
            // Only a child that was still ALIVE when the reason fired was
            // cut: one that died on its own right at the boundary is not.
            if finalizar(&vigia_hijo, plazo.grace) {
                *vigia_fin.lock().unwrap() = Some(fin);
            }
            return;
        }
    });

    let mut stdout_str = String::new();
    let mut fallo_lectura: Option<std::io::Error> = None;
    for line in BufReader::new(stdout).lines() {
        match line {
            Ok(line) => {
                on_line(&line);
                stdout_str.push_str(&line);
                stdout_str.push('\n');
            }
            Err(e) => {
                fallo_lectura = Some(e);
                break;
            }
        }
    }

    // Polling wait: brief locks only, so the watchdog can always get in.
    let status = loop {
        if let Some(st) = hijo.lock().unwrap().try_wait()? {
            break st;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let _ = listo_tx.send(());
    let _ = vigia.join();
    let stderr_str = stderr_handle
        .join()
        .map_err(|_| std::io::Error::other("stderr thread died"))?;

    match como_termino.lock().unwrap().take() {
        Some(Fin::Plazo) => return Err(plazo_vencido(&cmd, plazo.total)),
        Some(Fin::Detenido) => return Err(detenido_a_pedido()),
        None => {}
    }
    if let Some(e) = fallo_lectura {
        return Err(e);
    }
    Ok(RunnerOutput {
        stdout: stdout_str,
        stderr: stderr_str,
        exit_code: status.code().unwrap_or(-1),
    })
}

// ---- Shared environment services ----

/// Resolves the node/npm bin directory of nvm's active version: the
/// `default` alias (following chains like `node` → `lts/iron`) if it ends
/// up in an installed version, otherwise the most recent installed one.
pub fn resolve_nvm_bin_dir(nvm_dir: &Path) -> Option<PathBuf> {
    let versions_dir = nvm_dir.join("versions").join("node");
    let mut versions: Vec<String> = std::fs::read_dir(&versions_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .map(|name| sin_v(&name).to_string())
        .collect();
    if versions.is_empty() {
        return None;
    }
    versions.sort_by_key(|v| version_key(v));

    let chosen = resolve_alias(nvm_dir, "default", 5)
        .filter(|d| versions.contains(d))
        .unwrap_or_else(|| versions.pop().expect("versions not empty"));
    Some(versions_dir.join(format!("v{chosen}")).join("bin"))
}

/// Follows an nvm alias chain (`default` → `node` → `lts/iron` →
/// version) until it hits a version number.
fn resolve_alias(nvm_dir: &Path, alias: &str, depth: u32) -> Option<String> {
    if depth == 0 {
        return None;
    }
    let raw = std::fs::read_to_string(nvm_dir.join("alias").join(alias)).ok()?;
    let raw = sin_v(raw.trim());
    if raw.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        Some(raw.to_string())
    } else {
        resolve_alias(nvm_dir, raw, depth - 1)
    }
}

/// Strips the leading `v` from a node version (`v26.2.0` → `26.2.0`).
pub fn sin_v(s: &str) -> &str {
    s.trim().trim_start_matches('v')
}

fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Prepends nvm's active node bin to the command's PATH: the shims (npm,
/// pnpm) carry `#!/usr/bin/env node` and a GUI app opened from Finder
/// does not inherit the shell's PATH.
pub fn guardar_path_nvm(cmd: &mut std::process::Command) {
    if let Some(bin_dir) = home().and_then(|h| resolve_nvm_bin_dir(&h.join(".nvm"))) {
        let path = std::env::var_os("PATH").unwrap_or_default();
        // join_paths uses the OS separator (`:` POSIX, `;` Windows).
        if let Ok(nueva) =
            std::env::join_paths(std::iter::once(bin_dir).chain(std::env::split_paths(&path)))
        {
            cmd.env("PATH", nueva);
        }
    }
}

// ---- Cross-platform discovery ----
//
// A GUI app does not inherit the shell's PATH: finding a manager takes
// PATH + known variables (when present) + standard per-OS locations. The
// user is NEVER required to configure anything: zero-config, vars are a
// bonus.

/// Cross-platform user home: `HOME` (POSIX) or `USERPROFILE` (Windows,
/// where HOME is often undefined).
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|h| !h.as_os_str().is_empty())
}

/// `%LOCALAPPDATA%` (Windows): where pnpm installs by default.
pub fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// `%ProgramFiles%` (Windows): node.js default installation.
#[cfg(windows)]
pub fn program_files() -> Option<PathBuf> {
    std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Binary name with the Windows extension (`pnpm` → `pnpm.exe`):
/// `is_file()` does not resolve extensions on its own.
pub fn con_extension(bin: &str) -> String {
    if cfg!(windows) {
        format!("{bin}.exe")
    } else {
        bin.to_string()
    }
}

/// Searches the PATH for a binary by its ALREADY extended name (see
/// [`con_extension`]). Presence check, no spawn.
pub fn find_in_path(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|candidate| candidate.is_file())
}

/// The first candidate that exists, in order: this is how each manager's
/// discovery resolves (PATH → vars → OS standard).
pub fn primer_existente(candidatos: Vec<PathBuf>) -> Option<PathBuf> {
    candidatos.into_iter().find(|c| c.is_file())
}

/// Discovery error that shows where it searched: kills the "it doesn't
/// detect X" ticket without guesswork.
pub fn no_encontrado(gestor: &str, buscadas: &[PathBuf]) -> std::io::Error {
    let rutas: Vec<String> = std::iter::once("PATH".to_string())
        .chain(buscadas.iter().map(|p| p.display().to_string()))
        .collect();
    std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{gestor} not found. Searched in: {}", rutas.join(", ")),
    )
}

/// Runs a probe command (`--version`) and returns its trimmed stdout if
/// it ended well; `None` if it could not run. Under the query deadline:
/// discovery cannot hang either.
pub fn version_de(cmd: std::process::Command) -> Option<String> {
    let out = correr_consulta(cmd).ok()?;
    let salida = out.stdout.trim().to_string();
    (out.exit_code == 0)
        .then_some(salida)
        .filter(|s| !s.is_empty())
}

/// Fake runners for the whole crate's tests: a single one, protocol
/// configured by first argument (ls/pm/outdated/install/add/…).
#[cfg(test)]
pub(crate) mod testutil {
    use super::*;
    use std::cell::RefCell;

    pub struct FakeRunner {
        gestor: String,
        node: Option<String>,
        /// (first argument, stdout, exit) — matched by first argument.
        salidas: Vec<(&'static str, String, i32)>,
        llamadas: RefCell<Vec<String>>,
    }

    impl FakeRunner {
        pub fn new(gestor: &str) -> Self {
            Self {
                gestor: gestor.into(),
                node: Some("26.2.0".into()),
                salidas: Vec::new(),
                llamadas: RefCell::new(Vec::new()),
            }
        }

        pub fn con_node(mut self, node: Option<&str>) -> Self {
            self.node = node.map(str::to_string);
            self
        }

        /// Answer for the command whose FIRST argument is `cmd`.
        pub fn respuesta(mut self, cmd: &'static str, stdout: &str, exit: i32) -> Self {
            self.salidas.push((cmd, stdout.into(), exit));
            self
        }

        pub fn se_llamo_a(&self, cmd: &str) -> bool {
            self.llamadas.borrow().iter().any(|c| c == cmd)
        }
    }

    impl Runner for FakeRunner {
        fn version_gestor(&self) -> String {
            self.gestor.clone()
        }
        fn version_node(&self) -> Option<String> {
            self.node.clone()
        }
        fn run(&self, args: &[&str]) -> std::io::Result<RunnerOutput> {
            self.llamadas.borrow_mut().push(args.join(" "));
            let primero = args.first().copied().unwrap_or_default();
            self.salidas
                .iter()
                .find(|(cmd, _, _)| *cmd == primero)
                .map(|(_, stdout, exit)| RunnerOutput {
                    stdout: stdout.clone(),
                    stderr: String::new(),
                    exit_code: *exit,
                })
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("unexpected command: {primero}"),
                    )
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::FakeRunner;
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

    fn runner_npm() -> FakeRunner {
        FakeRunner::new("11.4.2")
            .respuesta("ls", LS_JSON, 0)
            .respuesta("outdated", OUTDATED_JSON, 0)
            .respuesta("install", "added 1 package in 2s", 0)
    }

    #[test]
    fn parse_outdated_mapea_nombre_a_latest() {
        let map = parse_outdated(OUTDATED_JSON);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("context-mode").map(String::as_str), Some("1.0.170"));
        assert_eq!(map.get("hunkdiff").map(String::as_str), Some("0.18.0"));
    }

    #[test]
    fn parse_outdated_con_basura_da_mapa_vacio() {
        assert!(parse_outdated("ENOTFOUND registry").is_empty());
        assert!(parse_outdated("").is_empty());
    }

    #[test]
    fn armar_ordena_hereda_latest_y_marca_desactualizados() {
        let pares = vec![
            ("hunkdiff".to_string(), "0.17.2".to_string()),
            (
                "@alibaba-group/open-code-review".to_string(),
                "1.10.2".to_string(),
            ),
        ];
        let mut outdated = BTreeMap::new();
        outdated.insert("hunkdiff".to_string(), "0.18.0".to_string());
        let pkgs = armar(pares, &outdated);
        assert_eq!(pkgs.len(), 2);
        // alphabetical order, scoped included
        assert_eq!(pkgs[0].name, "@alibaba-group/open-code-review");
        // absent from outdated = up to date, inherits installed
        assert!(!pkgs[0].outdated);
        assert_eq!(pkgs[0].latest.as_deref(), Some("1.10.2"));
        assert!(pkgs[1].outdated);
        assert_eq!(pkgs[1].latest.as_deref(), Some("0.18.0"));
    }

    #[test]
    fn armar_filtra_los_paquetes_de_gestor() {
        // npm/pnpm/bun themselves never reach the tables: they update
        // outside Nuupa ("Paquete del gestor" in the glossary).
        let pares = vec![
            ("npm".to_string(), "11.4.2".to_string()),
            ("hunkdiff".to_string(), "0.17.2".to_string()),
            ("pnpm".to_string(), "10.33.0".to_string()),
            ("bun".to_string(), "1.3.14".to_string()),
        ];
        let pkgs = armar(pares, &BTreeMap::new());
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "hunkdiff");
    }

    #[test]
    fn guard_json_deja_pasar_exit_1_con_json_utilizable() {
        let out = RunnerOutput {
            stdout: "{...}".into(),
            stderr: String::new(),
            exit_code: 1,
        };
        assert!(guard_json(&out, "npm", "outdated", '{').is_ok());
    }

    #[test]
    fn guard_json_falla_exit_1_sin_json() {
        let out = RunnerOutput {
            stdout: String::new(),
            stderr: "ENOTFOUND".into(),
            exit_code: 1,
        };
        assert!(guard_json(&out, "npm", "outdated", '{').is_err());
    }

    #[test]
    fn instalar_usa_el_verbo_del_gestor_y_streamea() {
        let runner = runner_npm();
        let mut lineas = Vec::new();
        let out = instalar(
            &runner,
            "install",
            "hunkdiff",
            &mut |l| lineas.push(l.to_string()),
            &sin_parar(),
        )
        .expect("valid install");
        assert!(runner.se_llamo_a("install -g hunkdiff@latest"));
        assert!(out.success);
        assert_eq!(lineas, vec!["added 1 package in 2s"]);
    }

    #[test]
    fn instalar_con_verbo_add_sirve_a_pnpm_y_bun() {
        let runner = FakeRunner::new("10.33.0").respuesta("add", "Done in 1.5s", 0);
        let out = instalar(&runner, "add", "cowsay", &mut |_| {}, &sin_parar()).unwrap();
        assert!(out.success);
        assert!(runner.se_llamo_a("add -g cowsay@latest"));
    }

    #[test]
    fn instalar_fallido_devuelve_success_false() {
        let runner = FakeRunner::new("11.4.2").respuesta("install", "EACCES", 1);
        let out = instalar(&runner, "install", "hunkdiff", &mut |_| {}, &sin_parar()).unwrap();
        assert!(!out.success);
    }

    #[test]
    fn instalar_rechaza_nombres_invalidos() {
        let runner = runner_npm();
        assert!(instalar(&runner, "install", "", &mut |_| {}, &sin_parar()).is_err());
        assert!(instalar(&runner, "install", "--force", &mut |_| {}, &sin_parar()).is_err());
    }

    #[test]
    fn instalar_funciona_con_paquetes_scoped() {
        let runner = runner_npm();
        let out = instalar(
            &runner,
            "install",
            "@alibaba-group/open-code-review",
            &mut |_| {},
            &sin_parar(),
        )
        .expect("valid scoped name");
        assert!(out.success);
        assert!(runner.se_llamo_a("install -g @alibaba-group/open-code-review@latest"));
    }

    #[test]
    fn espacio_global_con_comando_agrega_el_verbo_visible() {
        let espacio = EspacioGlobal {
            version_gestor: "11.4.2".into(),
            version_node: Some("26.2.0".into()),
            packages: Vec::new(),
        };
        let snap = espacio.con_comando("npm i -g");
        assert_eq!(snap.comando_actualizar, "npm i -g");
        assert_eq!(snap.espacio.version_gestor, "11.4.2");
        assert_eq!(snap.espacio.version_node.as_deref(), Some("26.2.0"));
    }

    fn nvm_con(dir: &std::path::Path, versiones: &[&str]) {
        for v in versiones {
            std::fs::create_dir_all(dir.join("versions/node").join(v).join("bin")).unwrap();
        }
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

    #[test]
    fn sin_v_quita_la_v_inicial() {
        assert_eq!(sin_v("v26.2.0"), "26.2.0");
        assert_eq!(sin_v("26.2.0"), "26.2.0");
        assert_eq!(sin_v(" v1.0.0 "), "1.0.0");
    }

    #[test]
    fn primer_existente_toma_el_primero_que_existe_en_orden() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::write(&b, "").unwrap();
        // "a" does not exist: the search continues to "b"
        assert_eq!(primer_existente(vec![a, b.clone()]), Some(b));
        assert_eq!(primer_existente(vec![dir.path().join("nada")]), None);
    }

    #[test]
    fn no_encontrado_lista_el_path_y_las_rutas_exploradas() {
        let err = no_encontrado(
            "pnpm",
            &[
                PathBuf::from("/Users/ejemplo/Library/pnpm"),
                PathBuf::from("/opt/pnpm"),
            ],
        );
        let msg = err.to_string();
        assert!(
            msg.starts_with("pnpm not found. Searched in: PATH"),
            "{msg}"
        );
        assert!(msg.contains("/Users/ejemplo/Library/pnpm"), "{msg}");
        assert!(msg.contains("/opt/pnpm"), "{msg}");
    }

    // ---- Plazos: procesos REALES colgados (#11) ----
    //
    // Lo que se prueba es el manejo del hijo (escalada de finalización),
    // por eso no hay mock: `sh`/`sleep` de verdad. Guardadas a unix porque
    // CI corre `cargo test` en Linux; Windows compila el mismo mecanismo
    // (allí la única terminación es la dura) pero no ejecuta estas.

    /// Un comando que duerme `segundos` (el "gestor colgado").
    #[cfg(unix)]
    fn dormilon(segundos: u32) -> std::process::Command {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(format!("sleep {segundos}"));
        c
    }

    #[cfg(unix)]
    #[test]
    fn correr_con_proceso_colgado_corta_con_error_de_plazo() {
        let plazo = Plazo {
            total: Duration::from_millis(150),
            grace: Duration::from_millis(300),
        };
        let inicio = std::time::Instant::now();
        let err = correr(dormilon(30), plazo).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(err.to_string().contains("no respondió"), "{err}");
        // cortó por el plazo, no porque `sleep 30` terminara
        assert!(
            inicio.elapsed() < Duration::from_secs(5),
            "tardó {:?}",
            inicio.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn correr_escalada_a_kill_si_ignora_la_senal_cortes() {
        // `sh` ignora TERM (como un gestor atascado de verdad); sus
        // `sleep` nietos mueren con el TERM del grupo pero el bucle
        // sigue: solo el KILL del grupo posterior al grace lo corta.
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c")
            .arg("trap '' TERM; while :; do sleep 0.1; done");
        let plazo = Plazo {
            total: Duration::from_millis(150),
            grace: Duration::from_millis(300),
        };
        let inicio = std::time::Instant::now();
        let err = correr(cmd, plazo).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        // no murió con la señal cortés: esperó el grace y recibió KILL
        assert!(
            inicio.elapsed() >= plazo.total + plazo.grace,
            "murió antes de la escalada: {:?}",
            inicio.elapsed()
        );
        assert!(
            inicio.elapsed() < Duration::from_secs(5),
            "tardó {:?}",
            inicio.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn correr_streaming_entrega_lo_emitido_antes_del_corte() {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("echo linea1; sleep 30");
        let plazo = Plazo {
            total: Duration::from_millis(150),
            grace: Duration::from_millis(300),
        };
        let mut lineas = Vec::new();
        let err = correr_streaming(
            cmd,
            &mut |l| lineas.push(l.to_string()),
            plazo,
            &sin_parar(),
        )
        .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        // la salida parcial emitida antes del corte llegó al log
        assert_eq!(lineas, vec!["linea1"]);
    }

    #[cfg(unix)]
    #[test]
    fn correr_con_comando_rapido_termina_normal() {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("echo hola");
        let out = correr(cmd, PLAZO_CONSULTA).unwrap();
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout.contains("hola"), "{}", out.stdout);
    }

    #[cfg(unix)]
    #[test]
    fn correr_con_hijo_que_termina_antes_del_plazo_devuelve_ok() {
        // Frontera del vigía: un hijo que murió solo justo cuando el plazo
        // vence NO es un timeout (la bandera solo se marca si escaló).
        let plazo = Plazo {
            total: Duration::from_millis(1500),
            grace: Duration::from_millis(300),
        };
        let out = correr(dormilon(1), plazo).unwrap();
        assert_eq!(out.exit_code, 0);
    }

    #[cfg(unix)]
    #[test]
    fn detener_corta_al_proceso_en_curso_en_segundos() {
        // Stop arrives 80 ms into a `sleep 30`: the watchdog escalates on
        // the flag, not the deadline, and the cut is Interrupted.
        let parar = Arc::new(AtomicBool::new(false));
        let bandera = Arc::clone(&parar);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            bandera.store(true, Ordering::Release);
        });
        let inicio = std::time::Instant::now();
        let err = correr_instalacion(dormilon(30), &mut |_| {}, &parar).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
        assert!(
            inicio.elapsed() < Duration::from_secs(5),
            "tardó {:?}",
            inicio.elapsed()
        );
    }
}
