<script setup>
// Icons via sprite <use> (defined in App.vue): zero Vue machinery inside
// the <svg> — <template v-if> fragments inside SVG are problematic in
// WebKit/WKWebView, the app's engine on macOS.
// The ids follow the vocabulary: ic-refrescar, ic-actualizar, ic-detener,
// ic-excluir, ic-buscar, ic-sol, ic-luna.
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
/* Flex child: without this the collapsed min-content of the <use> leaves
   the svg at ~6px (default flex-shrink). */
svg {
  flex-shrink: 0;
}
</style>
