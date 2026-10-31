//! brew adapter (Homebrew on macOS, Linuxbrew on Linux): its global
//! space is the user's LEAVES (top-level formulae) plus every cask —
//! a dependency is not a row (vocabulary in GLOSSARY.md). Its update
//! OBJECTIVE is wider: everything `brew outdated` declares, a
//! dependency included, minus the pinned ones. The update line
//! (`upgrade --formula|--cask`) lives in [`crate`]'s manager table.

use crate::kernel::{
    con_extension, correr_consulta, correr_instalacion, home, version_de, EspacioGlobal,
    GlobalPackage, PlanGestor, Runner, RunnerOutput,
};
use crate::sondeo;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) const TIPO_FORMULA: &str = "formula";
pub(crate) const TIPO_CASK: &str = "cask";

/// brew's standard locations (outside the PATH), in order: the official
/// prefixes — Apple Silicon, Intel, Linuxbrew — then `~/homebrew` (the
/// documented unsupported-custom-prefix install). A GUI app does not
/// inherit the shell's PATH, so the defaults save the day. Zero-config.
fn ubicaciones_brew() -> Vec<PathBuf> {
    let mut rutas = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
    ];
    if let Some(h) = home() {
        rutas.push(h.join("homebrew/bin"));
    }
    rutas
        .into_iter()
        .map(|dir| dir.join(con_extension("brew")))
        .collect()
}

/// Is there a brew on this machine? Presence check: the environment
/// PATH, the official prefixes, the login-shell probe (#43 — the
/// probe layer skips brew on Windows by itself; the registry sondador
/// never feeds it, #45). Feeds which tabs exist: never true on
/// Windows.
pub fn instalado() -> bool {
    brew_aplica(std::env::consts::OS)
        && sondeo::descubrir_o_error("brew", ubicaciones_brew()).is_ok()
}

/// Can brew exist on the `so` OS? Not on Windows (#45): no tab and
/// no probe — the discovery chain is never even assembled, whether
/// it would come from the environment PATH or the registry.
fn brew_aplica(so: &str) -> bool {
    so != "windows"
}

/// brew's real runner: the discovered binary. No node involved.
pub struct RealBrewRunner {
    bin: PathBuf,
    brew_version: String,
}

impl RealBrewRunner {
    pub fn discover() -> std::io::Result<Self> {
        let bin = sondeo::descubrir_o_error("brew", ubicaciones_brew())?;
        // `brew --version` answers "Homebrew 7.0.4\n...": the version
        // is the last token of the first line.
        let brew_version = version_de(Self::command(&bin, &["--version"]))
            .map(|v| {
                v.lines()
                    .next()
                    .unwrap_or(&v)
                    .rsplit(' ')
                    .next()
                    .unwrap_or(&v)
                    .to_string()
            })
            .unwrap_or_else(|| "unknown".to_string());
        Ok(Self { bin, brew_version })
    }

    fn command(bin: &Path, args: &[&str]) -> std::process::Command {
        let mut cmd = std::process::Command::new(bin);
        cmd.args(args);
        cmd
    }
}

impl Runner for RealBrewRunner {
    fn version_gestor(&self) -> String {
        self.brew_version.clone()
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

/// One entry of `brew outdated --json=v2`: latest version and whether
/// the formula is pinned (casks carry no pinned field).
#[derive(Debug, Clone, PartialEq)]
struct Desactualizado {
    latest: String,
    pinned: bool,
}

/// Parses `brew outdated --json=v2` → ((tipo, nombre) → latest, pinned).
/// It only lists packages with an update available: absent ones are up
/// to date — same contract as npm's `outdated --json`.
fn parse_outdated_v2(json: &str) -> BTreeMap<(String, String), Desactualizado> {
    let mut map = BTreeMap::new();
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::Value::Null);
    let Some(obj) = parsed.as_object() else {
        return map;
    };
    for (clave, tipo) in [("formulae", TIPO_FORMULA), ("casks", TIPO_CASK)] {
        for entrada in obj
            .get(clave)
            .and_then(|v| v.as_array())
            .unwrap_or(&Vec::new())
        {
            let Some(nombre) = entrada.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(latest) = entrada.get("current_version").and_then(|v| v.as_str()) else {
                continue;
            };
            let pinned = entrada
                .get("pinned")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            map.insert(
                (tipo.to_string(), nombre.to_string()),
                Desactualizado {
                    latest: latest.to_string(),
                    pinned,
                },
            );
        }
    }
    map
}

/// Parses `brew leaves`: one formula name per line (the top level —
/// what the user installed on purpose).
fn parse_leaves(stdout: &str) -> BTreeSet<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// Parses `brew list --versions` (and `--cask --versions`): `name v1
/// [v2 …]` per line. Multiple installed versions join with ", ". The key
/// is the SHORT name (`engram`); the rows are identified by the full one
/// (`gentleman-programming/tap/engram`) — `nombre_corto` bridges both.
fn parse_versions(stdout: &str) -> BTreeMap<String, String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let mut partes = l.split_whitespace();
            let nombre = partes.next()?;
            Some((nombre.to_string(), partes.collect::<Vec<_>>().join(", ")))
        })
        .collect()
}

/// The short name of a brew formula: what comes after the tap prefix.
/// `gentleman-programming/tap/engram` → `engram`. Core formulae have no
/// prefix, so the short name is the full name.
fn nombre_corto(nombre: &str) -> &str {
    nombre.rsplit('/').next().unwrap_or(nombre)
}

/// Assembles brew's global space: leaves (formulae the user installed)
/// plus every cask. An outdated entry adds latest + the flag; absent
/// means up to date (latest = installed). Sorted by name, then type —
/// a formula/cask name collision shows as two distinguishable rows.
///
/// The update objective is NOT the outdated rows: it is EVERYTHING brew
/// declares outdated, dependencies included, minus the pinned ones.
fn armar_brew(
    version_gestor: String,
    hojas: BTreeSet<String>,
    versiones: BTreeMap<String, String>,
    casks: BTreeMap<String, String>,
    outdated: &BTreeMap<(String, String), Desactualizado>,
) -> PlanGestor {
    // The update objective: EVERYTHING brew declares outdated, not just
    // the rows. A dependency brew reports outdated is updatable on its
    // own (`brew upgrade --formula simdjson`); `brew upgrade --formula
    // node` would NOT take it along. The PINNED ones stay out: brew
    // refuses to upgrade them, so they are not updatable.
    let objetivo: Vec<GlobalPackage> = outdated
        .iter()
        .filter(|((_, _), d)| !d.pinned)
        .map(|((tipo, nombre), d)| {
            // brew names a formula by its FULL name (`tap/name`) and a
            // cask by its token; the installed version is keyed by the
            // short name (`brew list --versions`) or by the token.
            let installed = match tipo.as_str() {
                TIPO_CASK => casks.get(nombre).cloned().unwrap_or_default(),
                _ => versiones
                    .get(nombre_corto(nombre))
                    .cloned()
                    .unwrap_or_default(),
            };
            GlobalPackage {
                tipo: Some(tipo.clone()),
                name: nombre.clone(),
                installed,
                latest: Some(d.latest.clone()),
                outdated: true,
                pinned: d.pinned,
            }
        })
        .collect();

    let mut packages = Vec::new();
    // Only leaves: dependencies update with their parent, they are not
    // the user's packages (they never reach a table).
    for hoja in hojas {
        // brew's leaves print the FULL name (`tap/name`); `brew list
        // --versions` prints the SHORT one. The row keeps the full
        // name — it is the identity brew uses in `outdated` too.
        let installed = versiones
            .get(nombre_corto(&hoja))
            .cloned()
            .unwrap_or_default();
        packages.push(paquete(TIPO_FORMULA, hoja, installed, outdated));
    }
    for (token, installed) in casks {
        packages.push(paquete(TIPO_CASK, token, installed, outdated));
    }
    packages.sort_by(|a, b| {
        (&a.name, tipo_orden(a.tipo.as_deref())).cmp(&(&b.name, tipo_orden(b.tipo.as_deref())))
    });

    PlanGestor {
        espacio: EspacioGlobal {
            version_gestor,
            version_node: None,
            packages,
        },
        objetivo,
    }
}

/// Display order between kinds: formulae first, casks second (brew's
/// own listing order) — NOT alphabetical, so "cask" never jumps the
/// queue in a name collision.
fn tipo_orden(tipo: Option<&str>) -> u8 {
    match tipo {
        Some(TIPO_CASK) => 1,
        _ => 0,
    }
}

/// One row from the parts: the outdated map decides latest + flag +
/// pinned. A pinned formula only shows its pin when there IS an update
/// to skip — exactly when it matters (brew reports pins in `outdated`).
fn paquete(
    tipo: &str,
    nombre: String,
    installed: String,
    outdated: &BTreeMap<(String, String), Desactualizado>,
) -> GlobalPackage {
    match outdated.get(&(tipo.to_string(), nombre.clone())) {
        Some(d) => GlobalPackage {
            tipo: Some(tipo.to_string()),
            name: nombre,
            installed,
            latest: Some(d.latest.clone()),
            outdated: true,
            pinned: d.pinned,
        },
        None => GlobalPackage {
            tipo: Some(tipo.to_string()),
            name: nombre,
            latest: Some(installed.clone()),
            installed,
            outdated: false,
            pinned: false,
        },
    }
}

/// A plain-listing failure: brew's list/leaves have no "valid exit 1"
/// semantics — anything non-zero with no output is a real failure.
fn guard_salida(out: &RunnerOutput, gestor: &str, comando: &str) -> std::io::Result<()> {
    if out.exit_code != 0 {
        return Err(std::io::Error::other(format!(
            "{gestor} {comando} failed (exit {}): {}",
            out.exit_code,
            out.stderr.trim()
        )));
    }
    Ok(())
}

/// Photo of brew's global space: leaves + casks.
pub fn snapshot(runner: &dyn Runner) -> std::io::Result<PlanGestor> {
    let outdated = runner.run(&["outdated", "--json=v2"])?;
    crate::kernel::guard_json(&outdated, "brew", "outdated", '{')?;
    let leaves = runner.run(&["leaves"])?;
    guard_salida(&leaves, "brew", "leaves")?;
    let formulae = runner.run(&["list", "--versions"])?;
    guard_salida(&formulae, "brew", "list --versions")?;
    let casks = runner.run(&["list", "--cask", "--versions"])?;
    guard_salida(&casks, "brew", "list --cask --versions")?;
    Ok(armar_brew(
        runner.version_gestor(),
        parse_leaves(&leaves.stdout),
        parse_versions(&formulae.stdout),
        parse_versions(&casks.stdout),
        &parse_outdated_v2(&outdated.stdout),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::testutil::FakeRunner;

    #[test]
    fn brew_no_puede_existir_en_windows_ni_pestania_ni_sondeo() {
        // la pestaña nace de instalado(): en Windows ni se arma la
        // cadena de Descubrimiento — el registro (#45) nunca lo alimenta
        assert!(!brew_aplica("windows"));
        assert!(brew_aplica("macos"));
        assert!(brew_aplica("linux"));
    }

    const LEAVES_TAP: &str = "gentleman-programming/tap/engram\nwget\n";
    const FORMULAE_TAP: &str = "engram 3.6.0\nwget 1.25.0\n";
    const OUTDATED_TAP: &str = r#"{
        "formulae": [
            {"name":"gentleman-programming/tap/engram","installed_versions":["3.6.0"],"current_version":"3.7.0","pinned":false,"pinned_version":null}
        ],
        "casks": []
    }"#;

    fn runner_tap(hojas: &str, formulae: &str) -> FakeRunner {
        FakeRunner::new("7.0.11")
            .respuesta("outdated", OUTDATED_TAP, 0)
            .respuesta("leaves", hojas, 0)
            .respuesta_exacta("list --versions", formulae, 0)
            .respuesta_exacta("list --cask --versions", "", 0)
    }

    #[test]
    fn una_hoja_de_tap_aparece_con_su_nombre_completo() {
        let snap = snapshot(&runner_tap(LEAVES_TAP, FORMULAE_TAP)).expect("valid snapshot");
        let engram = fila(&snap, TIPO_FORMULA, "gentleman-programming/tap/engram");
        assert_eq!(engram.installed, "3.6.0");
        assert_eq!(engram.latest.as_deref(), Some("3.7.0"));
        assert!(engram.outdated);
    }

    #[test]
    fn la_hoja_de_tap_encuentra_su_version_por_el_nombre_corto() {
        let snap = snapshot(&runner_tap(LEAVES_TAP, FORMULAE_TAP)).expect("valid snapshot");
        let engram = fila(&snap, TIPO_FORMULA, "gentleman-programming/tap/engram");
        assert_eq!(engram.installed, "3.6.0");
    }

    #[test]
    fn una_hoja_sin_version_resoluble_aparece_con_la_celda_vacia() {
        let snap = snapshot(&runner_tap("otro-tap/huérfano\n", "")).expect("valid snapshot");
        let huerfano = fila(&snap, TIPO_FORMULA, "otro-tap/huérfano");
        assert_eq!(huerfano.installed, "");
    }

    // The objective with a dependency brew declares outdated: simdjson
    // is a dependency of node (not a leaf). Real Homebrew 7.0.11 shape.
    const OUTDATED_CON_DEPENDENCIA: &str = r#"{
        "formulae": [
            {"name":"node","installed_versions":["26.13.0_1"],"current_version":"26.13.0_2","pinned":false,"pinned_version":null},
            {"name":"simdjson","installed_versions":["5.0.6","5.0.7"],"current_version":"5.1.0","pinned":false,"pinned_version":null}
        ],
        "casks": []
    }"#;
    const OUTDATED_DEPENDENCIA_PINEADA: &str = r#"{
        "formulae": [
            {"name":"node","installed_versions":["26.13.0_1"],"current_version":"26.13.0_2","pinned":false,"pinned_version":null},
            {"name":"simdjson","installed_versions":["5.0.7"],"current_version":"5.1.0","pinned":true,"pinned_version":"5.0.7"}
        ],
        "casks": []
    }"#;
    const LEAVES_CON_DEPENDENCIA: &str = "node\n";
    const FORMULAE_CON_DEPENDENCIA: &str = "node 26.13.0_1\nsimdjson 5.0.6 5.0.7\n";

    fn runner_con_dependencia(outdated: &str) -> FakeRunner {
        FakeRunner::new("7.0.11")
            .respuesta("outdated", outdated, 0)
            .respuesta("leaves", LEAVES_CON_DEPENDENCIA, 0)
            .respuesta_exacta("list --versions", FORMULAE_CON_DEPENDENCIA, 0)
            .respuesta_exacta("list --cask --versions", "", 0)
    }

    #[test]
    fn el_objetivo_de_brew_incluye_una_dependencia_que_no_son_fila() {
        let snap =
            snapshot(&runner_con_dependencia(OUTDATED_CON_DEPENDENCIA)).expect("valid snapshot");
        // the table keeps only the leaves: simdjson is a dependency of node
        assert_eq!(snap.espacio.packages.len(), 1);
        assert_eq!(snap.espacio.packages[0].name, "node");
        // the objective is everything brew declares outdated
        let nombres: Vec<&str> = snap.objetivo.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(nombres.len(), 2);
        assert!(nombres.contains(&"node"));
        assert!(nombres.contains(&"simdjson"));
    }

    #[test]
    fn el_objetivo_de_brew_trae_la_version_instalada_de_cada_paquete() {
        let snap =
            snapshot(&runner_con_dependencia(OUTDATED_CON_DEPENDENCIA)).expect("valid snapshot");
        let simdjson = objetivo_de(&snap, TIPO_FORMULA, "simdjson");
        assert_eq!(simdjson.installed, "5.0.6, 5.0.7");
        assert_eq!(simdjson.latest.as_deref(), Some("5.1.0"));
        assert!(simdjson.outdated);
        let node = objetivo_de(&snap, TIPO_FORMULA, "node");
        assert_eq!(node.installed, "26.13.0_1");
        assert_eq!(node.latest.as_deref(), Some("26.13.0_2"));
    }

    #[test]
    fn una_dependencia_pineada_no_entra_en_el_objetivo() {
        // brew reports the pinned dependency outdated, but it refuses to
        // upgrade it: it is not updatable, so it never reaches the
        // objective (nor the count nor the button).
        let snap = snapshot(&runner_con_dependencia(OUTDATED_DEPENDENCIA_PINEADA))
            .expect("valid snapshot");
        let nombres: Vec<&str> = snap.objetivo.iter().map(|p| p.name.as_str()).collect();
        assert!(nombres.contains(&"node"));
        assert!(!nombres.contains(&"simdjson"));
    }

    // Fixtures captured from real Homebrew 7.0.4 shapes
    const OUTDATED_V2: &str = r#"{
        "formulae": [
            {"name":"wget","installed_versions":["1.25.0"],"current_version":"1.26.0","pinned":false,"pinned_version":null},
            {"name":"ffmpeg","installed_versions":["7.1","7.1"],"current_version":"8.0","pinned":true,"pinned_version":"7.1"}
        ],
        "casks": [
            {"name":"firefox","installed_versions":["142.0"],"current_version":"142.1"}
        ]
    }"#;
    const LEAVES: &str = "ffmpeg\nripgrep\nwget\n";
    const FORMULAE: &str = "abseil 20260817.0\nffmpeg 7.1 7.1\nripgrep 14.1.0\nwget 1.25.0\n";
    const CASKS: &str = "firefox 142.0\n";

    fn runner_brew() -> FakeRunner {
        FakeRunner::new("7.0.4")
            .con_node(None)
            .respuesta("outdated", OUTDATED_V2, 0)
            .respuesta("leaves", LEAVES, 0)
            .respuesta_exacta("list --versions", FORMULAE, 0)
            .respuesta_exacta("list --cask --versions", CASKS, 0)
            .respuesta("upgrade", "Upgrading 1 package", 0)
    }

    #[test]
    fn parse_outdated_v2_toma_tipos_latest_y_pinned() {
        let map = parse_outdated_v2(OUTDATED_V2);
        assert_eq!(map.len(), 3);
        let wget = map
            .get(&(TIPO_FORMULA.to_string(), "wget".to_string()))
            .unwrap();
        assert_eq!(wget.latest, "1.26.0");
        assert!(!wget.pinned);
        let ffmpeg = map
            .get(&(TIPO_FORMULA.to_string(), "ffmpeg".to_string()))
            .unwrap();
        assert!(ffmpeg.pinned);
        let firefox = map
            .get(&(TIPO_CASK.to_string(), "firefox".to_string()))
            .unwrap();
        assert_eq!(firefox.latest, "142.1");
        assert!(!firefox.pinned); // casks carry no pinned field
    }

    #[test]
    fn parse_versions_une_las_versiones_multiples() {
        let versiones = parse_versions("openssl 3.0.1 3.0.2\nripgrep 14.1.0\n");
        assert_eq!(
            versiones,
            BTreeMap::from([
                ("openssl".to_string(), "3.0.1, 3.0.2".to_string()),
                ("ripgrep".to_string(), "14.1.0".to_string()),
            ])
        );
    }

    #[test]
    fn snapshot_solo_hojas_y_casks() {
        let runner = runner_brew();
        let snap = snapshot(&runner).expect("valid snapshot");
        assert_eq!(snap.espacio.version_gestor, "7.0.4");
        assert_eq!(snap.espacio.version_node, None);
        // the whole line matters: the fake matches by FIRST argument, so
        // only this assertion catches a dropped `--json=v2`
        assert!(runner.se_llamo_a("outdated --json=v2"));
        // abseil is a dependency: never reaches the table
        assert_eq!(snap.espacio.packages.len(), 4);
        assert!(!snap.espacio.packages.iter().any(|p| p.name == "abseil"));
        let wget = fila(&snap, TIPO_FORMULA, "wget");
        assert!(wget.outdated);
        assert_eq!(wget.latest.as_deref(), Some("1.26.0"));
        assert_eq!(wget.installed, "1.25.0");
        let ripgrep = fila(&snap, TIPO_FORMULA, "ripgrep");
        assert!(!ripgrep.outdated);
        assert_eq!(ripgrep.latest.as_deref(), Some("14.1.0"));
        let ffmpeg = fila(&snap, TIPO_FORMULA, "ffmpeg");
        assert!(ffmpeg.outdated);
        assert_eq!(ffmpeg.installed, "7.1, 7.1");
        // the pin travels with the row (shown as excluded, read-only)
        assert!(ffmpeg.pinned);
        assert!(!wget.pinned);
        let firefox = fila(&snap, TIPO_CASK, "firefox");
        assert!(firefox.outdated);
        assert_eq!(firefox.latest.as_deref(), Some("142.1"));
        // a pinned formula stays OUT of the objective (brew refuses to
        // upgrade it): the row keeps the pin, the objective does not
        let objetivo: Vec<&str> = snap.objetivo.iter().map(|p| p.name.as_str()).collect();
        assert!(objetivo.contains(&"wget"));
        assert!(objetivo.contains(&"firefox"));
        assert!(!objetivo.contains(&"ffmpeg"));
    }

    #[test]
    fn snapshot_vacio_es_valido() {
        let runner = FakeRunner::new("7.0.4")
            .respuesta("outdated", r#"{"formulae":[],"casks":[]}"#, 0)
            .respuesta("leaves", "", 0)
            .respuesta_exacta("list --versions", "", 0)
            .respuesta_exacta("list --cask --versions", "", 0);
        let snap = snapshot(&runner).expect("empty is valid");
        assert!(snap.espacio.packages.is_empty());
    }

    #[test]
    fn snapshot_con_outdated_roto_sin_json_es_error() {
        let runner = FakeRunner::new("7.0.4")
            .respuesta("outdated", "Error: network", 1)
            .respuesta("leaves", "", 0)
            .respuesta_exacta("list --versions", "", 0)
            .respuesta_exacta("list --cask --versions", "", 0);
        assert!(snapshot(&runner).is_err());
    }

    #[test]
    fn snapshot_con_leaves_fallido_es_error() {
        // brew's lists have no "valid exit 1": non-zero is a failure
        let runner = FakeRunner::new("7.0.4")
            .respuesta("outdated", r#"{"formulae":[],"casks":[]}"#, 0)
            .respuesta("leaves", "Error: something", 1)
            .respuesta_exacta("list --versions", "", 0)
            .respuesta_exacta("list --cask --versions", "", 0);
        assert!(snapshot(&runner).is_err());
    }

    #[test]
    fn snapshot_arma_dos_filas_en_colision_de_nombre() {
        // wine exists as formula AND cask: two rows, distinguished by
        // tipo (the update flag derives from it)
        let outdated = r#"{
            "formulae": [{"name":"wine","installed_versions":["9.0"],"current_version":"10.0","pinned":false,"pinned_version":null}],
            "casks": [{"name":"wine","installed_versions":["9.0"],"current_version":"10.1"}]
        }"#;
        let runner = FakeRunner::new("7.0.4")
            .con_node(None)
            .respuesta("outdated", outdated, 0)
            .respuesta("leaves", "wine\n", 0)
            .respuesta_exacta("list --versions", "wine 9.0\n", 0)
            .respuesta_exacta("list --cask --versions", "wine 9.0\n", 0);
        let snap = snapshot(&runner).expect("valid snapshot");
        let vinos: Vec<_> = snap
            .espacio
            .packages
            .iter()
            .filter(|p| p.name == "wine")
            .collect();
        assert_eq!(vinos.len(), 2);
        // formulae first, casks second (display order, not alphabetical)
        assert_eq!(vinos[0].tipo.as_deref(), Some(TIPO_FORMULA));
        assert_eq!(vinos[0].latest.as_deref(), Some("10.0"));
        assert_eq!(vinos[1].tipo.as_deref(), Some(TIPO_CASK));
        assert_eq!(vinos[1].latest.as_deref(), Some("10.1"));
    }

    fn fila<'a>(snap: &'a PlanGestor, tipo: &str, nombre: &str) -> &'a GlobalPackage {
        snap.espacio
            .packages
            .iter()
            .find(|p| p.name == nombre && p.tipo.as_deref() == Some(tipo))
            .unwrap_or_else(|| panic!("fila {tipo}/{nombre} ausente"))
    }

    fn objetivo_de<'a>(snap: &'a PlanGestor, tipo: &str, nombre: &str) -> &'a GlobalPackage {
        snap.objetivo
            .iter()
            .find(|p| p.name == nombre && p.tipo.as_deref() == Some(tipo))
            .unwrap_or_else(|| panic!("objetivo {tipo}/{nombre} ausente"))
    }
}
