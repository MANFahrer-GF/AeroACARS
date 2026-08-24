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
import type { BahnZoom } from "../lib/useBahnZoom";
import type { RunwayDiagramV2Props } from "./RunwayDiagramV2";

export interface DisziplinProps {
  props: RunwayDiagramV2Props;
  projektion: Projektion;
  /** Derselbe Zoom-Zustand wie die Längsansicht — nicht ein zweiter. */
  zoom?: BahnZoom;
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
  zoom,
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
    // ZUERST der Grund, den die BEWERTUNG gefällt hat.
    //
    // Sie kennt sieben Gründe, die Anzeige kannte fünf. Bei
    // `untrusted_geometry` und `implausible_lateral_track` wertete die
    // Achse nicht — und die Grafik daneben zeichnete seelenruhig ein Band
    // mit Randabstand, auf einer Geometrie, der die Bewertung nicht traut,
    // oder aus einem Versatz, den sie als Messfehler verworfen hat.
    //
    // Ausgelesen, nicht hergeleitet: Die Achse hat schon entschieden. Eine
    // zweite Herleitung hier wäre eine Zweitimplementierung des Urteils,
    // und die driftet, sobald jemand eine Schwelle anfasst.
    if (props.lateral_skip_reason) return props.lateral_skip_reason;

    // ZUERST: Trägt dieser Flug überhaupt v1.7.0-Daten?
    //
    // Sonst greift die nächste Prüfung und behauptet etwas über die BAHN,
    // was gar nicht an ihr liegt. Live gesehen am 23.08.2026 bei EDDS 07
    // (Flug #1062): „Für diese Bahn ist keine Breite hinterlegt" — EDDS 07
    // ist 45 m breit und steht mit Breite in den Navdaten. Der Flug kam nur
    // von einem Client vor v1.7.0, der nichts davon sendet.
    //
    // Eine Meldung, die den falschen Grund nennt, schickt die Suche in die
    // falsche Richtung: Wer sie liest, prüft die Navdaten und findet nichts.
    const traegtBahndaten =
      props.runway_width_m != null ||
      props.track_width_m != null ||
      props.clearance_point_m != null ||
      samples.length > 0;
    if (!traegtBahndaten) return "no_lateral_track";

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
          // Der Maßstab reist mit `props` — eine Quelle, kein zweiter Weg.
          schriftMindest={props.schriftMindest}
          projektion={projektion}
          runwayWidthM={breite!}
          trackWidthM={props.track_width_m ?? null}
          samples={samples}
          touchdownM={props.td_distance_from_threshold_m}
          touchdownOffsetM={props.td_centerline_offset_m}
          clearanceM={props.clearance_point_m}
          scoringCutoffM={props.scoring_cutoff_m}
          clearanceSide={props.clearance_side}
          minEdgeClearanceM={props.min_edge_clearance_m}
          maxLateralOffsetM={props.max_lateral_offset_m}
          overrunM={props.overrun_m}
          ausfahrten={props.runway_exits}
          aircraftIcao={props.aircraft_icao}
          width={width}
          zoom={zoom}
          tokens={tokens}
        />
      )}

      <Ereignisliste props={props} />

      {grund == null && <QuerLegende props={props} />}

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
    case "insufficient_samples":
      return "Zu wenige Messpunkte auf der Bahn — aus zwei oder drei Proben lässt sich kein Verlauf ablesen.";
    case "untrusted_geometry":
      return "Die Bahndaten dieser Landung sind nicht verlässlich — ohne sie ist die Kante keine belastbare Grenze.";
    case "implausible_lateral_track":
      return "Der gemessene Versatz kann nicht stimmen — vermutlich ein Messfehler, deshalb keine seitliche Bewertung.";
    case "off_airport_landing":
      return "Keine erkannte Bahn — ohne sie gibt es keine Kante, zu der ein Abstand messbar wäre.";
    case "missing_lateral_track":
      return "Für diesen Flug ist kein Rollweg erfasst. Flüge von vor v1.7.0 haben ihn nicht.";
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

  // ── Welcher Verzicht entwertet welche Zahl ───────────────────────────
  //
  // Die Ereignisliste lief bis Runde 22 IMMER — auch wenn die Bewertung
  // die seitliche Lage verworfen hatte. Sie zeigte dann „äusseres Rad
  // 3,2 m vor der Kante" neben einem Hinweis, der genau das für
  // unbrauchbar erklärt.
  //
  // Es sind aber nicht alle Gründe gleich: Auf einer Graspiste ist der
  // gemessene VERSATZ in Ordnung, nur die Kante ist fliessend. Deshalb
  // wird unterschieden, statt pauschal alles wegzulassen — sonst
  // verschwände eine Messung, die stimmt.
  const versatzEntwertet = [
    // Der Versatz selbst ist unglaubwürdig.
    "implausible_lateral_track",
    // Die Bahnachse, auf die projiziert wurde, ist es nicht.
    "untrusted_geometry",
    "off_airport_landing",
    // Aus zwei, drei Proben lässt sich kein Grösstwert ablesen.
    "insufficient_samples",
  ].includes(props.lateral_skip_reason ?? "");
  // Die Kante trägt zusätzlich dann nicht, wenn sie keine feste Grenze ist.
  const kanteEntwertet =
    versatzEntwertet ||
    ["unpaved_runway", "surface_unknown", "water_runway"].includes(
      props.lateral_skip_reason ?? "",
    );

  const max = versatzEntwertet ? null : props.max_lateral_offset_m;
  if (max != null) {
    const rand = kanteEntwertet ? null : props.min_edge_clearance_m;
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
    // Die Laengsposition gehoert dazu: „26,9 m rechts" allein sagt nicht,
    // ob das kurz nach dem Aufsetzen passierte oder erst beim Abbiegen.
    const bewertungsEnde = props.scoring_cutoff_m ?? props.clearance_point_m;
    const wo = (props.lateral_samples ?? [])
      .filter((x) => bewertungsEnde == null || x.laengs_m < bewertungsEnde)
      .reduce<
      { laengs_m: number; quer_m: number } | null
    >((a, b) => (a == null || Math.abs(b.quer_m - max) < Math.abs(a.quer_m - max) ? b : a), null);
    const beiM =
      wo != null
        ? `${t("runway_v2.at_position", {
            defaultValue: "bei {{m}} m",
            m: wo.laengs_m.toFixed(0),
          })} · `
        : "";
    eintraege.push({
      n: 2,
      text: `${t("runway_v2.mark.max_offset", {
        defaultValue: "Grösster Versatz",
      })} · ${beiM}${seite(max, t)}${zusatz}`,
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

  // Endpunkt ohne Ausfahrt — dieselbe Bedingung wie die Marke in der
  // Grafik, damit Bild und Liste dieselben Nummern führen.
  // Dieselbe Bedingung wie die Marke in der Grafik — auch das Überrollen
  // schliesst den Endpunkt aus, denn Marke ④ sitzt bereits am Bahnende.
  if (props.clearance_point_m == null && (props.overrun_m ?? 0) <= 0) {
    const s = props.lateral_samples ?? [];
    const letzter = s.length >= 2 ? s[s.length - 1]! : null;
    const max = props.max_lateral_offset_m;
    const beiMax =
      max != null && letzter != null && Math.abs(letzter.quer_m - max) < 0.5;
    if (letzter && !beiMax) {
      eintraege.push({
        n: eintraege.length + 1,
        text: `${t("runway_v2.mark_track_end", {
          defaultValue: "Ende der Aufzeichnung",
        })} · ${fmt(letzter.laengs_m)} m · ${seite(letzter.quer_m, t)} · ${t(
          "runway_v2.mark_slowed",
          { defaultValue: "Auf Rollgeschwindigkeit" },
        )}`,
      });
    }
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

// ─── Legende der Queransicht ───────────────────────────────────────────
//
// Die vorhandene Legende gehört zur Längsansicht und erklärt Schwelle,
// Aufsetzzone und Zielmarkierung. Die Queransicht zeigt anderes: den
// gefahrenen Streifen, seine Messpunkte, die Ausfahrten und den Bogen zur
// genutzten. Ohne eigene Legende muss man raten, was die dünnen Striche am
// Rand bedeuten.

function QuerLegende({ props }: { props: RunwayDiagramV2Props }) {
  const { t } = useTranslation();
  const n = props.lateral_samples?.length ?? 0;
  const eintraege: Array<{ farbe: string; text: string; gestrichelt?: boolean }> = [
    {
      farbe: "#22c55e",
      text: t("runway_v2.legend_td", { defaultValue: "Aufsetzpunkt (TD)" }),
    },
    {
      farbe: "#38bdf8",
      text: t("runway_v2.legend_track", {
        defaultValue: "Spur — {{n}} gemessene Stützpunkte",
        n,
      }),
    },
  ];
  // Nur am Räumpunkt, NICHT an der Ausfahrtsseite.
  //
  // Die gestrichelte Spur im Bild hängt allein an `clearance_point_m`
  // (siehe `trennIdx` in RunwayCrossSection). Hing die Legende zusätzlich
  // an der Seite, fehlte die Erklärung genau dann, wenn die Richtung nicht
  // eindeutig war — und im Bild stand eine gestrichelte Linie, die
  // niemand deutet.
  //
  // Die Richtung ist bewusst oft leer: Sie wird nur gesetzt, wenn Kurs UND
  // Querbewegung dasselbe sagen (Spec §8.6). Das ist der Normalfall für
  // eine unklare Ausfahrt, nicht die Ausnahme.
  if (props.clearance_point_m != null) {
    eintraege.push({
      farbe: "#38bdf8",
      gestrichelt: true,
      text: t("runway_v2.legend_exit_arc", {
        defaultValue: "Ausfahrt — Richtung echt, ab hier nicht mehr gewertet",
      }),
    });
  }
  // Ausfahrten: entweder erklären, was die Stummel bedeuten — oder sagen,
  // warum keine da sind.
  //
  // Kleine Plätze haben oft keine OpenStreetMap-Bodenkarte; EDXB und EDXF
  // etwa sind gar nicht erfasst. Ohne Hinweis liest sich das wie „hier gibt
  // es keine Ausfahrten", und das ist etwas anderes als „wir wissen es
  // nicht".
  if ((props.runway_exits?.length ?? 0) > 0) {
    eintraege.push({
      farbe: "#4E6350",
      text: t("runway_v2.legend_exits", {
        defaultValue: "Ausfahrten (OSM) · genutzte hervorgehoben",
      }),
    });
  } else {
    eintraege.push({
      farbe: "#334155",
      text: t("runway_v2.exits_none", {
        defaultValue: "Für diesen Platz sind keine Rollwege hinterlegt",
      }),
    });
  }
  // Der Grünstreifen NEBEN der Bahn — nicht die Bahn selbst.
  //
  // Der Eintrag hiess „unbefestigt" und stand unter einer Legende, die
  // sonst nur von der Bahn handelt. Bei EDLW 24 (Asphalt, B738) las sich
  // das als Aussage über die Landebahn.
  //
  // Verkehrt war es doppelt: Er erschien bei allen fünf Varianten mit
  // befestigter Bahn — und gerade NICHT bei Gras und Wasser, weil dort
  // die Queransicht mitsamt Legende entfällt.
  eintraege.push({
    farbe: "#3F6B4A",
    text: t("runway_v2.legend_shoulder", {
      defaultValue: "neben der Bahn — unbefestigt",
    }),
  });

  return (
    <div
      style={{
        display: "flex",
        flexWrap: "wrap",
        gap: "5px 18px",
        fontSize: "0.72rem",
        color: "#94a3b8",
      }}
    >
      {eintraege.map((e) => (
        <span key={e.text} style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
          <span
            style={{
              display: "inline-block",
              width: 11,
              height: e.gestrichelt ? 0 : 11,
              borderRadius: e.gestrichelt ? 0 : 2,
              background: e.gestrichelt ? "none" : e.farbe,
              borderTop: e.gestrichelt ? `2px dashed ${e.farbe}` : undefined,
            }}
          />
          {e.text}
        </span>
      ))}
    </div>
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
      // Mit dem Muster dahinter: „Spannweite 51,7 m" ist eine Zahl,
      // „Spannweite MD-11 51,7 m" ist eine Aussage — man erkennt sofort,
      // ob der Vergleich zum eigenen Flugzeug passt.
      label: props.aircraft_icao
        ? `${t("runway_v2.scale.wingspan", { defaultValue: "Spannweite" })} ${props.aircraft_icao}`
        : t("runway_v2.scale.wingspan", { defaultValue: "Spannweite" }),
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
        // Raster statt Flexbox, und `minmax(0, 1fr)` für die Balkenspalte.
        //
        // Mit `flex: 0 0 88px` für den Namen lief die Zeile über, sobald der
        // Name länger war als seine Spalte — „Spannweite A321" braucht mehr,
        // und Flex-Elemente schrumpfen nicht unter ihre Inhaltsbreite. Der
        // Block ragte damit achtunddreissig Pixel über seinen Container
        // hinaus, und die Zahlen rechts wurden abgeschnitten (§8.6.5). Die
        // Referenzgrafik löst es mit demselben Raster: 132 / minmax(0,1fr) / 58.
        <div
          key={z.label}
          style={{
            display: "grid",
            gridTemplateColumns: "132px minmax(0, 1fr) 58px",
            alignItems: "center",
            gap: 10,
            fontSize: "0.75rem",
          }}
        >
          <span style={{ color: "#94a3b8", minWidth: 0 }}>{z.label}</span>
          <span
            style={{
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
              color: "#cbd5e1",
              textAlign: "right",
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
