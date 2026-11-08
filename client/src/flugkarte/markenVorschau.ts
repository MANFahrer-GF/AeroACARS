// Kleines Vorschaubild beim Ueberfahren einer Bildmarke, fuer alle drei Karten gleich.
// Liegt am <body> statt in der Karte: die kleine Karte der Detailspalte wuerde es sonst
// abschneiden. Nur mit der Maus; am Tablet oeffnet das Antippen wie bisher die Grossansicht.
import type maplibregl from "maplibre-gl";
import "./markenVorschau.css";

export interface MarkenVorschauDaten {
  /** Adresse des kleinen Bildes; null = keins da, dann bleibt die Beschriftung allein. */
  url: Promise<string | null>;
  /** Beschriftung, die erste Zeile hervorgehoben; leer = der Name der Marke. */
  zeilen: string[];
}

export type VorschauVon = (id: string) => MarkenVorschauDaten | null;

const VERZOEGERUNG_MS = 120;
const ABSTAND = 10;
const RAND = 8;

/** Haengt die Vorschau an eine Marke; die Rueckgabe schliesst sie (beim Entfernen der Marke aufrufen). */
export function markeMitVorschau(el: HTMLElement, map: maplibregl.Map, name: string, daten: () => MarkenVorschauDaten | null): () => void {
  el.removeAttribute("title"); // der Browser zeigte sonst denselben Text zusaetzlich
  let uhr: ReturnType<typeof setTimeout> | undefined;
  let tip: HTMLElement | null = null;

  function setzen() {
    if (!tip) return;
    const m = el.getBoundingClientRect(), k = map.getContainer().getBoundingClientRect();
    const x = m.left + m.width / 2, y = m.top + m.height / 2;
    tip.hidden = x < k.left || x > k.right || y < k.top || y > k.bottom; // Punkt aus dem Bild geschoben
    if (tip.hidden) return;
    const b = tip.offsetWidth, h = tip.offsetHeight;
    const oben = m.top - ABSTAND - h;
    tip.style.top = `${oben >= RAND ? oben : Math.min(m.bottom + ABSTAND, window.innerHeight - h - RAND)}px`;
    tip.style.left = `${Math.max(RAND, Math.min(x - b / 2, window.innerWidth - b - RAND))}px`;
  }

  function zu() {
    clearTimeout(uhr);
    if (!tip) return;
    tip.remove();
    tip = null;
    map.off("move", setzen);
    map.off("resize", setzen);
  }

  function auf() {
    tip = document.createElement("div");
    tip.className = "marken-vorschau";
    tip.setAttribute("aria-hidden", "true");
    const rahmen = document.createElement("div");
    rahmen.className = "marken-vorschau__bild";
    const d = daten();
    tip.append(rahmen, ...(d?.zeilen.length ? d.zeilen : [name]).map((text) => {
      const zeile = document.createElement("div");
      zeile.className = "marken-vorschau__zeile";
      zeile.textContent = text;
      return zeile;
    }));
    document.body.appendChild(tip);
    map.on("move", setzen);
    map.on("resize", setzen); // Fenster oder Spalte geaendert: der Punkt wandert ohne „move“
    setzen();
    void (d?.url ?? Promise.resolve(null)).catch(() => null).then((url) => {
      if (url) {
        const bild = document.createElement("img");
        bild.src = url;
        bild.alt = "";
        rahmen.appendChild(bild);
      } else {
        rahmen.remove();
        setzen();
      }
    });
  }

  el.addEventListener("pointerenter", (ev) => {
    if (ev.pointerType !== "mouse") return;
    zu();
    uhr = setTimeout(auf, VERZOEGERUNG_MS);
  });
  el.addEventListener("pointerleave", zu);
  el.addEventListener("click", zu); // die Grossansicht legt sich darueber, ein Verlassen kaeme zu spaet
  return zu;
}
