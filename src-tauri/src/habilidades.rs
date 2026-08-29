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
use std::path::{Path, PathBuf};

use crate::cola;

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
    Actual,
    /// Gestionada whose saved SHA differs from the remote one (#28).
    ActualizacionDisponible,
    /// Gestionada whose SHA could not be checked right now (#28): a
    /// network failure, never a verdict. The row carries the reason.
    SinVerificar,
}

#[derive(Debug, Serialize)]
pub struct Habilidad {
    pub nombre: String,
    pub estado: EstadoHabilidad,
    /// WHY the state could not be verified (#28): a per-row network
    /// failure — the row keeps its place, the rest still refresh.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
///
/// The REAL refresh (#28): every Gestionada's saved SHA is compared
/// against the remote one — same SHA → Actual, different → Actualización
/// disponible, unreachable → SinVerificar with the reason on ITS row
/// (one failure never blocks the rest). `chequeo == None` is the offline
/// derivation (tests only).
pub fn refrescar(raiz: &Path, proveedor: &dyn ProveedorRemoto) -> SalidaHabilidades {
    derivar(raiz, Some(proveedor))
}

fn derivar(raiz: &Path, chequeo: Option<&dyn ProveedorRemoto>) -> SalidaHabilidades {
    // One read carries BOTH the manifest's emergency state (#17) and the
    // managed entries the SHA check needs (#28).
    let (mapa, manifest) = match leer(raiz) {
        Lectura::Cargado { mapa } => (
            Some(mapa),
            EstadoManifest {
                estado: "ok",
                detalle: None,
            },
        ),
        Lectura::Inexistente => (
            None,
            EstadoManifest {
                estado: "ok",
                detalle: None,
            },
        ),
        Lectura::Corrupto => {
            // evidence FIRST (#17), same as exclusions; a failed copy
            // leaves a trace instead of silence
            let detalle = resguardar(raiz)
                .err()
                .map(|e| format!("(no se pudo conservar la copia .corrupt: {e})"));
            (
                None,
                EstadoManifest {
                    estado: "corrupto",
                    detalle,
                },
            )
        }
        Lectura::Ilegible(e) => (
            None,
            EstadoManifest {
                estado: "ilegible",
                detalle: Some(e.to_string()),
            },
        ),
    };
    let mut habilidades = Vec::new();
    if let Ok(entradas) = fs::read_dir(raiz) {
        for entrada in entradas.flatten() {
            let nombre = entrada.file_name().to_string_lossy().to_string();
            if nombre.starts_with('.') || !entrada.path().is_dir() {
                continue;
            }
            let conforme = validar(&entrada.path()).is_ok();
            let estado = if !conforme {
                EstadoHabilidad::Invalida
            } else {
                match mapa.as_ref().and_then(|m| m.get(&nombre)) {
                    // Managed + Conforme: the SHA decides (#28).
                    Some(gestionada) => match chequeo {
                        None => EstadoHabilidad::NoGestionada, // offline path
                        Some(proveedor) => {
                            match proveedor.sha_de(&gestionada.origen.repo, &gestionada.origen.ruta)
                            {
                                Ok(sha) if sha == gestionada.sha => EstadoHabilidad::Actual,
                                Ok(_) => EstadoHabilidad::ActualizacionDisponible,
                                Err(e) => {
                                    // ITS row carries the failure; the
                                    // rest of the list still refreshes.
                                    habilidades.push(Habilidad {
                                        nombre,
                                        estado: EstadoHabilidad::SinVerificar,
                                        error: Some(e),
                                    });
                                    continue;
                                }
                            }
                        }
                    },
                    None => EstadoHabilidad::NoGestionada,
                }
            };
            habilidades.push(Habilidad {
                nombre,
                estado,
                error: None,
            });
        }
    }
    habilidades.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    SalidaHabilidades {
        habilidades,
        manifest,
    }
}

// ---- Origen: the URL forms accepted (#27) ----

/// A parsed origin: the repo (`owner/repo`), an optional whole-tree
/// reference, an optional direct skill's ruta inside the repo and an
/// optional skills.sh slug to filter the scan by (#31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoUrl {
    pub repo: String,
    pub referencia: Option<String>,
    pub ruta: Option<String>,
    pub slug: Option<String>,
}

/// Accepts `owner/repo`, `github.com/owner/repo`,
/// `https://github.com/owner/repo` and the `/tree/{ref}[/ruta]` forms —
/// plus the skills.sh shortcut `skills.sh/{owner}/{repo}/{slug}` (#31),
/// resolved by filtering the repo scan on the slug. Anything else
/// (another host, traversal, wrong arity) is an error — the error string
/// is the UI's report.
pub fn parsear_origen(texto: &str) -> Result<RepoUrl, String> {
    let mal = |porque: String| format!("origen no válido ({porque}): {texto}");
    let t = texto.trim();
    let t = t
        .strip_prefix("https://")
        .or_else(|| t.strip_prefix("http://"))
        .unwrap_or(t);
    let t = t.strip_prefix("www.").unwrap_or(t);
    let t = t.trim_end_matches('/');
    if t.is_empty() {
        return Err(mal("falta owner/repo".into()));
    }

    // skills.sh shortcut: the id shape owner/repo/slug — the repo scan
    // filters on the slug (#31). No ruta, no ref.
    if let Ok(resto) = t.strip_prefix("skills.sh/").ok_or(()) {
        let segmentos: Vec<&str> = resto.split('/').filter(|s| !s.is_empty()).collect();
        if segmentos.len() != 3 {
            return Err(mal("skills.sh espera owner/repo/slug".into()));
        }
        for s in &segmentos {
            if *s == "." || *s == ".." || s.starts_with('.') {
                return Err(mal(format!("segmento no seguro: {s:?}")));
            }
        }
        return Ok(RepoUrl {
            repo: format!("{}/{}", segmentos[0], segmentos[1]),
            referencia: None,
            ruta: None,
            slug: Some(segmentos[2].to_string()),
        });
    }

    let (repo_part, resto) = match t.split_once("/tree/") {
        Some((r, resto)) => (r, Some(resto)),
        None => (t, None),
    };
    let segmentos: Vec<&str> = repo_part.split('/').filter(|s| !s.is_empty()).collect();
    // Two forms: `github.com/owner/repo` or bare `owner/repo` (the
    // skills.sh id shape). Anything else — another host, a subpath — is
    // refused.
    let repo = match segmentos.as_slice() {
        [host, o, r] if *host == "github.com" => format!("{o}/{r}"),
        // bare owner/repo — but "github.com" alone is a host remnant,
        // never an owner
        [o, r] if *o != "github.com" => format!("{o}/{r}"),
        _ => return Err(mal("se esperaba owner/repo en github.com".into())),
    };
    for s in &segmentos[segmentos.len() - 2..] {
        if *s == "." || *s == ".." || s.starts_with('.') {
            return Err(mal(format!("segmento de repo no seguro: {s:?}")));
        }
    }
    let (referencia, ruta) = match resto {
        None => (None, None),
        Some(resto) => {
            let mut partes = resto.splitn(2, '/');
            let r = partes.next().unwrap_or("").trim_end_matches('/');
            if r.is_empty() {
                (None, None)
            } else {
                match partes.next() {
                    Some(ruta) if !ruta.trim_end_matches('/').is_empty() => (
                        Some(r.to_string()),
                        Some(ruta.trim_end_matches('/').to_string()),
                    ),
                    _ => (Some(r.to_string()), None),
                }
            }
        }
    };
    if let Some(ruta) = &ruta {
        ruta_segura(ruta).map_err(mal)?;
    }
    Ok(RepoUrl {
        repo,
        referencia,
        ruta,
        slug: None,
    })
}

/// A ruta inside a repo that may become paths: no empty, hidden, parent
/// or absolute components.
fn ruta_segura(ruta: &str) -> Result<(), String> {
    if ruta.is_empty()
        || ruta.starts_with('/')
        || ruta.split('/').any(|p| p.is_empty() || p.starts_with('.'))
    {
        return Err(format!("ruta no segura: {ruta:?}"));
    }
    Ok(())
}

// ---- Proveedor remoto (the injected dependency; #27) ----

pub trait ProveedorRemoto {
    /// Extracts the repo tree at its reference into `destino` (created if
    /// needed); exactly one top directory remains there.
    fn descargar_en(&self, origen: &RepoUrl, destino: &Path) -> Result<(), String>;
    /// The latest commit SHA touching `ruta` in the repo.
    fn sha_de(&self, repo: &str, ruta: &str) -> Result<String, String>;
}

/// The real provider: GitHub public, no auth. Synchronous HTTP — every
/// caller runs it on a blocking pool thread. Timeouts bound the wait
/// (the Plazo concept applied to the network).
pub struct ProveedorReal {
    agente: ureq::Agent,
}

impl ProveedorReal {
    pub fn nuevo() -> Result<Self, String> {
        Ok(Self {
            agente: ureq::AgentBuilder::new()
                .timeout_connect(std::time::Duration::from_secs(15))
                .timeout(std::time::Duration::from_secs(120))
                .build(),
        })
    }

    fn get(&self, url: &str) -> Result<ureq::Response, String> {
        self.agente
            .get(url)
            .set("User-Agent", "nuupa")
            .set("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| format!("no se pudo contactar a GitHub: {e}"))
    }
}

impl ProveedorRemoto for ProveedorReal {
    fn descargar_en(&self, origen: &RepoUrl, destino: &Path) -> Result<(), String> {
        fs::create_dir_all(destino).map_err(|e| e.to_string())?;
        let url = match &origen.referencia {
            Some(r) => format!("https://api.github.com/repos/{}/tarball/{r}", origen.repo),
            None => format!("https://api.github.com/repos/{}/tarball", origen.repo),
        };
        let lector = self.get(&url)?.into_reader();
        let gz = flate2::read::GzDecoder::new(lector);
        tar::Archive::new(gz)
            .unpack(destino)
            .map_err(|e| format!("no se pudo extraer el repositorio: {e}"))
    }

    fn sha_de(&self, repo: &str, ruta: &str) -> Result<String, String> {
        let url = format!("https://api.github.com/repos/{repo}/commits?path={ruta}&per_page=1");
        let texto = self
            .get(&url)?
            .into_string()
            .map_err(|e| format!("respuesta ilegible: {e}"))?;
        if let Some(sha) = primer_sha(&texto) {
            return Ok(sha);
        }
        // A ruta with no commits of its own (an empty array): fall back
        // to the repo's HEAD so the skill stays trackable (#28).
        let texto = self
            .get(&format!(
                "https://api.github.com/repos/{repo}/commits?per_page=1"
            ))?
            .into_string()
            .map_err(|e| format!("respuesta ilegible: {e}"))?;
        primer_sha(&texto).ok_or_else(|| "respuesta sin sha".to_string())
    }
}

/// The first commit's sha of a GitHub commits-list JSON response.
fn primer_sha(texto: &str) -> Option<String> {
    let valor: serde_json::Value = serde_json::from_str(texto).ok()?;
    valor
        .as_array()?
        .first()?
        .get("sha")?
        .as_str()
        .map(str::to_string)
}

/// The tarball's single top directory (`repo-<ref>`); a GitHub tarball
/// always has exactly one — anything else is a surprise we refuse.
fn raiz_del_arbol(staging: &Path) -> Result<PathBuf, String> {
    let mut entradas: Vec<PathBuf> = Vec::new();
    for entrada in fs::read_dir(staging).map_err(|e| e.to_string())?.flatten() {
        if entrada.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        entradas.push(entrada.path());
    }
    match entradas.len() {
        1 => Ok(entradas.remove(0)),
        0 => Err("el repositorio llegó vacío".to_string()),
        _ => Err("el repositorio llegó con varias raíces".to_string()),
    }
}

// ---- Escaneo (#27): the report before anything is activated ----

#[derive(Debug, Clone, Serialize)]
pub struct HabilidadEscaneada {
    /// Ruta of the skill folder inside the repo (forward slashes).
    pub ruta: String,
    /// The skill's frontmatter name — the slug matcher (#31).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub conforme: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motivo: Option<String>,
}

/// Downloads the origin's tree into `staging` and returns its single
/// top directory — the shared first phase of escanear/instalar/actualizar.
fn descargar_arbol(
    proveedor: &dyn ProveedorRemoto,
    origen: &RepoUrl,
    staging: &Path,
) -> Result<PathBuf, String> {
    proveedor.descargar_en(origen, staging)?;
    raiz_del_arbol(staging)
}

/// Downloads the repo into `staging` (a throwaway dir the caller owns)
/// and reports every skill folder found: Conforme or Inválida with its
/// reason. NOTHING is activated here. With a skills.sh slug (#31) the
/// report is filtered to THAT skill — by folder name or frontmatter
/// name; an unknown slug is an explicit error, never silence.
pub fn escanear(
    proveedor: &dyn ProveedorRemoto,
    origen: &RepoUrl,
    staging: &Path,
) -> Result<Vec<HabilidadEscaneada>, String> {
    let raiz = descargar_arbol(proveedor, origen, staging)?;
    let mut encontradas = Vec::new();
    match &origen.ruta {
        // Direct skill: only that folder, present or not.
        Some(ruta) => {
            let carpeta = raiz.join(ruta);
            encontradas.push(if carpeta.is_dir() {
                candidato(&raiz, &carpeta)
            } else {
                HabilidadEscaneada {
                    ruta: ruta.clone(),
                    name: None,
                    conforme: false,
                    motivo: Some("la carpeta no existe en el repositorio".to_string()),
                }
            });
        }
        // Whole repo: every folder with a SKILL.md, recursively.
        None => recolectar(&raiz, &raiz, &mut encontradas),
    }
    // skills.sh shortcut: keep only the slug's skill (#31).
    if let Some(slug) = &origen.slug {
        let coincide = |h: &HabilidadEscaneada| {
            h.name.as_deref() == Some(slug.as_str())
                || h.ruta.rsplit('/').next() == Some(slug.as_str())
        };
        if !encontradas.iter().any(coincide) {
            return Err(format!(
                "no se encontró la habilidad «{slug}» en el repositorio"
            ));
        }
        encontradas.retain(coincide);
    }
    encontradas.sort_by(|a, b| a.ruta.cmp(&b.ruta));
    Ok(encontradas)
}

/// A folder holding a SKILL.md is a candidate (validated, NOT recursed
/// into — its files are the bundle); otherwise recurse. Hidden folders
/// are never candidates.
fn recolectar(base: &Path, dir: &Path, out: &mut Vec<HabilidadEscaneada>) {
    if dir.join(SKILL_MD).is_file() {
        out.push(candidato(base, dir));
        return;
    }
    for entrada in fs::read_dir(dir).into_iter().flatten().flatten() {
        let nombre = entrada.file_name().to_string_lossy().to_string();
        if nombre.starts_with('.') || !entrada.path().is_dir() {
            continue;
        }
        recolectar(base, &entrada.path(), out);
    }
}

fn candidato(base: &Path, carpeta: &Path) -> HabilidadEscaneada {
    let ruta = carpeta
        .strip_prefix(base)
        .unwrap_or(carpeta)
        .to_string_lossy()
        .replace('\\', "/");
    match validar(carpeta) {
        Ok(frente) => HabilidadEscaneada {
            ruta,
            name: Some(frente.name),
            conforme: true,
            motivo: None,
        },
        Err(e) => HabilidadEscaneada {
            ruta,
            name: None,
            conforme: false,
            motivo: Some(e),
        },
    }
}

// ---- Instalación (#27): two phases per skill, Origen recorded ----

#[derive(Debug, Clone, Serialize)]
pub struct ResultadoInstalacion {
    pub ruta: String,
    /// The installed folder's name (the ruta's leaf).
    pub nombre: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motivo: Option<String>,
}

/// Installs the requested rutas of ONE origen: download once to staging,
/// then per ruta — validate the content, fetch its SHA, activate by
/// copy-and-swap, record the Origen in `mapa`. Invalid content or a
/// failed SHA never activates; one failure does not stop the rest. The
/// caller persists `mapa` ONCE after (a single manifest write).
pub fn instalar(
    proveedor: &dyn ProveedorRemoto,
    origen: &RepoUrl,
    rutas: &[String],
    raiz_habilidades: &Path,
    mapa: &mut Mapa,
    ahora_epoch: i64,
) -> Result<Vec<ResultadoInstalacion>, String> {
    fs::create_dir_all(raiz_habilidades).map_err(|e| e.to_string())?;
    let staging = tempfile::tempdir().map_err(|e| e.to_string())?;
    let raiz = descargar_arbol(proveedor, origen, staging.path())?;
    let mut resultados = Vec::new();
    for ruta in rutas {
        resultados.push(instalar_una(
            proveedor,
            origen,
            ruta,
            &Destino {
                nombre: nombre_hoja(ruta),
                ahora_epoch,
            },
            &raiz,
            raiz_habilidades,
            mapa,
        ));
    }
    Ok(resultados)
}

/// Where and when a downloaded skill lands (#27): its DESTINATION name
/// under the skills root and the write timestamp.
struct Destino {
    nombre: String,
    ahora_epoch: i64,
}

fn instalar_una(
    proveedor: &dyn ProveedorRemoto,
    origen: &RepoUrl,
    ruta: &str,
    destino: &Destino,
    raiz_repo: &Path,
    raiz_habilidades: &Path,
    mapa: &mut Mapa,
) -> ResultadoInstalacion {
    // The DESTINATION name is the caller's identity: the ruta's leaf for
    // a fresh install, the LOCAL folder name for an update — updating
    // must replace the user's folder, never spawn a renamed duplicate.
    let nombre = destino.nombre.clone();
    let ahora_epoch = destino.ahora_epoch;
    let falla = |motivo: String| ResultadoInstalacion {
        ruta: ruta.to_string(),
        nombre: nombre.clone(),
        ok: false,
        motivo: Some(motivo),
    };
    if let Err(e) = ruta_segura(ruta) {
        return falla(e);
    }
    let carpeta = raiz_repo.join(ruta);
    // Phase 2 gate: the NEW content must be Conforme before activation.
    if let Err(e) = validar(&carpeta) {
        return falla(format!("inválida: {e}"));
    }
    let sha = match proveedor.sha_de(&origen.repo, ruta) {
        Ok(s) => s,
        Err(e) => return falla(format!("sin SHA del origen: {e}")),
    };
    // Activation: copy to a hidden staging folder inside the skills
    // root, then swap — a crash can never leave a half-copied skill.
    let destino = raiz_habilidades.join(&nombre);
    let temporal = raiz_habilidades.join(format!(".instalando-{nombre}"));
    let _ = fs::remove_dir_all(&temporal);
    if let Err(e) = copiar_dir(&carpeta, &temporal) {
        let _ = fs::remove_dir_all(&temporal);
        return falla(format!("no se pudo copiar: {e}"));
    }
    let _ = fs::remove_dir_all(&destino);
    if let Err(e) = fs::rename(&temporal, &destino) {
        let _ = fs::remove_dir_all(&temporal);
        return falla(format!("no se pudo activar: {e}"));
    }
    mapa.insert(
        nombre.clone(),
        Entrada {
            origen: Origen {
                repo: origen.repo.clone(),
                ruta: ruta.to_string(),
            },
            sha,
            instalada_en: ahora_epoch.to_string(),
        },
    );
    ResultadoInstalacion {
        ruta: ruta.to_string(),
        nombre,
        ok: true,
        motivo: None,
    }
}

fn nombre_hoja(ruta: &str) -> String {
    ruta.rsplit('/').next().unwrap_or(ruta).to_string()
}

/// Recursive folder copy. Symlinks are SKIPPED: a skill bundle has no
/// business leaving the folder, and following one could escape it.
fn copiar_dir(de: &Path, a: &Path) -> std::io::Result<()> {
    fs::create_dir_all(a)?;
    for entrada in fs::read_dir(de)? {
        let entrada = entrada?;
        let tipo = entrada.file_type()?;
        if tipo.is_symlink() {
            continue;
        }
        let destino = a.join(entrada.file_name());
        if tipo.is_dir() {
            copiar_dir(&entrada.path(), &destino)?;
        } else {
            fs::copy(entrada.path(), destino)?;
        }
    }
    Ok(())
}

/// The map an install may work on, or WHY writing is refused (#17):
/// mirrors exclusiones — a file we cannot understand is never
/// overwritten by us.
pub fn mapa_escriturable(raiz: &Path) -> Result<Mapa, String> {
    match leer(raiz) {
        Lectura::Cargado { mapa } => Ok(mapa),
        Lectura::Inexistente => Ok(Mapa::new()),
        Lectura::Corrupto => Err(
            "el manifest de habilidades está dañado (conservado como .corrupt): resuélvelo antes de escribir"
                .to_string(),
        ),
        Lectura::Ilegible(e) => Err(format!("no se puede leer el manifest de habilidades: {e}")),
    }
}

/// Updates ONE managed Habilidad to the latest content of its Origen
/// (#29): download → validate the NEW content → activate by
/// copy-and-swap → record the new SHA. Managed-only: a skill without a
/// manifest entry refuses (there is nothing to update FROM). The local
/// folder is only replaced after the new content passes Validación.
pub fn actualizar(
    proveedor: &dyn ProveedorRemoto,
    nombre: &str,
    raiz_habilidades: &Path,
    mapa: &mut Mapa,
    ahora_epoch: i64,
) -> Result<(), String> {
    let entrada = mapa
        .get(nombre)
        .ok_or_else(|| format!("{nombre} no es gestionada: sin Origen que actualizar"))?
        .clone();
    let origen = RepoUrl {
        repo: entrada.origen.repo.clone(),
        referencia: None,
        ruta: Some(entrada.origen.ruta.clone()),
        slug: None,
    };
    fs::create_dir_all(raiz_habilidades).map_err(|e| e.to_string())?;
    let staging = tempfile::tempdir().map_err(|e| e.to_string())?;
    let raiz_repo = descargar_arbol(proveedor, &origen, staging.path())?;
    let resultado = instalar_una(
        proveedor,
        &origen,
        &entrada.origen.ruta,
        &Destino {
            nombre: nombre.to_string(),
            ahora_epoch,
        },
        &raiz_repo,
        raiz_habilidades,
        mapa,
    );
    match resultado {
        r if r.ok => Ok(()),
        r => Err(r
            .motivo
            .unwrap_or_else(|| "falló la actualización".to_string())),
    }
}

// ---- Actualizar todo (#30): the sequential queue over skills ----

/// A queue event for a table row: the skill's update starts or ends.
pub enum EventoColaHabilidades {
    Empieza {
        habilidad: String,
    },
    Resultado {
        habilidad: String,
        motivo: cola::Motivo,
        salida: String,
    },
}

/// "Actualizar todo" over the skills: list order, one at a time, a
/// failure does not stop the queue. The pending set is the Gestionadas
/// whose remote SHA differs (Actualización disponible) — derived FRESH
/// at start; an unreachable origin (SinVerificar) is skipped, never
/// guessed. One flag set SHARING the packages queue's one-active gate:
/// both queues can never run together, while Detener/abandonment stay
/// independent per queue.
///
/// Detener stops the queue immediately — the in-flight update FINISHES
/// naturally (HTTP is bounded by its timeouts, unlike a hung process)
/// and counts normally; nothing pending starts and the summary says
/// `detenida`. There are no `detenidos` mid-flight here: HTTP cannot be
/// cut the way a child process can.
pub fn actualizar_todo(
    proveedor: &dyn ProveedorRemoto,
    raiz: &Path,
    mapa: &mut Mapa,
    banderas: &cola::Banderas,
    ahora_epoch: i64,
    emitir: &mut dyn FnMut(&EventoColaHabilidades),
) -> Result<cola::Resumen, String> {
    let _guarda = banderas.entrar()?;
    banderas.reiniciar();
    // The queue is built on FRESH state: a corrupt manifest would make
    // the pending set a lie — refuse until resolved (#17).
    let salida = refrescar(raiz, proveedor);
    let pendientes: Vec<String> = salida
        .habilidades
        .iter()
        .filter(|h| h.estado == EstadoHabilidad::ActualizacionDisponible)
        .map(|h| h.nombre.clone())
        .collect();
    let total = pendientes.len();
    let (mut ok, mut failed) = (0usize, 0usize);
    let mut detenida = false;
    for habilidad in pendientes {
        if banderas.proximo_detenido() {
            detenida = true;
            break;
        }
        emitir(&EventoColaHabilidades::Empieza {
            habilidad: habilidad.clone(),
        });
        let (motivo, salida) = match actualizar(proveedor, &habilidad, raiz, mapa, ahora_epoch) {
            Ok(()) => {
                ok += 1;
                (cola::Motivo::Ok, String::new())
            }
            Err(e) => {
                failed += 1;
                (cola::Motivo::Fallo, e)
            }
        };
        emitir(&EventoColaHabilidades::Resultado {
            habilidad,
            motivo,
            salida,
        });
    }
    Ok(cola::Resumen {
        total,
        ok,
        failed,
        // a user decision happens BETWEEN skills here; nothing is cut
        // mid-write
        detenidos: 0,
        detenida,
    })
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
        fs::create_dir(dir.path().join("invalida")).unwrap();
        fs::write(
            dir.path().join("invalida").join(SKILL_MD),
            "sin frontmatter",
        )
        .unwrap();
        fs::create_dir(dir.path().join(".oculta")).unwrap();
        fs::write(dir.path().join("suelto.txt"), "no es carpeta").unwrap();

        let salida = derivar(dir.path(), None);
        assert_eq!(salida.manifest.estado, "ok");
        assert_eq!(
            salida
                .habilidades
                .iter()
                .map(|h| (h.nombre.as_str(), h.estado))
                .collect::<Vec<_>>(),
            [
                ("buena", EstadoHabilidad::NoGestionada),
                ("invalida", EstadoHabilidad::Invalida)
            ]
        );
    }

    #[test]
    fn carpeta_inexistente_es_un_arranco_vacio_valido() {
        let dir = tempfile::tempdir().unwrap();
        let salida = derivar(&dir.path().join("no-existe"), None);
        assert!(salida.habilidades.is_empty());
        assert_eq!(salida.manifest.estado, "ok");
    }

    #[test]
    fn manifest_corrupto_se_reporta_y_se_preserva_sin_ocultar_filas() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("buena")).unwrap();
        fs::write(dir.path().join("buena").join(SKILL_MD), SKILL_OK).unwrap();
        fs::write(dir.path().join(ARCHIVO), "roto").unwrap();
        let salida = derivar(dir.path(), None);
        assert_eq!(salida.manifest.estado, "corrupto");
        assert_eq!(salida.habilidades.len(), 1); // reality stays visible
        assert!(dir.path().join(format!("{ARCHIVO}.corrupt")).exists());
    }

    // ---- #27: parsear_origen ----

    fn repo_de(texto: &str) -> RepoUrl {
        parsear_origen(texto).unwrap()
    }

    #[test]
    fn formas_de_origen_aceptadas() {
        // bare owner/repo, with host, with scheme, trailing slash
        for texto in [
            "o/r",
            "github.com/o/r",
            "https://github.com/o/r/",
            "github.com/o/r",
        ] {
            assert_eq!(repo_de(texto).repo, "o/r", "{texto}");
            assert_eq!(repo_de(texto).referencia, None);
            assert_eq!(repo_de(texto).ruta, None);
        }
        // tree form: ref only, and ref + ruta
        let con_ref = repo_de("github.com/o/r/tree/v1.0");
        assert_eq!(con_ref.referencia.as_deref(), Some("v1.0"));
        assert_eq!(con_ref.ruta, None);
        let completo = repo_de("https://github.com/o/r/tree/main/skills/productivity/x");
        assert_eq!(completo.referencia.as_deref(), Some("main"));
        assert_eq!(completo.ruta.as_deref(), Some("skills/productivity/x"));
    }

    #[test]
    fn orígenes_rechazados() {
        for texto in [
            "gitlab.com/o/r",
            "github.com/",
            "github.com/solo",
            "github.com/o/r/extra/más",
            "github.com/o/.r",
            "github.com/o/r/tree/main/../escape",
            "",
        ] {
            assert!(parsear_origen(texto).is_err(), "{texto} no debió pasar");
        }
    }

    // ---- #27: escanear with a fake provider ----

    /// A fake provider: copies a fixture tree into the staging dir — the
    /// same contract as the real one, no network.
    struct FalsoProveedor {
        fixture: PathBuf,
        sha: String,
        falla_sha: bool,
    }

    impl FalsoProveedor {
        fn nuevo(fixture: &Path) -> Self {
            Self {
                fixture: fixture.to_path_buf(),
                sha: "abc123".to_string(),
                falla_sha: false,
            }
        }
    }

    impl ProveedorRemoto for FalsoProveedor {
        fn descargar_en(&self, _origen: &RepoUrl, destino: &Path) -> Result<(), String> {
            // the real provider leaves ONE top dir (tarball root); the
            // fake mimics that: fixture/repo/<skills…>
            copiar_dir(&self.fixture, destino).map_err(|e| e.to_string())
        }

        fn sha_de(&self, _repo: &str, _ruta: &str) -> Result<String, String> {
            if self.falla_sha {
                Err("api caída".to_string())
            } else {
                Ok(self.sha.clone())
            }
        }
    }

    /// A fake repo tree: one top dir (`repo/`) with skills nested at
    /// several depths, one broken, one hidden, one loose file.
    fn fixture_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let skill = |rel: &str, texto: &str| {
            let p = repo.join(rel);
            fs::create_dir_all(&p).unwrap();
            fs::write(p.join(SKILL_MD), texto).unwrap();
        };
        skill("skills/productivity/buena", SKILL_OK);
        skill("skills/engineering/otra", SKILL_OK_COMILLAS);
        skill("utils/anidada", SKILL_OK); // recursion proof
        skill("skills/productivity/invalida", "sin frontmatter");
        skill(".oculta/buena", SKILL_OK); // hidden: never a candidate
        fs::write(repo.join("README.md"), "texto suelto").unwrap();
        dir
    }

    #[test]
    fn escanear_encuentra_anidadas_valida_e_invalida_y_salta_ocultas() {
        let fixture = fixture_repo();
        let proveedor = FalsoProveedor::nuevo(fixture.path());
        let staging = tempfile::tempdir().unwrap();
        let rutas: Vec<(String, bool)> = escanear(&proveedor, &repo_de("o/r"), staging.path())
            .unwrap()
            .into_iter()
            .map(|h| (h.ruta, h.conforme))
            .collect();
        assert_eq!(
            rutas,
            vec![
                ("skills/engineering/otra".to_string(), true),
                ("skills/productivity/buena".to_string(), true),
                ("skills/productivity/invalida".to_string(), false),
                ("utils/anidada".to_string(), true),
            ]
        );
    }

    #[test]
    fn escanear_directa_reporta_solo_esa_y_marca_la_inexistente() {
        let fixture = fixture_repo();
        let proveedor = FalsoProveedor::nuevo(fixture.path());
        let staging = tempfile::tempdir().unwrap();
        let directa = escanear(
            &proveedor,
            &repo_de("github.com/o/r/tree/main/utils/anidada"),
            staging.path(),
        )
        .unwrap();
        assert_eq!(directa.len(), 1);
        assert!(directa[0].conforme);
        let ausente = escanear(
            &proveedor,
            &repo_de("github.com/o/r/tree/main/no/esta"),
            staging.path(),
        )
        .unwrap();
        assert_eq!(ausente.len(), 1);
        assert!(!ausente[0].conforme);
        assert!(ausente[0].motivo.as_deref().unwrap().contains("no existe"));
    }

    // ---- #27: instalar ----

    fn origen_simple() -> RepoUrl {
        repo_de("o/r")
    }

    #[test]
    fn instalar_activa_valida_y_registra_origen_y_sha() {
        let fixture = fixture_repo();
        let proveedor = FalsoProveedor::nuevo(fixture.path());
        let raiz = tempfile::tempdir().unwrap();
        let mut mapa = Mapa::new();
        let resultados = instalar(
            &proveedor,
            &origen_simple(),
            &[
                "skills/productivity/buena".to_string(),
                "utils/anidada".to_string(),
            ],
            raiz.path(),
            &mut mapa,
            1_800_000_000,
        )
        .unwrap();
        assert!(resultados.iter().all(|r| r.ok));
        // both skills live under their leaf names
        assert!(raiz.path().join("buena").join(SKILL_MD).is_file());
        assert!(raiz.path().join("anidada").join(SKILL_MD).is_file());
        // the manifest entries carry repo, ruta and the fetched SHA
        let entrada = &mapa["buena"];
        assert_eq!(entrada.origen.repo, "o/r");
        assert_eq!(entrada.origen.ruta, "skills/productivity/buena");
        assert_eq!(entrada.sha, "abc123");
        assert_eq!(entrada.instalada_en, "1800000000");
        assert_eq!(mapa["anidada"].origen.ruta, "utils/anidada");
    }

    #[test]
    fn instalar_invalida_no_se_activa_pero_las_demas_si() {
        let fixture = fixture_repo();
        let proveedor = FalsoProveedor::nuevo(fixture.path());
        let raiz = tempfile::tempdir().unwrap();
        let mut mapa = Mapa::new();
        let resultados = instalar(
            &proveedor,
            &origen_simple(),
            &[
                "skills/productivity/invalida".to_string(),
                "skills/productivity/buena".to_string(),
            ],
            raiz.path(),
            &mut mapa,
            1,
        )
        .unwrap();
        assert_eq!(resultados.len(), 2);
        assert!(!resultados[0].ok);
        assert!(resultados[0]
            .motivo
            .as_deref()
            .unwrap()
            .contains("inválida"));
        assert!(!raiz.path().join("invalida").exists()); // never activated
        assert!(resultados[1].ok); // a failure does not stop the rest
        assert!(mapa.contains_key("buena"));
        assert!(!mapa.contains_key("invalida"));
    }

    #[test]
    fn instalar_sin_sha_falla_la_ruta_y_no_escribe_entrada() {
        let fixture = fixture_repo();
        let proveedor = FalsoProveedor {
            falla_sha: true,
            ..FalsoProveedor::nuevo(fixture.path())
        };
        let raiz = tempfile::tempdir().unwrap();
        let mut mapa = Mapa::new();
        let resultados = instalar(
            &proveedor,
            &origen_simple(),
            &["skills/productivity/buena".to_string()],
            raiz.path(),
            &mut mapa,
            1,
        )
        .unwrap();
        assert!(!resultados[0].ok);
        assert!(resultados[0].motivo.as_deref().unwrap().contains("SHA"));
        assert!(mapa.is_empty()); // without a SHA there is nothing to track
    }

    #[test]
    fn instalar_reemplaza_la_carpeta_existente() {
        let fixture = fixture_repo();
        let proveedor = FalsoProveedor::nuevo(fixture.path());
        let raiz = tempfile::tempdir().unwrap();
        // a previous install (or another tool's copy) with junk in it
        let previa = raiz.path().join("buena");
        fs::create_dir_all(&previa).unwrap();
        fs::write(previa.join("basura.txt"), "vieja").unwrap();
        let mut mapa = Mapa::new();
        let resultados = instalar(
            &proveedor,
            &origen_simple(),
            &["skills/productivity/buena".to_string()],
            raiz.path(),
            &mut mapa,
            1,
        )
        .unwrap();
        assert!(resultados[0].ok);
        assert!(!previa.join("basura.txt").exists()); // replaced, not merged
        assert!(previa.join(SKILL_MD).is_file());
    }

    #[test]
    fn instalar_ruta_traversal_rechazada() {
        let fixture = fixture_repo();
        let proveedor = FalsoProveedor::nuevo(fixture.path());
        let raiz = tempfile::tempdir().unwrap();
        let mut mapa = Mapa::new();
        let resultados = instalar(
            &proveedor,
            &origen_simple(),
            &["../escape".to_string()],
            raiz.path(),
            &mut mapa,
            1,
        )
        .unwrap();
        assert!(!resultados[0].ok);
        assert!(resultados[0].motivo.as_deref().unwrap().contains("segura"));
    }

    // ---- #31: atajo de skills.sh ----

    #[test]
    fn url_de_skills_sh_se_parsea_a_repo_y_slug() {
        for texto in [
            "skills.sh/o/r/buena",
            "https://skills.sh/o/r/buena",
            "www.skills.sh/o/r/buena",
        ] {
            let origen = parsear_origen(texto).unwrap();
            assert_eq!(origen.repo, "o/r", "{texto}");
            assert_eq!(origen.slug.as_deref(), Some("buena"));
            assert_eq!(origen.ruta, None);
            assert_eq!(origen.referencia, None);
        }
        // wrong arity is refused
        for texto in ["skills.sh/o/r", "skills.sh/o", "skills.sh/o/r/buena/extra"] {
            assert!(parsear_origen(texto).is_err(), "{texto} no debió pasar");
        }
    }

    #[test]
    fn escanear_con_slug_filtra_por_carpeta_y_por_name_del_frente() {
        let fixture = fixture_repo();
        let proveedor = FalsoProveedor::nuevo(fixture.path());
        let staging = tempfile::tempdir().unwrap();
        // by folder leaf: "buena"
        let por_carpeta = escanear(
            &proveedor,
            &parsear_origen("skills.sh/o/r/buena").unwrap(),
            staging.path(),
        )
        .unwrap();
        assert_eq!(por_carpeta.len(), 1);
        assert_eq!(por_carpeta[0].ruta, "skills/productivity/buena");
        assert!(por_carpeta[0].conforme);
        // by frontmatter name: BOTH folders carrying name "find-skills"
        // match (the fixture reuses it); sorted by ruta
        let por_name = escanear(
            &proveedor,
            &parsear_origen("skills.sh/o/r/find-skills").unwrap(),
            staging.path(),
        )
        .unwrap();
        assert_eq!(
            por_name.iter().map(|h| h.ruta.as_str()).collect::<Vec<_>>(),
            ["skills/productivity/buena", "utils/anidada"]
        );
        // an unknown slug is an explicit error, never silence
        let err = escanear(
            &proveedor,
            &parsear_origen("skills.sh/o/r/no-existe").unwrap(),
            staging.path(),
        )
        .unwrap_err();
        assert!(err.contains("no se encontró"), "{err}");
        assert!(err.contains("no-existe"));
    }

    #[test]
    fn instalar_con_slug_usa_las_rutas_filtradas() {
        let fixture = fixture_repo();
        let proveedor = FalsoProveedor::nuevo(fixture.path());
        let staging = tempfile::tempdir().unwrap();
        let encontradas = escanear(
            &proveedor,
            &parsear_origen("skills.sh/o/r/buena").unwrap(),
            staging.path(),
        )
        .unwrap();
        let rutas: Vec<String> = encontradas.iter().map(|h| h.ruta.clone()).collect();
        let raiz = tempfile::tempdir().unwrap();
        let mut mapa = Mapa::new();
        let resultados = instalar(
            &proveedor,
            &parsear_origen("skills.sh/o/r/buena").unwrap(),
            &rutas,
            raiz.path(),
            &mut mapa,
            7,
        )
        .unwrap();
        assert!(resultados[0].ok);
        assert!(raiz.path().join("buena").join(SKILL_MD).is_file());
        assert_eq!(mapa["buena"].origen.repo, "o/r");
    }

    #[test]
    fn manifest_ilegible_o_corrupto_bloquea_la_escritura() {
        let raiz = tempfile::tempdir().unwrap();
        assert!(matches!(mapa_escriturable(raiz.path()), Ok(m) if m.is_empty()));
        fs::write(raiz.path().join(ARCHIVO), "roto").unwrap();
        let err = mapa_escriturable(raiz.path()).unwrap_err();
        assert!(err.contains("resuélvelo"), "{err}");
        // repaired by hand → writable again
        fs::write(raiz.path().join(ARCHIVO), "{}").unwrap();
        assert!(mapa_escriturable(raiz.path()).is_ok());
    }

    // ---- #28: refrescar — the SHA decides ----

    /// Fake provider that COUNTS its SHA checks: no gestionada and
    /// invalida must never reach the network.
    struct ContadorProveedor {
        fixture: PathBuf,
        shas: std::cell::Cell<u32>,
        // what the remote answers for EVERY ruta
        remoto: String,
        falla: bool,
    }

    impl ContadorProveedor {
        fn nuevo(fixture: &Path, remoto: &str) -> Self {
            Self {
                fixture: fixture.to_path_buf(),
                shas: std::cell::Cell::new(0),
                remoto: remoto.to_string(),
                falla: false,
            }
        }
    }

    impl ProveedorRemoto for ContadorProveedor {
        fn descargar_en(&self, _origen: &RepoUrl, destino: &Path) -> Result<(), String> {
            copiar_dir(&self.fixture, destino).map_err(|e| e.to_string())
        }

        fn sha_de(&self, _repo: &str, _ruta: &str) -> Result<String, String> {
            self.shas.set(self.shas.get() + 1);
            if self.falla {
                Err("sin red".to_string())
            } else {
                Ok(self.remoto.clone())
            }
        }
    }

    /// A skills folder with ONE managed+conforme skill, one unmanaged and
    /// one invalida; the manifest knows buena's Origen and saved SHA.
    fn carpeta_con_gestionadas(sha_guardado: &str) -> (tempfile::TempDir, tempfile::TempDir) {
        let raiz = tempfile::tempdir().unwrap();
        for nombre in ["buena", "suelta"] {
            fs::create_dir_all(raiz.path().join(nombre)).unwrap();
            fs::write(raiz.path().join(nombre).join(SKILL_MD), SKILL_OK).unwrap();
        }
        fs::create_dir_all(raiz.path().join("invalida")).unwrap();
        fs::write(
            raiz.path().join("invalida").join(SKILL_MD),
            "sin frontmatter",
        )
        .unwrap();
        let mut mapa = Mapa::new();
        mapa.insert(
            "buena".to_string(),
            entrada("o/r", "skills/productivity/buena", sha_guardado),
        );
        guardar(raiz.path(), &mapa).unwrap();
        (raiz, fixture_repo())
    }

    #[test]
    fn sha_igual_es_actual_sha_distinto_es_actualizacion_disponible() {
        // same SHA → Actual
        let (raiz, fixture) = carpeta_con_gestionadas("abc123");
        let proveedor = ContadorProveedor::nuevo(fixture.path(), "abc123");
        let salida = refrescar(raiz.path(), &proveedor);
        let estados: std::collections::BTreeMap<_, _> = salida
            .habilidades
            .iter()
            .map(|h| (h.nombre.as_str(), h.estado))
            .collect();
        assert_eq!(estados["buena"], EstadoHabilidad::Actual);
        assert_eq!(estados["suelta"], EstadoHabilidad::NoGestionada);
        assert_eq!(estados["invalida"], EstadoHabilidad::Invalida);
        // 2 SHA checks total: the invalida and the unmanaged NEVER hit
        // the network
        assert_eq!(proveedor.shas.get(), 1);

        // different SHA → Actualización disponible
        let (raiz, fixture) = carpeta_con_gestionadas("viejo");
        let proveedor = ContadorProveedor::nuevo(fixture.path(), "nuevo");
        let salida = refrescar(raiz.path(), &proveedor);
        let fila = salida
            .habilidades
            .iter()
            .find(|h| h.nombre == "buena")
            .unwrap();
        assert_eq!(fila.estado, EstadoHabilidad::ActualizacionDisponible);
        assert!(fila.error.is_none());
    }

    #[test]
    fn fallo_de_red_marca_solo_esa_fila_como_sin_verificar() {
        let (raiz, fixture) = carpeta_con_gestionadas("abc123");
        // a SECOND managed skill so one failure cannot hide the other's
        // verdict; both entries live in ONE manifest
        fs::create_dir_all(raiz.path().join("segunda")).unwrap();
        fs::write(raiz.path().join("segunda").join(SKILL_MD), SKILL_OK).unwrap();
        let mut mapa = Mapa::new();
        mapa.insert(
            "buena".to_string(),
            entrada("o/r", "skills/productivity/buena", "abc123"),
        );
        mapa.insert(
            "segunda".to_string(),
            entrada("o/r", "utils/anidada", "def456"),
        );
        guardar(raiz.path(), &mapa).unwrap();

        // the fake fails only for buena's ruta
        struct FallaUna;
        impl ProveedorRemoto for FallaUna {
            fn descargar_en(&self, _o: &RepoUrl, _d: &Path) -> Result<(), String> {
                unreachable!("el refresco no descarga");
            }
            fn sha_de(&self, _repo: &str, ruta: &str) -> Result<String, String> {
                if ruta.ends_with("buena") {
                    Err("sin red".to_string())
                } else {
                    Ok("def456".to_string())
                }
            }
        }
        let _ = fixture; // unused here
        let proveedor = FallaUna;
        let salida = refrescar(raiz.path(), &proveedor);
        let buena = salida
            .habilidades
            .iter()
            .find(|h| h.nombre == "buena")
            .unwrap();
        assert_eq!(buena.estado, EstadoHabilidad::SinVerificar);
        assert!(buena.error.as_deref().unwrap().contains("sin red"));
        // the rest still got their verdicts
        let segunda = salida
            .habilidades
            .iter()
            .find(|h| h.nombre == "segunda")
            .unwrap();
        assert_eq!(segunda.estado, EstadoHabilidad::Actual);
        assert!(segunda.error.is_none());
    }

    // ---- #29: actualizar una gestionada ----

    fn mapa_de(raiz: &Path) -> Mapa {
        match leer(raiz) {
            Lectura::Cargado { mapa } => mapa,
            _ => Mapa::new(),
        }
    }

    #[test]
    fn actualizar_reemplaza_con_el_contenido_nuevo_y_registra_el_sha() {
        // saved SHA "viejo"; the fixture holds the NEW content and the
        // remote answers "abc123": after the update both agree
        let (raiz, fixture) = carpeta_con_gestionadas("viejo");
        fs::write(
            raiz.path().join("buena").join(SKILL_MD),
            "---\nname: buena\ndescription: contenido viejo\n---\n",
        )
        .unwrap();
        let proveedor = ContadorProveedor::nuevo(fixture.path(), "abc123");
        let mut mapa = mapa_de(raiz.path());
        actualizar(&proveedor, "buena", raiz.path(), &mut mapa, 9).unwrap();
        // the command wrapper persists ONCE on success — mirror it here
        guardar(raiz.path(), &mapa).unwrap();
        // the local folder now holds the NEW content (validated, swapped)
        let texto = fs::read_to_string(raiz.path().join("buena").join(SKILL_MD)).unwrap();
        assert!(texto.contains("Help discovering skills"));
        // the manifest records the NEW sha and the same Origen
        assert_eq!(mapa["buena"].sha, "abc123");
        assert_eq!(mapa["buena"].origen.repo, "o/r");
        // and a refresh now reports Actual
        let salida = refrescar(raiz.path(), &proveedor);
        let fila = salida
            .habilidades
            .iter()
            .find(|h| h.nombre == "buena")
            .unwrap();
        assert_eq!(fila.estado, EstadoHabilidad::Actual);
    }

    #[test]
    fn actualizar_con_contenido_nuevo_invalido_falla_y_no_toca_nada() {
        // the remote's content for the ruta is NOT conforme
        let (raiz, fixture) = carpeta_con_gestionadas("viejo");
        let proveedor = ContadorProveedor::nuevo(fixture.path(), "abc123");
        // point buena at the fixture's invalida folder
        mapa_de(raiz.path()); // shape check only
        let mut mapa = mapa_de(raiz.path());
        mapa.get_mut("buena").unwrap().origen.ruta = "skills/productivity/invalida".to_string();
        let contenido_antes = fs::read_to_string(raiz.path().join("buena").join(SKILL_MD)).unwrap();
        let err = actualizar(&proveedor, "buena", raiz.path(), &mut mapa, 9).unwrap_err();
        assert!(err.contains("inválida"), "{err}");
        // the local folder is INTACT and the saved entry unchanged
        assert_eq!(
            fs::read_to_string(raiz.path().join("buena").join(SKILL_MD)).unwrap(),
            contenido_antes
        );
        assert_eq!(mapa["buena"].sha, "viejo");
    }

    #[test]
    fn actualizar_no_gestionada_rechaza() {
        let (raiz, fixture) = carpeta_con_gestionadas("abc123");
        let proveedor = ContadorProveedor::nuevo(fixture.path(), "abc123");
        let mut mapa = Mapa::new();
        let err = actualizar(&proveedor, "suelta", raiz.path(), &mut mapa, 9).unwrap_err();
        assert!(err.contains("no es gestionada"), "{err}");
    }

    #[test]
    fn actualizar_con_red_caida_falla_visible_sin_cambios_locales() {
        let (raiz, fixture) = carpeta_con_gestionadas("viejo");
        let mut proveedor = ContadorProveedor::nuevo(fixture.path(), "abc123");
        proveedor.falla = true;
        // downloading fails before anything is touched
        struct SinRed;
        impl ProveedorRemoto for SinRed {
            fn descargar_en(&self, _o: &RepoUrl, _d: &Path) -> Result<(), String> {
                Err("sin red".to_string())
            }
            fn sha_de(&self, _r: &str, _p: &str) -> Result<String, String> {
                unreachable!()
            }
        }
        let mut mapa = mapa_de(raiz.path());
        let contenido_antes = fs::read_to_string(raiz.path().join("buena").join(SKILL_MD)).unwrap();
        let err = actualizar(&SinRed, "buena", raiz.path(), &mut mapa, 9).unwrap_err();
        assert!(err.contains("sin red"), "{err}");
        assert_eq!(
            fs::read_to_string(raiz.path().join("buena").join(SKILL_MD)).unwrap(),
            contenido_antes
        );
        assert_eq!(mapa["buena"].sha, "viejo");
        let _ = proveedor;
    }

    // ---- #30: la cola de habilidades ----

    /// Skills folder with TWO managed skills whose saved SHA is stale
    /// (both Actualización disponible against the fixture), plus one
    /// already up to date and one unmanaged: the queue must touch only
    /// the two stale ones.
    fn carpeta_para_cola() -> (tempfile::TempDir, tempfile::TempDir, Mapa) {
        let (raiz, fixture) = carpeta_con_gestionadas("viejo");
        fs::create_dir_all(raiz.path().join("segunda")).unwrap();
        fs::write(raiz.path().join("segunda").join(SKILL_MD), SKILL_OK).unwrap();
        let mut mapa = mapa_de(raiz.path());
        mapa.insert(
            "segunda".to_string(),
            entrada("o/r", "utils/anidada", "viejo"),
        );
        guardar(raiz.path(), &mapa).unwrap();
        (raiz, fixture, mapa)
    }

    #[test]
    fn cola_actualiza_solo_las_actualizables_en_orden_y_resume() {
        let (raiz, fixture, mut mapa) = carpeta_para_cola();
        let proveedor = ContadorProveedor::nuevo(fixture.path(), "abc123");
        let banderas = cola::Banderas::nuevas();
        let eventos: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let resumen = actualizar_todo(
            &proveedor,
            raiz.path(),
            &mut mapa,
            &banderas,
            5,
            &mut |ev| match ev {
                EventoColaHabilidades::Empieza { habilidad } => {
                    eventos.lock().unwrap().push(format!("empieza:{habilidad}"))
                }
                EventoColaHabilidades::Resultado {
                    habilidad, motivo, ..
                } => eventos
                    .lock()
                    .unwrap()
                    .push(format!("resultado:{habilidad}:{motivo:?}")),
            },
        )
        .unwrap();
        assert_eq!(resumen.total, 2);
        assert_eq!(resumen.ok, 2);
        assert_eq!(resumen.failed, 0);
        assert!(!resumen.detenida);
        // list order: buena before segunda; both now hold the new SHA
        let esperados = [
            "empieza:buena".to_string(),
            "resultado:buena:Ok".to_string(),
            "empieza:segunda".to_string(),
            "resultado:segunda:Ok".to_string(),
        ];
        assert_eq!(*eventos.lock().unwrap(), esperados);
        assert_eq!(mapa["buena"].sha, "abc123");
        assert_eq!(mapa["segunda"].sha, "abc123");
        assert_eq!(mapa["buena"].instalada_en, "5");
    }

    #[test]
    fn cola_un_fallo_no_corta_y_el_resumen_cuenta_aparte() {
        let (raiz, fixture, mut mapa) = carpeta_para_cola();
        // buena's ruta points at the fixture's INVALIDA folder: its
        // update fails; segunda's still succeeds
        mapa.get_mut("buena").unwrap().origen.ruta = "skills/productivity/invalida".to_string();
        let proveedor = ContadorProveedor::nuevo(fixture.path(), "abc123");
        let banderas = cola::Banderas::nuevas();
        let resumen = actualizar_todo(
            &proveedor,
            raiz.path(),
            &mut mapa,
            &banderas,
            5,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(resumen.total, 2);
        assert_eq!(resumen.ok, 1);
        assert_eq!(resumen.failed, 1);
        assert!(!resumen.detenida);
    }

    #[test]
    fn cola_detenida_antes_de_empezar_la_siguiente() {
        let (raiz, fixture, mut mapa) = carpeta_para_cola();
        let proveedor = ContadorProveedor::nuevo(fixture.path(), "abc123");
        let banderas = cola::Banderas::nuevas();
        let vistos = std::sync::Mutex::new(0);
        let resumen = actualizar_todo(
            &proveedor,
            raiz.path(),
            &mut mapa,
            &banderas,
            5,
            &mut |ev| {
                if matches!(ev, EventoColaHabilidades::Empieza { habilidad } if habilidad == "buena")
                {
                    // the user hits Detener while the FIRST one runs
                    banderas.detener();
                }
                *vistos.lock().unwrap() += 1;
            },
        )
        .unwrap();
        // the in-flight one finished naturally; the next never started
        assert_eq!(resumen.total, 2);
        assert_eq!(resumen.ok, 1);
        assert!(resumen.detenida);
        assert_eq!(resumen.detenidos, 0);
        assert_eq!(*vistos.lock().unwrap(), 2); // empieza+resultado de buena
                                                // the command persists ONCE on finish — mirror it before the
                                                // next queue reads disk
        guardar(raiz.path(), &mapa).unwrap();
        // and a finished queue can start again
        let segunda_vez = actualizar_todo(
            &proveedor,
            raiz.path(),
            &mut mapa,
            &banderas,
            6,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(segunda_vez.total, 1); // solo queda segunda
    }

    #[test]
    fn cola_comparte_la_guarda_con_la_de_paquetes_en_ambas_direcciones() {
        let (raiz, fixture, mut mapa) = carpeta_para_cola();
        let proveedor = ContadorProveedor::nuevo(fixture.path(), "abc123");
        let de_paquetes = cola::Banderas::nuevas();
        let de_habilidades = cola::Banderas::con_guarda_compartida(de_paquetes.activa());
        // paquetes holding the gate → skills refused
        let _guarda = de_paquetes.entrar().unwrap();
        let err = actualizar_todo(
            &proveedor,
            raiz.path(),
            &mut mapa,
            &de_habilidades,
            5,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("solo una"), "{err}");
        drop(_guarda);
        // and the reverse: skills holding it → paquetes refused
        let guarda_skills = de_habilidades.entrar().unwrap();
        assert!(de_paquetes.entrar().is_err());
        drop(guarda_skills);
        // released → both can start again (one at a time)
        assert!(de_paquetes.entrar().is_ok());
    }
}
