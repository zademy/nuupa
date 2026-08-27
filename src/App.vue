<script setup>
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import PanelGestor from "./PanelGestor.vue";
import { crearLog } from "./store";
import { useI18n } from "./i18n";

// Tabs: only supported AND installed managers; always opens on the first
// one (npm). The backend sets the order.
const gestores = ref(["npm"]);
const activo = ref("npm");
// A single log for the whole app: survives tab switches.
const log = crearLog();

const { t, destino, alternar } = useI18n();

let desubscribir = null;

onMounted(async () => {
  // The streaming listener lives HERE (App never unmounts): the lines of
  // an in-flight update reach the shared log even while the user switches
  // tabs — nothing is lost in remounts.
  desubscribir = await listen("pm-output", (e) => {
    const { gestor, package: paquete, line } = e.payload;
    log.appendLine(gestor, paquete, line);
  });
  try {
    const instalados = await invoke("gestores_instalados");
    if (instalados.length > 0) {
      gestores.value = instalados;
      activo.value = instalados[0];
    }
  } catch {
    // no detection: npm only — the panel will show its own error if it fails
  }
});

onUnmounted(() => desubscribir?.());
</script>

<template>
  <!-- Icon sprite (a single instance, hidden): the panels' <use> elements
       reference it by id. -->
  <svg width="0" height="0" style="position: absolute" aria-hidden="true">
    <defs>
      <symbol id="ic-refrescar" viewBox="0 0 24 24">
        <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
        <path d="M21 3v5h-5" />
      </symbol>
      <symbol id="ic-actualizar" viewBox="0 0 24 24">
        <path d="M12 17V3" />
        <path d="m6 11 6 6 6-6" />
        <path d="M19 21H5" />
      </symbol>
      <symbol id="ic-detener" viewBox="0 0 24 24">
        <rect x="5" y="5" width="14" height="14" rx="2" />
      </symbol>
      <symbol id="ic-excluir" viewBox="0 0 24 24">
        <rect x="14" y="4" width="4" height="16" rx="1" />
        <rect x="6" y="4" width="4" height="16" rx="1" />
      </symbol>
      <symbol id="ic-buscar" viewBox="0 0 24 24">
        <circle cx="11" cy="11" r="8" />
        <path d="m21 21-4.3-4.3" />
      </symbol>
    </defs>
  </svg>

  <main>
    <header class="barra">
      <h1 class="wordmark">nuupa</h1>
      <nav class="pestanas" :aria-label="t('gestoresInstalados')">
        <button
          v-for="g in gestores"
          :key="g"
          :class="{ activa: g === activo }"
          :aria-current="g === activo ? 'page' : undefined"
          @click="activo = g"
        >
          {{ g }}
        </button>
      </nav>
      <!-- Language toggle: shows the language it switches TO; English is
           the default on first launch. -->
      <button
        class="idioma"
        :title="t('cambiarIdioma')"
        :aria-label="t('cambiarIdioma')"
        @click="alternar"
      >
        {{ destino }}
      </button>
    </header>

    <PanelGestor :key="activo" :gestor="activo" :log="log" />
  </main>
</template>

<style>
/* Reset + canvas: the app is dark edge to edge. */
html,
body {
  margin: 0;
  height: 100%;
  background: #0d0f12;
}
</style>

<style scoped>
main {
  --bg: #0d0f12;
  --surface: #15181d;
  --surface-2: #1c2026;
  --border: #262b33;
  --border-strong: #3a404b;
  --fg: #e6e8eb;
  --fg-muted: #9aa1ab;
  --fg-faint: #6b7280;

  font-family: system-ui, sans-serif;
  font-size: 12px;
  color: var(--fg);
  background: var(--bg);
  padding: 14px 18px 16px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
}

.barra {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 10px;
  flex-shrink: 0;
}

.wordmark {
  font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  font-size: 12px;
  font-weight: 500;
  letter-spacing: 0.08em;
  color: var(--fg);
  margin: 0;
}

/* Tabs sit pushed to the right; the language toggle is the true top-right
   corner element, right after them. */
.pestanas {
  display: flex;
  gap: 4px;
  margin-left: auto;
}

.pestanas button {
  font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  font-size: 11px;
  letter-spacing: 0.06em;
  color: var(--fg-faint);
  background: transparent;
  border: 1px solid transparent;
  border-bottom: none;
  border-radius: 5px 5px 0 0;
  padding: 4px 12px;
  cursor: pointer;
  transition: color 120ms, background 120ms;
}

.pestanas button:hover {
  color: var(--fg);
}

.pestanas button.activa {
  color: var(--fg);
  background: var(--surface);
  border-color: var(--border);
  border-bottom: 1px solid var(--surface);
  /* "fuses" with the active panel's content */
  margin-bottom: -1px;
}

.pestanas button:focus-visible,
.idioma:focus-visible {
  outline: 1px solid var(--fg);
  outline-offset: 1px;
}

.idioma {
  font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  font-size: 11px;
  letter-spacing: 0.06em;
  color: var(--fg-faint);
  background: transparent;
  border: 1px solid var(--border);
  border-radius: 5px;
  padding: 4px 10px;
  cursor: pointer;
  transition: color 120ms, background 120ms;
}

.idioma:hover {
  color: var(--fg);
  background: var(--surface);
}
</style>
