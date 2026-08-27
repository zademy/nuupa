import { computed, ref } from "vue";

// Tiny homegrown theming: no dependency, two themes over the same roles.
// Light is the DEFAULT on first launch (no system detection, same policy
// as the language); the user's choice persists in localStorage. The
// palettes themselves live in App.vue as [data-tema] CSS blocks — this
// module only picks which one is active.
const CLAVE = "nuupa.tema";
const DEFECTO = "claro";
const TEMAS = ["claro", "oscuro"];

function temaGuardado() {
  try {
    const guardado = localStorage.getItem(CLAVE);
    if (TEMAS.includes(guardado)) return guardado;
  } catch {
    // no localStorage (tests, embedded contexts): default theme
  }
  return DEFECTO;
}

// Current theme, reactive: components re-render on change.
export const tema = ref(temaGuardado());

// Keep <html data-tema> in sync (the palettes key off it); guarded for
// contexts without a document.
function aplicarTema() {
  try {
    document.documentElement.dataset.tema = tema.value;
  } catch {
    // no document (tests)
  }
}

export function setTema(nuevo) {
  if (!TEMAS.includes(nuevo)) return;
  tema.value = nuevo;
  try {
    localStorage.setItem(CLAVE, nuevo);
  } catch {
    // no localStorage: the choice lives only for this session
  }
  aplicarTema();
}

aplicarTema();

// Per-component theme access, mirroring useI18n: the toggle shows the
// theme it switches TO, not the current one.
export function useTema() {
  const destino = computed(() => (tema.value === "claro" ? "oscuro" : "claro"));
  const alternar = () => setTema(tema.value === "claro" ? "oscuro" : "claro");
  return { tema, destino, alternar };
}
