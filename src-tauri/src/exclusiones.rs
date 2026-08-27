//! Exclusiones persistentes de "Actualizar todo", por (gestor, paquete).
//!
//! Un paquete Excluido se salta en la cola de SU gestor pero mantiene
//! disponible la actualización individual (glosario en CONTEXT.md). El
//! JSON es un mapa { gestor: [nombres] }; el formato legado (lista plana)
//! se normaliza al leer como exclusiones de npm y se reescribe una vez.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const ARCHIVO: &str = "exclusiones.json";

type Mapa = BTreeMap<String, Vec<String>>;

/// Carga el mapa por gestor. Devuelve `era_legado` cuando el archivo era la
/// lista plana de v1 (o no había nada que conservar): el llamador decide
/// reescribirlo en formato mapa — la migración corre una sola vez.
pub fn cargar(dir: &Path) -> (Mapa, bool) {
    let texto = match fs::read_to_string(dir.join(ARCHIVO)) {
        Ok(t) => t,
        Err(_) => return (Mapa::new(), false),
    };
    if let Ok(legado) = serde_json::from_str::<Vec<String>>(&texto) {
        // Formato v1: todo lo excluido era de npm.
        let mut mapa = Mapa::new();
        if !legado.is_empty() {
            mapa.insert("npm".to_string(), legado);
        }
        return (mapa, true);
    }
    (serde_json::from_str(&texto).unwrap_or_default(), false)
}

/// Guarda el mapa completo por gestor, creando el directorio si hace falta.
/// Escritura atómica (tmp + rename): un crash a mitad de escritura no puede
/// dejar un JSON a medias que el siguiente arranque trataría como corrupto.
pub fn guardar(dir: &Path, mapa: &Mapa) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let destino = dir.join(ARCHIVO);
    let temporal = dir.join(format!("{ARCHIVO}.tmp"));
    fs::write(&temporal, serde_json::to_string_pretty(mapa)?)?;
    fs::rename(&temporal, &destino)
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
        // Primera carga: legado → se reescribe en formato mapa
        let (mapa, era_legado) = cargar(dir.path());
        assert!(era_legado);
        guardar(dir.path(), &mapa).unwrap();
        // Segunda carga: ya no es legado
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
        // simula set_excluded("pnpm", [])
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
        assert!(!era_legado); // corrupto no dispara migración: arranca limpio
    }
}
