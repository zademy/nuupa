# Nuupa

App de escritorio (Tauri 2 + Vue 3 + Vite) para ver y actualizar los
paquetes globales de los gestores instalados (npm, pnpm, bun). Specs: zademy/nuupa#1, #8.

## Desarrollo

```sh
cargo tauri dev   # app completa (Rust + frontend)
npm test          # tests de la store (vitest)
npm run build     # build del frontend
```

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
