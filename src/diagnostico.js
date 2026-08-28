// Pure diagnostics-text helpers (#21): everything testable without the
// clipboard — the browser API itself is a thin, untested edge.

/** Redacts the user's home to `~` wherever it appears (#21: pasting the
 *  diagnostics in public must not leak the username). */
export function redactar(texto, home) {
  if (!home) return texto;
  return texto.replaceAll(home, "~");
}

/** How many log lines the diagnostics carry (#21). */
export const LIMITE_LOG = 50;

/** The diagnostics block: machine facts, the active gestor's situation
 *  and the LAST log lines — home-redacted, ready for the clipboard. */
export function armarDiagnostico({
  version,
  so,
  gestores,
  activo,
  conteo,
  lineas,
  home,
}) {
  const recorte = lineas.slice(-LIMITE_LOG);
  const r = (t) => redactar(t, home);
  return [
    `nuupa v${version}`,
    `os: ${so}`,
    `gestores: ${gestores.join(", ")}`,
    `gestor activo: ${activo}`,
    `paquetes: ${conteo.total} · desactualizados: ${conteo.desactualizados} · excluidos: ${conteo.excluidos}`,
    `--- log (últimas ${recorte.length}) ---`,
    ...recorte.map(r),
  ].join("\n");
}
