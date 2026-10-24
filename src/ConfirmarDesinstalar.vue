<script setup>
import { onMounted, onUnmounted, ref } from "vue";
import Icono from "./Icono.vue";
import { useI18n } from "./i18n";

// Desinstalar's explicit confirmation (#44): gestor + nombre, the brake
// between the row's trash and a command that cannot be undone.
const props = defineProps({
  gestor: { type: String, required: true },
  paquete: { type: String, required: true },
});

// `confirmar` runs the Desinstalar (the panel owns the store call);
// `cerrar` leaves everything exactly as it was.
const emit = defineEmits(["confirmar", "cerrar"]);

const { t } = useI18n();
const cancelar = ref(null);
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

onMounted(() => {
  disparador.value =
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
  document.addEventListener("keydown", alPulsarTecla);
  // Focus lands on the SAFE action: a stray Enter must not uninstall.
  cancelar.value?.focus();
});

onUnmounted(() => {
  document.removeEventListener("keydown", alPulsarTecla);
  disparador.value?.focus();
});
</script>

<template>
  <!-- Closes on overlay click (the dialog stops propagation), Esc and the
       Cancel button — three ways out, none of them destructive. -->
  <div class="velo" @click="emit('cerrar')">
    <div
      ref="dialogo"
      class="dialogo"
      role="dialog"
      aria-modal="true"
      :aria-label="t('desinstalarTituloModal')"
      @click.stop
    >
      <h2 class="titulo">{{ t("desinstalarTituloModal") }}</h2>
      <p class="cuerpo">
        {{
          t("desinstalarCuerpo", {
            paquete: props.paquete,
            gestor: props.gestor,
          })
        }}
      </p>

      <div class="botones">
        <button ref="cancelar" class="cancelar" @click="emit('cerrar')">
          {{ t("cancelar") }}
        </button>
        <button class="peligro" @click="emit('confirmar')">
          <Icono nombre="papelera" :tamano="12" />
          {{ t("desinstalar") }}
        </button>
      </div>
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
  width: min(360px, calc(100vw - 48px));
  background: var(--surface);
  border: 1px solid var(--border-strong);
  border-radius: 8px;
  padding: 18px 18px 16px;
  box-sizing: border-box;
}

.titulo {
  margin: 0 0 8px;
  font-size: 13px;
  font-weight: 600;
  color: var(--fg);
}

.cuerpo {
  margin: 0 0 16px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--fg-muted);
}

.botones {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.botones button {
  font: inherit;
  font-size: 12px;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border-radius: 5px;
  padding: 5px 14px;
  cursor: pointer;
  transition:
    background 120ms,
    color 120ms,
    opacity 120ms;
}

.botones button:focus-visible {
  outline: 1px solid var(--fg);
  outline-offset: 1px;
}

.cancelar {
  color: var(--fg-muted);
  background: transparent;
  border: 1px solid var(--border-strong);
}

.cancelar:hover {
  color: var(--fg);
  background: var(--surface-2);
}

/* Destructive weight for the one irreversible action: the theme's
   single accent (danger), filled like the primary button. */
.peligro {
  color: var(--on-danger);
  background: var(--danger);
  border: 1px solid var(--danger);
  font-weight: 600;
}

.peligro:hover {
  opacity: 0.88;
}
</style>
