<script setup>
import { nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { listen } from "@tauri-apps/api/event";
import Icono from "./Icono.vue";
import { createPackagesStore } from "./store";
import { useI18n } from "./i18n";

// ONE manager's global space: controls, statusbar, log and table.
// App.vue decides which panels exist (tabs) and which one is active.
const props = defineProps({
  gestor: { type: String, required: true },
  log: { type: Object, required: true }, // log shared across tabs
});

const {
  state,
  search,
  packages,
  logs,
  queue,
  conteo,
  anuncio,
  anuncioError,
  hayDesactualizados,
  refresh,
  update,
  updateAll,
  stopAll,
  abandonarCola,
  copiarDiagnostico,
  diagnosticoCopiado,
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
} = createPackagesStore(undefined, props.gestor, props.log);

const { t } = useI18n();

// The log always shows the last line: auto-scroll to the bottom, also on
// mount over a long history (immediate).
const logBox = ref(null);
watch(
  () => logs.value.length,
  async () => {
    await nextTick();
    if (logBox.value) logBox.value.scrollTop = logBox.value.scrollHeight;
  },
  { immediate: true },
);

let desuscribirCola = null;

onMounted(async () => {
  // Exclusions load BEFORE the list is usable: an early queue cannot be
  // computed without them. The streaming listener lives in App (always
  // mounted): nothing is lost when switching tabs.
  await cargarExclusiones();
  refresh();
  // Queue events (starts/result per package) move THIS panel's rows; log
  // lines travel through App.
  desuscribirCola = await listen("pm-cola", (e) =>
    procesarEventoCola(e.payload),
  );
});

// Leaving the panel winds its queue down gracefully: the in-flight
// update FINISHES (cutting an npm install mid-write leaves it broken)
// and the pending ones never start — no orphan queue remains on another
// tab. Only the Stop button cuts (#16).
onUnmounted(() => {
  desuscribirCola?.();
  abandonarCola();
});
</script>

<template>
  <section class="panel">
    <!-- Screen-reader announcements (#20): states polite, errors alert. -->
    <p class="solo-lector" aria-live="polite">{{ anuncio }}</p>
    <p class="solo-lector" role="alert">{{ anuncioError }}</p>

    <div class="barra">
      <div class="controles">
        <label class="busqueda" :title="t('filtrarTabla')">
          <Icono nombre="buscar" :tamano="13" />
          <input
            v-model="search"
            type="search"
            :placeholder="t('buscarPlaceholder')"
          />
        </label>
        <button
          class="primario"
          :disabled="
            !hayDesactualizados || queue.active || estadoExclusiones !== 'ok'
          "
          :title="t('actualizarTodoTitulo')"
          @click="updateAll"
        >
          <Icono nombre="actualizar" :tamano="14" />
          {{ t("actualizarTodo") }}
        </button>
        <button
          v-if="queue.active"
          class="detener solo-icono"
          :disabled="queue.stopped"
          :title="
            queue.stopped ? t('deteniendoColaTitulo') : t('detenerColaTitulo')
          "
          :aria-label="t('detenerCola')"
          @click="stopAll"
        >
          <span v-if="queue.stopped" class="spinner mini"></span>
          <Icono v-else nombre="detener" :tamano="14" />
        </button>
        <button
          class="refrescar solo-icono"
          :disabled="state.loading || queue.active"
          :title="state.loading ? t('refrescando') : t('refrescar')"
          :aria-label="t('refrescarBreve')"
          @click="refresh"
        >
          <span v-if="state.loading" class="spinner mini"></span>
          <Icono v-else nombre="refrescar" :tamano="14" />
        </button>
      </div>
    </div>

    <!-- Terminal-style statusbar: the manager's situation in one
         monochrome line. -->
    <div class="statusbar">
      <span v-if="state.snapshot" class="mono"
        >{{ gestor }} v{{ state.snapshot.version_gestor
        }}<template v-if="state.snapshot.version_node">
          · node {{ state.snapshot.version_node }}</template
        ></span
      >
      <span class="mono">
        {{ conteo.total }}
        {{ conteo.total === 1 ? t("paquete") : t("paquetes") }}
      </span>
      <span class="mono" :class="{ relevante: conteo.desactualizados > 0 }">
        {{ conteo.desactualizados }}
        {{
          conteo.desactualizados === 1
            ? t("desactualizado")
            : t("desactualizados")
        }}
      </span>
      <span v-if="conteo.excluidos" class="mono">
        {{ conteo.excluidos }}
        {{ conteo.excluidos === 1 ? t("excluido") : t("excluidos") }}
      </span>
      <span class="statusbar-der">
        <span v-if="queue.summary" class="mono">
          {{ queue.summary.ok }} {{ t("de") }} {{ queue.summary.total }}
          {{ t("actualizados")
          }}<template v-if="queue.summary.failed">
            · {{ queue.summary.failed }} {{ t("fallidos") }}</template
          ><template v-if="queue.summary.detenida">
            · {{ t("detenida") }}</template
          >
        </span>
        <span v-if="queue.active || state.loading" class="spinner mini"></span>
      </span>
    </div>

    <!-- Fixed log between the controls and the table: always visible (from
         startup, without popping in) while the table scrolls; auto-scroll
         to the last line. -->
    <section class="log">
      <div class="log-cabecera">
        <span class="log-titulo mono">log</span>
        <button
          class="copiar mono"
          :title="t('copiarDiagnostico')"
          @click="copiarDiagnostico"
        >
          {{ diagnosticoCopiado ? t("copiado") : t("copiarDiagnosticoBreve") }}
        </button>
      </div>
      <pre ref="logBox" class="mono">{{
        logs.length ? logs.join("\n") : t("sinActividad")
      }}</pre>
    </section>

    <p v-if="state.loading && !state.snapshot" class="estado mono">
      {{ t("consultando") }}
    </p>
    <p v-else-if="state.error" class="error mono" role="alert">
      {{ state.error }}
    </p>

    <div
      v-if="state.snapshot && packages.length === 0 && search"
      class="vacio mono"
    >
      {{ t("sinCoincidencias", { q: search }) }}
    </div>
    <div v-else-if="state.snapshot && packages.length === 0" class="vacio mono">
      {{ t("sinPaquetes", { gestor }) }}
    </div>

    <!-- #17: the exclusions file's emergency — writes blocked until the
         user resolves it. -->
    <div
      v-if="
        estadoExclusiones === 'corrupto' || estadoExclusiones === 'ilegible'
      "
      class="error emergencia"
    >
      <template v-if="estadoExclusiones === 'corrupto'">
        {{ t("exclusionesCorruptas") }}
      </template>
      <template v-else>
        {{ t("exclusionesIlegibles", { e: detalleExclusiones }) }}
      </template>
      <button @click="cargarExclusiones">{{ t("reintentar") }}</button>
      <button
        v-if="estadoExclusiones === 'corrupto'"
        @click="exclusionesDeCero"
      >
        {{ t("exclusionesDeCero") }}
      </button>
    </div>

    <div
      v-if="state.snapshot"
      class="tabla-scroll"
      :aria-busy="state.loading || queue.active"
    >
      <table>
        <caption class="solo-lector">
          {{
            t("captionTabla", { gestor })
          }}
        </caption>
        <thead>
          <tr>
            <th>{{ t("columnaPaquete") }}</th>
            <th class="angosta">{{ t("columnaInstalada") }}</th>
            <th class="angosta">{{ t("columnaUltima") }}</th>
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
            :title="hasError(p.name) ? detalleFallo(p.name) : undefined"
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
                  {{ queue.stopped ? t("deteniendo") : t("actualizando") }}
                </span>
                <template v-else>
                  <button
                    class="excluir"
                    :class="{ activo: isExcluded(p.name) }"
                    :disabled="
                      (!p.outdated && !isExcluded(p.name)) ||
                      excluyendoAhora(p.name) ||
                      estadoExclusiones !== 'ok'
                    "
                    :title="
                      isExcluded(p.name) ? t('quitarExclusion') : t('excluir')
                    "
                    :aria-label="t('excluirPaquete', { paquete: p.name })"
                    @click="toggleExcluded(p.name)"
                  >
                    <Icono nombre="excluir" :tamano="13" />
                  </button>
                  <button
                    class="actualizar solo-icono"
                    :disabled="!p.outdated || queue.active"
                    :title="
                      t('actualizarUltima', {
                        comando: state.snapshot.comando_actualizar,
                        paquete: p.name,
                      })
                    "
                    :aria-label="t('actualizarPaquete', { paquete: p.name })"
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
  transition:
    background 120ms,
    color 120ms;
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

/* The app's single highest-contrast element. */
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
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 4px 10px;
  border-bottom: 1px solid var(--border);
}

/* Copy-diagnostics (#21): a quiet ghost button in the log's header. */
.copiar {
  color: var(--fg-faint);
  background: transparent;
  border: none;
  border-radius: 4px;
  padding: 2px 6px;
  font-size: 11px;
  cursor: pointer;
  transition:
    color 120ms,
    background 120ms;
}

.copiar:hover {
  color: var(--fg);
  background: var(--surface-2);
}

.copiar:focus-visible {
  outline: 1px solid var(--fg);
  outline-offset: 1px;
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

/* Row states: contrast markers, not color tints. */
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
  color: var(--fg-faint);
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
