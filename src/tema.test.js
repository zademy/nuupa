import { beforeEach, describe, expect, it, vi } from "vitest";

// The theme module reads localStorage at import time: each test loads a
// fresh copy (vi.resetModules) with its own fake localStorage.
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

async function cargarTema() {
  return await import("./tema");
}

beforeEach(() => {
  almacen.clave = null;
  almacen.valor = null;
  globalThis.localStorage = fakeLocalStorage();
  vi.resetModules();
});

describe("tema", () => {
  it("defaults to light on first launch", async () => {
    const { tema } = await cargarTema();
    expect(tema.value).toBe("claro");
  });

  it("alternar switches to dark and persists", async () => {
    const { useTema } = await cargarTema();
    const { tema, alternar } = useTema();
    alternar();
    expect(tema.value).toBe("oscuro");
    expect(almacen.clave).toBe("nuupa.tema");
    expect(almacen.valor).toBe("oscuro");
  });

  it("the toggle shows the theme it switches to", async () => {
    const { useTema } = await cargarTema();
    const { destino, alternar } = useTema();
    expect(destino.value).toBe("oscuro"); // light active → offers dark
    alternar();
    expect(destino.value).toBe("claro"); // dark active → offers light
  });

  it("alternar cycles back to light", async () => {
    const { useTema } = await cargarTema();
    const { tema, alternar } = useTema();
    alternar();
    alternar();
    expect(tema.value).toBe("claro");
  });

  it("a persisted theme wins over the default", async () => {
    almacen.clave = "nuupa.tema";
    almacen.valor = "oscuro";
    const { tema } = await cargarTema();
    expect(tema.value).toBe("oscuro");
  });

  it("an unknown persisted value falls back to light", async () => {
    almacen.clave = "nuupa.tema";
    almacen.valor = "sepia";
    const { tema } = await cargarTema();
    expect(tema.value).toBe("claro");
  });

  it("setTema ignores unknown themes", async () => {
    const { setTema, tema } = await cargarTema();
    setTema("sepia");
    expect(tema.value).toBe("claro");
    expect(almacen.valor).toBe(null);
  });
});
