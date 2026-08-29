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
    { nombre: "invalida-skill", estado: "invalida" },
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
    expect(filaDe(c, "invalida-skill").text()).toContain("invalid");
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
    expect(filaDe(c, "invalida-skill").classes()).not.toContain("error");
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
    await c.get("input[type=search]").setValue("INVALID");
    expect(c.findAll("tbody tr")).toHaveLength(1);
    expect(c.findAll("tbody tr")[0].text()).toContain("invalida-skill");
  });

  it("accesibilidad: caption, aria-busy y regiones live", async () => {
    const c = await montarCargado();
    expect(c.get("caption").text()).toContain("skills");
    expect(c.get(".tabla-scroll").attributes("aria-busy")).toBe("false");
    expect(c.get('[aria-live="polite"]').exists()).toBe(true);
    expect(c.get('[role="alert"]').exists()).toBe(true);
  });

  // ---- #27: agregar desde origen ----

  const ITEMS = [
    { ruta: "skills/productivity/buena", conforme: true },
    {
      ruta: "skills/productivity/invalida",
      conforme: false,
      motivo: "sin frontmatter",
    },
  ];

  it("agregar: escanea, preselecciona conformes e instala solo la selección", async () => {
    tauri.responder("listar_habilidades", LISTA);
    tauri.responder("escanear_origen", ITEMS);
    tauri.responder("instalar_habilidades", [
      { ruta: "skills/productivity/buena", nombre: "buena", ok: true },
    ]);
    const c = montar();
    await flushPromises();
    await c.get("input[type=text]").setValue("github.com/o/r");
    await c.get("button.primario").trigger("click");
    await flushPromises();
    const seccion = c.get(".escaneo");
    const checks = seccion.findAll("input[type=checkbox]");
    expect(checks).toHaveLength(2);
    expect(checks[0].element.checked).toBe(true); // conforme preselected
    expect(checks[1].element.disabled).toBe(true); // invalida: never
    expect(seccion.text()).toContain("sin frontmatter");
    await seccion.get("button.primario").trigger("click");
    await flushPromises();
    expect(tauri.ultima("instalar_habilidades").args).toEqual({
      origen: "github.com/o/r",
      rutas: ["skills/productivity/buena"],
    });
    expect(c.find(".escaneo").exists()).toBe(false); // closes after install
  });

  it("cancelar cierra la sección del escaneo sin instalar", async () => {
    tauri.responder("listar_habilidades", LISTA);
    tauri.responder("escanear_origen", ITEMS);
    const c = await montarCargado();
    await c.get("input[type=text]").setValue("github.com/o/r");
    await c.get("button.primario").trigger("click");
    await flushPromises();
    expect(c.find(".escaneo").exists()).toBe(true);
    await c.get("button.cerrar").trigger("click");
    await flushPromises();
    expect(c.find(".escaneo").exists()).toBe(false);
    expect(tauri.registradas("instalar_habilidades")).toHaveLength(0);
  });

  it("un fallo del escaneo vive en la sección y no toca la tabla", async () => {
    tauri.responder("listar_habilidades", LISTA);
    tauri.responder("escanear_origen", () =>
      Promise.reject("repo no encontrado"),
    );
    const c = await montarCargado();
    await c.get("input[type=text]").setValue("github.com/o/r");
    await c.get("button.primario").trigger("click");
    await flushPromises();
    expect(c.get(".escaneo [role=alert]").text()).toContain(
      "repo no encontrado",
    );
    expect(c.findAll("tbody tr")).toHaveLength(2); // the table stays
  });

  // ---- #28: los cuatro estados + Sin verificar por fila ----

  it("refrescar refleja Actual y Actualización disponible por SHA", async () => {
    tauri.responder("listar_habilidades", {
      habilidades: [
        { nombre: "buena", estado: "actual" },
        { nombre: "vieja", estado: "actualizacion_disponible" },
        { nombre: "suelta", estado: "no_gestionada" },
        { nombre: "rota", estado: "invalida" },
      ],
      manifest: { estado: "ok" },
    });
    const c = montar();
    await flushPromises();
    expect(filaDe(c, "buena").text()).toContain("up to date");
    expect(filaDe(c, "vieja").text()).toContain("update available");
    expect(filaDe(c, "suelta").text()).toContain("not managed");
    expect(filaDe(c, "rota").text()).toContain("invalid");
  });

  it("un fallo de red marca SOLO esa fila como Sin verificar con su motivo", async () => {
    tauri.responder("listar_habilidades", {
      habilidades: [
        {
          nombre: "buena",
          estado: "sin_verificar",
          error: "no se pudo contactar a GitHub: sin red",
        },
        { nombre: "otra", estado: "actual" },
      ],
      manifest: { estado: "ok" },
    });
    const c = montar();
    await flushPromises();
    const fila = filaDe(c, "buena");
    expect(fila.text()).toContain("not verified");
    expect(fila.classes()).toContain("error");
    expect(fila.attributes("title")).toContain("sin red");
    // the rest keep their verdicts
    const otra = filaDe(c, "otra");
    expect(otra.text()).toContain("up to date");
    expect(otra.classes()).not.toContain("error");
  });

  // ---- #29: el botón Actualizar ----

  const CON_ACTUALIZACION = {
    habilidades: [
      { nombre: "vieja", estado: "actualizacion_disponible" },
      { nombre: "al-dia", estado: "actual" },
      { nombre: "suelta", estado: "no_gestionada" },
      { nombre: "rota", estado: "invalida" },
    ],
    manifest: { estado: "ok" },
  };

  it("Actualizar solo está habilitado en la Actualización disponible y pide el comando", async () => {
    tauri.responder("listar_habilidades", CON_ACTUALIZACION);
    tauri.responder("actualizar_habilidad", {});
    const c = montar();
    await flushPromises();
    const botonDe = (nombre) => filaDe(c, nombre).get("button.actualizar");
    expect(botonDe("vieja").attributes("disabled")).toBeUndefined();
    for (const nombre of ["al-dia", "suelta", "rota"]) {
      expect(botonDe(nombre).attributes("disabled")).toBeDefined();
    }
    await botonDe("vieja").trigger("click");
    await flushPromises();
    expect(tauri.ultima("actualizar_habilidad").args).toEqual({
      nombre: "vieja",
    });
    // after the update the list refreshed
    expect(tauri.registradas("listar_habilidades")).toHaveLength(2);
  });

  it("un fallo de actualización marca la fila en error con su motivo", async () => {
    tauri.responder("listar_habilidades", CON_ACTUALIZACION);
    tauri.responder("actualizar_habilidad", () =>
      Promise.reject("inválida: sin frontmatter"),
    );
    const c = await montarCargado(CON_ACTUALIZACION);
    await filaDe(c, "vieja").get("button.actualizar").trigger("click");
    await flushPromises();
    const fila = filaDe(c, "vieja");
    expect(fila.classes()).toContain("error");
    expect(fila.attributes("title")).toContain("sin frontmatter");
    // the neighbor was never touched
    expect(filaDe(c, "al-dia").classes()).not.toContain("error");
  });

  // ---- #30: la cola Actualizar todo ----

  it("la cola delega al comando, mueve las filas por eventos y adopta el resumen", async () => {
    tauri.responder("listar_habilidades", CON_ACTUALIZACION);
    tauri.responder("actualizar_habilidades_todo", {
      total: 1,
      ok: 1,
      failed: 0,
      detenidos: 0,
      detenida: false,
    });
    const c = await montarCargado(CON_ACTUALIZACION);
    expect(
      c.get("button.actualizar-todo").attributes("disabled"),
    ).toBeUndefined();
    await c.get("button.actualizar-todo").trigger("click");
    await flushPromises();
    expect(tauri.registradas("actualizar_habilidades_todo")).toHaveLength(1);
    // the queue events move the row while running
    tauri.emitir("skills-cola", {
      tipo: "empieza",
      habilidad: "vieja",
    });
    await flushPromises();
    expect(filaDe(c, "vieja").text()).toContain("updating");
    tauri.emitir("skills-cola", {
      tipo: "resultado",
      habilidad: "vieja",
      motivo: "ok",
    });
    await flushPromises();
    expect(filaDe(c, "vieja").text()).not.toContain("updating");
    // the summary lives in the statusbar
    expect(c.get(".statusbar").text()).toContain("1 of 1");
  });

  it("un resultado fallido de la cola marca la fila con el motivo", async () => {
    tauri.responder("listar_habilidades", CON_ACTUALIZACION);
    tauri.responder("actualizar_habilidades_todo", {
      total: 1,
      ok: 0,
      failed: 1,
      detenidos: 0,
      detenida: false,
    });
    const c = await montarCargado(CON_ACTUALIZACION);
    tauri.emitir("skills-cola", {
      tipo: "resultado",
      habilidad: "vieja",
      motivo: "fallo",
      salida: "no se pudo extraer el repositorio",
    });
    await flushPromises();
    const fila = filaDe(c, "vieja");
    expect(fila.classes()).toContain("error");
    expect(fila.attributes("title")).toContain("no se pudo extraer");
    const alertas = c.findAll('[role="alert"]');
    expect(alertas.some((a) => a.text().includes("vieja"))).toBe(true);
  });

  it("sin actualizables (o manifest corrupto) la cola está deshabilitada", async () => {
    const c = await montarCargado(LISTA); // ninguna Actualización disponible
    expect(
      c.get("button.actualizar-todo").attributes("disabled"),
    ).toBeDefined();
    tauri.responder("listar_habilidades", {
      habilidades: [{ nombre: "vieja", estado: "actualizacion_disponible" }],
      manifest: { estado: "corrupto" },
    });
    await c.get("button.refrescar").trigger("click");
    await flushPromises();
    expect(
      c.get("button.actualizar-todo").attributes("disabled"),
    ).toBeDefined();
  });

  it("Detener pide el corte de la cola de habilidades", async () => {
    tauri.responder("listar_habilidades", CON_ACTUALIZACION);
    tauri.responder("detener_habilidades_todo", {});
    tauri.responder("actualizar_habilidades_todo", () => new Promise(() => {}));
    const c = await montarCargado(CON_ACTUALIZACION);
    await c.get("button.actualizar-todo").trigger("click");
    await flushPromises();
    await c.get("button.detener").trigger("click");
    expect(tauri.registradas("detener_habilidades_todo")).toHaveLength(1);
    c.unmount();
    // leaving with an active queue: graceful abandonment, nothing cut
    expect(tauri.registradas("abandonar_habilidades_todo")).toHaveLength(1);
  });
});
