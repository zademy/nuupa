// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
// The fake bridge MUST be imported BEFORE the components: its vi.mock
// registrations have to run before they resolve the real Tauri modules.
import { tauri } from "./tauri-fake";
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

  it("los eventos pm-cola mueven las filas: empieza y resultado", async () => {
    const c = await montarCargado();
    const fila = filaDe(c, "hunkdiff");
    tauri.emitir("pm-cola", {
      gestor: "npm",
      tipo: "empieza",
      paquete: "hunkdiff",
    });
    await flushPromises();
    expect(fila.text()).toContain("updating");
    tauri.emitir("pm-cola", {
      gestor: "npm",
      tipo: "resultado",
      paquete: "hunkdiff",
      motivo: "fallo",
      salida: "EACCES",
    });
    await flushPromises();
    expect(fila.classes()).toContain("error");
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
