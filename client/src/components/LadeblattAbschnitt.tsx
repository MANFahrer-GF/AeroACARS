// Treibstoff und Gewicht: Sprit-Auswertung (keine Note) und Soll/Ist-Tabellen.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 in LandingPanel.tsx; die Webapp zeigte nur die Sprit-Auswertung,
// LDW und Landetreibstoff — ohne Plan, ohne Block, TOW, ZFW und Trip.

import { useTranslation } from "react-i18next";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { SpritSektion } from "./SpritSektion";
import { InfoBadge } from "./InfoBadge";
import { fmtNumber } from "../lib/landungsFormat";
import "./ladeblattAbschnitt.css";

// ---- Fuel comparison bar ------------------------------------------------

function FuelComparisonBar({
  plan,
  actual,
}: {
  plan: number;
  actual: number;
}) {
  const { t } = useTranslation();
  const max = Math.max(plan, actual, 1);
  const planPct = (plan / max) * 100;
  const actualPct = (actual / max) * 100;
  const diff = actual - plan;
  const sign = diff >= 0 ? "+" : "";
  const pct = (diff / Math.max(1, plan)) * 100;

  return (
    <div className="landing-fuelbar">
      <div className="landing-fuelbar__row">
        <span className="landing-fuelbar__label">{t("landing.plan_burn")}</span>
        <div className="landing-fuelbar__track">
          <div
            className="landing-fuelbar__fill landing-fuelbar__fill--plan"
            style={{ width: `${planPct}%` }}
          />
        </div>
        <span className="landing-fuelbar__value">{plan.toFixed(0)} kg</span>
      </div>
      <div className="landing-fuelbar__row">
        <span className="landing-fuelbar__label">{t("landing.actual_burn")}</span>
        <div className="landing-fuelbar__track">
          <div
            className={`landing-fuelbar__fill ${
              diff > 0
                ? "landing-fuelbar__fill--over"
                : "landing-fuelbar__fill--under"
            }`}
            style={{ width: `${actualPct}%` }}
          />
        </div>
        <span className="landing-fuelbar__value">{actual.toFixed(0)} kg</span>
      </div>
      {/* v0.11.0-dev: Delta-Pill statt versteckter Mini-Text. Farbcode:
          ±1% grün (im Rahmen) · 1–5% gelb · >5% rot — Symbolik klar
          unterscheidbar zwischen ok/warn/alert ohne nur auf Farbe zu
          setzen (auch für Color-Blind-Piloten lesbar). */}
      {(() => {
        const absPct = Math.abs(pct);
        const deltaColor =
          absPct < 1 ? "#22c55e" : absPct < 5 ? "#eab308" : "#ef4444";
        const deltaIcon =
          absPct < 1 ? "✓" : absPct < 5 ? "≈" : diff > 0 ? "▲" : "▼";
        return (
          <div
            style={{
              display: "flex",
              justifyContent: "flex-end",
              marginTop: 4,
            }}
          >
            <span
              className="farbwert"
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                padding: "3px 10px",
                borderRadius: 4,
                background: `${deltaColor}1a`,
                border: `1px solid ${deltaColor}55`,
                color: deltaColor,
                fontSize: "0.82rem",
                fontWeight: 600,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              <span>{deltaIcon}</span>
              <span>
                {sign}
                {diff.toFixed(0)} kg
              </span>
              <span style={{ opacity: 0.75 }}>
                ({sign}
                {pct.toFixed(1)}%)
              </span>
            </span>
          </div>
        );
      })()}
    </div>
  );
}

// ---- Soll/Ist-Vergleichstabelle (v0.3.0) ---------------------------------
//
// Drei Spalten: IST | SOLL | Δ. Zeilen werden nur gerendert wenn IST oder
// SOLL vorhanden ist — leere Zeilen kommen NICHT in die Tabelle. Δ wird mit
// Farbcode versehen: grün <1%, gelb 1-3%, rot >3% Abweichung. Bei Weight
// gilt "drüber Plan = warnung", bei Fuel gilt "stark unter Plan = warnung
// (zu wenig getankt)".

interface ComparisonRow {
  label: string;
  ist: number | null;
  soll: number | null;
}

function ComparisonTable({
  title,
  icon,
  rows,
}: {
  title: string;
  /** Optional Emoji/Icon das vor dem Section-Titel rendert (z.B. ⛽ ⚖) */
  icon?: string;
  rows: ComparisonRow[];
}) {
  // v0.11.0-dev Polish-Pass 4: kompletter Re-Design weg von der „Card-im-
  // Card-Klotz"-Optik hin zu einer schlanken Liste. Pilot-Feedback Pass 3:
  // „wirkt nicht modern, das ist ein großer Klotz". Ursache war: doppelte
  // Borders (parent landing-section + eigene Card), redundante SOLL-Spalte,
  // viele schwere Elemente nebeneinander.
  //
  // Neuer Look:
  // - KEINE eigene Card-Hülle mehr (transparent, fließt in die parent-
  //   landing-section ein — keine doppelten Borders)
  // - SOLL-Spalte aufgelöst → wird zur dezenten Sub-Zeile unter dem IST-
  //   Wert wenn Δ != 0 ("vs 13.884 kg"); spart eine ganze Spalte
  // - Mehr vertikales Spacing pro Zeile (Werte atmen)
  // - Δ-Pill bleibt rechts als visueller Anker, sonst alles ruhig
  // - Dünne Trenn-Linie zwischen Zeilen statt Zebra-Stripe-Background
  // v0.11.0-dev (Polish-Pass 2, Pass 3 fix): modernerer Look ohne Bruch
  // mit dem dark-Theme. Änderungen ggü. Pass 1:
  // - Section-Header bekommt optionales Icon (⛽ Treibstoff, ⚖ Gewicht)
  // - Δ-Pills sind rounded-full (rounded-999) statt rechteckig
  // - IST-Wert eine Stufe größer (1 rem statt 0,95)
  // - Hover-State auf den Zeilen (subtle Brightness-Heben)
  //
  // Pass-3-Fix: die Mini-Δ-Progress-Bar am unteren Rand der Zeile war
  // ein Pass-2-Experiment — der Pilot fand sie verwirrend („grüner Balken
  // lang heißt was?"), weil die Bar das Δ-Ausmaß codierte aber farblich
  // mit der ok/warn/alert-Pill kollidierte. Pill rechts sagt schon alles —
  // Mini-Bar wieder entfernt.
  const visible = rows.filter((r) => r.ist != null || r.soll != null);
  if (visible.length === 0) return null;
  return (
    <div
      style={{
        background: "color-mix(in srgb, var(--text) 2%, transparent)",
        border: "1px solid color-mix(in srgb, var(--text) 8%, transparent)",
        borderRadius: 12,
        padding: "14px 16px 8px 16px",
      }}
    >
      {/* Section-Header — schlank, im Card-Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          fontSize: "0.72rem",
          fontWeight: 700,
          color: "var(--text-muted)",
          textTransform: "uppercase",
          letterSpacing: "0.1em",
          paddingBottom: 8,
          marginBottom: 2,
          borderBottom: "1px solid rgba(255,255,255,0.05)",
        }}
      >
        {icon && (
          <span style={{ fontSize: "0.85rem", opacity: 0.7 }}>{icon}</span>
        )}
        <span>{title}</span>
      </div>

      {/* Datenliste — jede Zeile mit dünner Trennlinie nach oben.
          Großzügiges padding für Atemraum. */}
      <div>
        {visible.map((r, idx) => {
          const delta =
            r.ist != null && r.soll != null ? r.ist - r.soll : null;
          const deltaPct =
            delta != null && r.soll != null && r.soll !== 0
              ? Math.abs(delta / r.soll) * 100
              : null;

          const deltaColor =
            deltaPct == null
              ? "rgba(255,255,255,0.35)"
              : deltaPct < 5
                ? "#22c55e"
                : deltaPct < 10
                  ? "#eab308"
                  : "#ef4444";

          const deltaIcon =
            delta == null
              ? ""
              : deltaPct! < 1
                ? "✓"
                : deltaPct! < 5
                  ? "≈"
                  : delta > 0
                    ? "▲"
                    : "▼";

          // SOLL als Sub-Zeile nur zeigen wenn IST ≠ SOLL (= Δ exists und
          // != 0). Bei exaktem Match (oder fehlendem SOLL) keine Sub-Zeile,
          // damit die Liste ruhig bleibt.
          const showSollSubline =
            r.soll != null && delta != null && delta !== 0;

          return (
            <div
              key={r.label}
              style={{
                display: "grid",
                gridTemplateColumns: "1fr auto auto",
                columnGap: 16,
                rowGap: 2,
                alignItems: "baseline",
                padding: "12px 4px",
                borderTop:
                  idx === 0
                    ? "none"
                    : "1px solid color-mix(in srgb, var(--text) 8%, transparent)",
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {/* Label */}
              <span
                style={{
                  color: "var(--text-muted)",
                  fontSize: "0.86rem",
                  fontWeight: 500,
                  gridRow: "1 / 2",
                }}
              >
                {r.label}
              </span>

              {/* IST-Wert (primary, prominent) */}
              <span
                style={{
                  textAlign: "right",
                  fontSize: "1.02rem",
                  fontWeight: 600,
                  color: "var(--text)",
                  letterSpacing: "-0.01em",
                  gridRow: "1 / 2",
                }}
              >
                {r.ist != null ? fmtNumber(r.ist, 0, "kg") : "—"}
              </span>

              {/* Δ-Pill (oder em-dash bei fehlenden Daten) */}
              <span style={{ textAlign: "right", gridRow: "1 / 2" }}>
                {delta != null ? (
                  <span
                    className="farbwert"
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 5,
                      padding: "2px 9px",
                      borderRadius: 999,
                      background: `${deltaColor}18`,
                      color: deltaColor,
                      fontSize: "0.76rem",
                      fontWeight: 600,
                    }}
                  >
                    <span style={{ fontSize: "0.7rem" }}>{deltaIcon}</span>
                    <span>
                      {delta >= 0 ? "+" : ""}
                      {delta.toFixed(0)} kg
                    </span>
                  </span>
                ) : (
                  <span style={{ opacity: 0.3, fontSize: "0.86rem" }}>—</span>
                )}
              </span>

              {/* SOLL-Sub-Zeile (nur wenn signifikant) — direkt unter dem
                  IST-Wert. v0.11.0-dev Polish-Pass 6: Kontrast deutlich
                  hoch (Pilot-Feedback „vs ist schwer zu erkennen"). „vs"
                  bleibt blass als Label, der Zahlenwert ist gut lesbar. */}
              {showSollSubline && (
                <span
                  style={{
                    gridColumn: "2 / 3",
                    textAlign: "right",
                    fontSize: "0.78rem",
                    gridRow: "2 / 3",
                    fontVariantNumeric: "tabular-nums",
                  }}
                >
                  <span style={{ color: "var(--text-muted)", marginRight: 4 }}>
                    Plan
                  </span>
                  <span style={{ color: "var(--text)", fontWeight: 500 }}>
                    {fmtNumber(r.soll!, 0, "kg")}
                  </span>
                </span>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** Fuel + Weight — Soll/Ist-Vergleich (v0.3.0). Sichtbar, sobald die
 *  Sprit-Auswertung oder irgendein Treibstoff- oder Gewichtswert vorliegt. */
export function LadeblattAbschnitt({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();
  if (
    !(record.sprit != null ||
      record.planned_burn_kg != null ||
      record.actual_trip_burn_kg != null ||
      record.block_fuel_kg != null ||
      record.takeoff_fuel_kg != null ||
      record.landing_fuel_kg != null ||
      record.takeoff_weight_kg != null ||
      record.landing_weight_kg != null)
  ) {
    return null;
  }
  return (
    <section className="landing-section">
      <h3>
        {t("landing.loadsheet_section_title")}
        <InfoBadge explanation={t("landing.info.fuel_section")} />
      </h3>
      {/* v0.4.2: Hinweis wenn der Flug keinen SimBrief-OFP hatte —
          dann sind alle SOLL-Spalten in den Tabellen unten leer.
          Banner erklärt klar warum, statt nur ratlose Striche. */}
      {record.planned_block_fuel_kg == null &&
        record.planned_burn_kg == null &&
        record.planned_tow_kg == null &&
        record.planned_ldw_kg == null &&
        record.planned_zfw_kg == null && (
          <div className="landing-no-plan-hint" role="note">
            ℹ️ {t("landing.no_plan_hint")}
          </div>
        )}
      {/* v1.7.35: Sprit-Auswertung ohne Note. Der alte Plan/Ist-Balken
          bleibt nur fuer Datensaetze ohne Auswertung (vor v1.7.35). */}
      <SpritSektion sprit={record.sprit} />
      {!record.sprit && record.planned_burn_kg != null && record.actual_trip_burn_kg != null && (
        <FuelComparisonBar
          plan={record.planned_burn_kg}
          actual={record.actual_trip_burn_kg}
        />
      )}
      {/* v0.11.0-dev Pass 5: Treibstoff + Gewicht nebeneinander als
          2-Spalten-Grid auf breiten Screens (≥ 720 px Card-Breite,
          CSS minmax sorgt für auto-fit). Auf schmalen Screens
          stapeln sich die zwei Cards automatisch untereinander. So
          entsteht klarer Rhythmus statt einer endlosen vertikalen
          Liste. Score-Version 19: die frühere „Plan-Treue"-Karte mit
          eigener 0–100-Zahl ist entfallen — sie war keine Note der
          Bewertung, sah aber wie eine aus; die Abweichungen stehen in
          den Tabellen hier. */}
      <div
        style={{
          display: "grid",
          // min(): schmaler als 340 px (iPhone) eine Spalte statt Überlauf.
          gridTemplateColumns: "repeat(auto-fit, minmax(min(340px, 100%), 1fr))",
          gap: 14,
          marginTop: "1rem",
        }}
      >
        <ComparisonTable
          title={t("landing.fuel_table")}
          icon="⛽"
          rows={[
            {
              label: t("landing.block_fuel"),
              ist: record.block_fuel_kg,
              soll: record.planned_block_fuel_kg,
            },
            {
              label: t("landing.takeoff_fuel"),
              ist: record.takeoff_fuel_kg,
              soll: null, // SimBrief OFP hat nur Block + Burn, kein TO-Fuel separat
            },
            {
              label: t("landing.landing_fuel"),
              ist: record.landing_fuel_kg,
              soll:
                record.planned_block_fuel_kg != null && record.planned_burn_kg != null
                  ? record.planned_block_fuel_kg - record.planned_burn_kg
                  : null,
            },
            {
              label: t("landing.trip_burn"),
              ist: record.actual_trip_burn_kg,
              soll: record.planned_burn_kg,
            },
          ]}
        />
        <ComparisonTable
          title={t("landing.weight_table")}
          icon="⚖️"
          rows={[
            {
              label: t("landing.tow"),
              ist: record.takeoff_weight_kg,
              soll: record.planned_tow_kg,
            },
            {
              label: t("landing.ldw"),
              ist: record.landing_weight_kg,
              soll: record.planned_ldw_kg,
            },
            {
              label: t("landing.zfw"),
              ist:
                record.takeoff_weight_kg != null && record.takeoff_fuel_kg != null
                  ? record.takeoff_weight_kg - record.takeoff_fuel_kg
                  : null,
              soll: record.planned_zfw_kg,
            },
          ]}
        />
      </div>
    </section>
  );
}
