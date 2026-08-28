//! Persistent "Update all" exclusions, per (manager, package).
//!
//! An Excluded package is skipped in ITS manager's queue but keeps the
//! individual update available (glossary in CONTEXT.md). The JSON is a
//! map { manager: [names] }; the legacy format (flat list) is normalized
//! on read as npm's exclusions and rewritten once.
//!
//! A file we cannot understand is NEVER overwritten (#17): it is
//! preserved as evidence (.corrupt) and writing is refused until the
//! user resolves it.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const ARCHIVO: &str = "exclusiones.json";

type Mapa = BTreeMap<String, Vec<String>>;

/// Why the exclusions file could (not) be read (#17): each cause gets a
/// different treatment — corrupt is an EMERGENCY that blocks writes.
#[derive(Debug)]
pub enum Lectura {
    /// No file yet: a valid first run.
    Inexistente,
    /// The file exists but does not parse: preserve it, refuse to write,
    /// let the user decide.
    Corrupto,
    /// The file exists but reading it failed (permissions, disk).
    Ilegible(std::io::Error),
    /// Read and parsed; `era_legado` marks the v1 flat list that
    /// migrates to the map format on the first write.
    Cargado { mapa: Mapa, era_legado: bool },
}

/// Reads the file distinguishing WHY it could (not) be read (#17).
pub fn leer(dir: &Path) -> Lectura {
    let texto = match fs::read_to_string(dir.join(ARCHIVO)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Lectura::Inexistente,
        Err(e) => return Lectura::Ilegible(e),
    };
    if let Ok(legado) = serde_json::from_str::<Vec<String>>(&texto) {
        // v1 format: everything excluded belonged to npm.
        let mut mapa = Mapa::new();
        if !legado.is_empty() {
            mapa.insert("npm".to_string(), legado);
        }
        return Lectura::Cargado {
            mapa,
            era_legado: true,
        };
    }
    match serde_json::from_str(&texto) {
        Ok(mapa) => Lectura::Cargado {
            mapa,
            era_legado: false,
        },
        Err(_) => Lectura::Corrupto,
    }
}

/// Saves the whole per-manager map, creating the directory if needed.
/// Atomic write (tmp + rename): a crash mid-write cannot leave a
/// half-written JSON that the next startup would treat as corrupt.
pub fn guardar(dir: &Path, mapa: &Mapa) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let destino = dir.join(ARCHIVO);
    let temporal = dir.join(format!("{ARCHIVO}.tmp"));
    fs::write(&temporal, serde_json::to_string_pretty(mapa)?)?;
    fs::rename(&temporal, &destino)
}

/// Excludes one package for one gestor (#14): the granular op the UI
/// asks for — idempotent, repeating it never duplicates.
pub fn excluir(mapa: &mut Mapa, gestor: &str, paquete: &str) {
    let lista = mapa.entry(gestor.to_string()).or_default();
    if !lista.iter().any(|n| n == paquete) {
        lista.push(paquete.to_string());
    }
}

/// Removes one package's exclusion — idempotent: an absent package (or an
/// unknown gestor) is a no-op.
pub fn quitar(mapa: &mut Mapa, gestor: &str, paquete: &str) {
    if let Some(lista) = mapa.get_mut(gestor) {
        lista.retain(|n| n != paquete);
    }
}

/// Preserves the damaged original as evidence (#17): copies the live file
/// to `.corrupt` WITHOUT touching it — the corrupt one stays in place and
/// writing is refused until the user resolves.
pub fn resguardar(dir: &Path) -> std::io::Result<()> {
    fs::copy(dir.join(ARCHIVO), dir.join(format!("{ARCHIVO}.corrupt"))).map(|_| ())
}

/// Resolves the corrupt emergency by starting clean (#17): evidence
/// first (.corrupt), then a valid empty map. Only ever called with the
/// user's explicit choice.
pub fn empezar_de_cero(dir: &Path) -> std::io::Result<()> {
    let _ = resguardar(dir);
    guardar(dir, &Mapa::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guardar_y_leer_hacen_roundtrip_por_gestor() {
        let dir = tempfile::tempdir().unwrap();
        let mut mapa = Mapa::new();
        mapa.insert("npm".to_string(), vec!["hunkdiff".to_string()]);
        mapa.insert(
            "pnpm".to_string(),
            vec!["@org/paquete".to_string(), "otro".to_string()],
        );
        guardar(dir.path(), &mapa).unwrap();
        match leer(dir.path()) {
            Lectura::Cargado {
                mapa: cargado,
                era_legado,
            } => {
                assert_eq!(cargado, mapa);
                assert!(!era_legado);
            }
            otra => panic!("esperaba Cargado, llegó {otra:?}"),
        }
    }

    #[test]
    fn legado_lista_plana_se_normaliza_como_npm() {
        let dir = tempfile::tempdir().unwrap();
        let legado = r#"["hunkdiff", "@org/paquete"]"#;
        fs::write(dir.path().join(ARCHIVO), legado).unwrap();
        match leer(dir.path()) {
            Lectura::Cargado { mapa, era_legado } => {
                assert!(era_legado);
                assert_eq!(
                    mapa.get("npm"),
                    Some(&vec!["hunkdiff".to_string(), "@org/paquete".to_string()])
                );
            }
            otra => panic!("esperaba Cargado, llegó {otra:?}"),
        }
    }

    #[test]
    fn migracion_reescrita_es_idempotente() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(ARCHIVO), r#"["hunkdiff"]"#).unwrap();
        // First load: legacy → rewritten in map format
        let Lectura::Cargado { mapa, era_legado } = leer(dir.path()) else {
            panic!("esperaba Cargado")
        };
        assert!(era_legado);
        guardar(dir.path(), &mapa).unwrap();
        // Second load: no longer legacy
        match leer(dir.path()) {
            Lectura::Cargado {
                mapa: mapa2,
                era_legado: era_legado2,
            } => {
                assert!(!era_legado2);
                assert_eq!(mapa2.get("npm"), Some(&vec!["hunkdiff".to_string()]));
            }
            otra => panic!("esperaba Cargado, llegó {otra:?}"),
        }
    }

    #[test]
    fn guardar_crea_el_directorio_si_no_existe() {
        let dir = tempfile::tempdir().unwrap();
        let anidado = dir.path().join("config/anidado");
        let mut mapa = Mapa::new();
        mapa.insert("bun".to_string(), vec!["headroom-ai".to_string()]);
        guardar(&anidado, &mapa).unwrap();
        match leer(&anidado) {
            Lectura::Cargado { mapa, .. } => {
                assert_eq!(mapa.get("bun"), Some(&vec!["headroom-ai".to_string()]));
            }
            otra => panic!("esperaba Cargado, llegó {otra:?}"),
        }
    }

    #[test]
    fn guardar_vacio_para_un_gestor_no_borra_los_demas() {
        let dir = tempfile::tempdir().unwrap();
        let mut mapa = Mapa::new();
        mapa.insert("npm".to_string(), vec!["hunkdiff".to_string()]);
        mapa.insert("pnpm".to_string(), vec!["otro".to_string()]);
        // simulates removing pnpm's last exclusion (an empty list)
        mapa.insert("pnpm".to_string(), vec![]);
        guardar(dir.path(), &mapa).unwrap();
        match leer(dir.path()) {
            Lectura::Cargado { mapa, .. } => {
                assert_eq!(mapa.get("npm"), Some(&vec!["hunkdiff".to_string()]));
                assert_eq!(mapa.get("pnpm"), Some(&Vec::new()));
            }
            otra => panic!("esperaba Cargado, llegó {otra:?}"),
        }
    }

    #[test]
    fn excluir_es_idempotente_y_por_gestor() {
        let mut mapa = Mapa::new();
        excluir(&mut mapa, "npm", "hunkdiff");
        excluir(&mut mapa, "npm", "hunkdiff"); // repeated: no duplicate
        excluir(&mut mapa, "pnpm", "hunkdiff"); // same name, other gestor
        assert_eq!(mapa.get("npm"), Some(&vec!["hunkdiff".to_string()]));
        assert_eq!(mapa.get("pnpm"), Some(&vec!["hunkdiff".to_string()]));
    }

    #[test]
    fn quitar_es_idempotente_y_no_toca_a_los_demas() {
        let mut mapa = Mapa::new();
        excluir(&mut mapa, "npm", "hunkdiff");
        excluir(&mut mapa, "npm", "context-mode");
        quitar(&mut mapa, "npm", "hunkdiff");
        quitar(&mut mapa, "npm", "hunkdiff"); // already absent: fine
        quitar(&mut mapa, "bun", "cualquiera"); // unknown gestor: fine
        assert_eq!(mapa.get("npm"), Some(&vec!["context-mode".to_string()]));
    }

    // ---- #17: three causes, three treatments ----

    #[test]
    fn leer_distingue_inexistente_corrupto_y_valido() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(leer(dir.path()), Lectura::Inexistente));
        fs::write(dir.path().join(ARCHIVO), "no soy json").unwrap();
        assert!(matches!(leer(dir.path()), Lectura::Corrupto));
        let mut mapa = Mapa::new();
        mapa.insert("npm".to_string(), vec!["hunkdiff".to_string()]);
        guardar(dir.path(), &mapa).unwrap();
        assert!(matches!(
            leer(dir.path()),
            Lectura::Cargado {
                era_legado: false,
                ..
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn leer_distingue_ilegible() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let ruta = dir.path().join(ARCHIVO);
        fs::write(&ruta, "{}").unwrap();
        fs::set_permissions(&ruta, fs::Permissions::from_mode(0o000)).unwrap();
        assert!(matches!(leer(dir.path()), Lectura::Ilegible(_)));
    }

    #[test]
    fn resguardar_copia_el_corrupto_sin_tocar_el_original() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(ARCHIVO), "roto").unwrap();
        resguardar(dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(format!("{ARCHIVO}.corrupt"))).unwrap(),
            "roto"
        );
        // the damaged original stays in place: writes are refused, not
        // destructive
        assert_eq!(
            fs::read_to_string(dir.path().join(ARCHIVO)).unwrap(),
            "roto"
        );
    }

    #[test]
    fn empezar_de_cero_preserva_evidencia_y_arranca_limpio() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(ARCHIVO), "roto").unwrap();
        empezar_de_cero(dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(format!("{ARCHIVO}.corrupt"))).unwrap(),
            "roto"
        );
        // the live file is a VALID empty map
        assert!(matches!(
            leer(dir.path()),
            Lectura::Cargado {
                era_legado: false,
                ..
            }
        ));
    }
}
