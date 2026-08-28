// @vitest-environment jsdom
import axe from "axe-core";
import { beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { tauri } from "./tauri-fake"; // the mocks register in tests-setup
import PanelGestor from "./PanelGestor.vue";
import AcercaDe from "./AcercaDe.vue";
import { crearLog } from "./store";

// #20: axe on the mounted components. jsdom cannot run layout-dependent
// rules (color-contrast arrives as "incomplete", not a violation): the
// gate is SERIOUS+CRITICAL; everything else is a warning, not a failure.

const SNAPSHOT = {
  version_gestor: "11.4.2",
  version_node: "26.2.0",
  comando_actualizar: "npm i -g",
  packages: [
    { name: "hunkdiff", installed: "0.17.2", latest: "0.18.0", outdated: true },
    { name: "cowsay", installed: "1.0.0", latest: "1.0.0", outdated: false },
  ],
};

beforeEach(() => tauri.reiniciar());

async function sinViolacionesGraves() {
  const resultado = await axe.run(document.body);
  const graves = resultado.violations.filter((v) =>
    ["serious", "critical"].includes(v.impact ?? ""),
  );
  resultado.violations
    .filter((v) => !["serious", "critical"].includes(v.impact ?? ""))
    .forEach((v) => console.warn(`[axe aviso] ${v.id}: ${v.help}`));
  expect(graves.map((v) => `${v.id} (${v.impact})`)).toEqual([]);
}

describe("axe (accesibilidad)", () => {
  it("el panel cargado no tiene violaciones serias ni críticas", async () => {
    tauri.responder("get_excluded", { estado: "ok", nombres: [] });
    tauri.responder("list_globals", SNAPSHOT);
    mount(PanelGestor, {
      props: { gestor: "npm", log: crearLog() },
      attachTo: document.body,
    });
    await flushPromises();
    await sinViolacionesGraves();
  });

  it("el modal Acerca de tampoco", async () => {
    mount(AcercaDe, { attachTo: document.body });
    await flushPromises();
    await sinViolacionesGraves();
  });
});
