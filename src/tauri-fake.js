import { vi } from "vitest";

// Programmable fake of the Tauri bridge for COMPONENT tests (#19): one
// place, shared by every component test. Registered by tests-setup.js
// (before any test file resolves the real modules). vi.mock is hoisted
// above the imports, so the shared state travels inside vi.hoisted.
const puente = vi.hoisted(() => ({
  invocaciones: [],
  respuestas: new Map(),
  emisores: new Map(),
  version: "0.0.0-test",
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    puente.invocaciones.push({ cmd, args });
    const r = puente.respuestas.get(cmd);
    if (r === undefined) {
      throw new Error(`tauri-fake: sin respuesta programada para "${cmd}"`);
    }
    return typeof r === "function" ? r(args) : r;
  }),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => puente.version),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (evento, cb) => {
    if (!puente.emisores.has(evento)) puente.emisores.set(evento, new Set());
    puente.emisores.get(evento).add(cb);
    return () => puente.emisores.get(evento).delete(cb);
  }),
}));

// The handle over the fake for the tests.
export const tauri = {
  /** Every invoke the components made, in order. */
  registradas: (cmd) => puente.invocaciones.filter((i) => i.cmd === cmd),
  /** The LAST invoke of a command (undefined if none). */
  ultima: (cmd) =>
    [...puente.invocaciones].reverse().find((i) => i.cmd === cmd),
  /** Program a command's answer: a value, fn(args), or a Promise the
   *  test resolves when it wants (keeps the component waiting). */
  responder: (cmd, valor) => puente.respuestas.set(cmd, valor),
  /** Emit a Tauri event to the mounted listeners. */
  emitir: (evento, payload) =>
    (puente.emisores.get(evento) ?? new Set()).forEach((cb) => cb({ payload })),
  /** Reset between tests. */
  reiniciar: () => {
    puente.invocaciones.length = 0;
    puente.respuestas.clear();
    puente.emisores.clear();
  },
};
