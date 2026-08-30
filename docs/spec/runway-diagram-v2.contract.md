# Runway Diagram v2 — Contract & Spec

**Status:** DRAFT / QS ROUND 2
**Datum:** 2026-05-13
**Autor:** Slice „Runway Geometry v2 — Display-Only Polish nach v0.8.0"
**Anchor-Case:** GSG Pilot-Live-Befund 13.05.2026 (Runway-Diagram zu klein, Begriffe unverständlich, Pilot-Client vs. VPS-Webapp drifteten)
**Status-Begründung:** QS-Review-1 hat 3 Blocker + 3 Mediums + 2 Lows gemeldet (siehe §QS-Historie). Adressiert in dieser Revision. APPROVED erst nach Sign-off + Implementation-Round-Trip.

---

## Zweck

Eine einheitliche, gut lesbare Runway-Geometrie-Darstellung in **Pilot-Client „Landung"-Tab** UND **VPS-Webapp `LandingAnalysis`**. Display-Only Polish — **keine** neuen Score-Felder, **keine** neuen Wire-Format-Erweiterungen. Konsumiert ausschließlich die in v0.8.0/v0.8.1 bereits eingeführten `LandingRecord`-/`TouchdownPayload`-Felder.

## Leitprinzipien

1. **Display-Only.** Kein neues Scoring, kein neues Backend-Feld, keine neuen Spec-Anforderungen an `landing-scoring` oder `runway_assessment`. Nur die existierenden v0.8.0-Felder werden gerendert.
2. **Offline-fähig im Pilot-Client.** Pilot-Client persistiert den `LandingRecord` schon heute lokal in `<app_data_dir>/landings.json` (storage-crate). Das Diagram liest **ausschließlich** daraus (für abgeschlossene PIREPs) oder aus dem **In-Memory-`FlightStats`** (für den noch laufenden, nicht persistierten Flug) — **kein** VPS-Fetch zur Anzeige nötig. Pilot kann das Diagramm offline ansehen.
3. **Eine Spec, zwei Implementierungen — mit dokumentierten Ausnahmen.** Pilot-Client (TSX, React 19) und Webapp (TSX, React 19) implementieren **gegen denselben Contract**. Identische Props-Interface, identische visuelle Ausgabe **außer wo das Wire-Format Felder fehlen** (siehe §Surface — Bekannte Abweichung). Snapshot-Test-Fixture in beiden Repos verhindert Drift.
4. **Verständlich für Piloten, nicht für Aviation-Lawyers.** Begriffe werden in einfacher Sprache erklärt (siehe §Glossar). Hover-Tooltips + Glossar-Modal `[ⓘ]`.
5. **Responsive, breit.** Diagram nutzt die volle Container-Breite. Min Container 480 px, Max sinnvoll 1400 px. SVG-Höhe ≈ 280–320 px (Faktor ~2× größer als v0.8.0-Original).

---

## Datenquellen

### Pilot-Client (3 Quellen, in Priorität absteigend)

1. **In-Memory `FlightStats`** — der gerade aktive, noch laufende Flug. Live-Werte aus `ActiveFlight.stats` (Rust-side via `landing_get_current` Tauri-Command). **Kein** Disk-I/O, kein VPS-Fetch.
2. **Lokal persistiert `landings.json`** — abgeschlossene PIREPs via `landing_list` Tauri-Command. Disk-Read, kein Netzwerk.
3. **Kein Network-Pfad** — das Diagram ruft NIE `aeroacars_mqtt::navdata::*` oder phpVMS-APIs zur Anzeige auf.

### Webapp (eine Quelle)

- `TouchdownDto.payload as Record<string, unknown>` aus `/api/touchdowns/:id`. Recorder pipet den MQTT-TouchdownPayload unverändert durch.

**Wire-Identität:** Beide Strukturen tragen seit Slice A.4 identische v0.8.0-Felder (verifiziert in dieser Revision gegen `crates/aeroacars-mqtt/src/lib.rs`). **Ausnahme:** Surface — siehe §Surface unten.

---

## Surface — Bekannte Abweichung Client ↔ VPS

**Status:** Display-Asymmetrie bewusst akzeptiert.

`LandingRecord.runway_match.surface` (Pilot-Client) trägt den Surface-Code (`ASP`/`CON`/`GRS`/...). Im **MQTT TouchdownPayload** (= Quelle der Webapp) existiert dieses Feld **nicht** — wurde in Slice A.4 nicht aufgenommen weil das Wire-Format ohnehin schon ~80 Felder hatte und Surface display-only ist (kein Score-Impact).

**Entscheidung für v2:** Surface bleibt im Pilot-Client-Header, **fehlt** in der Webapp. UI-Konsequenz: Pilot-Client-Header zeigt `"EDDK · Bahn 24 · 2459 m · Beton"`; Webapp-Header zeigt `"EDDK · Bahn 24 · 2459 m"`. Dokumentiert und akzeptiert als Display-Asymmetrie — KEIN Mismatch im funktionalen Sinne.

**Wenn das später stört:** Wire-Erweiterung `TouchdownPayload.runway_surface_code: Option<String>` ist trivial (additiv, kein Breaking). Aktuell out of scope.

---

## Glossar (Pilot-freundliche Erklärungen)

Diese Erklärungen erscheinen 1:1 in der UI — als Tooltips auf Hover und im Glossar-Modal.

| Abk. / Begriff | Voll | Pilot-Erklärung |
|---|---|---|
| **Threshold (THR)** | Bahnschwelle | Die großen weißen Querstreifen am Bahnanfang. Ab dieser Linie darfst du landen. |
| **Touchdown (TD)** | Aufsetzen | Der Moment, in dem die Räder den Bahnbelag berühren. |
| **Centerline (CL)** | Mittellinie | Die gestrichelte weiße Linie genau in der Mitte der Bahn. |
| **Centerline-Offset / XTD** | Seitenabweichung | Wie weit links oder rechts von der Mittellinie bist du aufgesetzt? Idealwert: 0 m. |
| **TDZ — Touchdown Zone** | Aufsetzzone | Soll-Bereich zum Aufsetzen — proportional zur NUTZBAREN Bahnlänge (LDA, also hinter der versetzten Schwelle), gedeckelt bei 900 m. Formel: TDZ-Länge = min(LDA ÷ 3, 900 m). 1500 m LDA → 500 m TDZ; 2100 m → 700 m; ab 2700 m greift der 900-m-Cap. Ab 1200 m LDA überhaupt erst gemalt. ICAO Annex 14 mit Gruppen weißer Querstreifen entlang der Centerline. **v1.6.8:** vorher stand hier die volle Bahnlänge — auf versetzten Bahnen war die Zone dadurch zu lang. |
| **AIM — Aim Point** | Ziel-Markierung | Zwei breite weiße Streifen auf der Bahn (symmetrisch zur Centerline, ICAO Annex 14 §5.2.6). Pilot zielt im Anflug optisch drauf, setzt durch den Flare 50–150 m DAHINTER auf (= Anfang der TDZ). Position (AeroACARS 2-Bucket-Logik, FAA AIM 8-9-1 / ICAO Annex 14): **LDA** ≥ 2400 m → 400 m hinter Schwelle, LDA < 2400 m → 300 m hinter Schwelle. Maßgeblich ist die nutzbare Länge, nicht die Bahnfläche — LICC 08 misst 2436 m, hat aber nur 2341 m LDA und damit die 300-m-Markierung. |
| **TCH — Threshold Crossing Height** | Schwellen-Überflug-Höhe | Wie hoch warst du über dem Boden, als du die Schwelle überflogen hast? ILS-Anflug typisch 49 ft (≈ 15 m). Zu niedrig: Tail-Strike-Risiko. Zu hoch: Long-Landing. |
| **DDS — Displaced Threshold** | Versetzte Schwelle | Manche Bahnen haben einen Bereich VOR der echten Landeschwelle, der für die Landung verboten ist (Pfeile auf der Bahn). Aufsetzen davor = illegal. Beispiel: OLBA RWY 35, 820 m DDS. **Woher der Wert kommt (v1.6.8):** der Aerosoft-DFD-Export liefert ihn seit AIRAC 2608 nicht mehr als Zahl, sondern versetzt den Schwellenpunkt selbst — `displaced_threshold_ft` steht dann auf 0, `length_ft` bleibt die volle Bahn. Client und Recorder holen die Zahl deshalb aus der OurAirports-Tabelle und prüfen sie gegen die Geometrie (Bahnlänge minus BEIDE Versätze muss dem Abstand Schwelle→Bahnende entsprechen). Wichtig: die gemessene Aufsetzdistanz ist bei dieser Konvention bereits ab der Lande-Schwelle — sie darf NICHT ein zweites Mal um den Versatz gekürzt werden. |
| **Glide Slope** | Anflug-Winkel | ILS-Standard 3°. Du sinkst 1 m für je 19 m vorwärts. |
| **Rollout** | Ausrollstrecke | Wie viele Meter rollst du nach dem Aufsetzen, bis du auf ~40 kt abgebremst hast — das ist die typische High-Speed-Exit-Geschwindigkeit, mit der du am nächsten Rollwege-Abzweig die Bahn verlässt. Bis zum vollen Stand auf der Bahn rollt fast niemand aus (= ROLLOUT_STOP_GS_KT = 40 in src-tauri/src/lib.rs:4214). |
| **Bahn-Auslastung** | — | Ausrollstrecke ÷ Bahnlänge × 100 %. 80 % = nur 20 % Bahn übrig (knapp). |
| **AIRAC-Cycle** | — | Offizielle Aviation-Daten werden alle 28 Tage aktualisiert. „Cycle 2604" = 4. Update 2026. |
| **VPS Navdata** | — | Zentrale, vom VA-Admin gepflegte AIRAC-Daten auf dem VPS. Pilot-Client zieht sie pro Flugstart. Technische Quelle dahinter: Aerosoft DFD (Lizenz: VA-Admin-Subscription). |
| **OurAirports** | — | Community-Wiki-Datenquelle als Fallback wenn der VPS nicht erreichbar ist. Schwellen-Positionen können abweichen. |
| **AGL** | Above Ground Level | Höhe über Grund (nicht über Meer). |
| **fpm** | Feet per Minute | Sinkrate-Einheit. Negativ = Sinkflug. |
| **kt** | Knots / Knoten | Geschwindigkeitseinheit, ≈ 1.852 km/h. |

**Total: 17 Begriffe.**

---

## Layout

Das Diagram ist in **4 Bereiche** vertikal gestapelt, alle nutzen die volle Container-Breite.

### 1. Header (`<header>`)

```
🛬 LANDEBAHN-ANALYSE                                              [ⓘ Hilfe]

Flughafen EDDK (Köln-Bonn)  ·  Bahn 24  ·  2459 m  [· Beton ← nur im Pilot-Client]
Datenquelle: VPS Navdata (AIRAC 2604) ✓
ODER: Datenquelle: OurAirports (Fallback) — Schwellen-Position kann abweichen
```

- Title-Icon + „LANDEBAHN-ANALYSE" als h3
- Hilfe-Button rechts → öffnet Glossar-Modal
- Subline 1: Flughafen-Name (falls bekannt) + Bahn + Länge + **Surface nur wenn `surface != null`** (= Pilot-Client; Webapp blendet das Feld aus)
- Subline 2: Datenquelle. Bei `source === "navigraph"`: „Datenquelle: VPS Navdata (AIRAC 2604) ✓". Bei `source === "ourairports_fallback"`: „Datenquelle: OurAirports (Fallback) — Schwellen-Position kann abweichen". Bei `source === null` (pre-v0.8.0): „Datenquelle: OurAirports". **Sprachlich neutral** — kein direktes „Navigraph"-Wording im UI (Lizenz-Vorsicht); intern bleibt der Wire-String unverändert `"navigraph"`.

### 2. Hauptgrafik (`<svg>`)

**SVG viewBox:** `0 0 1200 320` — preserveAspectRatio `xMidYMid meet`.

**Logische Schichten (z-order von unten nach oben):**

1. **Bahn-Asphalt** — `<rect>` mit dunklem Tarmac-Tone (`#1a2030`), umrandet `rgba(255,255,255,0.18)`.
2. **Distanz-Skala** unter der Bahn — Tick-Marker bei 0, 300, 600, 900, 1200, 1500, 1800, 2100, 2400, 3000, 3600, 4200 m (nur die, die ≤ Bahn-Länge sind). Labels in Monospace, dezent.
3. **Pre-Threshold-Zone (DDS)** — nur wenn `displaced_threshold_m > 0`. Rote schraffierte Fläche `rgba(124,45,18,0.45)` über die ersten `displaced_threshold_m`.
4. **Threshold-Streifen** — links der Bahn, Block aus 8 weißen vertikalen Strichen.
5. **TDZ-Box** — nur wenn `td_tdz_length_m != null`. Schraffur-Pattern in Gelb (45°-Linien) statt einfache Fläche — gibt ICAO-konforme Optik.
6. **Centerline** — gestrichelte horizontale Linie durch die Bahn-Mitte (`#a3a3a3`, dasharray 14,10).
7. **Aim-Point-Markierung** — nur wenn `aim_point_m != null`. Vier kleine weiße Quadrate ober- und unterhalb der CL (≈ ICAO-Aiming-Streifen) + Pfeil-Down + Label „ZIEL 400 m".
8. **Rollout-Linie** — von TD zum errechneten Exit-Punkt (`td_distance + rollout_m`). Linie in Cyan `#22d3ee`, mit Glow.
9. **Touchdown-Punkt** — Glow-Ring r=18 (Opacity 18 %) + solid-Kreis r=9, Color nach `aim_class` (siehe `tdColor` in Implementierung).
10. **Exit-Punkt** — kleiner orange Kreis am Ende der Rollout-Linie, Label „EXIT".
11. **RWY-Designator** — groß als Asphalt-Schrift links der Bahn (`28 px Monospace, fontWeight 800`).
12. **Landerichtungs-Pfeil** — kleines Dreieck rechts der Bahn (→).

**Annotationen unterhalb der Bahn (im SVG):**

- TD-Label unter dem TD-Punkt: „TD 358 m" + zweite Zeile mit CL-Offset
- Skala-Label unter dem Tick: „300 m", „600 m" etc.

### 3. Legende

Direkt unter dem SVG, eine Zeile (ggf. wrap auf schmal). Jedes Symbol mit Mini-Swatch + Label, gleiche Color-Codierung wie im SVG. DDS-Eintrag nur sichtbar wenn DDS aktiv.

### 4. Detail-Karten (Cards)

4 Karten in einer Row (auf Desktop), 2×2-Grid bei Container < 900 px, gestapelt bei < 600 px.

| Card | Header | Inhalt |
|---|---|---|
| **AUFSETZ-BEWERTUNG** | „Aufsetz-Bewertung" | TDZ-Hit ja/nein + Drittel · Aim-Class + Δ · DDS-Warnung wenn `pre_displaced_threshold == true` |
| **POSITION** | „Position" | Hinter Schwelle: 358 m · Mittellinie: 8.8 m LINKS · Ausrollen: 1979 m (80 %) |
| **ANFLUG-PROFIL** | „Anflug-Profil" | TCH 47 ft (Soll 49) · Δ −2 ft auf Profil. **Card erscheint NICHT wenn `tch_actual_ft == null`** (Spec § TCH optional). |
| **DATENQUELLE** | „Datenquelle" | „VPS Navdata (AIRAC 2604) ✓" ODER „⚠ OurAirports Fallback (Schwellen können abweichen)" |

Jede Card: leichter Border, semi-transparenter Hintergrund, gleiche Höhe.

---

## Props-Interface

Component-Signatur (TypeScript):

```ts
export interface RunwayDiagramV2Props {
  airport_ident: string;            // "EDDK"
  airport_name?: string | null;     // "Köln-Bonn"
  runway_ident: string;             // "24"
  length_m: number;                 // 2459

  /** Surface-Code (z.B. "ASP"/"CON"). Optional weil das Webapp-Wire-
   *  Format dieses Feld NICHT trägt (Slice A.4 ausgelassen). Pilot-
   *  Client setzt es aus LandingRecord, Webapp übergibt null. */
  surface?: string | null;

  source: "navigraph" | "ourairports_fallback" | null;
  nav_cycle?: string | null;
  displaced_threshold_m?: number;

  td_distance_from_threshold_m: number;
  td_centerline_offset_m: number;

  td_in_tdz?: boolean | null;
  td_third?: 1 | 2 | 3 | null;
  td_tdz_length_m?: number | null;
  aim_point_m?: number | null;
  aim_delta_m?: number | null;
  aim_class?: AimClass | null;
  tch_actual_ft?: number | null;
  tch_expected_ft?: number | null;
  tch_delta_ft?: number | null;
  tch_class?: TchClass | null;
  pre_displaced_threshold?: boolean | null;

  rollout_m?: number | null;

  // ── v1.7.0 Bahndisziplin ─────────────────────────────────────────
  // Siehe docs/spec/v1.7.0-bahndisziplin.md §8. Alle optional: Flüge von
  // vor v1.7.0 haben sie nicht, und die Anzeige muss das ehrlich zeigen
  // ("für diesen Flug nicht erfasst") statt eine leere Querachse zu malen,
  // die wie ein Messwert aussieht.

  /** Längsposition beim Verlassen der Bahn, ab Lande-Schwelle. Ersetzt in
   *  der Anzeige den bisherigen "Bremspunkt 40 kt" — der war nie ein
   *  betrieblicher Punkt, sondern eine Messkonvention, und lag bei MPH 9
   *  bereits auf der Abrollbahn. */
  clearance_point_m?: number | null;
  clearance_speed_kt?: number | null;
  /** Richtung der Ausfahrt. Wird nur gesetzt, wenn ZWEI unabhängige Größen
   *  übereinstimmen (fallender Kurs UND wachsender Querversatz in dieselbe
   *  Richtung) — sonst null, siehe §8.3.
   *
   *  ⚠ **`null` darf NICHT als Seite gezeichnet werden.** Die Räumungsmarke
   *  sitzt bauartbedingt AUF einer Bahnkante — das ist ihre Aussage: „hier
   *  hat das Flugzeug die Bahn verlassen". Ohne bekannte Seite gibt es diese
   *  Aussage nicht, und die Marke gehört weg; an ihre Stelle tritt der
   *  normale Endpunkt der Spur, dort wo die Räder wirklich waren.
   *
   *  Ein `side === "left" ? linke : rechte` macht aus „unbekannt" ein
   *  „rechts raus". Genau so gebaut und live gesehen an EIN3641 (EGAC 04,
   *  29.08.2026): Räumungspunkt bei 884 m, Seite null,
   *  Räumungsgeschwindigkeit 1,16 kt — das Flugzeug war dort ausgerollt und
   *  hat die Bahn nie verlassen. Betraf 6 von 58 Landungen im Bestand.
   *
   *  Wer diese Marke implementiert, prüft `clearance_side != null`, nicht
   *  nur `clearance_point_m != null`. */
  clearance_side?: "left" | "right" | null;

  /** Spurweite des Hauptfahrwerks in Metern. Ohne sie zeigt die Querachse
   *  nur die Mittelspur, kein Band — und die seitliche Bewertung entfällt. */
  track_width_m?: number | null;
  track_width_source?: "type_table" | "aircraft_file" | null;

  /** Kleinster Abstand des äußeren Rades zur Bahnkante über den gewerteten
   *  Rollweg. Negativ = jenseits der Kante. */
  min_edge_clearance_m?: number | null;
  /** Größter seitlicher Versatz zur Mittellinie, mit Vorzeichen
   *  (positiv = rechts in Landerichtung). */
  max_lateral_offset_m?: number | null;

  /** Spurverlauf für die Querachse. ALLE 40 m ein Stützpunkt, nicht nur
   *  Anfang und Ende — eine gerade Linie zwischen zwei Punkten verschweigt
   *  jede Korrektur und jedes Ausbrechen. Bei MPH 9 wurde so ein Ausschlag
   *  von 17 m sichtbar, den alle bisherigen Zahlen nicht zeigten. */
  /** ⚠ `laengs_m` ist ab **BAHNANFANG** gemessen, nicht ab der
   *  Landeschwelle. Siehe den Abschnitt „Zwei Bezugspunkte" weiter
   *  unten — das ist der wichtigste Fallstrick dieses Vertrags. */
  lateral_samples?: Array<{ laengs_m: number; quer_m: number }> | null;

  /** Befestigt? Auf Gras- und Naturpisten entfällt die seitliche Bewertung,
   *  weil die Kante dort fließend ist. */
  surface_paved?: boolean | null;

  /** Strecke jenseits des Bahnendes. > 0 = Overrun. */
  overrun_m?: number | null;

  locale?: "de" | "en" | "it";
}

export type AimClass = "perfect" | "short_of_aim" | "past_aim" | "long_landing" | "severe";
export type TchClass = "on_profile" | "slightly_low" | "slightly_high" | "high" | "below_profile";
```

### Mapping aus `LandingRecord` (Pilot-Client, Disk- ODER Memory-Quelle)

```ts
const props: RunwayDiagramV2Props = {
  airport_ident: record.runway_match.airport_ident,
  runway_ident: record.runway_match.runway_ident,
  // NUTZBARE Bahn (LDA) — der Versatz kommt als eigener Bereich davor,
  // das Diagramm addiert beides zur physischen Bahn. Volle Länge hier
  // hinein zu geben zeichnet die Bahn um den Versatz zu lang.
  length_m: (record.runway_match.length_ft - (record.runway_match.displaced_threshold_ft ?? 0)) * 0.3048,
  surface: record.runway_match.surface ?? null,  // Pilot-Client ONLY
  source: record.runway_match.source ?? null,
  nav_cycle: record.runway_match.nav_cycle ?? null,
  displaced_threshold_m: (record.runway_match.displaced_threshold_ft ?? 0) * 0.3048,
  td_distance_from_threshold_m:
    record.td_distance_from_threshold_m
    ?? record.runway_match.touchdown_distance_from_threshold_ft * 0.3048,
  td_centerline_offset_m: record.runway_match.centerline_distance_m,
  td_in_tdz: record.td_in_tdz,
  td_third: record.td_third as 1 | 2 | 3 | null,
  td_tdz_length_m: record.td_tdz_length_m,
  aim_point_m: record.aim_point_m,
  aim_delta_m: record.aim_delta_m,
  aim_class: record.aim_class as AimClass | null,
  tch_actual_ft: record.tch_actual_ft,
  tch_expected_ft: record.runway_match.tch_expected_ft,
  tch_delta_ft: record.tch_delta_ft,
  tch_class: record.tch_class as TchClass | null,
  pre_displaced_threshold: record.pre_displaced_threshold,
  rollout_m: record.rollout_distance_m,
  locale: i18n.language as "de" | "en" | "it",
};
```

### Mapping aus `TouchdownDto.payload` (Webapp)

**Verifiziert gegen `aeroacars-mqtt/src/lib.rs` am 2026-05-13:**

```ts
const pl = touchdown.payload as Record<string, unknown> | null;
if (!pl) return null; // pre-v0.5.x payload, kein Render

const props: RunwayDiagramV2Props = {
  airport_ident: (pl.runway_match_icao as string | null) ?? "—",
  airport_name: null,  // Webapp resolved that via separate airport-cache
  runway_ident: (pl.runway_match_ident as string | null) ?? "—",
  // dito: `runway_length_m` im Wire-Format ist die VOLLE Bahn in Metern,
  // der Versatz muss abgezogen werden (v1.6.8 — genau hier lag der Fehler,
  // solange `runway_displaced_threshold_ft` immer 0 war und niemandem auffiel).
  length_m: ((pl.runway_length_m as number | null) ?? 0)
    - ((pl.runway_displaced_threshold_ft as number | null) ?? 0) * 0.3048,
  surface: null,  // ← bewusste Asymmetrie, siehe §Surface
  source: (pl.navdata_source as "navigraph" | "ourairports_fallback" | null) ?? null,
  nav_cycle: (pl.navdata_cycle as string | null) ?? null,
  displaced_threshold_m: ((pl.runway_displaced_threshold_ft as number | null) ?? 0) * 0.3048,
  td_distance_from_threshold_m:
    (pl.td_distance_from_threshold_m as number | null)
    ?? (pl.runway_match_distance_m as number | null)
    ?? 0,
  td_centerline_offset_m:
    (pl.runway_match_centerline_offset_m as number | null) ?? 0,
  td_in_tdz: pl.td_in_tdz as boolean | null,
  td_third: pl.td_third as 1 | 2 | 3 | null,
  td_tdz_length_m: pl.td_tdz_length_m as number | null,
  aim_point_m: pl.aim_point_m as number | null,
  aim_delta_m: pl.aim_delta_m as number | null,
  aim_class: pl.aim_class as AimClass | null,
  tch_actual_ft: pl.tch_actual_ft as number | null,
  tch_expected_ft: pl.runway_tch_expected_ft as number | null,
  tch_delta_ft: pl.tch_delta_ft as number | null,
  tch_class: pl.tch_class as TchClass | null,
  pre_displaced_threshold: pl.pre_displaced_threshold as boolean | null,
  rollout_m: pl.rollout_distance_m as number | null,

  // ── v1.7.0 Bahndisziplin ────────────────────────────────────────────
  // Gelesen, nicht gerechnet — siehe unten.
  clearance_point_m: pl.clearance_point_m as number | null,
  scoring_cutoff_m: pl.scoring_cutoff_m as number | null,
  clearance_speed_kt: pl.clearance_speed_kt as number | null,
  clearance_side: pl.clearance_side as "left" | "right" | null,
  track_width_m: pl.track_width_m as number | null,
  track_width_source: pl.track_width_source as "type_table" | "aircraft_file" | null,
  wingspan_m: pl.wingspan_m as number | null,
  runway_width_m: pl.runway_width_m as number | null,
  min_edge_clearance_m: pl.min_edge_clearance_m as number | null,
  max_lateral_offset_m: pl.max_lateral_offset_m as number | null,
  lateral_samples: pl.lateral_samples as Array<{laengs_m: number; quer_m: number}> | null,
  surface_paved: pl.surface_paved as boolean | null,
  overrun_m: pl.overrun_m as number | null,
  runway_exits: null,   // steht in der OSM-Bodenkarte, die der Server hat

  locale: "de",
};
```

**Der Client rechnet, der Server zeigt an.**

Keine der v1.7.0-Größen wird serverseitig hergeleitet. Sie entstehen im
Pilot-Client aus dem 5-Hz-Rollout-Fenster, das nur er sieht; der Server hat
die Positionen im Grundtakt von drei Sekunden und käme damit zwangsläufig auf
andere Zahlen. **Zwei Zahlen für dieselbe Landung sind schlimmer als eine
fehlende.**

Fehlen sie — Flüge von vor v1.7.0, oder Stratos-Landungen über die Brücke, die
keine Rollspur liefern —, bleibt es bei `null`, und die Anzeige sagt „für diesen
Flug nicht erfasst". Sie malt dann keine leere Querachse, die wie ein Messwert
aussieht.

**Auf der Leitung liegen die Felder flach.** Im Rust-Client sind sie als eine
Gruppe gefasst (`BahnWire`, `#[serde(flatten)]`), damit die Payload-Konstruktion
nicht dreizehn Zeilen an jeder Stelle braucht — der Server sieht davon nichts.
Ein Test hält das fest (`bahnfelder_liegen_flach_auf_der_leitung`); ohne das
Attribut läge alles unter einem Schlüssel `bahn`, der Mapper fände nichts, und
die Anzeige zeigte für jede Landung „nicht erfasst", ohne dass irgendwo ein
Fehler auftaucht.

**Verifizierte Wire-Felder (im TouchdownPayload präsent):**
- `runway_match_icao`, `runway_match_ident`, `runway_match_distance_m`, `runway_match_centerline_offset_m`, `runway_length_m`
- `navdata_source`, `navdata_cycle`
- `runway_displaced_threshold_ft`, `runway_tch_expected_ft`, `runway_true_course_deg`, `runway_glideslope_angle_deg`
- `td_distance_from_threshold_m`, `td_in_tdz`, `td_third`, `td_tdz_length_m`
- `aim_delta_m`, `aim_class`, `aim_point_m`
- `tch_actual_ft`, `tch_delta_ft`, `tch_class`
- `pre_displaced_threshold`
- `rollout_distance_m`
- **v1.7.0:** `clearance_point_m`, `scoring_cutoff_m`, `clearance_speed_kt`, `clearance_side`, `track_width_m`, `track_width_source`, `wingspan_m`, `runway_width_m`, `min_edge_clearance_m`, `max_lateral_offset_m`, `lateral_samples`, `surface_paved`, `overrun_m`
- **v1.7.8:** `bahn_geometrie_quelle` (`"szenerie"` | `"navdaten"`), `bahn_kurs_korrektur_grad`, `bahn_breiten_korrektur_m` — woher die Bahngeometrie stammt und wie weit sie von den Navdaten abwich
- **v1.7.10:** `bahn_szenerie_status`, `sim_kennung`
- **v1.7.11:** `bahn_szenerie_status` trägt jetzt die Zahlen (`ohne_bahnen(rollwege=243)`)
- **v1.7.12:** `bahn_schwellen_korrektur_m` — um wie viele Meter die versetzte
  Schwelle aus der Szenerie von den Navdaten abwich. Ab dieser Fassung liest der
  MSFS-Adapter die Schwelle aus dem PAVEMENT-Untersatz
  (`OPEN PRIMARY_THRESHOLD` … `CLOSE PRIMARY_THRESHOLD`) statt sie offen zu
  lassen.

  ⚠ **Der folgenreichste der drei Korrekturwerte.** Die versetzte Schwelle ist
  der Nullpunkt von `td_distance_from_threshold_m`, `aim_delta_m` und
  `tch_delta_ft`. Sagt die Szenerie „keine Schwelle", wo die Navdaten 573 m
  führen (TJPS 12, Flug LAN273 am 30.08.2026), verschieben sich diese drei
  Größen um eine halbe Bahnlänge. Ein großer Wert heißt deshalb nicht „Fehler",
  sondern „Szenerie und Navdaten sind sich hier uneins" — und dass wir der
  Szenerie gefolgt sind, weil der Pilot dort landet.

⚠ **Die Felder ab v1.7.8 sind Diagnose, keine Anzeige.** Sie werden aus der
Datenbank ausgewertet und stehen bewusst NICHT in `BAHN_FELDER` des Recorders
(`mqttSubscriber.ts`) — jene Positivliste steuert, was in die **Anzeige**
gepatcht wird. Wer eines dieser Felder je sichtbar machen will, muss es dort
ergänzen, sonst kommt es nie an. Reihenfolge dann: Recorder, Webapp, Client-Tag.

**`clearance_point_m` und `scoring_cutoff_m` sind nicht dasselbe.** Ein Flugzeug
schwenkt hunderte Meter vor der Ausfahrt nach außen. `scoring_cutoff_m` markiert
den Beginn dieses Ausschwenkens — ab dort wird nicht mehr **bewertet**, weil die
seitliche Lage an der Anweisung des Lotsen hängt. **Verlassen** hat es die Bahn
erst bei `clearance_point_m`, an der Kante. Bis v1.7.0 lieferte der Client nur
einen Wert für beides; die Marke „Bahn geräumt" saß dadurch mitten auf der Bahn,
und das Ausschwenken zählte als Fehler des Piloten (bei `0Ab3v9EvNN1LKZ8z`,
EDDH 05: 21,95 m gemeldet auf einer Bahn mit 23 m Halbbreite — auf der Bahn
selbst waren es 13,4 m).

**NICHT im Wire-Format:** `runway_surface_code`. Webapp kann surface nicht anzeigen — Pilot-Client-only.

---

## v1.7.0 — Darstellungsregeln der zwei Ansichten

Ausführlich in `docs/spec/v1.7.0-bahndisziplin.md` §8.3 bis §8.6. Verbindlich hier:

1. **Eine Projektionsfunktion für beide Ansichten.** Längs- und Queransicht
   benutzen dieselbe `x(meter)`, zeigen denselben Ausschnitt (Pre-Threshold bis
   Bahnende) und müssen pixelgleich fluchten. Im ersten Entwurf war die
   Längsansicht nach Augenmaß gebaut — der Aim-Marker stand über 200 m daneben,
   und auffällig wurde es erst, als beide Ansichten untereinander standen.
2. **Ereignisse als nummerierte Marken**, Erklärung darunter in einer Liste im
   Stil der Detail-Karten. Kein beschreibender Text in der Grafik.
3. **Radspuren als gefülltes Band**, nicht als zwei Linien plus Mittellinie —
   drei getrennte Linien laufen bei steilen Abschnitten optisch auseinander.
4. **Kein Flugzeugumriss in der Bahnfläche.** Das Größenverhältnis gehört als
   maßstäblicher Balkenvergleich unter die Grafik.
5. **Ausfahrten nur als Stummel** am Bahnrand, nie als vollständige Rollwege:
   Bei der nötigen Überhöhung der Querachse wäre ein 30°-Schnellabrollweg fast
   senkrecht gezeichnet.
6. **Lesbarkeit ist prüfbar, nicht Geschmack.** Kein Text überlappt einen
   anderen *oder Grafikinhalt*, nichts ragt aus dem Zeichenbereich, der
   Zwischenraum zwischen den Ansichten bleibt frei, die Seite scrollt nie
   horizontal. Gehört in den Snapshot-Test.

**Der Marker „Bremspunkt 40 kt" entfällt ersatzlos.**

## Visual-Tokens

Identisch zwischen beiden Implementierungen. Bei späteren Anpassungen: hier ändern, beide Repos pflegen.

```ts
export const RUNWAY_V2_TOKENS = {
  svgWidth: 1200,
  svgHeight: 320,
  rwyPaddingX: 70,
  rwyPaddingY: 70,
  // ... siehe RunwayDiagramV2.tsx für die volle Liste
} as const;
```

---

## Fixtures (5 Szenarien)

Werden in **beiden Repos** identisch abgelegt unter `tests/fixtures/runway-diagram/`:

1. **`navigraph-full.json`** (MS713-Anchor) — alle v0.8.0-Felder, TDZ-Treffer, Aim leicht zu kurz
2. **`ourairports-fallback.json`** — Source=Fallback, TCH/DDS null, TDZ/Aim trotzdem da
3. **`dds-violation.json`** — Pilot setzt vor displaced threshold auf (illegal)
4. **`long-landing.json`** — TDZ verfehlt, Aim+500 m, TCH zu hoch
5. **`pre-v080.json`** — Legacy, alle v0.8.0-Felder null → graceful degrade

### Snapshot-Test mit Determinismus-Guard

```ts
// In beiden Repos: tests/runway-diagram-v2.snapshot.test.tsx
test.each(["navigraph-full", "ourairports-fallback", "dds-violation", "long-landing", "pre-v080"])(
  "renders fixture %s identically",
  (name) => {
    const fixture = JSON.parse(fs.readFileSync(`tests/fixtures/runway-diagram/${name}.json`, "utf-8"));
    const { container } = render(<RunwayDiagramV2 {...fixture.props} />);
    // NUR SVG-Markup snapshotten — keine CSS-/Font-Reflows via container.innerHTML.
    expect(container.querySelector("svg")?.outerHTML).toMatchSnapshot();
  },
);
```

**Determinismus-Garantien (für stabile Snapshots):**
- Alle Styles inline im SVG via `style={...}` oder Attribute. **KEIN external CSS, KEIN Webfont-Loading.**
- Schriftarten ausschließlich via Monospace-/System-Stack (`"Segoe UI"`, `"Consolas"`) — kein Font-Loading-Effekt im JSDom.
- Keine `Date.now()` / `Math.random()` / Locale-Defaults im Render-Pfad.
- Snapshot vergleicht nur `svg.outerHTML`, nicht `container.innerHTML` (= keine CSS-/Layout-Drift).

---

## Responsive-Verhalten

| Container-Breite | Layout |
|---|---|
| ≥ 1200 px | 4 Cards in einer Row, SVG bei voller viewBox |
| 900–1199 px | 4 Cards 2×2-Grid, SVG full-width |
| 600–899 px | Cards 2×2 oder gestapelt, SVG viewBox bleibt (scales down via CSS) |
| < 600 px | Cards komplett gestapelt, SVG-Höhe via aspect-ratio gewahrt |

Werte dürfen unter keinen Umständen überlappen. Bei schmalen Containern kürzen sich Labels (z.B. „AUFSETZZONE 900 m" → „TDZ 900m"), Scale-Ticks reduzieren auf 0/600/1200/2400.

---

## Glossar-Modal

Hilfe-Button im Header öffnet ein Modal mit allen 17 Begriffen aus §Glossar. Pro Begriff: Name + Abkürzung (groß), Pilot-Erklärung. Bei Geo-Begriffen optional Mini-SVG-Inset („wo ist das auf der Bahn?"). Accessible: ESC schließt, Focus-Trap, ARIA-roles.

---

## Akzeptanz-Kriterien

- [ ] Beide Implementierungen rendern alle 5 Fixtures identisch (Snapshot-Test grün in beiden Repos), **ausgenommen Surface-Label** (Pilot-Client zeigt es, Webapp nicht — siehe §Surface)
- [ ] Bei container ≥ 1200 px: Diagram füllt komplette Breite, Höhe ≈ 320 px
- [ ] Pilot-Client zeigt das Diagram aus `landings.json` ODER `FlightStats`-Memory ohne irgendeinen Network-Call
- [ ] Glossar-Modal erklärt alle 17 Begriffe in einfacher Sprache
- [ ] Hover-Tooltips auf TDZ-Box, Aim-Marker, TD-Punkt, Exit-Punkt zeigen Werte + Glossar-Hint
- [ ] OurAirports-Fallback wird ohne rote Warnung als „Fallback — Schwellen-Position kann abweichen" angezeigt
- [ ] Pre-v0.8.0-Records degradieren graceful (kein TDZ, kein Aim, kein TCH, kein DDS — aber Basis-Geometrie bleibt sichtbar)
- [ ] TCH-Card erscheint NICHT wenn `tch_actual_ft == null`
- [ ] UI sagt **„VPS Navdata (AIRAC X)"** statt direkt „Navigraph" — Lizenz-Wording neutral
- [ ] Browser-QS: Desktop (1400 px), Tablet (900 px), Mobile (480 px) — keine Überlappungen, keine Cut-offs

---

## Implementierungs-Reihenfolge

1. **Fixtures sichern** — 5 JSON-Files in beiden Repos ✓ (Pilot-Client done auf `feat/v0.8.2-runway-diagram-v2`)
2. **Pilot-Client zuerst** — `RunwayDiagramV2.tsx` + Glossar-Modal + Dev-Preview-Tab. Live LandingPanel-Render-Pfad bleibt UNANGETASTET — V2 lebt parallel.
3. **User-Iteration lokal** via `npm run tauri dev`
4. **Webapp portieren** — gleicher Component-Code in `webapp/src/components/RunwayDiagramV2.tsx`
5. **Snapshot-Tests** in beiden Repos
6. **Browser-QS** breit + schmal
7. **Live-Integration** — alte `RunwayDiagram` aus `LandingPanel.tsx` und `LandingAnalysis.tsx` durch V2 ersetzen (separater Commit, klar reverteable)
8. **v0.8.2-Tag** — nur auf explizites User-go

## Out of Scope

- Approach-Track-Linie (= letzte 5 nmi Anflug-Visualisierung) — Future-Slice
- TCH-actual-ft-Computation aus Sample-Buffer — Future-Slice, schon im v0.8.0-Code als TODO
- Mobile-spezifische Touch-Interaktionen
- Print-/PDF-Export
- Surface-Wire-Erweiterung — nur wenn nach v0.8.2 explizit gewünscht

---

## QS-Historie

### Round 1 (2026-05-13)

Befunde:
- **Blocker:** Implementierung lag auf `main` statt Feature-Branch → behoben durch Switch auf `feat/v0.8.2-runway-diagram-v2` (Commit `22a6b03`)
- **Blocker:** „Identisch Client + VPS" stimmte nicht — Surface war asymmetrisch → behoben durch dokumentierte Asymmetrie (§Surface)
- **Blocker:** Payload-Feldnamen ungeprüft → behoben durch Wire-Field-Verifikation gegen `aeroacars-mqtt/src/lib.rs`
- **Medium:** Active-Flight-Offline-Garantie unscharf → behoben durch 3-Quellen-Modell (§Datenquellen)
- **Medium:** Snapshot-Drift via CSS/Fonts → behoben durch Determinismus-Garantien + nur-SVG-Snapshot
- **Medium:** Status APPROVED zu früh → auf DRAFT/QS gesetzt
- **Low:** `kt` fehlte → ergänzt (17 statt 16 Begriffe)
- **Low:** „Navigraph"-Wording → neutral „VPS Navdata (AIRAC)"

### Round 2 (offen)

Wartet auf User-Sign-off der QS-Round-1-Fixes oben.

---

## Tracker

Live-Befund 2026-05-13 (Pilot-Client Screenshot, EDDK/24 TDZ-Hit aber Diagram-Größe unzureichend + VPS-Webapp zeigt anderes Layout). Display-only Polish nach v0.8.0-Core. Implementation auf `feat/v0.8.2-runway-diagram-v2`. Pilot-Release wartet auf User-„go" für v0.8.2.


## ⚠ Zwei Bezugspunkte — und welcher wo gilt

Der Payload fuehrt Laengsmasse mit **zwei verschiedenen Nullpunkten**
nebeneinander. Bis v1.7.12 stand das nirgends geschrieben, und die
Zeichnung hat beide gleich behandelt.

**Ab LANDESCHWELLE** (negativ = davor, in der Zone der versetzten
Schwelle):

  * `td_distance_from_threshold_m`
  * `aim_point_m`
  * `tdz_end_m`

**Ab BAHNANFANG** (immer positiv):

  * `lateral_samples[].laengs_m`
  * `mess_ende_laengs_m`
  * `scoring_cutoff_m`
  * `clearance_point_m`
  * `runway_exits[].laengs_m`

**Laengen, keine Positionen** (kein Bezugspunkt noetig):
`runway_length_m` (volle Bahn), `td_tdz_length_m`, `rollout_distance_m`,
`overrun_m`.

### Warum das ein eigener Abschnitt ist

Auf Bahnen **ohne** versetzte Schwelle sind beide Nullpunkte identisch.
Deshalb stimmt fast jede Landung, und der Fehler zeigt sich nur dort, wo
eine versetzte Schwelle existiert.

Gemeldet an Flug **LAN273** (TJPS 12, 30.08.2026): 573 m versetzte
Schwelle. Aufgesetzt wurde 337 m hinter dem Bahnanfang — die Zeichnung
setzte die Aufsetzmarke bei −236 m (richtig) und die Rollspur bei +337 m
(um 573 m zu weit rechts). Der Pilot sah eine Marke, die mit seiner Spur
nicht zusammenhing, und einen Raeumpunkt 573 m hinter der Wahrheit.

### Wie es abgesichert ist

`runwayProjection` bietet **zwei** Funktionen, und die Bedeutung steht im
Namen:

  * `mToX(m)`             — Wert ist ab Landeschwelle gemessen
  * `mAbBahnanfangZuX(m)` — Wert ist ab Bahnanfang gemessen

Ein Griff zur falschen ist damit beim Lesen sichtbar statt still.

Dazu die Invariante in `RunwayCrossSection.bezugspunkt.test.tsx`:
**Aufsetzmarke und erste gemessene Spurprobe sind derselbe Ort** und
muessen im Bild an derselben Stelle landen. Der Test liest beide
Positionen aus der GEZEICHNETEN Grafik — ein erster Entwurf rechnete die
Spurposition selbst nach und blieb deshalb auch mit dem Fehler gruen.
