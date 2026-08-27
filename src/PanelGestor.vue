<script setup>
import { nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import Icono from "./Icono.vue";
import { createPackagesStore } from "./store";

// El espacio global de UN gestor: controles, barra de estado, log y tabla.
// App.vue decide qué paneles existen (pestañas) y cuál está activo.
const props = defineProps({
  gestor: { type: String, required: true },
  log: { type: Object, required: true }, // log compartido entre pestañas
});

// El comando real que corre al actualizar, para el tooltip.
const VERBO = { npm: "npm i -g", pnpm: "pnpm add -g", bun: "bun add -g" };

const {
  state,
  search,
  packages,
  logs,
  queue,
  conteo,
  hayDesactualizados,
  refresh,
  update,
  updateAll,
  stopAll,
  cargarExclusiones,
  toggleExcluded,
  isUpdating,
  hasError,
  isExcluded,
} = createPackagesStore(undefined, props.gestor, props.log);

// El log siempre muestra la última línea: auto-scroll al fondo, también
// al montar sobre un histórico largo (immediate).
const logBox = ref(null);
watch(
  () => logs.value.length,
  async () => {
    await nextTick();
    if (logBox.value) logBox.value.scrollTop = logBox.value.scrollHeight;
  },
  { immediate: true }
);

onMounted(async () => {
  // Las exclusiones cargan ANTES de que la lista esté usable: una cola
  // temprana no puede computarse sin ellas. El listener de streaming
  // vive en App (siempre montado): nada se pierde al cambiar de pestaña.
  await cargarExclusiones();
  refresh();
});

// Salir del panel detiene su cola con gracia: termina el paquete en curso
// y no empieza el siguiente — nunca queda una cola huérfana sin log ni
// botón Detener en otra pestaña.
onUnmounted(() => stopAll());
</script>

<template>
  <section class="panel">
    <div class="barra">
      <div class="controles">
        <label class="busqueda" title="Filtrar la tabla por nombre de paquete">
          <Icono nombre="buscar" :tamano="13" />
          <input v-model="search" type="search" placeholder="Buscar paquete…" />
        </label>
        <button
          class="primario"
          :disabled="!hayDesactualizados || queue.active"
          title="Actualizar, de a uno, todos los desactualizados no excluidos"
          @click="updateAll"
        >
          <Icono nombre="actualizar" :tamano="14" />
          Actualizar todo
        </button>
        <button
          v-if="queue.active"
          class="detener solo-icono"
          :disabled="queue.stopped"
          :title="queue.stopped ? 'Deteniendo tras el paquete en curso…' : 'Detener tras el paquete en curso'"
          :aria-label="'Detener cola'"
          @click="stopAll"
        >
          <span v-if="queue.stopped" class="spinner mini"></span>
          <Icono v-else nombre="detener" :tamano="14" />
        </button>
        <button
          class="refrescar solo-icono"
          :disabled="state.loading || queue.active"
          :title="state.loading ? 'Refrescando…' : 'Refrescar: volver a consultar la lista y sus últimas versiones'"
          aria-label="Refrescar"
          @click="refresh"
        >
          <span v-if="state.loading" class="spinner mini"></span>
          <Icono v-else nombre="refrescar" :tamano="14" />
        </button>
      </div>
    </div>

    <!-- Barra de estado tipo terminal: la situacionalidad del gestor en una
         línea monocroma. -->
    <div class="statusbar">
      <span v-if="state.snapshot" class="mono">{{ gestor }} v{{ state.snapshot.version }}</span>
      <span class="mono">
        {{ conteo.total }} {{ conteo.total === 1 ? "paquete" : "paquetes" }}
      </span>
      <span class="mono" :class="{ relevante: conteo.desactualizados > 0 }">
        {{ conteo.desactualizados }}
        {{ conteo.desactualizados === 1 ? "desactualizado" : "desactualizados" }}
      </span>
      <span v-if="conteo.excluidos" class="mono">
        {{ conteo.excluidos }} {{ conteo.excluidos === 1 ? "excluido" : "excluidos" }}
      </span>
      <span class="statusbar-der">
        <span v-if="queue.summary" class="mono">
          {{ queue.summary.ok }} de {{ queue.summary.total }} actualizados<template
            v-if="queue.summary.failed"
          >
            · {{ queue.summary.failed }} fallidos</template
          ><template v-if="queue.summary.detenida"> · detenida</template>
        </span>
        <span v-if="queue.active || state.loading" class="spinner mini"></span>
      </span>
    </div>

    <!-- Log fijo entre los controles y la tabla: siempre visible (desde el
         arranque, sin aparecer de golpe) mientras la tabla scrollea;
         auto-scroll a la última línea. -->
    <section class="log">
      <div class="log-cabecera">
        <span class="log-titulo mono">log</span>
      </div>
      <pre ref="logBox" class="mono">{{
        logs.length ? logs.join("\n") : "sin actividad todavía"
      }}</pre>
    </section>

    <p v-if="state.loading && !state.snapshot" class="estado mono">
      consultando paquetes globales…
    </p>
    <p v-else-if="state.error" class="error mono">{{ state.error }}</p>

    <div v-if="state.snapshot && packages.length === 0 && search" class="vacio mono">
      sin coincidencias para “{{ search }}”
    </div>
    <div v-else-if="state.snapshot && packages.length === 0" class="vacio mono">
      sin paquetes globales de {{ gestor }} todavía
    </div>

    <div v-if="state.snapshot" class="tabla-scroll">
      <table>
        <thead>
          <tr>
            <th>paquete</th>
            <th class="angosta">instalada</th>
            <th class="angosta">última</th>
            <th class="angosta"></th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="p in packages"
            :key="p.name"
            :class="{
              desactualizado: p.outdated,
              error: hasError(p.name),
              excluido: isExcluded(p.name),
            }"
          >
            <td class="nombre mono">{{ p.name }}</td>
            <td class="version mono">{{ p.installed }}</td>
            <td class="mono">
              <template v-if="p.outdated">
                <span class="version">{{ p.installed }}</span>
                <span class="flecha"> → </span>
                <span class="nueva">{{ p.latest }}</span>
              </template>
              <template v-else>
                <span class="version">{{ p.latest ?? p.installed }}</span>
                <span class="al-dia">✓</span>
              </template>
            </td>
            <td class="acciones">
              <div class="acciones-contenido">
                <span v-if="isUpdating(p.name)" class="actualizando">
                  <span class="spinner"></span>
                  {{ queue.stopped ? "deteniendo…" : "actualizando…" }}
                </span>
                <template v-else>
                  <button
                    class="excluir"
                    :class="{ activo: isExcluded(p.name) }"
                    :disabled="!p.outdated && !isExcluded(p.name)"
                    :title="
                      isExcluded(p.name)
                        ? 'Quitar exclusión (Actualizar todo volverá a incluirlo)'
                        : 'Excluir de Actualizar todo'
                    "
                    :aria-label="'Excluir ' + p.name + ' de Actualizar todo'"
                    @click="toggleExcluded(p.name)"
                  >
                    <Icono nombre="excluir" :tamano="13" />
                  </button>
                  <button
                    class="actualizar solo-icono"
                    :disabled="!p.outdated || queue.active"
                    :title="`Actualizar a la última versión (${VERBO[gestor] ?? gestor} ${p.name}@latest)`"
                    :aria-label="'Actualizar ' + p.name"
                    @click="update(p.name)"
                  >
                    <Icono nombre="actualizar" :tamano="14" />
                  </button>
                </template>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>

<style scoped>
.panel {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.mono {
  font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  font-variant-numeric: tabular-nums;
}

.barra {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 12px;
  margin-bottom: 10px;
  flex-shrink: 0;
}

.controles {
  display: flex;
  gap: 6px;
}

.busqueda {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--fg-faint);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 5px;
  padding: 0 10px;
  height: 26px;
}

.busqueda:focus-within {
  outline: 1px solid var(--fg);
  outline-offset: 1px;
}

.busqueda input {
  font: inherit;
  font-size: 12px;
  color: var(--fg);
  background: transparent;
  border: none;
  outline: none;
  padding: 0;
  width: 180px;
}

.busqueda input::placeholder {
  color: var(--fg-faint);
}

.busqueda:focus-within,
.controles button:focus-visible,
.acciones button:focus-visible {
  outline: 1px solid var(--fg);
  outline-offset: 1px;
}

.controles button,
.acciones button {
  font: inherit;
  font-size: 12px;
  color: var(--fg-muted);
  background: transparent;
  border: 1px solid var(--border-strong);
  border-radius: 5px;
  padding: 4px 12px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
  transition: background 120ms, color 120ms;
}

.controles button:not(:disabled):hover,
.acciones button:not(:disabled):hover {
  background: var(--surface-2);
  color: var(--fg);
}

.controles button:disabled {
  color: var(--fg-faint);
  border-color: var(--border);
  cursor: default;
}

/* El único elemento de máximo contraste de toda la app. */
.primario:not(:disabled) {
  background: var(--fg);
  color: var(--bg);
  border-color: var(--fg);
  font-weight: 600;
}

.primario:not(:disabled):hover {
  background: var(--fg);
  color: var(--bg);
  opacity: 0.88;
}

.solo-icono {
  padding: 4px 0;
  width: 32px;
}

.solo-icono:not(:disabled) {
  color: var(--fg);
}

.statusbar {
  display: flex;
  align-items: center;
  gap: 14px;
  font-size: 11px;
  color: var(--fg-faint);
  border-top: 1px solid var(--border);
  border-bottom: 1px solid var(--border);
  padding: 5px 2px;
  margin-bottom: 10px;
  flex-shrink: 0;
}

.statusbar .relevante {
  color: var(--fg);
}

.statusbar-der {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: var(--fg-muted);
}

.log {
  flex-shrink: 0;
  margin-bottom: 10px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 5px;
  overflow: hidden;
}

.log-cabecera {
  padding: 4px 10px;
  border-bottom: 1px solid var(--border);
}

.log-titulo {
  font-size: 10px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--fg-faint);
}

.log pre {
  height: 96px;
  overflow: auto;
  padding: 6px 10px;
  font-size: 11px;
  line-height: 1.55;
  margin: 0;
  color: var(--fg-muted);
  white-space: pre-wrap;
  word-break: break-all;
}

.vacio {
  color: var(--fg-faint);
  font-size: 11px;
  padding: 18px 2px;
}

.tabla-scroll {
  flex: 1;
  overflow: auto;
}

table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

th {
  text-align: left;
  font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  font-size: 10px;
  font-weight: 400;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--fg-faint);
  padding: 6px 10px;
  border-bottom: 1px solid var(--border-strong);
  position: sticky;
  top: 0;
  background: var(--bg);
}

th.angosta {
  width: 1%;
  white-space: nowrap;
}

td {
  padding: 6px 10px;
  border-bottom: 1px solid var(--border);
  height: 30px;
}

tbody tr:hover td {
  background: var(--surface);
}

.nombre {
  font-weight: 500;
  color: var(--fg);
}

.version {
  color: var(--fg-faint);
}

.flecha {
  color: var(--fg-faint);
}

.nueva {
  color: var(--fg);
  font-weight: 600;
}

.al-dia {
  color: var(--fg-faint);
  margin-left: 4px;
}

/* Estados de fila: marcadores de contraste, no tintes de color. */
tr.desactualizado td:first-child {
  box-shadow: inset 2px 0 0 0 var(--fg);
}

tr.error td:first-child {
  box-shadow: inset 2px 0 0 0 var(--fg);
}

tr.error .nombre::before {
  content: "× ";
  color: var(--fg);
}

tr.excluido .nombre {
  color: var(--fg-faint);
  font-weight: 400;
}

.acciones {
  text-align: right;
}

.acciones-contenido {
  display: inline-flex;
  gap: 5px;
  align-items: center;
  justify-content: flex-end;
}

.actualizar {
  height: 24px;
  width: 26px;
  padding: 0;
}

.excluir {
  height: 24px;
  width: 26px;
  padding: 0;
  color: var(--fg);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 5px;
}

.excluir.activo {
  background: var(--surface-2);
  color: var(--fg);
  border-color: var(--fg-muted);
}

.excluir:disabled {
  color: #767e8a;
  border-color: var(--border);
  cursor: default;
}

.actualizando {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 24px;
  font-size: 11px;
  color: var(--fg-muted);
}

.spinner {
  width: 11px;
  height: 11px;
  border: 2px solid var(--border-strong);
  border-top-color: var(--fg);
  border-radius: 50%;
  animation: girar 0.8s linear infinite;
  flex-shrink: 0;
}

.spinner.mini {
  width: 8px;
  height: 8px;
  border-width: 1.5px;
}

@keyframes girar {
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .spinner {
    animation: none;
  }
}

.estado,
.error {
  font-size: 11px;
  margin: 0;
}

.estado {
  color: var(--fg-faint);
}

.error {
  color: var(--fg);
  background: var(--surface-2);
  border: 1px solid var(--border-strong);
  border-radius: 5px;
  padding: 8px 10px;
}

.log pre::-webkit-scrollbar,
.tabla-scroll::-webkit-scrollbar {
  width: 4px;
  height: 4px;
}

.log pre::-webkit-scrollbar-thumb,
.tabla-scroll::-webkit-scrollbar-thumb {
  background: var(--border-strong);
  border-radius: 4px;
}

.log pre::-webkit-scrollbar-track,
.tabla-scroll::-webkit-scrollbar-track {
  background: transparent;
}
</style>
