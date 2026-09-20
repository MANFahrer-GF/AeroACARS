// Sprit · Wegpunkt für Wegpunkt — SimBrief an Bord gegen den Tankstand beim
// Überflug, mit Hochrechnung auf die Landung.
//
// Eine Anzeige für zwei Orte: im Cockpit live (über der METAR-Karte) und in
// der Landeauswertung, im Client und auf der Live-Seite. Gerechnet wird
// NICHTS hier — Zeilen, Hochrechnung und Ampel kommen aus
// `landing_scoring::sprit::wegpunkte_auswerten` (Rust). Zwei Rechnungen
// derselben Frage laufen auseinander; das hat dieses Projekt mehrfach
// bezahlt.
//
// # Farben
//
// Thomas (19.09.2026) wollte Farbe — aber keine Note. Grün/Gelb/Rot sagen,
// wie viel Luft im Tank bleibt, hochgerechnet auf die Landung (Fuel-Check wie
// im echten Flugbetrieb): grün = im Plan (die Contingency deckt), gelb = die
// Contingency wird aufgebraucht, rot = unter dem OFP-Minimum. Weniger
// verbraucht ist immer grün.

import { useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { kg, type SpritAmpel, type SpritWegpunkt } from "../lib/sprit";

export type { SpritAmpel, SpritWegpunkt };


export const AMPEL_FARBE: Record<SpritAmpel, string> = {
  gruen: "#3ecf8e",
  gelb: "#f2b24c",
  rot: "#ff6b6b",
};

const TEXT = "var(--text, #e8ecf3)";
const LEISE = "var(--text-muted, #9aa4b2)";
const LINIE = "var(--border, #2a3344)";

function uhrzeit(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) return "";
  const d = new Date(ms);
  return `${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}z`;
}

/**
 * Die Zeile, auf die sich der Verbrauch bezieht: die erste mit Ist UND Plan.
 * Normalerweise der Abflug, beim Einstieg in der Luft der erste Überflug
 * danach — dieselbe Zeile, auf die auch die Hochrechnung aufsetzt.
 */
function bezugszeile(zeilen: SpritWegpunkt[]): SpritWegpunkt | null {
  return (
    zeilen.find(
      (z) => z.ist_an_bord_kg != null && z.plan_an_bord_kg != null && z.zustand === "gemessen",
    ) ?? null
  );
}

/**
 * Mehrverbrauch gegenüber Plan in kg (positiv = mehr verbraucht).
 *
 * VERBRAUCH, nicht Tankstand: Wer 292 kg mehr getankt hat als geplant,
 * trägt diesen Vorsprung sonst durch alle Zeilen, und die Spalte meldet
 * „weniger verbraucht", während die Ampel rot ist. Genau so stand es bei
 * DLH #1439 (20.09.2026) im Bild — Text und Farbe widersprachen sich.
 */
function mehrverbrauch(z: SpritWegpunkt, bezug: SpritWegpunkt | null): number | null {
  if (z.ist_an_bord_kg == null || z.plan_an_bord_kg == null) return null;
  if (bezug?.ist_an_bord_kg == null || bezug.plan_an_bord_kg == null) return null;
  const ist = bezug.ist_an_bord_kg - z.ist_an_bord_kg;
  const plan = bezug.plan_an_bord_kg - z.plan_an_bord_kg;
  return ist - plan;
}

/** Wie viel mehr (positiv) oder weniger getankt wurde als geplant. */
function tankdifferenz(z: SpritWegpunkt): number | null {
  if (z.ist_an_bord_kg == null || z.plan_an_bord_kg == null) return null;
  return z.ist_an_bord_kg - z.plan_an_bord_kg;
}

function Punkt({ ampel, blass }: { ampel?: SpritAmpel | null; blass?: boolean }) {
  return (
    <span
      style={{
        display: "inline-block",
        width: 9,
        height: 9,
        borderRadius: "50%",
        marginRight: 8,
        flexShrink: 0,
        background: ampel ? AMPEL_FARBE[ampel] : "transparent",
        border: ampel ? "none" : `1px solid ${LINIE}`,
        opacity: blass ? 0.5 : 1,
      }}
    />
  );
}

export interface SpritWegpunkteProps {
  zeilen: SpritWegpunkt[] | null | undefined;
  /** Nur live: der nächste noch nicht überflogene Wegpunkt. */
  naechster?: number | null;
  naechsterNm?: number | null;
  /** Aufgeklappt beginnen (Landeauswertung) oder zugeklappt (Cockpit). */
  offen?: boolean;
  /** Ohne eigenen Rahmen — wenn die Umgebung schon eine Karte ist. */
  ohneRahmen?: boolean;
}

export function SpritWegpunkte(p: SpritWegpunkteProps) {
  const { t } = useTranslation();
  const [offen, setOffen] = useState(p.offen ?? false);
  const zeilen = p.zeilen ?? [];
  if (zeilen.length < 2) return null;

  const bezug = bezugszeile(zeilen);
  const letzte = [...zeilen].reverse().find((z) => z.zustand === "gemessen");
  const naechster = p.naechster != null ? zeilen[p.naechster] : null;
  const mv = letzte ? mehrverbrauch(letzte, bezug) : null;

  const mvText = (m: number | null, ungefaehr = false) =>
    m == null
      ? "—"
      : Math.abs(m) < 0.5
        ? t("landing.sprit.wp_wie_geplant")
        : // Zwei ausgeschriebene Aufrufe statt eines Schlüssels aus einer
          // Bedingung: Der Abgleich mit der Webapp findet nur ausgeschriebene
          // Schlüssel — dort stand sonst „landing.sprit.wp_mehr" im Bild.
          m > 0
          ? t("landing.sprit.wp_mehr", { kg: `${ungefaehr ? "≈ " : ""}${kg(Math.abs(m))}` })
          : t("landing.sprit.wp_weniger", { kg: `${ungefaehr ? "≈ " : ""}${kg(Math.abs(m))}` });

  // Die Bezugszeile hat noch nichts verbraucht; dort steht, wie viel mehr
  // oder weniger getankt wurde als geplant.
  const tankText = (d: number | null) =>
    d == null || Math.abs(d) < 0.5
      ? t("landing.sprit.wp_wie_getankt")
      : d > 0
        ? t("landing.sprit.wp_getankt_mehr", { kg: kg(Math.abs(d)) })
        : t("landing.sprit.wp_getankt_weniger", { kg: kg(Math.abs(d)) });

  const kopf = (
    <button
      type="button"
      onClick={() => setOffen((o) => !o)}
      aria-expanded={offen}
      style={{
        all: "unset",
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        gap: 12,
        width: "100%",
        flexWrap: "wrap",
      }}
    >
      <span style={{ color: LEISE, fontSize: 12, width: 12 }}>{offen ? "▼" : "▶"}</span>
      <Punkt ampel={letzte?.ampel} />
      <span
        style={{
          fontSize: 14,
          fontWeight: 700,
          textTransform: "uppercase",
          letterSpacing: "0.06em",
          color: "var(--accent, #4cc2ff)",
          whiteSpace: "nowrap",
        }}
      >
        {t("landing.sprit.wp_titel")}
      </span>
      {!offen && letzte && (
        <span style={{ marginLeft: "auto", display: "flex", gap: 14, flexWrap: "wrap", fontSize: 12.5, color: LEISE, fontVariantNumeric: "tabular-nums" }}>
          <span>
            {t("landing.sprit.wp_zuletzt")} <b style={{ color: TEXT }}>{letzte.ident}</b> {uhrzeit(letzte.zeit_ms)}
          </span>
          <span>
            SimBrief <b style={{ color: TEXT }}>{kg(letzte.plan_an_bord_kg)} kg</b>
          </span>
          <span>
            {t("landing.sprit.wp_ist")} <b style={{ color: TEXT }}>{kg(letzte.ist_an_bord_kg)} kg</b>
          </span>
          <span style={{ color: letzte.ampel ? AMPEL_FARBE[letzte.ampel] : LEISE }}>{mvText(mv)}</span>
          {letzte.landung_hochgerechnet_kg != null && (
            <span>
              {t("landing.sprit.wp_landung_kurz")} <b style={{ color: TEXT }}>≈ {kg(letzte.landung_hochgerechnet_kg)} kg</b>
            </span>
          )}
          {naechster && (
            <span>
              {t("landing.sprit.wp_naechster")} <b style={{ color: TEXT }}>{naechster.ident}</b>
              {p.naechsterNm != null ? ` · ${Math.round(p.naechsterNm)} NM` : ""}
            </span>
          )}
        </span>
      )}
    </button>
  );

  const rahmen: CSSProperties = p.ohneRahmen
    ? { display: "flex", flexDirection: "column", gap: 12 }
    : {
        background: "var(--surface, #141a21)",
        border: `1px solid ${LINIE}`,
        borderRadius: 10,
        padding: "14px 16px",
        display: "flex",
        flexDirection: "column",
        gap: 12,
      };

  return (
    <section style={rahmen} data-karte="sprit-wegpunkte">
      {kopf}
      {offen && (
        <>
          <Kurve zeilen={zeilen} />
          <div style={{ display: "flex", gap: 18, flexWrap: "wrap", fontSize: 11.5, color: LEISE }}>
            <span style={{ display: "inline-flex", alignItems: "center" }}>
              <Punkt ampel="gruen" />
              {t("landing.sprit.wp_legende_gruen")}
            </span>
            <span style={{ display: "inline-flex", alignItems: "center" }}>
              <Punkt ampel="gelb" />
              {t("landing.sprit.wp_legende_gelb")}
            </span>
            <span style={{ display: "inline-flex", alignItems: "center" }}>
              <Punkt ampel="rot" />
              {t("landing.sprit.wp_legende_rot")}
            </span>
          </div>
          <div style={{ overflowX: "auto" }}>
            <table style={{ width: "100%", borderCollapse: "collapse", fontVariantNumeric: "tabular-nums", fontSize: 13 }}>
              <thead>
                <tr>
                  {[
                    t("landing.sprit.wp_spalte_wegpunkt"),
                    t("landing.sprit.wp_spalte_zeit"),
                    t("landing.sprit.wp_spalte_plan"),
                    t("landing.sprit.wp_spalte_ist"),
                    t("landing.sprit.wp_spalte_abweichung"),
                    t("landing.sprit.wp_spalte_landung"),
                  ].map((h, i) => (
                    <th
                      key={h}
                      style={{
                        textAlign: i === 0 ? "left" : "right",
                        fontWeight: 600,
                        fontSize: 11,
                        letterSpacing: "0.05em",
                        textTransform: "uppercase",
                        color: LEISE,
                        padding: "6px 10px",
                        borderBottom: `1px solid ${LINIE}`,
                        whiteSpace: "nowrap",
                      }}
                    >
                      {h}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {zeilen.map((z, i) => {
                  const zukunft = z.zustand !== "gemessen" && z.zustand !== "uebersprungen";
                  const ueber = z.zustand === "uebersprungen";
                  const istNaechster = p.naechster === i;
                  const farbe = z.ampel ? AMPEL_FARBE[z.ampel] : LEISE;
                  const zelle: CSSProperties = {
                    textAlign: "right",
                    padding: "6px 10px",
                    borderBottom: "1px solid rgba(42,53,65,0.55)",
                    color: zukunft ? LEISE : TEXT,
                    fontStyle: ueber ? "italic" : undefined,
                    background: istNaechster ? "rgba(76,194,255,0.12)" : undefined,
                    whiteSpace: "nowrap",
                  };
                  // „Abheben" nur, wenn die Zeile auch wirklich der Abflug
                  // ist: Fehlen Block- und Taxi-Sprit, gibt es keine
                  // Abflugzeile, und der erste Eintrag ist ein ganz
                  // normaler Streckenpunkt (SimBrief beginnt das Navlog
                  // oft dort, nicht am Flughafen).
                  const ersteZeile =
                    i === 0 && z.zustand === "gemessen" && z.ist_an_bord_kg != null;
                  const letzteZeile = i === zeilen.length - 1;
                  return (
                    <tr key={`${z.ident}-${i}`} data-zustand={z.zustand ?? "offen"}>
                      <td style={{ ...zelle, textAlign: "left", fontWeight: 600 }}>
                        <span style={{ display: "inline-flex", alignItems: "center" }}>
                          <Punkt ampel={z.ampel} blass={ueber} />
                          {istNaechster && <span style={{ color: "var(--accent, #4cc2ff)", marginRight: 4 }}>▸</span>}
                          {z.ident}
                          <small style={{ fontWeight: 400, color: LEISE, marginLeft: 6 }}>
                            {ersteZeile
                              ? t("landing.sprit.wp_abflug")
                              : letzteZeile
                                ? t("landing.sprit.wp_ziel")
                                : z.hoehe_ft != null && z.hoehe_ft > 0
                                  ? z.hoehe_ft >= 10_000
                                    ? `FL${String(Math.round(z.hoehe_ft / 100)).padStart(3, "0")}`
                                    : `${kg(z.hoehe_ft)} ft`
                                  : ""}
                            {istNaechster && p.naechsterNm != null ? ` · ${t("landing.sprit.wp_in_nm", { nm: Math.round(p.naechsterNm) })}` : ""}
                          </small>
                          {ueber && (
                            <span
                              style={{
                                fontStyle: "normal",
                                fontSize: 10.5,
                                padding: "1px 6px",
                                borderRadius: 9,
                                border: `1px solid ${LINIE}`,
                                color: LEISE,
                                marginLeft: 8,
                                fontWeight: 400,
                              }}
                            >
                              {t("landing.sprit.wp_uebersprungen")}
                            </span>
                          )}
                        </span>
                      </td>
                      <td style={zelle}>{ueber ? "—" : uhrzeit(z.zeit_ms)}</td>
                      <td style={zelle}>{kg(z.plan_an_bord_kg)}</td>
                      <td style={zelle}>{zukunft ? "—" : `${ueber ? "≈ " : ""}${kg(z.ist_an_bord_kg)}`}</td>
                      <td style={{ ...zelle, color: zukunft ? LEISE : farbe }}>
                        {zukunft ? "—" : z === bezug ? tankText(tankdifferenz(z)) : mvText(mehrverbrauch(z, bezug), ueber)}
                      </td>
                      <td style={{ ...zelle, color: zukunft ? LEISE : farbe }}>
                        {zukunft || z.landung_hochgerechnet_kg == null ? (
                          letzteZeile && z.min_an_bord_kg != null ? (
                            <span style={{ fontSize: 11, color: LEISE }}>
                              {t("landing.sprit.wp_min_ofp", { kg: kg(z.min_an_bord_kg) })}
                            </span>
                          ) : (
                            "—"
                          )
                        ) : (
                          `≈ ${kg(z.landung_hochgerechnet_kg)}`
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          <div style={{ color: LEISE, fontSize: 11.5, lineHeight: 1.5 }}>{t("landing.sprit.wp_fuss")}</div>
        </>
      )}
    </section>
  );
}

/**
 * Mehrverbrauch über die Wegpunkte — die Punkte in der Farbe ihrer Zeile.
 * Nur gemessene und übersprungene Wegpunkte; übersprungene als offener Kreis.
 */
function Kurve({ zeilen }: { zeilen: SpritWegpunkt[] }) {
  const { t } = useTranslation();
  const bezug = bezugszeile(zeilen);
  const W = 1060;
  const H = 150;
  const X0 = 60;
  const X1 = W - 10;
  const YM = 72;
  const punkte = zeilen
    .map((z, i) => ({ z, i, m: mehrverbrauch(z, bezug) }))
    // Die Bezugszeile selbst bleibt draussen: Ihr Verbrauch ist per
    // Definition null, die Tabelle nennt dort aber die Tankdifferenz
    // ("292 kg mehr getankt"). Ein Punkt auf der Nulllinie neben dieser
    // Zahl liest sich wie ein Widerspruch (QS 20.09.2026).
    .filter(
      (q) =>
        q.z !== bezug &&
        q.m != null &&
        (q.z.zustand === "gemessen" || q.z.zustand === "uebersprungen"),
    );
  const groesst = Math.max(100, ...punkte.map((q) => Math.abs(q.m!)));
  const skala = Math.ceil(groesst / 100) * 100;
  const x = (i: number) => X0 + ((X1 - X0) * i) / Math.max(1, zeilen.length - 1);
  // Mehr verbraucht = nach UNTEN: weniger im Tank.
  const y = (m: number) => YM + (m / skala) * 42;
  // Beschriftungen ausdünnen: höchstens etwa zwölf Namen.
  const jeder = Math.max(1, Math.ceil(zeilen.length / 12));
  return (
    <svg viewBox={`0 0 ${W} ${H}`} width="100%" role="img" aria-label={t("landing.sprit.wp_kurve_titel")}>
      <text x={0} y={12} fill="#8695a3" fontSize={11} letterSpacing={1}>
        {t("landing.sprit.wp_kurve_titel")}
      </text>
      <line x1={X0} y1={YM} x2={X1} y2={YM} stroke="#8695a3" strokeDasharray="6 5" opacity={0.6} />
      <text x={X0 - 8} y={YM + 4} fill="#8695a3" fontSize={11} textAnchor="end">
        0
      </text>
      <text x={X0 - 8} y={YM - 38} fill="#8695a3" fontSize={11} textAnchor="end">
        {skala}
      </text>
      <text x={X0 - 8} y={YM + 46} fill="#8695a3" fontSize={11} textAnchor="end">
        {skala}
      </text>
      <text x={X1} y={YM - 30} fill="#8695a3" fontSize={10} textAnchor="end" opacity={0.8}>
        {t("landing.sprit.wp_kurve_weniger")}
      </text>
      <text x={X1} y={YM + 40} fill="#8695a3" fontSize={10} textAnchor="end" opacity={0.8}>
        {t("landing.sprit.wp_kurve_mehr")}
      </text>
      {punkte.length >= 2 && (
        <polyline
          fill="none"
          stroke="#8695a3"
          strokeWidth={2}
          points={punkte.map((q) => `${x(q.i).toFixed(1)},${y(q.m!).toFixed(1)}`).join(" ")}
        />
      )}
      {punkte.map((q) =>
        q.z.zustand === "uebersprungen" ? (
          <circle key={q.i} cx={x(q.i)} cy={y(q.m!)} r={4} fill="none" stroke={q.z.ampel ? AMPEL_FARBE[q.z.ampel] : "#8695a3"} strokeWidth={1.5} />
        ) : (
          <circle key={q.i} cx={x(q.i)} cy={y(q.m!)} r={4.5} fill={q.z.ampel ? AMPEL_FARBE[q.z.ampel] : "#8695a3"} />
        ),
      )}
      {zeilen.map((z, i) =>
        i % jeder === 0 || i === zeilen.length - 1 ? (
          <text
            key={i}
            x={x(i)}
            y={H - 6}
            fill="#8695a3"
            fontSize={10.5}
            textAnchor={i === 0 ? "start" : i === zeilen.length - 1 ? "end" : "middle"}
          >
            {z.ident}
          </text>
        ) : null,
      )}
    </svg>
  );
}
