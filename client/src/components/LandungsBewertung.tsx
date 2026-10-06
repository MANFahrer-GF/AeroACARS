// Bewertung einer Landung: Kopf (Buchstabe, Flug, Wort, Note, Deckel,
// Flugzeug, Messgüte), Unfall- und Divert-Banner, Hinweis-Marken,
// Teilnoten mit Erklärungen und Coach-Tipp.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 lebte das in LandingPanel.tsx; die Webapp hatte eigene Fassungen
// (anderer Kopf ohne Buchstaben, Teilnoten ohne Erklärungen, eigene
// Banner). Jetzt zeigen beide Seiten dasselbe aus demselben Code.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { LandingCategory, LandingRecord } from "../lib/landungsDatensatz";
import { gateAus, gateGruende, gateMarke, gateUrteil, type GatePunkt } from "../lib/stableGate";
import { landungsMarkenV19 } from "../lib/landungsUrteil";
import { fensterWerteGueltig, scoreG } from "../lib/landungsFormat";
import { rolloutLdaMeters } from "../lib/runwayGeometry";
import { scoreBasisVs } from "./SinkrateForensik";
import { istBewertbar } from "../lib/landungsFormat";
import { deckelText } from "../lib/landungsUrteil";
import { ForensicsBadge } from "./ForensicsBadge";
import { SpritBadge } from "./SpritSektion";
import { InfoBadge } from "./InfoBadge";
import { RunwayUtilizationHelpModal } from "./RunwayUtilizationHelpModal";
import { surfaceLabelKey } from "./RunwayDiagramV2";
import "./landungsBewertung.css";

export interface SubScore {
  key: string;
  points: number;
  /** Pre-formatted value to show on the card ("379 fpm", "1.10 G", …). */
  value: string;
  /** "good" | "ok" | "bad" | "skipped" — drives the colour band.
   *  v0.7.1 (P1.2-Fix): "skipped" als visuell graue Variante damit
   *  "nicht bewertet" sichtbar bleibt (Loadsheet/Fuel ohne Plan). */
  band: "good" | "ok" | "bad" | "skipped";
  /** Why we awarded this score (one short sentence). */
  rationale: string;
  /** v0.7.1 (P1.2-Fix): true wenn dieser Sub-Score nicht bewertet wurde
   *  (z.B. VFR ohne ZFW → loadsheet skipped, ohne planned_burn → fuel
   *  skipped). UI rendert eine "nicht bewertet"-Karte statt der Karte
   *  ganz auszublenden. */
  skipped?: boolean;
  /** Skip-Reason fuer i18n-Key landing.skipped_reason.* */
  skipReason?: string;
  /** v0.10.0 (#runway-utilization-score) — Extra-Display-Zeilen unter
   *  der Rationale. Wird vom Card-Renderer als Bullet-Liste gezeigt.
   *  Leeres Array → nichts gerendert (= forward-compat mit pre-v0.10
   *  Records ohne `extra`-Feld). */
  extra?: string[];
  /** v0.10.0 — Warning-Wert (z.B. "pre_displaced_threshold") für die
   *  Warning-Pill. UI lookup: `landing.warn.<warning>`. */
  warning?: string;
  /** Score-Version 19: Prüfliste des Stable Gate (nur `stability`). */
  gate?: GatePunkt[] | null;
  /** i18n-Schlüssel des Achsennamens aus dem Datensatz (z. B.
   *  „landing.sub.runway_discipline" für die Bahn-Achse ab v2). */
  label_key?: string | null;
}

/**
 * Die Beschriftung einer Achse — aus dem Datensatz, nicht aus dem Schlüssel.
 *
 * # Warum das nicht `achsenLabel(t, s)` sein darf
 *
 * Der Schlüssel `rollout` trägt seit v1.7.0 eine andere Bewertung als
 * vorher: erst die Bahn-Auslastung, jetzt die Bahndisziplin. Er wurde
 * bewusst beibehalten, damit alte Datensätze nicht brechen — und genau
 * deshalb sagt er nichts mehr darüber, was gemessen wurde.
 *
 * Jeder Datensatz trägt sein eigenes `label_key` mit: alte
 * `landing.sub.rollout`, neue `landing.sub.runway_discipline`. Bis
 * Runde 23 hat es niemand gelesen — die Anzeige baute den Schlüssel selbst
 * zusammen und zeigte damit über der neuen Bewertung die alte
 * Beschriftung „Bahn-Auslastung".
 *
 * Der Rückfall deckt Datensätze vor v0.7.1 ab, die noch kein `label_key`
 * haben.
 */
export function achsenLabel(
  t: (k: string, o?: Record<string, unknown>) => string,
  s: { key: string; label_key?: string | null },
): string {
  const aus_schluessel = `landing.sub.${s.key}`;
  const k = s.label_key && s.label_key.length > 0 ? s.label_key : aus_schluessel;
  // Der Rückfall geht auf den Schlüssel, NICHT wieder auf diese Funktion.
  // Beim Umstellen hat die Ersetzung sich selbst getroffen; der Testlauf
  // meldete es als Endlosrekursion.
  return t(k, { defaultValue: t(aus_schluessel) });
}

/** Rationale → i18n key for the coach tip. We point straight at the
 *  fully-qualified `landing.tip.*` path so a missing translation
 *  shows up as the key (easier to spot in QA) rather than as a
 *  silent fallback. */
export function coachTipKey(rationale: string): string {
  return `landing.tip.${rationale}`;
}

export function gradeColor(grade: string | null | undefined): string {
  // Ohne Note keine Farbe der Skala: Grau heisst "nicht bewertet", nicht
  // "schlecht". Vorher fiel `null` durch bis zum roten F (12.09.2026).
  if (grade == null || grade === "") return "var(--text-muted, #8D99AD)";
  if (grade === "A+" || grade === "A") return "#22c55e"; // green
  if (grade === "B+" || grade === "B") return "#84cc16"; // lime
  if (grade === "C") return "#eab308"; // amber
  if (grade === "D") return "#f97316"; // orange
  return "#ef4444"; // red — F
}

/**
 * Datum und Uhrzeit in der Sprache, die der Pilot eingestellt hat.
 *
 * `toLocaleString()` ohne Angabe nimmt die Sprache des Betriebssystems,
 * nicht die der App. Auf einem englischsprachigen Windows stand im
 * deutschen Bericht „5/13/2026, 7:42:00 PM" — ein amerikanisches Datum
 * zwischen deutschen Beschriftungen, und bei Tagen unter 13 nicht einmal
 * als falsch erkennbar (05.12. oder 12.05.?).
 */
export function fmtDateTime(iso: string, sprache?: string): string {
  try {
    const d = new Date(iso);
    // Die Sprache der App kommt vom Aufrufer (`useTranslation().i18n`) —
    // jede App hat ihre eigene i18n-Instanz.
    return d.toLocaleString(sprache || undefined);
  } catch {
    return iso;
  }
}

/** This screen's single category for colour-coding (chart bars, header
 *  distribution, sinkrate cell, NOTE cell).
 *
 *  History: v1 derived this from the raw sink-rate fpm alone (mirroring
 *  `classifyByVS()`), independent of `grade_letter` — same letter (e.g. "A")
 *  could sit next to either SMOOTH or ACCEPTABLE. Pilot asked to couple word
 *  and letter instead.
 *
 *  v2 read `record.score_label` verbatim — also wrong, because that field is
 *  frozen at file-time: Rust's `aggregate_score_label()` was realigned in
 *  v0.20.0, so older PIREPs carried the OLD ladder's result forever.
 *
 *  v3 recomputed the ladder here in TypeScript. That fixed this screen but
 *  put the SAME rule in the codebase twice — and promptly produced the next
 *  contradiction: this overview said "SMOOTH" while the report right next to
 *  it (still reading the frozen field) said "ACCEPTABLE" for one and the same
 *  landing. Measured on real data: 2 of 31 flights, both 88 points / grade A.
 *
 *  v4 (QS 2026-08-04) fixes it where it belongs — `landing_list` in
 *  src-tauri/src/lib.rs recomputes BOTH derived fields (`score_label` and
 *  `grade_letter`) from `score_numeric` when it hands records to the UI. That
 *  leaves exactly one source of truth, in Rust, for the whole app: every
 *  surface gets the same word by construction, a future change to the ladder
 *  applies retroactively to the entire history, and this duplicate ladder can
 *  go away. So: just read the (now guaranteed-fresh) label.
 *
 *  The `default` arm is a pure safety net for a label the backend might add
 *  later — it must never be reached with today's five values, and it
 *  deliberately does NOT reintroduce a second threshold ladder. */
export function recordCategory(r: LandingRecord): LandingCategory | null {
  // Ohne Bewertung gibt es keine Kategorie.
  //
  // Prüfbefund 13.09.2026: `null` fiel in den `default`-Zweig und wurde zu
  // "firm" — im Druckbericht stand dann "FEST" über einer Landung, die gar
  // nicht gemessen wurde, und der Statistikfuss zählte sie in diesen Topf.
  if (r.score_label == null) return null;
  switch (r.score_label) {
    case "smooth":
    case "acceptable":
    case "firm":
    case "hard":
    case "severe":
      return r.score_label;
    default:
      return "firm";
  }
}

/** Literal English, not translated — same wording the backend's own
 *  `aggregate_score_label()` produces. Since QS 2026-08-04 this is the ONE
 *  way the category word reaches the screen (overview list, chart, and the
 *  report headline all go through here), so the two can no longer drift. */
/** Wort der GESAMTNOTE zur Kategorie (Score-Version 19): „hervorragend /
 *  gut / ausreichend / mangelhaft / ungenügend" statt „smooth … severe" —
 *  die beschreiben einen Touchdown, nicht die Note (eine butterweiche
 *  Landung nach instabilem Anflug hieß sonst „HARD"). Die Kategorie selbst
 *  (Farben, Kennung im Datensatz) bleibt unverändert. */
export function rateCategoryWord(
  cat: LandingCategory,
  t: (k: string) => string,
): string {
  return t(`landing.gesamt.${cat}`).toUpperCase();
}

/** Erste score_algorithm_version mit TS-gerenderten Extra-Zeilen. */
const ROLLOUT_ALGO_V3 = 3;

/** True wenn der Record v3-Scoring nutzt (TS rendert die Extra-Zeilen). */
export function isRolloutV3(r: LandingRecord): boolean {
  return (r.score_algorithm_version ?? 0) >= ROLLOUT_ALGO_V3;
}

/** Minimaler t-Typ — reicht für die Extra-Zeilen-Interpolation. */
type RolloutTFn = (key: string, opts?: Record<string, string | number>) => string;

/**
 * v0.12.0 LE4 — sprach-lokalisiertes Value-Label für die rollout-Card.
 * Zeigt die ECHTE Auslastung (raw, nicht toleranzbereinigt). Liefert null
 * wenn Felder fehlen — der Caller fällt dann auf den sprachneutralen
 * `value`-String des Rust-Crates zurück.
 */
export function buildRolloutValueLabel(
  r: LandingRecord,
  t: RolloutTFn,
): string | null {
  const rm = r.runway_match;
  if (!rm) return null;
  const lda = rolloutLdaMeters(rm);
  if (lda == null) return null;
  const td = r.td_distance_from_threshold_m;
  const rollout = r.rollout_distance_m;
  if (td == null || rollout == null) return null;
  const used = Math.max(td + rollout, rollout);
  return t("landing.rollout_extra.value_label", {
    pct: Math.round((used / lda) * 100),
    used: Math.round(used),
    lda: Math.round(lda),
  });
}

/**
 * v0.12.0 LE5 — die drei TS-gerenderten Extra-Zeilen der rollout-Card.
 * Reihenfolge: Aufsetzpunkt → Ausrollstrecke → Bahn. Jede Zeile entfällt
 * einzeln wenn ihr Quell-Feld fehlt (z.B. kein `runway_match` → keine
 * Bahn-Zeile, die anderen zwei bleiben).
 *
 * R2-P2-Fix: negatives `td_distance` (Aufsetzen VOR der Schwelle) wählt
 * den `_before`-Key und übergibt den BETRAG — nie „−50 m hinter …".
 */
export function buildRolloutExtraLines(
  r: LandingRecord,
  t: RolloutTFn,
): string[] {
  const lines: string[] = [];

  const td = r.td_distance_from_threshold_m;
  if (td != null) {
    const m = Math.round(Math.abs(td));
    const key =
      td < 0
        ? "landing.rollout_extra.touchdown_point_before"
        : "landing.rollout_extra.touchdown_point";
    lines.push(t(key, { m }));
  }

  if (r.rollout_distance_m != null) {
    lines.push(
      t("landing.rollout_extra.rollout_distance", {
        m: Math.round(r.rollout_distance_m),
      }),
    );
  }

  const rm = r.runway_match;
  if (rm) {
    const lda = rolloutLdaMeters(rm);
    if (lda != null) {
      lines.push(
        t("landing.rollout_extra.runway", {
          icao: rm.airport_ident,
          ident: rm.runway_ident,
          lda: Math.round(lda),
        }),
      );
    }
  }

  return lines;
}

export function ScoreBreakdown({
  subs,
  record,
}: {
  subs: SubScore[];
  record: LandingRecord;
}) {
  const { t, i18n } = useTranslation();
  // v0.11.0-dev: Pilot-Hilfe-Modal für den "Bahn-Auslastung"-Sub-Score.
  // Wird über den "🛬 Wie wird das berechnet?"-Button am Boden der
  // rollout-Card geöffnet. Andere Sub-Scores behalten ihren bestehenden
  // InfoBadge-Tooltip — nur Bahn-Auslastung bekommt das tiefe Erklärungs-
  // Modal, weil sie mit Bändern + Heavy-Bonus + Pre-Displaced-Cap die
  // komplexeste Score-Logik hat.
  const [runwayUtilHelpOpen, setRunwayUtilHelpOpen] = useState(false);
  if (subs.length === 0) return null;
  return (
    <div className="landing-subscores">
      {runwayUtilHelpOpen && (
        <RunwayUtilizationHelpModal
          onClose={() => setRunwayUtilHelpOpen(false)}
        />
      )}
      {subs.map((s) => {
        // v0.7.1 P1.2-Fix: skipped wird sichtbar als "nicht bewertet"
        // (graue Variante, keine Punkte/Wert/Rationale).
        if (s.skipped) {
          const reasonKey = s.skipReason
            ? `landing.skipped_reason.${s.skipReason}`
            : "landing.skipped_reason.fallback";
          return (
            <div
              key={s.key}
              className="landing-subscore landing-subscore--skipped"
              style={{ opacity: 0.65 }}
            >
              <div className="landing-subscore__head">
                <span className="landing-subscore__label">
                  {achsenLabel(t, s)}
                  {/* v0.11.0-dev: kein i-Tooltip für rollout — der
                      "🛬 Wie wird das berechnet?"-Button am Boden öffnet
                      bereits das ausführliche Modal. Zwei Erklärungen
                      auf der gleichen Card wären redundant. */}
                  {s.key !== "rollout" && (
                    <InfoBadge explanation={t(`landing.info.${s.key}`)} />
                  )}
                </span>
                <span
                  className="landing-subscore__points"
                  style={{ fontStyle: "italic", fontSize: "0.75rem" }}
                >
                  {t("landing.skipped_label")}
                </span>
              </div>
              <div
                className="landing-subscore__rationale"
                style={{ fontStyle: "italic" }}
              >
                {t(reasonKey)}
              </div>
            </div>
          );
        }
        // v0.10.0 (#runway-utilization-score) — Warning-Pill (z.B.
        // pre_displaced_threshold) + extra-Lines (Float-Distance,
        // Bahn-Info). Beides Vorhanden NUR wenn das Rust-Crate sie
        // gefüllt hat — pre-v0.10 SubScoreEntries kommen ohne diese
        // Felder durch (undefined) und das Rendering ist No-op.
        const hasWarning =
          typeof s.warning === "string" && s.warning.length > 0;
        // v0.12.0 (#runway-utilization-refinement, LE4/LE5): die rollout-
        // Card rendert ab score_algorithm_version >= 3 ihr Value-Label
        // und ihre Extra-Zeilen sprach-lokalisiert aus den Record-Feldern.
        // Alt-v2-Records (< 3) zeigen den sprachneutralen Rust-`value`
        // bzw. die gespeicherten `extra`-Strings unverändert (Legacy).
        // Score-Version 19: Bahndisziplin (label_key) ist die bewertete Bahn-
        // Achse — dann der eingefrorene Wert, keine Auslastungs-Zeilen.
        const bahndisziplin = s.label_key === "landing.sub.runway_discipline";
        const isV3Rollout = s.key === "rollout" && isRolloutV3(record) && !bahndisziplin;
        const extraLines = isV3Rollout
          ? buildRolloutExtraLines(record, t)
          : (s.extra ?? []);
        const valueText = isV3Rollout
          ? (buildRolloutValueLabel(record, t) ?? s.value)
          : s.value;
        return (
          <div
            key={s.key}
            className={`landing-subscore landing-subscore--${s.band}`}
          >
            <div className="landing-subscore__head">
              <span className="landing-subscore__label">
                {achsenLabel(t, s)}
                {/* v0.11.0-dev: kein i-Tooltip für rollout — der
                    "🛬 Wie wird das berechnet?"-Button am Boden öffnet
                    bereits das ausführliche Modal. Score-Version 19: die
                    Bahndisziplin hat ihren eigenen i-Text statt des
                    Auslastungs-Modals. */}
                {s.key !== "rollout" ? (
                  <InfoBadge explanation={t(`landing.info.${s.key}`)} />
                ) : bahndisziplin ? (
                  <InfoBadge explanation={t("landing.info.runway_discipline")} />
                ) : null}
              </span>
              <span className="landing-subscore__points">{s.points} PTS</span>
            </div>
            {/* `title` als Netz: der Wert darf umbrechen, aber wenn ihn
                jemand doch einmal kürzt, bleibt er wenigstens erreichbar. */}
            <div className="landing-subscore__value" title={valueText}>
              {valueText}
            </div>
            <div className="landing-subscore__bar">
              <div
                className="landing-subscore__fill"
                style={{ width: `${s.points}%` }}
              />
            </div>
            <div className="landing-subscore__rationale">
              {t(`landing.rat.${s.rationale}`)}
            </div>
            {hasWarning && (
              <div
                className="landing-subscore__warning farbwert"
                style={{
                  marginTop: 4,
                  fontSize: "0.75rem",
                  color: "#fbbf24",
                  fontWeight: 600,
                }}
              >
                {t(`landing.warn.${s.warning}`)}
              </div>
            )}
            {/* Score-Version 19: das Warum gleich unter dem Hinweis. */}
            {s.key === "stability" && gateGruende(t, s.gate, i18n.language).length > 0 && (
              <ul
                data-testid="stabilitaet-gruende"
                style={{ margin: "4px 0 0", paddingLeft: 18, fontSize: "0.75rem" }}
              >
                {gateGruende(t, s.gate, i18n.language).map((g) => (
                  <li key={g}>{g}</li>
                ))}
              </ul>
            )}
            {extraLines.length > 0 && (
              <ul
                className="landing-subscore__extra"
                style={{
                  marginTop: 4,
                  marginBottom: 0,
                  paddingLeft: 14,
                  fontSize: "0.72rem",
                  color: "var(--text-muted)",
                  listStyle: "'▸ '",
                }}
              >
                {extraLines.map((line, idx) => (
                  <li key={idx}>{line}</li>
                ))}
              </ul>
            )}
            {/* v0.11.0-dev: Pilot-Hilfe-Button nur auf der rollout-Card.
                Öffnet RunwayUtilizationHelpModal mit Formel, allen Bändern,
                Heavy-Bonus, Pre-Displaced-Cap und Skip-Reasons. */}
            {s.key === "rollout" && !bahndisziplin && (
              <button
                // Bedienung, kein Inhalt — nicht aufs Papier (QS 06.10.2026).
                className="nur-bildschirm"
                type="button"
                onClick={() => setRunwayUtilHelpOpen(true)}
                style={{
                  marginTop: 8,
                  padding: "4px 10px",
                  background: "rgba(34,197,94,0.10)",
                  border: "1px solid rgba(34,197,94,0.35)",
                  borderRadius: 4,
                  color: "#bbf7d0",
                  fontSize: "0.72rem",
                  cursor: "pointer",
                  alignSelf: "flex-start",
                }}
              >
                {t("landing.runway_utilization_help.open_button")}
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}

export function CoachTip({ subs }: { subs: SubScore[] }) {
  const { t } = useTranslation();
  if (subs.length === 0) return null;
  // v0.7.1 P1.2-Fix: skipped Sub-Scores nicht als "schwaechster"
  // Punkt im Coach-Tip nutzen (sie sind nicht bewertet, nicht schlecht).
  const scored = subs.filter((s) => !s.skipped);
  if (scored.length === 0) return null;
  // Sort ascending, the lowest sub-score is the area to improve. If
  // everything is ≥ 90, surface the genuine "good landing" message.
  const sorted = [...scored].sort((a, b) => a.points - b.points);
  const worst = sorted[0];
  const tipKey = coachTipKey(worst.rationale);
  return (
    <div
      className={`landing-coach landing-coach--${
        // Bänder der Bewertung (band_from_points): ab 75 gut, ab 45 mittel.
        worst.points >= 75 ? "good" : worst.points >= 45 ? "ok" : "bad"
      }`}
    >
      <div className="landing-coach__head">
        {t("landing.coach_title")} ·{" "}
        <strong>{achsenLabel(t, worst)}</strong>
      </div>
      <p className="landing-coach__body">{t(tipKey)}</p>
    </div>
  );
}

/**
 * v0.7.19 GAF-707 Accident-Detection — Banner als Primary-Klassifikation.
 *
 * Spec docs/spec/v0.7.19-gaf707-crash-accident-detection.md §AeroACARS
 * Client Tab "Landung". GAF 707 darf hier NICHT als normale Hard-
 * Landing/Bone-Rattler erscheinen.
 *
 * - `accident === true` (Confirmed): roter Top-Level-Banner "ABSTURZ
 *   ERKANNT" mit Gruenden-Liste.
 * - `accident_confidence === "medium"` ohne accident=true (Suspected):
 *   gelber Review-Hinweis-Banner.
 * - Sonst: kein Banner.
 */
export function AccidentBanner({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();

  const isConfirmed = record.accident === true;
  const isSuspected =
    !isConfirmed && record.accident_confidence === "medium";

  if (!isConfirmed && !isSuspected) {
    return null;
  }

  const kindLabel = (() => {
    switch (record.accident_kind) {
      case "sim_crash":
        return t("landing.accident.kind.sim_crash");
      case "impact":
        return t("landing.accident.kind.impact");
      case "off_airport_impact":
        return t("landing.accident.kind.off_airport_impact");
      default:
        return null;
    }
  })();

  const reasons = record.accident_reasons ?? [];

  if (isConfirmed) {
    return (
      <div className="accident-banner accident-banner--confirmed" role="alert">
        <div className="accident-banner__head">
          ⚠ {t("landing.accident.confirmed_title")}
        </div>
        <div className="accident-banner__body">
          {t("landing.accident.confirmed_body")}
        </div>
        {kindLabel && (
          <div className="accident-banner__kind">
            <strong>{t("landing.accident.kind_label")}:</strong> {kindLabel}
          </div>
        )}
        {reasons.length > 0 && (
          <ul className="accident-banner__reasons">
            {reasons.map((r) => (
              <li key={r}>{r}</li>
            ))}
          </ul>
        )}
      </div>
    );
  }

  // Suspected
  return (
    <div className="accident-banner accident-banner--suspected" role="alert">
      <div className="accident-banner__head">
        ⚠ {t("landing.accident.suspected_title")}
      </div>
      <div className="accident-banner__body">
        {t("landing.accident.suspected_body")}
      </div>
      {reasons.length > 0 && (
        <ul className="accident-banner__reasons">
          {reasons.map((r) => (
            <li key={r}>{r}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * Wie weit vom Ziel entfernt eine Landung als "off airport" gilt.
 *
 * Spiegelt `MAX_FILE_DISTANCE_NM` im Backend (lib.rs): das ist die Distanz, ab
 * der `flight_end` das Einreichen mit `not_at_arrival` verweigert. Beide Werte
 * beantworten dieselbe Frage — "bist du eigentlich angekommen?" — und müssen
 * deshalb übereinstimmen. v0.19.3: stand vorher als nackte `5` mitten in der
 * Bedingung, ohne Bezug zum Backend-Wert.
 */
const OFF_AIRPORT_MIN_NM = 5;

export function OffAirportBanner({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();

  const td = record.touchdown_airport;
  const planned = record.arr_airport;
  const source = record.touchdown_airport_source;
  const distToDest = record.touchdown_distance_to_destination_nm;
  const nearestDist = record.touchdown_nearest_distance_nm;

  // Normaler Fall: gleiche ICAO → kein Banner. Ohne geplanten Platz
  // (Stratos, Payloads vor v0.7.18) gibt es nichts zu vergleichen — kein
  // „Geplant: (leer)" (QS 06.10.2026, wie die frühere Webapp).
  if (!td || !planned || td === planned) {
    // Selbst bei runway_match==arr_airport kann ein > 5nm-Distanz
    // auftreten (Multi-Field-Airports), aber das ist kein Off-airport-Fall.
    return null;
  }

  // Spec-konforme Varianten:
  if (source === "nearest_25nm") {
    return (
      <div className="off-airport-banner off-airport-banner--nearest" role="alert">
        <div className="off-airport-banner__head">
          ⚠ {t("landing.off_airport.title")}
        </div>
        <div className="off-airport-banner__line">
          {t("landing.off_airport.planned")}:{" "}
          <strong>{planned}</strong>
        </div>
        <div className="off-airport-banner__line">
          {t("landing.off_airport.actual")}:{" "}
          <strong>{td}</strong>
          {nearestDist != null && (
            <span className="off-airport-banner__hint">
              {" — "}
              {t("landing.off_airport.nearest_hint", {
                nm: nearestDist.toFixed(1),
              })}
            </span>
          )}
        </div>
        {distToDest != null && distToDest > 1 && (
          <div className="off-airport-banner__line">
            {t("landing.off_airport.distance_to_dest", {
              nm: distToDest.toFixed(1),
            })}
          </div>
        )}
      </div>
    );
  }

  if (source === "planned_fallback") {
    // Position bekannt aber kein Airport in 25 nmi. Distanz-Hinweis nur
    // sinnvoll wenn echter Off-airport-Crash (> OFF_AIRPORT_MIN_NM).
    if (distToDest == null || distToDest <= OFF_AIRPORT_MIN_NM) return null;
    return (
      <div className="off-airport-banner off-airport-banner--no-nearest" role="alert">
        <div className="off-airport-banner__head">
          ⚠ {t("landing.off_airport.no_nearest_title")}
        </div>
        <div className="off-airport-banner__line">
          {t("landing.off_airport.planned")}:{" "}
          <strong>{planned}</strong>
        </div>
        <div className="off-airport-banner__line">
          {t("landing.off_airport.no_nearest_body", {
            nm: distToDest.toFixed(1),
          })}
        </div>
      </div>
    );
  }

  // source == "runway_match" aber td != planned → Diversion.
  return (
    <div className="off-airport-banner off-airport-banner--divert" role="alert">
      <div className="off-airport-banner__head">
        🛬 {t("landing.off_airport.divert_title")}
      </div>
      <div className="off-airport-banner__line">
        {t("landing.off_airport.planned")}:{" "}
        <strong>{planned}</strong>
      </div>
      <div className="off-airport-banner__line">
        {t("landing.off_airport.actual")}:{" "}
        <strong>{td}</strong>
        {distToDest != null && distToDest > 1 && (
          <span className="off-airport-banner__hint">
            {" — "}
            {t("landing.off_airport.distance_to_dest", {
              nm: distToDest.toFixed(1),
            })}
          </span>
        )}
      </div>
    </div>
  );
}

export function QuickFlags({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();
  const flags: { label: string; tone: "warn" | "err" }[] = [];

  // Ohne gemessenes Aufsetzfenster kein Hopser-Flag: Ein Hopser zwischen
  // zwei fehlenden Proben bleibt unsichtbar, und "0 Hopser" läse sich dann
  // als saubere Landung (Prüfbefund 13.09.2026).
  const hopserGemessen = fensterWerteGueltig(record);

  // Score-Version 19 (QS 05.10.2026): alle Marken aus lib/landungsUrteil.ts —
  // dieselbe Datei erzeugt sie in der Webapp. Gelesen wird nur, was die
  // Bewertung eingefroren hat; keine eigenen Grenzen.
  const gate = gateAus(record.sub_scores);
  if (gate != null || (record.score_algorithm_version ?? 0) >= 19) {
    flags.push(
      ...landungsMarkenV19(
        {
          subs: record.sub_scores ?? [],
          deckel: record.score_deckel,
          urteil: gateUrteil(gate, gateMarke(record.sub_scores)),
          bounceCount: hopserGemessen ? (record.bounce_count ?? 0) : 0,
          forensicBounceCount: hopserGemessen ? record.forensic_bounce_count : 0,
          bounceMaxAglFt: record.bounce_max_agl_ft,
        },
        t,
      ),
    );
  } else {
    // Altbestand (vor Score-Version 19) bleibt, wie er war.
    // HARD LANDING — V/S oder Peak-G erreichen Hard/Severe-Schwellen
    // (gespiegelt aus landingScoring.ts T_VS_HARD_FPM / T_G_HARD).
    // v0.20.0: ueber scoreBasisVs() statt handkopierter Kaskade (siehe oben).
    // v0.12.3 (LE9): G-Flag auf dem gescorten (EMA) Wert, nicht dem Roh-Peak.
    const peakVs = scoreBasisVs(record);
    const gForFlag = scoreG(record) ?? 0;
    // Ohne gemessene Sinkrate gibt es auch kein "hart" — die Landung war
    // vielleicht hart, wir wissen es nur nicht.
    const isHardVs = peakVs != null && Math.abs(peakVs) >= 600;
    const isHardG = gForFlag >= 1.7;
    if (isHardVs || isHardG) {
      const severe = (peakVs != null && Math.abs(peakVs) >= 1000) || gForFlag >= 2.1;
      flags.push({
        label: severe ? t("landing.flag.severe") : t("landing.flag.hard"),
        tone: "err",
      });
    }

    // BOUNCE × n
    // v0.8.3 (#8): Auch score-freie Hopser (5-14 ft) zeigen. Drei Faelle:
    //   bounce_count > 0                            → voller Flag
    //   bounce_count = 0, forensic_bounce_count > 0 → Light-bounce-Hinweis
    //   alle 0                                       → kein Flag
    if (hopserGemessen && record.bounce_count != null && record.bounce_count > 0) {
      flags.push({
        label: `${t("landing.flag.bounce")} × ${record.bounce_count}`,
        tone: record.bounce_count >= 2 ? "err" : "warn",
      });
    } else if (hopserGemessen && (record.forensic_bounce_count ?? 0) > 0) {
      const heightFt = record.bounce_max_agl_ft != null
        ? Math.round(record.bounce_max_agl_ft)
        : null;
      flags.push({
        label: heightFt != null
          ? t("landing.flag.bounce_light_with_height", { ft: heightFt })
          : t("landing.flag.bounce_light"),
        tone: "warn",
      });
    }

    // OFF-CENTERLINE — > 5 m vom Centerline weg.
    if (record.runway_match && Math.abs(record.runway_match.centerline_distance_m) > 5) {
      flags.push({
        label: t("landing.flag.off_centerline"),
        tone: "warn",
      });
    }

    // UNSTABLE APPROACH — σ V/S > 400 (Score-Lib-Schwelle für "bad").
    if ((record.approach_vs_stddev_fpm ?? 0) > 400) {
      flags.push({
        label: t("landing.flag.unstable_approach"),
        tone: "warn",
      });
    }
  }

  // Durchstarts — ein Messwert, keine Note (bis 05.10.2026 nur in der
  // Webapp als „GO-AROUND × n").
  if ((record.go_around_count ?? 0) > 0) {
    flags.push({
      label: t("landing.flag.go_around", { count: record.go_around_count ?? 0 }),
      tone: "warn",
    });
  }

  // Bestätigter Unfall: die Marke UNFALL statt „harte/schwere Landung" —
  // sonst stünde neben dem Unfall-Banner eine widersprüchliche Marke
  // (frühere Webapp, QS-R1 Finding 4; QS 06.10.2026 wieder hergestellt).
  if (record.accident === true) {
    const hart = new Set([t("landing.flag.hard"), t("landing.flag.severe")]);
    const rest = flags.filter((f) => !hart.has(f.label));
    flags.length = 0;
    flags.push({ label: t("landing.flag.accident"), tone: "err" }, ...rest);
  }

  if (flags.length === 0) return null;
  return (
    <div className="landing-flags">
      {flags.map((f, i) => (
        <span key={i} className={`landing-flag landing-flag--${f.tone}`}>
          {f.label}
        </span>
      ))}
    </div>
  );
}


/** Die eingefrorenen Teilnoten eines Datensatzes für die Anzeige (ab
 *  ux_version 1 — ältere Datensätze rechnet nur der Client nach). */
export function subScoresAusDatensatz(r: LandingRecord): SubScore[] {
  if (!r.sub_scores || r.sub_scores.length === 0) return [];
  return r.sub_scores.map((s) => {
    const band: SubScore["band"] =
      s.band === "good" || s.band === "ok" || s.band === "bad"
        ? s.band
        : ("skipped" as unknown as SubScore["band"]);
    return {
      key: s.key,
      points: s.points ?? s.score,
      // skipped → menschlicher String statt leer
      value: s.skipped ? "" : (s.value ?? ""),
      band,
      rationale: (s.rationale_key ?? "").replace(/^landing\.rat\./, ""),
      skipped: s.skipped,
      skipReason: s.reason,
      extra: s.extra ?? [],
      warning: s.warning,
      gate: s.gate ?? null,
      // Ohne den Schlüssel hieß die Bahndisziplin-Achse „Bahn-Auslastung".
      label_key: s.label_key ?? null,
    };
  });
}

// ---- Kasten „nicht bewertbar" -------------------------------------------
//
// Untersuchung 12.09.2026 (CFG 2090): Eine Landung erhielt 97 Punkte und die
// Note A+, obwohl im Aufsetzmoment 0,92 s lang keine Probe ankam. Seitdem
// gibt es dafür keine Note mehr — und dieser Kasten sagt dem Piloten, warum.
// Er steht ÜBER der Kopfzeile, damit niemand erst nach der fehlenden Zahl
// sucht.
export function NichtBewertbarKasten({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();
  if (record.landung_nicht_bewertbar == null) return null;
  return (
    <div className="landing-nicht-bewertbar" role="status">
      <div className="landing-nicht-bewertbar__titel">
        {t("landing.nicht_bewertbar.titel", {
          defaultValue: "Landung erkannt — Aufzeichnung unvollständig",
        })}
      </div>
      <div className="landing-nicht-bewertbar__text">
        {t("landing.nicht_bewertbar.text", {
          defaultValue:
            "Im Aufsetzmoment fehlen Messwerte, deshalb gibt es für diese Landung keine Bewertung. Das sagt nichts über die Landung selbst — sie wurde nur nicht vollständig aufgezeichnet.",
        })}
      </div>
      <div className="landing-nicht-bewertbar__zahlen">
        {t("landing.nicht_bewertbar.messwerte", {
          defaultValue:
            "Grösste Lücke {{luecke}} ms · {{proben}} Messpunkte im Aufsetzfenster",
          luecke: record.landung_nicht_bewertbar.groesste_luecke_ms,
          proben: record.landung_nicht_bewertbar.proben,
        })}
        {record.sampler_diagnose?.proben_je_sekunde != null && (
          <>
            {" · "}
            {t("landing.nicht_bewertbar.takt", {
              defaultValue: "{{rate}} statt 50 Messungen je Sekunde",
              rate: record.sampler_diagnose.proben_je_sekunde.toFixed(1),
            })}
          </>
        )}
      </div>
      <div className="landing-nicht-bewertbar__rat">
        {t("landing.nicht_bewertbar.rat", {
          defaultValue:
            "Häufigste Ursache ist ein ausgelasteter Simulator. Weniger Grafiklast und Zusatzprogramme im Endanflug helfen; den Client währenddessen nicht neu starten.",
        })}
      </div>
    </div>
  
  );
}

// ---- Kopfzeile ------------------------------------------------------------

export function LandungsKopf({
  record,
  callsign,
  isPreview = false,
  isNewBest = false,
  personalBest = null,
  ohnePirep = false,
}: {
  record: LandingRecord;
  /** Rufzeichen, wie die App es anzeigt (z. B. „QAF 434"). */
  callsign: string;
  isPreview?: boolean;
  isNewBest?: boolean;
  /** Bestleistung des Piloten — nur der Client kennt die eigene Historie. */
  personalBest?: LandingRecord | null;
  /** Webapp: Touchdown, zu dem der Flugbericht (PIREP) noch fehlt — dann
   *  gibt es noch keine Note (nicht „nicht bewertbar", und keine Ersatzzahl). */
  ohnePirep?: boolean;
}) {
  const { t, i18n } = useTranslation();
  return (
    <div className="landing-headline">
      <div
        className="landing-grade-big"
        style={{ background: gradeColor(record.grade_letter) }}
      >
        {record.grade_letter ?? "—"}
      </div>
      <div className="landing-headline__text">
        <h2>
          {callsign}
          {(record.dpt_airport || record.arr_airport) &&
            ` · ${record.dpt_airport || "?"} → ${record.arr_airport || "?"}`}
        </h2>
        <div className="landing-headline__sub">
          {/* v0.7.19 GAF-707: bei Confirmed Accident wird die Primary-
              Klassifikation auf "ABSTURZ ERKANNT" ueberschrieben. Score
              bleibt sichtbar (0/100), bekommt aber die Bedeutung
              "Accident" statt "normale schwere Landung". Spec §AeroACARS
              Client Tab "Landung". */}
          {record.accident === true
            ? t("landing.accident.primary_label")
            : ohnePirep && record.score_numeric == null && record.landung_nicht_bewertbar == null
            ? t("landing.kopf.ohne_pirep")
            : record.score_numeric == null
            ? t("landing.nicht_bewertbar.kurz", { defaultValue: "nicht bewertbar" })
            : recordCategory(record) != null
            ? rateCategoryWord(recordCategory(record)!, t)
            : null}
          {record.score_numeric != null ? (
            <>
              {/* Ohne gespeichertes Wort nur die Zahl — kein erfundenes
                  „FEST" (QS 06.10.2026). */}
              {recordCategory(record) != null || record.accident === true ? " · " : ""}
              {record.score_numeric}/100{" "}
              {/* Die Erklärung passt zur Version, mit der bewertet wurde —
                  Altbestand wird nie neu gerechnet (QS 06.10.2026). */}
              <InfoBadge
                explanation={t(
                  (record.score_algorithm_version ?? 0) >= 19
                    ? "landing.erklaer.kopf_note"
                    : "landing.erklaer.kopf_note_alt",
                )}
              />
            </>
          ) : null}{" "}
          ·{" "}
          {fmtDateTime(record.touchdown_at, i18n.language)}
          {isPreview && (
            <span className="landing-preview-badge">{t("landing.preview")}</span>
          )}
          {isNewBest && (
            <span className="landing-best-badge">★ {t("landing.new_best")}</span>
          )}
        </div>
        {/* Score-Version 19: warum die Note gedeckelt ist — auf dem
            Bildschirm, nicht nur im PDF (vorher nur dort). */}
        {/* Auch bei Unfall: der frühere Bericht nannte den Deckel immer
            (z. B. Überlast → 14), QS 06.10.2026. */}
        {record.score_deckel &&
          deckelText(t, record.score_deckel) && (
            <div className="landing-headline__deckel" data-testid="kopf-deckel">
              {deckelText(t, record.score_deckel)}
            </div>
          )}
        {/* QS 06.10.2026: die Zeile hing ganz am Titel — ohne ihn fehlten
            auch Kennzeichen, Muster und Simulator (der frühere PDF-Kopf
            zeigte den Simulator immer). */}
        {flugzeugZeile(record) && (
          <div className="landing-headline__aircraft">{flugzeugZeile(record)}</div>
        )}
        {personalBest && !isNewBest && istBewertbar(record) && (
          <div className="landing-headline__pb">
            {t("landing.this_landing")}: {(scoreBasisVs(record) ?? 0).toFixed(0)} fpm ·{" "}
            {t("landing.personal_best")}: {(scoreBasisVs(personalBest) ?? 0).toFixed(0)}{" "}
            fpm ({personalBest.dpt_airport} → {personalBest.arr_airport})
          </div>
        )}
        {/* v0.7.1 Phase 3 F4 + P2.4-Fix: Forensik-v2 Badge mit
            Confidence-Pill. Bedingung im Component (P1.1-C:
            ux_version >= 1 AND forensics_version >= 2). Beide
            Werte kommen jetzt sauber aus dem LandingRecord. */}
        <div style={{ marginTop: "0.5rem", display: "flex", flexWrap: "wrap", gap: "0.5rem", alignItems: "center" }}>
          <ForensicsBadge
            forensicsVersion={record.forensics_version}
            uxVersion={record.ux_version}
            confidence={record.landing_confidence}
            source={record.landing_source}
          />
          {/* v1.7.35: Sprit-Badge derselben Bauart — Reserve intakt / unter
              Reserve / nicht pruefbar. Nie rot, keine Zahl, keine Note. */}
          <SpritBadge sprit={record.sprit} />
          {/* Welche AeroACARS-Fassung die Landung aufgezeichnet hat — die
              Webapp zeigte das schon, jetzt beide. */}
          {record.client_version && (
            <span className="landing-headline__version">
              {t("landing.kopf.version", { version: record.client_version })}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}

// ---- Teilnoten ------------------------------------------------------------

export function BewertungsAbschnitt({ record, subs }: { record: LandingRecord; subs: SubScore[] }) {
  const { t } = useTranslation();
  return (
    <section className="landing-section">
      <h3>
        {t("landing.score_breakdown")}
        <InfoBadge explanation={t("landing.info.score_section")} />
      </h3>
      <ScoreBreakdown subs={subs} record={record} />
      <CoachTip subs={subs} />
    </section>
  );
}

// ---- Kopf des Bahn-Abschnitts ---------------------------------------------
//
// v0.7.6 P1-3: Bei nicht vertrauenswürdiger Bahngeometrie werden Versatz,
// Distanz hinter der Schwelle und die Bahngrafik ausgeblendet — der Pilot
// soll nicht mit kaputter Geometrie konfrontiert werden. Die Warnung sagt,
// warum. "no_runway_match" bleibt still (Privatplatz, kein Fehler).
// Bis 05.10.2026 stand die Warnung fest deutsch im Client; die Webapp
// zeigte sie gar nicht.
export function BahnUeberschrift({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();
  const vertraut = record.runway_geometry_trusted ?? true;
  const grund = record.runway_geometry_reason;
  const warnung =
    !vertraut &&
    (grund === "icao_mismatch" || grund === "centerline_offset_too_large" || grund === "negative_float_distance")
      ? t(`landing.bahn_vertrauen.${grund}`)
      : null;
  return (
    <>
      <h3>
        {t("landing.runway")}
        <InfoBadge explanation={t("landing.info.runway_section")} />
      </h3>
      {warnung && (
        <div
          className="bahn-warnung"
          style={{
            padding: "6px 10px",
            marginBottom: 10,
            borderRadius: 6,
            background: "#3f2b0e",
            border: "1px solid #b8842a",
            color: "#f5d68b",
            fontSize: "0.85rem",
          }}
        >
          ⚠ {warnung}
        </div>
      )}
      {/* Unsichere Geometrie, aber die richtige Bahn (Versatz/Aufsetzpunkt
          unplausibel): Bahn, Länge, landbarer Teil und Belag sind keine
          Ableitungen aus der Geometrie — der frühere Bericht zeigte sie, die
          Grafik entfällt hier (QS 06.10.2026). Bei falschem Platz nicht. */}
      {!vertraut && record.runway_match && grund !== "icao_mismatch" && (
        <p className="bahn-fakten" data-testid="bahn-fakten">
          {bahnFakten(record, t)}
        </p>
      )}
    </>
  );
}

function bahnFakten(record: LandingRecord, t: (k: string, o?: Record<string, unknown>) => string): string {
  const rm = record.runway_match!;
  const voll = rm.length_ft * 0.3048;
  const lda = rolloutLdaMeters(rm);
  const teile = [`${rm.airport_ident} ${rm.runway_ident}`];
  if (voll > 0) {
    teile.push(
      lda != null && Math.abs(voll - lda) >= 1
        ? `${voll.toFixed(0)} m · ${t("runway_v2.davon_landbar", { m: lda.toFixed(0) })}`
        : `${voll.toFixed(0)} m`,
    );
  }
  const belag = rm.surface ? t(surfaceLabelKey(rm.surface)) || rm.surface : null;
  if (belag) teile.push(belag);
  return teile.join(" · ");
}

/** Simulator wie im Client: „MSFS" / „X-Plane" — der Client-Datensatz
 *  führt „MSFS"/„X-PLANE", der Recorder „msfs"/„xplane" (QS 06.10.2026). */
export function simName(kind: string | null | undefined): string | null {
  if (!kind) return null;
  const k = kind.toLowerCase();
  if (k.includes("msfs")) return "MSFS";
  if (k.includes("xplane") || k.includes("x-plane")) return "X-Plane";
  return kind;
}

/** Kopfzeile „Titel · Kennzeichen · Muster · Simulator" — was da ist. */
function flugzeugZeile(record: LandingRecord): string | null {
  const teile = [
    record.aircraft_title,
    record.aircraft_registration,
    record.aircraft_icao,
    simName(record.sim_kind),
  ].filter((x): x is string => typeof x === "string" && x.trim() !== "");
  return teile.length > 0 ? teile.join(" · ") : null;
}
