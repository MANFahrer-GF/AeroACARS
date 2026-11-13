// Bordbuch im Reiter „Flug“ (13.11.2026): die Checkliste des laufenden Flugs,
// ohne in den Logbuch-Reiter zu wechseln. Seit dem Wegfall des eigenen
// Bordbuch-Reiters (c2f28878) stand sie nur noch zugeklappt im Logbuch.
// Standardmäßig offen, zuklappbar; der Stand („7 von 12“) steht auch
// zugeklappt im Kopf. Nichts zu markieren — das geht erst nach dem Flug.

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, listen } from "../../lib/ipc";
import { bilanz, sichtbar, type LiveAnsicht } from "../../lib/bordbuch";
import { BordbuchCheckliste } from "./BordbuchCheckliste";
import "../flugView.css";
import "./bordbuch.css";

export const BORDBUCH_IM_FLUG_OFFEN_KEY = "aeroacars.flug.bordbuchOffen";
const TAKT_MS = 4000;

function anfangsOffen(): boolean {
  try {
    return localStorage.getItem(BORDBUCH_IM_FLUG_OFFEN_KEY) !== "0";
  } catch {
    return true;
  }
}

export function BordbuchImFlug({ aktiv }: { aktiv: boolean }) {
  const { t } = useTranslation();
  const [live, setLive] = useState<LiveAnsicht | null>(null);
  const [offen, setOffen] = useState(anfangsOffen);

  const laden = useCallback(async () => {
    try {
      setLive(await invoke<LiveAnsicht | null>("bordbuch_live"));
    } catch (e) {
      console.warn("bordbuch_live", e);
    }
  }, []);

  useEffect(() => {
    if (!aktiv) {
      setLive(null);
      return;
    }
    void laden();
    const id = window.setInterval(() => void laden(), TAKT_MS);
    let lebt = true;
    let weg: (() => void) | undefined;
    void listen("bordbuch_geaendert", () => void laden())
      .then((u) => {
        if (lebt) weg = u;
        else u();
      })
      .catch(() => undefined);
    return () => {
      lebt = false;
      window.clearInterval(id);
      weg?.();
    };
  }, [aktiv, laden]);

  // Abgeschaltet (z. B. GA) oder noch keine sichtbaren Punkte: kein leerer Kasten im Flug.
  if (!aktiv || !live || live.aus_grund) return null;
  if (!live.punkte.some((p) => sichtbar(p, live.eingeschaltet))) return null;
  const b = bilanz(live.punkte, live.eingeschaltet);

  return (
    <details
      className="flug-briefing flug-bordbuch"
      data-testid="flug-bordbuch"
      open={offen}
      onToggle={(e) => {
        const jetzt = (e.currentTarget as HTMLDetailsElement).open;
        setOffen(jetzt);
        try {
          localStorage.setItem(BORDBUCH_IM_FLUG_OFFEN_KEY, jetzt ? "1" : "0");
        } catch {
          /* Private-Mode o.ä. — Zuklappen geht, wird nur nicht gemerkt. */
        }
      }}
    >
      <summary className="flug-briefing__kopf">
        <span className="flug-briefing__titel">{t("bordbuch.titel")}</span>
        {b.von > 0 && <span className="flug-briefing__inhalt">{t("bordbuch.bilanz", { ok: b.ok, von: b.von })}</span>}
      </summary>
      {offen && (
        <div className="flug-briefing__koerper">
          <BordbuchCheckliste
            punkte={live.punkte}
            eingeschaltet={live.eingeschaltet}
            flugzeug={null}
            rolltempoGrenzeKt={live.rolltempo_grenze_kt}
          />
        </div>
      )}
    </details>
  );
}
