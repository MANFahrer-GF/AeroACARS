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
