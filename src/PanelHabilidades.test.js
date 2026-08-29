// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { tauri } from "./tauri-fake"; // the mocks register in tests-setup
import PanelHabilidades from "./PanelHabilidades.vue";
import { crearLog } from "./store";

// The panel exercises the real Vue↔Tauri flow through the established
// seam: a fake bridge (tauri-fake) instead of the runtime.

const LISTA = {
  habilidades: [
    { nombre: "markdownlint", estado: "no_gestionada" },
    { nombre: "rota-skill", estado: "invalida" },
  ],
  manifest: { estado: "ok" },
};

function montar() {
  return mount(PanelHabilidades, { props: { log: crearLog() } });
}

async function montarCargado(lista = LISTA) {
  tauri.responder("listar_habilidades", lista);
  const c = montar();
  await flushPromises();
  return c;
}

const filaDe = (c, nombre) =>
  c.findAll("tbody tr").find((r) => r.text().includes(nombre));

beforeEach(() => tauri.reiniciar());

describe("PanelHabilidades montado", () => {
  it("muestra la tabla con el estado de cada habilidad", async () => {
    const c = await montarCargado();
    expect(c.findAll("tbody tr")).toHaveLength(2);
    expect(filaDe(c, "markdownlint").text()).toContain("not managed");
    expect(filaDe(c, "rota-skill").text()).toContain("invalid");
    expect(c.get(".statusbar").text()).toContain("2");
  });

  it("carpeta sin habilidades muestra el estado vacío con la llamada a la acción", async () => {
    const c = await montarCargado({
      habilidades: [],
      manifest: { estado: "ok" },
    });
    expect(c.find("table").exists()).toBe(false);
    expect(c.get(".vacio").text()).toContain("no skills");
  });

  it("el botón Refrescar vuelve a escanear la carpeta", async () => {
    const c = await montarCargado();
    await c.get("button.refrescar").trigger("click");
    await flushPromises();
    expect(tauri.registradas("listar_habilidades")).toHaveLength(2);
  });

  it("Abrir carpeta pide el comando con el nombre de la habilidad", async () => {
    tauri.responder("abrir_habilidad", {});
    const c = await montarCargado();
    await filaDe(c, "markdownlint").get("button.abrir").trigger("click");
    await flushPromises();
    expect(tauri.ultima("abrir_habilidad").args).toEqual({
      nombre: "markdownlint",
    });
  });

  it("un fallo al abrir marca SOLO esa fila en error", async () => {
    tauri.responder("abrir_habilidad", () => Promise.reject("no such folder"));
    const c = await montarCargado();
    await filaDe(c, "markdownlint").get("button.abrir").trigger("click");
    await flushPromises();
    expect(filaDe(c, "markdownlint").classes()).toContain("error");
    expect(filaDe(c, "rota-skill").classes()).not.toContain("error");
  });

  it("manifest corrupto: banner de emergencia, reintentar y empezar de cero resuelven", async () => {
    const c = await montarCargado({
      habilidades: [],
      manifest: { estado: "corrupto" },
    });
    expect(c.get(".emergencia").text()).toContain("damaged");
    // Reintentar scans again: the re-scan comes back clean
    tauri.responder("listar_habilidades", LISTA);
    await c.findAll(".emergencia button")[0].trigger("click");
    await flushPromises();
    expect(c.find(".emergencia").exists()).toBe(false);

    // corrupt again: Start clean asks the command and re-scans
    tauri.responder("listar_habilidades", {
      habilidades: [],
      manifest: { estado: "corrupto" },
    });
    await c.get("button.refrescar").trigger("click");
    await flushPromises();
    expect(c.get(".emergencia").text()).toContain("damaged");
    tauri.responder("habilidades_de_cero", {});
    tauri.responder("listar_habilidades", LISTA);
    await c.get("button.emergencia-de-cero").trigger("click");
    await flushPromises();
    expect(tauri.registradas("habilidades_de_cero")).toHaveLength(1);
    expect(c.find(".emergencia").exists()).toBe(false);
  });

  it("manifest ilegible muestra el detalle y no ofrece empezar de cero", async () => {
    const c = await montarCargado({
      habilidades: [],
      manifest: { estado: "ilegible", detalle: "Permission denied" },
    });
    expect(c.get(".emergencia").text()).toContain("Permission denied");
    expect(c.find("button.emergencia-de-cero").exists()).toBe(false);
  });

  it("la búsqueda filtra las filas", async () => {
    const c = await montarCargado();
    await c.get("input[type=search]").setValue("ROTA");
    expect(c.findAll("tbody tr")).toHaveLength(1);
    expect(c.findAll("tbody tr")[0].text()).toContain("rota-skill");
  });

  it("accesibilidad: caption, aria-busy y regiones live", async () => {
    const c = await montarCargado();
    expect(c.get("caption").text()).toContain("skills");
    expect(c.get(".tabla-scroll").attributes("aria-busy")).toBe("false");
    expect(c.get('[aria-live="polite"]').exists()).toBe(true);
    expect(c.get('[role="alert"]').exists()).toBe(true);
  });
});
