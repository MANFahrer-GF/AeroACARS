// Tastenkuerzel aus einem Tastendruck; Schreibweise wie `bilder-kern/src/tastenkuerzel.rs` (nur A-Z, 0-9, F1-F24).

export interface TastenDruck {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

/** Die Haupttaste; `null` bei reinen Modifikatoren und Tasten, die der Kern nicht kennt. */
function haupttaste(e: TastenDruck): string | null {
  if (/^[a-zA-Z0-9]$/.test(e.key)) return e.key.toUpperCase(); // `key` zuerst: QWERTZ, Y und Z
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(e.key)) return e.key;
  const c = /^(?:Key([A-Z])|Digit([0-9]))$/.exec(e.code); // Shift+1 oder AltGr liefern in `key` ein Sonderzeichen
  return c ? (c[1] ?? c[2]) : null;
}

/** `kuerzel`: fertig; `modifikator`: Haupttaste ohne Strg/Alt/Shift/Win; `null`: weiter warten. */
export function kuerzelLesen(e: TastenDruck): { kuerzel: string } | "modifikator" | null {
  const taste = haupttaste(e);
  if (!taste) return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Win"].filter(Boolean);
  return mods.length ? { kuerzel: [...mods, taste].join("+") } : "modifikator";
}
