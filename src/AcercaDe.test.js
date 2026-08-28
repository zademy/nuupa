// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
// The fake bridge MUST be imported BEFORE the components (see its file).
import { tauri } from "./tauri-fake";
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
});
