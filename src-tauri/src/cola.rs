//! "Update all": sequential queue over the outdated packages of ONE
//! manager's global space (concept in CONTEXT.md's glossary).
//!
//! Semantics — the same that lived in the frontend store, now with
//! locality below the Rust seam:
//! * list order, one at a time, a failure does not stop the queue;
//! * Excluded packages are ALWAYS skipped, even when marked mid-queue
//!   (exclusions are re-read from disk at every step);
//! * "Stop" CUTS the in-flight package (#16: same escalation as the
//!   deadline) and never starts the next one;
//! * on finish it returns summary + final snapshot (a single refresh).

use crate::kernel::{GlobalPackage, Snapshot};
use crate::DefinicionGestor;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Queue accounting: what it ran against, how it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Resumen {
    pub total: usize,
    pub ok: usize,
    pub failed: usize,
    /// Packages CUT mid-flight by Stop (#16): a user decision, not a
    /// failure.
    pub detenidos: usize,
    /// Queue left unrun (stopping during the last package leaves nothing
    /// pending; excluded ones met their fate too: being skipped is not
    /// being stopped).
    pub detenida: bool,
}

/// Why a package's queue result ended the way it did. The wire value is
/// the glossary term ("plazo", never "timeout").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Motivo {
    Ok,
    Fallo,
    #[serde(rename = "plazo")]
    PlazoVencido,
    Detenido,
}

/// A package's final queue result: its output and why it ended.
pub struct ResultadoCola {
    pub paquete: String,
    pub salida: String,
    pub motivo: Motivo,
}

/// What the queue reports as it advances. Output lines go to the log
/// (`pm-output`); starts/result move the table row.
pub enum EventoCola {
    Empieza { paquete: String },
    Linea { paquete: String, linea: String },
    Resultado(ResultadoCola),
}

fn excluidos_de(dir: &Path, gestor: &str) -> HashSet<String> {
    // Corrupt/unreadable (#17): nothing to skip on — the banner asks the
    // user to resolve before this matters.
    match crate::exclusiones::leer(dir) {
        crate::exclusiones::Lectura::Cargado { mapa, .. } => mapa
            .get(gestor)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect(),
        _ => HashSet::new(),
    }
}

/// The queue's shared flags: Stop (#16: cuts the in-flight install),
/// graceful abandonment (the panel went away: finish, don't start) and
/// the ONE-active-queue guard (#12) — app-wide (across tabs): a second
/// start is an explicit error, so it can never silently un-stop the
/// first. Stop and abandonment were ALREADY global; now it's explicit.
pub struct Banderas {
    parar: Arc<AtomicBool>,
    suave: Arc<AtomicBool>,
    activa: Arc<AtomicBool>,
}

impl Banderas {
    pub fn nuevas() -> Self {
        Self {
            parar: Arc::new(AtomicBool::new(false)),
            suave: Arc::new(AtomicBool::new(false)),
            activa: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Shares the same underlying flags (to cross into a thread).
    pub fn compartidas(&self) -> Self {
        Self {
            parar: Arc::clone(&self.parar),
            suave: Arc::clone(&self.suave),
            activa: Arc::clone(&self.activa),
        }
    }

    /// Stop (#16): the in-flight install is CUT.
    pub fn detener(&self) {
        self.parar.store(true, Ordering::Relaxed);
    }

    /// The panel went away: finish the in-flight, start nothing new.
    pub fn abandonar(&self) {
        self.suave.store(true, Ordering::Relaxed);
    }

    /// The ONE-active-queue gate (#12): swaps the flag on entry; the
    /// guard releases it on return AND on a panic — a crashed queue must
    /// not block the next one forever. An error while another queue
    /// holds it.
    pub fn entrar(&self) -> Result<GuardaActiva<'_>, String> {
        if self.activa.swap(true, Ordering::AcqRel) {
            return Err("solo una Actualizar todo a la vez".to_string());
        }
        Ok(GuardaActiva(&self.activa))
    }
}

/// Releases the ONE-active flag on drop (return or panic).
pub struct GuardaActiva<'a>(&'a Arc<AtomicBool>);

impl Drop for GuardaActiva<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Runs the whole queue. Only ONE at a time (#12): while one runs, a
/// second start is rejected with an explicit error — it cannot reset the
/// first one's flags. Once finished, the next one starts normally.
pub fn correr(
    def: &DefinicionGestor,
    dir_config: &Path,
    banderas: &Banderas,
    emitir: &mut dyn FnMut(&EventoCola),
) -> Result<(Resumen, Snapshot), String> {
    let _guarda = banderas.entrar()?;
    correr_activa(def, dir_config, banderas, emitir)
}

/// The accepted queue's body. `parar` is checked before each package AND
/// carried to the engine's watchdog, which CUTS the in-flight one. `suave`
/// is the panel going away: the in-flight package FINISHES (an `npm i -g`
/// cut mid-write leaves a broken install) and the next ones never start.
/// Both reset at the start: every accepted queue starts clean.
fn correr_activa(
    def: &DefinicionGestor,
    dir_config: &Path,
    banderas: &Banderas,
    emitir: &mut dyn FnMut(&EventoCola),
) -> Result<(Resumen, Snapshot), String> {
    let parar = &banderas.parar;
    let suave = banderas.suave.as_ref();
    parar.store(false, Ordering::Relaxed);
    suave.store(false, Ordering::Relaxed);
    // An unresolved exclusions file (#17): the queue would run over
    // EVERYTHING (unknown exclusions) — refuse until the user resolves.
    if !matches!(
        crate::exclusiones::leer(dir_config),
        crate::exclusiones::Lectura::Cargado { .. } | crate::exclusiones::Lectura::Inexistente
    ) {
        return Err(
            "el archivo de exclusiones está dañado o ilegible: resuélvelo antes de Actualizar todo"
                .to_string(),
        );
    }
    let runner = (def.runner)().map_err(|e| e.to_string())?;

    // The queue is built on the real state at start: outdated, not
    // excluded, not pinned (a Pineada is brew's own skip — Nuupa shows
    // it as excluded and never updates it), in list order. Packages
    // travel WHOLE: the type (brew) decides each row's update flag.
    let snap0 = (def.snapshot)(runner.as_ref()).map_err(|e| e.to_string())?;
    let pendientes: Vec<GlobalPackage> = snap0
        .packages
        .iter()
        .filter(|p| {
            p.outdated && !p.pinned && !excluidos_de(dir_config, def.nombre).contains(&p.name)
        })
        .cloned()
        .collect();
    let total = pendientes.len();
    let (mut ok, mut failed, mut detenidos, mut saltados) = (0usize, 0usize, 0usize, 0usize);

    for paquete in &pendientes {
        let name = &paquete.name;
        if parar.load(Ordering::Relaxed) || suave.load(Ordering::Relaxed) {
            break;
        }
        // Re-read from disk: an exclusion marked mid-queue skips the
        // already-enqueued package (the granular exclusion commands write
        // here).
        if excluidos_de(dir_config, def.nombre).contains(name) {
            saltados += 1;
            continue;
        }
        emitir(&EventoCola::Empieza {
            paquete: name.clone(),
        });
        // The update line is built per package by the def (single
        // source of the verb; brew's per-row flag arrives with it).
        let args = (def.args_update)(name, paquete.tipo.as_deref());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let resultado = crate::kernel::instalar(
            runner.as_ref(),
            &refs,
            &mut |linea| {
                emitir(&EventoCola::Linea {
                    paquete: name.clone(),
                    linea: linea.to_string(),
                })
            },
            parar,
        );
        // The engine's error kind carries WHY an install died: TimedOut
        // is the deadline winning (#15); the engine's own Stop error
        // (matched by message — a genuine EINTR from the pipe must not
        // pass as a user decision) is Stop cutting it mid-flight (#16).
        let (salida, motivo) = match resultado {
            Ok(out) => (
                out.output,
                if out.success {
                    Motivo::Ok
                } else {
                    Motivo::Fallo
                },
            ),
            Err(e) => (
                e.to_string(),
                match e.kind() {
                    std::io::ErrorKind::TimedOut => Motivo::PlazoVencido,
                    std::io::ErrorKind::Interrupted
                        if e.to_string().starts_with("detenido a pedido") =>
                    {
                        Motivo::Detenido
                    }
                    _ => Motivo::Fallo,
                },
            ),
        };
        match motivo {
            Motivo::Ok => ok += 1,
            Motivo::Detenido => detenidos += 1,
            _ => failed += 1,
        }
        emitir(&EventoCola::Resultado(ResultadoCola {
            paquete: name.clone(),
            salida,
            motivo,
        }));
    }

    // A single refresh at the end: the final photo already comes with the
    // queue.
    let snapshot_final = (def.snapshot)(runner.as_ref()).map_err(|e| e.to_string())?;
    let resumen = Resumen {
        total,
        ok,
        failed,
        detenidos,
        detenida: ok + failed + detenidos + saltados < total,
    };
    Ok((resumen, snapshot_final.con_comando(def.comando)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::testutil::FakeRunner;
    use crate::kernel::{EspacioGlobal, GlobalPackage as Paquete, Runner, RunnerOutput};
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    // Toy manager definition with npm's protocol: two outdated
    // (context-mode, hunkdiff) and one up to date.
    const LS_JSON: &str = r#"{"dependencies": {
        "@alibaba-group/open-code-review": {"version": "1.10.2"},
        "context-mode": {"version": "1.0.169"},
        "hunkdiff": {"version": "0.17.2"}
    }}"#;
    const OUTDATED_JSON: &str =
        r#"{"context-mode": {"latest": "1.0.170"}, "hunkdiff": {"latest": "0.18.0"}}"#;

    fn def_de_prueba() -> DefinicionGestor {
        DefinicionGestor {
            nombre: "npm",
            comando: "npm i -g",
            args_update: crate::args_npm,
            instalado: || true,
            runner: || {
                Ok(Box::new(
                    FakeRunner::new("11.4.2")
                        .respuesta("ls", LS_JSON, 0)
                        .respuesta("outdated", OUTDATED_JSON, 0)
                        .respuesta("install", "added 1 package in 2s", 0),
                ) as Box<dyn Runner>)
            },
            snapshot: crate::npm::snapshot,
        }
    }

    fn cola_con(def: &DefinicionGestor, dir: &Path) -> (Resumen, Snapshot) {
        let banderas = Banderas::nuevas();
        correr(def, dir, &banderas, &mut |_| {}).unwrap()
    }

    #[test]
    fn actualiza_solo_los_desactualizados_en_orden_y_refresca_al_final() {
        let dir = tempfile::tempdir().unwrap();
        let (resumen, snap) = cola_con(&def_de_prueba(), dir.path());
        assert_eq!(
            resumen,
            Resumen {
                total: 2,
                ok: 2,
                failed: 0,
                detenidos: 0,
                detenida: false
            }
        );
        assert_eq!(snap.comando_actualizar, "npm i -g");
        assert_eq!(snap.espacio.packages.len(), 3);
    }

    #[test]
    fn salta_a_los_excluidos_desde_el_arranque() {
        let dir = tempfile::tempdir().unwrap();
        let mut mapa = std::collections::BTreeMap::new();
        mapa.insert("npm".to_string(), vec!["hunkdiff".to_string()]);
        crate::exclusiones::guardar(dir.path(), &mapa).unwrap();
        let (resumen, _) = cola_con(&def_de_prueba(), dir.path());
        // hunkdiff excluded: the queue is built WITHOUT it
        assert_eq!(resumen.total, 1);
        assert_eq!(resumen.ok, 1);
    }

    #[test]
    fn salta_a_los_pineados_de_brew() {
        // A Pineada is brew's own skip (#38): the queue is built without
        // it — attempting it would find no exact-line answer and fail.
        fn snapshot_pineada(_r: &dyn Runner) -> io::Result<EspacioGlobal> {
            Ok(EspacioGlobal {
                version_gestor: "7.0.4".into(),
                version_node: None,
                packages: vec![
                    Paquete {
                        tipo: Some("formula".into()),
                        name: "ffmpeg".into(),
                        installed: "7.1".into(),
                        latest: Some("8.0".into()),
                        outdated: true,
                        pinned: true,
                    },
                    Paquete {
                        tipo: Some("formula".into()),
                        name: "wget".into(),
                        installed: "1.25.0".into(),
                        latest: Some("1.26.0".into()),
                        outdated: true,
                        pinned: false,
                    },
                ],
            })
        }
        fn args_brew(name: &str, tipo: Option<&str>) -> Vec<String> {
            let flag = match tipo {
                Some("cask") => "--cask",
                _ => "--formula",
            };
            vec!["upgrade".into(), flag.into(), name.into()]
        }
        fn runner_brew() -> io::Result<Box<dyn Runner>> {
            Ok(Box::new(
                crate::kernel::testutil::FakeRunner::new("7.0.4").respuesta_exacta(
                    "upgrade --formula wget",
                    "Upgrading wget",
                    0,
                ),
            ) as Box<dyn Runner>)
        }
        let dir = tempfile::tempdir().unwrap();
        let def = DefinicionGestor {
            nombre: "brew",
            comando: "brew upgrade",
            args_update: args_brew,
            instalado: || true,
            runner: runner_brew,
            snapshot: snapshot_pineada,
        };
        let (resumen, _) = cola_con(&def, dir.path());
        assert_eq!(resumen.total, 1); // only wget: ffmpeg never enqueued
        assert_eq!(resumen.ok, 1);
        assert_eq!(resumen.failed, 0);
    }

    #[test]
    fn la_cola_de_brew_actualiza_formula_y_cask_con_sus_flags() {
        // Both kinds outdated: each row runs with ITS flag, in list
        // order (formulae first).
        fn snapshot_brew(_r: &dyn Runner) -> io::Result<EspacioGlobal> {
            Ok(EspacioGlobal {
                version_gestor: "7.0.4".into(),
                version_node: None,
                packages: vec![
                    Paquete {
                        tipo: Some("formula".into()),
                        name: "wget".into(),
                        installed: "1.25.0".into(),
                        latest: Some("1.26.0".into()),
                        outdated: true,
                        pinned: false,
                    },
                    Paquete {
                        tipo: Some("cask".into()),
                        name: "firefox".into(),
                        installed: "142.0".into(),
                        latest: Some("142.1".into()),
                        outdated: true,
                        pinned: false,
                    },
                ],
            })
        }
        fn args_brew(name: &str, tipo: Option<&str>) -> Vec<String> {
            let flag = match tipo {
                Some("cask") => "--cask",
                _ => "--formula",
            };
            vec!["upgrade".into(), flag.into(), name.into()]
        }
        fn runner_brew() -> io::Result<Box<dyn Runner>> {
            Ok(Box::new(
                crate::kernel::testutil::FakeRunner::new("7.0.4")
                    .respuesta_exacta("upgrade --formula wget", "Upgrading wget", 0)
                    .respuesta_exacta("upgrade --cask firefox", "Upgrading firefox", 0),
            ) as Box<dyn Runner>)
        }
        let def = DefinicionGestor {
            nombre: "brew",
            comando: "brew upgrade",
            args_update: args_brew,
            instalado: || true,
            runner: runner_brew,
            snapshot: snapshot_brew,
        };
        let dir = tempfile::tempdir().unwrap();
        let mut empiezan = Vec::new();
        let banderas = Banderas::nuevas();
        let (resumen, _) = correr(&def, dir.path(), &banderas, &mut |ev| {
            if let EventoCola::Empieza { paquete } = ev {
                empiezan.push(paquete.clone());
            }
        })
        .unwrap();
        assert_eq!(resumen.total, 2);
        assert_eq!(resumen.ok, 2);
        let esperado: Vec<String> = vec!["wget".into(), "firefox".into()];
        assert_eq!(empiezan, esperado); // list order
    }

    #[test]
    fn las_exclusiones_de_nuupa_aplican_tambien_a_los_casks() {
        // Excluding the cask by (gestor=brew, paquete=firefox) — the
        // same granular mechanism as npm, over brew's cask row.
        fn snapshot_brew(_r: &dyn Runner) -> io::Result<EspacioGlobal> {
            Ok(EspacioGlobal {
                version_gestor: "7.0.4".into(),
                version_node: None,
                packages: vec![
                    Paquete {
                        tipo: Some("formula".into()),
                        name: "wget".into(),
                        installed: "1.25.0".into(),
                        latest: Some("1.26.0".into()),
                        outdated: true,
                        pinned: false,
                    },
                    Paquete {
                        tipo: Some("cask".into()),
                        name: "firefox".into(),
                        installed: "142.0".into(),
                        latest: Some("142.1".into()),
                        outdated: true,
                        pinned: false,
                    },
                ],
            })
        }
        fn args_brew(name: &str, tipo: Option<&str>) -> Vec<String> {
            let flag = match tipo {
                Some("cask") => "--cask",
                _ => "--formula",
            };
            vec!["upgrade".into(), flag.into(), name.into()]
        }
        fn runner_brew() -> io::Result<Box<dyn Runner>> {
            Ok(Box::new(
                crate::kernel::testutil::FakeRunner::new("7.0.4").respuesta_exacta(
                    "upgrade --formula wget",
                    "Upgrading wget",
                    0,
                ),
            ) as Box<dyn Runner>)
        }
        let dir = tempfile::tempdir().unwrap();
        let mut mapa = std::collections::BTreeMap::new();
        mapa.insert("brew".to_string(), vec!["firefox".to_string()]);
        crate::exclusiones::guardar(dir.path(), &mapa).unwrap();
        let def = DefinicionGestor {
            nombre: "brew",
            comando: "brew upgrade",
            args_update: args_brew,
            instalado: || true,
            runner: runner_brew,
            snapshot: snapshot_brew,
        };
        let (resumen, _) = cola_con(&def, dir.path());
        assert_eq!(resumen.total, 1); // only wget: the cask is excluded
        assert_eq!(resumen.ok, 1);
    }

    /// Runner with a side effect: installing its first package marks the
    /// second as excluded — simulates "exclude mid-queue".
    struct ExcluyenteAF;
    impl ExcluyenteAF {
        fn new() -> Self {
            ExcluyenteAF
        }
    }
    impl Runner for ExcluyenteAF {
        fn version_gestor(&self) -> String {
            "11.4.2".into()
        }
        fn run(&self, args: &[&str]) -> io::Result<RunnerOutput> {
            if args.first() == Some(&"install") && args.contains(&"context-mode@latest") {
                // while the 1st runs, someone excludes the 2nd
                let dir = EXCLUSION_DIR.with(|d| d.borrow().clone()).unwrap();
                let mut mapa = std::collections::BTreeMap::new();
                mapa.insert("npm".to_string(), vec!["hunkdiff".to_string()]);
                crate::exclusiones::guardar(&dir, &mapa).unwrap();
            }
            PROTOCOLO.with(|p| p.run(args))
        }
    }

    thread_local! {
        static PROTOCOLO: FakeRunner = FakeRunner::new("11.4.2")
            .respuesta("ls", LS_JSON, 0)
            .respuesta("outdated", OUTDATED_JSON, 0)
            .respuesta("install", "added 1 package in 2s", 0);
        static EXCLUSION_DIR: std::cell::RefCell<Option<PathBuf>> =
            const { std::cell::RefCell::new(None) };
    }

    #[test]
    fn excluir_a_mitad_de_cola_salta_al_ya_encolado() {
        let dir = tempfile::tempdir().unwrap();
        EXCLUSION_DIR.with(|d| *d.borrow_mut() = Some(dir.path().to_path_buf()));

        let def = DefinicionGestor {
            runner: || Ok(Box::new(ExcluyenteAF::new()) as Box<dyn Runner>),
            ..def_de_prueba()
        };
        let (resumen, _) = cola_con(&def, dir.path());
        // context-mode ran; hunkdiff got excluded mid-queue: skipped
        assert_eq!(resumen.total, 2); // built with both
        assert_eq!(resumen.ok, 1);
        assert!(!resumen.detenida); // skipping is not stopping
    }

    #[test]
    fn detener_finaliza_el_en_curso_como_detenido_y_no_toca_los_pendientes() {
        let dir = tempfile::tempdir().unwrap();
        let banderas = Banderas::nuevas();
        let def = def_de_prueba();
        let mut eventos = Vec::new();
        let (resumen, _) = correr(&def, dir.path(), &banderas, &mut |ev| match ev {
            EventoCola::Empieza { paquete } => {
                // Stop requested AS the first package starts: the engine
                // cuts it mid-flight (the fake runner is faithful to
                // that), the second one never starts.
                if paquete == "context-mode" {
                    banderas.parar.store(true, Ordering::Relaxed);
                }
                eventos.push(format!("empieza {paquete}"));
            }
            EventoCola::Resultado(r) => {
                eventos.push(format!("resultado {} {:?}", r.paquete, r.motivo))
            }
            EventoCola::Linea { .. } => {}
        })
        .unwrap();
        // the in-flight one was CUT (not failed), the pending one intact
        assert_eq!(
            eventos,
            vec![
                "empieza context-mode".to_string(),
                "resultado context-mode Detenido".to_string(),
            ]
        );
        assert_eq!(
            resumen,
            Resumen {
                total: 2,
                ok: 0,
                failed: 0,
                detenidos: 1,
                detenida: true
            }
        );
    }

    #[test]
    fn detener_entre_paquetes_deja_a_los_pendientes_intactos() {
        // Stop AFTER the first result, BEFORE the second start: nothing
        // was in flight, so nothing gets cut — same as it always was.
        let dir = tempfile::tempdir().unwrap();
        let banderas = Banderas::nuevas();
        let def = def_de_prueba();
        let mut actualizados = Vec::new();
        let (resumen, _) = correr(&def, dir.path(), &banderas, &mut |ev| {
            if let EventoCola::Resultado(_) = ev {
                banderas.parar.store(true, Ordering::Relaxed);
            }
            if let EventoCola::Empieza { paquete } = ev {
                actualizados.push(paquete.clone());
            }
        })
        .unwrap();
        assert_eq!(actualizados, vec!["context-mode"]);
        assert_eq!(
            resumen,
            Resumen {
                total: 2,
                ok: 1,
                failed: 0,
                detenidos: 0,
                detenida: true
            }
        );
    }

    #[test]
    fn abandonar_deja_terminar_el_actual_y_no_empieza_el_siguiente() {
        // The panel going away (`suave`): the in-flight package FINISHES —
        // cutting an npm install mid-write leaves it broken — and the
        // next ones never start.
        let dir = tempfile::tempdir().unwrap();
        let banderas = Banderas::nuevas();
        let def = def_de_prueba();
        let mut actualizados = Vec::new();
        let (resumen, _) = correr(&def, dir.path(), &banderas, &mut |ev| {
            if let EventoCola::Empieza { paquete } = ev {
                if paquete == "context-mode" {
                    banderas.suave.store(true, Ordering::Relaxed);
                }
                actualizados.push(paquete.clone());
            }
        })
        .unwrap();
        // the first one ran to completion; the second never started
        assert_eq!(actualizados, vec!["context-mode"]);
        assert_eq!(
            resumen,
            Resumen {
                total: 2,
                ok: 1,
                failed: 0,
                detenidos: 0,
                detenida: true
            }
        );
    }

    #[test]
    fn sin_desactualizados_la_cola_es_cero_y_no_detenida() {
        let dir = tempfile::tempdir().unwrap();
        let def = DefinicionGestor {
            runner: || {
                Ok(Box::new(
                    FakeRunner::new("11.4.2")
                        .respuesta("ls", LS_JSON, 0)
                        .respuesta("outdated", "", 0),
                ) as Box<dyn Runner>)
            },
            ..def_de_prueba()
        };
        let (resumen, snap) = cola_con(&def, dir.path());
        assert_eq!(
            resumen,
            Resumen {
                total: 0,
                ok: 0,
                failed: 0,
                detenidos: 0,
                detenida: false
            }
        );
        assert!(snap.espacio.packages.iter().all(|p| !p.outdated));
    }

    #[test]
    fn un_fallo_puntual_no_detiene_a_los_demas() {
        // Stateful runner: only the FIRST install fails, the rest succeed.
        struct FlaquezaDelPrimero {
            intentos: std::cell::Cell<usize>,
        }
        impl Runner for FlaquezaDelPrimero {
            fn version_gestor(&self) -> String {
                "11.4.2".into()
            }
            fn run(&self, args: &[&str]) -> io::Result<RunnerOutput> {
                if args.first() == Some(&"install") {
                    let n = self.intentos.get();
                    self.intentos.set(n + 1);
                    return Ok(RunnerOutput {
                        stdout: if n == 0 { "EACCES".into() } else { "ok".into() },
                        stderr: String::new(),
                        exit_code: if n == 0 { 1 } else { 0 },
                    });
                }
                PROTOCOLO.with(|p| p.run(args))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let def = DefinicionGestor {
            runner: || {
                Ok(Box::new(FlaquezaDelPrimero {
                    intentos: std::cell::Cell::new(0),
                }) as Box<dyn Runner>)
            },
            ..def_de_prueba()
        };
        let (resumen, _) = cola_con(&def, dir.path());
        assert_eq!(
            resumen,
            Resumen {
                total: 2,
                ok: 1,
                failed: 1,
                detenidos: 0,
                detenida: false
            }
        );
    }

    /// Runner whose FIRST install hits the deadline: run_streaming errors
    /// with TimedOut, exactly what the engine returns when the watchdog
    /// wins (#15).
    struct ColgadoEnElPrimero {
        intentos: std::cell::Cell<usize>,
    }
    impl Runner for ColgadoEnElPrimero {
        fn version_gestor(&self) -> String {
            "11.4.2".into()
        }
        fn run(&self, args: &[&str]) -> io::Result<RunnerOutput> {
            PROTOCOLO.with(|p| p.run(args))
        }
        fn run_streaming(
            &self,
            _args: &[&str],
            _on_line: &mut dyn FnMut(&str),
            _parar: &Arc<AtomicBool>,
        ) -> io::Result<RunnerOutput> {
            let n = self.intentos.get();
            self.intentos.set(n + 1);
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "npm no respondió en 300 s (proceso finalizado)",
                ));
            }
            Ok(RunnerOutput {
                stdout: "added 1 package in 2s".into(),
                stderr: String::new(),
                exit_code: 0,
            })
        }
    }

    #[test]
    fn timeout_de_un_paquete_lo_marca_con_motivo_y_la_cola_sigue() {
        let dir = tempfile::tempdir().unwrap();
        let def = DefinicionGestor {
            runner: || {
                Ok(Box::new(ColgadoEnElPrimero {
                    intentos: std::cell::Cell::new(0),
                }) as Box<dyn Runner>)
            },
            ..def_de_prueba()
        };
        let banderas = Banderas::nuevas();
        let mut resultados = Vec::new();
        let (resumen, _) = correr(&def, dir.path(), &banderas, &mut |ev| {
            if let EventoCola::Resultado(r) = ev {
                resultados.push((r.paquete.clone(), r.motivo));
            }
        })
        .unwrap();
        // the deadlined one is failed WITH its reason; the next one ran
        assert_eq!(
            resumen,
            Resumen {
                total: 2,
                ok: 1,
                failed: 1,
                detenidos: 0,
                detenida: false
            }
        );
        assert_eq!(
            resultados,
            vec![
                ("context-mode".to_string(), Motivo::PlazoVencido),
                ("hunkdiff".to_string(), Motivo::Ok),
            ]
        );
    }

    #[test]
    fn segundo_arranque_con_una_cola_activa_es_rechazado_y_conserva_el_detener() {
        // A second start (as if another tab raced) runs WHILE the first
        // queue is mid-package: it must be rejected AND leave the first
        // one's Stop intact.
        let dir = tempfile::tempdir().unwrap();
        let banderas = Banderas::nuevas();
        let def = def_de_prueba();
        let (resumen, _) = correr(&def, dir.path(), &banderas, &mut |ev| {
            if let EventoCola::Empieza { paquete } = ev {
                if paquete == "context-mode" {
                    banderas.detener();
                    let err = correr(&def, dir.path(), &banderas, &mut |_| {}).unwrap_err();
                    assert!(err.contains("solo una"));
                    // DIRECT check: the rejected start never reset the
                    // first queue's Stop.
                    assert!(banderas.parar.load(Ordering::Relaxed));
                }
            }
        })
        .unwrap();
        // and the first queue was still CUT
        assert_eq!(
            resumen,
            Resumen {
                total: 2,
                ok: 0,
                failed: 0,
                detenidos: 1,
                detenida: true
            }
        );
    }

    #[test]
    fn terminada_la_cola_se_puede_arrancar_otra() {
        let dir = tempfile::tempdir().unwrap();
        let banderas = Banderas::nuevas();
        let def = def_de_prueba();
        correr(&def, dir.path(), &banderas, &mut |_| {}).unwrap();
        let (segunda, _) = correr(&def, dir.path(), &banderas, &mut |_| {}).unwrap();
        assert_eq!(segunda.ok, 2);
    }

    #[test]
    fn cola_rechazada_con_exclusiones_irresolubles() {
        // #17: with a corrupt exclusions file the queue would run over
        // EVERYTHING (unknown exclusions) — it is refused instead.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("exclusiones.json"), "roto").unwrap();
        let banderas = Banderas::nuevas();
        let err = correr(&def_de_prueba(), dir.path(), &banderas, &mut |_| {}).unwrap_err();
        assert!(err.contains("resuélvelo"), "{err}");
    }
}
