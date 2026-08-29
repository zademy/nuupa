import { describe, expect, it } from "vitest";
import { crearLog } from "./store";
import { createHabilidadesStore, ESTADO_HABILIDAD } from "./habilidades";

// The skills store through the SAME seam as the packages store: an
// injectable invoke — no Tauri, no network, no real user folder.

const LISTA = {
  habilidades: [
    { nombre: "markdownlint", estado: ESTADO_HABILIDAD.NO_GESTIONADA },
    { nombre: "invalida-skill", estado: ESTADO_HABILIDAD.INVALIDA },
  ],
  manifest: { estado: "ok" },
};

function fakeInvoke() {
  return async (cmd) => {
    if (cmd === "listar_habilidades") return LISTA;
    throw new Error(`comando inesperado: ${cmd}`);
  };
}

describe("store de habilidades", () => {
  it("refrescar consulta listar_habilidades y llena la lista", async () => {
    const store = createHabilidadesStore(fakeInvoke());
    await store.refresh();
    expect(store.state.habilidades).toEqual(LISTA.habilidades);
    expect(store.state.loading).toBe(false);
    expect(store.state.error).toBe("");
    expect(store.estadoManifest.value).toBe("ok");
  });

  it("refrescar deja el error visible si invoke falla y conserva la lista vieja", async () => {
    let llamadas = 0;
    const store = createHabilidadesStore(async () => {
      llamadas++;
      if (llamadas === 1) return LISTA;
      throw "no home";
    });
    await store.refresh();
    await store.refresh();
    expect(store.state.error).toContain("no home");
    expect(store.state.habilidades).toEqual(LISTA.habilidades);
    expect(store.state.loading).toBe(false);
  });

  it("manifest corrupto llega al estado y a la fila de emergencia", async () => {
    const store = createHabilidadesStore(async () => ({
      habilidades: [],
      manifest: { estado: "corrupto" },
    }));
    await store.refresh();
    expect(store.estadoManifest.value).toBe("corrupto");
  });

  it("manifest ilegible expone su detalle", async () => {
    const store = createHabilidadesStore(async () => ({
      habilidades: [],
      manifest: { estado: "ilegible", detalle: "Permission denied" },
    }));
    await store.refresh();
    expect(store.estadoManifest.value).toBe("ilegible");
    expect(store.detalleManifest.value).toContain("Permission denied");
  });

  it("un estado de manifest desconocido es fail-closed ilegible", async () => {
    const store = createHabilidadesStore(async () => ({
      habilidades: [],
      manifest: { estado: "algo-raro" },
    }));
    await store.refresh();
    expect(store.estadoManifest.value).toBe("ilegible");
  });

  it("manifestDeCero pide el comando y refresca", async () => {
    const llamadas = [];
    const store = createHabilidadesStore(async (cmd) => {
      llamadas.push(cmd);
      if (cmd === "listar_habilidades") return LISTA;
      if (cmd === "habilidades_de_cero") return {};
    });
    await store.refresh();
    await store.manifestDeCero();
    expect(llamadas).toEqual([
      "listar_habilidades",
      "habilidades_de_cero",
      "listar_habilidades",
    ]);
    expect(store.estadoManifest.value).toBe("ok");
  });

  it("abrirCarpeta pasa el nombre validado", async () => {
    const store = createHabilidadesStore(async (cmd, args) => {
      if (cmd === "listar_habilidades") return LISTA;
      if (cmd === "abrir_habilidad") {
        expect(args).toEqual({ nombre: "markdownlint" });
        return {};
      }
    });
    await store.refresh();
    await store.abrirCarpeta("markdownlint");
    expect(store.hasError("markdownlint")).toBe(false);
  });

  it("abrirCarpeta con fallo marca la fila en error y lo deja en el log", async () => {
    const log = crearLog();
    const store = createHabilidadesStore(async (cmd) => {
      if (cmd === "listar_habilidades") return LISTA;
      if (cmd === "abrir_habilidad") throw "no such folder";
    }, log);
    await store.refresh();
    await store.abrirCarpeta("markdownlint");
    expect(store.hasError("markdownlint")).toBe(true);
    expect(store.detalleFallo("markdownlint")).toContain("no such folder");
    expect(log.lineas.value.some((l) => l.includes("markdownlint"))).toBe(true);
  });

  it("la búsqueda filtra por subcadena insensible a mayúsculas", async () => {
    const store = createHabilidadesStore(fakeInvoke());
    await store.refresh();
    expect(store.filtradas.value).toHaveLength(2);
    store.search.value = "MARK";
    expect(store.filtradas.value.map((h) => h.nombre)).toEqual([
      "markdownlint",
    ]);
  });

  it("el conteo suma el total y las inválidas", async () => {
    const store = createHabilidadesStore(fakeInvoke());
    await store.refresh();
    expect(store.conteo.value).toEqual({ total: 2, invalidas: 1 });
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

  it("escanear consulta el origen y preselecciona las conformes", async () => {
    const store = createHabilidadesStore(async (cmd, args) => {
      if (cmd === "listar_habilidades") return LISTA;
      if (cmd === "escanear_origen") {
        expect(args).toEqual({ origen: "github.com/o/r" });
        return ITEMS;
      }
    });
    await store.refresh();
    store.origenInput.value = "github.com/o/r";
    await store.escanear();
    expect(store.escaneo.abierto).toBe(true);
    expect(store.escaneo.items).toEqual(ITEMS);
    expect(store.escaneo.cargando).toBe(false);
    // conformes preselected, invalidas never
    expect(store.seleccion.value).toEqual(["skills/productivity/buena"]);
  });

  it("escanear sin URL no invoca nada", async () => {
    const llamadas = [];
    const store = createHabilidadesStore(async (cmd) => {
      llamadas.push(cmd);
    });
    await store.escanear();
    expect(llamadas).toEqual([]);
  });

  it("escanear con fallo muestra el error en la sección", async () => {
    const store = createHabilidadesStore(async (cmd) => {
      if (cmd === "escanear_origen") throw "repo no encontrado";
    });
    store.origenInput.value = "github.com/o/r";
    await store.escanear();
    expect(store.escaneo.error).toContain("repo no encontrado");
    expect(store.escaneo.cargando).toBe(false);
  });

  it("toggleRuta agrega y quita de la selección", async () => {
    const store = createHabilidadesStore(async (cmd) => {
      if (cmd === "listar_habilidades") return LISTA;
      if (cmd === "escanear_origen") return ITEMS;
    });
    await store.refresh();
    store.origenInput.value = "o/r";
    await store.escanear();
    expect(store.seleccion.value).toEqual(["skills/productivity/buena"]);
    store.toggleRuta("skills/productivity/buena"); // off
    expect(store.seleccion.value).toEqual([]);
    store.toggleRuta("skills/productivity/buena"); // on again
    expect(store.seleccion.value).toEqual(["skills/productivity/buena"]);
  });

  it("instalarSeleccionadas invoca con origen y rutas, refresca y cierra", async () => {
    const llamadas = [];
    const store = createHabilidadesStore(async (cmd, args) => {
      llamadas.push(cmd);
      if (cmd === "listar_habilidades") return LISTA;
      if (cmd === "escanear_origen") return ITEMS;
      if (cmd === "instalar_habilidades") {
        expect(args).toEqual({
          origen: "o/r",
          rutas: ["skills/productivity/buena"],
        });
        return [
          { ruta: "skills/productivity/buena", nombre: "buena", ok: true },
        ];
      }
    });
    await store.refresh();
    store.origenInput.value = "o/r";
    await store.escanear();
    await store.instalarSeleccionadas();
    expect(llamadas).toContain("instalar_habilidades");
    expect(llamadas.filter((c) => c === "listar_habilidades").length).toBe(2); // refreshed
    expect(store.escaneo.abierto).toBe(false);
    expect(store.instalando.value).toBe(false);
  });

  it("las instalaciones fallidas quedan en el log con su motivo", async () => {
    const log = crearLog();
    const store = createHabilidadesStore(async (cmd) => {
      if (cmd === "listar_habilidades") return LISTA;
      if (cmd === "escanear_origen") return ITEMS;
      if (cmd === "instalar_habilidades")
        return [
          { ruta: "skills/productivity/buena", nombre: "buena", ok: true },
          {
            ruta: "skills/productivity/invalida",
            nombre: "invalida",
            ok: false,
            motivo: "inválida: sin frontmatter",
          },
        ];
    }, log);
    store.origenInput.value = "o/r";
    await store.escanear();
    store.toggleRuta("skills/productivity/invalida"); // the user insisted
    await store.instalarSeleccionadas();
    expect(
      log.lineas.value.some(
        (l) => l.includes("invalida") && l.includes("sin frontmatter"),
      ),
    ).toBe(true);
  });

  it("instalarSeleccionadas sin selección no invoca", async () => {
    const llamadas = [];
    const store = createHabilidadesStore(async (cmd) => {
      llamadas.push(cmd);
      if (cmd === "escanear_origen") return ITEMS;
    });
    store.origenInput.value = "o/r";
    await store.escanear();
    store.seleccion.value = [];
    await store.instalarSeleccionadas();
    expect(llamadas).toEqual(["escanear_origen"]);
  });

  it("cerrarEscaneo limpia la sección y la selección", async () => {
    const store = createHabilidadesStore(async (cmd) =>
      cmd === "escanear_origen" ? ITEMS : LISTA,
    );
    store.origenInput.value = "o/r";
    await store.escanear();
    store.cerrarEscaneo();
    expect(store.escaneo.abierto).toBe(false);
    expect(store.escaneo.items).toEqual([]);
    expect(store.seleccion.value).toEqual([]);
  });

  // ---- #29: actualizar una gestionada ----

  const CON_ESTADOS = {
    habilidades: [
      { nombre: "vieja", estado: ESTADO_HABILIDAD.ACTUALIZACION },
      { nombre: "al-dia", estado: ESTADO_HABILIDAD.ACTUAL },
      { nombre: "suelta", estado: ESTADO_HABILIDAD.NO_GESTIONADA },
      { nombre: "rota", estado: ESTADO_HABILIDAD.INVALIDA },
    ],
    manifest: { estado: "ok" },
  };

  it("actualizar pide el comando con el nombre y refresca", async () => {
    const llamadas = [];
    const store = createHabilidadesStore(async (cmd, args) => {
      llamadas.push(cmd);
      if (cmd === "listar_habilidades") return CON_ESTADOS;
      if (cmd === "actualizar_habilidad") {
        expect(args).toEqual({ nombre: "vieja" });
        return {};
      }
    });
    await store.refresh();
    await store.actualizar("vieja");
    expect(llamadas).toEqual([
      "listar_habilidades",
      "actualizar_habilidad",
      "listar_habilidades",
    ]);
    expect(store.estaActualizando("vieja")).toBe(false);
    expect(store.hasError("vieja")).toBe(false);
  });

  it("actualizar fallido marca SOLO esa fila y deja el motivo en el log", async () => {
    const log = crearLog();
    const store = createHabilidadesStore(async (cmd) => {
      if (cmd === "listar_habilidades") return CON_ESTADOS;
      if (cmd === "actualizar_habilidad") throw "inválida: sin frontmatter";
    }, log);
    await store.refresh();
    await store.actualizar("vieja");
    expect(store.hasError("vieja")).toBe(true);
    expect(store.detalleFallo("vieja")).toContain("sin frontmatter");
    expect(log.lineas.value.some((l) => l.includes("vieja"))).toBe(true);
    expect(store.state.error).toBe(""); // the TABLE's error stays clean
  });

  it("solo una Actualización disponible se puede actualizar", async () => {
    const llamadas = [];
    const store = createHabilidadesStore(async (cmd) => {
      llamadas.push(cmd);
      if (cmd === "listar_habilidades") return CON_ESTADOS;
      if (cmd === "actualizar_habilidad") return {};
    });
    await store.refresh();
    expect(store.puedeActualizar("vieja")).toBe(true);
    for (const nombre of ["al-dia", "suelta", "rota"]) {
      expect(store.puedeActualizar(nombre)).toBe(false);
      await store.actualizar(nombre); // the store guards too: no invoke
    }
    expect(llamadas).toEqual(["listar_habilidades"]);
  });

  it("manifest corrupto bloquea la actualización sin invocar", async () => {
    const llamadas = [];
    const store = createHabilidadesStore(async (cmd) => {
      llamadas.push(cmd);
      if (cmd === "listar_habilidades")
        return {
          habilidades: [
            { nombre: "vieja", estado: ESTADO_HABILIDAD.SIN_VERIFICAR },
          ],
          manifest: { estado: "corrupto" },
        };
      if (cmd === "actualizar_habilidad") return {};
    });
    await store.refresh();
    expect(store.puedeActualizar("vieja")).toBe(false);
    await store.actualizar("vieja");
    expect(llamadas).toEqual(["listar_habilidades"]);
  });

  it("un segundo clic en la misma fila en vuelo es ignorado", async () => {
    let resolver;
    const comandos = [];
    const store = createHabilidadesStore(async (cmd) => {
      if (cmd === "listar_habilidades") return CON_ESTADOS;
      if (cmd === "actualizar_habilidad") {
        comandos.push(cmd);
        return new Promise((r) => (resolver = r));
      }
    });
    await store.refresh();
    const primera = store.actualizar("vieja");
    await store.actualizar("vieja"); // in flight: no-op
    expect(comandos).toEqual(["actualizar_habilidad"]);
    expect(store.estaActualizando("vieja")).toBe(true);
    resolver();
    await primera;
  });
});
