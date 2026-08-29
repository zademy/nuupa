import { computed, nextTick, reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "./i18n";
import { crearLog, MOTIVO } from "./store";

/**
 * Wire values of a Habilidad's state (`EstadoHabilidad` in Rust,
 * serialized snake_case). Single source for store and tests; "actual"
 * and "actualizacion_disponible" arrive with the SHA refresh (#28).
 */
export const ESTADO_HABILIDAD = {
  NO_GESTIONADA: "no_gestionada",
  INVALIDA: "invalida",
  ACTUAL: "actual",
  ACTUALIZACION: "actualizacion_disponible",
  SIN_VERIFICAR: "sin_verificar",
};

/**
 * State of the skills folder's table.
 *
 * Testing seam B (same as the packages store): injectable `invokeFn` —
 * tests pass a fake invoke and exercise loading, the manifest states,
 * search and the folder opening without touching Tauri.
 */
export function createHabilidadesStore(
  invokeFn = invoke,
  logCompartido = null,
) {
  const { t } = useI18n();
  const state = reactive({
    habilidades: [],
    error: "",
    loading: false,
  });
  const search = ref("");
  const ESTADO = { ACTUALIZANDO: "updating", ERROR: "error" };
  // Failure detail per skill (the row's tooltip): why opening failed.
  const detalle = reactive({});
  // Log: shared if the app passes one; own otherwise. crearLog owns the
  // line cap.
  const logPropio = crearLog();
  const logs = logCompartido ? logCompartido.lineas : logPropio.lineas;
  const appendLog = logCompartido ? logCompartido.append : logPropio.append;

  // Refresh guard (#13): only the LATEST refresh may publish.
  let idPublicacion = 0;
  let cargasEnVuelo = 0;

  // Refresh: scan the skills folder again (glossary vocabulary).
  async function refresh() {
    const id = ++idPublicacion;
    cargasEnVuelo++;
    state.loading = true;
    state.error = "";
    try {
      const r = await invokeFn("listar_habilidades");
      if (id === idPublicacion) {
        state.habilidades = r?.habilidades ?? [];
        // fail-closed: an unknown estado BLOCKS writes — never act on
        // what we do not understand (#17)
        const e = r?.manifest?.estado;
        estadoManifest.value =
          e === "ok" || e === "corrupto" || e === "ilegible" ? e : "ilegible";
        detalleManifest.value = r?.manifest?.detalle ?? "";
      }
    } catch (e) {
      if (id === idPublicacion) state.error = String(e);
    } finally {
      cargasEnVuelo--;
      if (cargasEnVuelo === 0) state.loading = false;
    }
  }

  // The manifest's state: "corrupto"/"ilegible" show the emergency
  // banner and block every write until the user resolves (#17).
  const estadoManifest = ref("ok");
  const detalleManifest = ref("");

  // The user's explicit choice (#17): the damaged original is kept as
  // evidence (.corrupt) and the manifest starts clean.
  async function manifestDeCero() {
    try {
      await invokeFn("habilidades_de_cero");
      await refresh();
    } catch (e) {
      appendLog(`habilidades: ${t("guardarExclusionesFallo", { e })}`);
    }
  }

  async function abrirCarpeta(nombre) {
    try {
      await invokeFn("abrir_habilidad", { nombre });
      delete status[nombre];
      delete detalle[nombre];
    } catch (e) {
      status[nombre] = ESTADO.ERROR;
      detalle[nombre] = String(e);
      anunciar(anuncioError, `${nombre}: ${t("abrirCarpetaFallo", { e })}`);
      appendLog(`habilidades/${nombre}: ${t("abrirCarpetaFallo", { e })}`);
    }
  }

  const status = reactive({});
  const hasError = (nombre) => status[nombre] === ESTADO.ERROR;
  const detalleFallo = (nombre) => detalle[nombre];

  // Skills filtered by the search (substring, case-insensitive).
  const filtradas = computed(() => {
    const q = search.value.trim().toLowerCase();
    return q
      ? state.habilidades.filter((h) => h.nombre.toLowerCase().includes(q))
      : state.habilidades;
  });

  // Counts for the statusbar (pure derivations, no invoke).
  const conteo = computed(() => ({
    total: state.habilidades.length,
    invalidas: state.habilidades.filter(
      (h) => h.estado === ESTADO_HABILIDAD.INVALIDA,
    ).length,
  }));

  // Screen-reader announcements: states polite, errors alert. aria-live
  // only announces CHANGES: clear first, set on the next tick.
  const anuncio = ref("");
  const anuncioError = ref("");

  function anunciar(region, texto) {
    region.value = "";
    nextTick(() => {
      region.value = texto;
    });
  }

  // ---- Agregar desde origen (#27) ----

  const origenInput = ref("");
  // The scan's section: report rows, loading and error live here — a
  // network failure NEVER blocks the folder's table.
  const escaneo = reactive({
    abierto: false,
    cargando: false,
    error: "",
    origen: "",
    items: [],
  });
  // Selected rutas: conformes preselected, invalidas never (they cannot
  // be activated).
  const seleccion = ref([]);
  const instalando = ref(false);

  async function escanear() {
    const origen = origenInput.value.trim();
    if (!origen || escaneo.cargando) return;
    escaneo.abierto = true;
    escaneo.cargando = true;
    escaneo.error = "";
    escaneo.origen = origen;
    escaneo.items = [];
    anunciar(anuncio, t("escaneando"));
    try {
      escaneo.items = await invokeFn("escanear_origen", { origen });
      seleccion.value = escaneo.items
        .filter((i) => i.conforme)
        .map((i) => i.ruta);
      const conformes = seleccion.value.length;
      anunciar(
        anuncio,
        t("escaneoListo", { conformes, total: escaneo.items.length }),
      );
    } catch (e) {
      escaneo.error = String(e);
    } finally {
      escaneo.cargando = false;
    }
  }

  function toggleRuta(ruta) {
    seleccion.value = seleccion.value.includes(ruta)
      ? seleccion.value.filter((r) => r !== ruta)
      : [...seleccion.value, ruta];
  }

  async function instalarSeleccionadas() {
    if (instalando.value || seleccion.value.length === 0) return;
    instalando.value = true;
    try {
      const resultados = await invokeFn("instalar_habilidades", {
        origen: escaneo.origen,
        rutas: [...seleccion.value],
      });
      let ok = 0;
      for (const r of resultados) {
        if (r.ok) ok++;
        else appendLog(`habilidades/${r.nombre}: ${r.motivo}`);
      }
      await refresh();
      const linea = t("instalacionLista", { ok, total: resultados.length });
      appendLog(`habilidades: ${linea}`);
      anunciar(anuncio, linea);
      cerrarEscaneo();
      origenInput.value = "";
    } catch (e) {
      appendLog(`habilidades: ${t("instalarFallo", { e })}`);
      anunciar(anuncioError, t("instalarFallo", { e }));
    } finally {
      instalando.value = false;
    }
  }

  function cerrarEscaneo() {
    escaneo.abierto = false;
    escaneo.error = "";
    escaneo.items = [];
    seleccion.value = [];
  }

  // ---- Actualizar una gestionada (#29) ----

  // Per-row in-flight: disables THAT row's update only.
  const actualizando = reactive({});

  // The store guards too: only an Actualización disponible with a
  // resolved manifest may update (#17: a corrupt manifest blocks every
  // write).
  function puedeActualizar(nombre) {
    const h = state.habilidades.find((x) => x.nombre === nombre);
    return (
      h?.estado === ESTADO_HABILIDAD.ACTUALIZACION &&
      estadoManifest.value === "ok"
    );
  }

  const estaActualizando = (nombre) =>
    !!actualizando[nombre] || status[nombre] === ESTADO.ACTUALIZANDO;

  async function actualizar(nombre) {
    if (actualizando[nombre] || !puedeActualizar(nombre)) return;
    actualizando[nombre] = true;
    anunciar(anuncio, `${nombre}: ${t("actualizando")}`);
    try {
      await invokeFn("actualizar_habilidad", { nombre });
      delete status[nombre];
      delete detalle[nombre];
      await refresh();
      anunciar(anuncio, t("actualizacionLista", { habilidad: nombre }));
    } catch (e) {
      const texto = `${t("actualizarFallo")}\n${String(e)}`.trim();
      status[nombre] = ESTADO.ERROR;
      detalle[nombre] = texto;
      anunciar(anuncioError, `${nombre}: ${t("actualizarFallo")}`);
      appendLog(`habilidades/${nombre}: ${texto}`);
    } finally {
      delete actualizando[nombre];
    }
  }

  // ---- Actualizar todo (#30): the sequential queue ----

  // The queue lives in Rust (list order, one at a time, a failure does
  // not stop it, shared ONE-active gate with the packages queue). The
  // store delegates and reacts to `skills-cola` events; the invoke
  // returns the summary — the list refreshes once at the end.
  const queue = reactive({
    active: false,
    current: null,
    stopped: false,
    summary: null,
  });

  const hayActualizables = computed(
    () =>
      estadoManifest.value === "ok" &&
      state.habilidades.some(
        (h) => h.estado === ESTADO_HABILIDAD.ACTUALIZACION,
      ),
  );

  async function actualizarTodo() {
    if (!hayActualizables.value || queue.active) return;
    queue.active = true;
    queue.stopped = false;
    queue.summary = null;
    anunciar(anuncio, t("actualizando"));
    try {
      const resumen = await invokeFn("actualizar_habilidades_todo");
      queue.summary = resumen;
      await refresh();
      const linea =
        t("colaTerminada", { ok: resumen.ok, total: resumen.total }) +
        (resumen.failed ? ` · ${resumen.failed} ${t("fallidos")}` : "") +
        (resumen.detenida ? ` · ${t("detenida")}` : "");
      appendLog(`habilidades: ${linea}`);
      anunciar(anuncio, linea);
    } catch (e) {
      appendLog(`habilidades: ${t("colaFallo", { e })}`);
      await refresh();
    } finally {
      queue.current = null;
      queue.active = false;
    }
  }

  // `skills-cola` event (starts/result per skill): moves the table row.
  function procesarEventoCola(e) {
    if (e.tipo === "empieza") {
      queue.current = e.habilidad;
      status[e.habilidad] = ESTADO.ACTUALIZANDO;
      anunciar(anuncio, `${e.habilidad}: ${t("actualizando")}`);
    } else if (e.tipo === "resultado") {
      if (e.motivo === MOTIVO.OK) {
        delete status[e.habilidad];
        delete detalle[e.habilidad];
      } else {
        status[e.habilidad] = ESTADO.ERROR;
        detalle[e.habilidad] =
          `${t("actualizarFallo")}\n${e.salida ?? ""}`.trim();
        anunciar(anuncioError, `${e.habilidad}: ${t("actualizarFallo")}`);
        appendLog(`habilidades/${e.habilidad}: ${detalle[e.habilidad]}`);
      }
      if (queue.current === e.habilidad) queue.current = null;
    }
  }

  function detenerTodo() {
    if (!queue.active) return;
    queue.stopped = true;
    invokeFn("detener_habilidades_todo").catch(() => {});
  }

  // The panel went away: graceful — the in-flight update finishes, the
  // next ones never start. No UI feedback: the panel is being destroyed.
  function abandonarTodo() {
    if (!queue.active) return;
    invokeFn("abandonar_habilidades_todo").catch(() => {});
  }

  return {
    state,
    search,
    filtradas,
    logs,
    conteo,
    anuncio,
    anuncioError,
    estadoManifest,
    detalleManifest,
    refresh,
    manifestDeCero,
    abrirCarpeta,
    hasError,
    detalleFallo,
    origenInput,
    escaneo,
    seleccion,
    instalando,
    escanear,
    toggleRuta,
    instalarSeleccionadas,
    cerrarEscaneo,
    actualizar,
    puedeActualizar,
    estaActualizando,
    queue,
    hayActualizables,
    actualizarTodo,
    procesarEventoCola,
    detenerTodo,
    abandonarTodo,
  };
}
