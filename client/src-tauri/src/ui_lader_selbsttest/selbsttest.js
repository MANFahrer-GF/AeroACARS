// P-8.03-Messung (Etappe 5): laeuft im Fenster `ui` ueber aeroui:// und
// schickt das Ergebnis an den Lader (ui_lader.rs). Nur fuer die CI.
const erg = { ursprung: location.origin, ua: navigator.userAgent };
const verstoesse = [];
document.addEventListener("securitypolicyviolation", (e) =>
  verstoesse.push({ richtlinie: e.violatedDirective, ziel: e.blockedURI }),
);
const warte = (ms) => new Promise((r) => setTimeout(r, ms));
const fehler = (e) => String(e && e.message ? e.message : e);

async function messen() {
  // Wie WebView2 von sich aus: eine fehlende Nebendatei darf das Paket NICHT
  // auf den eingebauten Stand umschalten (Messung 29.10.2026).
  erg.favicon_status = (await fetch("/favicon.ico")).status;

  // CSP: Inline-Skript, eval und fremder Host abgewiesen
  erg.inline_script_blockiert = window.__inline !== 1;
  try { (0, eval)("1+1"); erg.eval_blockiert = false; } catch { erg.eval_blockiert = true; }
  try { await fetch("https://example.com/", { cache: "no-store" }); erg.fremder_host_blockiert = false; }
  catch (e) { erg.fremder_host_blockiert = true; erg.fremder_host_fehler = fehler(e); }

  // fetch an live (CSP connect-src erlaubt live.kant.ovh; /status.json sendet CORS *)
  try {
    const r = await fetch("https://live.kant.ovh/status.json", { cache: "no-store" });
    erg.live_fetch = r.ok ? "ok" : `http ${r.status}`;
  } catch (e) { erg.live_fetch = `fehler: ${fehler(e)}`; }

  // Schrift: MIME und Laden ueber FontFace
  try {
    const r = await fetch("/schrift.woff2");
    erg.schrift_mime = r.headers.get("content-type");
    const f = new FontFace("Selbsttest", "url(/schrift.woff2)");
    await f.load();
    erg.schrift = f.status;
  } catch (e) { erg.schrift = `fehler: ${fehler(e)}`; }

  // WASM: MIME, Streaming-Instanz (braucht application/wasm) und reines Kompilieren
  try {
    const r = await fetch("/mini.wasm");
    erg.wasm_mime = r.headers.get("content-type");
    const puffer = await r.arrayBuffer();
    await WebAssembly.compile(puffer);
    erg.wasm_kompilieren = "ok";
  } catch (e) { erg.wasm_kompilieren = `fehler: ${fehler(e)}`; }
  try { await WebAssembly.instantiateStreaming(fetch("/mini.wasm")); erg.wasm_streaming = "ok"; }
  catch (e) { erg.wasm_streaming = `fehler: ${fehler(e)}`; }

  // Ursprung ueber Neustarts: der Wert des vorigen Laufs muss noch da sein
  erg.speicher_vorher = localStorage.getItem("aa-selbsttest");
  erg.speicher_neu = String(Date.now());
  localStorage.setItem("aa-selbsttest", erg.speicher_neu);

  // Rechte (P-8.10): jeder Systembefehl abgewiesen, lesen/fachlich erlaubt
  const ipc = window.__TAURI_INTERNALS__;
  erg.ipc_vorhanden = !!ipc;
  const aufruf = async (befehl) => {
    try {
      await Promise.race([ipc.invoke(befehl, {}), warte(8000).then(() => { throw new Error("zeit"); })]);
      return { ok: true };
    } catch (e) { return { ok: false, text: fehler(e) }; }
  };
  const acl = (r) => !r.ok && /not allowed/i.test(r.text || "");
  const { befehle } = await (await fetch("/befehle.json")).json();
  erg.system_nicht_abgewiesen = [];
  erg.system_zahl = 0;
  for (const [befehl, stufe] of Object.entries(befehle)) {
    if (stufe !== "system") continue;
    erg.system_zahl += 1;
    const r = await aufruf(befehl);
    if (!acl(r)) erg.system_nicht_abgewiesen.push({ befehl, ...r });
  }
  erg.lesen_app_info = await aufruf("app_info");
  erg.lesen_erlaubt = !acl(erg.lesen_app_info);
  erg.fachlich_discord_rpc_clear_flight = await aufruf("discord_rpc_clear_flight");
  erg.fachlich_erlaubt = !acl(erg.fachlich_discord_rpc_clear_flight);

  // Fehlerseite bei fehlender Datei, danach eingebauter Stand (zuletzt!)
  const f = await fetch("/gibt-es-nicht.js");
  erg.fehlerseite = { status: f.status, text: (await f.text()).includes("Paket defekt, eingebauter Stand aktiv") };
  const danach = await (await fetch("/index.html")).text();
  erg.danach_eingebaut = !danach.includes("aa-selbsttest-seite");

  await warte(200);
  erg.csp_verstoesse = verstoesse;
}

messen()
  .catch((e) => { erg.abbruch = fehler(e); })
  .finally(() => {
    document.getElementById("stand").textContent = JSON.stringify(erg, null, 2);
    // GET mit Abfrage statt POST: WKWebView reicht den Rumpf an eigene
    // Protokolle nicht verlaesslich durch.
    fetch(`/__selbsttest/ergebnis?d=${encodeURIComponent(JSON.stringify(erg, null, 2))}`);
  });
