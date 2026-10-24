//! Capa de sondeo del Descubrimiento (#43): el PATH que el usuario
//! configura en su shell de login — el que una app GUI no hereda.
//!
//! Tercera capa, después del PATH del entorno y de las ubicaciones
//! habituales: se consulta UNA vez por sesión (al arranque, cuando se
//! deciden las pestañas) y su resultado lo consumen listado,
//! actualización y cola a través del binario cacheado. Corre bajo el
//! Plazo de consultas: si el shell falla o vence, la capa se omite y
//! el arranque sigue. Windows lee el registro (#45, aún no): hoy sin
//! `$SHELL` la capa simplemente se omite.

use crate::kernel::{con_extension, correr_consulta, primer_existente};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// La costura nueva del Descubrimiento (#43): ejecuta el shell de
/// login del usuario ($SHELL, no interactivo) y reporta el PATH que
/// ese shell configura — el que una app GUI no hereda. Los tests
/// inyectan falsos de shell; el real es [`SondadorShell`].
pub(crate) trait Sondador {
    fn path_del_shell(&self) -> std::io::Result<Vec<PathBuf>>;
}

/// El sondeo de UNA sesión (#43): el PATH del shell se consulta una
/// sola vez y el binario resuelto por gestor queda cacheado — listado,
/// actualización y cola consumen lo cacheado, nunca re-sondean.
#[derive(Default)]
pub(crate) struct SesionSondeo {
    path_shell: Option<Vec<PathBuf>>,
    binarios: BTreeMap<String, Option<PathBuf>>,
}

/// ¿El gestor `nombre` se sondea en el sistema `so`? brew no existe
/// en Windows (#43); los gestores de node van en todas las plataformas
/// (el registro de Windows llega con #45: hoy, sin `$SHELL`, su capa
/// simplemente se omite).
pub(crate) fn sondeo_aplica(nombre: &str, so: &str) -> bool {
    !(nombre == "brew" && so == "windows")
}

/// Las rutas que la capa de sondeo miró para `nombre` — el PATH del
/// shell de login, expandido a candidatos del gestor: alimentan el
/// mensaje de «no encontrado» junto a las capas anteriores.
pub(crate) fn buscadas_sondeo(sesion: &SesionSondeo, nombre: &str) -> Vec<PathBuf> {
    sesion
        .path_shell
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|dir| dir.join(con_extension(nombre)))
        .collect()
}

/// El sondador real: ejecuta el shell de login del usuario (`$SHELL
/// -l -c env`, no interactivo) y lee el PATH que ese shell configura.
/// Corre bajo el Plazo de consultas: un shell colgado vence y la capa
/// se omite, nunca congela el arranque.
pub(crate) struct SondadorShell;

impl Sondador for SondadorShell {
    fn path_del_shell(&self) -> std::io::Result<Vec<PathBuf>> {
        let shell = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|sh| !sh.as_os_str().is_empty())
            .ok_or_else(|| {
                std::io::Error::other("sin shell de login ($SHELL): capa de sondeo omitida")
            })?;
        let mut cmd = std::process::Command::new(&shell);
        // -l: shell de login, lee el perfil donde vive el PATH del
        // usuario; sin -i: no interactivo.
        cmd.args(["-l", "-c", "env"]);
        let out = correr_consulta(cmd)?;
        if out.exit_code != 0 {
            return Err(std::io::Error::other(format!(
                "el shell de login ({}) terminó con {}: capa de sondeo omitida",
                shell.display(),
                out.exit_code
            )));
        }
        Ok(path_del_env(&out.stdout))
    }
}

/// La sesión de sondeo de esta ejecución de la app: se llena al
/// arranque (las pestañas) y la consumen listado, actualización y
/// cola — el binario queda cacheado, no se re-sondea (#43).
static SESION: std::sync::LazyLock<std::sync::Mutex<SesionSondeo>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(SesionSondeo::default()));

/// La capa de sondeo del Descubrimiento para el gestor `nombre`, sobre
/// la sesión global. Fallo, vencimiento del Plazo o brew-en-Windows la
/// omiten: `None`, jamás un error de arranque.
pub(crate) fn capa(nombre: &str) -> Option<PathBuf> {
    if !sondeo_aplica(nombre, std::env::consts::OS) {
        return None;
    }
    SESION
        .lock()
        .ok()
        .and_then(|mut sesion| capa_sondeo(nombre, &SondadorShell, &mut sesion))
}

/// Las rutas que la capa de sondeo miró para `nombre` en esta sesión:
/// para el mensaje de «no encontrado».
pub(crate) fn buscadas(nombre: &str) -> Vec<PathBuf> {
    SESION
        .lock()
        .ok()
        .map(|sesion| buscadas_sondeo(&sesion, nombre))
        .unwrap_or_default()
}

/// La capa de sondeo como función pura: cache primero, shell UNA vez
/// por sesión (su fallo también queda cacheado), y el binario es el
/// primer hallazgo en el PATH del shell.
pub(crate) fn capa_sondeo(
    nombre: &str,
    sondador: &dyn Sondador,
    sesion: &mut SesionSondeo,
) -> Option<PathBuf> {
    if let Some(bin) = sesion.binarios.get(nombre) {
        return bin.clone();
    }
    if sesion.path_shell.is_none() {
        // El fallo (o vencimiento del Plazo) también queda cacheado:
        // la capa se omite por el resto de la sesión, no se reintenta.
        sesion.path_shell = Some(sondador.path_del_shell().unwrap_or_default());
    }
    let candidatos: Vec<PathBuf> = sesion
        .path_shell
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|dir| dir.join(con_extension(nombre)))
        .collect();
    let bin = primer_existente(candidatos);
    sesion.binarios.insert(nombre.to_string(), bin.clone());
    bin
}

/// Lee el PATH de la salida de `env` tal como la imprime el shell de
/// login: la línea `PATH=...`, separada por `:` (todos los shells la
/// exportan unida por dos puntos, fish incluido). El ruido que el
/// perfil escriba en stdout se ignora: solo la línea `PATH=` manda.
pub(crate) fn path_del_env(salida: &str) -> Vec<PathBuf> {
    salida
        .lines()
        .find_map(|linea| linea.strip_prefix("PATH="))
        .map(|valor| {
            std::env::split_paths(valor)
                .filter(|dir| !dir.as_os_str().is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Falso de shell (#43): responde un PATH fijo contando cuántas
    /// veces se lo consultó — el contrato de «una vez por sesión» se
    /// prueba aquí, sin ejecutar shells de verdad.
    struct FalsoShell {
        respuesta: RespuestaShell,
        consultas: std::cell::RefCell<usize>,
    }

    enum RespuestaShell {
        Path(Vec<PathBuf>),
        Fallo,
    }

    impl FalsoShell {
        fn con_path(dirs: Vec<PathBuf>) -> Self {
            Self {
                respuesta: RespuestaShell::Path(dirs),
                consultas: std::cell::RefCell::new(0),
            }
        }

        fn que_falla() -> Self {
            Self {
                respuesta: RespuestaShell::Fallo,
                consultas: std::cell::RefCell::new(0),
            }
        }
    }

    impl Sondador for FalsoShell {
        fn path_del_shell(&self) -> std::io::Result<Vec<PathBuf>> {
            *self.consultas.borrow_mut() += 1;
            match &self.respuesta {
                RespuestaShell::Path(dirs) => Ok(dirs.clone()),
                RespuestaShell::Fallo => Err(std::io::Error::other("shell de login colgado")),
            }
        }
    }

    fn dir_con_bin(dir: &std::path::Path, nombre: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(con_extension(nombre)), "#!/bin/sh\n").unwrap();
    }

    #[test]
    fn resuelve_el_binario_en_el_path_del_shell() {
        let dir = tempfile::tempdir().unwrap();
        let sin_gestor = dir.path().join("a");
        let con_gestor = dir.path().join("b");
        std::fs::create_dir_all(&sin_gestor).unwrap();
        dir_con_bin(&con_gestor, "pnpm");
        let falso = FalsoShell::con_path(vec![sin_gestor, con_gestor.clone()]);
        let mut sesion = SesionSondeo::default();
        assert_eq!(
            capa_sondeo("pnpm", &falso, &mut sesion),
            Some(con_gestor.join(con_extension("pnpm")))
        );
    }

    #[test]
    fn el_shell_se_consulta_una_vez_por_sesion() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("volta");
        dir_con_bin(&bin_dir, "pnpm");
        dir_con_bin(&bin_dir, "bun");
        let falso = FalsoShell::con_path(vec![bin_dir]);
        let mut sesion = SesionSondeo::default();
        assert!(capa_sondeo("pnpm", &falso, &mut sesion).is_some());
        assert!(capa_sondeo("bun", &falso, &mut sesion).is_some());
        assert_eq!(*falso.consultas.borrow(), 1, "un solo shell por sesión");
    }

    #[test]
    fn el_binario_resuelto_queda_cacheado_por_gestor() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("volta");
        dir_con_bin(&bin_dir, "pnpm");
        let falso = FalsoShell::con_path(vec![bin_dir]);
        let mut sesion = SesionSondeo::default();
        let primero = capa_sondeo("pnpm", &falso, &mut sesion);
        let segundo = capa_sondeo("pnpm", &falso, &mut sesion);
        assert_eq!(primero, segundo);
        assert_eq!(*falso.consultas.borrow(), 1);
    }

    #[test]
    fn el_fallo_del_shell_queda_cacheado_como_omision() {
        let falso = FalsoShell::que_falla();
        let mut sesion = SesionSondeo::default();
        assert!(capa_sondeo("pnpm", &falso, &mut sesion).is_none());
        assert!(capa_sondeo("pnpm", &falso, &mut sesion).is_none());
        assert_eq!(
            *falso.consultas.borrow(),
            1,
            "el fallo no se reintenta: la capa queda omitida"
        );
    }

    #[test]
    fn brew_no_se_sondea_en_windows() {
        assert!(!sondeo_aplica("brew", "windows"));
        assert!(sondeo_aplica("brew", "macos"));
        assert!(sondeo_aplica("brew", "linux"));
        assert!(sondeo_aplica("npm", "windows"));
    }

    #[test]
    fn el_no_encontrado_incluye_las_rutas_del_sondeo() {
        let dir = tempfile::tempdir().unwrap();
        // El shell reporta un PATH donde el gestor NO está: la capa
        // miró esas rutas y el mensaje de «no encontrado» las lista.
        let mirada = dir.path().join("volta");
        std::fs::create_dir_all(&mirada).unwrap();
        let falso = FalsoShell::con_path(vec![mirada.clone()]);
        let mut sesion = SesionSondeo::default();
        assert!(capa_sondeo("pnpm", &falso, &mut sesion).is_none());
        let mut buscadas = vec![PathBuf::from("/opt/pnpm")];
        buscadas.extend(buscadas_sondeo(&sesion, "pnpm"));
        let msg = crate::kernel::no_encontrado("pnpm", &buscadas).to_string();
        assert!(
            msg.contains(&mirada.join(con_extension("pnpm")).display().to_string()),
            "{msg}"
        );
    }

    #[test]
    fn buscadas_del_sondeo_vacias_si_el_shell_fallo() {
        let falso = FalsoShell::que_falla();
        let mut sesion = SesionSondeo::default();
        let _ = capa_sondeo("pnpm", &falso, &mut sesion);
        assert!(buscadas_sondeo(&sesion, "pnpm").is_empty());
    }

    /// Smoke manual en la máquina REAL (`cargo test -- --ignored`):
    /// el shell de login reporta un PATH usable. La ejecución normal
    /// no lo corre: sondea shells de verdad.
    #[test]
    #[ignore]
    #[cfg(not(windows))]
    fn el_shell_de_login_reporta_su_path() {
        let dirs = SondadorShell.path_del_shell().expect("sondeo del shell");
        assert!(!dirs.is_empty(), "el PATH del shell de login llegó vacío");
    }

    #[test]
    fn path_del_env_toma_la_linea_path_en_orden_y_filtra_vacias() {
        let salida = "HOME=/Users/ejemplo\nPATH=/Users/ejemplo/.volta/bin::/usr/local/bin:/usr/bin\nSHELL=/bin/zsh\n";
        assert_eq!(
            path_del_env(salida),
            vec![
                PathBuf::from("/Users/ejemplo/.volta/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/bin"),
            ]
        );
    }

    #[test]
    fn path_del_env_sin_linea_path_es_vacio() {
        assert!(path_del_env("HOME=/Users/ejemplo\nSHELL=/bin/zsh").is_empty());
    }
}
