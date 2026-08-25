//! v0.16.12 (#phase-v2): Phasen-Engine v2 — Schatten-Modus.
//!
//! Hintergrund (Daten-Audit über 429 echte Flüge): 26 % der Flüge haben
//! Phasen-Fehler — 193× Descent→Cruise-Flaps (ATC-Level-Off im Sinkflug
//! kippt die Phase fälschlich auf Cruise), 69× Premature-Cruise
//! (Level-Restriction im Steigflug latcht Cruise, es gibt keine
//! Cruise→Climb-Kante zurück), Level-Restrictions werden als „Climb"
//! gelabelt obwohl der Flieger minutenlang level fliegt.
//!
//! Die alte FSM in `step_flight` entscheidet aus EINZEL-TICK-Werten mit
//! über Jahre gestapelten Incident-Patches (v0.5.9/10/11, v0.7.17 …).
//! Statt eines weiteren Schwellwert-Patches: eine fenster-basierte,
//! evidenz-getriebene Engine mit VOLLSTÄNDIGEM symmetrischem
//! Übergangs-Graph für das En-Route-Band.
//!
//! **Schatten-Modus:** die alte FSM bleibt zu 100 % autoritativ. Diese
//! Engine beobachtet nur, loggt ihre Sicht in die Position-JSONL
//! (`shadow_phase`/`shadow_segment` im SimSnapshot) und zählt
//! Divergenz-Sekunden. Umstellung erfolgt erst nach Wochen Datensammlung
//! per Owner-Entscheidung.
//!
//! Architektur (2 Layer, beide pur + uhrlos — Caller liefert Timestamps):
//!
//!   * **Layer 1 — [`KinematicSegmenter`]:** zeitbasierter Ring-Buffer
//!     (~60 s Fenster, max. 64 Samples). Klassifiziert die Bewegung als
//!     `Ground / Climbing / Level / Descending / Insufficient` aus
//!     Fenster-EVIDENZ (Altitude-Rate über das Fenster + Median-V/S),
//!     nie aus einem einzelnen Tick. Hysterese-Band zwischen den
//!     Schwellen schluckt Turbulenz-/Trim-Spikes.
//!
//!   * **Layer 2 — [`ShadowPhaseEngine`]:** Semantik. Im
//!     Boden-/Terminal-Band spiegelt sie die alte FSM 1:1 (deren Phase
//!     wird übernommen — der Schatten-Diff soll das En-Route-Band
//!     beleuchten, nicht die über Incidents gehärtete Boden-Logik neu
//!     verhandeln). Im En-Route-Band (Climb/Cruise/Descent/Approach/
//!     Final) läuft sie frei auf dem vollständigen Übergangs-Graph.
//!
//! **Bekannte, dokumentierte Diffs (Runde 1):**
//!   * `Holding` wird in v2 nicht modelliert. Sagt die alte FSM
//!     Holding, läuft v2 einfach weiter (typisch: Cruise). Der
//!     Divergenz-Zähler in lib.rs schließt old==Holding aus, und der
//!     Schatten-Report muss Holding-Abschnitte ausklammern.
//!   * Persistenz über App-Restarts ist NICHT implementiert — beim
//!     Resume synct die Engine sich einmalig auf die alte FSM-Phase
//!     und hat nach ≤ 2 Fenstern (≈ 3 min) wieder volle Evidenz.

use chrono::{DateTime, Utc};
use sim_core::FlightPhase;
use std::collections::VecDeque;

// ─── Layer-1-Konstanten (KinematicSegmenter) ────────────────────────────

/// Klassifikations-Fenster: es werden Samples gehalten, die zusammen
/// mindestens dieses Zeitfenster abdecken.
///
/// 2026-07-06: 90 → 60 s. Messung über 189 reale Schatten-Flüge
/// (418k Enroute-Ticks): die tatsächliche Streamer-Tick-Kadenz liegt im
/// GESAMTEN En-Route-Band bei median ~3 s (nicht den früher angenommenen
/// bis zu 60 s), p99 < 6 s. Ein 90-s-Fenster war dadurch messbar zu träge
/// (Top-of-Climb-Latenz median 77,5 s). 60 s spart median 24 s an ToC /
/// 15 s an ToD und schließt die beobachteten Phasen-Lag-Läufe; Kosten nur
/// +~10 % Segment-Flips (+0,56/Flug). Bei 3-s-Kadenz trägt 60 s noch ~20
/// Samples — mehr als genug für eine stabile Rate.
pub const WINDOW_SECS: f64 = 60.0;
/// Unter dieser Fenster-Spannweite (oder < 2 Samples) gibt es keine
/// belastbare Evidenz → `Insufficient` (vorheriges Segment wird gehalten).
/// Proportional zum 60-s-Fenster gesenkt (war 60 s bei 90-s-Fenster).
pub const MIN_SPAN_SECS: f64 = 45.0;
/// Sample-Cap gegen unbegrenztes Buffer-Wachstum. HINWEIS: In 189 realen
/// Flügen wurde dieser Cap NIE erreicht — die Trim-by-Span-Logik wirft
/// alte Samples längst vorher raus (bei ~3-s-Kadenz ~20 Samples im
/// 60-s-Fenster, bei 0.5-s-Flare-Kadenz ~40 über MIN_SPAN). Der Cap ist
/// reiner Sicherheitsgurt, keine kalibrierte Konstante.
pub const MAX_SAMPLES: usize = 64;
/// Aufnahme-Mindestabstand in den Buffer. Schnellere Ticks werden für
/// die Klassifikation trotzdem verarbeitet, nur nicht gespeichert —
/// über ein 60-s-Fenster trägt ein 0.5-s-Nachbarsample keine neue
/// Information.
pub const MIN_SAMPLE_SPACING_SECS: f64 = 1.5;
/// Sample-Lücke, ab der der Segmenter komplett resettet (Sim-Reload,
/// Pause, Slew, Reposition): die Werte vor und nach der Lücke gehören
/// kinematisch nicht zusammen → keine falsche Transition aus
/// Misch-Evidenz, stattdessen `Insufficient` bis das Fenster neu steht.
pub const RESET_GAP_SECS: f64 = 300.0;

/// Altitude-Rate-Schwelle (Fenster-Delta / Spannweite, fpm) für
/// Climbing/Descending. Korroboriert durch den Median-V/S.
pub const RATE_CLIMB_FPM: f64 = 300.0;
/// Median-V/S-Korroboration für Climbing/Descending.
pub const MEDIAN_VS_CLIMB_FPM: f64 = 200.0;
/// |Rate| unter dieser Schwelle (+ |Median-V/S| < 200) → Level.
pub const RATE_LEVEL_FPM: f64 = 150.0;
/// |Median-V/S|-Schwelle für Level.
pub const MEDIAN_VS_LEVEL_FPM: f64 = 200.0;

// ─── Layer-2-Konstanten (PhaseResolver) ─────────────────────────────────

/// Band um die geplante Cruise-Altitude: Level innerhalb
/// `cruise_ref − 1000 ft` (oder darüber) gilt als „am Cruise-Level".
pub const CRUISE_REF_BAND_FT: f64 = 1000.0;
/// Fallback ohne `cruise_ref`: so lange muss ein Level-Segment
/// anhalten, bevor es als Cruise gilt (am 429-Flug-Korpus validierte
/// Heuristik — typische ATC-Level-Offs im Climb/Descent sind kürzer).
/// Wird im SINKFLUG durch die Observed-Cruise-Ref (s. u.) ersetzt und
/// greift dort nur noch, wenn noch keine Referenz-Höhe beobachtet wurde.
pub const LEVEL_TO_CRUISE_FALLBACK_SECS: i64 = 240;
/// Backstop bei VORHANDENER `cruise_ref`, wenn das Level-Segment im Steig
/// klar UNTER der Referenz liegt (v0.21.x, #phase-v2 Fix A): dann ist das
/// entweder ein echter Tief-Reiseflug (der ganze Enroute unter der Planhöhe,
/// z. B. ATC hält tief) oder eine lange Zwischen-Reiseflughöhe. Ohne diesen
/// Backstop bliebe ein solcher Flug mit gesetzter ref dauerhaft „Level/Climb"
/// (der `Some(false)`-Zweig hatte keinen Dauer-Fallback). Bewusst DEUTLICH
/// länger als `LEVEL_TO_CRUISE_FALLBACK_SECS` (240 s): typische ATC-
/// Restriktionen im Steig sind kürzer als 10 min → der Premature-Cruise-
/// Schutz bleibt intakt, nur echte Tief-/Zwischen-Reiseflüge kippen zu Cruise.
/// NUR im Steig-Zweig (Climb→Level); im Sinkflug bleibt ein Level-Off unter
/// der ref eine Restriktion (Descent), sonst kämen die Descent→Cruise-Flaps
/// zurück.
pub const LEVEL_BELOW_REF_TO_CRUISE_FALLBACK_SECS: i64 = 600;
/// Observed-Cruise-Ref-Band (2026-07-06): das höchste je im En-Route-Band
/// erreichte Level (`obsref`) ist eine ref-lose, aus dem Höhenstrom selbst
/// abgeleitete Cruise-Referenz — verfügbar für 100 % der Flüge (im
/// Gegensatz zum bei GSG fast immer leeren `cruise_ref`). Ein Level-Segment
/// im SINKFLUG gilt nur dann wieder als Cruise, wenn `alt ≥ obsref − BAND`.
/// Das killt die Descent-Level-Off→Cruise-Fehlpromotion (158 → 3 Ticks über
/// den 189-Flug-Korpus) ohne die Steig-Gewinne anzutasten. BEWUSST NUR im
/// Sinkflug: im Steig ist `obsref == aktuelle Höhe` → eine Level-Restriction
/// würde sofort fälschlich Cruise (Premature-Cruise-Falle) — dort bleibt der
/// Dauer-Fallback oben maßgeblich.
pub const OBSREF_BAND_FT: f64 = 2000.0;
/// Go-Around-Bestätigung (2026-07-06): aus Approach/Final wird erst dann
/// Climb, wenn seit dem ERSTEN Climbing-Segment-Tick netto mindestens so
/// viel AGL gewonnen wurde. Trennt echte Go-Arounds von Glideslope-/
/// Flare-Ballooning (kurzer +V/S-Spike bzw. Level-Bump hoch oben, der
/// wieder kollabiert). Killt die geklebte-Climb-Kaskade (0 Stuck-Flüge im
/// 189-Flug-Korpus). Schwelle 700 ft empirisch bestimmt: der echte
/// pto705-Go-Around (+1219 ft ab Tiefpunkt, ab Segment-Anker ~+800 ft)
/// bestätigt, der a3V0DXn-Level-Bump (~+150 ft) und der 207-ft-Blip aus
/// `phase_holding_pending_leak` werden korrekt gehalten. Der Segment-Anker
/// (nicht der Anflug-Tiefpunkt) ist bewusst gewählt: er misst den LOKALEN
/// Steig ab Climbing-Beginn und wird dadurch nicht von einem viel tieferen
/// früheren Anflugpunkt getäuscht (der a3V0DXn fälschlich +2000 ft gäbe).
pub const GA_CONFIRM_FT: f64 = 700.0;
/// Fallback ohne `cruise_ref`: so lange muss ein Climbing-Segment im
/// Cruise anhalten, bevor die Phase zurück auf Climb geht (Step-Climbs
/// ohne Referenz nicht von echten Re-Climbs unterscheidbar — kurze
/// Korrekturen bleiben Cruise).
pub const CLIMBING_TO_CLIMB_FALLBACK_SECS: i64 = 60;
/// Cruise erfordert AGL über dieser Schwelle (identisch zur alten FSM):
/// GA-Platzrunden auf 2500 ft AGL und Busch-Hops werden NIE Cruise.
pub const CRUISE_MIN_AGL_FT: f64 = 5000.0;
/// Approach-Eintritt: AGL unter dieser Schwelle + sinkend (alte FSM 1:1 —
/// Downstream-Konsumenten sind darauf kalibriert).
pub const APPROACH_AGL_FT: f64 = 5000.0;
/// Final-Eintritt: AGL unter dieser Schwelle (alte FSM 1:1).
pub const FINAL_AGL_FT: f64 = 700.0;
/// Cruise→Descent-Fast-Path (alte FSM 1:1): tief + deutlich sinkend
/// entscheidet sofort, ohne auf Fenster-Evidenz zu warten.
pub const CRUISE_FASTPATH_AGL_FT: f64 = 3000.0;
/// V/S-Schwelle des Fast-Paths.
pub const CRUISE_FASTPATH_VS_FPM: f32 = -500.0;

// ─── Layer 1: KinematicSegmenter ────────────────────────────────────────

/// Fenster-Klassifikation der Vertikalbewegung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Segment {
    /// Mehrheit des Fensters am Boden.
    Ground,
    /// Nachhaltiges Steigen (Rate > +300 fpm UND Median-V/S > +200 fpm).
    Climbing,
    /// Nachhaltig level (|Rate| < 150 fpm UND |Median-V/S| < 200 fpm).
    Level,
    /// Nachhaltiges Sinken (Rate < −300 fpm UND Median-V/S < −200 fpm).
    Descending,
    /// Keine belastbare Evidenz (Fenster < 60 s / < 2 Samples / frisch
    /// resettet). Resolver hält die aktuelle Phase.
    #[default]
    Insufficient,
}

impl Segment {
    /// snake_case-Wire-Form für JSONL (`shadow_segment`) + UI.
    pub fn as_snake_str(self) -> &'static str {
        match self {
            Segment::Ground => "ground",
            Segment::Climbing => "climbing",
            Segment::Level => "level",
            Segment::Descending => "descending",
            Segment::Insufficient => "insufficient",
        }
    }
}

/// Ein Sample im Segmenter-Fenster.
#[derive(Debug, Clone, Copy)]
struct KinSample {
    t: DateTime<Utc>,
    alt_msl_ft: f64,
    /// Mitgeführt für zukünftige fenster-basierte AGL-Auswertung
    /// (Runde 1 nutzt das Live-AGL des Resolvers).
    #[allow(dead_code)]
    agl_ft: f64,
    vs_fpm: f32,
    on_ground: bool,
}

/// Layer 1 — zeitbasierter Ring-Buffer + Fenster-Klassifikator.
///
/// Pur und uhrlos: der Caller liefert die Timestamps. Funktioniert mit
/// sparsamen Samples (Cruise-Cadence) genauso wie mit dichten
/// (Flare-Cadence, via Aufnahme-Mindestabstand gedrosselt).
#[derive(Debug, Default)]
pub struct KinematicSegmenter {
    samples: VecDeque<KinSample>,
    /// Zuletzt KLASSIFIZIERTES Segment. Bei `Insufficient`-Evidenz und
    /// im Hysterese-Band wird dieser Wert gehalten (= „keep previous").
    last_segment: Segment,
}

impl KinematicSegmenter {
    /// Sample einspeisen + Fenster klassifizieren.
    ///
    /// Rückgabe ist das ANZUWENDENDE Segment: bei unzureichender Evidenz
    /// oder Hysterese-Band das zuletzt klassifizierte (initial
    /// `Insufficient`).
    pub fn push(
        &mut self,
        t: DateTime<Utc>,
        alt_msl_ft: f64,
        agl_ft: f64,
        vs_fpm: f32,
        on_ground: bool,
    ) -> Segment {
        if let Some(last) = self.samples.back() {
            let gap_secs = (t - last.t).num_milliseconds() as f64 / 1000.0;
            if gap_secs > RESET_GAP_SECS {
                // Time-Skip (Reload/Pause/Slew): Evidenz vor der Lücke
                // ist wertlos → kompletter Reset, keine falsche
                // Transition aus Misch-Daten.
                self.samples.clear();
                self.last_segment = Segment::Insufficient;
            } else if gap_secs < MIN_SAMPLE_SPACING_SECS {
                // Zu dichter Tick (oder rückwärts laufende Zeit —
                // Sim-Clock-Glitch): nicht speichern, nur klassifizieren.
                return self.classify();
            }
        }
        self.samples.push_back(KinSample {
            t,
            alt_msl_ft,
            agl_ft,
            vs_fpm,
            on_ground,
        });
        // Trim: älteste Samples raus, solange das Fenster auch ohne sie
        // noch ≥ WINDOW_SECS abdeckt — dann hartes Cap.
        while self.samples.len() >= 2 {
            let span_without_front =
                (t - self.samples[1].t).num_milliseconds() as f64 / 1000.0;
            if span_without_front >= WINDOW_SECS {
                self.samples.pop_front();
            } else {
                break;
            }
        }
        while self.samples.len() > MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.classify()
    }

    /// Buffer-Reset (z. B. Flugstart). `Default` ist äquivalent.
    pub fn reset(&mut self) {
        self.samples.clear();
        self.last_segment = Segment::Insufficient;
    }

    fn classify(&mut self) -> Segment {
        let n = self.samples.len();
        if n < 2 {
            return self.last_segment;
        }
        let first = self.samples.front().expect("n >= 2");
        let last = self.samples.back().expect("n >= 2");
        let span_secs = (last.t - first.t).num_milliseconds() as f64 / 1000.0;

        // Boden-Mehrheit zuerst: on_ground ist ein robustes Boolean und
        // braucht keine 60 s Bewegungs-Evidenz.
        let ground_count = self.samples.iter().filter(|s| s.on_ground).count();
        if ground_count * 2 > n {
            self.last_segment = Segment::Ground;
            return self.last_segment;
        }

        if span_secs < MIN_SPAN_SECS {
            // Zu wenig Evidenz → vorheriges Segment halten.
            return self.last_segment;
        }

        // Altitude-Rate über das Fenster (fpm), korroboriert mit dem
        // Median-V/S — zwei unabhängige Signale müssen übereinstimmen.
        let rate_fpm = (last.alt_msl_ft - first.alt_msl_ft) / (span_secs / 60.0);
        let median_vs = {
            let mut vs: Vec<f32> = self.samples.iter().map(|s| s.vs_fpm).collect();
            vs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            f64::from(vs[vs.len() / 2])
        };

        let classified = if rate_fpm > RATE_CLIMB_FPM && median_vs > MEDIAN_VS_CLIMB_FPM {
            Some(Segment::Climbing)
        } else if rate_fpm < -RATE_CLIMB_FPM && median_vs < -MEDIAN_VS_CLIMB_FPM {
            Some(Segment::Descending)
        } else if rate_fpm.abs() < RATE_LEVEL_FPM && median_vs.abs() < MEDIAN_VS_LEVEL_FPM {
            Some(Segment::Level)
        } else {
            // Hysterese-Band: Spikes/Turbulenz/Übergangs-Mischzonen landen
            // hier → vorheriges Segment halten, NIE aus einem Grenzfall
            // umklassifizieren.
            None
        };
        if let Some(c) = classified {
            self.last_segment = c;
        }
        self.last_segment
    }
}

// ─── Layer 2: ShadowPhaseEngine (PhaseResolver) ─────────────────────────

/// Phasen, in denen v2 frei läuft (auf eigener Kinematik-Evidenz
/// entscheidet) statt in `step()`s Baseline-Sync-Zweig auf die alte Phase
/// zurückgesetzt zu werden. Gilt NUR für die Baseline-Sync-Prüfung
/// (`!shadow_is_enroute(self.phase)` in `step()`) — für die Frage, ob die
/// alte FSM diesen Tick als vertrauenswürdige Sync-Quelle gilt, ist
/// `old_is_enroute` zuständig, MIT ABSICHT eine separate, unveränderte
/// Liste (s. dort).
///
/// `Landing` gehört seit v0.19.1 dazu, obwohl es kein En-Route-Band mehr
/// ist: `resolve()`s `Final`-Arm promotet jetzt selbst auf `Landing`,
/// sobald der Kinematik-Segmenter `Ground` meldet (robustes `on_ground`-
/// Mehrheits-Signal, siehe `KinematicSegmenter::classify`) — unabhängig
/// davon, ob die alte FSM diese Kante überhaupt je selbst schafft. Vorher
/// verließ sich `Final` dafür ausschließlich auf den 1:1-Sync von der
/// alten FSM (Kommentar dort: "Landing kommt per Sync von der alten
/// FSM"); bleibt die alte FSM auf einer En-Route-Phase hängen (Feld-Fund
/// GSG22 EDLN→EDDL: `stats.phase` verharrte die ganze Landung/Rollout
/// über bei `Climb`), kommt dieser Sync nie — v2 (und alles, was
/// `effective_phase`/`effective_display_phase` liest: Cockpit-UI,
/// Live-Karte, phpVMS-Heartbeat, Tray, Discord, ACARS-Log) hing dadurch
/// unbegrenzt lange auf `Final` fest, obwohl der Flieger längst am Boden
/// rollte. `Landing` MUSS deshalb HIER (nicht in `old_is_enroute`)
/// stehen — sonst würde die Baseline-Sync-Verzweigung in `step()` die
/// frisch promotete `Landing`-Phase im allernächsten Tick sofort wieder
/// auf die (weiterhin hängende) alte Phase zurücksetzen. Holt die alte
/// FSM später doch noch auf (erreicht TaxiIn/BlocksOn/Arrived), gewinnt
/// automatisch wieder der normale 1:1-Sync-Zweig oben in `step()`
/// (`!old_is_enroute(old_phase)`), da jene Phasen nicht in `old_is_enroute`
/// stehen — kein Sonderfall nötig.
fn shadow_is_enroute(p: FlightPhase) -> bool {
    matches!(
        p,
        FlightPhase::Climb
            | FlightPhase::Cruise
            | FlightPhase::Descent
            | FlightPhase::Approach
            | FlightPhase::Final
            | FlightPhase::Landing
    )
}

/// Phasen der ALTEN FSM, bei denen v2 NICHT auf die alte Phase synct
/// (= das Band, in dem die Engines voneinander abweichen dürfen).
/// `Holding` gehört dazu: v2 modelliert es nicht (dokumentierter Diff),
/// darf während old==Holding aber auch nicht zwangs-gesynct werden.
///
/// v0.19.1: bewusst NICHT (mehr) von `shadow_is_enroute` abgeleitet, obwohl
/// beide Listen bis auf `Landing` identisch sind — die beiden Prädikate
/// beantworten unterschiedliche Fragen und dürfen nicht zusammenfallen.
/// `old_is_enroute` entscheidet "ist die alte Phase DIESEN Tick eine
/// vertrauenswürdige Sync-Quelle" — eine alte FSM, die korrekt und prompt
/// `Landing` erreicht (der Normalfall), MUSS weiterhin sofort 1:1
/// übernommen werden (Tests `bush_hop_never_cruise`, `normal_arrival_arc`,
/// `touch_and_go_resyncs_to_climb`). Würde `old_is_enroute(Landing)` auch
/// `true` liefern (wie es eine naive Ableitung von `shadow_is_enroute` mit
/// `Landing` täte), bliebe genau dieser funktionierende Sync-Pfad aus —
/// entdeckt beim ersten Testlauf dieser Änderung, bevor es released wurde.
fn old_is_enroute(p: FlightPhase) -> bool {
    matches!(
        p,
        FlightPhase::Climb
            | FlightPhase::Cruise
            | FlightPhase::Descent
            | FlightPhase::Approach
            | FlightPhase::Final
            | FlightPhase::Holding
    )
}

/// Layer 2 — Schatten-Engine: Segment + Kontext → `(FlightPhase, Segment)`.
///
/// Gleiches `FlightPhase`-Enum wie die alte FSM (Diffs direkt
/// vergleichbar), das Display-Segment (z. B. `Level` während einer
/// ATC-Restriction) wird separat mitgeführt.
#[derive(Debug, Default)]
pub struct ShadowPhaseEngine {
    segmenter: KinematicSegmenter,
    /// Aktuelle Schatten-Phase. Startet bei `Preflight` (Default) und
    /// synct sich auf die alte FSM, bis das En-Route-Band erreicht ist.
    phase: FlightPhase,
    /// Aktuell gemeldetes Segment (inkl. Hysterese-Hold).
    current_segment: Segment,
    /// Seit wann `current_segment` ununterbrochen gemeldet wird — Basis
    /// der Dauer-Heuristiken (Level ≥ 240 s, Climbing ≥ 60 s).
    segment_since: Option<DateTime<Utc>>,
    /// Observed-Cruise-Ref: höchste je im En-Route-Band erreichte MSL-Höhe.
    /// Ref-lose Cruise-Referenz für den Sinkflug (s. `OBSREF_BAND_FT`).
    ///
    /// Wird bewusst NUR per `.max()` angehoben und über Resets/Time-Skips/
    /// Baseline-Sync NICHT gelöscht: die Engine lebt per-Flug (frisch pro
    /// Flug in `FlightStats`), und ein „zu hoch" stehen gebliebener Wert (z. B.
    /// nach einem Touch-and-Go zu tieferem Zweit-Leg) macht den Descent-Level-
    /// Guard nur KONSERVATIVER (bleibt Descent) — er kann nie fälschlich
    /// Cruise promoten. Damit gibt es keinen Premature-Cruise-Pfad über obsref.
    obsref_ft: Option<f64>,
    /// AGL beim ERSTEN Climbing-Tick eines Approach/Final-Climb-Outs — Anker
    /// der Go-Around-Bestätigung (`GA_CONFIRM_FT`). `None`, solange nicht in
    /// einem Approach/Final-Climbing-Ausstieg.
    ga_entry_agl_ft: Option<f64>,
    /// v0.19.1: `old_phase` des VORHERIGEN Ticks — Basis für die
    /// Touch-and-Go-Weiche in `step()` (erkennt eine FRISCHE alte-FSM-
    /// Transition zurück ins En-Route-Band, während wir selbst `Landing`
    /// halten). Startet bei `Preflight` (Default), harmlos: die Weiche
    /// prüft zusätzlich `self.phase == Landing`, was zu Flugbeginn nie
    /// zutrifft.
    last_old_phase: FlightPhase,
}

impl ShadowPhaseEngine {
    /// Zuletzt klassifiziertes Kinematik-Segment (Stand: Ende des
    /// VORHERIGEN `step()`-Aufrufs — `step_flight` (alte FSM) läuft im
    /// Streamer-Tick vor `shadow_engine.step()`, kann diesen Tick also nur
    /// die Evidenz des Vorticks lesen). Konsument: der En-Route-Versöhner
    /// in lib.rs — `Insufficient` heißt dort "v2 hat gerade keine echte
    /// Evidenz (frisch resettet), nicht versöhnen". Für dessen 90-s-Dwell
    /// ist der Ein-Tick-Versatz irrelevant.
    pub fn current_segment(&self) -> Segment {
        self.current_segment
    }

    /// Ein Streamer-Tick. Pur: alle Zeit kommt vom Caller.
    ///
    /// * `old_phase` — Phase der alten FSM NACH `step_flight` dieses Ticks
    ///   (autoritativ; Sync-Quelle außerhalb des En-Route-Bands).
    /// * `cruise_ref_ft` — geplante Cruise-Altitude (OFP/Bid-Level bzw.
    ///   FMC), wenn verfügbar. Die Engine funktioniert auch ohne
    ///   (Dauer-Heuristiken als Fallback).
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        t: DateTime<Utc>,
        alt_msl_ft: f64,
        agl_ft: f64,
        vs_fpm: f32,
        on_ground: bool,
        old_phase: FlightPhase,
        cruise_ref_ft: Option<f64>,
    ) -> (FlightPhase, Segment) {
        let mut segment = self.segmenter.push(t, alt_msl_ft, agl_ft, vs_fpm, on_ground);
        if segment != self.current_segment || self.segment_since.is_none() {
            self.current_segment = segment;
            self.segment_since = Some(t);
        }
        let seg_held_secs = self
            .segment_since
            .map(|s| (t - s).num_seconds())
            .unwrap_or(0);

        // Observed-Cruise-Ref pflegen: höchste MSL-Höhe, solange irgendeine
        // der beiden Engines im En-Route-Band ist (deckt den ganzen Flug ab).
        if old_is_enroute(old_phase) || shadow_is_enroute(self.phase) {
            self.obsref_ft = Some(self.obsref_ft.map_or(alt_msl_ft, |r| r.max(alt_msl_ft)));
        }

        // v0.19.1: FRISCHE alte-FSM-Transition zurück ins En-Route-Band,
        // während wir selbst `Landing` halten (unabhängig ob durch
        // 1:1-Mirror einer funktionierenden alten FSM oder durch unsere
        // eigene `Final`→`Landing`-Promotion erreicht) — z. B. Touch-and-Go
        // oder Rejected Landing. Die alte FSM hat einen eigenen,
        // spezialisierten On-Ground-Kanten-Detektor dafür; der reagiert
        // sofort. Unsere eigene Climbing-Klassifikation im Landing-Arm von
        // `resolve()` würde dagegen erst reagieren, sobald die Fenster-
        // MEHRHEIT der gepufferten Samples nicht mehr `on_ground` zeigt —
        // bei einem frischen Abheben nach längerem Boden-Rollen dauert das
        // (Fenster bis 60 s) deutlich zu lange. Bedingung `last_old_phase
        // != old_phase` (nicht nur `old_is_enroute(old_phase)`) ist
        // entscheidend: ein DURCHGEHEND hängender alter Wert (z. B. Climb
        // über die gesamte Landung/Rollout — der GSG22-Fall, den dieser
        // ganze Fix behebt) darf NICHT jeden Tick erneut zurück-syncen.
        //
        // Härtung (Code-Review): `old_is_enroute(old_phase) && last_old_phase
        // != old_phase` allein vertraut JEDER Änderung des alten Werts,
        // nicht nur einer, die tatsächlich "Boden verlassen" bedeutet — eine
        // alte FSM, die zwischen zwei En-Route-Phasen hin- und herspringt
        // (z. B. Climb→Approach), OHNE je tatsächlich abzuheben, würde sonst
        // das korrekt gehaltene `Landing` verlassen. Über die aktuell
        // bekannten alten Übergangs-Pfade (Go-Around-Klassifikator, T&G-
        // Erkennung) nicht auslösbar (beide sind hart hinter `!on_ground`
        // gated, dasselbe `on_ground`-Signal wie hier), aber genau das
        // Verhalten einer NICHT vollständig verstandenen alten FSM ist der
        // Grund für diesen ganzen Fix — zusätzlich `!on_ground` verlangen:
        // nur eine frische alte Transition VERTRAUEN, wenn das ROHE
        // Telemetrie-Signal DIESES Ticks auch wirklich "in der Luft" sagt.
        // Bewusst `on_ground` (Rohwert, sofort) statt `segment != Ground`
        // (Fenster-Mehrheit, hinkt nach dem Aufsetzen bis zu 60 s hinterher
        // — hätte exakt den Touch-and-Go-Fastpath wieder ausgehebelt, den
        // dieser ganze Zweig herstellen soll).
        let fresh_enroute_transition_from_landing = self.phase == FlightPhase::Landing
            && old_is_enroute(old_phase)
            && self.last_old_phase != old_phase
            && !on_ground;
        self.last_old_phase = old_phase;

        if !old_is_enroute(old_phase) || fresh_enroute_transition_from_landing {
            // Boden-/Terminal-Band: alte FSM 1:1 spiegeln (Boarding/
            // Pushback/Taxi/TakeoffRoll/Takeoff/Landing/TaxiIn/BlocksOn/
            // Arrived/…) — bzw. bei `fresh_enroute_transition_from_landing`
            // der Touch-and-Go-Sonderfall oben. Der Diff soll das
            // En-Route-Band zeigen.
            self.phase = old_phase;
            if fresh_enroute_transition_from_landing {
                // Gleiche Begründung wie beim Baseline-Sync unten: Evidenz
                // aus der Landung/dem Rollout würde einen frischen Climb
                // sofort wieder nach Ground/Descent kippen.
                self.segmenter.reset();
                segment = Segment::Insufficient;
                self.current_segment = segment;
                self.segment_since = Some(t);
            }
            self.ga_entry_agl_ft = None;
        } else if !shadow_is_enroute(self.phase) {
            // Baseline-Sync: die alte FSM ist im En-Route-Band, v2 noch
            // nicht (Engine-Start mid-flight / Resume / Takeoff→Climb-
            // Kante / T&G-Reset). Einmalig übernehmen, danach frei
            // laufen. Holding wird nicht modelliert → als Cruise starten.
            self.phase = if old_phase == FlightPhase::Holding {
                FlightPhase::Cruise
            } else {
                old_phase
            };
            // Regime-Wechsel: das Fenster trägt Evidenz aus dem ALTEN
            // Regime (z. B. den Anflug-Sinkflug vor einem Touch-and-Go,
            // der den frischen Climb sofort wieder nach Descent kippen
            // würde). Evidenz verwerfen und im neuen Regime neu sammeln
            // — ehrlich als `Insufficient` gemeldet.
            self.segmenter.reset();
            segment = Segment::Insufficient;
            self.current_segment = segment;
            self.segment_since = Some(t);
            self.ga_entry_agl_ft = None;
        } else {
            self.phase = self.resolve(
                segment,
                seg_held_secs,
                alt_msl_ft,
                agl_ft,
                vs_fpm,
                cruise_ref_ft,
            );
        }
        (self.phase, segment)
    }

    /// Aktuelle Schatten-Phase (zuletzt von [`Self::step`] geliefert).
    pub fn phase(&self) -> FlightPhase {
        self.phase
    }

    /// Vollständiger symmetrischer Übergangs-Graph für das
    /// En-Route-Band — der Kern-Fix. „Sustained" steckt bereits im
    /// Segment selbst (60-s-Fenster-Evidenz); die Dauer-Heuristiken
    /// (`seg_held_secs`) greifen nur in den ref-losen Fallbacks.
    ///
    /// `&mut self`, weil die Go-Around-Bestätigung (`GA_CONFIRM_FT`) den
    /// AGL-Anker über mehrere Ticks hält (`ga_entry_agl_ft`).
    fn resolve(
        &mut self,
        segment: Segment,
        seg_held_secs: i64,
        alt_msl_ft: f64,
        agl_ft: f64,
        vs_fpm: f32,
        cruise_ref_ft: Option<f64>,
    ) -> FlightPhase {
        // „Am/über dem geplanten Cruise-Level" (Band: ref − 1000 ft).
        let near_or_above_ref =
            |alt: f64| cruise_ref_ft.map(|r| alt >= r - CRUISE_REF_BAND_FT);
        // Ref-lose Cruise-Entscheidung im SINKFLUG: Observed-Cruise-Ref
        // (höchste erreichte Höhe) schlägt den Dauer-Fallback. Nur im
        // Sinkflug korrekt — der Aufrufer nutzt das nur im Descent-Zweig.
        let cruise_from_level_descending = |alt: f64| match near_or_above_ref(alt) {
            Some(at) => at,
            None => match self.obsref_ft {
                Some(r) => alt >= r - OBSREF_BAND_FT,
                None => seg_held_secs >= LEVEL_TO_CRUISE_FALLBACK_SECS,
            },
        };

        // Der GA-Anker gilt im Approach/Final-Ausstieg. `Landing` (seit
        // v0.19.1) braucht ihn NICHT — dessen eigener Climbing-Zweig ruft
        // `confirm_go_around` bewusst nicht auf (kein Flare-Balloon mehr
        // möglich, sobald `Ground` schon bestätigt war; siehe die
        // `FlightPhase::Landing`-Arm-Doku). Er ÜBERLEBT kurze Level-/
        // Insufficient-Dips im Steigflug (gestufter Go-Around, z. B.
        // ATC-Stopp-Höhe oder eine Steigrate, die kurz unter die
        // Fensterschwelle pendelt), damit der AGL-Gewinn KUMULATIV ab dem
        // ersten Climbing-Tick zählt. Ein Re-Anker bei jedem Dip würde
        // höher ansetzen und einen gestuften GA nie `GA_CONFIRM_FT` am
        // Stück erreichen lassen (→ geklebter Approach, das Spiegelbild
        // des Bugs, den der Guard behebt). Gelöscht wird der Anker nur,
        // wenn wieder gesunken wird (`Descending` = GA aufgegeben) oder
        // das Approach/Final-Band verlassen ist (sonst bestätigte ein
        // späterer Anflug gegen einen veralteten, tieferen Anker).
        if !matches!(self.phase, FlightPhase::Approach | FlightPhase::Final)
            || segment == Segment::Descending
        {
            self.ga_entry_agl_ft = None;
        }

        match self.phase {
            FlightPhase::Climb => match segment {
                // Climb + Descending sustained → Descent. Das Fenster
                // ersetzt die alten Peak-Loss-Guards (v0.5.9) auf
                // natürliche Weise — ein einzelner −800-fpm-Tick erzeugt
                // nie ein Descending-Segment.
                Segment::Descending => FlightPhase::Descent,
                Segment::Level => {
                    let cruise = match near_or_above_ref(alt_msl_ft) {
                        // ref bekannt + am/über dem geplanten Level →
                        // sofort Cruise (Schnellpfad; Kern von Fix A).
                        Some(true) => true,
                        // ref bekannt aber DARUNTER: erst mal eine Level-
                        // RESTRICTION → Climb + Label Level (killt die 69
                        // Premature-Cruise-Flüge). ABER: hält das Level
                        // sehr lange an (LEVEL_BELOW_REF_TO_CRUISE_
                        // FALLBACK_SECS), ist es ein echter Tief-/Zwischen-
                        // Reiseflug → doch Cruise. Kurze ATC-Restriktionen
                        // (< 10 min) bleiben Climb.
                        Some(false) => {
                            seg_held_secs >= LEVEL_BELOW_REF_TO_CRUISE_FALLBACK_SECS
                        }
                        // ref unbekannt: Dauer-Heuristik. BEWUSST NICHT
                        // obsref — im Steigflug ist obsref == aktuelle Höhe,
                        // das würde jede Level-Restriction sofort fälschlich
                        // zu Cruise promoten (Premature-Cruise-Falle).
                        None => seg_held_secs >= LEVEL_TO_CRUISE_FALLBACK_SECS,
                    };
                    if cruise && agl_ft > CRUISE_MIN_AGL_FT {
                        FlightPhase::Cruise
                    } else {
                        FlightPhase::Climb
                    }
                }
                _ => FlightPhase::Climb,
            },
            FlightPhase::Cruise => {
                // Fast-Path (alte FSM 1:1): tief + deutlich sinkend —
                // sofort Descent, nicht auf Fenster-Evidenz warten.
                if vs_fpm < CRUISE_FASTPATH_VS_FPM && agl_ft < CRUISE_FASTPATH_AGL_FT {
                    return FlightPhase::Descent;
                }
                // Cruise→Approach-Kante (symmetrisch zur Descent→Approach-
                // Kante): tief + sinkend. Schließt das Cruise-Kleben bei
                // < 5000 AGL (approach→cruise 181 → 0). Departure-sicher:
                // Cruise unter 5000 AGL entsteht nur durch Hineinsinken
                // (CRUISE_MIN_AGL_FT-Gate), nie im Steig — 0 Gegenbeispiele
                // im 189-Flug-Korpus.
                if agl_ft < APPROACH_AGL_FT && vs_fpm < 0.0 {
                    return FlightPhase::Approach;
                }
                match segment {
                    // Cruise + Descending sustained → Descent. KEIN
                    // 5000-ft-Verlust nötig — das Fenster beweist die
                    // Absicht bereits.
                    Segment::Descending => FlightPhase::Descent,
                    // Die bisher FEHLENDE Kante (Premature-Cruise-
                    // Lock-in): Cruise + Climbing sustained → Climb,
                    // wenn klar UNTER der Referenz. Step-Climb am/über
                    // ref bleibt Cruise.
                    Segment::Climbing => match cruise_ref_ft {
                        Some(r) => {
                            if alt_msl_ft < r - CRUISE_REF_BAND_FT {
                                FlightPhase::Climb
                            } else {
                                FlightPhase::Cruise
                            }
                        }
                        None => {
                            if seg_held_secs >= CLIMBING_TO_CLIMB_FALLBACK_SECS {
                                FlightPhase::Climb
                            } else {
                                FlightPhase::Cruise
                            }
                        }
                    },
                    _ => FlightPhase::Cruise,
                }
            }
            FlightPhase::Descent => {
                // Approach-Kante (alte FSM 1:1 — Downstream darauf
                // kalibriert): tief + sinkend.
                if agl_ft < APPROACH_AGL_FT && vs_fpm < 0.0 {
                    return FlightPhase::Approach;
                }
                match segment {
                    // Go-Around in der Höhe / Diversion-Climb.
                    Segment::Climbing => FlightPhase::Climb,
                    Segment::Level => {
                        // ATC-Level-Off im Sinkflug: bleibt Descent +
                        // Label Level (killt die 193 Descent→Cruise-
                        // Flaps). Nur am/über dem geplanten Level (ref-los:
                        // Observed-Cruise-Ref) ist es wieder Cruise — der
                        // Restriktions-Level tief im Sinkflug bleibt Descent
                        // (descent→cruise 158 → 3 im Korpus).
                        let cruise = cruise_from_level_descending(alt_msl_ft);
                        if cruise && agl_ft > CRUISE_MIN_AGL_FT {
                            FlightPhase::Cruise
                        } else {
                            FlightPhase::Descent
                        }
                    }
                    _ => FlightPhase::Descent,
                }
            }
            FlightPhase::Approach => {
                if segment == Segment::Climbing {
                    // Go-Around — aber erst NACH bestätigtem AGL-Gewinn
                    // (Balloon-Guard): ein Glideslope-Capture-Balloon
                    // erzeugt kurz ein Climbing-Segment, ohne dass der
                    // Flieger nachhaltig steigt.
                    if self.confirm_go_around(agl_ft) {
                        FlightPhase::Climb
                    } else {
                        FlightPhase::Approach
                    }
                } else if agl_ft < FINAL_AGL_FT {
                    // Final-Kante (alte FSM 1:1).
                    FlightPhase::Final
                } else {
                    FlightPhase::Approach
                }
            }
            FlightPhase::Final => {
                if segment == Segment::Climbing {
                    // Go-Around aus dem Final — mit demselben Balloon-Guard
                    // wie im Approach (Flare-Ballooning erzeugt sonst einen
                    // Phantom-Climb).
                    if self.confirm_go_around(agl_ft) {
                        FlightPhase::Climb
                    } else {
                        FlightPhase::Final
                    }
                } else if segment == Segment::Ground {
                    // v0.19.1: promote on our OWN kinematic evidence
                    // (robust on_ground-majority signal, see
                    // KinematicSegmenter::classify) instead of waiting on
                    // the old FSM's 1:1 sync — see shadow_is_enroute's doc
                    // comment for why that sync alone isn't reliable
                    // enough (field report GSG22 EDLN→EDDL: old FSM never
                    // left Climb for the whole flight).
                    FlightPhase::Landing
                } else {
                    FlightPhase::Final
                }
            }
            FlightPhase::Landing => {
                // v0.19.1: unlike the Final arm, NO balloon-guard here —
                // that guard exists solely for the flare-ballooning
                // ambiguity while still airborne on short final (a VS
                // spike that looks like climbing but isn't really leaving).
                // Once genuinely on the ground (segment reached Ground —
                // the majority-of-window on_ground evidence that got us
                // into this arm in the first place), there's no such
                // ambiguity left: a `Climbing` segment here means the
                // 60 s-window-confirmed kinematic evidence already shows a
                // real climb-out (touch-and-go / rejected landing), not a
                // flare bounce — Layer 1 already filtered the transient
                // case. Immediate transition, matching how promptly the
                // old FSM's own touch-and-go reset fires (test
                // `touch_and_go_resyncs_to_climb`).
                //
                // Otherwise holds here on its own evidence until the old
                // FSM catches up to a real terminal phase (TaxiIn/
                // BlocksOn/Arrived), at which point step()'s ordinary
                // 1:1-sync branch takes back over (those phases aren't in
                // `old_is_enroute`).
                if segment == Segment::Climbing {
                    FlightPhase::Climb
                } else {
                    FlightPhase::Landing
                }
            }
            // Nicht-En-Route-Phasen erreichen resolve() nie (Guard in
            // step) — defensiv: Phase halten.
            other => other,
        }
    }

    /// Go-Around-Bestätigung: liefert `true`, sobald seit dem ersten
    /// Climbing-Tick netto ≥ `GA_CONFIRM_FT` AGL gewonnen wurde. Ankert den
    /// AGL-Startwert beim ersten Aufruf. Der Aufrufer stellt sicher, dass
    /// der Anker außerhalb des Approach/Final-Climbing-Ausstiegs gelöscht
    /// wird (Guard am Anfang von `resolve`).
    fn confirm_go_around(&mut self, agl_ft: f64) -> bool {
        let anchor = *self.ga_entry_agl_ft.get_or_insert(agl_ft);
        agl_ft >= anchor + GA_CONFIRM_FT
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn t0() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_750_000_000, 0).expect("valid ts")
    }

    /// Synthetisches Profil-Stück: feedet die Engine `secs` Sekunden in
    /// `tick_secs`-Schritten mit konstantem V/S ab `start_alt`. Gibt
    /// (letzte Phase, letztes Segment, End-Altitude, End-Zeit) zurück.
    struct Sim {
        engine: ShadowPhaseEngine,
        t: DateTime<Utc>,
        alt: f64,
        terrain_elev: f64,
        cruise_ref: Option<f64>,
    }

    impl Sim {
        fn new(cruise_ref: Option<f64>) -> Self {
            Sim {
                engine: ShadowPhaseEngine::default(),
                t: t0(),
                alt: 0.0,
                terrain_elev: 0.0,
                cruise_ref,
            }
        }

        /// `secs` Sekunden mit konstantem `vs_fpm` fliegen; `old_phase`
        /// ist die (konstante) Sicht der alten FSM in dem Abschnitt.
        fn fly(
            &mut self,
            secs: i64,
            vs_fpm: f32,
            old_phase: FlightPhase,
        ) -> (FlightPhase, Segment) {
            self.fly_with_tick(secs, 5, vs_fpm, old_phase, false)
        }

        fn fly_with_tick(
            &mut self,
            secs: i64,
            tick_secs: i64,
            vs_fpm: f32,
            old_phase: FlightPhase,
            on_ground: bool,
        ) -> (FlightPhase, Segment) {
            let mut out = (self.engine.phase(), Segment::Insufficient);
            let steps = secs / tick_secs;
            for _ in 0..steps {
                self.t += Duration::seconds(tick_secs);
                self.alt += f64::from(vs_fpm) * (tick_secs as f64 / 60.0);
                let agl = (self.alt - self.terrain_elev).max(0.0);
                out = self.engine.step(
                    self.t,
                    self.alt,
                    agl,
                    vs_fpm,
                    on_ground,
                    old_phase,
                    self.cruise_ref,
                );
            }
            out
        }

        /// Boden-Phase + Takeoff bis zur Climb-Übergabe der alten FSM.
        fn depart(&mut self) {
            self.fly_with_tick(120, 5, 0.0, FlightPhase::Boarding, true);
            self.fly_with_tick(60, 5, 0.0, FlightPhase::TaxiOut, true);
            // Liftoff + Takeoff-Phase bis ~500 AGL (alte FSM-Übergabe).
            self.fly_with_tick(20, 5, 1500.0, FlightPhase::Takeoff, false);
        }
    }

    // ── Layer 1 ──────────────────────────────────────────────────────

    #[test]
    fn segmenter_insufficient_until_min_span() {
        let mut seg = KinematicSegmenter::default();
        let mut t = t0();
        // 35 s Klettern (7 Samples ab t0+5): span < MIN_SPAN_SECS(45) → Insufficient.
        let mut s = Segment::Insufficient;
        for i in 0..7 {
            t += Duration::seconds(5);
            s = seg.push(t, 1000.0 + (i as f64) * 200.0, 1000.0, 2400.0, false);
        }
        assert_eq!(s, Segment::Insufficient);
        // Weiter steigen bis die Fenster-Spannweite ≥ 45 s trägt → Climbing.
        for i in 0..7 {
            t += Duration::seconds(5);
            s = seg.push(t, 2400.0 + (i as f64) * 200.0, 2400.0, 2400.0, false);
        }
        assert_eq!(s, Segment::Climbing);
    }

    #[test]
    fn segmenter_vs_spike_does_not_flip_climb() {
        // DER Kern-Regressionsschutz ggü. der alten FSM: ein einzelner
        // −800-fpm-Tick im Steigflug erzeugt KEIN Descending-Segment.
        let mut seg = KinematicSegmenter::default();
        let mut t = t0();
        let mut alt = 5000.0;
        let mut s = Segment::Insufficient;
        for i in 0..40 {
            t += Duration::seconds(5);
            let vs = if i == 25 { -800.0 } else { 2000.0 };
            alt += f64::from(vs) / 12.0;
            s = seg.push(t, alt, alt, vs, false);
            assert_ne!(s, Segment::Descending, "spike darf nie Descending erzeugen");
        }
        assert_eq!(s, Segment::Climbing);
    }

    #[test]
    fn segmenter_hysteresis_band_keeps_previous() {
        // Rate zwischen 150 und 300 fpm = Hysterese-Band → vorheriges
        // Segment wird gehalten.
        let mut seg = KinematicSegmenter::default();
        let mut t = t0();
        let mut alt = 10000.0;
        // Erst klares Level etablieren.
        for _ in 0..30 {
            t += Duration::seconds(5);
            seg.push(t, alt, alt, 0.0, false);
        }
        assert_eq!(seg.classify(), Segment::Level);
        // Dann sanftes Driften mit +220 fpm (Band zwischen Level- und
        // Climb-Schwelle): bleibt Level.
        for _ in 0..30 {
            t += Duration::seconds(5);
            alt += 220.0 / 12.0;
            let s = seg.push(t, alt, alt, 220.0, false);
            assert_eq!(s, Segment::Level, "Hysterese-Band muss halten");
        }
    }

    #[test]
    fn segmenter_time_skip_resets_to_insufficient() {
        let mut seg = KinematicSegmenter::default();
        let mut t = t0();
        let mut s = Segment::Insufficient;
        for i in 0..30 {
            t += Duration::seconds(5);
            s = seg.push(t, 10000.0 + (i as f64) * 100.0, 10000.0, 1200.0, false);
        }
        assert_eq!(s, Segment::Climbing);
        // > 5 min Lücke → Reset → Insufficient, keine falsche Transition.
        t += Duration::seconds(400);
        s = seg.push(t, 30000.0, 30000.0, 0.0, false);
        assert_eq!(s, Segment::Insufficient);
    }

    #[test]
    fn segmenter_ground_majority_wins() {
        let mut seg = KinematicSegmenter::default();
        let mut t = t0();
        let mut s = Segment::Insufficient;
        for _ in 0..20 {
            t += Duration::seconds(5);
            s = seg.push(t, 300.0, 0.0, 0.0, true);
        }
        assert_eq!(s, Segment::Ground);
    }

    #[test]
    fn segmenter_caps_buffer_and_keeps_window() {
        let mut seg = KinematicSegmenter::default();
        let mut t = t0();
        // 10 Minuten dichte 1-s-Ticks: Spacing-Drossel + Cap greifen.
        for i in 0..600 {
            t += Duration::seconds(1);
            seg.push(t, 10000.0 + f64::from(i) * 10.0, 10000.0, 600.0, false);
        }
        assert!(seg.samples.len() <= MAX_SAMPLES);
        let span = (seg.samples.back().unwrap().t - seg.samples.front().unwrap().t)
            .num_seconds();
        assert!(span >= MIN_SPAN_SECS as i64, "span={span}");
    }

    // ── Layer 2: Failure-Surface (synthetische Profile) ──────────────

    /// ATC-Level-Off 3 min auf 7000 ft, dann Weiter-Steigen — mit
    /// bekannter Referenz NIEMALS Premature-Cruise; Label Level.
    #[test]
    fn level_restriction_with_ref_stays_climb() {
        let mut sim = Sim::new(Some(34000.0));
        sim.depart();
        sim.alt = 500.0;
        let (p, _) = sim.fly(180, 2200.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Climb);
        // Level-Off auf ~7100 ft für 3 min.
        let (p, s) = sim.fly(180, 0.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Climb, "Restriction unter ref bleibt Climb");
        assert_eq!(s, Segment::Level, "Label muss Level zeigen");
        // Weiter steigen → Climb bleibt.
        let (p, _) = sim.fly(300, 2200.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Climb);
        // Oben angekommen + level → Cruise.
        sim.alt = 34000.0;
        let (p, _) = sim.fly(180, 0.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Cruise);
    }

    /// Ohne Referenz: kurze Restriction (< 240 s) bleibt Climb; eine
    /// LANGE Level-Phase wird ehrlich Cruise — und die neue
    /// Cruise→Climb-Kante holt sie beim Weitersteigen zurück.
    #[test]
    fn level_restriction_without_ref_recovers_via_cruise_climb_edge() {
        let mut sim = Sim::new(None);
        sim.depart();
        sim.alt = 500.0;
        sim.fly(300, 2200.0, FlightPhase::Climb); // ~11.5k ft
        // 3 min Level: unter dem 240-s-Fallback → Climb + Level.
        let (p, s) = sim.fly(180, 0.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Climb);
        assert_eq!(s, Segment::Level);
        // 6 min Level: Fallback greift → Cruise (ehrliche Heuristik).
        let (p, _) = sim.fly(180, 0.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Cruise);
        // Weitersteigen ≥ 60 s sustained → die FEHLENDE Kante: zurück zu Climb.
        let (p, _) = sim.fly(240, 2000.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Climb, "Cruise→Climb-Kante muss feuern");
    }

    /// Step-Climb FL340 → FL380 am/über der Referenz bleibt Cruise.
    #[test]
    fn step_climb_at_ref_stays_cruise() {
        let mut sim = Sim::new(Some(34000.0));
        sim.depart();
        sim.alt = 34000.0;
        let (p, _) = sim.fly(300, 0.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Cruise);
        // Step-Climb mit +1500 fpm auf FL380 (alt >= ref − 1000 überall).
        let (p, _) = sim.fly(160, 1500.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Cruise, "Step-Climb darf nicht Climb werden");
        let (p, _) = sim.fly(300, 0.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Cruise);
    }

    /// Premature-Cruise-Lock-in: alte FSM latcht Cruise auf FL150,
    /// Flieger steigt aber weiter Richtung FL340 → v2 geht zurück auf
    /// Climb (alt < ref − 1000).
    #[test]
    fn premature_cruise_unlocks_to_climb_with_ref() {
        let mut sim = Sim::new(Some(34000.0));
        sim.depart();
        sim.alt = 15000.0;
        // Baseline-Sync: alte FSM sagt Cruise.
        let (p, _) = sim.fly(120, 0.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Cruise);
        // Steigen wird wieder aufgenommen → Climbing sustained → Climb.
        let (p, _) = sim.fly(240, 2200.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Climb);
    }

    /// v0.21.x (#phase-v2 Fix A): Tief-Reiseflug UNTER der geplanten ref.
    /// Kurzes Level unter ref bleibt Climb (Restriktion), aber ein sehr
    /// langes Level (> LEVEL_BELOW_REF_TO_CRUISE_FALLBACK_SECS) kippt über
    /// den Backstop doch zu Cruise — sonst hinge ein ganzer Flug unter der
    /// Planhöhe dauerhaft in „Climb/Level".
    #[test]
    fn low_cruise_below_ref_uses_long_backstop() {
        let mut sim = Sim::new(Some(34000.0));
        sim.depart();
        // Natürlicher Steigflug bis ~FL205 — klar unter ref (FL340).
        let (p, _) = sim.fly(600, 2000.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Climb);
        // Level-Off unter ref, kurze Dauer → bleibt Climb (kurze ATC-
        // Restriktion darf NICHT verfrüht Cruise werden).
        let (p, _) = sim.fly(240, 0.0, FlightPhase::Cruise);
        assert_eq!(
            p,
            FlightPhase::Climb,
            "kurzes Level unter ref muss Climb bleiben"
        );
        // Level hält sehr lange an → echter Tief-Reiseflug → Backstop.
        let (p, _) = sim.fly(480, 0.0, FlightPhase::Cruise);
        assert_eq!(
            p,
            FlightPhase::Cruise,
            "langes Level unter ref muss über den Backstop zu Cruise werden"
        );
    }

    /// Gegenprobe: MIT ref am/über dem Level greift der Schnellpfad sofort
    /// (kein Warten auf den Backstop) — Fix-A-Kernverhalten unverändert.
    #[test]
    fn level_off_at_ref_is_immediate_cruise() {
        let mut sim = Sim::new(Some(20000.0));
        sim.depart();
        let (p, _) = sim.fly(600, 2000.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Climb);
        // Level-Off auf ~FL205, am/über ref → nach Fenster-Etablierung
        // sofort Cruise, lange bevor der 600s-Backstop je zählen würde.
        let (p, _) = sim.fly(120, 0.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Cruise);
    }

    /// Drift-Down (Engine-Out): Cruise FL380 → Descent → Level FL240 =
    /// Descent + Label Level, NICHT Cruise (ref bekannt).
    #[test]
    fn drift_down_levels_as_descent_level() {
        let mut sim = Sim::new(Some(38000.0));
        sim.depart();
        sim.alt = 38000.0;
        sim.fly(300, 0.0, FlightPhase::Cruise);
        // Drift-Down mit −1500 fpm auf FL240.
        let (p, _) = sim.fly(560, -1500.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Descent);
        // Level-Off auf FL240 — 10 min lang: bleibt Descent + Level.
        sim.alt = 24000.0;
        let (p, s) = sim.fly(600, 0.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Descent, "Drift-Down-Level ≠ Cruise");
        assert_eq!(s, Segment::Level);
    }

    /// ATC-Level-Off im Sinkflug (die 193-Flaps-Klasse): Descent →
    /// 3 min Level auf 11000 ft → bleibt Descent + Level, danach
    /// weiter sinken → Approach unten.
    #[test]
    fn descent_level_off_stays_descent() {
        let mut sim = Sim::new(Some(36000.0));
        sim.depart();
        sim.alt = 36000.0;
        sim.fly(300, 0.0, FlightPhase::Cruise);
        let (p, _) = sim.fly(600, -2200.0, FlightPhase::Descent);
        assert_eq!(p, FlightPhase::Descent);
        sim.alt = 11000.0;
        let (p, s) = sim.fly(180, 0.0, FlightPhase::Descent);
        assert_eq!(p, FlightPhase::Descent, "Level-Off im Descent ≠ Cruise");
        assert_eq!(s, Segment::Level);
        // Weiter runter bis < 5000 AGL → Approach.
        let (p, _) = sim.fly(220, -1800.0, FlightPhase::Descent);
        assert_eq!(p, FlightPhase::Approach);
    }

    /// Go-Around in der Höhe (z. B. Missed Approach hoch / Diversion):
    /// Descent + Climbing sustained → Climb.
    #[test]
    fn go_around_high_descent_to_climb() {
        let mut sim = Sim::new(Some(30000.0));
        sim.depart();
        sim.alt = 30000.0;
        sim.fly(300, 0.0, FlightPhase::Cruise);
        let (p, _) = sim.fly(300, -2000.0, FlightPhase::Descent);
        assert_eq!(p, FlightPhase::Descent);
        // Diversion: zurück steigen.
        let (p, _) = sim.fly(180, 2000.0, FlightPhase::Descent);
        assert_eq!(p, FlightPhase::Climb, "Descent+Climbing → Climb");
    }

    /// Go-Around tief: Approach/Final → Climb-Out → v2 folgt auf Climb
    /// (fenster-evident, etwas träger als die alte Edge — akzeptiert).
    #[test]
    fn go_around_low_final_to_climb() {
        let mut sim = Sim::new(Some(20000.0));
        sim.depart();
        sim.alt = 20000.0;
        sim.fly(300, 0.0, FlightPhase::Cruise);
        sim.fly(700, -1400.0, FlightPhase::Descent); // bis ~3700 ft
        let (p, _) = sim.fly(60, -800.0, FlightPhase::Approach);
        assert_eq!(p, FlightPhase::Approach);
        // Bis unter 700 AGL sinken → Final.
        sim.alt = 650.0;
        let (p, _) = sim.fly(10, -700.0, FlightPhase::Final);
        assert_eq!(p, FlightPhase::Final);
        // Go-Around: volle Leistung, +2500 fpm. Fenster braucht Evidenz,
        // dann Climb — und NIE Cruise auf dem Weg.
        let mut reached_climb = false;
        for _ in 0..12 {
            let (p, _) = sim.fly(15, 2500.0, FlightPhase::Climb);
            assert_ne!(p, FlightPhase::Cruise, "GA darf nie Cruise zeigen");
            if p == FlightPhase::Climb {
                reached_climb = true;
                break;
            }
        }
        assert!(reached_climb, "GA muss in Climb ankommen");
    }

    /// GA-Pattern-Cruise auf 2500 ft AGL: AGL-Gate verhindert Cruise.
    #[test]
    fn ga_cruise_low_agl_never_cruise() {
        let mut sim = Sim::new(None);
        sim.depart();
        sim.alt = 2500.0;
        // 10 min level auf 2500 ft AGL — weit über dem 240-s-Fallback,
        // aber unter dem AGL-Gate.
        let (p, s) = sim.fly(600, 0.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Climb, "AGL-Gate muss Cruise blocken");
        assert_eq!(s, Segment::Level);
    }

    /// Busch-Hop (3-min-Flug): sinnvoller Bogen ohne Cruise.
    #[test]
    fn bush_hop_never_cruise() {
        let mut sim = Sim::new(None);
        sim.depart();
        sim.alt = 800.0;
        let mut phases = Vec::new();
        let (p, _) = sim.fly(60, 800.0, FlightPhase::Climb); // ~1600 ft
        phases.push(p);
        let (p, _) = sim.fly(60, 0.0, FlightPhase::Climb);
        phases.push(p);
        let (p, _) = sim.fly(90, -900.0, FlightPhase::Descent);
        phases.push(p);
        for p in &phases {
            assert_ne!(*p, FlightPhase::Cruise, "Busch-Hop darf nie Cruise sehen");
        }
        // Landung: alte FSM übernimmt → Sync.
        let (p, _) = sim.fly_with_tick(30, 5, 0.0, FlightPhase::Landing, true);
        assert_eq!(p, FlightPhase::Landing);
    }

    /// Touch-and-Go: Landing (Sync) → alte FSM resettet auf Climb →
    /// Baseline-Sync zieht v2 mit.
    #[test]
    fn touch_and_go_resyncs_to_climb() {
        let mut sim = Sim::new(None);
        sim.depart();
        sim.alt = 1500.0;
        sim.fly(120, -700.0, FlightPhase::Approach);
        let (p, _) = sim.fly_with_tick(10, 5, 0.0, FlightPhase::Landing, true);
        assert_eq!(p, FlightPhase::Landing);
        // T&G: alte FSM springt auf Climb.
        let (p, _) = sim.fly(10, 1800.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Climb, "T&G-Reset muss syncen");
    }

    /// Emergency Descent: Cruise → sustained Sinken → Descent (ohne
    /// 5000-ft-Verlust-Anforderung).
    #[test]
    fn emergency_descent_fires_from_window() {
        let mut sim = Sim::new(Some(38000.0));
        sim.depart();
        sim.alt = 38000.0;
        sim.fly(300, 0.0, FlightPhase::Cruise);
        // −6000 fpm: nach ~60 s Fenster-Evidenz → Descent (alte FSM
        // hätte 5000 ft Verlust ohnehin nach ~50 s).
        let (p, _) = sim.fly(120, -6000.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Descent);
    }

    /// Time-Skip (> 5 min Lücke): Segmenter resettet, Phase bleibt —
    /// keine falsche Transition aus Misch-Evidenz.
    #[test]
    fn time_skip_holds_phase() {
        let mut sim = Sim::new(Some(34000.0));
        sim.depart();
        sim.alt = 34000.0;
        let (p, _) = sim.fly(300, 0.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Cruise);
        // 6-min-Lücke + danach erster Tick mit wirrem V/S-Wert.
        sim.t += Duration::seconds(360);
        let (p, s) = sim.engine.step(
            sim.t,
            34000.0,
            34000.0,
            -1200.0,
            false,
            FlightPhase::Cruise,
            Some(34000.0),
        );
        assert_eq!(p, FlightPhase::Cruise, "Phase muss über den Skip halten");
        assert_eq!(s, Segment::Insufficient);
        // Fenster baut sich neu auf → Level → Cruise bleibt.
        let (p, _) = sim.fly(120, 0.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Cruise);
    }

    /// Mid-Flight-Resume: frische Engine + alte FSM im En-Route-Band →
    /// Baseline-Sync sofort, korrekte eigene Evidenz binnen 2 Fenstern.
    #[test]
    fn mid_flight_resume_warms_up() {
        let mut engine = ShadowPhaseEngine::default();
        let mut t = t0();
        // Erster Tick: alte FSM sagt Cruise → sofortiger Sync.
        let (p, s) = engine.step(t, 36000.0, 36000.0, 0.0, false, FlightPhase::Cruise, None);
        assert_eq!(p, FlightPhase::Cruise);
        assert_eq!(s, Segment::Insufficient, "noch keine Evidenz");
        // Nach < 2 Fenstern eigene Evidenz: Level.
        let mut seg = s;
        for _ in 0..36 {
            t += Duration::seconds(5);
            (_, seg) = engine.step(t, 36000.0, 36000.0, 0.0, false, FlightPhase::Cruise, None);
        }
        assert_eq!(seg, Segment::Level, "Evidenz binnen 2 Fenstern");
    }

    /// Holding (dokumentierter Diff): old==Holding erzwingt keinen
    /// Sync — v2 läuft als Cruise weiter.
    #[test]
    fn holding_is_not_modeled_documented_diff() {
        let mut sim = Sim::new(Some(24000.0));
        sim.depart();
        sim.alt = 24000.0;
        let (p, _) = sim.fly(300, 0.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Cruise);
        // Alte FSM kippt auf Holding — v2 bleibt Cruise (level).
        let (p, _) = sim.fly(300, 0.0, FlightPhase::Holding);
        assert_eq!(p, FlightPhase::Cruise, "v2 modelliert Holding nicht");
        // Baseline-Sync-Fall: frische Engine + old==Holding → Cruise.
        let mut fresh = ShadowPhaseEngine::default();
        let (p, _) = fresh.step(
            t0(),
            24000.0,
            24000.0,
            0.0,
            false,
            FlightPhase::Holding,
            None,
        );
        assert_eq!(p, FlightPhase::Cruise);
    }

    /// Cruise→Descent-Fast-Path: tief + stark sinkend entscheidet
    /// sofort (Pattern-Altitude-Flüge, alte FSM 1:1).
    #[test]
    fn cruise_low_agl_fastpath_to_descent() {
        let mut sim = Sim::new(None);
        sim.depart();
        // Hohes Terrain-Szenario abstrahiert: AGL via terrain_elev.
        sim.alt = 7000.0;
        sim.terrain_elev = 0.0;
        sim.fly(360, 0.0, FlightPhase::Cruise);
        // Plötzlich tief + sinkend (AGL < 3000, V/S < −500): ein Tick reicht.
        sim.alt = 2900.0;
        let (p, _) = sim.fly(5, -700.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Descent, "Fast-Path muss sofort feuern");
    }

    /// Final → Landung normal: Approach- und Final-Kanten spiegeln die
    /// alte FSM; Landing kommt per Sync.
    #[test]
    fn normal_arrival_arc() {
        let mut sim = Sim::new(Some(36000.0));
        sim.depart();
        sim.alt = 36000.0;
        sim.fly(300, 0.0, FlightPhase::Cruise);
        sim.fly(900, -2000.0, FlightPhase::Descent); // bis ~6000 ft
        let (p, _) = sim.fly(120, -1000.0, FlightPhase::Descent);
        assert_eq!(p, FlightPhase::Approach, "AGL<5000 + sinkend → Approach");
        sim.alt = 600.0;
        let (p, _) = sim.fly(10, -700.0, FlightPhase::Final);
        assert_eq!(p, FlightPhase::Final);
        let (p, _) = sim.fly_with_tick(15, 5, 0.0, FlightPhase::Landing, true);
        assert_eq!(p, FlightPhase::Landing);
        let (p, _) = sim.fly_with_tick(60, 5, 0.0, FlightPhase::TaxiIn, true);
        assert_eq!(p, FlightPhase::TaxiIn);
        let (p, _) = sim.fly_with_tick(60, 5, 0.0, FlightPhase::Arrived, true);
        assert_eq!(p, FlightPhase::Arrived);
    }

    /// Baut denselben Anflug wie `normal_arrival_arc` bis zu `Final` auf, so
    /// dass die vier v0.19.1-Tests unten (Stuck-Old-FSM-Szenario) sich nicht
    /// wiederholen müssen.
    fn sim_at_final() -> Sim {
        let mut sim = Sim::new(Some(36000.0));
        sim.depart();
        sim.alt = 36000.0;
        sim.fly(300, 0.0, FlightPhase::Cruise);
        sim.fly(900, -2000.0, FlightPhase::Descent);
        sim.fly(120, -1000.0, FlightPhase::Descent);
        sim.alt = 600.0;
        let (p, _) = sim.fly(10, -700.0, FlightPhase::Final);
        assert_eq!(p, FlightPhase::Final, "setup precondition");
        sim
    }

    /// v0.19.1 — Kernfix. Feld-Fund GSG22 EDLN→EDDL: die alte FSM
    /// (`stats.phase`) blieb die GESAMTE Landung/Rollout über bei `Climb`
    /// hängen (ein separater, vorbestehender Legacy-FSM-Bug, den dieser Fix
    /// NICHT repariert) — vorher hätte das v2 für immer auf `Final`
    /// festgenagelt (der 1:1-Sync von der alten FSM war der EINZIGE Weg zu
    /// `Landing`). Jetzt promotet `Final` sich selbst anhand des robusten
    /// `Ground`-Mehrheits-Signals, unabhängig vom (weiterhin hängenden)
    /// `old_phase`.
    /// Boden-Ticks bis der Kinematik-Segmenter zuverlässig auf `Ground`
    /// umklassifiziert. Das braucht bis zu `WINDOW_SECS` (60 s) — das
    /// 60-s-Fenster kann beim Eintritt noch voller Luft-Samples aus dem
    /// vorangegangenen Anflug sein (hier: der lange `sim_at_final()`-
    /// Descent/Approach/Final-Bogen sättigt das Fenster garantiert), und
    /// „Boden-Mehrheit" braucht so lange, bis die alten Luft-Samples aus
    /// dem gleitenden Fenster gealtert sind. 90 s liegt sicher darüber.
    const GROUND_MAJORITY_SETTLE_SECS: i64 = 90;

    /// v0.19.1 — Kernfix. Feld-Fund GSG22 EDLN→EDDL: die alte FSM
    /// (`stats.phase`) blieb die GESAMTE Landung/Rollout über bei `Climb`
    /// hängen (ein separater, vorbestehender Legacy-FSM-Bug, den dieser Fix
    /// NICHT repariert) — vorher hätte das v2 für immer auf `Final`
    /// festgenagelt (der 1:1-Sync von der alten FSM war der EINZIGE Weg zu
    /// `Landing`). Jetzt promotet `Final` sich selbst anhand des robusten
    /// `Ground`-Mehrheits-Signals, unabhängig vom (weiterhin hängenden)
    /// `old_phase`.
    #[test]
    fn final_promotes_to_landing_via_ground_evidence_even_when_old_fsm_stuck() {
        let mut sim = sim_at_final();
        let (p, s) =
            sim.fly_with_tick(GROUND_MAJORITY_SETTLE_SECS, 5, 0.0, FlightPhase::Climb, true);
        assert_eq!(
            p,
            FlightPhase::Landing,
            "muss sich selbst promoten statt auf die haengende alte FSM zu warten"
        );
        assert_eq!(s, Segment::Ground);
    }

    /// Fortsetzung: die alte FSM bleibt noch MINUTENLANG (wie im echten
    /// GSG22-Fall, ~4 min) bei `Climb` hängen — `Landing` darf dadurch NICHT
    /// jeden Tick zurück auf `Climb` fallen (der eigentliche Bug). Siehe
    /// `old_is_enroute`/`shadow_is_enroute`s Doku für die bewusste
    /// Entkopplung, die das ermöglicht.
    #[test]
    fn landing_holds_indefinitely_while_old_fsm_remains_stuck() {
        let mut sim = sim_at_final();
        let (p, _) =
            sim.fly_with_tick(GROUND_MAJORITY_SETTLE_SECS, 5, 0.0, FlightPhase::Climb, true);
        assert_eq!(p, FlightPhase::Landing, "setup precondition: promotion must have happened");
        let (p, _) = sim.fly_with_tick(240, 5, 0.0, FlightPhase::Climb, true);
        assert_eq!(
            p,
            FlightPhase::Landing,
            "darf NICHT auf die weiterhin haengende alte FSM zurueckfallen"
        );
    }

    /// Selbstheilung: sobald die alte FSM (z. B. via des ebenfalls in
    /// v0.19.1 verkürzten `ARRIVED_FALLBACK_DWELL_SECS`-Fallbacks in
    /// lib.rs) endlich `Arrived` erreicht, übernimmt der ganz normale
    /// 1:1-Sync-Zweig sofort — `Arrived` steht nicht in `old_is_enroute`,
    /// also kein Sonderfall nötig.
    #[test]
    fn landing_self_heals_once_old_fsm_finally_reaches_arrived() {
        let mut sim = sim_at_final();
        let (p, _) =
            sim.fly_with_tick(GROUND_MAJORITY_SETTLE_SECS, 5, 0.0, FlightPhase::Climb, true);
        assert_eq!(p, FlightPhase::Landing, "setup precondition: promotion must have happened");
        let (p, _) = sim.fly_with_tick(10, 5, 0.0, FlightPhase::Arrived, true);
        assert_eq!(p, FlightPhase::Arrived);
    }

    /// Wie `touch_and_go_resyncs_to_climb`, aber ausgehend von einem SELBST
    /// promoteten `Landing` (alte FSM hing bei `Approach` fest, statt via
    /// 1:1-Mirror `Landing` selbst korrekt zu melden) — die
    /// Fresh-Transition-Weiche in `step()` muss unabhängig davon greifen,
    /// WIE `Landing` erreicht wurde.
    #[test]
    fn self_promoted_landing_resyncs_immediately_on_fresh_old_fsm_transition() {
        let mut sim = sim_at_final();
        // Alte FSM haengt bei Approach fest (noch nicht mal Final) waehrend
        // wir schon selbst auf Landing promoten.
        let (p, _) =
            sim.fly_with_tick(GROUND_MAJORITY_SETTLE_SECS, 5, 0.0, FlightPhase::Approach, true);
        assert_eq!(p, FlightPhase::Landing, "self-promoted trotz haengender alter FSM");
        // Fresh Transition: alte FSM meldet jetzt (verspätet) Climb — z. B.
        // ein spät erkannter Touch-and-Go/Rejected-Landing.
        let (p, _) = sim.fly(5, 1800.0, FlightPhase::Climb);
        assert_eq!(
            p,
            FlightPhase::Climb,
            "frische alte-FSM-Transition muss sofort syncen, auch ab selbst-promotetem Landing"
        );
    }

    /// v0.19.1 Code-Review-Härtung: eine frische `old_phase`-Änderung ALLEIN
    /// (ohne dass `on_ground` auch nur einmal `false` wird) darf das korrekt
    /// gehaltene `Landing` NICHT verlassen — die alte FSM könnte (in einem
    /// aktuell nicht reproduzierbaren, aber wegen der Ausgangslage dieses
    /// ganzen Fixes plausiblen Fall) zwischen zwei En-Route-Phasen
    /// hin- und herspringen, ohne dass der Flieger je abgehoben hat.
    /// `fresh_enroute_transition_from_landing` verlangt deshalb zusätzlich
    /// `!on_ground` DIESES Ticks (Rohsignal, nicht das nachhinkende Segment).
    #[test]
    fn stuck_old_fsm_flip_between_enroute_phases_does_not_leave_landing_while_grounded() {
        let mut sim = sim_at_final();
        let (p, _) =
            sim.fly_with_tick(GROUND_MAJORITY_SETTLE_SECS, 5, 0.0, FlightPhase::Climb, true);
        assert_eq!(p, FlightPhase::Landing, "setup precondition: promotion must have happened");
        // Alte FSM springt auf einen ANDEREN En-Route-Wert — aber der
        // Flieger steht die ganze Zeit am Boden (on_ground=true durchgehend).
        let (p, _) = sim.fly_with_tick(10, 5, 0.0, FlightPhase::Approach, true);
        assert_eq!(
            p,
            FlightPhase::Landing,
            "eine alte-FSM-Aenderung OHNE on_ground=false darf Landing nicht verlassen"
        );
    }

    /// R1 (Observed-Cruise-Ref): ref-loser Descent-Level-Off tief unten
    /// bleibt Descent — der 240-s-Fallback hätte fälschlich Cruise gelatcht,
    /// obsref (höchste erreichte Höhe) verhindert das.
    #[test]
    fn descent_level_off_refless_stays_descent_via_obsref() {
        let mut sim = Sim::new(None);
        sim.depart();
        sim.alt = 500.0;
        // Auf ~FL350 steigen → obsref etabliert sich bei ~35000.
        sim.fly(1000, 2100.0, FlightPhase::Climb);
        sim.alt = 35000.0;
        sim.fly(180, 0.0, FlightPhase::Cruise);
        // Sinkflug.
        sim.fly(800, -2000.0, FlightPhase::Descent);
        sim.alt = 8000.0;
        // 10 min Level-Off auf 8000 ft (Restriktion tief im Sinkflug),
        // ref-los. Ohne obsref → 240-s-Fallback → Cruise (falsch).
        let (p, s) = sim.fly(600, 0.0, FlightPhase::Descent);
        assert_eq!(p, FlightPhase::Descent, "Restriktion tief im Sinkflug ≠ Cruise");
        assert_eq!(s, Segment::Level);
    }

    /// E1 (Cruise→Approach-Kante): Cruise, das unter 5000 ft AGL sinkt,
    /// geht in Approach statt kleben zu bleiben.
    #[test]
    fn cruise_low_agl_descending_to_approach_edge() {
        let mut sim = Sim::new(None);
        sim.depart();
        // Cruise tief etablieren (agl > 5000, ref-los via Dauer-Fallback).
        sim.alt = 6000.0;
        let (p, _) = sim.fly(300, 0.0, FlightPhase::Cruise);
        assert_eq!(p, FlightPhase::Cruise);
        // Moderater Sinkflug unter 5000 AGL (NICHT Fast-Path: agl > 3000).
        sim.alt = 4500.0;
        let (p, _) = sim.fly(10, -300.0, FlightPhase::Descent);
        assert_eq!(p, FlightPhase::Approach, "Cruise <5000 AGL + sinkend → Approach");
    }

    /// E2 (Balloon-Guard): ein kurzer, schwacher Steig-Blip im Final
    /// (< `GA_CONFIRM_FT` = 700 ft AGL-Gewinn) latcht NICHT Climb; erst der
    /// bestätigte Go-Around (≥ 700 ft Gewinn) tut es.
    #[test]
    fn go_around_balloon_guard_holds_until_confirmed() {
        let mut sim = Sim::new(None);
        sim.depart();
        // In den Anflug/Final bringen.
        sim.alt = 4000.0;
        sim.fly(200, -1000.0, FlightPhase::Approach);
        sim.alt = 600.0;
        let (p, _) = sim.fly(20, -300.0, FlightPhase::Final);
        assert_eq!(p, FlightPhase::Final);
        // Schwacher, anhaltender Steig-Blip (+400 fpm): erzeugt irgendwann
        // ein Climbing-Segment, aber solange < 700 ft ab Anker gewonnen sind,
        // bleibt es Final (Balloon-Guard). +400 fpm × 60 s = 400 ft < 700.
        let (p, _) = sim.fly(60, 400.0, FlightPhase::Climb);
        assert_eq!(p, FlightPhase::Final, "Balloon <700 ft AGL darf nicht Climb latchen");
        // Echter Go-Around-Steigflug (+2500 fpm) → > 700 ft Gewinn → Climb.
        let mut reached = false;
        for _ in 0..12 {
            let (p, _) = sim.fly(15, 2500.0, FlightPhase::Climb);
            if p == FlightPhase::Climb {
                reached = true;
                break;
            }
        }
        assert!(reached, "bestätigter Go-Around muss Climb erreichen");
    }

    #[test]
    fn segment_snake_strings_are_stable() {
        // Wire-Format-Vertrag für JSONL-Konsumenten.
        assert_eq!(Segment::Ground.as_snake_str(), "ground");
        assert_eq!(Segment::Climbing.as_snake_str(), "climbing");
        assert_eq!(Segment::Level.as_snake_str(), "level");
        assert_eq!(Segment::Descending.as_snake_str(), "descending");
        assert_eq!(Segment::Insufficient.as_snake_str(), "insufficient");
    }
}
