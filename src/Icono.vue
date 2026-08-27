<script setup>
// Iconos vía sprite <use> (definido en App.vue): cero maquinaria de Vue
// dentro del <svg> — los fragmentos de <template v-if> dentro de SVG son
// problemáticos en WebKit/WKWebView, el motor de la app en macOS.
// Los ids siguen el vocabulario: ic-refrescar, ic-actualizar, ic-detener,
// ic-excluir, ic-buscar.
defineProps({
  nombre: { type: String, required: true },
  tamano: { type: Number, default: 14 },
});
</script>

<template>
  <svg
    :width="tamano"
    :height="tamano"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2.25"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <use :href="`#ic-${nombre}`" />
  </svg>
</template>

<style scoped>
/* Hijo flex: sin esto el min-content colapsado por el <use> deja el svg en
 ~6px (flex-shrink por defecto). */
svg {
  flex-shrink: 0;
}
</style>
