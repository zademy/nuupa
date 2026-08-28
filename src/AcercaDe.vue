<script setup>
import { onMounted, onUnmounted, ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import Icono from "./Icono.vue";
import { useI18n } from "./i18n";

// Static project facts: this dialog is their only consumer.
const AUTOR = { nombre: "zademy", url: "https://github.com/zademy" };
const REPOSITORIO = {
  nombre: "zademy/nuupa",
  url: "https://github.com/zademy/nuupa",
};
const LICENCIA = {
  nombre: "AGPL-3.0-or-later",
  url: "https://github.com/zademy/nuupa/blob/master/LICENSE",
};

const emit = defineEmits(["cerrar"]);

const { t } = useI18n();
const version = ref("");
const botonCerrar = ref(null);
const dialogo = ref(null);
// Who opened us (#20): focus returns there when the dialog closes.
const disparador = ref(null);

function alPulsarTecla(e) {
  if (e.key === "Escape") emit("cerrar");
  if (e.key !== "Tab") return;
  // Focus trap: Tab cycles INSIDE the dialog, never escaping it (#20).
  const focos = [
    ...(dialogo.value?.querySelectorAll("a[href], button:not([disabled])") ??
      []),
  ];
  if (focos.length === 0) return;
  const primero = focos[0];
  const ultimo = focos[focos.length - 1];
  const dentro = dialogo.value?.contains(document.activeElement) ?? false;
  if (!dentro) {
    // focus outside the dialog (blur, programmatic): bring it back in
    e.preventDefault();
    (e.shiftKey ? ultimo : primero).focus();
    return;
  }
  if (e.shiftKey && document.activeElement === primero) {
    e.preventDefault();
    ultimo.focus();
  } else if (!e.shiftKey && document.activeElement === ultimo) {
    e.preventDefault();
    primero.focus();
  }
}

onMounted(async () => {
  disparador.value =
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
  // The Tauri runtime owns the real version; frontend-only dev (npm run
  // dev) has no runtime behind it — "dev" keeps the dialog honest there.
  try {
    version.value = await getVersion();
  } catch {
    version.value = "dev";
  }
  document.addEventListener("keydown", alPulsarTecla);
  botonCerrar.value?.focus();
});

onUnmounted(() => {
  document.removeEventListener("keydown", alPulsarTecla);
  disparador.value?.focus();
});
</script>

<template>
  <!-- Closes on overlay click (the dialog stops propagation), Esc and the
       ✕ button — three ways out, none of them a trap. -->
  <div class="velo" @click="emit('cerrar')">
    <div
      ref="dialogo"
      class="dialogo"
      role="dialog"
      aria-modal="true"
      :aria-label="t('acercaDe')"
      @click.stop
    >
      <button
        ref="botonCerrar"
        class="cerrar"
        :title="t('cerrar')"
        :aria-label="t('cerrar')"
        @click="emit('cerrar')"
      >
        <Icono nombre="cerrar" :tamano="12" />
      </button>

      <h2 class="titulo">
        nuupa <span v-if="version" class="version">v{{ version }}</span>
      </h2>
      <p class="descripcion">{{ t("descripcion") }}</p>

      <dl class="datos">
        <div>
          <dt>{{ t("autor") }}</dt>
          <dd>
            <a :href="AUTOR.url" target="_blank" rel="noopener noreferrer">{{
              AUTOR.nombre
            }}</a>
          </dd>
        </div>
        <div>
          <dt>{{ t("repositorio") }}</dt>
          <dd>
            <a
              :href="REPOSITORIO.url"
              target="_blank"
              rel="noopener noreferrer"
              >{{ REPOSITORIO.nombre }}</a
            >
          </dd>
        </div>
        <div>
          <dt>{{ t("licencia") }}</dt>
          <dd>
            <a :href="LICENCIA.url" target="_blank" rel="noopener noreferrer">{{
              LICENCIA.nombre
            }}</a>
          </dd>
        </div>
      </dl>
    </div>
  </div>
</template>

<style scoped>
.velo {
  position: fixed;
  inset: 0;
  background: var(--overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10;
}

.dialogo {
  position: relative;
  width: min(320px, calc(100vw - 48px));
  box-sizing: border-box;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 5px;
  padding: 18px 18px 16px;
}

.titulo {
  font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.08em;
  margin: 0 24px 6px 0;
}

.version {
  color: var(--fg-faint);
}

.descripcion {
  margin: 0;
  color: var(--fg-muted);
  line-height: 1.5;
}

.datos {
  margin: 14px 0 0;
  display: grid;
  gap: 6px;
}

.datos > div {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 12px;
}

.datos dt {
  color: var(--fg-faint);
}

.datos dd {
  margin: 0;
}

.datos a {
  color: var(--fg);
  text-decoration: underline;
  text-underline-offset: 2px;
}

.cerrar {
  position: absolute;
  top: 8px;
  right: 8px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--fg-faint);
  background: transparent;
  border: none;
  border-radius: 4px;
  padding: 4px;
  cursor: pointer;
  transition:
    color 120ms,
    background 120ms;
}

.cerrar:hover {
  color: var(--fg);
  background: var(--surface-2);
}

.cerrar:focus-visible {
  outline: 1px solid var(--fg);
  outline-offset: 1px;
}
</style>
