// „Zeiten gegen Flugplan" (13.10.2026): Plan aus dem SimBrief-OFP gegen die
// Ist-Zeiten — Kopfsatz, Zeitstrahl, Tabelle. Dazu die Kurzform für Listen.
// Keine Note, keine Ampelfarben; gerechnet wird nur in lib/flugzeiten.ts.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN).

import { useTranslation } from "react-i18next";
import type { LandingRecord } from "../lib/landungsDatensatz";
import {
  PUENKTLICH_BIS_MIN,
  dauern,
  dauerText,
  hatPlan,
  kopf,
  minutenText,
  uhrzeit,
  zeitachse,
  zeitZeilen,
  type FlugZeiten,
  type ZeitachsenBalken,
} from "../lib/flugzeiten";
import "./flugzeitenAbschnitt.css";

function Balken({ b }: { b: ZeitachsenBalken }) {
  return (
    <div
      className={`flugzeiten__balken flugzeiten__balken--${b.art}`}
      style={{ left: `${b.von * 100}%`, width: `${Math.max(b.bis - b.von, 0.002) * 100}%` }}
    />
  );
}

function Zeitstrahl({ z }: { z: FlugZeiten }) {
  const { t } = useTranslation();
  const achse = zeitachse(z);
  if (!achse) return null;
  return (
    <div className="flugzeiten__strahl" aria-hidden="true">
      <div className="flugzeiten__zeile flugzeiten__zeile--plan">
        <span className="flugzeiten__zeilenname">{t("flugzeiten.plan")}</span>
        <div className="flugzeiten__spur">
          {achse.plan.map((b, i) => (
            <Balken key={i} b={b} />
          ))}
        </div>
      </div>
      <div className="flugzeiten__zeile flugzeiten__zeile--ist">
        <span className="flugzeiten__zeilenname">{t("flugzeiten.ist")}</span>
        <div className="flugzeiten__spur">
          {achse.ist.map((b, i) => (
            <Balken key={i} b={b} />
          ))}
        </div>
      </div>
      <div className="flugzeiten__achse">
        <span />
        <div className="flugzeiten__striche">
          {achse.striche.map((s, i) => (
            <span key={s.sek} className={i % 2 === 1 ? "flugzeiten__strich--ungerade" : undefined} style={{ left: `${s.pos * 100}%` }}>
              {uhrzeit(s.sek)}
            </span>
          ))}
        </div>
      </div>
      <div className="flugzeiten__legende">
        <span><i className="flugzeiten__farbe--plan" />{t("flugzeiten.legende_plan")}</span>
        <span><i className="flugzeiten__farbe--ist" />{t("flugzeiten.ist")}</span>
        <span><i className="flugzeiten__farbe--rollen" />{t("flugzeiten.legende_rollen")}</span>
      </div>
    </div>
  );
}

/** Kopfsatz: „On-Block 7 min nach Plan · Blockzeit 4 min kürzer als geplant". */
function Kopfsatz({ z }: { z: FlugZeiten }) {
  const { t } = useTranslation();
  const k = kopf(z);
  if (!k) return null;
  const schritt = t(`flugzeiten.schritt.${k.schritt}`);
  const min = Math.abs(k.abw_min);
  const teil1 =
    k.abw_min > 0
      ? t("flugzeiten.kopf.nach_plan", { schritt, min })
      : k.abw_min < 0
        ? t("flugzeiten.kopf.vor_plan", { schritt, min })
        : t("flugzeiten.kopf.genau", { schritt });
  const d = k.block_diff_min;
  const teil2 =
    d == null
      ? null
      : d > 0
        ? t("flugzeiten.kopf.block_laenger", { min: d })
        : d < 0
          ? t("flugzeiten.kopf.block_kuerzer", { min: -d })
          : t("flugzeiten.kopf.block_gleich");
  // Wie Airlines zählen (A15) — nur für On-Block, nur als Text.
  const a15 =
    k.schritt === "on_block"
      ? k.abw_min <= PUENKTLICH_BIS_MIN
        ? t("flugzeiten.a15_puenktlich", { min: PUENKTLICH_BIS_MIN })
        : t("flugzeiten.a15_verspaetet", { min: PUENKTLICH_BIS_MIN })
      : null;
  return (
    <p className="flugzeiten__kopf">
      {teil1}
      {teil2 && <> · {teil2}</>}
      {a15 && <span className="flugzeiten__a15">{a15}</span>}
    </p>
  );
}

export function FlugzeitenAbschnitt({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();
  const z = record.flugzeiten;
  // Datensätze von vor dem 13.10.2026 kennen das Feld nicht — dann gar nichts.
  if (!z) return null;
  if (!hatPlan(z)) {
    return (
      <section className="landing-section flugzeiten" data-testid="flugzeiten">
        <h3>{t("flugzeiten.titel")}</h3>
        <p className="flugzeiten__leer">{t("flugzeiten.kein_plan")}</p>
      </section>
    );
  }
  const zeilen = zeitZeilen(z);
  const d = dauern(z);
  const dauerZeilen: { name: string; plan: number | null; ist: number | null }[] = [
    { name: t("flugzeiten.blockzeit"), plan: d.block_plan, ist: d.block_ist },
    { name: t("flugzeiten.flugzeit"), plan: d.flug_plan, ist: d.flug_ist },
  ].filter((r) => r.plan != null || r.ist != null);
  return (
    <section className="landing-section flugzeiten" data-testid="flugzeiten">
      <h3>{t("flugzeiten.titel")}</h3>
      <Kopfsatz z={z} />
      <Zeitstrahl z={z} />
      <div className="flugzeiten__tabelle-rahmen">
        <table className="flugzeiten__tabelle">
          <thead>
            <tr>
              <th />
              <th>{t("flugzeiten.plan")}</th>
              <th>{t("flugzeiten.ist")}</th>
              <th>
                <span className="flugzeiten__lang">{t("flugzeiten.abweichung")}</span>
                <span className="flugzeiten__kurz">{t("flugzeiten.abweichung_kurz")}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {zeilen.map((r) => (
              <tr key={r.schritt}>
                <td>{t(`flugzeiten.schritt.${r.schritt}`)}</td>
                <td className="flugzeiten__zahl">{uhrzeit(r.plan)}</td>
                <td className="flugzeiten__zahl">{uhrzeit(r.ist)}</td>
                <td className="flugzeiten__zahl flugzeiten__abw">{minutenText(r.abw_min)}</td>
              </tr>
            ))}
            {dauerZeilen.map((r, i) => (
              <tr key={r.name} className={i === 0 ? "flugzeiten__summe" : undefined}>
                <td>{r.name}</td>
                <td className="flugzeiten__zahl">{dauerText(r.plan)}</td>
                <td className="flugzeiten__zahl">{dauerText(r.ist)}</td>
                <td className="flugzeiten__zahl flugzeiten__abw">
                  {r.plan != null && r.ist != null ? minutenText(r.ist - r.plan) : "—"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="flugzeiten__fuss">{t("flugzeiten.fuss")}</p>
    </section>
  );
}

/** Kurzform für Listen: „On-Block +7 min" und „Block 1:06 / 1:10".
 *  `null`, wenn es nichts zu vergleichen gibt. */
export function FlugzeitenKurz({ z }: { z: FlugZeiten | null | undefined }) {
  const { t } = useTranslation();
  if (!z || !hatPlan(z)) return null;
  const k = kopf(z);
  const d = dauern(z);
  if (!k && (d.block_plan == null || d.block_ist == null)) return null;
  return (
    <span className="flugzeiten-kurz" data-testid="flugzeiten-kurz">
      {k && (
        <span className="flugzeiten-kurz__chip">
          <span className="flugzeiten-kurz__k">{t(`flugzeiten.schritt.${k.schritt}`)}</span>
          <span className="flugzeiten__zahl">{minutenText(k.abw_min)}</span>
        </span>
      )}
      {d.block_plan != null && d.block_ist != null && (
        <span className="flugzeiten-kurz__chip">
          <span className="flugzeiten-kurz__k">{t("flugzeiten.block_kurz")}</span>
          <span className="flugzeiten__zahl">
            {dauerText(d.block_ist)} / {dauerText(d.block_plan)}
          </span>
        </span>
      )}
    </span>
  );
}
