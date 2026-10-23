// Verbindung zum lokalen Panel-Server von AeroACARS (nur 127.0.0.1, ohne PIN).
// Lesen: /panel/status, /panel/kabine, /panel/hoppie, /panel/debrief.
// Bedienen: POST /panel/cmd/{name} mit Pflichtkopf X-AeroACARS-Deck: 1.

export const BASIS = "http://127.0.0.1:47847";
const ZEITLIMIT_MS = 1500;

async function holen(pfad, init = {}) {
  const ctl = new AbortController();
  const t = setTimeout(() => ctl.abort(), ZEITLIMIT_MS);
  try {
    const res = await fetch(BASIS + pfad, { ...init, signal: ctl.signal });
    if (!res.ok) throw new Error(`${pfad}: HTTP ${res.status}`);
    return await res.json();
  } finally {
    clearTimeout(t);
  }
}

export const lesen = (pfad) => holen(pfad);

/** Löst einen Befehl aus; Antwort ist der frische Kabinenzustand. */
export function befehl(name, args = {}) {
  return holen(`/panel/cmd/${name}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-AeroACARS-Deck": "1" },
    body: JSON.stringify(args),
  });
}
