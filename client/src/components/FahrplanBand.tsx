// Fahrplan-Band im Cockpit (15.10.2026, Thomas: „Band plus Trip und Uhr in
// einer Karte … den Piloten dahin bringen, dass er sich an die Zeiten hält").
//
// Ein reiner Zeitstrahl: Off-Block → Abheben → Aufsetzen → On-Block, je mit
// Planzeit, Ist- oder erwarteter Zeit und der Abweichung. Die EINE Zahl, die
// zählt (erwartetes On-Block bzw. Off-Block-Countdown), steht nicht hier,
// sondern fett in der Trip-Karte (`InfoStrip`) — keine Zahl doppelt.
//
// Farbe nur an der Abweichung, nach `abweichungTon`; nie in die Note.
// Gerechnet wird nur in lib/flugzeiten.ts.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ActiveFlightInfo } from "../types";
import {
  SCHRITTE,
  abflugSchaetzung,
  abweichungMin,
  abweichungTon,
  cockpitSchaetzung,
  hatPlan,
  ist,
  plan,
  planPasst,
  rollzeitNachLandungS,
  uhrzeit,
  type FlugZeiten,
  type ZeitSchritt,
} from "../lib/flugzeiten";
import "./fahrplanBand.css";

/** Die Uhr fürs Cockpit: alle 15 s neu, das genügt für Minuten. */
export function useJetztSek(): number {
  const [jetzt, setJetzt] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const id = setInterval(() => setJetzt(Date.now() / 1000), 15_000);
    return () => clearInterval(id);
  }, []);
  return jetzt;
}

/** Ortszeit des Piloten (Zeitzone seines Rechners), „20:40" — Thomas
 *  15.10.2026: „dann muss man nicht immer umrechnen". `zeitzone` nur für
 *  Tests; im Betrieb gilt die des Rechners. */
export function lokalzeit(sek: number | null | undefined, zeitzone?: string): string {
  if (sek == null || !Number.isFinite(sek)) return "—";
  return new Intl.DateTimeFormat("de-DE", {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    timeZone: zeitzone,
  }).format(new Date(sek * 1000));
}

/** „+16", „−4", „±0" — kurz, damit es unter den Stationen Platz hat. */
export function kurzMinuten(min: number): string {
  if (min === 0) return "±0";
  return `${min > 0 ? "+" : "−"}${Math.abs(min)}`;
}

export interface BandStation {
  schritt: ZeitSchritt;
  plan: number | null;
  /** Tatsächlich (gefüllter Punkt). */
  ist: number | null;
  /** Erwartet, solange noch nicht erreicht. */
  erwartet: number | null;
}

/**
 * Die vier Stationen mit Plan, Ist und Erwartung.
 *
 * Erwartung: Am Gate nur der Off-Block, und nur wenn er schon überfällig ist
 * (vorher ist „erwartet" gleich Plan und sagt nichts). Ab dem Abrollen kommen
 * Aufsetzen und On-Block aus `cockpitSchaetzung` (das Neueste, was feststeht,
 * gewinnt), das Abheben aus Off-Block plus geplanter Rollzeit.
 */
export function bandStationen(
  z: FlugZeiten,
  landungErwartet: number | null | undefined,
  jetzt: number,
): BandStation[] {
  const erw: Partial<Record<ZeitSchritt, number>> = {};
  const abflug = abflugSchaetzung(z, jetzt);
  if (abflug) {
    if (abflug.gegen_plan_min > 0) erw.off_block = abflug.erwartet;
  } else {
    const s = cockpitSchaetzung(z, landungErwartet);
    if (s) {
      erw.on_block = s.erwartet;
      const rollen = rollzeitNachLandungS(z);
      if (rollen != null) erw.aufsetzen = s.erwartet - rollen;
    }
    const off = ist(z, "off_block");
    const pOff = plan(z, "off_block");
    const pAb = plan(z, "abheben");
    if (off != null && pOff != null && pAb != null && pAb >= pOff) {
      erw.abheben = off + (pAb - pOff);
    }
  }
  return SCHRITTE.map((schritt) => {
    const i = ist(z, schritt);
    return {
      schritt,
      plan: plan(z, schritt),
      ist: i,
      erwartet: i == null ? (erw[schritt] ?? null) : null,
    };
  });
}

/** Wo das Flugzeug steht: 0–1 entlang gleich verteilter Stationen, `null`
 *  vor dem Abrollen. Innerhalb eines Abschnitts zeitgerecht. */
export function bandFortschritt(st: BandStation[], jetzt: number): number | null {
  const n = st.length - 1;
  if (st[0]?.ist == null) return null;
  for (let i = 0; i < n; i++) {
    const a = st[i]!;
    const b = st[i + 1]!;
    if (b.ist != null) continue;
    const von = a.ist ?? a.erwartet ?? a.plan;
    // Ende des Abschnitts ist die Erwartung. Dauert etwa das Rollen länger
    // als geplant, bleibt das Flugzeug am nächsten Punkt stehen, bis er
    // tatsächlich erreicht ist — es läuft nie über einen offenen Punkt.
    const bis = b.erwartet ?? b.plan;
    if (von == null || bis == null || bis <= von) return i / n;
    const anteil = Math.min(1, Math.max(0, (jetzt - von) / (bis - von)));
    return (i + anteil) / n;
  }
  return 1;
}

function Flugzeug() {
  // Material „flight" (Apache 2.0), nach rechts gedreht. Ein SVG statt „✈":
  // Das Zeichen fehlt der Cockpit-Schrift, es spränge eine fremde ein.
  return (
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" style={{ transform: "rotate(90deg)" }}>
      <path
        fill="currentColor"
        d="M21 16v-2l-8-5V3.5a1.5 1.5 0 0 0-3 0V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5l8 2.5z"
      />
    </svg>
  );
}

/** `jetzt`: dieselbe Uhrzeit wie die Trip-Karte (aus dem Panel) — sonst
 *  könnten Band und Karte am geplanten Off-Block bis zu 15 s lang
 *  Verschiedenes sagen. Ohne Angabe die eigene Uhr (Vorschau, Tests). */
export function FahrplanBand({ info, jetzt: jetztVonAussen }: { info: ActiveFlightInfo; jetzt?: number }) {
  const { t } = useTranslation();
  const eigeneUhr = useJetztSek();
  const jetzt = jetztVonAussen ?? eigeneUhr;
  const z = info.flugzeiten;
  if (!z || !hatPlan(z) || !planPasst(z, jetzt)) return null;
  const stationen = bandStationen(z, info.landung_erwartet, jetzt);
  const fortschritt = bandFortschritt(stationen, jetzt);
  const n = stationen.length - 1;
  const links = (p: number) => `calc(var(--fb-rand) + (100% - 2 * var(--fb-rand)) * ${p.toFixed(4)})`;
  return (
    <div className="card fahrplan-band" data-testid="fahrplan-band">
      {/* Jetzt in UTC und in der Ortszeit des Piloten — der Versatz auf einen
          Blick, ohne Umrechnen. */}
      <div className="fahrplan-band__uhr" data-testid="band-uhr">
        {t("flugzeiten.band.jetzt")} {uhrzeit(jetzt)} / {t("flugzeiten.band.lokal")} {lokalzeit(jetzt)}
      </div>
      <div className="fahrplan-band__bahn">
        <div className="fahrplan-band__linie" />
        {fortschritt != null && fortschritt > 0 && (
          <div
            className="fahrplan-band__linie fahrplan-band__linie--geflogen"
            style={{ width: `calc((100% - 2 * var(--fb-rand)) * ${fortschritt.toFixed(4)})` }}
          />
        )}
        {stationen.map((s, i) => {
          const zeit = s.ist ?? s.erwartet;
          const d = abweichungMin(s.plan, zeit);
          const ton = abweichungTon(d);
          return (
            <div
              key={s.schritt}
              className="fahrplan-band__station"
              style={{ left: links(i / n) }}
              data-testid={`band-${s.schritt}`}
            >
              <div className="fahrplan-band__name">{t(`flugzeiten.schritt.${s.schritt}`)}</div>
              <div className="fahrplan-band__plan">
                {t("flugzeiten.plan")} {uhrzeit(s.plan)}
              </div>
              <div
                className={`fahrplan-band__punkt${s.ist != null ? " fahrplan-band__punkt--erreicht" : ""}`}
              />
              <div className="fahrplan-band__zeit">
                {zeit != null ? (
                  <>
                    {s.ist == null && <span className="fahrplan-band__erw">{t("flugzeiten.band.erwartet_kurz")} </span>}
                    {uhrzeit(zeit)}
                    {d != null && (
                      <span className={`fahrplan-band__delta${ton ? ` fahrplan-band__delta--${ton}` : ""}`}>
                        {" "}
                        {kurzMinuten(d)}
                      </span>
                    )}
                  </>
                ) : (
                  " "
                )}
              </div>
            </div>
          );
        })}
        {fortschritt != null && (
          <div className="fahrplan-band__flugzeug" style={{ left: links(fortschritt) }}>
            <Flugzeug />
          </div>
        )}
      </div>
    </div>
  );
}
