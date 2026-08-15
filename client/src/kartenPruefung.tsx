// Prüfseite für die Live-Karte.
//
// Der Tauri-WebView hat keine von außen erreichbare Konsole — fehlt eine
// Ebene, sieht man nur das Ergebnis, nicht den Grund. Diese Seite
// rendert dieselbe Komponente im Browser, wo die Konsole lesbar ist.
//
// Start: npm run dev, dann http://localhost:1420/?karte
import { createRoot } from "react-dom/client";
import maplibregl from "maplibre-gl";
import { LiveMapView } from "./components/LiveMapView";
import "maplibre-gl/dist/maplibre-gl.css";
import "./i18n";

// Jede angelegte Karte greifbar machen, samt Protokoll der Ebenen —
// sonst lässt sich von außen nicht feststellen, welche fehlt und warum.
const Original = maplibregl.Map;
const gemerkt: unknown[] = [];
(maplibregl as unknown as { Map: unknown }).Map = class extends Original {
  constructor(...args: ConstructorParameters<typeof Original>) {
    super(...args);
    gemerkt.push(this);
    (window as unknown as Record<string, unknown>).__karten = gemerkt;
    const fehler: string[] = [];
    (window as unknown as Record<string, unknown>).__ebenenFehler = fehler;
    // Externe Kacheln sind im Prüf-Browser blockiert, und MapLibre feuert
    // `load` erst mit vollständig geladenem Stil. Ohne dieses Ereignis
    // läuft `addOverlays` nie — man sähe den gesuchten Fehler nicht.
    // Deshalb sofort auf einen Stil ohne Fremdquellen umstellen.
    this.setStyle({
      version: 8,
      glyphs: "https://demotiles.maplibre.org/font/{fontstack}/{range}.pbf",
      sources: {},
      layers: [{ id: "grund", type: "background", paint: { "background-color": "#0b1220" } }],
    } as never);
    const anlegen = this.addLayer.bind(this);
    this.addLayer = ((spec: { id?: string }, davor?: string) => {
      try { return anlegen(spec as never, davor); }
      catch (e) { fehler.push(`${spec?.id}: ${(e as Error).message}`); throw e; }
    }) as typeof this.addLayer;
  }
};

const wurzel = document.getElementById("root");
if (wurzel) {
  createRoot(wurzel).render(
    <div style={{ position: "absolute", inset: 0 }}>
      <LiveMapView
        activeFlight={null}
        simSnapshot={null}
        simKind={undefined}
        onSwitchToBriefing={() => {}}
      />
    </div>,
  );
}
