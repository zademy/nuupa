//! Habilidades: agent skills managed at user level in the skills folder
//! (`~/.agents/skills`, glossary in CONTEXT.md).
//!
//! Everything here is a core with its dependencies by parameter (the
//! folder root): no Tauri, no real user folder. The manifest lives NEXT
//! TO the skills (same folder), with the same corruption contract as
//! exclusiones: a file we cannot understand is NEVER overwritten (#17) —
//! it is preserved as evidence (.corrupt) and resolution is explicit.
//!
//! Ticket #26: scan + validation + states derivable offline (No
//! gestionada / Inválida) + manifest read/creation. Actual /
//! Actualización disponible arrive with the remote SHA comparison (#28).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const ARCHIVO: &str = "habilidades.json";
const SKILL_MD: &str = "SKILL.md";

/// The manifest's entry for ONE managed Habilidad: where it came from
/// (repo + ruta) and the SHA of its tree at install time. Written by the
/// add/update flow; read here to know who is Gestionada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origen {
    pub repo: String,
    pub ruta: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entrada {
    pub origen: Origen,
    pub sha: String,
    pub instalada_en: String,
}

pub type Mapa = std::collections::BTreeMap<String, Entrada>;

/// Why the manifest could (not) be read: each cause gets a different
/// treatment — corrupt is an EMERGENCY that blocks writes (#17).
/// `mapa` is only written today; the add flow reads it from #27 on.
#[derive(Debug)]
#[allow(dead_code)]
pub enum Lectura {
    Inexistente,
    Corrupto,
    Ilegible(std::io::Error),
    Cargado { mapa: Mapa },
}

pub fn leer(dir: &Path) -> Lectura {
    let texto = match fs::read_to_string(dir.join(ARCHIVO)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Lectura::Inexistente,
        Err(e) => return Lectura::Ilegible(e),
    };
    match serde_json::from_str(&texto) {
        Ok(mapa) => Lectura::Cargado { mapa },
        Err(_) => Lectura::Corrupto,
    }
}

/// Atomic write (tmp + rename), same rule as exclusiones: a crash
/// mid-write cannot leave a half-written JSON the next read would call
/// corrupt.
pub fn guardar(dir: &Path, mapa: &Mapa) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let destino = dir.join(ARCHIVO);
    let temporal = dir.join(format!("{ARCHIVO}.tmp"));
    fs::write(&temporal, serde_json::to_string_pretty(mapa)?)?;
    fs::rename(&temporal, &destino)
}

/// Preserves the damaged original as evidence (#17): the FIRST evidence
/// is the one that matters; repeated reads never overwrite it.
pub fn resguardar(dir: &Path) -> std::io::Result<()> {
    let resguardo = dir.join(format!("{ARCHIVO}.corrupt"));
    if resguardo.exists() {
        return Ok(());
    }
    fs::copy(dir.join(ARCHIVO), &resguardo).map(|_| ())
}

/// Resolves the corrupt emergency by starting clean (#17): evidence
/// first (.corrupt), then a valid empty map. ONLY acts on a file that is
/// STILL corrupt.
pub fn empezar_de_cero(dir: &Path) -> std::io::Result<()> {
    if !matches!(leer(dir), Lectura::Corrupto) {
        return Ok(()); // already valid (or gone): nothing to resolve
    }
    let _ = resguardar(dir);
    guardar(dir, &Mapa::new())
}

// ---- Validación (glossary: Conforme) ----

/// The front matter a Conforme SKILL.md must carry.
#[derive(Debug, PartialEq, Eq)]
pub struct Frente {
    pub name: String,
    pub description: String,
}

/// Validates ONE Habilidad's folder: SKILL.md present, valid front
/// matter (name in lowercase-with-hyphens, description present, no XML
/// tags). The error string is the row's report: WHY it is Inválida.
pub fn validar(carpeta: &Path) -> Result<Frente, String> {
    let texto = fs::read_to_string(carpeta.join(SKILL_MD))
        .map_err(|_| "sin SKILL.md legible".to_string())?;
    let (name, description) =
        frente_de(&texto).ok_or("SKILL.md sin frontmatter válido (--- con name y description)")?;
    if !nombre_valido(&name) {
        return Err(format!("name inválido: {name:?} (minúsculas-con-guiones)"));
    }
    if description.trim().is_empty() {
        return Err("description vacía".to_string());
    }
    if name.contains('<') || description.contains('<') {
        return Err("etiquetas XML en name/description".to_string());
    }
    Ok(Frente { name, description })
}

/// Minimal front matter: the file must START with an `---` line and have
/// a CLOSING `---` line; between them, `name:` and `description:` with a
/// non-empty value (optional matching quotes are stripped). Everything
/// else in the front matter is ignored.
fn frente_de(texto: &str) -> Option<(String, String)> {
    let mut lineas = texto.lines();
    let primera = lineas.next()?.trim_end();
    if primera.strip_suffix('\r').unwrap_or(primera) != "---" {
        return None;
    }
    let mut name = None;
    let mut description = None;
    let mut cerrado = false;
    for linea in lineas {
        let linea = linea.strip_suffix('\r').unwrap_or(linea);
        if linea.trim_end() == "---" {
            cerrado = true;
            break;
        }
        if let Some(v) = linea.strip_prefix("name:") {
            name = Some(limpiar(v));
        } else if let Some(v) = linea.strip_prefix("description:") {
            description = Some(limpiar(v));
        }
    }
    if !cerrado {
        return None;
    }
    Some((name?, description?))
}

fn limpiar(v: &str) -> String {
    let v = v.trim();
    let v = v
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(v);
    v.trim().to_string()
}

/// The glossary's format: lowercase-with-hyphens (digits allowed), no
/// dots, no spaces, no doubled or edge hyphens.
fn nombre_valido(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !n.starts_with('-')
        && !n.ends_with('-')
        && !n.contains("--")
}

/// A folder name the UI may open: a single safe component under the
/// skills root — never a path, never a hidden or parent entry.
pub fn nombre_seguro(nombre: &str) -> Result<(), String> {
    if nombre.is_empty()
        || nombre.starts_with('.')
        || nombre.contains('/')
        || nombre.contains('\\')
        || nombre.contains("..")
    {
        return Err(format!("nombre de habilidad no seguro: {nombre:?}"));
    }
    Ok(())
}

/// The user's skills folder: `~/.agents/skills` — the ONLY path Nuupa
/// touches (glossary: Carpeta de habilidades).
pub fn carpeta_del_usuario() -> Result<std::path::PathBuf, String> {
    crate::kernel::home()
        .map(|h| h.join(".agents").join("skills"))
        .ok_or_else(|| "no se pudo determinar la carpeta del usuario".to_string())
}

// ---- Estados (glossary) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoHabilidad {
    /// Present without a known Origen: another tool put it there.
    NoGestionada,
    /// Present but its content no longer passes Validación.
    Invalida,
    /// Gestionada whose saved SHA matches the remote one (#28).
    #[allow(dead_code)] // wire value fixed NOW so the frontend mapping never changes
    Actual,
    /// Gestionada whose saved SHA differs from the remote one (#28).
    #[allow(dead_code)]
    ActualizacionDisponible,
}

#[derive(Debug, Serialize)]
pub struct Habilidad {
    pub nombre: String,
    pub estado: EstadoHabilidad,
}

/// The manifest's state for the UI (#17): "corrupto"/"ilegible" block
/// every write until the user resolves.
#[derive(Debug, Serialize)]
pub struct EstadoManifest {
    pub estado: &'static str, // "ok" | "corrupto" | "ilegible"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detalle: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SalidaHabilidades {
    pub habilidades: Vec<Habilidad>,
    pub manifest: EstadoManifest,
}

/// Scans the skills folder (direct children, hidden entries skipped) and
/// derives each row's state. A missing folder is a valid first run: an
/// empty list, not an error. A corrupt manifest still lists the rows —
/// it BLOCKS writes, it does not hide reality.
pub fn listar(raiz: &Path) -> SalidaHabilidades {
    let manifest = match leer(raiz) {
        Lectura::Cargado { .. } | Lectura::Inexistente => EstadoManifest {
            estado: "ok",
            detalle: None,
        },
        Lectura::Corrupto => {
            // evidence FIRST (#17), same as exclusions; a failed copy
            // leaves a trace instead of silence
            let detalle = resguardar(raiz)
                .err()
                .map(|e| format!("(no se pudo conservar la copia .corrupt: {e})"));
            EstadoManifest {
                estado: "corrupto",
                detalle,
            }
        }
        Lectura::Ilegible(e) => EstadoManifest {
            estado: "ilegible",
            detalle: Some(e.to_string()),
        },
    };
    let mut habilidades = Vec::new();
    if let Ok(entradas) = fs::read_dir(raiz) {
        for entrada in entradas.flatten() {
            let nombre = entrada.file_name().to_string_lossy().to_string();
            if nombre.starts_with('.') || !entrada.path().is_dir() {
                continue;
            }
            let estado = if validar(&entrada.path()).is_ok() {
                EstadoHabilidad::NoGestionada
            } else {
                EstadoHabilidad::Invalida
            };
            habilidades.push(Habilidad { nombre, estado });
        }
    }
    habilidades.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    SalidaHabilidades {
        habilidades,
        manifest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carpeta_con_skill_md(texto: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(SKILL_MD), texto).unwrap();
        dir
    }

    const SKILL_OK: &str =
        "---\nname: find-skills\ndescription: Help discovering skills\n---\n\n# body\n";
    const SKILL_OK_COMILLAS: &str = "---\nname: \"otra-skill\"\ndescription: 'Con comillas'\n---\n";

    // ---- Validación: the matrix ----

    #[test]
    fn skill_conforme_pasa_y_extrae_el_frente() {
        let dir = carpeta_con_skill_md(SKILL_OK);
        let frente = validar(dir.path()).unwrap();
        assert_eq!(frente.name, "find-skills");
        assert_eq!(frente.description, "Help discovering skills");
    }

    #[test]
    fn frontmatter_con_comillas_se_limpia() {
        let dir = carpeta_con_skill_md(SKILL_OK_COMILLAS);
        let frente = validar(dir.path()).unwrap();
        assert_eq!(frente.name, "otra-skill");
        assert_eq!(frente.description, "Con comillas");
    }

    #[test]
    fn sin_skill_md_es_invalida() {
        let dir = tempfile::tempdir().unwrap();
        let err = validar(dir.path()).unwrap_err();
        assert!(err.contains("sin SKILL.md"), "{err}");
    }

    #[test]
    fn skill_md_ilegible_es_invalida() {
        let dir = tempfile::tempdir().unwrap();
        let ruta = dir.path().join(SKILL_MD);
        fs::write(&ruta, "texto").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&ruta, fs::Permissions::from_mode(0o000)).unwrap();
            assert!(validar(dir.path()).is_err());
        }
    }

    #[test]
    fn sin_frontmatter_es_invalida() {
        for texto in [
            "solo texto",
            "---\nsin cierre: nunca\n",
            "---\nname: solo-nombre\n---\n", // missing description
            "---\nname: x\nmissing-description: y\n---\n",
        ] {
            let dir = carpeta_con_skill_md(texto);
            let err = validar(dir.path()).unwrap_err();
            assert!(err.contains("frontmatter"), "{texto} → {err}");
        }
    }

    #[test]
    fn name_mal_formado_es_invalida() {
        for name in [
            "ConMayusculas",
            "con espacios",
            "con.puntos",
            "-arranque-guion",
            "cierre-",
            "doble--guion",
            "",
        ] {
            let texto = format!("---\nname: {name}\ndescription: d\n---\n");
            let dir = carpeta_con_skill_md(&texto);
            let err = validar(dir.path()).unwrap_err();
            assert!(err.contains("name inválido"), "{name:?} → {err}");
        }
    }

    #[test]
    fn etiquetas_xml_en_el_frente_son_invalidas() {
        let texto = "---\nname: buen-nombre\ndescription: usa <script>malicioso</script>\n---\n";
        let dir = carpeta_con_skill_md(texto);
        let err = validar(dir.path()).unwrap_err();
        assert!(err.contains("XML"), "{err}");
    }

    #[test]
    fn description_vacia_es_invalida() {
        let dir = carpeta_con_skill_md("---\nname: buen-nombre\ndescription: \"\"\n---\n");
        let err = validar(dir.path()).unwrap_err();
        assert!(err.contains("description vacía"), "{err}");
    }

    // ---- Manifest: the same contract as exclusiones ----

    fn entrada(repo: &str, ruta: &str, sha: &str) -> Entrada {
        Entrada {
            origen: Origen {
                repo: repo.to_string(),
                ruta: ruta.to_string(),
            },
            sha: sha.to_string(),
            instalada_en: "2026-08-29T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn manifest_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut mapa = Mapa::new();
        mapa.insert(
            "markdownlint".to_string(),
            entrada(
                "zademy/skills",
                "skills/productivity/markdownlint",
                "abc123",
            ),
        );
        guardar(dir.path(), &mapa).unwrap();
        match leer(dir.path()) {
            Lectura::Cargado { mapa: cargado } => assert_eq!(cargado, mapa),
            otra => panic!("esperaba Cargado, llegó {otra:?}"),
        }
    }

    #[test]
    fn leer_distingue_inexistente_corrupto_y_valido() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(leer(dir.path()), Lectura::Inexistente));
        fs::write(dir.path().join(ARCHIVO), "no soy json").unwrap();
        assert!(matches!(leer(dir.path()), Lectura::Corrupto));
        // a VALID json with the WRONG shape is also corrupt: the contract
        // is "a file we cannot understand"
        fs::write(dir.path().join(ARCHIVO), r#"["una lista"]"#).unwrap();
        assert!(matches!(leer(dir.path()), Lectura::Corrupto));
    }

    #[test]
    fn resguardar_y_empezar_de_cero_preservan_la_evidencia() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(ARCHIVO), "roto").unwrap();
        empezar_de_cero(dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(format!("{ARCHIVO}.corrupt"))).unwrap(),
            "roto"
        );
        assert!(matches!(
            leer(dir.path()),
            Lectura::Cargado { ref mapa } if mapa.is_empty()
        ));
        // starting clean AGAIN is a no-op: the live file is valid
        empezar_de_cero(dir.path()).unwrap();
        assert!(matches!(leer(dir.path()), Lectura::Cargado { ref mapa } if mapa.is_empty()));
    }

    #[test]
    fn resguardar_no_sobrescribe_la_evidencia_previa() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(ARCHIVO), "roto v2").unwrap();
        fs::write(dir.path().join(format!("{ARCHIVO}.corrupt")), "roto v1").unwrap();
        resguardar(dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(format!("{ARCHIVO}.corrupt"))).unwrap(),
            "roto v1"
        );
    }

    // ---- nombre_seguro ----

    #[test]
    fn nombres_seguros_y_peligrosos() {
        for bueno in ["markdownlint", "open-code-review", "a1-b2"] {
            assert!(nombre_seguro(bueno).is_ok(), "{bueno}");
        }
        for malo in ["", ".", "..", ".oculta", "a/b", "a\\b", "../escape", "a..b"] {
            assert!(nombre_seguro(malo).is_err(), "{malo}");
        }
    }

    // ---- listar: the offline states ----

    #[test]
    fn listar_deriva_no_gestionada_e_invalida_y_ordena() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("buena")).unwrap();
        fs::write(dir.path().join("buena").join(SKILL_MD), SKILL_OK).unwrap();
        fs::create_dir(dir.path().join("rota")).unwrap();
        fs::write(dir.path().join("rota").join(SKILL_MD), "sin frontmatter").unwrap();
        fs::create_dir(dir.path().join(".oculta")).unwrap();
        fs::write(dir.path().join("suelto.txt"), "no es carpeta").unwrap();

        let salida = listar(dir.path());
        assert_eq!(salida.manifest.estado, "ok");
        assert_eq!(
            salida
                .habilidades
                .iter()
                .map(|h| (h.nombre.as_str(), h.estado))
                .collect::<Vec<_>>(),
            [
                ("buena", EstadoHabilidad::NoGestionada),
                ("rota", EstadoHabilidad::Invalida)
            ]
        );
    }

    #[test]
    fn carpeta_inexistente_es_un_arranco_vacio_valido() {
        let dir = tempfile::tempdir().unwrap();
        let salida = listar(&dir.path().join("no-existe"));
        assert!(salida.habilidades.is_empty());
        assert_eq!(salida.manifest.estado, "ok");
    }

    #[test]
    fn manifest_corrupto_se_reporta_y_se_preserva_sin_ocultar_filas() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("buena")).unwrap();
        fs::write(dir.path().join("buena").join(SKILL_MD), SKILL_OK).unwrap();
        fs::write(dir.path().join(ARCHIVO), "roto").unwrap();
        let salida = listar(dir.path());
        assert_eq!(salida.manifest.estado, "corrupto");
        assert_eq!(salida.habilidades.len(), 1); // reality stays visible
        assert!(dir.path().join(format!("{ARCHIVO}.corrupt")).exists());
    }
}
