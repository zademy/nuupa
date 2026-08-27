import { computed, reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

/**
 * Log compartido entre todos los gestores: un único histórico con líneas
 * prefijadas `gestor/paquete:` que sobrevive a los cambios de pestaña.
 */
export function crearLog(capacidad = 500) {
  const lineas = ref([]);
  const append = (linea) => {
    lineas.value.push(linea);
    if (lineas.value.length > capacidad) lineas.value.shift();
  };
  return { lineas, append };
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

  // Línea streameada de un gestor (evento pm-output): la store es dueña
  // de la convención de prefijo `gestor/paquete:`.
  function appendLogLine(g, pkg, line) {
    appendLog(`${g}/${pkg}: ${line}`);
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

  // "Actualizar todo": cola estrictamente secuencial sobre los
  // desactualizados (en orden de lista, sin filtro de búsqueda). Un fallo no
  // detiene la cola; "Detener" deja terminar el paquete en curso y no
  // empieza el siguiente.
  const queue = reactive({
    active: false,
    current: null,
    stopped: false,
    summary: null, // { total, ok, failed, detenida }
  });

  async function updateAll() {
    const pendientes = desactualizables.value.map((p) => p.name);
    if (pendientes.length === 0 || queue.active) return;

    queue.active = true;
    queue.stopped = false;
    queue.summary = null;
    let ok = 0;
    let failed = 0;
    let saltados = 0;
    for (const name of pendientes) {
      if (queue.stopped) break;
      // "Actualizar todo" lo salta SIEMPRE: incluso excluido a mitad de
      // cola, una vez construida.
      if (isExcluded(name)) {
        saltados++;
        continue;
      }
      queue.current = name;
      const exito = await update(name, { refrescar: false });
      if (exito === true) ok++;
      else if (exito === false) failed++;
    }
    queue.current = null;
    queue.active = false;
    await refresh();
    // "Detenida" solo si de verdad quedó cola sin correr (parar durante el
    // último paquete no dejó nada pendiente; los excluidos sí corrieron su
    // destino: saltarse no es detenerse).
    queue.summary = {
      total: pendientes.length,
      ok,
      failed,
      detenida: ok + failed + saltados < pendientes.length,
    };
    // El resumen también vive en el log: si el usuario está en otra
    // pestaña, la statusbar del panel desmontado no lo perdería… esto sí.
    appendLog(
      `${gestor}: cola terminada — ${ok} de ${pendientes.length} actualizados` +
        (failed ? ` · ${failed} fallidos` : "") +
        (queue.summary.detenida ? " · detenida" : "")
    );
  }

  function stopAll() {
    if (queue.active) queue.stopped = true;
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
    appendLogLine,
    cargarExclusiones,
    toggleExcluded,
    isUpdating,
    hasError,
    isExcluded,
  };
}
