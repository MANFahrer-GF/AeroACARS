// Der eine Store des Bilder-Tabs mit den echten Abhaengigkeiten, plus React-Haken.
import { useSyncExternalStore } from "react";
import { invoke, isTauri } from "../../lib/ipc";
import { bildAktionen } from "./logik/api";
import { kodieren } from "./logik/leinwand";
import { erzeugeStore, type BilderZustand } from "./logik/store";

/** UUID v4 ueber `getRandomValues`: `crypto.randomUUID` fehlt dem Tablet, die LAN-Bruecke liefert ueber http aus. */
function neueUuid(): string {
  const b = crypto.getRandomValues(new Uint8Array(16));
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  const h = Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

export const bilderStore = erzeugeStore({
  api: bildAktionen,
  kodieren,
  zustandLesen: () => invoke<Record<string, string>>("ui_state_get_all"),
  zustandSchreiben: async (schluessel, wert) => {
    await invoke("ui_state_set", { key: schluessel, value: wert });
  },
  neueId: neueUuid,
  warten: (ms) => new Promise((r) => setTimeout(r, ms)),
  jetzt: () => Date.now(),
  ohneEingang: !isTauri,
});

export function useBilder(): BilderZustand {
  return useSyncExternalStore(bilderStore.abonnieren, bilderStore.lesen, bilderStore.lesen);
}
