// Cockpit: „Sprit · Wegpunkt für Wegpunkt" live, direkt über der METAR-Karte.
//
// Holt die Tabelle alle zehn Sekunden vom Backend (`flight_sprit_wegpunkte`)
// — gerechnet wird dort, in derselben Funktion, die nach der Landung mit der
// Sprit-Auswertung einfriert. Zugeklappt ist der Standard; ob der Pilot sie
// aufgeklappt hat, merkt sich die Karte je Gerät.

import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";
import type { SpritWegpunkt } from "../lib/sprit";
import { SpritWegpunkte } from "./SpritWegpunkte";

interface Dto {
  zeilen: SpritWegpunkt[];
  naechster: number | null;
  naechster_nm: number | null;
}

const MERKER = "aeroacars.cockpit.sprit_wegpunkte_offen";

function gemerkt(): boolean {
  try {
    return localStorage.getItem(MERKER) === "1";
  } catch {
    return false;
  }
}

export function SpritWegpunkteLive() {
  const [daten, setDaten] = useState<Dto | null>(null);
  const [offen] = useState(gemerkt);

  useEffect(() => {
    let aktiv = true;
    const holen = () =>
      invoke<Dto>("flight_sprit_wegpunkte")
        .then((d) => {
          if (aktiv) setDaten(d);
        })
        .catch(() => {});
    holen();
    const id = window.setInterval(holen, 10_000);
    return () => {
      aktiv = false;
      window.clearInterval(id);
    };
  }, []);

  if (!daten || daten.zeilen.length < 2) return null;
  return (
    <div
      onClick={(e) => {
        // Den Zustand nach dem Umschalten merken — der Kopf ist ein Knopf
        // mit `aria-expanded`, dessen neuer Wert nach dem Klick dasteht.
        const knopf = (e.target as HTMLElement).closest("button[aria-expanded]");
        if (!knopf) return;
        window.setTimeout(() => {
          try {
            localStorage.setItem(MERKER, knopf.getAttribute("aria-expanded") === "true" ? "1" : "0");
          } catch {
            /* ohne Speicher eben nicht gemerkt */
          }
        }, 0);
      }}
    >
      <SpritWegpunkte zeilen={daten.zeilen} naechster={daten.naechster} naechsterNm={daten.naechster_nm} offen={offen} />
    </div>
  );
}
