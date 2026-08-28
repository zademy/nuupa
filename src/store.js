import { computed, reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "./i18n";

/**
 * Log shared by all managers: a single history with `manager/package:`
 * prefixed lines that survives tab switches. `appendLine` is the ONLY
 * owner of that prefix convention.
 */
export function crearLog(capacidad = 500) {
  const lineas = ref([]);
  const append = (linea) => {
    lineas.value.push(linea);
    if (lineas.value.length > capacidad) lineas.value.shift();
  };
  const appendLine = (gestor, paquete, linea) =>
    append(`${gestor}/${paquete}: ${linea}`);
  return { lineas, append, appendLine };
}

/**
 * Wire values of the queue result's reason (`Motivo` in Rust, serialized
 * lowercase; "plazo" per the glossary — never "timeout"). Single source
 * for store and tests.
 */
export const MOTIVO = {
  OK: "ok",
  FALLO: "fallo",
  PLAZO: "plazo",
  DETENIDO: "detenido",
};

/**
 * State of ONE manager's global package table.
 *
 * Testing seam B: injectable `invokeFn` — tests pass a fake invoke and
 * exercise loading, error, filtering, updates and exclusions without
 * touching Tauri. `gestor` travels in every invocation.
 */
export function createPackagesStore(
  invokeFn = invoke,
  gestor = "npm",
  logCompartido = null,
) {
  const { t } = useI18n();
  const state = reactive({
    snapshot: null,
    error: "",
    loading: false,
  });
  const search = ref("");
  const ESTADO = { ACTUALIZANDO: "updating", ERROR: "error" };
  const status = reactive({});
  // Failure detail per package (the row shows it as its tooltip): same
  // text that markFailed writes to the log.
  const detalle = reactive({});
  // Log: shared if the app passes one (single history across tabs);
  // own otherwise (tests and isolated use). No duplicated cap: crearLog
  // owns it.
  const logPropio = crearLog();
  const logs = logCompartido ? logCompartido.lineas : logPropio.lineas;
  const appendLog = logCompartido ? logCompartido.append : logPropio.append;

  // Refresh guard (#13): monotonic publication id — only the LATEST
  // operation (a Refresh or the queue's final snapshot) may publish;
  // stale answers are discarded, and what's in flight is never aborted.
  let idPublicacion = 0;
  // loading counts in-flight REFRESHES. The queue is NOT one: it has its
  // own flag (queue.active) and the UI combines both — a single counter
  // would couple two unrelated lifecycles (#13 review decision).
  let cargasEnVuelo = 0;

  // Refresh: query the package list and its latest versions again
  // (glossary vocabulary; also serves the initial load).
  async function refresh() {
    const id = ++idPublicacion;
    cargasEnVuelo++;
    state.loading = true;
    state.error = "";
    try {
      const snapshot = await invokeFn("list_globals", { gestor });
      if (id === idPublicacion) state.snapshot = snapshot;
    } catch (e) {
      if (id === idPublicacion) state.error = String(e);
    } finally {
      cargasEnVuelo--;
      if (cargasEnVuelo === 0) state.loading = false;
    }
  }

  // Update one package: install its latest version. On success the list
  // refreshes (the row becomes up to date) except inside "Update all",
  // which refreshes once when the queue finishes; on failure the row
  // stays marked in error and the detail (the manager's real output)
  // lives in the log. Returns true/false on success/failure; undefined
  // if it was already in-flight (the queue only counts explicit results).
  async function update(name, { refrescar = true } = {}) {
    if (status[name] === ESTADO.ACTUALIZANDO) return;
    status[name] = ESTADO.ACTUALIZANDO;
    try {
      const res = await invokeFn("update_package", { gestor, name });
      if (res?.success) {
        delete status[name];
        delete detalle[name];
        if (refrescar) await refresh();
        return true;
      }
      markFailed(
        name,
        `${t("actualizacionFallo")}\n${res?.output ?? ""}`.trim(),
      );
      return false;
    } catch (e) {
      // Same shape as the queue path (e.g. an individual timeout): prefix
      // + raw text, never the bare error.
      markFailed(name, `${t("actualizacionFallo")}\n${String(e)}`.trim());
      return false;
    }
  }

  function markFailed(name, detail) {
    status[name] = ESTADO.ERROR;
    detalle[name] = detail;
    appendLog(`${gestor}/${name}: ${detail}`);
  }

  const isUpdating = (name) => status[name] === ESTADO.ACTUALIZANDO;
  const hasError = (name) => status[name] === ESTADO.ERROR;
  // The row's tooltip: why THIS package failed (motivo + gestor output).
  const detalleFallo = (name) => detalle[name];

  // Excluded: "Update all" skips them; the individual update stays
  // available. The BACKEND is the single writer (#14): granular
  // excluir/quitar per (gestor, paquete) — the store applies only what
  // the backend confirms, so two fast clicks can never lose an exclusion.
  const excluded = ref([]);
  const isExcluded = (name) => excluded.value.includes(name);
  // #17: the file's state — "corrupto"/"ilegible" block every write until
  // the user resolves (the banner asks).
  const estadoExclusiones = ref("ok");
  const detalleExclusiones = ref("");
  // Per-package in-flight: disables THAT row's toggle only.
  const excluyendo = reactive({});

  async function cargarExclusiones() {
    try {
      const r = await invokeFn("get_excluded", { gestor });
      estadoExclusiones.value = r?.estado ?? "ok";
      detalleExclusiones.value = r?.detalle ?? "";
      excluded.value = r?.nombres ?? [];
    } catch (e) {
      appendLog(`${gestor}: ${t("cargarExclusionesFallo", { e })}`);
    }
  }

  // The user's explicit choice (#17): the damaged original is kept as
  // evidence (.corrupt) and the file starts clean.
  async function exclusionesDeCero() {
    try {
      await invokeFn("exclusiones_de_cero");
      await cargarExclusiones();
    } catch (e) {
      appendLog(`${gestor}: ${t("guardarExclusionesFallo", { e })}`);
    }
  }

  async function toggleExcluded(name) {
    if (excluyendo[name]) return; // in flight: the second click is a no-op
    if (estadoExclusiones.value !== "ok") return; // unresolved file: blocked
    excluyendo[name] = true;
    const quitar = isExcluded(name);
    try {
      await invokeFn(quitar ? "quitar_exclusion" : "excluir_paquete", {
        gestor,
        paquete: name,
      });
      // apply exactly what the backend confirmed (idempotent)
      excluded.value = quitar
        ? excluded.value.filter((n) => n !== name)
        : excluded.value.includes(name)
          ? excluded.value
          : [...excluded.value, name];
    } catch (e) {
      // no optimism to roll back: the state stays as it was
      appendLog(`${gestor}: ${t("guardarExclusionesFallo", { e })}`);
    } finally {
      delete excluyendo[name];
    }
  }

  const excluyendoAhora = (name) => !!excluyendo[name];

  // Single rule for "updatable outdated": outdated and not excluded. It
  // feeds the button and the queue.
  const desactualizables = computed(() =>
    (state.snapshot?.packages ?? []).filter(
      (p) => p.outdated && !isExcluded(p.name),
    ),
  );

  // Is there anything "Update all" can do? Looks at the full list (not
  // the filter) and discounts the excluded ones.
  const hayDesactualizados = computed(() => desactualizables.value.length > 0);

  // Counts for the statusbar (pure derivations, no invoke).
  const conteo = computed(() => {
    const all = state.snapshot?.packages ?? [];
    return {
      total: all.length,
      desactualizados: all.filter((p) => p.outdated).length,
      excluidos: excluded.value.length,
    };
  });

  // "Update all": the queue lives in Rust (list order, one at a time, a
  // failure does not stop it, excluded always skipped, graceful Stop).
  // The store delegates and reacts to `pm-cola` events (starts/result per
  // package); the invoke returns summary + final snapshot — a single
  // refresh.
  const queue = reactive({
    active: false,
    current: null,
    stopped: false,
    summary: null, // { total, ok, failed, detenidos, detenida }
  });

  async function updateAll() {
    if (desactualizables.value.length === 0 || queue.active) return;

    // The queue's final snapshot publishes through the same guard (#13):
    // a Refresh that started later wins when it resolves.
    const id = ++idPublicacion;
    queue.active = true;
    queue.stopped = false;
    queue.summary = null;
    try {
      const { resumen, snapshot } = await invokeFn("actualizar_todo", {
        gestor,
      });
      // Fresh data invalidates a stale refresh error. The summary and the
      // log are the queue's accomplished fact: they publish regardless of
      // the guard (the snapshot alone is global state).
      if (id === idPublicacion) {
        state.snapshot = snapshot;
        state.error = "";
      }
      queue.summary = resumen;
      // The summary also lives in the log: if the user is on another tab,
      // the unmounted panel's statusbar would lose it… this one keeps it.
      appendLog(
        `${gestor}: ${t("colaTerminada", { ok: resumen.ok, total: resumen.total })}` +
          (resumen.failed ? ` · ${resumen.failed} ${t("fallidos")}` : "") +
          (resumen.detenidos
            ? ` · ${resumen.detenidos} ${t("paquetesDetenidos")}`
            : "") +
          (resumen.detenida ? ` · ${t("detenida")}` : ""),
      );
    } catch (e) {
      appendLog(`${gestor}: ${t("colaFallo", { e })}`);
      await refresh();
    } finally {
      queue.current = null;
      queue.active = false;
    }
  }

  // `pm-cola` event of THIS manager (starts/result): moves the table row.
  // Output lines arrive via `pm-output` straight to the shared log
  // (App.vue, always mounted). A deadline or a stop carries its own
  // message — neither is a generic failure (#15, #16).
  function procesarEventoCola(e) {
    if (e.gestor !== gestor) return;
    if (e.tipo === "empieza") {
      queue.current = e.paquete;
      status[e.paquete] = ESTADO.ACTUALIZANDO;
    } else if (e.tipo === "resultado") {
      if (e.motivo === MOTIVO.OK || e.motivo === MOTIVO.DETENIDO) {
        // OK clears the row; a stop is a user decision, not an error —
        // the row goes back to normal and the log says it was stopped.
        delete status[e.paquete];
        delete detalle[e.paquete];
        if (e.motivo === MOTIVO.DETENIDO)
          appendLog(`${gestor}/${e.paquete}: ${t("actualizacionDetenida")}`);
      } else {
        const prefijo =
          e.motivo === MOTIVO.PLAZO
            ? t("actualizacionPlazo")
            : t("actualizacionFallo");
        markFailed(e.paquete, `${prefijo}\n${e.salida ?? ""}`.trim());
      }
    }
  }

  function stopAll() {
    if (!queue.active) return;
    queue.stopped = true;
    invokeFn("detener_actualizar_todo").catch(() => {});
  }

  // The panel went away: graceful — the in-flight update finishes, the
  // next ones never start, nothing is cut. No UI feedback: the panel is
  // being destroyed.
  function abandonarCola() {
    if (!queue.active) return;
    invokeFn("abandonar_actualizar_todo").catch(() => {});
  }

  // Packages filtered by the search (substring, case-insensitive; scoped
  // ones filter by their full name "@org/package").
  const packages = computed(() => {
    const q = search.value.trim().toLowerCase();
    const all = state.snapshot?.packages ?? [];
    return q ? all.filter((p) => p.name.toLowerCase().includes(q)) : all;
  });

  return {
    state,
    search,
    packages,
    logs,
    queue,
    hayDesactualizados,
    conteo,
    refresh,
    update,
    updateAll,
    stopAll,
    abandonarCola,
    procesarEventoCola,
    cargarExclusiones,
    exclusionesDeCero,
    estadoExclusiones,
    detalleExclusiones,
    toggleExcluded,
    excluyendoAhora,
    isUpdating,
    hasError,
    detalleFallo,
    isExcluded,
  };
}
