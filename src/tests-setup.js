// Vitest setup: importing tauri-fake HERE registers its vi.mocks before
// ANY test file resolves the real Tauri modules (#19) — there is no
// import-order convention to remember.
import "./tauri-fake";
