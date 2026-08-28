import { describe, expect, it } from "vitest";
import { createPackagesStore, crearLog } from "./store";

const SNAPSHOT = {
  version_gestor: "11.4.2",
  version_node: "26.2.0",
  comando_actualizar: "npm i -g",
  packages: [
    {
      name: "@alibaba-group/open-code-review",
      installed: "1.10.2",
      latest: "1.10.2",
      outdated: false,
    },
    {
      name: "context-mode",
      installed: "1.0.169",
      latest: "1.0.170",
      outdated: true,
    },
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
      throw "npm not found";
    });
    await store.refresh();
    expect(store.state.snapshot).toBeNull();
    expect(store.state.error).toContain("npm not found");
    expect(store.state.loading).toBe(false);
  });

  it("un refresco fallido conserva el snapshot viejo y marca el error", async () => {
    let llamadas = 0;
    const store = createPackagesStore(async () => {
      llamadas++;
      if (llamadas === 1) return SNAPSHOT;
      throw "network failure";
    });
    await store.refresh();
    await store.refresh();
    expect(store.state.snapshot).toEqual(SNAPSHOT); // old data persists
    expect(store.state.error).toContain("network failure");
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
      () => new Promise((r) => (resolver = () => r(SNAPSHOT))),
    );
    const carga = store.refresh();
    store.search.value = "hunk"; // typing during the load
    expect(store.state.loading).toBe(true);
    resolver();
    await carga;
    expect(store.state.loading).toBe(false);
    expect(store.packages.value.map((p) => p.name)).toEqual(["hunkdiff"]);
  });

  it("respuestas cruzadas de refresco: gana el más nuevo aunque resuelva antes", async () => {
    const pendientes = [];
    const store = createPackagesStore(
      () => new Promise((r) => pendientes.push(r)),
    );
    const carga1 = store.refresh();
    const carga2 = store.refresh();
    // counter: BOTH in flight → still loading
    expect(store.state.loading).toBe(true);
    const masNuevo = { ...SNAPSHOT, version_gestor: "nueva" };
    pendientes[1](masNuevo); // newest resolves FIRST
    await carga2;
    pendientes[0](SNAPSHOT); // oldest resolves LAST: discarded
    await carga1;
    expect(store.state.snapshot.version_gestor).toBe("nueva");
    expect(store.state.loading).toBe(false);
  });

  it("el error del refresco vigente no lo borra uno descartado exitoso", async () => {
    const pendientes = [];
    const store = createPackagesStore(
      () => new Promise((r, rej) => pendientes.push({ r, rej })),
    );
    const carga1 = store.refresh();
    const carga2 = store.refresh();
    pendientes[1].rej("network failure"); // the current one FAILS
    await carga2;
    expect(store.state.error).toContain("network failure");
    pendientes[0].r(SNAPSHOT); // the old one succeeds, too late
    await carga1;
    expect(store.state.error).toContain("network failure");
    expect(store.state.snapshot).toBeNull();
  });

  it("el snapshot final de la cola pasa por el mismo guard", async () => {
    let consultas = 0;
    let resolverRefresco;
    let resolverCola;
    const store = createPackagesStore(async (cmd) => {
      if (cmd === "list_globals") {
        consultas++;
        if (consultas === 1) return SNAPSHOT; // base: 2 outdated
        return new Promise((r) => (resolverRefresco = r));
      }
      if (cmd === "actualizar_todo")
        return new Promise((r) => (resolverCola = r));
    });
    await store.refresh(); // id 1
    const refresco = store.refresh(); // id 2, hangs
    const cola = store.updateAll(); // id 3: the current one
    resolverCola({
      resumen: { total: 2, ok: 2, failed: 0, detenidos: 0, detenida: false },
      snapshot: { ...SNAPSHOT, version_gestor: "cola" },
    });
    await cola; // publishes (current)
    resolverRefresco({ ...SNAPSHOT, version_gestor: "refresco" }); // stale
    await refresco;
    expect(store.state.snapshot.version_gestor).toBe("cola");
  });

  it("caso inverso: el refresco más nuevo publica y la cola descarta su snapshot", async () => {
    let consultas = 0;
    let resolverCola;
    const store = createPackagesStore(async (cmd) => {
      if (cmd === "list_globals") {
        consultas++;
        return consultas === 1
          ? SNAPSHOT
          : { ...SNAPSHOT, version_gestor: "refresco" };
      }
      if (cmd === "actualizar_todo")
        return new Promise((r) => (resolverCola = r));
    });
    await store.refresh(); // base: 2 outdated
    const cola = store.updateAll(); // id 2, hangs
    await store.refresh(); // id 3: resolves FIRST, publishes
    expect(store.state.snapshot.version_gestor).toBe("refresco");
    resolverCola({
      resumen: { total: 2, ok: 2, failed: 0, detenidos: 0, detenida: false },
      snapshot: { ...SNAPSHOT, version_gestor: "cola" },
    });
    await cola; // stale: discarded
    expect(store.state.snapshot.version_gestor).toBe("refresco");
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
      throw new Error(`unexpected command: ${cmd}`);
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
      throw new Error(`unexpected command: ${cmd}`);
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
      if (cmd === "update_package") throw "spawn failed";
    });
    await store.refresh();
    await store.update("context-mode");
    expect(store.hasError("context-mode")).toBe(true);
  });

  it("la store pasa su gestor en cada invocación", async () => {
    const vistos = [];
    const store = createPackagesStore(async (cmd, args) => {
      vistos.push(cmd);
      if (args?.gestor !== "pnpm") throw new Error(`wrong manager in ${cmd}`);
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
      "list_globals", // refresh after the successful update
      "set_excluded",
    ]);
  });

  it("dos stores con gestores distintos aíslan su estado", async () => {
    const excluidosPorGestor = { npm: ["hunkdiff"], pnpm: [] };
    const fakePorGestor = (gestor) => async (cmd, args) => {
      if (args?.gestor !== gestor)
        throw new Error(`crossed manager: ${args?.gestor}`);
      if (cmd === "get_excluded") return excluidosPorGestor[gestor];
      if (cmd === "list_globals") return SNAPSHOT;
      throw new Error(`unexpected command: ${cmd}`);
    };
    const storeNpm = createPackagesStore(fakePorGestor("npm"), "npm");
    const storePnpm = createPackagesStore(fakePorGestor("pnpm"), "pnpm");
    await storeNpm.cargarExclusiones();
    await storePnpm.cargarExclusiones();
    // each asked for ITS OWN (the fakes blow up if managers cross)
    expect(storeNpm.isExcluded("hunkdiff")).toBe(true);
    expect(storePnpm.isExcluded("hunkdiff")).toBe(false);
  });

  it("gestor no soportado produce error visible en la store", async () => {
    const store = createPackagesStore(async (cmd, args) => {
      if (args?.gestor === "yarn") throw "unsupported manager: yarn";
      return SNAPSHOT;
    }, "yarn");
    await store.refresh();
    expect(store.state.snapshot).toBeNull();
    expect(store.state.error).toContain("unsupported manager");
  });

  it("crearLog.appendLine acumula líneas con prefijo gestor/paquete", () => {
    const log = crearLog();
    log.appendLine("npm", "hunkdiff", "added 1 package");
    log.appendLine("npm", "hunkdiff", "done");
    expect(log.lineas.value).toEqual([
      "npm/hunkdiff: added 1 package",
      "npm/hunkdiff: done",
    ]);
  });

  it("dos stores comparten un log con líneas de ambos gestores", async () => {
    const log = crearLog();
    const storeNpm = createPackagesStore(
      async (cmd) => (cmd === "list_globals" ? SNAPSHOT : []),
      "npm",
      log,
    );
    const storeBun = createPackagesStore(
      async (cmd) => (cmd === "list_globals" ? SNAPSHOT : []),
      "bun",
      log,
    );
    log.appendLine("npm", "hunkdiff", "added 1 package");
    log.appendLine("bun", "headroom-ai", "installed");
    // single history: both stores see the same lines
    expect(storeNpm.logs.value).toEqual([
      "npm/hunkdiff: added 1 package",
      "bun/headroom-ai: installed",
    ]);
    expect(storeBun.logs.value).toBe(storeNpm.logs.value);
  });

  it("el log compartido respeta el tope de líneas", () => {
    const log = crearLog(3);
    const store = createPackagesStore(fakeInvoke(), "npm", log);
    for (let i = 0; i < 5; i++) log.appendLine("npm", "p", `line ${i}`);
    expect(store.logs.value).toHaveLength(3);
    expect(store.logs.value[0]).toBe("npm/p: line 2");
  });

  it("updateAll delega la cola a actualizar_todo y adopta resumen y snapshot", async () => {
    const llamadas = [];
    const store = createPackagesStore(async (cmd, args) => {
      llamadas.push(cmd);
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "actualizar_todo") {
        expect(args).toEqual({ gestor: "npm" });
        return {
          resumen: {
            total: 2,
            ok: 2,
            failed: 0,
            detenidos: 0,
            detenida: false,
          },
          snapshot: SNAPSHOT,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await store.refresh();
    await store.updateAll();
    // a single invoke for the whole queue; the final snapshot comes with it
    expect(llamadas).toEqual(["list_globals", "actualizar_todo"]);
    expect(store.queue.summary).toEqual({
      total: 2,
      ok: 2,
      failed: 0,
      detenidos: 0,
      detenida: false,
    });
    expect(store.queue.active).toBe(false);
    expect(store.state.snapshot).toEqual(SNAPSHOT);
    // the summary also lives in the log (visible even if you switch tabs)
    expect(
      store.logs.value.some((l) =>
        l.includes("queue finished — 2 of 2 updated"),
      ),
    ).toBe(true);
  });

  it("los eventos de la cola mueven las filas: empieza y resultado", async () => {
    let store;
    store = createPackagesStore(async (cmd) => {
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "actualizar_todo") {
        store.procesarEventoCola({
          gestor: "npm",
          tipo: "empieza",
          paquete: "context-mode",
        });
        store.procesarEventoCola({
          gestor: "npm",
          tipo: "resultado",
          paquete: "context-mode",
          motivo: "ok",
        });
        store.procesarEventoCola({
          gestor: "npm",
          tipo: "empieza",
          paquete: "hunkdiff",
        });
        store.procesarEventoCola({
          gestor: "npm",
          tipo: "resultado",
          paquete: "hunkdiff",
          motivo: "fallo",
          salida: "EACCES",
        });
        return {
          resumen: { total: 2, ok: 1, failed: 1, detenida: false },
          snapshot: SNAPSHOT,
        };
      }
    });
    await store.refresh();
    await store.updateAll();
    expect(store.hasError("hunkdiff")).toBe(true); // failed result
    expect(store.isUpdating("context-mode")).toBe(false); // success clears
    expect(store.queue.current).toBeNull();
    expect(
      store.logs.value.some((l) =>
        l.includes("npm/hunkdiff: the update failed"),
      ),
    ).toBe(true);
  });

  it("los eventos de OTRO gestor no tocan esta store", async () => {
    const store = createPackagesStore(fakeInvoke());
    store.procesarEventoCola({
      gestor: "pnpm",
      tipo: "empieza",
      paquete: "cowsay",
    });
    expect(store.isUpdating("cowsay")).toBe(false);
    expect(store.queue.current).toBeNull();
  });

  it("resultado de cola con motivo timeout marca la fila con el texto de plazo vencido", async () => {
    const store = createPackagesStore(fakeInvoke());
    await store.refresh();
    store.procesarEventoCola({
      gestor: "npm",
      tipo: "empieza",
      paquete: "hunkdiff",
    });
    store.procesarEventoCola({
      gestor: "npm",
      tipo: "resultado",
      paquete: "hunkdiff",
      motivo: "plazo",
      salida: "npm no respondió en 300 s (proceso finalizado)",
    });
    expect(store.hasError("hunkdiff")).toBe(true);
    expect(
      store.logs.value.some((l) => l.includes("did not respond in time")),
    ).toBe(true);
  });

  it("el detalle del fallo queda en la fila (tooltip) y se limpia al reintentar con éxito", async () => {
    const store = createPackagesStore(fakeInvoke());
    await store.refresh();
    store.procesarEventoCola({
      gestor: "npm",
      tipo: "resultado",
      paquete: "hunkdiff",
      motivo: "plazo",
      salida: "npm no respondió en 300 s (proceso finalizado)",
    });
    expect(store.detalleFallo("hunkdiff")).toContain("did not respond in time");
    expect(store.detalleFallo("hunkdiff")).toContain("npm no respondió");
    store.procesarEventoCola({
      gestor: "npm",
      tipo: "resultado",
      paquete: "hunkdiff",
      motivo: "ok",
    });
    expect(store.hasError("hunkdiff")).toBe(false);
    expect(store.detalleFallo("hunkdiff")).toBeUndefined();
  });

  it("update que lanza excepción usa el prefijo de fallo igual que la cola", async () => {
    const store = createPackagesStore(async (cmd) => {
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "update_package") throw "npm no respondió en 300 s";
    });
    await store.refresh();
    await store.update("context-mode");
    expect(store.detalleFallo("context-mode")).toContain("the update failed");
    expect(store.detalleFallo("context-mode")).toContain("npm no respondió");
  });

  it("updateAll con fallo de comando deja el log y refresca", async () => {
    const llamadas = [];
    const store = createPackagesStore(async (cmd) => {
      llamadas.push(cmd);
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "actualizar_todo") throw "spawn failed";
    });
    await store.refresh();
    await store.updateAll();
    expect(llamadas).toEqual([
      "list_globals",
      "actualizar_todo",
      "list_globals",
    ]);
    expect(store.queue.active).toBe(false);
    expect(store.queue.summary).toBeNull();
    expect(store.logs.value.some((l) => l.includes("the queue failed"))).toBe(
      true,
    );
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

  it("updateAll salta a los excluidos: la cola del backend los filtra", async () => {
    const store = createPackagesStore(async (cmd, args) => {
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "get_excluded") return ["hunkdiff"];
      if (cmd === "actualizar_todo") {
        // the backend builds the queue without hunkdiff: it shows in the summary
        expect(args).toEqual({ gestor: "npm" });
        return {
          resumen: { total: 1, ok: 1, failed: 0, detenida: false },
          snapshot: SNAPSHOT,
        };
      }
    });
    await store.cargarExclusiones();
    await store.refresh();
    await store.updateAll();
    expect(store.queue.summary.total).toBe(1);
  });

  it("Detener pide el alto al backend y muestra el estado deteniendo", async () => {
    const llamadas = [];
    let resolver;
    const store = createPackagesStore(async (cmd) => {
      llamadas.push(cmd);
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "actualizar_todo")
        return new Promise(
          (r) =>
            (resolver = () =>
              r({
                resumen: {
                  total: 2,
                  ok: 1,
                  failed: 0,
                  detenidos: 0,
                  detenida: true,
                },
                snapshot: SNAPSHOT,
              })),
        );
      if (cmd === "detener_actualizar_todo") return {};
    });
    await store.refresh();
    const cola = store.updateAll();
    store.stopAll();
    expect(llamadas).toContain("detener_actualizar_todo");
    expect(store.queue.stopped).toBe(true); // immediate feedback for Stop
    resolver();
    await cola;
    expect(store.queue.summary.detenida).toBe(true); // the backend confirms
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
    store.toggleExcluded("hunkdiff"); // adds
    store.toggleExcluded("context-mode"); // removes
    expect(store.isExcluded("hunkdiff")).toBe(true);
    expect(store.isExcluded("context-mode")).toBe(false);
    expect(guardado).toEqual([["context-mode", "hunkdiff"], ["hunkdiff"]]);
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
      if (cmd === "set_excluded") throw "disk full";
    });
    await store.cargarExclusiones();
    await store.toggleExcluded("hunkdiff");
    expect(store.isExcluded("hunkdiff")).toBe(false); // reverted
    expect(store.logs.value.some((l) => l.includes("exclusions"))).toBe(true);
  });

  it("resultado detenido limpia la fila sin marcar error y el resumen cuenta detenidos", async () => {
    const store = createPackagesStore(async (cmd) => {
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "actualizar_todo") {
        store.procesarEventoCola({
          gestor: "npm",
          tipo: "empieza",
          paquete: "hunkdiff",
        });
        store.procesarEventoCola({
          gestor: "npm",
          tipo: "resultado",
          paquete: "hunkdiff",
          motivo: "detenido",
          salida: "detenido a pedido (proceso finalizado)",
        });
        return {
          resumen: {
            total: 2,
            ok: 0,
            failed: 0,
            detenidos: 1,
            detenida: true,
          },
          snapshot: SNAPSHOT,
        };
      }
    });
    await store.refresh();
    await store.updateAll();
    // a user decision is not an error: the row just goes back to normal
    expect(store.hasError("hunkdiff")).toBe(false);
    expect(store.isUpdating("hunkdiff")).toBe(false);
    expect(store.queue.summary.detenidos).toBe(1);
    expect(store.logs.value.some((l) => l.includes("was stopped"))).toBe(true);
  });

  it("abandonarCola (desmonte) pide el alto suave sin marcar deteniendo", async () => {
    const llamadas = [];
    let resolver;
    const store = createPackagesStore(async (cmd) => {
      llamadas.push(cmd);
      if (cmd === "list_globals") return SNAPSHOT;
      if (cmd === "actualizar_todo") return new Promise((r) => (resolver = r));
      if (cmd === "abandonar_actualizar_todo") return {};
    });
    await store.refresh();
    const cola = store.updateAll();
    store.abandonarCola();
    expect(llamadas).toContain("abandonar_actualizar_todo");
    // graceful: no "stopping" feedback (the panel is going away)
    expect(store.queue.stopped).toBe(false);
    resolver({
      resumen: { total: 2, ok: 1, failed: 0, detenidos: 0, detenida: true },
      snapshot: SNAPSHOT,
    });
    await cola;
    expect(store.queue.active).toBe(false);
  });
});
