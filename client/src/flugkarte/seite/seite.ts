// v3 (Etappe 5, P-8.21): Die Flugkarte als eigenständige Seite im Laufzeit-Paket
// (ui/flugkarte/). Dieselbe Komponente (flugkarte.ts) zeigt der Client im
// Logbuch; die Admin-Webapp lädt diese Seite aus dem freigegebenen Paket
// (live: /api/bundle/ui/flugkarte/, Sandkasten ohne eigenen Ursprung) und
// reicht die Flugakte per postMessage herein. Zum Prüfen ohne App läuft sie
// aus dem Paketordner im Browser: ?akte=<adresse einer Flugakte (.json)> oder
// Datei wählen.
import { flugkarteAnzeigen } from "../flugkarte";
import type { Flugakte } from "../flugakteTypen";
import stilUrl from "../stil-dunkel.json?url";
import "../fenster.css";
import alleTexte from "virtual:flugkarte-texte";
import { akteAusNachricht, seitenSprache, uebersetzer } from "./texte";

const sprachen = Object.keys(alleTexte);
let sprache = seitenSprache(location.search, navigator.language ?? "", sprachen);
const feld = document.getElementById("karte") as HTMLDivElement;
const hinweis = document.getElementById("hinweis") as HTMLDivElement;
let weg: (() => void) | null = null;

function text(k: string): string {
  return uebersetzer(alleTexte, sprache)(k) ?? k;
}

function zeigen(akte: Flugakte) {
  weg?.();
  hinweis.textContent = "";
  (document.getElementById("wahl") as HTMLLabelElement).hidden = true;
  const start = akte.kopf?.start ? new Date(akte.kopf.start) : null;
  weg = flugkarteAnzeigen(feld, akte, {
    stilUrl,
    t: uebersetzer(alleTexte, sprache),
    sprache,
    datum: start ? start.toLocaleDateString(sprache, { timeZone: "UTC" }) : undefined,
    // Für Browser-Prüfungen greifbar (wie im Entwicklungsmodus von Client und Webapp)
    beiKarte: (karte) => { (window as unknown as { __flugkarte: unknown }).__flugkarte = karte; },
  });
}

function fehler(e: unknown) {
  hinweis.textContent = uebersetzer(alleTexte, sprache)("flugkarte.fenster.fehler", { fehler: String((e as Error)?.message ?? e) }) ?? String(e);
}

// Einbettung (Admin-Webapp): Akte kommt vom Elternfenster.
window.addEventListener("message", (ev) => {
  const n = akteAusNachricht(ev.data, ev.source === window.parent && window.parent !== window);
  if (!n) return;
  if (n.sprache && sprachen.includes(n.sprache)) sprache = n.sprache;
  try { zeigen(n.akte as Flugakte); } catch (e) { fehler(e); }
});
// Esc schließt das Fenster der einbettenden Seite (der Fokus liegt im Rahmen).
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !e.defaultPrevented && window.parent !== window) window.parent.postMessage({ typ: "flugkarte-schliessen" }, "*");
});

const adresse = new URLSearchParams(location.search).get("akte");
if (adresse) {
  hinweis.textContent = text("flugkarte.fenster.laedt");
  fetch(adresse)
    .then((r) => { if (!r.ok) throw new Error(`HTTP ${r.status}`); return r.json(); })
    .then((a) => zeigen(a as Flugakte))
    .catch(fehler);
} else if (window.parent === window) {
  const wahl = document.getElementById("wahl") as HTMLLabelElement;
  (document.getElementById("wahl-text") as HTMLSpanElement).textContent = text("flugkarte.fenster.titel");
  wahl.hidden = false;
  (document.getElementById("datei") as HTMLInputElement).addEventListener("change", async (e) => {
    const f = (e.target as HTMLInputElement).files?.[0];
    if (!f) return;
    try { zeigen(JSON.parse(await f.text()) as Flugakte); } catch (err) { fehler(err); }
  });
} else {
  window.parent.postMessage({ typ: "flugkarte-bereit" }, "*");
}
