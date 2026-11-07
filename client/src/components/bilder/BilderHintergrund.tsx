// Uebernahme im Hintergrund (P-11.30): Der Eingang wird abgearbeitet, auch wenn der Tab
// nicht offen ist. Rust legt jeden Fund eines aktiven Flugs persistent im Eingang ab;
// die Ereignisse `bild_quelle_neu` und `bild_aufgenommen` sind nur Wecksignale, ein
// langsamer Takt holt nach, was verpasst wurde (Webview gedrosselt, Neustart).
// Am Tablet laeuft das nur bei offenem Tab: Der Sim-PC verarbeitet die Funde, das Tablet
// laedt die Liste im kurzen Takt neu und meldet gescheiterte Aufnahmen.
import { useEffect } from "react";
import { isTauri, listen } from "../../lib/ipc";
import { bilderStore } from "./bilderStore";

export const EINGANG_TAKT_MS = 20000;
export const TABLET_TAKT_MS = 5000;

interface Aufgenommen { ok?: boolean; code?: string; meldung?: string }

export function BilderHintergrund() {
  useEffect(() => {
    let aus = false;
    const abmelden: Array<() => void> = [];
    const wecken = () => { void bilderStore.eingang().catch(() => undefined); };
    const merken = (u: () => void) => { if (aus) u(); else abmelden.push(u); };
    void listen("bild_quelle_neu", wecken).then(merken).catch(() => undefined);
    void listen<Aufgenommen>("bild_aufgenommen", (e) => {
      // Taste und Yoke haben keinen Aufrufer, der den Fehler sieht.
      if (e.payload?.ok === false) bilderStore.aufnahmeGescheitert(e.payload.code ?? "", e.payload.meldung ?? "");
      wecken();
    }).then(merken).catch(() => undefined);
    wecken(); // Funde, die seit dem letzten Lauf liegen geblieben sind
    const takt = window.setInterval(wecken, isTauri ? EINGANG_TAKT_MS : TABLET_TAKT_MS);
    return () => { aus = true; window.clearInterval(takt); abmelden.forEach((u) => u()); };
  }, []);
  return null;
}
