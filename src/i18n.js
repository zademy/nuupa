import { computed, ref } from "vue";

// Tiny homegrown i18n: no dependency, two locales. English is the DEFAULT
// on first launch; the user's choice persists in localStorage. Missing
// keys fall back to English.
const CLAVE = "nuupa.idioma";
const DEFECTO = "en";
const IDIOMAS = ["en", "es"];

export const MENSAJES = {
  en: {
    gestoresInstalados: "Installed managers",
    cambiarIdioma: "Switch language",
    temaOscuro: "Switch to the dark theme",
    temaClaro: "Switch to the light theme",
    acercaDe: "About Nuupa",
    cerrar: "Close",
    descripcion:
      "See and update the global packages of your installed package managers — no commands required.",
    autor: "Author",
    repositorio: "Repository",
    licencia: "License",
    filtrarTabla: "Filter the table by package name",
    buscarPlaceholder: "Search packages…",
    actualizarTodo: "Update all",
    actualizarTodoTitulo:
      "Update all non-excluded outdated packages, one at a time",
    detenerTras: "Stop after the in-flight package",
    deteniendoTras: "Stopping after the in-flight package…",
    detenerCola: "Stop queue",
    refrescarBreve: "Refresh",
    refrescando: "Refreshing…",
    refrescar: "Refresh: query the package list and its latest versions again",
    paquete: "package",
    paquetes: "packages",
    desactualizado: "outdated",
    desactualizados: "outdated",
    excluido: "excluded",
    excluidos: "excluded",
    de: "of",
    actualizados: "updated",
    fallidos: "failed",
    detenida: "stopped",
    sinActividad: "no activity yet",
    consultando: "querying global packages…",
    sinCoincidencias: "no matches for “{q}”",
    sinPaquetes: "no global packages for {gestor} yet",
    columnaPaquete: "package",
    columnaInstalada: "installed",
    columnaUltima: "latest",
    deteniendo: "stopping…",
    actualizando: "updating…",
    quitarExclusion: "Remove exclusion (Update all will include it again)",
    excluir: "Exclude from Update all",
    excluirPaquete: "Exclude {paquete} from Update all",
    actualizarUltima:
      "Update to the latest version ({comando} {paquete}@latest)",
    actualizarPaquete: "Update {paquete}",
    actualizacionFallo: "the update failed",
    cargarExclusionesFallo: "could not load exclusions: {e}",
    guardarExclusionesFallo: "could not save exclusions: {e}",
    colaTerminada: "queue finished — {ok} of {total} updated",
    colaFallo: "the queue failed: {e}",
  },
  es: {
    gestoresInstalados: "Gestores instalados",
    cambiarIdioma: "Cambiar idioma",
    temaOscuro: "Cambiar al tema oscuro",
    temaClaro: "Cambiar al tema claro",
    acercaDe: "Acerca de Nuupa",
    cerrar: "Cerrar",
    descripcion:
      "Ve y actualiza los paquetes globales de tus gestores instalados — sin escribir comandos.",
    autor: "Autor",
    repositorio: "Repositorio",
    licencia: "Licencia",
    filtrarTabla: "Filtrar la tabla por nombre de paquete",
    buscarPlaceholder: "Buscar paquete…",
    actualizarTodo: "Actualizar todo",
    actualizarTodoTitulo:
      "Actualizar, de a uno, todos los desactualizados no excluidos",
    detenerTras: "Detener tras el paquete en curso",
    deteniendoTras: "Deteniendo tras el paquete en curso…",
    detenerCola: "Detener cola",
    refrescarBreve: "Refrescar",
    refrescando: "Refrescando…",
    refrescar: "Refrescar: volver a consultar la lista y sus últimas versiones",
    paquete: "paquete",
    paquetes: "paquetes",
    desactualizado: "desactualizado",
    desactualizados: "desactualizados",
    excluido: "excluido",
    excluidos: "excluidos",
    de: "de",
    actualizados: "actualizados",
    fallidos: "fallidos",
    detenida: "detenida",
    sinActividad: "sin actividad todavía",
    consultando: "consultando paquetes globales…",
    sinCoincidencias: "sin coincidencias para “{q}”",
    sinPaquetes: "sin paquetes globales de {gestor} todavía",
    columnaPaquete: "paquete",
    columnaInstalada: "instalada",
    columnaUltima: "última",
    deteniendo: "deteniendo…",
    actualizando: "actualizando…",
    quitarExclusion: "Quitar exclusión (Actualizar todo volverá a incluirlo)",
    excluir: "Excluir de Actualizar todo",
    excluirPaquete: "Excluir {paquete} de Actualizar todo",
    actualizarUltima:
      "Actualizar a la última versión ({comando} {paquete}@latest)",
    actualizarPaquete: "Actualizar {paquete}",
    actualizacionFallo: "la actualización falló",
    cargarExclusionesFallo: "no se pudieron cargar las exclusiones: {e}",
    guardarExclusionesFallo: "no se pudieron guardar las exclusiones: {e}",
    colaTerminada: "cola terminada — {ok} de {total} actualizados",
    colaFallo: "la cola falló: {e}",
  },
};

function idiomaGuardado() {
  try {
    const guardado = localStorage.getItem(CLAVE);
    if (IDIOMAS.includes(guardado)) return guardado;
  } catch {
    // no localStorage (tests, embedded contexts): default language
  }
  return DEFECTO;
}

// Current language, reactive: components re-render on change.
export const idioma = ref(idiomaGuardado());

// Keep <html lang> in sync (a11y, hyphenation); guarded for contexts
// without a document.
function aplicarLang() {
  try {
    document.documentElement.lang = idioma.value;
  } catch {
    // no document (tests)
  }
}

export function setIdioma(nuevo) {
  if (!IDIOMAS.includes(nuevo)) return;
  idioma.value = nuevo;
  try {
    localStorage.setItem(CLAVE, nuevo);
  } catch {
    // no localStorage: the choice lives only for this session
  }
  aplicarLang();
}

aplicarLang();

// Per-component translations: `t` resolves keys against the current
// language and interpolates {param} placeholders. Log lines bake the
// language at append time (history keeps the language it was written in).
export function useI18n() {
  const t = (clave, params = {}) => {
    const tabla = MENSAJES[idioma.value] ?? MENSAJES[DEFECTO];
    let texto = tabla[clave] ?? MENSAJES[DEFECTO][clave] ?? clave;
    for (const [k, v] of Object.entries(params)) {
      texto = texto.replaceAll(`{${k}}`, String(v));
    }
    return texto;
  };
  // The toggle shows the language it switches TO, not the current one.
  const destino = computed(() => (idioma.value === "en" ? "ES" : "EN"));
  const alternar = () => setIdioma(idioma.value === "en" ? "es" : "en");
  return { t, idioma, destino, alternar };
}
