import { describe, expect, it } from "vitest";
import { crearLog } from "./store";
import { createHabilidadesStore, ESTADO_HABILIDAD } from "./habilidades";

// The skills store through the SAME seam as the packages store: an
// injectable invoke — no Tauri, no network, no real user folder.

const LISTA = {
  habilidades: [
    { nombre: "markdownlint", estado: ESTADO_HABILIDAD.NO_GESTIONADA },
    { nombre: "invalida-skill", estado: ESTADO_HABILIDAD.INVALIDA },
  ],
  manifest: { estado: "ok" },
};

function fakeInvoke() {
  return async (cmd) => {
    if (cmd === "listar_habilidades") return LISTA;
    throw new Error(`comando inesperado: ${cmd}`);
  };
}

describe("store de habilidades", () => {
  it("refrescar consulta listar_habilidades y llena la lista", async () => {
    const store = createHabilidadesStore(fakeInvoke());
    await store.refresh();
    expect(store.state.habilidades).toEqual(LISTA.habilidades);
    expect(store.state.loading).toBe(false);
    expect(store.state.error).toBe("");
    expect(store.estadoManifest.value).toBe("ok");
  });

  it("refrescar deja el error visible si invoke falla y conserva la lista vieja", async () => {
    let llamadas = 0;
    const store = createHabilidadesStore(async () => {
      llamadas++;
      if (llamadas === 1) return LISTA;
      throw "no home";
    });
    await store.refresh();
    await store.refresh();
    expect(store.state.error).toContain("no home");
    expect(store.state.habilidades).toEqual(LISTA.habilidades);
    expect(store.state.loading).toBe(false);
  });

  it("manifest corrupto llega al estado y a la fila de emergencia", async () => {
    const store = createHabilidadesStore(async () => ({
      habilidades: [],
      manifest: { estado: "corrupto" },
    }));
    await store.refresh();
    expect(store.estadoManifest.value).toBe("corrupto");
  });

  it("manifest ilegible expone su detalle", async () => {
    const store = createHabilidadesStore(async () => ({
      habilidades: [],
      manifest: { estado: "ilegible", detalle: "Permission denied" },
    }));
    await store.refresh();
    expect(store.estadoManifest.value).toBe("ilegible");
    expect(store.detalleManifest.value).toContain("Permission denied");
  });

  it("un estado de manifest desconocido es fail-closed ilegible", async () => {
    const store = createHabilidadesStore(async () => ({
      habilidades: [],
      manifest: { estado: "algo-raro" },
    }));
    await store.refresh();
    expect(store.estadoManifest.value).toBe("ilegible");
  });

  it("manifestDeCero pide el comando y refresca", async () => {
    const llamadas = [];
    const store = createHabilidadesStore(async (cmd) => {
      llamadas.push(cmd);
      if (cmd === "listar_habilidades") return LISTA;
      if (cmd === "habilidades_de_cero") return {};
    });
    await store.refresh();
    await store.manifestDeCero();
    expect(llamadas).toEqual([
      "listar_habilidades",
      "habilidades_de_cero",
      "listar_habilidades",
    ]);
    expect(store.estadoManifest.value).toBe("ok");
  });

  it("abrirCarpeta pasa el nombre validado", async () => {
    const store = createHabilidadesStore(async (cmd, args) => {
      if (cmd === "listar_habilidades") return LISTA;
      if (cmd === "abrir_habilidad") {
        expect(args).toEqual({ nombre: "markdownlint" });
        return {};
      }
    });
    await store.refresh();
    await store.abrirCarpeta("markdownlint");
    expect(store.hasError("markdownlint")).toBe(false);
  });

  it("abrirCarpeta con fallo marca la fila en error y lo deja en el log", async () => {
    const log = crearLog();
    const store = createHabilidadesStore(async (cmd) => {
      if (cmd === "listar_habilidades") return LISTA;
      if (cmd === "abrir_habilidad") throw "no such folder";
    }, log);
    await store.refresh();
    await store.abrirCarpeta("markdownlint");
    expect(store.hasError("markdownlint")).toBe(true);
    expect(store.detalleFallo("markdownlint")).toContain("no such folder");
    expect(log.lineas.value.some((l) => l.includes("markdownlint"))).toBe(true);
  });

  it("la búsqueda filtra por subcadena insensible a mayúsculas", async () => {
    const store = createHabilidadesStore(fakeInvoke());
    await store.refresh();
    expect(store.filtradas.value).toHaveLength(2);
    store.search.value = "MARK";
    expect(store.filtradas.value.map((h) => h.nombre)).toEqual([
      "markdownlint",
    ]);
  });

  it("el conteo suma el total y las inválidas", async () => {
    const store = createHabilidadesStore(fakeInvoke());
    await store.refresh();
    expect(store.conteo.value).toEqual({ total: 2, invalidas: 1 });
  });
});
