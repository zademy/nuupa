import js from "@eslint/js";
import pluginVue from "eslint-plugin-vue";
import configPrettier from "eslint-config-prettier";
import globals from "globals";

// División de responsabilidades, sin choques: Prettier es el único dueño
// del FORMATO (eslint-config-prettier apaga todas las reglas de estilo de
// ESLint) y ESLint queda solo con CALIDAD de código + SFC de Vue.
export default [
  {
    ignores: [
      "dist/",
      "src-tauri/", // Rust: cargo fmt / clippy son sus dueños
      ".github/", // YAML de CI, sensible a reformatos
      ".claude/",
      ".serena/",
    ],
  },
  js.configs.recommended,
  ...pluginVue.configs["flat/recommended"],
  configPrettier,
  {
    languageOptions: {
      globals: { ...globals.browser },
    },
    rules: {
      // Convención del proyecto: nombres de componente en español, aunque
      // sean de una sola palabra (Icono). "App" ya está exento por defecto.
      "vue/multi-word-component-names": "off",
    },
  },
];
