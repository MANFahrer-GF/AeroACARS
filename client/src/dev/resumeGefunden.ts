// Vorschau `?gefunden=1`: ein nur auf dem Server gefundener Flug. Muss vor
// `lib/ipc` geladen sein — der Weg zum Backend wird beim Laden festgelegt.
const q = new URLSearchParams(window.location.search);
if (q.has("gefunden")) {
  const fund = {
    pirep_id: "p", flight_number: "442", dpt_airport: "YBBN", arr_airport: "VHHH",
    status: "ENR", zuletzt_gemeldet_vor_s: q.has("woanders") ? 40 : 7200,
    laeuft_woanders: q.has("woanders"),
  };
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string) => (cmd === "flight_discover_resumable" ? [fund] : null),
    transformCallback: () => 0,
  };
}

// Vorschau `?arrived=1` (DAL667): `&sim=1` Simulator verbunden. `flight_end`
// gelingt; alle Aufrufe stehen zum Nachsehen in `window.__aufrufe`.
if (q.has("arrived")) {
  const aufrufe: string[] = [];
  (window as unknown as Record<string, unknown>).__aufrufe = aufrufe;
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string) => {
      aufrufe.push(cmd);
      if (cmd === "sim_status") {
        return q.has("sim")
          ? { state: "connected", kind: "msfs2024", snapshot: { lat: -22.81, lon: -43.25 }, last_error: null, available: true }
          : { state: "disconnected", kind: "msfs2024", snapshot: null, last_error: null, available: true };
      }
      return null;
    },
    transformCallback: () => 0,
  };
}
