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

/// La costura nueva del Descubrimiento (#43): reporta el PATH que el
/// usuario configura donde su sistema lo define y que una app GUI no
/// hereda — el shell de login ($SHELL, no interactivo) en macOS/Linux;
/// el registro de Windows llega en #45 con [`SondadorWindows`]. Los
/// tests inyectan falsos de shell; el real es [`SondadorShell`].
pub(crate) trait Sondador {
    fn path_configurado(&self) -> std::io::Result<Vec<PathBuf>>;
}

/// El sondeo de UNA sesión (#43): el PATH del shell se consulta una
/// sola vez y el binario resuelto por gestor queda cacheado — listado,
/// actualización y cola consumen lo cacheado, nunca re-sondean.
#[derive(Default)]
pub(crate) struct SesionSondeo {
    path_configurado: Option<Vec<PathBuf>>,
    binarios: BTreeMap<String, Option<PathBuf>>,
}

/// ¿El gestor `nombre` se sondea en el sistema `so`? brew no existe
/// en Windows (#43); los gestores de node van en todas las plataformas
/// (el registro de Windows llega con #45: hoy, sin `$SHELL`, su capa
/// simplemente se omite).
pub(crate) fn sondeo_aplica(nombre: &str, so: &str) -> bool {
    !(nombre == "brew" && so == "windows")
}

/// Las rutas que la capa de sondeo miró para `nombre` — el PATH
/// configurado por el usuario, expandido a candidatos del gestor:
/// alimentan el mensaje de «no encontrado» junto a las capas
/// anteriores.
pub(crate) fn buscadas_sondeo(sesion: &SesionSondeo, nombre: &str) -> Vec<PathBuf> {
    sesion
        .path_configurado
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
    fn path_configurado(&self) -> std::io::Result<Vec<PathBuf>> {
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

/// La costura del registro de Windows (#45): el PATH crudo —con
/// `%REFERENCIAS%` sin expandir— que el usuario configura para sí
/// (usuario) y para toda la máquina (máquina). El real es
/// [`RegistroReg`] (`reg query`, bajo el Plazo de consultas); los
/// tests inyectan falsos de registro.
pub(crate) trait Registro {
    fn path_usuario(&self) -> std::io::Result<String>;
    fn path_maquina(&self) -> std::io::Result<String>;
}

/// El sondador de Windows (#45): donde el usuario configura su PATH
/// es el registro — se suman el de usuario y el de máquina al del
/// entorno ([`paths_de_registro`]). Una rama sin valor `Path` no tira
/// la capa: aporta lo que haya, como el shell que falla deja la capa
/// en lo que ya existe. Solo Windows lo ejerce desde [`capa`]; los
/// tests lo ejercen en todas las plataformas con falsos de registro.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct SondadorWindows<'a> {
    registro: &'a dyn Registro,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl<'a> SondadorWindows<'a> {
    pub(crate) fn new(registro: &'a dyn Registro) -> Self {
        Self { registro }
    }
}

impl Sondador for SondadorWindows<'_> {
    fn path_configurado(&self) -> std::io::Result<Vec<PathBuf>> {
        let usuario = self.registro.path_usuario().unwrap_or_default();
        let maquina = self.registro.path_maquina().unwrap_or_default();
        let entorno = std::env::var_os("PATH")
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        Ok(paths_de_registro(&entorno, &usuario, &maquina, &|nombre| {
            std::env::var(nombre).ok()
        }))
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
    if sesion.path_configurado.is_none() {
        // El fallo (o vencimiento del Plazo) también queda cacheado:
        // la capa se omite por el resto de la sesión, no se reintenta.
        sesion.path_configurado = Some(sondador.path_configurado().unwrap_or_default());
    }
    let candidatos: Vec<PathBuf> = sesion
        .path_configurado
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

/// Lee el valor `Path` de la salida de `reg query ... /v Path` tal
/// como la imprime reg.exe: la fila sangrada `    Path    TIPO
/// VALOR`, donde el tipo (`REG_SZ`, `REG_EXPAND_SZ`, …) precede al
/// valor separado por espacios y el valor puede contenerlos. Sin fila
/// de valor (la rama existe pero el valor no) no hay nada que leer.
/// La referencia cruda, sin expandir: eso lo hace
/// [`expandir_referencias`].
pub(crate) fn path_del_reg(salida: &str) -> Option<&str> {
    salida.lines().find_map(|linea| {
        let linea = linea.trim();
        let resto = linea
            .strip_prefix("Path")
            .filter(|resto| resto.starts_with(char::is_whitespace))?
            .trim_start();
        let (tipo, valor) = resto.split_once(char::is_whitespace)?;
        if !tipo.starts_with("REG_") {
            return None;
        }
        let valor = valor.trim();
        (!valor.is_empty()).then_some(valor)
    })
}

/// Expande las `%REFERENCIAS%` de un valor del registro (los PATH de
/// usuario y de máquina suelen ser `REG_EXPAND_SZ`): cada referencia
/// con valor se reemplaza, las que no tienen valor quedan literales —
/// el comportamiento de Windows — y un `%` suelto no se toca.
pub(crate) fn expandir_referencias(valor: &str, vars: &dyn Fn(&str) -> Option<String>) -> String {
    let mut resultado = String::with_capacity(valor.len());
    let mut resto = valor;
    while let Some(inicio) = resto.find('%') {
        resultado.push_str(&resto[..inicio]);
        resto = &resto[inicio + 1..];
        match resto.find('%') {
            Some(fin) => {
                let nombre = &resto[..fin];
                match vars(nombre) {
                    Some(valor_de_variable) => resultado.push_str(&valor_de_variable),
                    // sin valor no hay nada que sustituir: queda literal
                    None => resultado.push_str(&format!("%{nombre}%")),
                }
                resto = &resto[fin + 1..];
            }
            None => {
                resultado.push('%');
                break;
            }
        }
    }
    resultado.push_str(resto);
    resultado
}

/// El PATH configurado en Windows (#45), como lógica pura: el del
/// entorno más el de máquina y el de usuario del registro, separados
/// por `;`, vacíos fuera y sin repetidos. El orden es el del ticket:
/// primero el entorno (la combinación que el inicio de sesión ya
/// hizo), luego el registro en el orden en que Windows lo construye —
/// máquina antes que usuario. Las `%REFERENCIAS%` de los valores del
/// registro se expanden con `vars`; el entorno ya llega expandido.
pub(crate) fn paths_de_registro(
    entorno: &str,
    usuario: &str,
    maquina: &str,
    vars: &dyn Fn(&str) -> Option<String>,
) -> Vec<PathBuf> {
    let usuario = expandir_referencias(usuario, vars);
    let maquina = expandir_referencias(maquina, vars);
    let mut vistas: std::collections::BTreeSet<std::ffi::OsString> = Default::default();
    entorno
        .split(';')
        .chain(maquina.split(';'))
        .chain(usuario.split(';'))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        // el registro repite lo que el entorno ya trae: gana la
        // primera aparición, las repetidas no generan candidatos
        .filter(|dir| vistas.insert(dir.clone().into_os_string()))
        .collect()
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
        fn path_configurado(&self) -> std::io::Result<Vec<PathBuf>> {
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
        let dirs = SondadorShell.path_configurado().expect("sondeo del shell");
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

    // ---- Registro de Windows (#45): lógica pura con falsos de registro ----

    /// Falso de variables de entorno para expandir `%REFERENCIAS%` sin
    /// depender del entorno real de la máquina que corre el test.
    fn sin_variables(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn paths_de_registro_suma_entorno_y_registro_en_orden() {
        assert_eq!(
            paths_de_registro(
                "C:\\Windows\\system32;C:\\Windows",
                "%HOMEDRIVE%\\herramientas",
                "C:\\Archivos de programa\\nodejs;;C:\\Windows",
                &sin_variables,
            ),
            vec![
                PathBuf::from("C:\\Windows\\system32"),
                PathBuf::from("C:\\Windows"),
                PathBuf::from("C:\\Archivos de programa\\nodejs"),
                // una referencia sin valor queda literal, como en Windows
                PathBuf::from("%HOMEDRIVE%\\herramientas"),
            ]
        );
    }

    #[test]
    fn expandir_referencias_resuelve_las_variables_y_deja_las_desconocidas() {
        let appdata = |nombre: &str| {
            (nombre == "APPDATA").then(|| "C:\\Usuarios\\ej\\AppData\\Roaming".to_string())
        };
        assert_eq!(
            expandir_referencias("%APPDATA%\\npm;%DESCONOCIDA%\\bin", &appdata),
            "C:\\Usuarios\\ej\\AppData\\Roaming\\npm;%DESCONOCIDA%\\bin"
        );
        // un % suelto (un directorio llamado «100%») no se toca
        assert_eq!(
            expandir_referencias("D:\\100%\\listo", &appdata),
            "D:\\100%\\listo"
        );
    }

    #[test]
    fn paths_de_registro_expande_las_referencias_del_registro() {
        let appdata = |nombre: &str| {
            (nombre == "APPDATA").then(|| "C:\\Usuarios\\ej\\AppData\\Roaming".to_string())
        };
        assert_eq!(
            paths_de_registro("C:\\Windows", "%APPDATA%\\npm", "", &appdata),
            vec![
                PathBuf::from("C:\\Windows"),
                PathBuf::from("C:\\Usuarios\\ej\\AppData\\Roaming\\npm"),
            ]
        );
    }

    #[test]
    fn path_del_reg_toma_el_valor_tras_el_tipo() {
        // salida de `reg query HKCU\Environment /v Path` (REG_EXPAND_SZ)
        let usuario = "\r\n\
             HKEY_CURRENT_USER\\Environment\r\n\
             \x20   Path    REG_EXPAND_SZ    %USERPROFILE%\\bin;%APPDATA%\\npm\r\n\r\n";
        assert_eq!(
            path_del_reg(usuario),
            Some("%USERPROFILE%\\bin;%APPDATA%\\npm")
        );
        // salida de la rama de máquina (REG_SZ, valor con espacios)
        let maquina = "HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment\r\n\
             \x20   Path    REG_SZ    C:\\Program Files\\nodejs;C:\\Windows\r\n";
        assert_eq!(
            path_del_reg(maquina),
            Some("C:\\Program Files\\nodejs;C:\\Windows")
        );
    }

    #[test]
    fn path_del_reg_sin_fila_de_valor_es_ninguno() {
        // la rama existe pero el valor Path no: reg.exe responde con error
        // (exit != 0) y sin fila de valor — nada que leer
        assert_eq!(path_del_reg("HKEY_CURRENT_USER\\Environment\r\n"), None);
        // una fila de valor sin contenido tampoco aporta
        assert_eq!(path_del_reg("    Path    REG_SZ    \r\n"), None);
    }

    /// Falso de registro (#45): responde los PATH crudos de usuario y
    /// de máquina contando las lecturas — el contrato de «una vez por
    /// sesión» se prueba aquí, sin tocar el registro de verdad.
    struct FalsoRegistro {
        usuario: Result<String, String>,
        maquina: Result<String, String>,
        lecturas: std::cell::RefCell<usize>,
    }

    impl FalsoRegistro {
        fn con_paths(usuario: &str, maquina: &str) -> Self {
            Self {
                usuario: Ok(usuario.to_string()),
                maquina: Ok(maquina.to_string()),
                lecturas: std::cell::RefCell::new(0),
            }
        }

        fn que_falla() -> Self {
            Self {
                usuario: Err("registro inaccesible".to_string()),
                maquina: Err("registro inaccesible".to_string()),
                lecturas: std::cell::RefCell::new(0),
            }
        }
    }

    impl Registro for FalsoRegistro {
        fn path_usuario(&self) -> std::io::Result<String> {
            *self.lecturas.borrow_mut() += 1;
            self.usuario.clone().map_err(std::io::Error::other)
        }

        fn path_maquina(&self) -> std::io::Result<String> {
            *self.lecturas.borrow_mut() += 1;
            self.maquina.clone().map_err(std::io::Error::other)
        }
    }

    #[test]
    fn el_sondador_de_windows_suma_entorno_y_registro() {
        // nombres de archivo que no existen en el PATH de ningún runner:
        // la resolución no puede colisionar con binarios preinstalados
        let dir = tempfile::tempdir().unwrap();
        let del_usuario = dir.path().join("del_usuario");
        let de_maquina = dir.path().join("de_maquina");
        dir_con_bin(&del_usuario, "fantasma45");
        dir_con_bin(&de_maquina, "fantasma45b");
        // los PATH del registro llegan crudos, con `%REFERENCIAS%` sin
        // expandir incluidas: aquí van expandidos para no depender del
        // entorno de la máquina que corre el test
        let falso = FalsoRegistro::con_paths(
            &del_usuario.display().to_string(),
            &de_maquina.display().to_string(),
        );
        let mut sesion = SesionSondeo::default();
        // la capa resuelve el binario de usuario…
        assert_eq!(
            capa_sondeo("fantasma45", &SondadorWindows::new(&falso), &mut sesion),
            Some(del_usuario.join(con_extension("fantasma45")))
        );
        // …y el de máquina: ambos quedaron en el PATH configurado
        assert_eq!(
            capa_sondeo("fantasma45b", &SondadorWindows::new(&falso), &mut sesion),
            Some(de_maquina.join(con_extension("fantasma45b")))
        );
    }

    #[test]
    fn el_registro_se_consulta_una_vez_por_sesion() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("volta");
        dir_con_bin(&bin_dir, "fantasma45");
        let falso = FalsoRegistro::con_paths(&bin_dir.display().to_string(), "C:\\Windows");
        let sondador = SondadorWindows::new(&falso);
        let mut sesion = SesionSondeo::default();
        assert!(capa_sondeo("fantasma45", &sondador, &mut sesion).is_some());
        let _ = capa_sondeo("fantasma45", &sondador, &mut sesion);
        let _ = capa_sondeo("otro", &sondador, &mut sesion);
        assert_eq!(
            *falso.lecturas.borrow(),
            2,
            "usuario y máquina se leen una sola vez por sesión, ni por gestor ni por comando"
        );
    }

    #[test]
    fn si_una_rama_del_registro_falla_las_demas_suman() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("por_maquina");
        dir_con_bin(&bin_dir, "fantasma45");
        let mut falso =
            FalsoRegistro::con_paths("C:\\Usuarios\\ej\\bin", &bin_dir.display().to_string());
        falso.usuario = Err("sin valor Path de usuario".to_string());
        let mut sesion = SesionSondeo::default();
        // el PATH de usuario no existe (valor ausente): la capa no se
        // cae — la máquina sigue sumando
        assert_eq!(
            capa_sondeo("fantasma45", &SondadorWindows::new(&falso), &mut sesion),
            Some(bin_dir.join(con_extension("fantasma45")))
        );
        // y con el registro entero inaccesible la capa tampoco es
        // error: aporta el PATH del entorno, lo que haya
        let inaccesible = FalsoRegistro::que_falla();
        let roto = SondadorWindows::new(&inaccesible);
        assert!(roto.path_configurado().is_ok());
    }

    #[test]
    fn el_no_encontrado_lista_tambien_el_registro_de_windows() {
        let dir = tempfile::tempdir().unwrap();
        // el registro reporta un PATH donde el gestor NO está: la capa
        // miró esas rutas y el mensaje de «no encontrado» las lista
        let mirada = dir.path().join("npm_global");
        std::fs::create_dir_all(&mirada).unwrap();
        let falso = FalsoRegistro::con_paths(&mirada.display().to_string(), "");
        let mut sesion = SesionSondeo::default();
        assert!(capa_sondeo("fantasma45", &SondadorWindows::new(&falso), &mut sesion).is_none());
        let mut buscadas = vec![PathBuf::from("C:\\pnpm")];
        buscadas.extend(buscadas_sondeo(&sesion, "fantasma45"));
        let msg = crate::kernel::no_encontrado("pnpm", &buscadas).to_string();
        assert!(
            msg.contains(
                &mirada
                    .join(con_extension("fantasma45"))
                    .display()
                    .to_string()
            ),
            "{msg}"
        );
    }
}
