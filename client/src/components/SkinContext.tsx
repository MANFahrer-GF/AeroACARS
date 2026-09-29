// SkinContext — React-Context der den aktuellen V2-Skin liefert.
//
// Lade-Strategie:
//   1. Beim Provider-Mount: bundled DEFAULT_SKIN sofort verwenden
//      (synchrones initial render — keine flash of unstyled content)
//   2. Im Hintergrund (await): Skin ueber `laden` holen
//   3. Bei Erfolg: localStorage cachen + State aktualisieren
//   4. Bei Fehler (offline, nicht angemeldet, keine Skin hinterlegt): aus
//      localStorage Cache laden (falls vorhanden), sonst DEFAULT_SKIN
//
// Diese Datei ist in Client und Webapp BYTEGLEICH (scripts/anzeige-sync.mjs).
// Wie geladen wird, bestimmt deshalb der Aufrufer (`laden`): Seit 29.09.2026
// gibt der Server die Skin nur noch GSG-Piloten und dem Admin. Der Client
// holt sie ueber Rust mit dem Pilot-Token (`GsgSkinProvider`), die Webapp
// per `fetch` am eigenen Server mit dem Admin-Cookie (Vorgabe hier).

import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import {
  DEFAULT_SKIN,
  mergeWithDefaults,
  type V2Skin,
} from "./runwayV2Skin";

const CACHE_KEY = "aeroacars.v2-skin.cache.v1";

const SkinContext = createContext<V2Skin>(DEFAULT_SKIN);

export function useV2Skin(): V2Skin {
  return useContext(SkinContext);
}

/** Holt die Skin; wirft bei jedem Fehler (auch 401/404). */
export type SkinLaden = (signal: AbortSignal) => Promise<Partial<V2Skin>>;

interface SkinProviderProps {
  children: ReactNode;
  /** Wie die Skin geholt wird. Ohne Angabe: `fetch(endpoint)`. */
  laden?: SkinLaden;
  /** Aendert sich dieser Wert, wird neu geladen (Client: nach der Anmeldung). */
  neuLaden?: unknown;
  /** Nur fuer die Vorgabe ohne `laden`: Adresse fuer `fetch`. */
  endpoint?: string;
}

function perFetch(endpoint: string): SkinLaden {
  return async (signal) => {
    const r = await fetch(endpoint, {
      signal,
      headers: { Accept: "application/json" },
    });
    // 404 = noch keine Skin hinterlegt, 401 = nicht angemeldet — beides
    // kein Fehler fuer die Anzeige, es bleibt beim Cache/Default.
    if (!r.ok) throw new Error(`v2-skin HTTP ${r.status}`);
    return (await r.json()) as Partial<V2Skin>;
  };
}

export function SkinProvider({
  children,
  laden,
  neuLaden,
  endpoint = "/api/v2-skin",
}: SkinProviderProps) {
  // Initial: Cache aus localStorage probieren, sonst DEFAULT.
  const [skin, setSkin] = useState<V2Skin>(() => {
    try {
      const cached = localStorage.getItem(CACHE_KEY);
      if (cached) {
        const parsed = JSON.parse(cached) as Partial<V2Skin>;
        return mergeWithDefaults(parsed);
      }
    } catch {
      /* localStorage-Fehler ignorieren, fallback auf default */
    }
    return DEFAULT_SKIN;
  });

  useEffect(() => {
    const ac = new AbortController();
    const holen = laden ?? perFetch(endpoint);
    (async () => {
      try {
        const json = await holen(ac.signal);
        const merged = mergeWithDefaults(json);
        setSkin(merged);
        try {
          localStorage.setItem(CACHE_KEY, JSON.stringify(merged));
        } catch {
          /* quota-error o.ä. — kein Caching, kein Drama */
        }
      } catch (err) {
        if ((err as Error)?.name === "AbortError") return;
        // Offline, nicht angemeldet oder keine Skin hinterlegt → bleib
        // beim Cache/Default.
        console.info("v2-skin nicht abrufbar (Cache/Default):", err);
      }
    })();
    return () => ac.abort();
    // `laden` ist beim Aufrufer eine feste Funktion; neu geladen wird ueber
    // `neuLaden` bzw. eine geaenderte Adresse.
  }, [endpoint, neuLaden]);

  return <SkinContext.Provider value={skin}>{children}</SkinContext.Provider>;
}
