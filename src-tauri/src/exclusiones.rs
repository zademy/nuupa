//! Persistent "Update all" exclusions, per (manager, package).
//!
//! An Excluded package is skipped in ITS manager's queue but keeps the
//! individual update available (glossary in CONTEXT.md). The JSON is a
//! map { manager: [names] }; the legacy format (flat list) is normalized
//! on read as npm's exclusions and rewritten once.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const ARCHIVO: &str = "exclusiones.json";

type Mapa = BTreeMap<String, Vec<String>>;

/// Loads the per-manager map. Returns `era_legado` when the file was the
/// v1 flat list (or there was nothing to preserve): the caller decides
/// to rewrite it in map format — the migration runs once.
pub fn cargar(dir: &Path) -> (Mapa, bool) {
    let texto = match fs::read_to_string(dir.join(ARCHIVO)) {
        Ok(t) => t,
        Err(_) => return (Mapa::new(), false),
    };
    if let Ok(legado) = serde_json::from_str::<Vec<String>>(&texto) {
        // v1 format: everything excluded belonged to npm.
        let mut mapa = Mapa::new();
        if !legado.is_empty() {
            mapa.insert("npm".to_string(), legado);
        }
        return (mapa, true);
    }
    (serde_json::from_str(&texto).unwrap_or_default(), false)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargar_sin_archivo_devuelve_mapa_vacio_sin_legado() {
        let dir = tempfile::tempdir().unwrap();
        let (mapa, era_legado) = cargar(dir.path());
        assert!(mapa.is_empty());
        assert!(!era_legado);
    }

    #[test]
    fn guardar_y_cargar_hacen_roundtrip_por_gestor() {
        let dir = tempfile::tempdir().unwrap();
        let mut mapa = Mapa::new();
        mapa.insert("npm".to_string(), vec!["hunkdiff".to_string()]);
        mapa.insert(
            "pnpm".to_string(),
            vec!["@org/paquete".to_string(), "otro".to_string()],
        );
        guardar(dir.path(), &mapa).unwrap();
        let (cargado, era_legado) = cargar(dir.path());
        assert_eq!(cargado, mapa);
        assert!(!era_legado);
    }

    #[test]
    fn legado_lista_plana_se_normaliza_como_npm() {
        let dir = tempfile::tempdir().unwrap();
        let legado = r#"["hunkdiff", "@org/paquete"]"#;
        fs::write(dir.path().join(ARCHIVO), legado).unwrap();
        let (mapa, era_legado) = cargar(dir.path());
        assert!(era_legado);
        assert_eq!(
            mapa.get("npm"),
            Some(&vec!["hunkdiff".to_string(), "@org/paquete".to_string()])
        );
    }

    #[test]
    fn migracion_reescrita_es_idempotente() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(ARCHIVO), r#"["hunkdiff"]"#).unwrap();
        // First load: legacy → rewritten in map format
        let (mapa, era_legado) = cargar(dir.path());
        assert!(era_legado);
        guardar(dir.path(), &mapa).unwrap();
        // Second load: no longer legacy
        let (mapa2, era_legado2) = cargar(dir.path());
        assert!(!era_legado2);
        assert_eq!(mapa2.get("npm"), Some(&vec!["hunkdiff".to_string()]));
    }

    #[test]
    fn guardar_crea_el_directorio_si_no_existe() {
        let dir = tempfile::tempdir().unwrap();
        let anidado = dir.path().join("config/anidado");
        let mut mapa = Mapa::new();
        mapa.insert("bun".to_string(), vec!["headroom-ai".to_string()]);
        guardar(&anidado, &mapa).unwrap();
        assert_eq!(
            cargar(&anidado).0.get("bun"),
            Some(&vec!["headroom-ai".to_string()])
        );
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
        let (cargado, _) = cargar(dir.path());
        assert_eq!(cargado.get("npm"), Some(&vec!["hunkdiff".to_string()]));
        assert_eq!(cargado.get("pnpm"), Some(&Vec::new()));
    }

    #[test]
    fn archivo_corrupto_arranca_limpio() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(ARCHIVO), "no soy json").unwrap();
        let (mapa, era_legado) = cargar(dir.path());
        assert!(mapa.is_empty());
        assert!(!era_legado); // corrupt does not trigger migration: starts clean
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
}
