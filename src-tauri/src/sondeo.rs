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

use crate::kernel::{con_extension, primer_existente};
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
