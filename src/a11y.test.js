// @vitest-environment jsdom
import axe from "axe-core";
import { beforeEach, describe, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { tauri } from "./tauri-fake"; // the mocks register in tests-setup
import PanelGestor from "./PanelGestor.vue";
import PanelHabilidades from "./PanelHabilidades.vue";
import AcercaDe from "./AcercaDe.vue";
import { crearLog } from "./store";

// #20: axe on the mounted components, as a reproducible LOCAL check.
// Per the spec this first iteration is WARNING-ONLY (no blocking gate):
// every violation lands in the console; nothing fails the suite. jsdom
// cannot run layout-dependent rules (color-contrast arrives as
// "incomplete", not a violation).

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

async function avisarViolaciones() {
  const resultado = await axe.run(document.body);
  for (const v of resultado.violations) {
    console.warn(
      `[axe ${v.impact ?? "?"}] ${v.id}: ${v.help} (${v.nodes.length} nodos)`,
    );
  }
  if (resultado.violations.length === 0) return true;
  console.warn(
    `[axe] ${resultado.violations.length} violacion(es) — ver arriba; primera iteración sin gate (#20)`,
  );
  return false;
}

describe("axe (accesibilidad)", () => {
  it("chequea el panel cargado (avisos por consola, sin gate)", async () => {
    tauri.responder("get_excluded", { estado: "ok", nombres: [] });
    tauri.responder("list_globals", SNAPSHOT);
    mount(PanelGestor, {
      props: { gestor: "npm", log: crearLog() },
      attachTo: document.body,
    });
    await flushPromises();
    await avisarViolaciones();
  });

  it("chequea el panel de habilidades (avisos por consola, sin gate)", async () => {
    tauri.responder("listar_habilidades", {
      habilidades: [
        { nombre: "markdownlint", estado: "no_gestionada" },
        { nombre: "invalida-skill", estado: "invalida" },
      ],
      manifest: { estado: "ok" },
    });
    mount(PanelHabilidades, {
      props: { log: crearLog() },
      attachTo: document.body,
    });
    await flushPromises();
    await avisarViolaciones();
  });

  it("chequea el modal Acerca de (avisos por consola, sin gate)", async () => {
    mount(AcercaDe, { attachTo: document.body });
    await flushPromises();
    await avisarViolaciones();
  });
});
