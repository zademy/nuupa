// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { tauri } from "./tauri-fake"; // the mocks register in tests-setup
import AcercaDe from "./AcercaDe.vue";

beforeEach(() => tauri.reiniciar());

describe("AcercaDe", () => {
  it("muestra el diálogo con la versión del runtime", async () => {
    const c = mount(AcercaDe);
    await flushPromises(); // getVersion arrives async
    expect(c.get('[role="dialog"]').exists()).toBe(true);
    expect(c.text()).toContain("v0.0.0-test");
  });

  it("cierra con el botón y con Esc (la visibilidad la decide el padre)", async () => {
    const c = mount(AcercaDe);
    await c.get("button.cerrar").trigger("click");
    expect(c.emitted("cerrar")).toHaveLength(1);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(c.emitted("cerrar")).toHaveLength(2);
  });

  it("atrapa el foco dentro del diálogo: Tab cicla sin escapar", async () => {
    const c = mount(AcercaDe, { attachTo: document.body });
    await flushPromises();
    const focos = [c.get("button.cerrar"), ...c.findAll("a")];
    const primero = focos[0].element;
    const ultimo = focos[focos.length - 1].element;
    ultimo.focus();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab" }));
    expect(document.activeElement).toBe(primero); // wrap: last → first
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Tab", shiftKey: true }),
    );
    expect(document.activeElement).toBe(ultimo); // back: first → last
  });

  it("restaura el foco al botón que abrió el diálogo", async () => {
    const disparador = document.createElement("button");
    document.body.appendChild(disparador);
    disparador.focus();
    const c = mount(AcercaDe, { attachTo: document.body });
    await flushPromises();
    expect(document.activeElement).not.toBe(disparador); // inside
    c.unmount();
    expect(document.activeElement).toBe(disparador); // back home
    disparador.remove();
  });
});
