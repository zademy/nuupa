//! «Actualizar todo»: cola secuencial sobre los paquetes desactualizados
//! del espacio global de UN gestor (concepto del glosario en CONTEXT.md).
//!
//! Semántica — la misma que vivía en la store del frontend, ahora con
//! localidad debajo de la seam de Rust:
//! * orden de lista, de a uno, un fallo no detiene la cola;
//! * los Excluidos se saltan SIEMPRE, incluso marcados a mitad de cola
//!   (las exclusiones se releen del disco en cada paso);
//! * «Detener» deja terminar el paquete en curso y no empieza el siguiente;
//! * al terminar devuelve resumen + snapshot final (un solo refresco).

use crate::kernel::Snapshot;
use crate::DefinicionGestor;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cuenta de la cola: contra qué se corrió, cómo terminó.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Resumen {
    pub total: usize,
    pub ok: usize,
    pub failed: usize,
    /// Quedó cola sin correr (parar durante el último paquete no deja
    /// pendiente; los excluidos sí corrieron su destino: saltarse no es
    /// detenerse).
    pub detenida: bool,
}

/// Lo que la cola informa conforme avanza. Las líneas de salida van al log
/// (`pm-output`); empieza/resultado mueven la fila de la tabla.
pub enum EventoCola {
    Empieza {
        paquete: String,
    },
    Linea {
        paquete: String,
        linea: String,
    },
    Resultado {
        paquete: String,
        exito: bool,
        salida: String,
    },
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

/// Corre la cola completa. `parar` se consulta antes de cada paquete (y se
/// resetea al empezar: cada cola nueva arranca limpia).
pub fn correr(
    def: &DefinicionGestor,
    dir_config: &Path,
    parar: &AtomicBool,
    emitir: &mut dyn FnMut(&EventoCola),
) -> Result<(Resumen, Snapshot), String> {
    parar.store(false, Ordering::Relaxed);
    let runner = (def.runner)().map_err(|e| e.to_string())?;

    // La cola se construye sobre el estado real al empezar: desactualizados
    // no excluidos, en orden de lista.
    let snap0 = (def.snapshot)(runner.as_ref()).map_err(|e| e.to_string())?;
    let pendientes: Vec<String> = snap0
        .packages
        .iter()
        .filter(|p| p.outdated && !excluidos_de(dir_config, def.nombre).contains(&p.name))
        .map(|p| p.name.clone())
        .collect();
    let total = pendientes.len();
    let (mut ok, mut failed, mut saltados) = (0usize, 0usize, 0usize);

    for name in &pendientes {
        if parar.load(Ordering::Relaxed) {
            break;
        }
        // Releer del disco: una exclusión marcada a mitad de cola salta el
        // paquete ya encolado (set_excluded escribe aquí).
        if excluidos_de(dir_config, def.nombre).contains(name) {
            saltados += 1;
            continue;
        }
        emitir(&EventoCola::Empieza {
            paquete: name.clone(),
        });
        let resultado = crate::kernel::instalar(runner.as_ref(), def.verbo, name, &mut |linea| {
            emitir(&EventoCola::Linea {
                paquete: name.clone(),
                linea: linea.to_string(),
            })
        });
        let (exito, salida) = match resultado {
            Ok(out) => (out.success, out.output),
            Err(e) => (false, e.to_string()),
        };
        if exito {
            ok += 1;
        } else {
            failed += 1;
        }
        emitir(&EventoCola::Resultado {
            paquete: name.clone(),
            exito,
            salida,
        });
    }

    // Un solo refresco al final: la foto final ya viene con la cola.
    let snapshot_final = (def.snapshot)(runner.as_ref()).map_err(|e| e.to_string())?;
    let resumen = Resumen {
        total,
        ok,
        failed,
        detenida: ok + failed + saltados < total,
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

    // Definición de gestor de juguete con protocolo npm: dos desactualizados
    // (context-mode, hunkdiff) y uno al día.
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
        let parar = AtomicBool::new(false);
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
        // hunkdiff excluido: la cola se construye SIN él
        assert_eq!(resumen.total, 1);
        assert_eq!(resumen.ok, 1);
    }

    /// Runner con efecto: al instalar su primer paquete marca al segundo
    /// como excluido — simula "excluir a mitad de cola".
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
                // mientras corre el 1º, alguien excluye al 2º
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
        // context-mode corrió; hunkdiff quedó excluido a mitad: saltado
        assert_eq!(resumen.total, 2); // se construyó con los dos
        assert_eq!(resumen.ok, 1);
        assert!(!resumen.detenida); // saltarse no es detenerse
    }

    #[test]
    fn detener_deja_terminar_el_actual_y_no_empieza_el_siguiente() {
        let dir = tempfile::tempdir().unwrap();
        let parar = AtomicBool::new(false);
        let def = def_de_prueba();
        let mut actualizados = Vec::new();
        let (resumen, _) = correr(&def, dir.path(), &parar, &mut |ev| {
            if let EventoCola::Empieza { paquete } = ev {
                // Detener apenas empieza el PRIMERO: debe terminar, y el
                // segundo no debe arrancar nunca.
                if paquete == "context-mode" {
                    parar.store(true, Ordering::Relaxed);
                }
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
                detenida: false
            }
        );
        assert!(snap.espacio.packages.iter().all(|p| !p.outdated));
    }

    #[test]
    fn un_fallo_puntual_no_detiene_a_los_demas() {
        // Runner con memoria: falla solo el PRIMER install, acierta el resto.
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
                detenida: false
            }
        );
    }
}
