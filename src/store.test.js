import { describe, expect, it } from "vitest";
import { createPackagesStore, crearLog } from "./store";

const SNAPSHOT = {
  version: "26.2.0",
  packages: [
    { name: "@alibaba-group/open-code-review", installed: "1.10.2", latest: "1.10.2", outdated: false },
    { name: "context-mode", installed: "1.0.169", latest: "1.0.170", outdated: true },
    { name: "hunkdiff", installed: "0.17.2", latest: "0.18.0", outdated: true },
  ],
};

function fakeInvoke() {
  return async (cmd) => {
    if (cmd === "list_globals") return SNAPSHOT;
    throw new Error(`comando inesperado: ${cmd}`);
  };
}

describe("store de paquetes globales", () => {
  it("refrescar consulta list_globals y llena el snapshot", async () => {
    const store = createPackagesStore(fakeInvoke());
    await store.refresh();
    expect(store.state.snapshot).toEqual(SNAPSHOT);
    expect(store.state.loading).toBe(false);
    expect(store.state.error).toBe("");
  });

  it("refrescar deja el error visible si invoke falla", async () => {
    const store = createPackagesStore(async () => {
      throw "npm no encontrado";
    });
    await store.refresh();
    expect(store.state.snapshot).toBeNull();
    expect(store.state.error).toContain("npm no encontrado");
    expect(store.state.loading).toBe(false);
  });

  it("un refresco fallido conserva el snapshot viejo y marca el error", async () => {
    let llamadas = 0;
    const store = createPackagesStore(async () => {
      llamadas++;
      if (llamadas === 1) return SNAPSHOT;
      throw "fallo de red";
    });
    await store.refresh();
    await store.refresh();
    expect(store.state.snapshot).toEqual(SNAPSHOT); // datos viejos persistentes
    expect(store.state.error).toContain("fallo de red");
  });

  it("sin búsqueda se ven todos los paquetes", async () => {
    const store = createPackagesStore(fakeInvoke());
    await store.refresh();
    expect(store.packages.value).toHaveLength(3);
  });

  it("la búsqueda filtra por subcadena insensible a mayúsculas", async () => {
    const store = createPackagesStore(fakeInvoke());
    await store.refresh();
    store.search.value = "HUNK";
    expect(store.packages.value.map((p) => p.name)).toEqual(["hunkdiff"]);
  });

  it("la búsqueda filtra scoped por su nombre completo", async () => {
    const store = createPackagesStore(fakeInvoke());
    await store.refresh();
    store.search.value = "open-code";
    expect(store.packages.value.map((p) => p.name)).toEqual([
      "@alibaba-group/open-code-review",
    ]);
  });

  it("búsqueda sin coincidencias deja la tabla vacía", async () => {
    const store = createPackagesStore(fakeInvoke());
    await store.refresh();
    store.search.value = "inexistente";
    expect(store.packages.value).toEqual([]);
  });

  it("el filtro no interfiere con el estado de carga", async () => {
    let resolver;
    const store = createPackagesStore(
      () => new Promise((r) => (resolver = () => r(SNAPSHOT)))
    );
    const carga = store.refresh();
    store.search.value = "hunk"; // escribir durante la carga
    expect(store.state.loading).toBe(true);
    resolver();
    await carga;
    expect(store.state.loading).toBe(false);
    expect(store.packages.value.map((p) => p.name)).toEqual(["hunkdiff"]);
  });

  it("update exitoso refresca la lista y limpia el estado del paquete", async () => {
    const calls = [];
    const store = createPackagesStore(async (cmd, args) => {
      calls.push(cmd);
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "update_package") {
        expect(args).toEqual({ gestor: "npm", name: "hunkdiff" });
        return { success: true };
      }
      throw new Error(`comando inesperado: ${cmd}`);
    });
    await store.refresh();
    await store.update("hunkdiff");
    expect(store.isUpdating("hunkdiff")).toBe(false);
    expect(store.hasError("hunkdiff")).toBe(false);
    expect(calls).toEqual(["list_globals", "update_package", "list_globals"]);
  });

  it("update fallido marca la fila en error y NO refresca", async () => {
    const calls = [];
    const store = createPackagesStore(async (cmd) => {
      calls.push(cmd);
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "update_package") return { success: false };
      throw new Error(`comando inesperado: ${cmd}`);
    });
    await store.refresh();
    await store.update("hunkdiff");
    expect(store.hasError("hunkdiff")).toBe(true);
    expect(calls).toEqual(["list_globals", "update_package"]);
    expect(store.logs.value.some((l) => l.includes("hunkdiff"))).toBe(true);
  });

  it("update que lanza excepción marca la fila en error", async () => {
    const store = createPackagesStore(async (cmd) => {
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "update_package") throw "spawn falló";
    });
    await store.refresh();
    await store.update("context-mode");
    expect(store.hasError("context-mode")).toBe(true);
  });

  it("la store pasa su gestor en cada invocación", async () => {
    const vistos = [];
    const store = createPackagesStore(async (cmd, args) => {
      vistos.push(cmd);
      if (args?.gestor !== "pnpm") throw new Error(`gestor erróneo en ${cmd}`);
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "get_excluded") return [];
      if (cmd === "set_excluded") return {};
      if (cmd === "update_package") return { success: true };
    }, "pnpm");
    await store.cargarExclusiones();
    await store.refresh();
    await store.update("hunkdiff");
    await store.toggleExcluded("context-mode");
    expect(vistos).toEqual([
      "get_excluded",
      "list_globals",
      "update_package",
      "list_globals", // refresh tras la actualización exitosa
      "set_excluded",
    ]);
  });

  it("dos stores con gestores distintos aíslan su estado", async () => {
    const excluidosPorGestor = { npm: ["hunkdiff"], pnpm: [] };
    const fakePorGestor = (gestor) => async (cmd, args) => {
      if (args?.gestor !== gestor) throw new Error(`gestor cruzado: ${args?.gestor}`);
      if (cmd === "get_excluded") return excluidosPorGestor[gestor];
      if (cmd === "list_globals") return SNAPSHOT;
      throw new Error(`comando inesperado: ${cmd}`);
    };
    const storeNpm = createPackagesStore(fakePorGestor("npm"), "npm");
    const storePnpm = createPackagesStore(fakePorGestor("pnpm"), "pnpm");
    await storeNpm.cargarExclusiones();
    await storePnpm.cargarExclusiones();
    // cada una pidió LAS SUYAS (los fakes revientan si el gestor se cruza)
    expect(storeNpm.isExcluded("hunkdiff")).toBe(true);
    expect(storePnpm.isExcluded("hunkdiff")).toBe(false);
  });

  it("gestor no soportado produce error visible en la store", async () => {
    const store = createPackagesStore(
      async (cmd, args) => {
        if (args?.gestor === "yarn") throw "gestor no soportado: yarn";
        return SNAPSHOT;
      },
      "yarn"
    );
    await store.refresh();
    expect(store.state.snapshot).toBeNull();
    expect(store.state.error).toContain("gestor no soportado");
  });

  it("appendLogLine acumula líneas con prefijo gestor/paquete", () => {
    const store = createPackagesStore(fakeInvoke());
    store.appendLogLine("npm", "hunkdiff", "added 1 package");
    store.appendLogLine("npm", "hunkdiff", "done");
    expect(store.logs.value).toEqual([
      "npm/hunkdiff: added 1 package",
      "npm/hunkdiff: done",
    ]);
  });

  it("dos stores comparten un log con líneas de ambos gestores", async () => {
    const log = crearLog();
    const storeNpm = createPackagesStore(
      async (cmd) => (cmd === "list_globals" ? SNAPSHOT : []),
      "npm",
      log
    );
    const storeBun = createPackagesStore(
      async (cmd) => (cmd === "list_globals" ? SNAPSHOT : []),
      "bun",
      log
    );
    storeNpm.appendLogLine("npm", "hunkdiff", "added 1 package");
    storeBun.appendLogLine("bun", "headroom-ai", "installed");
    // el histórico es Único: ambas stores ven las mismas líneas
    expect(storeNpm.logs.value).toEqual([
      "npm/hunkdiff: added 1 package",
      "bun/headroom-ai: installed",
    ]);
    expect(storeBun.logs.value).toBe(storeNpm.logs.value);
  });

  it("el log compartido respeta el tope de líneas", () => {
    const log = crearLog(3);
    const store = createPackagesStore(fakeInvoke(), "npm", log);
    for (let i = 0; i < 5; i++) store.appendLogLine("npm", "p", `línea ${i}`);
    expect(store.logs.value).toHaveLength(3);
    expect(store.logs.value[0]).toBe("npm/p: línea 2");
  });

  it("updateAll actualiza solo los desactualizados, en orden de lista", async () => {
    const actualizados = [];
    let refrescos = 0;
    const store = createPackagesStore(async (cmd, args) => {
      if (cmd === "list_globals") {
        refrescos++;
        return SNAPSHOT;
      }
      if (cmd === "update_package") {
        actualizados.push(args.name);
        return { success: true };
      }
    });
    await store.refresh();
    await store.updateAll();
    // SNAPSHOT trae desactualizados: context-mode y hunkdiff, en ese orden
    expect(actualizados).toEqual(["context-mode", "hunkdiff"]);
    expect(refrescos).toBe(2); // carga inicial + uno al final de la cola
    expect(store.queue.summary).toEqual({
      total: 2,
      ok: 2,
      failed: 0,
      detenida: false,
    });
    expect(store.queue.active).toBe(false);
    // el resumen también vive en el log (visible aunque cambies de pestaña)
    expect(
      store.logs.value.some((l) => l.includes("cola terminada — 2 de 2 actualizados"))
    ).toBe(true);
  });

  it("updateAll continúa tras un fallo y lo cuenta", async () => {
    const actualizados = [];
    const store = createPackagesStore(async (cmd, args) => {
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "update_package") {
        actualizados.push(args.name);
        return { success: args.name !== "context-mode" };
      }
    });
    await store.refresh();
    await store.updateAll();
    expect(actualizados).toEqual(["context-mode", "hunkdiff"]); // no se detuvo
    expect(store.queue.summary).toEqual({
      total: 2,
      ok: 1,
      failed: 1,
      detenida: false,
    });
    expect(store.hasError("context-mode")).toBe(true);
  });

  it("Detener deja terminar el actual y no empieza el siguiente", async () => {
    const actualizados = [];
    const enCurso = [];
    const store = createPackagesStore(async (cmd, args) => {
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "update_package") {
        actualizados.push(args.name);
        return new Promise((resolve) => enCurso.push(() => resolve({ success: true })));
      }
    });
    await store.refresh();
    const cola = store.updateAll();
    await new Promise((r) => setTimeout(r, 0)); // arranca el primer update
    expect(store.queue.current).toBe("context-mode");
    store.stopAll();
    enCurso[0](); // termina el paquete en curso
    await cola;
    expect(actualizados).toEqual(["context-mode"]); // no empezó hunkdiff
    expect(store.queue.summary).toEqual({
      total: 2,
      ok: 1,
      failed: 0,
      detenida: true,
    });
  });

  it("updateAll sin desactualizados no hace nada", async () => {
    const sinDesactualizados = {
      ...SNAPSHOT,
      packages: SNAPSHOT.packages.map((p) => ({
        ...p,
        latest: p.installed,
        outdated: false,
      })),
    };
    const store = createPackagesStore(async () => sinDesactualizados);
    await store.refresh();
    await store.updateAll();
    expect(store.queue.active).toBe(false);
    expect(store.queue.summary).toBeNull();
  });

  it("updateAll salta los excluidos", async () => {
    const actualizados = [];
    const store = createPackagesStore(async (cmd, args) => {
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "get_excluded") return ["hunkdiff"];
      if (cmd === "update_package") {
        actualizados.push(args.name);
        return { success: true };
      }
    });
    await store.cargarExclusiones();
    await store.refresh();
    expect(store.isExcluded("hunkdiff")).toBe(true);
    await store.updateAll();
    expect(actualizados).toEqual(["context-mode"]); // hunkdiff excluido
    expect(store.queue.summary.total).toBe(1);
  });

  it("toggleExcluded persiste la lista completa por ambos lados", async () => {
    const guardado = [];
    const store = createPackagesStore(async (cmd, args) => {
      if (cmd === "get_excluded") return ["context-mode"];
      if (cmd === "set_excluded") {
        guardado.push(args.nombres);
        return {};
      }
    });
    await store.cargarExclusiones();
    store.toggleExcluded("hunkdiff"); // añade
    store.toggleExcluded("context-mode"); // quita
    expect(store.isExcluded("hunkdiff")).toBe(true);
    expect(store.isExcluded("context-mode")).toBe(false);
    expect(guardado).toEqual([
      ["context-mode", "hunkdiff"],
      ["hunkdiff"],
    ]);
  });

  it("un excluido desactualizado desactiva 'Actualizar todo' si es el único", async () => {
    const unSoloDesactualizadoExcluido = {
      ...SNAPSHOT,
      packages: SNAPSHOT.packages.map((p) => ({
        ...p,
        outdated: p.name === "hunkdiff",
      })),
    };
    const store = createPackagesStore(async (cmd) => {
      if (cmd === "list_globals") return unSoloDesactualizadoExcluido;
      if (cmd === "get_excluded") return ["hunkdiff"];
    });
    await store.cargarExclusiones();
    await store.refresh();
    expect(store.hayDesactualizados.value).toBe(false);
  });

  it("toggleExcluded revierte el estado si falla el guardado", async () => {
    const store = createPackagesStore(async (cmd) => {
      if (cmd === "get_excluded") return [];
      if (cmd === "set_excluded") throw "disco lleno";
    });
    await store.cargarExclusiones();
    await store.toggleExcluded("hunkdiff");
    expect(store.isExcluded("hunkdiff")).toBe(false); // revertido
    expect(store.logs.value.some((l) => l.includes("exclusiones"))).toBe(true);
  });

  it("excluir a mitad de cola salta el paquete ya encolado", async () => {
    const actualizados = [];
    let store;
    let primera = true;
    store = createPackagesStore(async (cmd, args) => {
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "get_excluded") return [];
      if (cmd === "set_excluded") return {};
      if (cmd === "update_package") {
        if (primera) {
          primera = false;
          store.toggleExcluded("hunkdiff"); // excluye mientras corre el 1º
        }
        actualizados.push(args.name);
        return { success: true };
      }
    });
    await store.cargarExclusiones();
    await store.refresh();
    await store.updateAll();
    expect(actualizados).toEqual(["context-mode"]); // hunkdiff saltado
    expect(store.queue.summary).toEqual({
      total: 2,
      ok: 1,
      failed: 0,
      detenida: false, // saltarse no es detenerse
    });
  });
});
