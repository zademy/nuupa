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

use crate::kernel::Snapshot;
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
    crate::exclusiones::cargar(dir)
        .0
        .get(gestor)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Runs the whole queue. `parar` is checked before each package and —
/// via the engine's watchdog — CUTS the in-flight one (#16). It is reset
/// at the start: every new queue starts clean.
pub fn correr(
    def: &DefinicionGestor,
    dir_config: &Path,
    parar: &Arc<AtomicBool>,
    emitir: &mut dyn FnMut(&EventoCola),
) -> Result<(Resumen, Snapshot), String> {
    parar.store(false, Ordering::Relaxed);
    let runner = (def.runner)().map_err(|e| e.to_string())?;

    // The queue is built on the real state at start: outdated, not
    // excluded, in list order.
    let snap0 = (def.snapshot)(runner.as_ref()).map_err(|e| e.to_string())?;
    let pendientes: Vec<String> = snap0
        .packages
        .iter()
        .filter(|p| p.outdated && !excluidos_de(dir_config, def.nombre).contains(&p.name))
        .map(|p| p.name.clone())
        .collect();
    let total = pendientes.len();
    let (mut ok, mut failed, mut detenidos, mut saltados) = (0usize, 0usize, 0usize, 0usize);

    for name in &pendientes {
        if parar.load(Ordering::Relaxed) {
            break;
        }
        // Re-read from disk: an exclusion marked mid-queue skips the
        // already-enqueued package (set_excluded writes here).
        if excluidos_de(dir_config, def.nombre).contains(name) {
            saltados += 1;
            continue;
        }
        emitir(&EventoCola::Empieza {
            paquete: name.clone(),
        });
        let resultado = crate::kernel::instalar(
            runner.as_ref(),
            def.verbo,
            name,
            &mut |linea| {
                emitir(&EventoCola::Linea {
                    paquete: name.clone(),
                    linea: linea.to_string(),
                })
            },
            parar,
        );
        // The engine's error kind carries WHY an install died: TimedOut
        // is the deadline winning (#15), Interrupted is Stop cutting it
        // mid-flight (#16).
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
                    std::io::ErrorKind::Interrupted => Motivo::Detenido,
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
    use crate::kernel::{Runner, RunnerOutput};
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
            verbo: "install",
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
        let parar = Arc::new(AtomicBool::new(false));
        correr(def, dir, &parar, &mut |_| {}).unwrap()
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
        let parar = Arc::new(AtomicBool::new(false));
        let def = def_de_prueba();
        let mut eventos = Vec::new();
        let (resumen, _) = correr(&def, dir.path(), &parar, &mut |ev| match ev {
            EventoCola::Empieza { paquete } => {
                // Stop requested AS the first package starts: the engine
                // cuts it mid-flight (the fake runner is faithful to
                // that), the second one never starts.
                if paquete == "context-mode" {
                    parar.store(true, Ordering::Relaxed);
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
        let parar = Arc::new(AtomicBool::new(false));
        let mut resultados = Vec::new();
        let (resumen, _) = correr(&def, dir.path(), &parar, &mut |ev| {
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
}
