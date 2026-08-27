//! Kernel compartido de los gestores: tipos de dominio, ejecución de
//! comandos y servicios de entorno (nvm, probes de versión).
//!
//! Los adapters (npm, pnpm, bun) aportan SOLO lo que varía entre gestores:
//! cómo listar su espacio global y cómo parsear la salida. Todo lo demás —
//! correr procesos, validar nombres, instalar, armar la lista final — vive
//! aquí, una sola vez.
//!
//! Seam de testing: [`Runner`] es el único punto por donde el proceso real
//! de un gestor entra al sistema; todo lo demás se prueba por encima con
//! [`testutil::FakeRunner`].

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Paquete global: nombre, versión instalada, última versión conocida y
/// si está desactualizado (calculado en Rust: la UI no re-deriva).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalPackage {
    pub name: String,
    pub installed: String,
    pub latest: Option<String>,
    pub outdated: bool,
}

/// El espacio global de un gestor: sus paquetes y sus versiones, en la
/// forma gestor-agnóstica que produce cada adapter (vocabulario en
/// CONTEXT.md). [`Snapshot`] le suma el comando visible al cruzar la IPC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EspacioGlobal {
    /// Versión del gestor mismo (`npm --version`).
    pub version_gestor: String,
    /// Versión activa de node cuando el gestor corre sobre node (npm, pnpm);
    /// `None` para gestores autocontenidos (bun).
    pub version_node: Option<String>,
    pub packages: Vec<GlobalPackage>,
}

/// Foto que cruza la seam hacia la UI: el espacio global más el comando
/// visible de actualización (fuente única del verbo; la UI no lo duplica).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub comando_actualizar: String,
    #[serde(flatten)]
    pub espacio: EspacioGlobal,
}

impl EspacioGlobal {
    /// Completa el espacio global con su comando visible para la IPC.
    pub fn con_comando(self, comando: &str) -> Snapshot {
        Snapshot {
            comando_actualizar: comando.to_string(),
            espacio: self,
        }
    }
}

/// Resultado de actualizar un paquete global.
#[derive(Debug, Serialize)]
pub struct UpdateOutcome {
    pub success: bool,
    pub output: String,
}

/// Salida de un comando del gestor: stdout, stderr y código de salida.
///
/// Contrato: `outdated` de npm/pnpm termina con código 1 cuando HAY
/// paquetes desactualizados — es un resultado válido, no un error. Solo el
/// fallo de spawn (o un fallo duro sin salida útil) se propaga como `Err`.
pub struct RunnerOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Seam: único punto por donde el proceso real de un gestor entra al
/// sistema.
pub trait Runner {
    fn version_gestor(&self) -> String;
    fn version_node(&self) -> Option<String> {
        None
    }
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
    packages.sort_by_key(|p| p.name.clone());
    packages
}

/// Fallo duro de un comando JSON: código de error y la salida no empieza
/// con el carácter esperado. Compartido por los gestores JSON.
pub(crate) fn guard_json(
    out: &RunnerOutput,
    gestor: &str,
    comando: &str,
    abre: char,
) -> std::io::Result<()> {
    if out.exit_code != 0 && !out.stdout.trim_start().starts_with(abre) {
        return Err(std::io::Error::other(format!(
            "{gestor} {comando} falló (exit {}): {}",
            out.exit_code,
            out.stderr.trim()
        )));
    }
    Ok(())
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
/// (npm: install · pnpm/bun: add), streameando cada línea a `on_line`.
/// El verbo llega desde la definición del gestor: única fuente.
pub fn instalar(
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

/// Corre un comando juntando toda su salida (sin streaming).
pub fn correr(cmd: &mut std::process::Command) -> std::io::Result<RunnerOutput> {
    let out = cmd.output()?;
    Ok(RunnerOutput {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        exit_code: out.status.code().unwrap_or(-1),
    })
}

/// Corre un comando con pipes y streamea cada línea de stdout a `on_line`
/// conforme llega; stderr se lee en un hilo aparte (no se streamea: el
/// progreso de los gestores va contaminado de secuencias de control).
pub fn correr_streaming(
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
        let mut lineas = BufReader::new(stderr).lines();
        // Un error de lectura aquí es terminal: se corta el drenado.
        while let Some(Ok(line)) = lineas.next() {
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
    let stderr_str = stderr_handle
        .join()
        .map_err(|_| std::io::Error::other("hilo de stderr murió"))?;
    let status = child.wait()?;
    Ok(RunnerOutput {
        stdout: stdout_str,
        stderr: stderr_str,
        exit_code: status.code().unwrap_or(-1),
    })
}

// ---- Servicios de entorno (compartidos) ----

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
        .map(|name| sin_v(&name).to_string())
        .collect();
    if versions.is_empty() {
        return None;
    }
    versions.sort_by_key(|v| version_key(v));

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
    let raw = sin_v(raw.trim());
    if raw.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        Some(raw.to_string())
    } else {
        resolve_alias(nvm_dir, raw, depth - 1)
    }
}

/// Quita la `v` inicial de una versión de node (`v26.2.0` → `26.2.0`).
pub fn sin_v(s: &str) -> &str {
    s.trim().trim_start_matches('v')
}

fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Antepone el bin de node de la versión activa de nvm al PATH del
/// comando: los shims (npm, pnpm) llevan `#!/usr/bin/env node` y una app
/// GUI abierta desde Finder no hereda el PATH del shell.
pub fn guardar_path_nvm(cmd: &mut std::process::Command) {
    if let Some(bin_dir) = home().and_then(|h| resolve_nvm_bin_dir(&h.join(".nvm"))) {
        let path = std::env::var_os("PATH").unwrap_or_default();
        // join_paths usa el separador del SO (`:` POSIX, `;` Windows).
        if let Ok(nueva) =
            std::env::join_paths(std::iter::once(bin_dir).chain(std::env::split_paths(&path)))
        {
            cmd.env("PATH", nueva);
        }
    }
}

// ---- Descubrimiento multiplataforma ----
//
// Una app GUI no hereda el PATH del shell: hallar un gestor exige PATH +
// variables conocidas (si existen) + ubicaciones estándar por SO. Nunca se
// EXIGE al usuario configurar nada: cero-config, las vars son bonus.

/// Home del usuario multiplataforma: `HOME` (POSIX) o `USERPROFILE`
/// (Windows, donde HOME suele no estar definido).
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|h| !h.as_os_str().is_empty())
}

/// `%LOCALAPPDATA%` (Windows): ahí instala pnpm por defecto.
pub fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// `%ProgramFiles%` (Windows): instalación por defecto de node.js.
#[cfg(windows)]
pub fn program_files() -> Option<PathBuf> {
    std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Nombre del binario con extensión de Windows (`pnpm` → `pnpm.exe`):
/// `is_file()` no resuelve extensiones por su cuenta.
pub fn con_extension(bin: &str) -> String {
    if cfg!(windows) {
        format!("{bin}.exe")
    } else {
        bin.to_string()
    }
}

/// Busca un binario en el PATH por su nombre YA extendido (ver
/// [`con_extension`]). Chequeo de presencia, sin spawn.
pub fn find_in_path(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|candidate| candidate.is_file())
}

/// El primer candidato que existe, en orden: así se resuelve el
/// descubrimiento de cada gestor (PATH → vars → estándar del SO).
pub fn primer_existente(candidatos: Vec<PathBuf>) -> Option<PathBuf> {
    candidatos.into_iter().find(|c| c.is_file())
}

/// Error de descubrimiento que enseña dónde se buscó: mata el ticket
/// "no me detecta X" sin adivinación.
pub fn no_encontrado(gestor: &str, buscadas: &[PathBuf]) -> std::io::Error {
    let rutas: Vec<String> = std::iter::once("PATH".to_string())
        .chain(buscadas.iter().map(|p| p.display().to_string()))
        .collect();
    std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{gestor} no encontrado. Busqué en: {}", rutas.join(", ")),
    )
}

/// Corre un comando de probe (`--version`) y devuelve su stdout recortado
/// si terminó bien; `None` si no se pudo ejecutar.
pub fn version_de(cmd: &mut std::process::Command) -> Option<String> {
    let out = cmd.output().ok()?;
    let salida = String::from_utf8_lossy(&out.stdout).trim().to_string();
    out.status
        .success()
        .then_some(salida)
        .filter(|s| !s.is_empty())
}

/// Falsos runners para los tests de todo el crate: uno solo, protocolo
/// configurable por primer argumento (ls/pm/outdated/install/add/…).
#[cfg(test)]
pub(crate) mod testutil {
    use super::*;
    use std::cell::RefCell;

    pub struct FakeRunner {
        gestor: String,
        node: Option<String>,
        /// (primer argumento, stdout, exit) — match por orden de registro.
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

        /// Respuesta para el comando cuyo PRIMER argumento es `cmd`.
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
                        format!("comando inesperado: {primero}"),
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
        // orden alfabético, scoped incluido
        assert_eq!(pkgs[0].name, "@alibaba-group/open-code-review");
        assert!(!pkgs[0].outdated); // ausente en outdated = al día, hereda installed
        assert_eq!(pkgs[0].latest.as_deref(), Some("1.10.2"));
        assert!(pkgs[1].outdated);
        assert_eq!(pkgs[1].latest.as_deref(), Some("0.18.0"));
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
        let out = instalar(&runner, "install", "hunkdiff", &mut |l| {
            lineas.push(l.to_string())
        })
        .expect("instalación válida");
        assert!(runner.se_llamo_a("install -g hunkdiff@latest"));
        assert!(out.success);
        assert_eq!(lineas, vec!["added 1 package in 2s"]);
    }

    #[test]
    fn instalar_con_verbo_add_sirve_a_pnpm_y_bun() {
        let runner = FakeRunner::new("10.33.0").respuesta("add", "Done in 1.5s", 0);
        let out = instalar(&runner, "add", "cowsay", &mut |_| {}).unwrap();
        assert!(out.success);
        assert!(runner.se_llamo_a("add -g cowsay@latest"));
    }

    #[test]
    fn instalar_fallido_devuelve_success_false() {
        let runner = FakeRunner::new("11.4.2").respuesta("install", "EACCES", 1);
        let out = instalar(&runner, "install", "hunkdiff", &mut |_| {}).unwrap();
        assert!(!out.success);
    }

    #[test]
    fn instalar_rechaza_nombres_invalidos() {
        let runner = runner_npm();
        assert!(instalar(&runner, "install", "", &mut |_| {}).is_err());
        assert!(instalar(&runner, "install", "--force", &mut |_| {}).is_err());
    }

    #[test]
    fn instalar_funciona_con_paquetes_scoped() {
        let runner = runner_npm();
        let out = instalar(
            &runner,
            "install",
            "@alibaba-group/open-code-review",
            &mut |_| {},
        )
        .expect("scoped válido");
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
        // "a" no existe: la búsqueda sigue hasta "b"
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
            msg.starts_with("pnpm no encontrado. Busqué en: PATH"),
            "{msg}"
        );
        assert!(msg.contains("/Users/ejemplo/Library/pnpm"), "{msg}");
        assert!(msg.contains("/opt/pnpm"), "{msg}");
    }
}
