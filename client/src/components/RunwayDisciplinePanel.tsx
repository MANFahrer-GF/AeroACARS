// Bahndisziplin-Block: Queransicht, Ereignisliste, Grössenvergleich.
//
// Spec: `docs/spec/v1.7.0-bahndisziplin.md` §8.3, §8.5, §8.6.
//
// Sitzt unter der Längsansicht und teilt sich mit ihr die Projektion — das
// ist der Punkt: Beide Ansichten zeigen denselben Ausschnitt mit identischen
// Kanten, damit der Aufsetzpunkt oben senkrecht über der Marke unten liegt.

import { useTranslation } from "react-i18next";
import { RunwayCrossSection } from "./RunwayCrossSection";
import type { Projektion } from "../lib/runwayProjection";
import type { RunwayDiagramV2Props } from "./RunwayDiagramV2";

export interface DisziplinProps {
  props: RunwayDiagramV2Props;
  projektion: Projektion;
  width: number;
  tokens: {
    tarmac: string;
    tarmacBorder: string;
    centerline: string;
    rollout: string;
    tdPerfect: string;
    tdWarn: string;
    tdSevere: string;
  };
}

export function RunwayDisciplinePanel({
  props,
  projektion,
  width,
  tokens,
}: DisziplinProps) {
  const { t } = useTranslation();

  const samples = props.lateral_samples ?? [];
  const breite = props.runway_width_m ?? null;

  // ── Warum hier nichts geraten wird ───────────────────────────────────
  //
  // Fehlt die Bahnbreite oder die Spur, entfällt die Queransicht **sichtbar**
  // — mit dem Grund, aus dem sie entfällt. Eine leere Querachse zu malen wäre
  // schlimmer als gar keine: Sie sähe aus wie eine Messung, die „nichts
  // Auffälliges" ergeben hat, und genau das steht dann nicht fest.
  const grund = ((): string | null => {
    if (breite == null || breite <= 0) return "runway_width_unknown";
    if (samples.length < 2) return "no_lateral_track";
    if (props.surface_paved === false) return "unpaved_runway";
    if (props.surface_paved == null) return "surface_unknown";
    if (props.track_width_m == null) return "track_width_unknown";
    return null;
  })();

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      {grund ? (
        <Hinweis text={t(`runway_v2.discipline_skip.${grund}`, skipText(grund))} />
      ) : (
        <RunwayCrossSection
          projektion={projektion}
          runwayWidthM={breite!}
          trackWidthM={props.track_width_m ?? null}
          samples={samples}
          touchdownM={props.td_distance_from_threshold_m}
          touchdownOffsetM={props.td_centerline_offset_m}
          clearanceM={props.clearance_point_m}
          clearanceSide={props.clearance_side}
          minEdgeClearanceM={props.min_edge_clearance_m}
          maxLateralOffsetM={props.max_lateral_offset_m}
          overrunM={props.overrun_m}
          width={width}
          tokens={tokens}
        />
      )}

      <Ereignisliste props={props} />

      {breite != null && breite > 0 && <Groessenvergleich props={props} breiteM={breite} />}
    </div>
  );
}

/** Der ausgeschriebene Grund, warum die Queransicht entfällt. */
function skipText(grund: string): string {
  switch (grund) {
    case "runway_width_unknown":
      return "Für diese Bahn ist keine Breite hinterlegt — die Queransicht braucht sie als Massstab.";
    case "no_lateral_track":
      return "Für diesen Flug ist kein Rollweg erfasst. Flüge von vor v1.7.0 haben ihn nicht.";
    case "unpaved_runway":
      return "Gras- oder Naturpiste: Der Rand ist fliessend, eine Kante lässt sich nicht bemassen.";
    case "surface_unknown":
      return "Der Belag dieser Bahn ist nicht bekannt — ohne ihn ist die Kante keine belastbare Grenze.";
    case "track_width_unknown":
      return "Die Spurweite dieses Musters ist nicht hinterlegt; ohne sie lässt sich die Lage der Räder nicht bestimmen.";
    default:
      return "Die Queransicht steht für diesen Flug nicht zur Verfügung.";
  }
}

function Hinweis({ text }: { text: string }) {
  return (
    <div
      style={{
        padding: "10px 12px",
        borderRadius: 6,
        background: "rgba(148,163,184,0.10)",
        border: "1px solid rgba(148,163,184,0.25)",
        fontSize: "0.82rem",
        color: "#94a3b8",
        lineHeight: 1.5,
      }}
    >
      {text}
    </div>
  );
}

// ─── Ereignisliste ─────────────────────────────────────────────────────
//
// §8.5: Ereignisse stehen in der Grafik nur als nummerierte Marke; der Text
// steht darunter. Damit kann im Bild nichts überlappen, und bei einer Landung
// mit mehr Ereignissen wächst die Liste statt des Gedränges in der Grafik.

function Ereignisliste({ props }: { props: RunwayDiagramV2Props }) {
  const { t } = useTranslation();
  const eintraege: Array<{ n: number; text: string }> = [];

  eintraege.push({
    n: 1,
    text: `${t("runway_v2.mark.touchdown", { defaultValue: "Aufsetzen" })} · ${fmt(
      props.td_distance_from_threshold_m,
    )} m ${t("runway_v2.mark.past_threshold", { defaultValue: "hinter der Schwelle" })} · ${seite(
      props.td_centerline_offset_m,
      t,
    )}`,
  });

  const max = props.max_lateral_offset_m;
  if (max != null) {
    const rand = props.min_edge_clearance_m;
    const zusatz =
      rand == null
        ? ""
        : rand < 0
        ? ` · ${t("runway_v2.mark.off_pavement", {
            defaultValue: "äusseres Rad {{m}} m neben der befestigten Fläche",
            m: Math.abs(rand).toFixed(1),
          })}`
        : ` · ${t("runway_v2.mark.edge_distance", {
            defaultValue: "äusseres Rad {{m}} m vor der Kante",
            m: rand.toFixed(1),
          })}`;
    eintraege.push({
      n: 2,
      text: `${t("runway_v2.mark.max_offset", { defaultValue: "Grösster Versatz" })} · ${seite(
        max,
        t,
      )}${zusatz}`,
    });
  }

  if (props.clearance_point_m != null) {
    const seiteTxt =
      props.clearance_side === "left"
        ? t("runway_v2.side_left_word", { defaultValue: "links" })
        : props.clearance_side === "right"
        ? t("runway_v2.side_right_word", { defaultValue: "rechts" })
        : // §8.6: Stimmen Kurs und Querbewegung nicht überein, wird die
          // Richtung NICHT behauptet. Eine falsche Seite ist schlimmer als
          // keine — sie liest sich wie eine Messung.
          t("runway_v2.side_unclear", { defaultValue: "Richtung nicht eindeutig" });
    const tempo =
      props.clearance_speed_kt != null
        ? ` · ${props.clearance_speed_kt.toFixed(0)} kt`
        : "";
    eintraege.push({
      n: 3,
      text: `${t("runway_v2.mark.cleared", { defaultValue: "Bahn geräumt" })} · ${fmt(
        props.clearance_point_m,
      )} m${tempo} · ${seiteTxt}`,
    });
  }

  if (props.overrun_m != null && props.overrun_m > 0) {
    eintraege.push({
      n: 4,
      text: `${t("runway_v2.mark.overrun", {
        defaultValue: "Über das Bahnende hinaus",
      })} · ${fmt(props.overrun_m)} m`,
    });
  }

  return (
    <ol
      style={{
        listStyle: "none",
        margin: 0,
        padding: 0,
        display: "flex",
        flexDirection: "column",
        gap: 6,
      }}
    >
      {eintraege.map((e) => (
        <li
          key={e.n}
          style={{
            display: "flex",
            gap: 8,
            alignItems: "baseline",
            fontSize: "0.82rem",
            color: "#cbd5e1",
            lineHeight: 1.5,
          }}
        >
          <span
            style={{
              flex: "0 0 auto",
              width: 18,
              height: 18,
              borderRadius: "50%",
              background: "rgba(148,163,184,0.18)",
              color: "#e2e8f0",
              fontSize: "0.7rem",
              fontWeight: 700,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            {e.n}
          </span>
          <span>{e.text}</span>
        </li>
      ))}
    </ol>
  );
}

// ─── Grössenvergleich ──────────────────────────────────────────────────
//
// §8.3: „Das Grössenverhältnis gehört als massstäblicher Balkenvergleich unter
// die Grafik. Die Spannweite ragt dort sichtbar über die Bahnbreite hinaus —
// nur so versteht man, warum die Fahrspur so schmal wirkt."
//
// §8.6.5: Auf den **grössten** Wert normiert, nicht auf die Bahnbreite. Sonst
// schiebt der längste Balken den Container auf und die Seite scrollt seitlich
// — genau das ist im Entwurf vom 23.08. passiert, mit 114,6 % Balkenbreite.

function Groessenvergleich({
  props,
  breiteM,
}: {
  props: RunwayDiagramV2Props;
  breiteM: number;
}) {
  const { t } = useTranslation();
  const zeilen = [
    {
      label: t("runway_v2.scale.runway_width", { defaultValue: "Bahnbreite" }),
      m: breiteM,
      farbe: "#64748b",
    },
    props.wingspan_m != null && {
      label: t("runway_v2.scale.wingspan", { defaultValue: "Spannweite" }),
      m: props.wingspan_m,
      farbe: "#38bdf8",
    },
    props.track_width_m != null && {
      label: t("runway_v2.scale.track_width", { defaultValue: "Spurweite" }),
      m: props.track_width_m,
      farbe: "#22c55e",
    },
  ].filter(Boolean) as Array<{ label: string; m: number; farbe: string }>;

  const groesster = Math.max(...zeilen.map((z) => z.m));
  if (!Number.isFinite(groesster) || groesster <= 0) return null;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
      {zeilen.map((z) => (
        <div
          key={z.label}
          style={{ display: "flex", alignItems: "center", gap: 8, fontSize: "0.75rem" }}
        >
          <span style={{ flex: "0 0 88px", color: "#94a3b8" }}>{z.label}</span>
          <span
            style={{
              flex: "1 1 auto",
              minWidth: 0,
              height: 8,
              background: "rgba(148,163,184,0.10)",
              borderRadius: 4,
              overflow: "hidden",
            }}
          >
            <span
              style={{
                display: "block",
                height: "100%",
                // Normiert auf den grössten Wert — nie über 100 %.
                width: `${(z.m / groesster) * 100}%`,
                background: z.farbe,
                borderRadius: 4,
              }}
            />
          </span>
          <span
            style={{
              flex: "0 0 auto",
              color: "#cbd5e1",
              fontVariantNumeric: "tabular-nums",
            }}
          >
            {z.m.toFixed(1)} m
          </span>
        </div>
      ))}
    </div>
  );
}

// ─── Helfer ────────────────────────────────────────────────────────────

function fmt(m: number): string {
  return Number.isFinite(m) ? m.toFixed(0) : "—";
}

/**
 * Versatz als Seitenangabe. §8.6: Ein Pilot denkt in Seiten, nicht in
 * Koordinaten — intern bleibt `quer > 0 = rechts`, die Umrechnung geschieht
 * hier und in der Queransicht, sonst nirgends.
 */
function seite(
  quer_m: number,
  t: (k: string, o: { defaultValue: string }) => string,
): string {
  if (!Number.isFinite(quer_m)) return "—";
  const betrag = Math.abs(quer_m).toFixed(1);
  if (Math.abs(quer_m) < 0.5) {
    return t("runway_v2.on_centerline", { defaultValue: "auf der Mittellinie" });
  }
  return quer_m > 0
    ? `${betrag} m ${t("runway_v2.side_right_word", { defaultValue: "rechts" })}`
    : `${betrag} m ${t("runway_v2.side_left_word", { defaultValue: "links" })}`;
}
