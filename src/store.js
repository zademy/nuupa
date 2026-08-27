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
 * State of ONE manager's global package table.
 *
 * Testing seam B: injectable `invokeFn` — tests pass a fake invoke and
 * exercise loading, error, filtering, updates and exclusions without
 * touching Tauri. `gestor` travels in every invocation.
 */
export function createPackagesStore(invokeFn = invoke, gestor = "npm", logCompartido = null) {
  const { t } = useI18n();
  const state = reactive({
    snapshot: null,
    error: "",
    loading: false,
  });
  const search = ref("");
  const ESTADO = { ACTUALIZANDO: "updating", ERROR: "error" };
  const status = reactive({});
  // Log: shared if the app passes one (single history across tabs);
  // own otherwise (tests and isolated use). No duplicated cap: crearLog
  // owns it.
  const logPropio = crearLog();
  const logs = logCompartido ? logCompartido.lineas : logPropio.lineas;
  const appendLog = logCompartido ? logCompartido.append : logPropio.append;

  // Refresh: query the package list and its latest versions again
  // (glossary vocabulary; also serves the initial load).
  async function refresh() {
    state.loading = true;
    state.error = "";
    try {
      state.snapshot = await invokeFn("list_globals", { gestor });
    } catch (e) {
      state.error = String(e);
    } finally {
      state.loading = false;
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
        if (refrescar) await refresh();
        return true;
      }
      markFailed(name, `${t("actualizacionFallo")}\n${res?.output ?? ""}`.trim());
      return false;
    } catch (e) {
      markFailed(name, String(e));
      return false;
    }
  }

  function markFailed(name, detail) {
    status[name] = ESTADO.ERROR;
    appendLog(`${gestor}/${name}: ${detail}`);
  }

  const isUpdating = (name) => status[name] === ESTADO.ACTUALIZANDO;
  const hasError = (name) => status[name] === ESTADO.ERROR;

  // Excluded: "Update all" skips them; the individual update stays
  // available. Persisted in the backend (config JSON).
  const excluded = ref([]);
  const isExcluded = (name) => excluded.value.includes(name);

  async function cargarExclusiones() {
    try {
      excluded.value = await invokeFn("get_excluded", { gestor });
    } catch (e) {
      appendLog(`${gestor}: ${t("cargarExclusionesFallo", { e })}`);
    }
  }

  // Optimistic with rollback: if the save fails, the UI goes back to the
  // real on-disk state and the failure lands in the log.
  async function toggleExcluded(name) {
    const previos = excluded.value;
    const next = isExcluded(name)
      ? previos.filter((n) => n !== name)
      : [...previos, name];
    excluded.value = next;
    try {
      await invokeFn("set_excluded", { gestor, nombres: next });
    } catch (e) {
      excluded.value = previos;
      appendLog(`${gestor}: ${t("guardarExclusionesFallo", { e })}`);
    }
  }

  // Single rule for "updatable outdated": outdated and not excluded. It
  // feeds the button and the queue.
  const desactualizables = computed(() =>
    (state.snapshot?.packages ?? []).filter(
      (p) => p.outdated && !isExcluded(p.name)
    )
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
    summary: null, // { total, ok, failed, detenida }
  });

  async function updateAll() {
    if (desactualizables.value.length === 0 || queue.active) return;

    queue.active = true;
    queue.stopped = false;
    queue.summary = null;
    try {
      const { resumen, snapshot } = await invokeFn("actualizar_todo", { gestor });
      state.snapshot = snapshot;
      queue.summary = resumen;
      // The summary also lives in the log: if the user is on another tab,
      // the unmounted panel's statusbar would lose it… this one keeps it.
      appendLog(
        `${gestor}: ${t("colaTerminada", { ok: resumen.ok, total: resumen.total })}` +
          (resumen.failed ? ` · ${resumen.failed} ${t("fallidos")}` : "") +
          (resumen.detenida ? ` · ${t("detenida")}` : "")
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
  // (App.vue, always mounted).
  function procesarEventoCola(e) {
    if (e.gestor !== gestor) return;
    if (e.tipo === "empieza") {
      queue.current = e.paquete;
      status[e.paquete] = ESTADO.ACTUALIZANDO;
    } else if (e.tipo === "resultado") {
      if (e.exito) delete status[e.paquete];
      else markFailed(e.paquete, `${t("actualizacionFallo")}\n${e.salida ?? ""}`.trim());
    }
  }

  function stopAll() {
    if (!queue.active) return;
    queue.stopped = true;
    invokeFn("detener_actualizar_todo").catch(() => {});
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
    procesarEventoCola,
    cargarExclusiones,
    toggleExcluded,
    isUpdating,
    hasError,
    isExcluded,
  };
}
