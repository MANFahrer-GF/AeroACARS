// Der eine Store des Bilder-Tabs mit den echten Abhaengigkeiten, plus React-Haken.
import { useSyncExternalStore } from "react";
import { invoke, isTauri } from "../../lib/ipc";
import { bildAktionen } from "./logik/api";
import { kodieren } from "./logik/leinwand";
import { erzeugeStore, type BilderZustand } from "./logik/store";

export const bilderStore = erzeugeStore({
  api: bildAktionen,
  kodieren,
  zustandLesen: () => invoke<Record<string, string>>("ui_state_get_all"),
  zustandSchreiben: async (schluessel, wert) => {
    await invoke("ui_state_set", { key: schluessel, value: wert });
  },
  neueId: () => crypto.randomUUID(),
  warten: (ms) => new Promise((r) => setTimeout(r, ms)),
  jetzt: () => Date.now(),
  ohneEingang: !isTauri,
});

export function useBilder(): BilderZustand {
  return useSyncExternalStore(bilderStore.abonnieren, bilderStore.lesen, bilderStore.lesen);
}
