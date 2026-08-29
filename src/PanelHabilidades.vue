<script setup>
import { onMounted } from "vue";
import Icono from "./Icono.vue";
import { createHabilidadesStore } from "./habilidades";
import { useI18n } from "./i18n";

// The user's skills folder: controls, statusbar and table. App.vue
// decides when this panel is the active tab.
const props = defineProps({
  log: { type: Object, required: true }, // log shared across tabs
});

const {
  state,
  search,
  filtradas,
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
} = createHabilidadesStore(undefined, props.log);

const { t } = useI18n();

// Wire state → i18n key. Unknown states never reach the wire (the
// backend's enum is closed), but a missing key would render raw.
const CLAVE_ESTADO = {
  no_gestionada: "estadoNoGestionada",
  invalida: "estadoInvalida",
  actual: "estadoActual",
  actualizacion_disponible: "estadoActualizacion",
};
const textoEstado = (estado) => t(CLAVE_ESTADO[estado] ?? estado);

onMounted(refresh);
</script>

<template>
  <section class="panel">
    <!-- Screen-reader announcements: states polite, errors alert. -->
    <p class="solo-lector" aria-live="polite">{{ anuncio }}</p>
    <p class="solo-lector" role="alert">{{ anuncioError }}</p>

    <div class="barra">
      <div class="controles">
        <label class="busqueda" :title="t('buscarHabilidadesTitulo')">
          <Icono nombre="buscar" :tamano="13" />
          <input
            v-model="search"
            type="search"
            :placeholder="t('buscarHabilidadPlaceholder')"
          />
        </label>
        <button
          class="refrescar solo-icono"
          :disabled="state.loading"
          :title="state.loading ? t('refrescando') : t('refrescarHabilidades')"
          :aria-label="t('refrescar')"
          @click="refresh"
        >
          <span v-if="state.loading" class="spinner mini"></span>
          <Icono v-else nombre="refrescar" :tamano="14" />
        </button>
      </div>
    </div>

    <!-- Terminal-style statusbar: the folder's situation in one line. -->
    <div class="statusbar">
      <span class="mono">
        {{ conteo.total }}
        {{ conteo.total === 1 ? t("habilidad") : t("habilidades") }}
      </span>
      <span v-if="conteo.invalidas" class="mono relevante">
        {{ conteo.invalidas }}
        {{ conteo.invalidas === 1 ? t("invalida") : t("invalidas") }}
      </span>
      <span class="statusbar-der">
        <span v-if="state.loading" class="spinner mini"></span>
      </span>
    </div>

    <p
      v-if="state.loading && state.habilidades.length === 0"
      class="estado mono"
    >
      {{ t("cargandoHabilidades") }}
    </p>
    <p v-else-if="state.error" class="error mono" role="alert">
      {{ state.error }}
    </p>

    <!-- Empty state: a valid first run — the folder does not exist yet
         or has no skills. -->
    <div
      v-if="!state.error && state.habilidades.length === 0 && !state.loading"
      class="vacio mono"
    >
      {{ t("sinHabilidades") }}
    </div>

    <!-- The manifest's emergency — writes blocked until the user
         resolves it (#17). -->
    <div
      v-if="estadoManifest === 'corrupto' || estadoManifest === 'ilegible'"
      class="error emergencia"
    >
      <template v-if="estadoManifest === 'corrupto'">
        {{ t("habilidadesCorruptas") }}
      </template>
      <template v-else>
        {{ t("habilidadesIlegibles", { e: detalleManifest }) }}
      </template>
      <button @click="refresh">{{ t("reintentar") }}</button>
      <button
        v-if="estadoManifest === 'corrupto'"
        class="emergencia-de-cero"
        @click="manifestDeCero"
      >
        {{ t("habilidadesDeCero") }}
      </button>
    </div>

    <div
      v-if="state.habilidades.length"
      class="tabla-scroll"
      :aria-busy="state.loading"
    >
      <table>
        <caption class="solo-lector">
          {{
            t("captionTablaHabilidades")
          }}
        </caption>
        <thead>
          <tr>
            <th>{{ t("columnaHabilidad") }}</th>
            <th class="angosta">{{ t("columnaEstado") }}</th>
            <th class="angosta"></th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="h in filtradas"
            :key="h.nombre"
            :class="{
              invalida: h.estado === 'invalida',
              error: hasError(h.nombre),
            }"
            :title="hasError(h.nombre) ? detalleFallo(h.nombre) : undefined"
          >
            <td class="nombre mono">{{ h.nombre }}</td>
            <td class="estado-celda mono">{{ textoEstado(h.estado) }}</td>
            <td class="acciones">
              <div class="acciones-contenido">
                <button
                  class="abrir solo-icono"
                  :title="t('abrirCarpetaHabilidad', { habilidad: h.nombre })"
                  :aria-label="
                    t('abrirCarpetaHabilidad', { habilidad: h.nombre })
                  "
                  @click="abrirCarpeta(h.nombre)"
                >
                  <Icono nombre="carpeta" :tamano="14" />
                </button>
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

.estado-celda {
  color: var(--fg-faint);
}

/* Row states: contrast markers, not color tints. */
tr.invalida td:first-child {
  box-shadow: inset 2px 0 0 0 var(--fg);
}

tr.invalida .nombre::before {
  content: "× ";
  color: var(--fg);
}

tr.error td:first-child {
  box-shadow: inset 2px 0 0 0 var(--fg);
}

tr.error .nombre::before {
  content: "× ";
  color: var(--fg);
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

.abrir {
  height: 24px;
  width: 26px;
  padding: 0;
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

.emergencia {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  margin-bottom: 10px;
}

.emergencia button {
  font: inherit;
  font-size: 11px;
  color: var(--fg-muted);
  background: transparent;
  border: 1px solid var(--border-strong);
  border-radius: 5px;
  padding: 3px 10px;
  cursor: pointer;
}

.emergencia button:hover {
  background: var(--surface);
  color: var(--fg);
}

.tabla-scroll::-webkit-scrollbar {
  width: 4px;
  height: 4px;
}

.tabla-scroll::-webkit-scrollbar-thumb {
  background: var(--border-strong);
  border-radius: 4px;
}

.tabla-scroll::-webkit-scrollbar-track {
  background: transparent;
}
</style>
