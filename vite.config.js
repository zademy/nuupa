import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [vue()],

  // Vite options tailored for Tauri development
  //
  // 1. prevent Vite from obscuring Rust errors
  clearScreen: false,
  // 2. Tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
  },
  // 3. to make use of `TAURI_ENV_*` variables instead of `TAURI_*`:
  // https://vitejs.dev/guide/env-and-mode.html#env-prefixes
  envPrefix: ["VITE_", "TAURI_ENV_"],
});
