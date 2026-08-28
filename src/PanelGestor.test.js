// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { tauri } from "./tauri-fake"; // the mocks register in tests-setup
import PanelGestor from "./PanelGestor.vue";
import { crearLog } from "./store";

// The panel exercises the real Vue↔Tauri flow through the established
// seam: a fake bridge (tauri-fake) instead of the runtime.

const SNAPSHOT = {
  version_gestor: "11.4.2",
  version_node: "26.2.0",
  comando_actualizar: "npm i -g",
  packages: [
    {
      name: "@alibaba-group/open-code-review",
      installed: "1.2.0",
      latest: "1.2.0",
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

function montar(gestor = "npm") {
  return mount(PanelGestor, { props: { gestor, log: crearLog() } });
}

async function montarCargado() {
  tauri.responder("get_excluded", { estado: "ok", nombres: [] });
  tauri.responder("list_globals", SNAPSHOT);
  const c = montar();
  await flushPromises();
  return c;
}

const filaDe = (c, nombre) =>
  c.findAll("tbody tr").find((r) => r.text().includes(nombre));

beforeEach(() => tauri.reiniciar());

describe("PanelGestor montado", () => {
  it("muestra la tabla del espacio global del gestor", async () => {
    const c = await montarCargado();
    expect(c.findAll("tbody tr")).toHaveLength(3);
    expect(c.get(".statusbar").text()).toContain("npm v11.4.2");
  });

  it("los eventos pm-cola mueven las fila por los cuatro motivos", async () => {
    const c = await montarCargado();
    const fila = filaDe(c, "hunkdiff");
    // empieza → the row shows the updating state
    tauri.emitir("pm-cola", {
      gestor: "npm",
      tipo: "empieza",
      paquete: "hunkdiff",
    });
    await flushPromises();
    expect(fila.text()).toContain("updating");
    // fallo → error with the manager's output
    tauri.emitir("pm-cola", {
      gestor: "npm",
      tipo: "resultado",
      paquete: "hunkdiff",
      motivo: "fallo",
      salida: "EACCES",
    });
    await flushPromises();
    expect(fila.classes()).toContain("error");
    // plazo → error with its own message (the row's tooltip)
    tauri.emitir("pm-cola", {
      gestor: "npm",
      tipo: "empieza",
      paquete: "hunkdiff",
    });
    tauri.emitir("pm-cola", {
      gestor: "npm",
      tipo: "resultado",
      paquete: "hunkdiff",
      motivo: "plazo",
      salida: "npm no respondió en 300 s",
    });
    await flushPromises();
    expect(fila.classes()).toContain("error");
    expect(fila.attributes("title")).toContain("did not respond in time");
    // detenido → a user decision: the row goes back to normal
    tauri.emitir("pm-cola", {
      gestor: "npm",
      tipo: "empieza",
      paquete: "hunkdiff",
    });
    tauri.emitir("pm-cola", {
      gestor: "npm",
      tipo: "resultado",
      paquete: "hunkdiff",
      motivo: "detenido",
    });
    await flushPromises();
    expect(fila.classes()).not.toContain("error");
    expect(fila.attributes("title")).toBeUndefined();
  });

  it("quitar la exclusión usa quitar_exclusion y la fila pierde el estado", async () => {
    tauri.responder("get_excluded", { estado: "ok", nombres: ["hunkdiff"] });
    tauri.responder("list_globals", SNAPSHOT);
    tauri.responder("quitar_exclusion", {});
    const c = montar();
    await flushPromises();
    const fila = filaDe(c, "hunkdiff");
    expect(fila.classes()).toContain("excluido");
    await fila.get("button.excluir").trigger("click");
    await flushPromises();
    expect(tauri.ultima("quitar_exclusion").args).toEqual({
      gestor: "npm",
      paquete: "hunkdiff",
    });
    expect(fila.classes()).not.toContain("excluido");
  });

  it("accesibilidad: caption del gestor, aria-busy durante la cola y regiones live", async () => {
    tauri.responder("detener_actualizar_todo", {});
    tauri.responder("actualizar_todo", () => new Promise(() => {})); // hangs
    const c = await montarCargado();
    // the table announces what it holds
    expect(c.get("caption").text()).toContain("npm");
    expect(c.get(".tabla-scroll").attributes("aria-busy")).toBe("false");
    await c.get("button.primario").trigger("click");
    await flushPromises();
    expect(c.get(".tabla-scroll").attributes("aria-busy")).toBe("true");
    // a package's start announces politely; a failure alerts
    tauri.emitir("pm-cola", {
      gestor: "npm",
      tipo: "empieza",
      paquete: "hunkdiff",
    });
    await flushPromises();
    expect(c.get('[aria-live="polite"]').text()).toContain("hunkdiff");
    tauri.emitir("pm-cola", {
      gestor: "npm",
      tipo: "resultado",
      paquete: "hunkdiff",
      motivo: "fallo",
      salida: "EACCES",
    });
    await flushPromises();
    const alertas = c.findAll('[role="alert"]');
    expect(alertas.some((a) => a.text().includes("hunkdiff"))).toBe(true);
  });

  it("copiar diagnóstico arma el bloque redactado y lo pone en el portapapeles", async () => {
    const escritura = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: escritura },
      configurable: true,
    });
    tauri.responder("diagnostico", {
      version: "0.3.3",
      so: "macos (aarch64)",
      gestores: ["npm", "pnpm"],
      home: "/Users/sadot",
    });
    const c = await montarCargado();
    await c.get("button.copiar").trigger("click");
    await flushPromises();
    expect(escritura).toHaveBeenCalledTimes(1);
    const texto = escritura.mock.calls[0][0];
    expect(texto).toContain("nuupa v0.3.3");
    expect(texto).toContain("gestores: npm, pnpm");
    expect(texto).not.toContain("/Users/sadot"); // redacted on the way out
    // visible + announced confirmation
    expect(c.get("button.copiar").text()).toContain("copied");
    expect(c.get('[aria-live="polite"]').text()).toContain("clipboard");
  });

  it("el cambio de gestor abandona la cola saliente y filtra eventos por gestor", async () => {
    tauri.responder("abandonar_actualizar_todo", {});
    tauri.responder("actualizar_todo", () => new Promise(() => {})); // hangs
    const npm = await montarCargado();
    await npm.get("button.primario").trigger("click");
    await flushPromises();
    npm.unmount(); // tab switch: graceful abandonment
    expect(tauri.registradas("abandonar_actualizar_todo")).toHaveLength(1);

    // the incoming panel only reacts to ITS OWN gestor's events
    tauri.responder("get_excluded", { estado: "ok", nombres: [] });
    const pnpm = montar("pnpm");
    await flushPromises();
    const fila = filaDe(pnpm, "hunkdiff");
    tauri.emitir("pm-cola", {
      gestor: "npm",
      tipo: "empieza",
      paquete: "hunkdiff",
    });
    await flushPromises();
    expect(fila.text()).not.toContain("updating"); // foreign gestor
    tauri.emitir("pm-cola", {
      gestor: "pnpm",
      tipo: "empieza",
      paquete: "hunkdiff",
    });
    await flushPromises();
    expect(fila.text()).toContain("updating"); // its own
  });

  it("el toggle de Excluido pide el comando granular y se deshabilita solo en esa fila", async () => {
    let resolver;
    tauri.responder(
      "excluir_paquete",
      () => new Promise((r) => (resolver = r)),
    );
    const c = await montarCargado();
    const fila = filaDe(c, "hunkdiff");
    const boton = fila.get("button.excluir");
    await boton.trigger("click");
    await flushPromises();
    // granular: (gestor, paquete) — never a full list
    expect(tauri.ultima("excluir_paquete").args).toEqual({
      gestor: "npm",
      paquete: "hunkdiff",
    });
    // in flight: THAT row disabled, the neighbor operable
    expect(boton.attributes("disabled")).toBeDefined();
    const otra = filaDe(c, "context-mode").get("button.excluir");
    expect(otra.attributes("disabled")).toBeUndefined();
    resolver({});
    await flushPromises();
    expect(boton.attributes("disabled")).toBeUndefined();
    expect(fila.classes()).toContain("excluido");
  });

  it("Detener pide el corte y el desmonte pide el abandono suave", async () => {
    tauri.responder("detener_actualizar_todo", {});
    tauri.responder("abandonar_actualizar_todo", {});
    tauri.responder("actualizar_todo", () => new Promise(() => {})); // hangs
    const c = await montarCargado();
    await c.get("button.primario").trigger("click");
    await flushPromises();
    // the queue is active → the Stop button exists and CUTS
    await c.get("button.detener").trigger("click");
    expect(tauri.registradas("detener_actualizar_todo")).toHaveLength(1);
    // leaving the panel (tab switch): graceful, nothing cut
    c.unmount();
    expect(tauri.registradas("abandonar_actualizar_todo")).toHaveLength(1);
  });
});
