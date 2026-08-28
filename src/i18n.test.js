import { beforeEach, describe, expect, it, vi } from "vitest";

// The language module reads localStorage at import time: each test loads
// a fresh copy (vi.resetModules) with its own fake localStorage.
const almacen = { clave: null, valor: null };

function fakeLocalStorage() {
  return {
    getItem: (clave) =>
      clave === almacen.clave ? almacen.valor : almacen.valor,
    setItem: (clave, valor) => {
      almacen.clave = clave;
      almacen.valor = valor;
    },
  };
}

async function cargarI18n() {
  return await import("./i18n");
}

beforeEach(() => {
  almacen.clave = null;
  almacen.valor = null;
  globalThis.localStorage = fakeLocalStorage();
  vi.resetModules();
});

describe("i18n", () => {
  it("defaults to English on first launch", async () => {
    const { useI18n } = await cargarI18n();
    const { t } = useI18n();
    expect(t("actualizarTodo")).toBe("Update all");
  });

  it("alternar switches to Spanish, translates and persists", async () => {
    const { useI18n } = await cargarI18n();
    const { t, alternar } = useI18n();
    alternar();
    expect(t("actualizarTodo")).toBe("Actualizar todo");
    expect(almacen.clave).toBe("nuupa.idioma");
    expect(almacen.valor).toBe("es");
  });

  it("the toggle shows the language it switches to", async () => {
    const { useI18n } = await cargarI18n();
    const { destino, alternar } = useI18n();
    expect(destino.value).toBe("ES"); // English active → offers Spanish
    alternar();
    expect(destino.value).toBe("EN"); // Spanish active → offers English
  });

  it("a persisted language wins over the default", async () => {
    almacen.valor = "es";
    const { useI18n } = await cargarI18n();
    const { t } = useI18n();
    expect(t("sinActividad")).toBe("sin actividad todavía");
  });

  it("an unknown persisted value falls back to English", async () => {
    almacen.valor = "fr";
    const { useI18n } = await cargarI18n();
    const { t } = useI18n();
    expect(t("actualizarTodo")).toBe("Update all");
  });

  it("interpolates {param} placeholders", async () => {
    const { useI18n } = await cargarI18n();
    const { t } = useI18n();
    expect(t("sinPaquetes", { gestor: "bun" })).toBe(
      "no global packages for bun yet",
    );
  });

  it("both dictionaries cover exactly the same keys", async () => {
    const { MENSAJES } = await cargarI18n();
    expect(Object.keys(MENSAJES.es).sort()).toEqual(
      Object.keys(MENSAJES.en).sort(),
    );
  });

  it("a missing key falls back to English instead of leaking the key", async () => {
    const { useI18n, MENSAJES } = await cargarI18n();
    delete MENSAJES.es.actualizarTodo; // simulate an incomplete translation
    const { t, alternar } = useI18n();
    alternar();
    expect(t("actualizarTodo")).toBe("Update all");
  });
});
