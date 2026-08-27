//! Gestor bun: binario autocontenido (sin shim de node) y espacio global
//! propio. `pm ls -g` dibuja un árbol y `outdated -g` una tabla para
//! humanos — sin JSON: se parsean ambas, y un formato inesperado es error
//! visible, nunca un falso "todo al día".

use crate::npm::{
    armar, correr_streaming, instalar, Runner, RunnerOutput, Snapshot, UpdateOutcome,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// ¿Hay binario de bun en esta máquina? Chequeo de presencia (sin spawn).
pub fn instalado() -> bool {
    if crate::npm::find_in_path("bun").is_some() {
        return true;
    }
    std::env::var_os("HOME")
        .is_some_and(|h| PathBuf::from(h).join(".bun/bin/bun").is_file())
}

/// Runner real de bun: PATH o ~/.bun/bin/bun. Bun no depende de node: no
/// hace falta anteponer el PATH de nvm.
pub struct RealBunRunner {
    bin: PathBuf,
    bun_version: String,
}

impl RealBunRunner {
    pub fn discover() -> std::io::Result<Self> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let bin = crate::npm::find_in_path("bun")
            .or_else(|| home.map(|h| h.join(".bun/bin/bun")))
            .filter(|p| p.is_file())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "bun no encontrado")
            })?;
        let bun_version = std::process::Command::new(&bin)
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| "desconocida".to_string());
        Ok(Self { bin, bun_version })
    }
}

impl Runner for RealBunRunner {
    fn version(&self) -> String {
        self.bun_version.clone()
    }

    fn run(&self, args: &[&str]) -> std::io::Result<RunnerOutput> {
        let out = std::process::Command::new(&self.bin).args(args).output()?;
        Ok(RunnerOutput {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            exit_code: out.status.code().unwrap_or(-1),
        })
    }

    fn run_streaming(
        &self,
        args: &[&str],
        on_line: &mut dyn FnMut(&str),
    ) -> std::io::Result<RunnerOutput> {
        let mut cmd = std::process::Command::new(&self.bin);
        cmd.args(args);
        correr_streaming(cmd, on_line)
    }
}

/// `bun pm ls -g` dibuja un árbol; solo las dependencias de PRIMER nivel
/// (prefijo ├──/└──) son los globales: las anidadas son transitivas.
/// Formato: `├── nombre@versión` (scoped: `@org/pkg@1.2.3`).
fn parse_ls(salida: &str) -> Vec<(String, String)> {
    let mut pares = Vec::new();
    for linea in salida.lines() {
        let limpia = linea.trim_start();
        let resto = limpia
            .strip_prefix("├── ")
            .or_else(|| limpia.strip_prefix("└── "));
        let Some(resto) = resto else { continue };
        let Some((name, version)) = resto.rsplit_once('@') else {
            continue;
        };
        if !name.is_empty() && !version.is_empty() {
            pares.push((name.to_string(), version.to_string()));
        }
    }
    pares
}

/// `bun outdated -g` imprime una tabla para humanos. Las posiciones de las
/// columnas se leen del ENCABEZADO real (Package/Latest); toda fila que no
/// respete el ancho del encabezado, o celda vacía donde va la versión,
/// invalida la tabla completa (None → error visible): un formato cambiado
/// jamás se traduce en falsos al-día. Tabla sin filas = todo al día.
fn parse_tabla(salida: &str) -> Option<BTreeMap<String, String>> {
    let lineas: Vec<&str> = salida.lines().collect();
    let encabezado = lineas
        .iter()
        .find(|l| l.starts_with('|') && l.contains("Package"))?;
    let celdas_h: Vec<&str> = encabezado.split('|').map(str::trim).collect();
    let col_pkg = celdas_h.iter().position(|c| *c == "Package")?;
    let col_lat = celdas_h.iter().position(|c| *c == "Latest")?;
    let ancho = celdas_h.len();

    let mut map = BTreeMap::new();
    for l in lineas {
        if !l.starts_with('|') || l.contains("--") || l.contains("Package") {
            continue; // separadores y encabezado
        }
        let celdas: Vec<&str> = l.split('|').map(str::trim).collect();
        if celdas.len() != ancho {
            return None; // fila con otro ancho: el formato cambió
        }
        let name = celdas[col_pkg];
        let latest = celdas[col_lat];
        if name.is_empty() {
            continue;
        }
        if latest.is_empty() {
            return None; // fila rota: no fabricar al-día
        }
        map.insert(name.to_string(), latest.to_string());
    }
    Some(map)
}

/// Foto del espacio global de bun.
pub fn snapshot(runner: &dyn Runner) -> std::io::Result<Snapshot> {
    let ls = runner.run(&["pm", "ls", "-g"])?;
    if ls.exit_code != 0 && !ls.stdout.contains("──") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("bun pm ls falló (exit {}): {}", ls.exit_code, ls.stderr.trim()),
        ));
    }
    let pares = parse_ls(&ls.stdout);
    if pares.is_empty() {
        // Espacio vacío legítimo: salida vacía o el encabezado del árbol
        // sin ramas. Otra cosa con cero globales parseados = el formato
        // del árbol cambió: error visible, no un falso "sin paquetes".
        if ls.stdout.trim().is_empty() || ls.stdout.contains("node_modules") {
            return Ok(Snapshot {
                version: runner.version(),
                packages: Vec::new(),
            });
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "bun pm ls no produjo el árbol esperado (exit {}): {}",
                ls.exit_code,
                ls.stderr.trim()
            ),
        ));
    }
    let out = runner.run(&["outdated", "-g"])?;
    // Salida vacía con exit 0 = nada desactualizado (bun puede omitir la
    // tabla); sin tabla reconocible el resto de casos es error visible.
    let tabla = if out.exit_code == 0 && out.stdout.trim().is_empty() {
        Some(BTreeMap::new())
    } else {
        parse_tabla(&out.stdout)
    };
    match tabla {
        Some(mapa) => Ok(Snapshot {
            version: runner.version(),
            packages: armar(pares, &mapa),
        }),
        // Sin tabla reconocible: fallo real (exit != 0) o formato de bun
        // cambiado — en ambos casos error visible, jamás "todo al día".
        None => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "bun outdated no produjo la tabla esperada (exit {}): {}",
                out.exit_code,
                out.stderr.trim()
            ),
        )),
    }
}

/// Actualiza un paquete global de bun (`bun add -g <paquete>@latest`).
pub fn update(
    runner: &dyn Runner,
    name: &str,
    on_line: &mut dyn FnMut(&str),
) -> std::io::Result<UpdateOutcome> {
    instalar(runner, "add", name, on_line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // Fixtures capturados de bun 1.3.14 reales
    const LS_SALIDA: &str = "/Users/sadot node_modules (49)\n├── @antfu/ni@30.5.0\n├── cowsay@1.0.0\n└── headroom-ai@0.22.4\n";
    const TABLA: &str = "bun outdated v1.3.14 (0d9b296a)\n|-----------------------------------------|\n| Package     | Current | Update | Latest |\n|-------------|---------|--------|--------|\n| cowsay      | 1.0.0   | 1.0.0  | 1.6.0  |\n|-------------|---------|--------|--------|\n| headroom-ai | 0.22.4  | 0.22.4 | 0.36.5 |\n|-----------------------------------------|\n";

    struct FakeBun {
        ls: String,
        outdated: String,
        outdated_exit: i32,
        calls: RefCell<Vec<String>>,
    }

    impl FakeBun {
        fn con_globales() -> Self {
            Self {
                ls: LS_SALIDA.into(),
                outdated: TABLA.into(),
                outdated_exit: 0,
                calls: RefCell::new(Vec::new()),
            }
        }

        fn se_llamo_a(&self, cmd: &str) -> bool {
            self.calls.borrow().iter().any(|c| c == cmd)
        }
    }

    impl Runner for FakeBun {
        fn version(&self) -> String {
            "1.3.14".into()
        }
        fn run(&self, args: &[&str]) -> std::io::Result<RunnerOutput> {
            self.calls.borrow_mut().push(args.join(" "));
            match args.first() {
                Some(&"pm") => Ok(RunnerOutput {
                    stdout: self.ls.clone(),
                    stderr: String::new(),
                    exit_code: 0,
                }),
                Some(&"outdated") => Ok(RunnerOutput {
                    stdout: self.outdated.clone(),
                    stderr: String::new(),
                    exit_code: self.outdated_exit,
                }),
                Some(&"add") => Ok(RunnerOutput {
                    stdout: "installed cowsay@1.6.0".into(),
                    stderr: String::new(),
                    exit_code: 0,
                }),
                _ => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "comando inesperado",
                )),
            }
        }
    }

    #[test]
    fn parse_ls_toma_primer_nivel_con_scoped() {
        let pares = parse_ls(LS_SALIDA);
        assert_eq!(pares.len(), 3);
        assert!(pares.contains(&("@antfu/ni".to_string(), "30.5.0".to_string())));
        assert!(pares.contains(&("headroom-ai".to_string(), "0.22.4".to_string())));
    }

    #[test]
    fn parse_ls_ignora_lineas_anidadas_y_ruido() {
        // transitivas (│ ├──) y basura del log no son globales
        let salida = "│ ├── ansi-styles@4.0.0\n[14ms] algo\n└── cowsay@1.6.0\n";
        let pares = parse_ls(salida);
        assert_eq!(pares, vec![("cowsay".to_string(), "1.6.0".to_string())]);
    }

    #[test]
    fn parse_tabla_mapea_nombre_a_latest() {
        let mapa = parse_tabla(TABLA).expect("tabla válida");
        assert_eq!(mapa.get("cowsay").map(String::as_str), Some("1.6.0"));
        assert_eq!(mapa.get("headroom-ai").map(String::as_str), Some("0.36.5"));
    }

    #[test]
    fn parse_tabla_sin_filas_es_todo_al_dia() {
        let tabla = "| Package | Current | Update | Latest |\n|---------|---------|--------|--------|\n|---------|---------|--------|--------|\n";
        assert!(parse_tabla(tabla).expect("formato ok").is_empty());
    }

    #[test]
    fn parse_tabla_formato_desconocido_da_none() {
        assert!(parse_tabla("bun cambió su salida").is_none());
        assert!(parse_tabla("").is_none());
    }

    #[test]
    fn parse_tabla_fila_con_otro_ancho_invalida_toda() {
        // encabezado de 4 columnas + fila de 3: el formato cambió
        let tabla = "| Package | Current | Latest |\n|---------|---------|--------|\n| cowsay  | 1.0.0 |\n";
        assert!(parse_tabla(tabla).is_none());
    }

    #[test]
    fn parse_tabla_fila_con_celda_vacia_invalida_toda() {
        let tabla = "| Package | Current | Latest |\n|---------|---------|--------|\n| cowsay  | 1.0.0   |        |\n";
        assert!(parse_tabla(tabla).is_none());
    }

    #[test]
    fn snapshot_arbol_cambiado_es_error_visible() {
        let mut runner = FakeBun::con_globales();
        runner.ls = "bun cambió el formato del árbol".into();
        assert!(snapshot(&runner).is_err());
    }

    #[test]
    fn snapshot_outdated_vacio_con_exit_0_es_todo_al_dia() {
        let mut runner = FakeBun::con_globales();
        runner.outdated = String::new();
        runner.outdated_exit = 0;
        let snap = snapshot(&runner).expect("vacío = al día");
        assert!(snap.packages.iter().all(|p| !p.outdated));
    }

    #[test]
    fn snapshot_detecta_desactualizados() {
        let runner = FakeBun::con_globales();
        let snap = snapshot(&runner).expect("snapshot válido");
        assert_eq!(snap.version, "1.3.14");
        let cowsay = snap.packages.iter().find(|p| p.name == "cowsay").unwrap();
        assert!(cowsay.outdated);
        assert_eq!(cowsay.latest.as_deref(), Some("1.6.0"));
    }

    #[test]
    fn snapshot_vacio_no_llama_a_outdated() {
        let mut runner = FakeBun::con_globales();
        runner.ls = String::new(); // sin árbol, sin globales
        let snap = snapshot(&runner).expect("vacío válido");
        assert!(snap.packages.is_empty());
        assert!(!runner.se_llamo_a("outdated -g"));
    }

    #[test]
    fn snapshot_formato_inesperado_es_error_visible() {
        let mut runner = FakeBun::con_globales();
        runner.outdated = "bun cambió todo el formato".into();
        assert!(snapshot(&runner).is_err());
    }

    #[test]
    fn update_usa_add_global_con_latest() {
        let runner = FakeBun::con_globales();
        let mut lineas = Vec::new();
        let out = update(&runner, "cowsay", &mut |l| lineas.push(l.to_string())).unwrap();
        assert!(out.success);
        assert!(runner.se_llamo_a("add -g cowsay@latest"));
    }
}
