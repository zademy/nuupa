//! Adapter del gestor bun: binario autocontenido (sin shim de node) y
//! espacio global propio. `pm ls -g` dibuja un árbol y `outdated -g` una
//! tabla para humanos — sin JSON: se parsean ambas, y un formato
//! inesperado es error visible, nunca un falso "todo al día". El verbo
//! (`add`) y el comando visible viven en la tabla de gestores de [`crate`].

use crate::kernel::{
    armar, con_extension, correr, correr_streaming, find_in_path, home, no_encontrado,
    primer_existente, version_de, EspacioGlobal, Runner, RunnerOutput,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Ubicaciones estándar de bun (fuera del PATH): la variable `BUN_INSTALL`
/// si existe y el default del instalador (`~/.bun/bin`, también en
/// Windows). Cero-config: la var es bonus, nunca requisito.
fn ubicaciones_bun() -> Vec<PathBuf> {
    let mut rutas = Vec::new();
    if let Some(v) = std::env::var_os("BUN_INSTALL") {
        rutas.push(PathBuf::from(v).join("bin").join(con_extension("bun")));
    }
    if let Some(h) = home() {
        rutas.push(h.join(".bun/bin").join(con_extension("bun")));
    }
    rutas
}

/// ¿Hay binario de bun en esta máquina? Chequeo de presencia (sin spawn).
pub fn instalado() -> bool {
    find_in_path(&con_extension("bun")).is_some() || primer_existente(ubicaciones_bun()).is_some()
}

/// Runner real de bun: PATH o ubicaciones estándar. Bun no depende de
/// node: no hace falta anteponer el PATH de nvm ni reportar versión de
/// node.
pub struct RealBunRunner {
    bin: PathBuf,
    bun_version: String,
}

impl RealBunRunner {
    pub fn discover() -> std::io::Result<Self> {
        let buscadas = ubicaciones_bun();
        let bin = find_in_path(&con_extension("bun"))
            .or_else(|| primer_existente(buscadas.clone()))
            .ok_or_else(|| no_encontrado("bun", &buscadas))?;
        let bun_version = version_de(std::process::Command::new(&bin).arg("--version"))
            .unwrap_or_else(|| "desconocida".to_string());
        Ok(Self { bin, bun_version })
    }

    fn command(&self, args: &[&str]) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.bin);
        cmd.args(args);
        cmd
    }
}

impl Runner for RealBunRunner {
    fn version_gestor(&self) -> String {
        self.bun_version.clone()
    }

    fn run(&self, args: &[&str]) -> std::io::Result<RunnerOutput> {
        correr(&mut self.command(args))
    }

    fn run_streaming(
        &self,
        args: &[&str],
        on_line: &mut dyn FnMut(&str),
    ) -> std::io::Result<RunnerOutput> {
        correr_streaming(self.command(args), on_line)
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
pub fn snapshot(runner: &dyn Runner) -> std::io::Result<EspacioGlobal> {
    let ls = runner.run(&["pm", "ls", "-g"])?;
    if ls.exit_code != 0 && !ls.stdout.contains("──") {
        return Err(std::io::Error::other(format!(
            "bun pm ls falló (exit {}): {}",
            ls.exit_code,
            ls.stderr.trim()
        )));
    }
    let pares = parse_ls(&ls.stdout);
    if pares.is_empty() {
        // Espacio vacío legítimo: salida vacía o el encabezado del árbol
        // sin ramas. Otra cosa con cero globales parseados = el formato
        // del árbol cambió: error visible, no un falso "sin paquetes".
        if ls.stdout.trim().is_empty() || ls.stdout.contains("node_modules") {
            return Ok(EspacioGlobal {
                version_gestor: runner.version_gestor(),
                version_node: runner.version_node(),
                packages: Vec::new(),
            });
        }
        return Err(std::io::Error::other(format!(
            "bun pm ls no produjo el árbol esperado (exit {}): {}",
            ls.exit_code,
            ls.stderr.trim()
        )));
    }
    let out = runner.run(&["outdated", "-g"])?;
    // Salida vacía —o solo el banner "bun outdated vX (hash)"— con
    // exit 0 = nada desactualizado: bun omite la tabla cuando todo
    // está al día. Cualquier otra salida sin tabla reconocible es
    // error visible.
    let todo_al_dia = out.exit_code == 0
        && out
            .stdout
            .lines()
            .all(|l| l.trim().is_empty() || l.trim_start().starts_with("bun outdated v"));
    let tabla = if todo_al_dia {
        Some(BTreeMap::new())
    } else {
        parse_tabla(&out.stdout)
    };
    match tabla {
        Some(mapa) => Ok(EspacioGlobal {
            version_gestor: runner.version_gestor(),
            version_node: runner.version_node(),
            packages: armar(pares, &mapa),
        }),
        // Sin tabla reconocible: fallo real (exit != 0) o formato de bun
        // cambiado — en ambos casos error visible, jamás "todo al día".
        None => Err(std::io::Error::other(format!(
            "bun outdated no produjo la tabla esperada (exit {}): {}",
            out.exit_code,
            out.stderr.trim()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::testutil::FakeRunner;

    // Fixtures capturados de bun 1.3.14 reales
    const LS_SALIDA: &str = "/Users/ejemplo node_modules (49)\n├── @antfu/ni@30.5.0\n├── cowsay@1.0.0\n└── headroom-ai@0.22.4\n";
    const TABLA: &str = "bun outdated v1.3.14 (0d9b296a)\n|-----------------------------------------|\n| Package     | Current | Update | Latest |\n|-------------|---------|--------|--------|\n| cowsay      | 1.0.0   | 1.0.0  | 1.6.0  |\n|-------------|---------|--------|--------|\n| headroom-ai | 0.22.4  | 0.22.4  | 0.36.5 |\n|-----------------------------------------|\n";

    fn runner_bun() -> FakeRunner {
        FakeRunner::new("1.3.14")
            .con_node(None)
            .respuesta("pm", LS_SALIDA, 0)
            .respuesta("outdated", TABLA, 0)
            .respuesta("add", "installed cowsay@1.6.0", 0)
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
        let tabla =
            "| Package | Current | Latest |\n|---------|---------|--------|\n| cowsay  | 1.0.0 |\n";
        assert!(parse_tabla(tabla).is_none());
    }

    #[test]
    fn parse_tabla_fila_con_celda_vacia_invalida_toda() {
        let tabla = "| Package | Current | Latest |\n|---------|---------|--------|\n| cowsay  | 1.0.0   |        |\n";
        assert!(parse_tabla(tabla).is_none());
    }

    #[test]
    fn snapshot_arbol_cambiado_es_error_visible() {
        let runner =
            FakeRunner::new("1.3.14").respuesta("pm", "bun cambió el formato del árbol", 0);
        assert!(snapshot(&runner).is_err());
    }

    #[test]
    fn snapshot_outdated_vacio_con_exit_0_es_todo_al_dia() {
        let runner = FakeRunner::new("1.3.14")
            .respuesta("pm", LS_SALIDA, 0)
            .respuesta("outdated", "", 0);
        let snap = snapshot(&runner).expect("vacío = al día");
        assert!(snap.packages.iter().all(|p| !p.outdated));
    }

    #[test]
    fn snapshot_banner_solo_todo_al_dia() {
        // bun 1.3.x omite la tabla cuando no hay desactualizados y deja
        // únicamente el banner de versión en stdout (exit 0)
        let runner = FakeRunner::new("1.3.14")
            .respuesta("pm", LS_SALIDA, 0)
            .respuesta("outdated", "bun outdated v1.3.14 (0d9b296a)\n", 0);
        let snap = snapshot(&runner).expect("banner solo = al día");
        assert!(snap.packages.iter().all(|p| !p.outdated));
    }

    #[test]
    fn snapshot_banner_mas_ruido_desconocido_es_error_visible() {
        // el banner es aceptable solo si es la ÚNICA línea: cualquier
        // contenido extra sin tabla sigue siendo un formato cambiado
        let runner = FakeRunner::new("1.3.14")
            .respuesta("pm", LS_SALIDA, 0)
            .respuesta(
                "outdated",
                "bun outdated v1.3.14 (0d9b296a)\nalgo nuevo\n",
                0,
            );
        assert!(snapshot(&runner).is_err());
    }

    #[test]
    fn snapshot_detecta_desactualizados_y_no_reporta_node() {
        let snap = snapshot(&runner_bun()).expect("snapshot válido");
        assert_eq!(snap.version_gestor, "1.3.14");
        assert_eq!(snap.version_node, None); // bun es autocontenido
        let cowsay = snap.packages.iter().find(|p| p.name == "cowsay").unwrap();
        assert!(cowsay.outdated);
        assert_eq!(cowsay.latest.as_deref(), Some("1.6.0"));
    }

    #[test]
    fn snapshot_vacio_no_llama_a_outdated() {
        let runner = FakeRunner::new("1.3.14").respuesta("pm", "", 0); // sin árbol, sin globales
        let snap = snapshot(&runner).expect("vacío válido");
        assert!(snap.packages.is_empty());
        assert!(!runner.se_llamo_a("outdated -g"));
    }

    #[test]
    fn snapshot_formato_inesperado_es_error_visible() {
        let runner = FakeRunner::new("1.3.14")
            .respuesta("pm", LS_SALIDA, 0)
            .respuesta("outdated", "bun cambió todo el formato", 0);
        assert!(snapshot(&runner).is_err());
    }
}
