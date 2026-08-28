import { describe, expect, it } from "vitest";
import { armarDiagnostico, redactar } from "./diagnostico";

const CASO = {
  version: "0.3.3",
  so: "macos (aarch64)",
  gestores: ["npm", "pnpm"],
  activo: "npm",
  conteo: { total: 12, desactualizados: 3, excluidos: 1 },
  lineas: [
    "npm/hunkdiff: the update failed",
    "npm: could not load exclusions: /Users/sadot/Library/…",
    "npm/context-mode: added 1 package in 2s",
  ],
  home: "/Users/sadot",
};

describe("diagnóstico (#21)", () => {
  it("redactar convierte el home absoluto en ~", () => {
    expect(redactar("/Users/sadot/Library/x → ok", "/Users/sadot")).toBe(
      "~/Library/x → ok",
    );
    // a DIFFERENT user's path stays untouched
    expect(redactar("/Users/otro/Library", "/Users/sadot")).toBe(
      "/Users/otro/Library",
    );
    // no home known: nothing to redact
    expect(redactar("/Users/sadot/x", null)).toBe("/Users/sadot/x");
  });

  it("el bloque lleva versión, so, gestores, activo y conteos", () => {
    const texto = armarDiagnostico(CASO);
    expect(texto).toContain("nuupa v0.3.3");
    expect(texto).toContain("os: macos (aarch64)");
    expect(texto).toContain("gestores: npm, pnpm");
    expect(texto).toContain("gestor activo: npm");
    expect(texto).toContain("desactualizados: 3");
    expect(texto).toContain("excluidos: 1");
  });

  it("incluye las líneas del log redactadas — nunca el home absoluto", () => {
    const texto = armarDiagnostico(CASO);
    expect(texto).toContain("the update failed");
    expect(texto).toContain("~/Library/…");
    expect(texto).not.toContain("/Users/sadot");
  });

  it("recorta el log a las últimas 50 líneas", () => {
    const lineas = Array.from({ length: 120 }, (_, i) => `línea ${i}`);
    const texto = armarDiagnostico({ ...CASO, lineas });
    expect(texto).toContain("--- log (últimas 50) ---");
    expect(texto).toContain("línea 119");
    expect(texto).not.toContain("línea 69\n"); // 70 and older are out
    expect(texto).toContain("línea 70"); // the 50th from the end stays
  });
});
