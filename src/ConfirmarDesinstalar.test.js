// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import ConfirmarDesinstalar from "./ConfirmarDesinstalar.vue";

// The Desinstalar confirmation (#44): gestor + nombre, cancelable without
// consequence, and the a11y pattern of the About dialog (#20) — focus
// in, trap, Esc/overlay/Cancel out, focus back to the trigger.

const montar = () =>
  mount(ConfirmarDesinstalar, {
    props: { gestor: "npm", paquete: "hunkdiff" },
    attachTo: document.body,
  });

describe("ConfirmarDesinstalar", () => {
  it("muestra el gestor y el nombre con diálogo modal accesible", () => {
    const c = montar();
    const dialogo = c.get('[role="dialog"]');
    expect(dialogo.attributes("aria-modal")).toBe("true");
    expect(dialogo.attributes("aria-label")).toBeTruthy();
    expect(dialogo.text()).toContain("npm");
    expect(dialogo.text()).toContain("hunkdiff");
    c.unmount();
  });

  it("el foco entra en Cancelar: la salida segura, no la destructiva", async () => {
    const c = montar();
    await flushPromises();
    expect(document.activeElement?.textContent).toContain("Cancel");
    c.unmount();
  });

  it("Cancelar, Esc y el velo emiten cerrar sin confirmar", async () => {
    const c = montar();
    await c.get("button.cancelar").trigger("click");
    await c.vm.$nextTick();
    let eventos = c.emitted("cerrar");
    expect(eventos).toHaveLength(1);
    expect(c.emitted("confirmar")).toBeUndefined();
    c.unmount();

    const c2 = montar();
    await c2.get(".dialogo").trigger("keydown", { key: "Escape" });
    expect(c2.emitted("cerrar")).toHaveLength(1);
    c2.unmount();

    const c3 = montar();
    await c3.get(".velo").trigger("click");
    expect(c3.emitted("cerrar")).toHaveLength(1);
    expect(c3.emitted("confirmar")).toBeUndefined();
    c3.unmount();
  });

  it("Desinstalar emite confirmar (la acción vive en el panel)", async () => {
    const c = montar();
    await c.get("button.peligro").trigger("click");
    expect(c.emitted("confirmar")).toHaveLength(1);
    expect(c.emitted("cerrar")).toBeUndefined();
    c.unmount();
  });

  it("Tab cicla dentro del diálogo y el foco vuelve a quien lo abrió", async () => {
    const disparador = document.createElement("button");
    disparador.textContent = "papelera";
    document.body.appendChild(disparador);
    disparador.focus();

    const c = montar();
    const ultimo = c.get("button.peligro").element;
    ultimo.focus();
    await c.get(".dialogo").trigger("keydown", { key: "Tab" });
    expect(document.activeElement).toBe(c.get("button.cancelar").element);

    c.unmount();
    expect(document.activeElement).toBe(disparador);
    disparador.remove();
  });
});
