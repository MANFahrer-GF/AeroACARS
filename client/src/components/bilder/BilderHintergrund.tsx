// Uebernahme im Hintergrund (P-11.30): Der Eingang wird abgearbeitet, auch wenn der Tab
// nicht offen ist. Rust legt jeden Fund eines aktiven Flugs persistent im Eingang ab;
// die Ereignisse `bild_quelle_neu` und `bild_aufgenommen` sind nur Wecksignale, ein
// langsamer Takt holt nach, was verpasst wurde (Webview gedrosselt, Neustart).
import { useEffect } from "react";
import { listen } from "../../lib/ipc";
import { bilderStore } from "./bilderStore";

export const EINGANG_TAKT_MS = 20000;

export function BilderHintergrund() {
  useEffect(() => {
    let aus = false;
    const abmelden: Array<() => void> = [];
    const wecken = () => { void bilderStore.eingang().catch(() => undefined); };
    for (const name of ["bild_quelle_neu", "bild_aufgenommen"]) {
      void listen(name, wecken).then((u) => { if (aus) u(); else abmelden.push(u); }).catch(() => undefined);
    }
    wecken(); // Funde, die seit dem letzten Lauf liegen geblieben sind
    const takt = window.setInterval(wecken, EINGANG_TAKT_MS);
    return () => { aus = true; window.clearInterval(takt); abmelden.forEach((u) => u()); };
  }, []);
  return null;
}
