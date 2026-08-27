import { computed, reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

/**
 * Log compartido entre todos los gestores: un único histórico con líneas
 * prefijadas `gestor/paquete:` que sobrevive a los cambios de pestaña.
 * `appendLine` es la ÚNICA dueña de esa convención de prefijo.
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
 * Estado de la tabla de paquetes globales de UN gestor.
 *
 * Seam B de testing: `invokeFn` inyectable — los tests pasan un falso
 * invoke y prueban carga, error, filtrado, actualización y exclusiones
 * sin tocar Tauri. `gestor` viaja en cada invocación.
 */
export function createPackagesStore(invokeFn = invoke, gestor = "npm", logCompartido = null) {
  const state = reactive({
    snapshot: null,
    error: "",
    loading: false,
  });
  const search = ref("");
  const ESTADO = { ACTUALIZANDO: "updating", ERROR: "error" };
  const status = reactive({});
  // Log: compartido si la app pasa uno (histórico único entre pestañas);
  // propio si no (tests y uso aislado). Sin duplicar el tope: crearLog manda.
  const logPropio = crearLog();
  const logs = logCompartido ? logCompartido.lineas : logPropio.lineas;
  const appendLog = logCompartido ? logCompartido.append : logPropio.append;

  // Refrescar: volver a consultar la lista y sus últimas versiones
  // (vocabulario del glosario; sirve también para la carga inicial).
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

  // Actualizar un paquete: instalar su última versión. Con éxito se refresca
  // la lista (la fila pasa a al día) salvo dentro de "Actualizar todo", que
  // refresca una sola vez al terminar la cola; con fallo la fila queda
  // marcada en error y el detalle (salida real del gestor) vive en el log.
  // Devuelve true/false según éxito; undefined si ya estaba en curso (la
  // cola solo cuenta resultados explícitos).
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
      markFailed(name, `la actualización falló\n${res?.output ?? ""}`.trim());
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

  // Excluidos: "Actualizar todo" los salta; la individual sigue disponible.
  // Persisten en el backend (JSON de configuración).
  const excluded = ref([]);
  const isExcluded = (name) => excluded.value.includes(name);

  async function cargarExclusiones() {
    try {
      excluded.value = await invokeFn("get_excluded", { gestor });
    } catch (e) {
      appendLog(`${gestor}: no se pudieron cargar las exclusiones: ${e}`);
    }
  }

  // Optimista con rollback: si el guardado falla, la UI vuelve al estado
  // real del disco y el fallo queda en el log.
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
      appendLog(`${gestor}: no se pudieron guardar las exclusiones: ${e}`);
    }
  }

  // Regla única de "desactualizado actualizable": desactualizado y no
  // excluido. La alimenta el botón y la cola.
  const desactualizables = computed(() =>
    (state.snapshot?.packages ?? []).filter(
      (p) => p.outdated && !isExcluded(p.name)
    )
  );

  // ¿Hay algo que "Actualizar todo" pueda hacer? Mira la lista completa (no
  // el filtro) y descuenta los excluidos.
  const hayDesactualizados = computed(() => desactualizables.value.length > 0);

  // Conteos para la barra de estado (derivados puros, sin invoke).
  const conteo = computed(() => {
    const all = state.snapshot?.packages ?? [];
    return {
      total: all.length,
      desactualizados: all.filter((p) => p.outdated).length,
      excluidos: excluded.value.length,
    };
  });

  // "Actualizar todo": la cola vive en Rust (orden de lista, de a uno, un
  // fallo no detiene, excluidos saltados siempre, Detener con gracia).
  // La store delega y reacciona a los eventos `pm-cola` (empieza/resultado
  // por paquete); el invoke devuelve resumen + snapshot final — un solo
  // refresco.
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
      // El resumen también vive en el log: si el usuario está en otra
      // pestaña, la statusbar del panel desmontado no lo perdería… esto sí.
      appendLog(
        `${gestor}: cola terminada — ${resumen.ok} de ${resumen.total} actualizados` +
          (resumen.failed ? ` · ${resumen.failed} fallidos` : "") +
          (resumen.detenida ? " · detenida" : "")
      );
    } catch (e) {
      appendLog(`${gestor}: la cola falló: ${e}`);
      await refresh();
    } finally {
      queue.current = null;
      queue.active = false;
    }
  }

  // Evento `pm-cola` de ESTE gestor (empieza/resultado): mueve la fila de
  // la tabla. Las líneas de salida llegan por `pm-output` directo al log
  // compartido (App.vue, siempre montado).
  function procesarEventoCola(e) {
    if (e.gestor !== gestor) return;
    if (e.tipo === "empieza") {
      queue.current = e.paquete;
      status[e.paquete] = ESTADO.ACTUALIZANDO;
    } else if (e.tipo === "resultado") {
      if (e.exito) delete status[e.paquete];
      else markFailed(e.paquete, `la actualización falló\n${e.salida ?? ""}`.trim());
    }
  }

  function stopAll() {
    if (!queue.active) return;
    queue.stopped = true;
    invokeFn("detener_actualizar_todo").catch(() => {});
  }

  // Paquetes filtrados por la búsqueda (subcadena, insensible a mayúsculas;
  // los scoped se filtran por su nombre completo "@org/paquete").
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
