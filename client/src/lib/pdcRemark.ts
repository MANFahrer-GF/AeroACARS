// v1.9.5 (#hoppie-pdc-freetext) — the optional remark a pilot can append
// to a PDC request, as Fenix and iniBuilds offer.

/** v1.9.5 (#hoppie-pdc-freetext): mirrors `FREE_TEXT_MAX_CHARS` in
 *  hoppie-protocol's pdc.rs — two CDU scratchpad lines, as Fenix offers. */
export const PDC_REMARK_MAX = 48;

/** The remark as it will go on the wire (the backend's
 *  `sanitize_free_text` does the same): uppercase, German letters spelled
 *  out, printable ASCII only (EuroScope plugins mangle the rest), no
 *  `{`/`}` — they delimit messages in the controller's poll response —
 *  single spaces, capped. */
export function cleanPdcRemark(raw: string): string {
  return raw
    .toUpperCase()
    .replace(/Ä/g, "AE")
    .replace(/Ö/g, "OE")
    .replace(/Ü/g, "UE")
    .replace(/ẞ/g, "SS")
    .replace(/\s/g, " ")
    .replace(/[^\x20-\x7E]/g, "")
    .replace(/[{}]/g, "")
    .replace(/ +/g, " ")
    .trimStart()
    .slice(0, PDC_REMARK_MAX);
}
