//! Touchdown-Forensik v2 — Layer 1 (Detection) + Layer 2 (Validation) +
//! Layer 3 (VS-Calculation am impact_frame).
//!
//! Spec: docs/spec/touchdown-forensics-v2.md (v2.3, approved)
//!
//! **Designed pure** (sim-agnostisch, ohne Side-Effects auf Tauri-State)
//! damit die Logik 1:1 gegen historische JSONL-Replays gepruft werden kann.
//! Layer 4 (LandingEpisode-Aggregation + Lifecycle) sitzt ausserhalb dieses
//! Moduls weil sie Tauri-State + Persistierung braucht.
//!
//! **Sim-Trennung** ist STRUKTURELL unvermeidbar weil X-Plane das wichtigste
//! Validation-Signal hat (gear_normal_force_n) und MSFS nicht. Siehe Spec
//! Sektion 4.

use chrono::{DateTime, Duration, Utc};
use recorder::TouchdownWindowSample;
use serde::{Deserialize, Serialize};

use crate::aircraft_category::AircraftCategory;
use crate::SimKind;

/// **forensics_version=2** Marker fuer Events + PIREP-payload.
/// Recorder/aeroacars-live identifiziert via diesem Wert welche
/// Auswertungs-Logik zu verwenden ist.
pub const FORENSICS_VERSION: u8 = 2;

// ─── v0.7.6 P1-2: Bounce-Threshold-Konstanten ─────────────────────────────
//
// Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-2.
//
// Trennung zwischen "forensisch sichtbar" und "scoring-relevant" damit Pilot
// kleine Federwerk-Hopser im Replay sehen kann ohne dass jeder Mikro-Hopser
// ihn im Score bestraft. Real-Beleg: SAS9987 (v0.7.5 PW68L0QGJkq0D63J) hatte
// bounce_max_agl_ft=13.6 → forensisch ein Hopser, scoring-irrelevant.
//
// Beide Konstanten leben hier (touchdown_v2 = Forensik-Schicht), NICHT in
// der landing-scoring-Crate, weil nur die Forensik AGL-Verlauf + Hopser-
// Hoehen kennt. Die landing-scoring-Crate vertraut dem Caller und bekommt
// nur den finalen scored_bounce_count als Input.

/// Mindestens 5 ft AGL-Excursion damit ein Wiederabheben **forensisch**
/// gezaehlt wird. Filtert Sim-Float-Noise (typisch 1-2 ft) und sanftes
/// Federwerk-Oszillieren raus, laesst aber sichtbare Hopser im Replay-
/// Tab durch. Erscheint im PIREP als `forensic_bounce_count`.
pub const BOUNCE_FORENSIC_MIN_AGL_FT: f32 = 5.0;

/// Mindestens 15 ft AGL-Excursion damit ein Wiederabheben im Sub-Score
/// **bestraft** wird. Hoch genug dass ein "echter" Bounce (Pitch-Up nach
/// harter Landung) erfasst wird, aber nicht jeder Federwerk-Hopser den
/// Pilot bestraft. Erscheint im PIREP als `scored_bounce_count` und
/// landet im `landing-scoring::sub_bounces`-Sub-Score.
///
/// v0.7.7+: Schwelle bei 15.0 ft eingependelt nach Echt-Daten-Review,
/// kein Patch notwendig — Beobachtungs-Sample war ausgeglichen.
pub const BOUNCE_SCORED_MIN_AGL_FT: f32 = 15.0;

/// Seaplane / amphibian water-touchdown descent gate (fpm). A water touchdown
/// is the floats / hull SETTLING onto the water — the impact V/S must be a
/// genuine sink below this. This is what separates a landing from a level low
/// pass, a step-taxi skim, or a glassy go-around that merely lingers in the
/// water-contact band: those have V/S ≈ 0 and are rejected even though they
/// sustain low-AGL. A real glassy-water landing descends ~100-200 fpm (well
/// past this), so it is unaffected. Deliberately larger in magnitude than the
/// fixed-wing −10 fpm floor so no phantom-validation window is left open for
/// non-landing low flight over water (review finding, v0.15.21).
pub const WATER_TOUCHDOWN_MIN_DESCENT_FPM: f32 = -50.0;

// ─── Bodenhöhe des Flugzeugs (Referenz für die Tiefflug-Prüfung) ─────────
//
// Befund DLH 880 (Sven M, 15.09.2026, Fenix A321, MSFS 2024): Eine echte,
// sehr weiche Landung (−50 fpm im Kontaktmoment, G-Spitze 1,01) wurde als
// FALSE_EDGE verworfen. Die Tiefflug-Prüfung verlangte absolut „unter 5 ft
// über Grund". Der Simulator misst die Höhe über Grund aber am Bezugspunkt
// des Flugzeugs, nicht an den Rädern: Der Fenix A321 steht mit 9 ft auf
// dem Boden, im Kontaktmoment waren es 11,9 ft. Die Prüfung konnte für
// dieses Flugzeug also nie bestehen — nur ein kräftigeres Aufsetzen (G und
// Sinkrate) rettete bisher die Abstimmung. Folge: kein Aufsetzfenster, kein
// Touchdown beim Server, der PIREP hing in der Prüfung.
//
// Die Prüfung meint physikalisch „die Räder sind am Boden". Deshalb gilt
// die Grenze jetzt RELATIV zur Bodenhöhe genau dieses Flugzeugs, gemessen
// beim Rollen vor dem Start. Der Hopser-Zähler rechnet schon so (Höhe über
// dem Boden, nicht über dem Gelände).
//
// Warum nicht der Wert im Kontaktmoment als Bezug: Ein flackerndes
// Bodenkontakt-Signal in der Luft brächte seine Flughöhe als „Boden" mit
// und bestünde die Prüfung immer. Die Rollmessung ist vom Kandidaten
// unabhängig — ein Flackern in 50 ft bleibt weit über der Grenze.

/// Spielraum über der gemessenen Bodenhöhe. Entspricht der bisherigen
/// absoluten 5-ft-Grenze bei Flugzeugen, deren Bezugspunkt am Boden liegt.
pub const TIEFFLUG_SPIELRAUM_FT: f32 = 5.0;

/// Obergrenze der anerkannten Bodenhöhe. Proben darüber zählen gar nicht
/// (kein Kappen) — ein Flugzeug jenseits davon behält die alte absolute
/// Grenze, nie eine weichere.
///
/// Korpus 15.09.2026 (1203 Server-Flüge): höchster Musterwert 17,4 ft
/// (ToLiss A346), A350 15,1, MD-11F 16,4, 777F 13,5; Streuung je Muster
/// über alle Flüge unter 2 ft. Die erste Fassung erlaubte 30 ft — Codex
/// wies zu Recht darauf hin, dass das eine Tiefflug-Grenze bis 35 ft
/// zuliess, ohne dass ein reales Muster das braucht.
pub const BODENHOEHE_MAX_FT: f32 = 22.0;

/// Mindestzahl Rollproben, bevor die Bodenhöhe gilt (bei 50 Hz etwa 5 s).
pub const BODENHOEHE_MIN_PROBEN: u32 = 250;

/// Proben oberhalb dieser Rollgeschwindigkeit zählen nicht (Startlauf,
/// Ausrollen — die Federung arbeitet, der Wert ist unruhig).
pub const BODENHOEHE_MAX_GS_KT: f32 = 40.0;

const BODENHOEHE_UNTERGRENZE_FT: f32 = -5.0;
const BODENHOEHE_KLASSE_FT: f32 = 0.25;
const BODENHOEHE_KLASSEN: usize =
    ((BODENHOEHE_MAX_FT - BODENHOEHE_UNTERGRENZE_FT) / BODENHOEHE_KLASSE_FT) as usize;

/// Robuste Bodenhöhe aus den Rollproben: Median über ein Häufigkeitsraster.
///
/// Median statt Mittelwert, weil beim Laden der Szenerie oder im Menü
/// einzelne absurde Werte auftreten (RYR 2: −148 ft während einer Pause).
/// Das Raster hält den Speicher konstant und lässt sich mit dem Flug
/// sichern, damit ein Wiederaufnehmen nach Neustart die Messung behält.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BodenhoehenReferenz {
    #[serde(default)]
    klassen: Vec<u32>,
    #[serde(default)]
    anzahl: u32,
}

impl BodenhoehenReferenz {
    /// Eine Probe aufnehmen. Der Aufrufer entscheidet, ob das Flugzeug
    /// gerade ruhig am Boden steht oder rollt (siehe `ist_rollprobe`).
    pub fn beobachte(&mut self, agl_ft: f32) {
        if !agl_ft.is_finite() || agl_ft < BODENHOEHE_UNTERGRENZE_FT || agl_ft >= BODENHOEHE_MAX_FT
        {
            return;
        }
        if self.klassen.len() != BODENHOEHE_KLASSEN {
            self.klassen = vec![0; BODENHOEHE_KLASSEN];
        }
        let idx = ((agl_ft - BODENHOEHE_UNTERGRENZE_FT) / BODENHOEHE_KLASSE_FT) as usize;
        let idx = idx.min(BODENHOEHE_KLASSEN - 1);
        self.klassen[idx] = self.klassen[idx].saturating_add(1);
        self.anzahl = self.anzahl.saturating_add(1);
    }

    pub fn anzahl(&self) -> u32 {
        self.anzahl
    }

    /// Median der Rollproben — `None`, solange zu wenige vorliegen.
    pub fn bodenhoehe_ft(&self) -> Option<f32> {
        if self.anzahl < BODENHOEHE_MIN_PROBEN || self.klassen.len() != BODENHOEHE_KLASSEN {
            return None;
        }
        let haelfte = self.anzahl.div_ceil(2);
        let mut summe = 0_u32;
        for (i, n) in self.klassen.iter().enumerate() {
            summe = summe.saturating_add(*n);
            if summe >= haelfte {
                return Some(BODENHOEHE_UNTERGRENZE_FT + (i as f32 + 0.5) * BODENHOEHE_KLASSE_FT);
            }
        }
        None
    }
}

/// Zählt diese Probe zur Bodenhöhe? Am Boden, langsam, kein Pausen- oder
/// Versetzmodus.
pub fn ist_rollprobe(on_ground: bool, groundspeed_kt: f32, paused: bool, slew: bool) -> bool {
    on_ground
        && !paused
        && !slew
        && groundspeed_kt.is_finite()
        && groundspeed_kt < BODENHOEHE_MAX_GS_KT
}

/// Grenze der Tiefflug-Prüfung für dieses Flugzeug. Ohne gültige Messung
/// (Flug in der Luft wiederaufgenommen, zu kurz gerollt) gilt die bisherige
/// absolute Grenze — nie weniger streng als vorher.
pub fn tiefflug_grenze_ft(bodenhoehe_ft: Option<f32>) -> f32 {
    let boden = bodenhoehe_ft
        .filter(|b| b.is_finite())
        .map(|b| b.clamp(0.0, BODENHOEHE_MAX_FT))
        .unwrap_or(0.0);
    boden + TIEFFLUG_SPIELRAUM_FT
}

// ─── Layer 1: TD-Candidate Detection ──────────────────────────────────────

/// Ein Sample-Pair fuer Edge-Detection (prev → current).
/// Liefert Some(TdCandidate) wenn ein Edge-Trigger detected wurde.
///
/// X-Plane: Edge wenn prev.in_air UND (current.on_ground ODER gear_force > epsilon)
/// MSFS:    Edge wenn prev.in_air UND current.on_ground
///
/// `prev.in_air` = !prev.on_ground UND (prev.gear_force.unwrap_or(0) <= epsilon)
pub fn detect_td_candidate(
    prev: Option<&TouchdownWindowSample>,
    current: &TouchdownWindowSample,
    current_idx: usize,
    sim: SimKind,
) -> Option<TdCandidate> {
    const GEAR_FORCE_EPSILON_N: f32 = 1.0;

    let prev = prev?;
    let prev_in_air = !prev.on_ground
        && prev
            .gear_normal_force_n
            .map(|f| f <= GEAR_FORCE_EPSILON_N)
            .unwrap_or(true);
    if !prev_in_air {
        return None;
    }

    let edge_now = if sim.is_xplane() {
        current.on_ground
            || current
                .gear_normal_force_n
                .map(|f| f > GEAR_FORCE_EPSILON_N)
                .unwrap_or(false)
    } else {
        // MSFS / Off — only on_ground edge (kein gear_normal_force_n verfuegbar)
        current.on_ground
    };

    if !edge_now {
        return None;
    }

    Some(TdCandidate {
        edge_sample_index: current_idx,
        edge_at: current.at,
        edge_agl_ft: current.agl_ft,
        edge_vs_fpm: current.vs_fpm,
        edge_gear_force_n: current.gear_normal_force_n,
        edge_g_force: current.g_force,
        edge_total_weight_kg: current.total_weight_kg,
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TdCandidate {
    pub edge_sample_index: usize,
    pub edge_at: DateTime<Utc>,
    pub edge_agl_ft: f32,
    pub edge_vs_fpm: f32,
    pub edge_gear_force_n: Option<f32>,
    pub edge_g_force: f32,
    pub edge_total_weight_kg: Option<f32>,
}

// ─── Layer 2: TD-Validation (sim-spezifisch) ──────────────────────────────

/// Ergebnis der Validation. Bei VALIDATED wird die TD als „echter contact"
/// behandelt; bei FALSE_EDGE ist es ein Streifschuss/Float.
#[derive(Debug, Clone)]
pub enum ValidationResult {
    Validated {
        result: ValidationDetail,
    },
    FalseEdge {
        reason: FalseEdgeReason,
        result: ValidationDetail,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationDetail {
    pub sim: SimKind,
    pub gear_force_threshold_n: f32,
    pub gear_force_pass: bool,
    pub gear_force_peak_in_window_n: Option<f32>,
    pub gear_force_sustained_ms: Option<u64>,
    /// How many samples in the [edge_at, edge_at+500ms] window carried
    /// ANY finite `gear_normal_force_n` value, regardless of whether it
    /// was above or below `gear_force_threshold_n`. Distinct from
    /// `gear_force_pass`'s own 2-sample "sustained run" requirement —
    /// this counts the raw data density available to judge from at
    /// all. See `validate_candidate`'s X-Plane branch for why this
    /// matters: below `MIN_GEAR_FORCE_SAMPLES_IN_WINDOW`, gear_force
    /// data is too sparse to trust as a MUST-PASS gate (e.g. an FPS
    /// stall/reconnect right at touchdown), so validation falls back to
    /// voting instead of treating "not enough data" the same as
    /// "genuinely no impact force". `#[serde(default)]` so old
    /// persisted episodes/false-edges from before this field existed
    /// still replay (as 0 — they never had this data to begin with).
    #[serde(default)]
    pub gear_force_sample_count_in_window: usize,
    pub g_force_pass: Option<bool>,
    pub g_force_peak_in_window: f32,
    pub low_agl_persistence_pass: bool,
    pub low_agl_actual_ms: u64,
    /// Angewandte Grenze der Tiefflug-Prüfung (Bodenhöhe + Spielraum).
    /// `None` bei Datensätzen von vor dieser Änderung (damals fest 5 ft).
    #[serde(default)]
    pub low_agl_grenze_ft: Option<f32>,
    pub sustained_ground_pass: Option<bool>,
    pub sustained_ground_actual_ms: u64,
    pub vs_negative_pass: bool,
    pub vs_at_impact_used_for_test: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FalseEdgeReason {
    /// X-Plane: gear_force unter threshold (oder nicht lange genug)
    GearForceBelowThreshold,
    /// MSFS: weniger als 3 von 4 Tests passed
    InsufficientVoteScore,
    /// MSFS: das Post-Edge-Fenster hatte zu wenige Samples, um low_agl_persistence
    /// ODER sustained_ground_contact ueberhaupt zu beurteilen — "keine Daten" ist
    /// keine Aussage ueber "kein Kontakt". Getrennt von `InsufficientVoteScore`
    /// (das heisst: es GAB genug Daten, und die Stimmen reichten trotzdem nicht).
    /// Siehe `validate_candidate`s MSFS-Zweig fuer die Herleitung.
    InsufficientTelemetry,
}

/// Mass-aware gear-force threshold:
/// max(1000 N floor, 3% × total_weight × 9.80665).
///
/// 1000 N als hartes Minimum schuetzt vor zu strikt-strict bei Glidern/Ultralight.
/// 3% × static weight skaliert mit Aircraft-Mass — A330 (250t) ergibt ~73.5 kN,
/// Cessna 152 (757kg) ergibt ~222 N → floor wins → 1000 N.
pub fn gear_force_threshold_n(total_weight_kg: Option<f32>) -> f32 {
    const ABS_FLOOR: f32 = 1000.0;
    const MASS_RATIO: f32 = 0.03;
    const G: f32 = 9.80665;

    let dynamic = total_weight_kg
        .filter(|w| *w > 100.0)
        .map(|w| w * G * MASS_RATIO)
        .unwrap_or(ABS_FLOOR);
    dynamic.max(ABS_FLOOR)
}

/// Prueft, ob `samples` das Fenster `[fenster_start, fenster_ende]` LUECKENLOS
/// abdeckt — kein Abstand groesser als `max_gap_ms`, weder am Rand noch
/// dazwischen.
///
/// # Warum das mehr ist als "gibt es ein Sample nah am Ende"
///
/// Codex-QS-Fund (Vereinheitlichung 09/2026, Abschlusspruefung vor dem
/// Settle-Pfad-Release): eine fruehere Fassung dieser Pruefung mass nur den
/// Abstand des LETZTEN Samples zum Fensterende. Samples bei 0/400/900ms in
/// einem 1000ms-Fenster haetten sie bestanden (900ms liegt nur 100ms vom
/// Ende entfernt) — obwohl zwischen 400ms und 900ms 500ms lang GAR NICHTS
/// beobachtet wurde. Fuer den Settle-Pfad, der die physikalische Abstimmung
/// komplett ersetzt, ist das nicht "durchgehende Abdeckung", sondern zwei
/// duenne Inseln mit einer Luecke dazwischen. Diese Funktion sortiert die
/// Samples im Fenster nach Zeit und verlangt, dass JEDER Abstand — vom
/// Fensteranfang zum ersten Sample, zwischen je zwei aufeinanderfolgenden
/// Samples, und vom letzten Sample zum Fensterende — innerhalb von
/// `max_gap_ms` bleibt.
fn deckt_fenster_durchgehend_ab(
    samples: &[TouchdownWindowSample],
    fenster_start: DateTime<Utc>,
    fenster_ende: DateTime<Utc>,
    max_gap_ms: i64,
) -> bool {
    let mut zeitpunkte: Vec<DateTime<Utc>> = samples
        .iter()
        .filter(|s| s.at >= fenster_start && s.at <= fenster_ende)
        .map(|s| s.at)
        .collect();
    if zeitpunkte.is_empty() {
        return false;
    }
    zeitpunkte.sort();

    let anfangsluecke = (zeitpunkte[0] - fenster_start).num_milliseconds();
    if anfangsluecke > max_gap_ms {
        return false;
    }
    for paar in zeitpunkte.windows(2) {
        let luecke = (paar[1] - paar[0]).num_milliseconds();
        if luecke > max_gap_ms {
            return false;
        }
    }
    let endluecke = (fenster_ende - *zeitpunkte.last().expect("nicht leer")).num_milliseconds();
    endluecke <= max_gap_ms
}

/// Grösste zulässige Lücke zwischen zwei Proben im Bewertungsfenster.
///
/// # Wie die Zahl zustande kommt (Untersuchung 12.09.2026)
///
/// Die Landerate wird in Stufen bewertet; die schmalste ist 90 fpm breit
/// (`sub_landing_rate.rs`: unter 90 → 85 Punkte, 90–250 → 100, 250–400 → 80,
/// 400–600 → 45, 600–1000 → 20, darüber 0). Eine Lücke ist dann zu gross,
/// wenn sich die Sinkrate darin um mehr als eine solche Stufe ändern kann —
/// denn dann ist unbekannt, in welcher Stufe die Landung wirklich lag.
///
/// Über 1119 Landungen des Bestands gemessen (stärkste Änderung je Landung
/// in der letzten Sekunde vor dem Aufsetzen, nur aus Probenpaaren mit
/// 20–200 ms Abstand): Median 188 fpm/s, bei den dynamischsten zehn Prozent
/// 663 fpm/s. Eine Stufe ist damit im Mittel nach 479 ms durchlaufen, bei
/// den dynamischen Landungen aber schon nach 136 ms.
///
/// 200 ms ist eine **empirische Ausschlussgrenze** deutlich oberhalb der
/// beobachteten normalen Probenabstände (Median 33 ms, 99. Perzentil 112 ms)
/// und deutlich unterhalb der gefundenen Ausfälle (229 ms bis 921 ms).
///
/// Sie garantiert KEINE unveränderte Score-Stufe — bei 663 fpm/s sind
/// innerhalb von 200 ms schon 133 fpm Änderung möglich, und nahe einer
/// Stufengrenze genügt weniger (Codex-Abnahme 12.09.2026: die erste Fassung
/// dieses Kommentars behauptete das Gegenteil). Die Grenze trennt
/// „aufgezeichnet" von „nicht aufgezeichnet", nicht „genau" von „ungenau".
///
/// Dass `MAX_COVERAGE_GAP_MS` in der Kontaktvalidierung denselben Wert nutzt,
/// ist ein Hinweis auf dieselbe Grössenordnung, kein unabhängiger Nachweis.
pub const MAX_BEWERTUNGS_LUECKE_MS: i64 = 200;

/// Mindestzahl verwertbarer Proben im Bewertungsfenster.
///
/// Fängt den Fall, den eine reine Lückenprüfung übersieht: gleichmässig,
/// aber viel zu grob abgetastet. Bei Soll-Takt (50 Hz) liegen im Fenster
/// rund 55 Proben, im Bestand sind es im Mittel 35. Unter 12 ist die
/// Aufzeichnung in keinem Fall mehr belastbar — die beiden schlechtesten
/// Landungen des Bestands hatten 5 und 8.
pub const MIN_BEWERTUNGS_PROBEN: usize = 12;

/// Das Fenster, in dem die Aufzeichnung sitzen muss: eine Sekunde vor dem
/// Bodenkontakt bis kurz danach.
///
/// Fest am Kontakt, NICHT am gewählten Impact-Frame: Sonst schwankte die
/// Fensterlänge zwischen 850 und 1200 ms, und `MIN_BEWERTUNGS_PROBEN`
/// bedeutete bei jeder Landung etwas anderes. Die Sekunde davor deckt die
/// Flare ab, in der sich die Sinkrate entscheidet; die 100 ms danach
/// enthalten das gesamte Auswahlfenster des Impact-Frames.
pub const BEWERTUNGS_FENSTER_VOR_MS: i64 = 1000;
pub const BEWERTUNGS_FENSTER_NACH_MS: i64 = 100;

/// Warum eine Landung nicht bewertet werden kann.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FehlendeAbdeckung {
    /// Grösste Lücke zwischen zwei Proben im Fenster, in Millisekunden.
    pub groesste_luecke_ms: i64,
    /// Verwertbare Proben im Fenster.
    pub proben: usize,
}

/// Reicht die Aufzeichnung, um diese Landung zu bewerten?
///
/// # Der Anlass (CFG 2090, EDDF→KPDX, 12.09.2026)
///
/// Eine Landung bekam 97 Punkte und die Note A+, obwohl der Aufsetzmoment
/// nicht aufgezeichnet wurde: Zwischen der letzten Probe in der Luft und dem
/// ersten Bodenkontakt lagen 0,92 s ohne jede Messung. Die Bewertung nahm
/// den ersten Frame NACH dem Aufsetzen — dort stand −10,39 fpm, und weil
/// die Kaskade nur prüft, ob der Wert unter −10,0 liegt, galt er als
/// bestmöglich belegt.
///
/// Die Prüfung hier trennt "Landung erkannt" von "Landung messbar". Beides
/// darf auseinanderfallen: Das Flugzeug ist gelandet, wir wissen nur nicht
/// wie. Aus einer solchen Landung darf keine Zahl und keine Note entstehen —
/// weder eine gute noch eine schlechte.
///
/// Gibt `Ok(())` zurück, wenn bewertet werden darf, sonst die Messwerte,
/// die dagegen sprechen.
pub fn pruefe_bewertbarkeit(
    samples: &[TouchdownWindowSample],
    contact_at: DateTime<Utc>,
) -> Result<(), FehlendeAbdeckung> {
    let start = contact_at - chrono::Duration::milliseconds(BEWERTUNGS_FENSTER_VOR_MS);
    let ende = contact_at + chrono::Duration::milliseconds(BEWERTUNGS_FENSTER_NACH_MS);

    // Verwertbar heisst: im Fenster, mit endlichen Werten. Eine Probe mit
    // NaN-Sinkrate zählt nicht mit — sie trägt nichts zur Messung bei, und
    // mitgezählt würde sie eine Abdeckung vortäuschen.
    let mut zeitpunkte: Vec<DateTime<Utc>> = samples
        .iter()
        .filter(|s| s.at >= start && s.at <= ende)
        .filter(|s| s.vs_fpm.is_finite() && s.agl_ft.is_finite())
        .map(|s| s.at)
        .collect();
    zeitpunkte.sort();
    zeitpunkte.dedup();

    let proben = zeitpunkte.len();
    if proben == 0 {
        return Err(FehlendeAbdeckung {
            groesste_luecke_ms: (ende - start).num_milliseconds(),
            proben: 0,
        });
    }

    // Randlücken zählen mit: Liegt die erste Probe erst 600 ms nach
    // Fensterbeginn, fehlt die halbe Flare — auch wenn die restlichen
    // Proben dicht liegen.
    let mut groesste = (zeitpunkte[0] - start).num_milliseconds();
    for paar in zeitpunkte.windows(2) {
        groesste = groesste.max((paar[1] - paar[0]).num_milliseconds());
    }
    groesste = groesste.max((ende - *zeitpunkte.last().expect("nicht leer")).num_milliseconds());

    if groesste > MAX_BEWERTUNGS_LUECKE_MS || proben < MIN_BEWERTUNGS_PROBEN {
        return Err(FehlendeAbdeckung {
            groesste_luecke_ms: groesste,
            proben,
        });
    }
    Ok(())
}

/// Validate eine TdCandidate gegen die sim-spezifischen Tests.
///
/// X-Plane: gear_force ist MUST-PASS (Anchor). A1 FAIL → Validation FAIL.
/// MSFS:    weiches Voting (3 von 4 Tests muessen PASS).
///
/// Time-windows werden via `at`-Timestamps gemessen, NICHT via Sample-Count
/// (Sampler ist nicht garantiert genau 50 Hz).
pub fn validate_candidate(
    candidate: &TdCandidate,
    samples: &[TouchdownWindowSample],
    sim: SimKind,
    impact_frame_vs: f32,
    category: AircraftCategory,
    bodenhoehe_ft: Option<f32>,
) -> ValidationResult {
    let edge_at = candidate.edge_at;
    let threshold_n = gear_force_threshold_n(candidate.edge_total_weight_kg);

    // Test: gear_force-impact (X-Plane only — MUST-PASS)
    let (gear_force_pass, gear_force_peak, gear_force_sustained_ms, gear_force_sample_count) =
        evaluate_gear_force_test(samples, edge_at, threshold_n);

    // Test: g_force-spike (MSFS-relevant)
    let g_force_peak = evaluate_g_force_peak(samples, edge_at);
    let g_force_pass = g_force_peak > 1.05;

    // Test: low_agl_persistence (beide Sims)
    // Relativ zur Bodenhöhe dieses Flugzeugs (Befund DLH 880, siehe
    // `BodenhoehenReferenz`).
    //
    // Hubschrauber und Wasserflugzeuge behalten die absolute Grenze (Codex-
    // Abnahme 15.09.2026): Ihr Bezugspunkt liegt ohnehin nah am Boden, sie
    // starten aber oft von Plattformen, Dächern oder aus dem Wasser — dort
    // wäre die Rollmessung kein Maß für "die Kufen/Schwimmer sind unten".
    // Ihr eigener Bestätigungsweg (Präsenz über die Zeit) hat zudem keine
    // Sinkraten- und G-Anker mehr, eine höhere Grenze wöge dort schwerer.
    let tiefflug_grenze = if category.is_non_conventional() {
        tiefflug_grenze_ft(None)
    } else {
        tiefflug_grenze_ft(bodenhoehe_ft)
    };
    let (low_agl_pass, low_agl_ms) =
        evaluate_low_agl_persistence(samples, edge_at, tiefflug_grenze);

    // Test: sustained_ground_contact (MSFS-relevant)
    let (sustained_pass, sustained_ms) = evaluate_sustained_ground(samples, edge_at);

    // Test: vs_negative_at_impact (beide)
    let vs_negative_pass = impact_frame_vs < -10.0;

    let detail = ValidationDetail {
        sim,
        gear_force_threshold_n: threshold_n,
        gear_force_pass,
        gear_force_peak_in_window_n: gear_force_peak,
        gear_force_sustained_ms,
        gear_force_sample_count_in_window: gear_force_sample_count,
        g_force_pass: Some(g_force_pass),
        g_force_peak_in_window: g_force_peak,
        low_agl_persistence_pass: low_agl_pass,
        low_agl_actual_ms: low_agl_ms,
        low_agl_grenze_ft: Some(tiefflug_grenze),
        sustained_ground_pass: Some(sustained_pass),
        sustained_ground_actual_ms: sustained_ms,
        vs_negative_pass,
        vs_at_impact_used_for_test: impact_frame_vs,
    };

    // ── Category-aware validation (rotorcraft / seaplane) ──────────────────
    // Real helicopter and seaplane touchdowns are deliberately near-zero V/S
    // with NO gear-force or G-force spike: the FAA Helicopter Flying Handbook
    // has the pilot cushion the set-down with collective to "the slowest rate
    // possible", and a glassy-water seaplane landing is a constant-attitude
    // soft contact (AOPA: "wait for contact … never flare"). The fixed-wing
    // anchors below (gear-force MUST-PASS, g>1.05, vs<-10) therefore REJECT a
    // clean soft set-down as a FalseEdge — which is exactly why these
    // categories were silently dropped. For them we anchor on PRESENCE
    // instead: sustained low-AGL (<5 ft for ≥1000 ms) plus, for rotorcraft,
    // sustained ground contact (≥500 ms). A single glitched on-ground/low-AGL
    // tick CANNOT satisfy a sustained-1000 ms window, so phantom-touchdown
    // protection is fully preserved. Gated on category ⇒ fixed-wing
    // validation is byte-for-byte unchanged.
    if category.is_non_conventional() {
        let presence_ok = if category.water_capable() {
            // Water: `on_ground` stays false while floating, so the sustained
            // low-AGL window (AGL≈0 = on the water surface) confirms the
            // aircraft is ON the surface. But sustained low-AGL ALONE is also
            // satisfied by a level low pass / step-taxi skim / glassy go-around
            // that lingers near the water WITHOUT landing, so we ALSO require a
            // genuine descent onto the surface (impact V/S a clear sink). The
            // two together separate a settling water touchdown from non-landing
            // low flight — without the descent gate any sustained <5 ft water
            // flight would phantom-validate as a touchdown (review finding).
            low_agl_pass && impact_frame_vs < WATER_TOUCHDOWN_MIN_DESCENT_FPM
        } else {
            // Rotorcraft: skids/wheels assert `on_ground`, so require BOTH the
            // sustained ground contact and the sustained low-AGL window.
            low_agl_pass && sustained_pass
        };
        return if presence_ok {
            ValidationResult::Validated { result: detail }
        } else {
            ValidationResult::FalseEdge {
                reason: FalseEdgeReason::InsufficientVoteScore,
                result: detail,
            }
        };
    }

    if sim.is_xplane() {
        // v0.19.x QS (backlog "X-Plane gear-force MUST-PASS defeated by
        // <2-sample windows"): gear_force is only trustworthy as a
        // MUST-PASS anchor when the window actually HAS enough of it to
        // judge from. Below MIN_GEAR_FORCE_SAMPLES_IN_WINDOW, "not
        // enough data" was being treated exactly the same as
        // "genuinely no impact force" — both fell straight to FalseEdge
        // — which could discard a REAL hard-landing/gear-collapse
        // whenever the 500ms window happened to be data-sparse (an FPS
        // stall or telemetry reconnect right at touchdown is a
        // realistic trigger, and X-Plane has no independent native
        // crash flag — this heuristic is the only accident detector).
        // "Not enough data" and "genuinely no gear_force dataref at
        // all" (legacy JSONLs pre-v0.7.0, or an addon that never sets
        // it) are the same kind of unknown, so both now fall through to
        // the SAME 4-of-4 voting fallback below instead of discarding
        // the candidate outright.
        const MIN_GEAR_FORCE_SAMPLES_IN_WINDOW: usize = 2;
        let any_gear_force_data = samples.iter().any(|s| s.gear_normal_force_n.is_some());
        let gear_force_data_sufficient =
            any_gear_force_data && gear_force_sample_count >= MIN_GEAR_FORCE_SAMPLES_IN_WINDOW;

        if gear_force_data_sufficient {
            // Echte X-Plane-Validation mit gear_force als MUST-PASS
            if !gear_force_pass {
                return ValidationResult::FalseEdge {
                    reason: FalseEdgeReason::GearForceBelowThreshold,
                    result: detail,
                };
            }
            if !low_agl_pass || !vs_negative_pass {
                return ValidationResult::FalseEdge {
                    reason: FalseEdgeReason::GearForceBelowThreshold,
                    result: detail,
                };
            }
            return ValidationResult::Validated { result: detail };
        }
        // Fallback ohne (ausreichend) gear_force: 4-of-4 Voting (=
        // strenger als MSFS damit X-Plane edge-trigger-happy-
        // Streifschuesse nicht durch). Plus: agl_persistence ist hier
        // kritisch weil g_force-spike bei X-Plane ohne gear_force evtl
        // unzuverlaessig
        let passes = [g_force_pass, sustained_pass, low_agl_pass, vs_negative_pass]
            .iter()
            .filter(|p| **p)
            .count();
        if passes >= 4 {
            ValidationResult::Validated { result: detail }
        } else {
            ValidationResult::FalseEdge {
                reason: FalseEdgeReason::InsufficientVoteScore,
                result: detail,
            }
        }
    } else {
        // MSFS / Off.
        //
        // v1.7.23 (echter Flug GSG1249, EDDW→EDHE, 2026-09-09): eine sehr
        // sanfte, echte Landung (gemeldete Sinkrate -12/-13 fpm, G-Kraft-
        // Spitze exakt an der 1.05-Kippgrenze) hatte dadurch NUR 2 von 4
        // Stimmen (g_force UND vs_negative fielen knapp durch) — obwohl
        // `low_agl_persistence` und `sustained_ground_contact`, die beiden
        // Boden-WAHRHEITS-Signale, beide klar bestanden haetten. Die Landung
        // wurde nie als Touchdown-Ereignis erkannt, obwohl ein voller Score
        // dafuer berechnet wurde — der PIREP hing im Integrity Gate fest
        // (`no_touchdown_recorded`).
        //
        // Ein einfaches Hochsetzen der g_force-/vs_negative-Schwellen waere
        // ein Pflaster: es verschiebt nur, WELCHE sanfte Landung als naechstes
        // knapp durchfaellt. Stattdessen bekommt die Boden-Wahrheit einen
        // EIGENEN, vorrangigen Bestaetigungsweg — kein gleichgewichtiges
        // Zaehlen mehr, sondern ein benannter Beweisweg:
        //
        //   1. "Settle": haelt `low_agl_persistence` (< 5ft fuer >= 1000ms)
        //      UND `sustained_ground_contact` (>= 500ms) BEIDE — ein langer,
        //      eindeutiger Bodenkontakt braucht keine physikalische
        //      Bestaetigung mehr durch g_force/vs, egal wie schwach die
        //      ausfielen. Ein Bounce/Streifschuss/Taxi-Ruckler kann diese
        //      Kombination aus 1000ms+500ms KONTINUIERLICHEM Bodenkontakt
        //      praktisch nicht vortaeuschen (dieselbe Garantie, die die
        //      Kategorie-Sonderbehandlung fuer Helikopter/Wasserflugzeuge
        //      oben schon nutzt — hier auf Festflaechenflugzeuge uebertragen).
        //   2. Faellt "Settle" nicht (z.B. PTO 705: echter erster Bodenkontakt
        //      nur 307ms, unter der 500ms-sustained-Schwelle — ein Touch-and-
        //      Go-Muster, kein Fehler), bleibt die BISHERIGE 3-von-4-Abstimmung
        //      als Auffangnetz erhalten — byte-identische Entscheidungsgrenze
        //      zu vorher. PTO 705 bleibt dadurch weiterhin als echter
        //      Touchdown erkannt (g_force/low_agl/vs_negative bestehen dort).
        //
        // Vorher: "zu wenig Daten" (leeres Post-Edge-Fenster) und "Boden-
        // Wahrheit hat wirklich nicht bestanden" wurden beide zu genau
        // derselben Stimme "false" — nicht mehr unterscheidbar von einem
        // Test, der die Frage gar nicht beantworten konnte. Ein eigener
        // Dichte-Wächter (analog zum bestehenden
        // MIN_GEAR_FORCE_SAMPLES_IN_WINDOW-Muster bei X-Plane oben) faengt
        // das jetzt VOR jeder Settle-/Vote-Entscheidung ab: zu duenne
        // Telemetrie in BEIDEN Boden-Wahrheits-Fenstern fuehrt zu einem
        // eigenen, ehrlich benannten Grund (`InsufficientTelemetry`) statt
        // stillschweigend als gescheiterte Abstimmung durchzulaufen.
        const MIN_GROUND_TRUTH_SAMPLES: usize = 2;
        let low_agl_window_end = edge_at + Duration::milliseconds(1000);
        let low_agl_sample_count = samples
            .iter()
            .filter(|s| s.at >= edge_at && s.at <= low_agl_window_end)
            .count();
        let sustained_ground_sample_count = samples.iter().filter(|s| s.at >= edge_at).count();
        if low_agl_sample_count < MIN_GROUND_TRUTH_SAMPLES
            && sustained_ground_sample_count < MIN_GROUND_TRUTH_SAMPLES
        {
            return ValidationResult::FalseEdge {
                reason: FalseEdgeReason::InsufficientTelemetry,
                result: detail,
            };
        }

        // Settle-Pfad braucht zusaetzlich EIN minimales Anzeichen eines
        // echten Aufsetzens — sonst wuerde er einen bestehenden Schutz
        // durchbrechen: `heli_soft_setdown_rejected_as_fixed_wing_but_
        // validated_as_heli` (unten in den Tests) haelt bewusst fest, dass
        // ein Festfluegler mit einem VOELLIG kraftlosen (g_force==1.0,
        // also gar keine messbare Kraftaenderung) und nicht wirklich
        // sinkenden (vs=-3.0, weit ueber der -10-Schwelle) langen Boden-
        // kontakt WEITERHIN abgelehnt werden muss — das Muster ist exakt
        // das eines Hubschrauber-/Wasserflugzeug-Aufsetzers (dafuer gibt
        // es die eigene Kategorie-Behandlung oben), nicht das eines
        // Festfluegler-Touchdowns. GSG1249 hatte dagegen eine echte, wenn
        // auch schwache Kraft-Spitze (gemeldet 1.05 G — keine Null-Aenderung,
        // sondern ein echter, nur knapp unter der strikten Schwelle
        // liegender Aufprall). Der Dead-Band (>1.02, statt exakt >1.0)
        // laesst echtes Sensor-Rauschen um 1.0G nicht durchrutschen, faengt
        // aber jeden Aufprall auf, der ueberhaupt eine spuerbare
        // Gewichtsuebertragung zeigt.
        const MIN_SETTLE_G_FORCE_PEAK: f32 = 1.02;
        //
        // Codex-QS-Fund (Vereinheitlichung 09/2026): `evaluate_low_agl_persistence`
        // und `evaluate_sustained_ground` melden PASS schon, wenn im (ggf.
        // duennen) Fenster KEINE Verletzung/kein Abbruch beobachtet wurde —
        // das gilt absichtlich auch bei nur 2-3 fruehen Samples, die den
        // Rest des Fensters gar nicht abdecken (siehe deren eigene Tests
        // `low_agl_persistence_still_passes_with_real_no_violation_data`,
        // die genau das als GEWOLLTES Verhalten festhalten — dort korrekt,
        // weil dieses Signal nur EINE von vier Stimmen in der alten
        // Abstimmung war). Der Settle-Pfad ERSETZT die Abstimmung komplett
        // und braucht deshalb ECHTE, durchgehende Abdeckung nahe der vollen
        // Fensterlaenge, sonst koennte ein kurzer Streifschuss (DAH3181-
        // Muster) gefolgt von einer Telemetrie-Luecke faelschlich bestaetigt
        // werden. Eigene, strengere Abdeckungs-Pruefung hier — die beiden
        // `evaluate_*`-Funktionen bleiben fuer das Auffangnetz unveraendert.
        //
        // Codex-QS-Fund (Vereinheitlichung 09/2026, Abschlusspruefung): die
        // erste Fassung dieser Pruefung mass nur den Abstand des LETZTEN
        // Samples zum Fensterende — eine Luecke MITTEN im Fenster (z.B.
        // Samples bei 0/400/900ms: 400-900ms unbeobachtet, aber 900ms liegt
        // nah genug am 1000ms-Fensterende) waere durchgerutscht, obwohl die
        // "durchgehende Abdeckung", die dieser Pfad verlangt, genau das
        // ausschliessen soll. `deckt_fenster_durchgehend_ab` prueft deshalb
        // JEDEN Abstand — vom Rand-Zeitpunkt zum ersten Sample, zwischen
        // allen aufeinanderfolgenden Samples, und vom letzten Sample zum
        // Fensterende.
        const MAX_COVERAGE_GAP_MS: i64 = 200;
        let low_agl_covers_window =
            deckt_fenster_durchgehend_ab(samples, edge_at, low_agl_window_end, MAX_COVERAGE_GAP_MS);
        let sustained_window_end = edge_at + Duration::milliseconds(500);
        let sustained_covers_window = deckt_fenster_durchgehend_ab(
            samples,
            edge_at,
            sustained_window_end,
            MAX_COVERAGE_GAP_MS,
        );
        if low_agl_pass
            && sustained_pass
            && g_force_peak > MIN_SETTLE_G_FORCE_PEAK
            && low_agl_covers_window
            && sustained_covers_window
        {
            return ValidationResult::Validated { result: detail };
        }

        // Auffangnetz: bisherige 3-von-4-Abstimmung, unveraendert.
        let passes = [g_force_pass, sustained_pass, low_agl_pass, vs_negative_pass]
            .iter()
            .filter(|p| **p)
            .count();
        if passes >= 3 {
            ValidationResult::Validated { result: detail }
        } else {
            ValidationResult::FalseEdge {
                reason: FalseEdgeReason::InsufficientVoteScore,
                result: detail,
            }
        }
    }
}

/// gear_force-impact Evaluation (X-Plane).
/// Returns (pass, peak_in_window, sustained_ms_above_threshold, sample_count_in_window).
///
/// Confirmation-Window: Force ueber threshold fuer mind. 60ms anhaltend
/// (gemessen via Timestamps), mit mind. 2 distinct samples (Anti-Glitch).
///
/// `sample_count_in_window` counts ANY finite gear_normal_force_n value
/// in the window regardless of threshold — the caller uses this to
/// tell "the data says no impact force" apart from "there's barely any
/// data to judge from at all" (see `validate_candidate`'s X-Plane
/// branch).
fn evaluate_gear_force_test(
    samples: &[TouchdownWindowSample],
    edge_at: DateTime<Utc>,
    threshold_n: f32,
) -> (bool, Option<f32>, Option<u64>, usize) {
    let window_end = edge_at + Duration::milliseconds(500);

    // Sammle alle Samples im Window in Reihenfolge (sorted nach `at`).
    let in_window: Vec<&TouchdownWindowSample> = samples
        .iter()
        .filter(|s| s.at >= edge_at && s.at <= window_end)
        .collect();

    let sample_count_in_window = in_window
        .iter()
        .filter(|s| s.gear_normal_force_n.is_some_and(|f| f.is_finite()))
        .count();

    let peak_in_window = in_window
        .iter()
        .filter_map(|s| s.gear_normal_force_n)
        .filter(|f| f.is_finite())
        .fold(None::<f32>, |acc, f| {
            Some(acc.map(|a| a.max(f)).unwrap_or(f))
        });

    // P2-Fix: Suche den LAENGSTEN CONTINUOUS RUN von samples mit
    // gear_force >= threshold. Reset bei Gap (= sample mit force < threshold
    // ODER missing force value).
    //
    // Vorher (BUG): erste/letzte above-sample-Span - das counted Spans mit
    // Luecken in der Mitte als sustained, was die Spec widerspricht.
    let mut best_run_ms: u64 = 0;
    let mut best_run_count: usize = 0;
    let mut current_run_start: Option<DateTime<Utc>> = None;
    let mut current_run_count: usize = 0;

    for s in &in_window {
        let above = s
            .gear_normal_force_n
            .map(|f| f.is_finite() && f >= threshold_n)
            .unwrap_or(false);
        if above {
            if current_run_start.is_none() {
                current_run_start = Some(s.at);
                current_run_count = 1;
            } else {
                current_run_count += 1;
            }
            // Update best wenn dieser run laenger
            let run_ms = (s.at - current_run_start.unwrap())
                .num_milliseconds()
                .max(0) as u64;
            if run_ms > best_run_ms || (run_ms == best_run_ms && current_run_count > best_run_count)
            {
                best_run_ms = run_ms;
                best_run_count = current_run_count;
            }
        } else {
            // Gap → reset current run
            current_run_start = None;
            current_run_count = 0;
        }
    }

    let pass = best_run_ms >= 60 && best_run_count >= 2;
    (
        pass,
        peak_in_window,
        Some(best_run_ms),
        sample_count_in_window,
    )
}

/// peak g_force im Window [edge_at, edge_at + 500ms]
fn evaluate_g_force_peak(samples: &[TouchdownWindowSample], edge_at: DateTime<Utc>) -> f32 {
    let window_end = edge_at + Duration::milliseconds(500);
    samples
        .iter()
        .filter(|s| s.at >= edge_at && s.at <= window_end)
        .map(|s| s.g_force)
        .filter(|g| g.is_finite())
        .fold(0.0_f32, f32::max)
}

/// low_agl_persistence: agl_ft < 5.0 fuer mind. 1000ms ab edge_at.
/// Returns (pass, actual_ms_below_5ft).
fn evaluate_low_agl_persistence(
    samples: &[TouchdownWindowSample],
    edge_at: DateTime<Utc>,
    grenze_ft: f32,
) -> (bool, u64) {
    let target_dur = Duration::milliseconds(1000);
    let window_end = edge_at + target_dur;

    let in_window: Vec<&TouchdownWindowSample> = samples
        .iter()
        .filter(|s| s.at >= edge_at && s.at <= window_end)
        .collect();

    // v0.19.x FIX: zero samples in the window is NOT the same as "confirmed
    // no violation for the full 1000ms" — it's zero evidence either way.
    // The old code treated an empty window identically to "checked every
    // sample and found none above 5 ft", silently PASSing with a claimed
    // full-duration confirmation. A telemetry stall right at the touchdown
    // edge — exactly what a real hard-impact/CTD-adjacent crash tends to
    // cause — used to sail through this test for free.
    if in_window.is_empty() {
        return (false, 0);
    }

    // Suche erste violation (agl >= Grenze) im Target-Window
    let mut first_violation_at: Option<DateTime<Utc>> = None;
    for s in &in_window {
        if s.agl_ft >= grenze_ft {
            first_violation_at = Some(s.at);
            break;
        }
    }

    match first_violation_at {
        None => {
            // Keine violation im Window — PASS
            (true, target_dur.num_milliseconds() as u64)
        }
        Some(t) => {
            let actual_ms = (t - edge_at).num_milliseconds().max(0) as u64;
            (actual_ms >= 1000, actual_ms)
        }
    }
}

/// sustained_ground_contact: on_ground=True fuer mind. 500ms continuous.
/// Returns (pass, actual_continuous_ms).
fn evaluate_sustained_ground(
    samples: &[TouchdownWindowSample],
    edge_at: DateTime<Utc>,
) -> (bool, u64) {
    let in_window: Vec<&TouchdownWindowSample> =
        samples.iter().filter(|s| s.at >= edge_at).collect();

    // v0.19.x FIX: zero samples means zero evidence of sustained ground
    // contact, not a free pass. The old defaults (found_break=false,
    // last_at=edge_at) made "no break observed" and "confirmed grounded
    // for 0ms" indistinguishable, so an empty window passed with literally
    // no data behind it.
    if in_window.is_empty() {
        return (false, 0);
    }

    let mut last_at = edge_at;
    let mut found_break = false;

    for s in &in_window {
        if !s.on_ground {
            found_break = true;
            break;
        }
        last_at = s.at;
    }

    let dur_ms = (last_at - edge_at).num_milliseconds().max(0) as u64;
    let pass = !found_break || dur_ms >= 500;
    (pass, dur_ms)
}

// ─── Layer 3: VS-Calculation am IMPACT-Frame ──────────────────────────────

/// Drei Frames die wir aus dem Buffer extrahieren (siehe Spec 5.1):
///   contact_frame:      = candidate.edge_sample_index (von Layer 1)
///   impact_frame:       min vs in [contact-250ms, contact+100ms] = raw härteste Sink
///   initial_load_peak:  max gear_force/g_force in [contact, contact+500ms]
///   episode_load_peak:  max in ganzer Episode (kommt von Layer 4, nicht hier)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactFrameResult {
    pub contact_at: DateTime<Utc>,
    pub impact_at: DateTime<Utc>,
    pub impact_vs_fpm: f32,
    pub initial_load_peak_n: Option<f32>, // X-Plane
    pub initial_load_peak_g: f32,         // beide
}

/// Berechne impact_frame + initial_load_peak aus dem Buffer um den contact_frame.
/// NaN-safe: nur finite vs_fpm Samples + total_cmp().
pub fn compute_impact_frame(
    samples: &[TouchdownWindowSample],
    contact_at: DateTime<Utc>,
) -> Option<ImpactFrameResult> {
    // impact_frame = min vs in [contact-250ms, contact+100ms]
    let window_start = contact_at - Duration::milliseconds(250);
    let window_end = contact_at + Duration::milliseconds(100);

    let impact_sample = samples
        .iter()
        .filter(|s| s.at >= window_start && s.at <= window_end && s.vs_fpm.is_finite())
        .min_by(|a, b| a.vs_fpm.total_cmp(&b.vs_fpm))?;

    // initial_load_peak: max in [contact, contact+500ms]
    let load_window_end = contact_at + Duration::milliseconds(500);
    let load_window: Vec<&TouchdownWindowSample> = samples
        .iter()
        .filter(|s| s.at >= contact_at && s.at <= load_window_end)
        .collect();

    let initial_load_peak_n = load_window
        .iter()
        .filter_map(|s| s.gear_normal_force_n)
        .filter(|f| f.is_finite())
        .fold(None::<f32>, |acc, f| {
            Some(acc.map(|a| a.max(f)).unwrap_or(f))
        });

    let initial_load_peak_g = load_window
        .iter()
        .map(|s| s.g_force)
        .filter(|g| g.is_finite())
        .fold(0.0_f32, f32::max);

    Some(ImpactFrameResult {
        contact_at,
        impact_at: impact_sample.at,
        impact_vs_fpm: impact_sample.vs_fpm,
        initial_load_peak_n,
        initial_load_peak_g,
    })
}

// ─── Layer 3 Cont: VS-Cascade + HARD GUARDS ───────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Low,
    VeryLow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandingRateResult {
    pub vs_fpm: f32,
    pub source: String,
    pub confidence: Confidence,
    pub forensics_version: u8,
    pub contact_at: DateTime<Utc>,
    pub impact_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RejectionReason {
    EmptyWindow,
    AllSourcesPositive,
    PositiveVs,
    ImplausiblyHigh,
}

/// Untergrenze einer physikalisch moeglichen Landerate.
///
/// v0.20.2: exportiert, damit die Kanonik in `lib.rs` DIESELBE Grenze nutzt.
/// Sie stand vorher nur hier drin — und die Kanonik (die den ungeguardeten
/// Edge-Wert ausliefert) kannte sie nicht. Zwei Definitionen von "plausibel"
/// waeren genau der Riss, den wir gerade ausraeumen.
pub const VS_FLOOR_FPM: f32 = -3000.0;

/// HARD GUARD: niemals positiv, niemals unter `VS_FLOOR_FPM`
fn finalize_vs(candidate_fpm: f32) -> Result<f32, RejectionReason> {
    if !candidate_fpm.is_finite() {
        return Err(RejectionReason::EmptyWindow);
    }
    if candidate_fpm > 0.0 {
        return Err(RejectionReason::PositiveVs);
    }
    if candidate_fpm < VS_FLOOR_FPM {
        return Err(RejectionReason::ImplausiblyHigh);
    }
    Ok(candidate_fpm)
}

/// Compute the final landing rate using the sim-agnostic cascade.
/// Cascade priority: vs_at_impact → smoothed_500ms → smoothed_1000ms →
/// pre_flare_peak → REJECT.
pub fn compute_landing_rate(
    samples: &[TouchdownWindowSample],
    impact_result: &ImpactFrameResult,
    category: AircraftCategory,
) -> Result<LandingRateResult, RejectionReason> {
    let impact_at = impact_result.impact_at;
    let vs_at_impact = impact_result.impact_vs_fpm;

    // Smoothed averages around impact_frame
    let vs_smoothed_500 = avg_vs_in_window(samples, impact_at, -500, 0);
    let vs_smoothed_1000 = avg_vs_in_window(samples, impact_at, -1000, 0);
    let pre_flare_peak = min_vs_in_window(samples, impact_at, -3000, -500);

    // Helicopters / seaplanes touch down at a deliberately near-zero V/S
    // (collective cushion / glassy-water contact), so the -10 fpm fixed-wing
    // acceptance floor would reject a real soft set-down and yield no landing
    // rate. Use a near-zero floor for these categories; `finalize_vs` still
    // rejects any non-negative rate, so a level/climbing sample never produces
    // a landing. Fixed-wing keeps the -10 fpm floor unchanged.
    let floor = if category.is_non_conventional() {
        0.0
    } else {
        -10.0
    };

    let chosen = if vs_at_impact < floor {
        (vs_at_impact, "vs_at_impact_frame", Confidence::High)
    } else if vs_smoothed_500.map(|v| v < floor).unwrap_or(false) {
        (
            vs_smoothed_500.unwrap(),
            "vs_smoothed_500ms_at_impact",
            Confidence::Medium,
        )
    } else if vs_smoothed_1000.map(|v| v < floor).unwrap_or(false) {
        (
            vs_smoothed_1000.unwrap(),
            "vs_smoothed_1000ms_at_impact",
            Confidence::Low,
        )
    } else if pre_flare_peak.map(|v| v < 0.0).unwrap_or(false) {
        (
            pre_flare_peak.unwrap(),
            "pre_flare_peak",
            Confidence::VeryLow,
        )
    } else {
        return Err(RejectionReason::AllSourcesPositive);
    };

    let final_vs = finalize_vs(chosen.0)?;

    Ok(LandingRateResult {
        vs_fpm: final_vs,
        source: chosen.1.to_string(),
        confidence: chosen.2,
        forensics_version: FORENSICS_VERSION,
        contact_at: impact_result.contact_at,
        impact_at,
    })
}

// ─── Layer 4: LandingEpisode + Episode-Klassifizierung ────────────────────

/// Snapshot des Aircraft-Zustands zum Zeitpunkt eines TD-Contacts.
/// Wird einmal pro Episode beim ersten validated contact festgehalten —
/// wird fuer mass-aware threshold + deterministische Replays gebraucht.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AircraftStateSnapshot {
    pub total_weight_kg: Option<f32>,
    pub sim: SimKind,
}

/// Eine TD-Candidate die Validation gefailt hat (Float/Streifschuss).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FalseEdge {
    pub edge_at: DateTime<Utc>,
    pub edge_agl_ft: f32,
    pub edge_vs_fpm: f32,
    pub reason: FalseEdgeReason,
    pub validation: ValidationDetail,
}

/// Echter erster Bodenkontakt einer Episode (validated).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactDetail {
    pub contact_at: DateTime<Utc>,
    pub impact_at: DateTime<Utc>,
    pub vs_at_impact_fpm: f32,
    pub vs_at_contact_fpm: f32,
    pub agl_at_contact_ft: f32,
    pub validation: ValidationDetail,
    pub initial_load_peak_n: Option<f32>,
    pub initial_load_peak_g: f32,
    pub confidence: Confidence,
    pub source: String,
}

/// Ein nachfolgender low-level Touch innerhalb derselben Episode
/// (= aircraft bleibt unter 50ft AGL, kein climb-out).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LowLevelTouch {
    pub at: DateTime<Utc>,
    pub vs_at_impact_fpm: f32,
    pub agl_max_ft: f32,
    pub sustained_ms: u64,
}

/// Wann + wie eine Episode in den finalen Settle uebergeht.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettleDetail {
    pub settle_at: DateTime<Utc>,
    pub final_groundspeed_kt: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HardestImpactSource {
    Contact,
    LowLevelTouch(u8),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeClass {
    /// aircraft blieb am Boden, gs sinkt — Pilot ist gelandet
    FinalLanding,
    /// aircraft hob nach Touch wieder ab, stieg auf 100-1000ft AGL,
    /// kam zurueck — Pattern-Flug
    TouchAndGo,
    /// aircraft stieg > 1000ft AGL nach Touch — Go-Around
    GoAround,
    /// noch nicht klassifiziert (Episode laeuft noch)
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandingEpisode {
    pub episode_index: u8,
    pub aircraft_state_at_contact: AircraftStateSnapshot,
    pub false_edges: Vec<FalseEdge>,
    pub contact: ContactDetail,
    pub low_level_touches: Vec<LowLevelTouch>,
    pub settle: Option<SettleDetail>,
    /// max gear_force / g_force innerhalb der GANZEN Episode (incl. rollout) —
    /// Forensik-only, NICHT fuer Score
    pub episode_load_peak_n: Option<f32>,
    pub episode_load_peak_g: f32,
    pub hardest_impact_vs_fpm: f32,
    pub hardest_impact_source: HardestImpactSource,
    pub classification: EpisodeClass,
}

/// Kontext fuer Episode-Klassifizierung — Werte kommen aus der laufenden
/// Sampler-State-Machine (Layer 4 caller).
#[derive(Debug, Clone, Copy)]
pub struct EpisodePostContactState {
    /// Maximum AGL nach contact (in der ganzen post-contact Periode).
    pub max_agl_ft_after_contact: f32,
    /// Hat aircraft fuer >= 30s unter 50ft AGL geblieben mit gs<30kt?
    pub settled_under_50ft_for_30s: bool,
    /// Aktuelle groundspeed (fuer Settle-Detection)
    pub current_gs_kt: f32,
}

/// Klassifiziere eine Episode basierend auf was nach dem contact passiert ist.
/// Spec Sektion 6.2 (FinalLanding) + 6.4 (DAH 3181 Beispiel).
pub fn classify_episode(state: EpisodePostContactState) -> EpisodeClass {
    if state.max_agl_ft_after_contact > 1000.0 {
        EpisodeClass::GoAround
    } else if state.max_agl_ft_after_contact > 100.0 {
        EpisodeClass::TouchAndGo
    } else if state.settled_under_50ft_for_30s {
        EpisodeClass::FinalLanding
    } else {
        EpisodeClass::Pending
    }
}

/// Bestimme den haertesten Impact innerhalb einer Episode (= Bounce-Score-Regel).
/// Spec Sektion 6.5: härtester Impact = min vs_at_impact (= numerisch kleinster
/// = haertester Sink) zwischen contact + allen low_level_touches.
pub fn compute_hardest_impact(
    contact_vs: f32,
    low_level_touches: &[LowLevelTouch],
) -> (f32, HardestImpactSource) {
    let mut hardest = contact_vs;
    let mut source = HardestImpactSource::Contact;

    for (i, touch) in low_level_touches.iter().enumerate() {
        if touch.vs_at_impact_fpm < hardest {
            hardest = touch.vs_at_impact_fpm;
            source = HardestImpactSource::LowLevelTouch(i as u8);
        }
    }

    (hardest, source)
}

// ─── Helpers ──────────────────────────────────────────────────────────────

fn avg_vs_in_window(
    samples: &[TouchdownWindowSample],
    center: DateTime<Utc>,
    delta_start_ms: i64,
    delta_end_ms: i64,
) -> Option<f32> {
    let window_start = center + Duration::milliseconds(delta_start_ms);
    let window_end = center + Duration::milliseconds(delta_end_ms);

    let values: Vec<f32> = samples
        .iter()
        .filter(|s| s.at >= window_start && s.at <= window_end && s.vs_fpm.is_finite())
        .map(|s| s.vs_fpm)
        .collect();

    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f32>() / values.len() as f32)
    }
}

fn min_vs_in_window(
    samples: &[TouchdownWindowSample],
    center: DateTime<Utc>,
    delta_start_ms: i64,
    delta_end_ms: i64,
) -> Option<f32> {
    let window_start = center + Duration::milliseconds(delta_start_ms);
    let window_end = center + Duration::milliseconds(delta_end_ms);

    samples
        .iter()
        .filter(|s| s.at >= window_start && s.at <= window_end && s.vs_fpm.is_finite())
        .map(|s| s.vs_fpm)
        .min_by(|a, b| a.total_cmp(b))
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gear_force_threshold_floor_for_glider() {
        // Cessna 152 weight 757kg → dynamic = 757*9.80665*0.03 = 222.7N → floor wins
        let t = gear_force_threshold_n(Some(757.0));
        assert_eq!(t, 1000.0);
    }

    #[test]
    fn gear_force_threshold_dynamic_for_a330() {
        // DAH 3181 A330 250t → 250000*9.80665*0.03 = ~73550N
        let t = gear_force_threshold_n(Some(250000.0));
        assert!((t - 73549.875).abs() < 1.0, "expected ~73550, got {}", t);
    }

    #[test]
    fn gear_force_threshold_floor_for_no_weight() {
        let t = gear_force_threshold_n(None);
        assert_eq!(t, 1000.0);
    }

    #[test]
    fn gear_force_threshold_floor_for_zero_weight() {
        let t = gear_force_threshold_n(Some(0.0));
        assert_eq!(t, 1000.0);
    }

    #[test]
    fn finalize_vs_rejects_positive() {
        assert_eq!(finalize_vs(100.0), Err(RejectionReason::PositiveVs));
    }

    #[test]
    fn finalize_vs_rejects_implausibly_high() {
        assert_eq!(finalize_vs(-3500.0), Err(RejectionReason::ImplausiblyHigh));
    }

    #[test]
    fn finalize_vs_rejects_nan() {
        assert_eq!(finalize_vs(f32::NAN), Err(RejectionReason::EmptyWindow));
    }

    #[test]
    fn finalize_vs_accepts_typical_landing() {
        assert_eq!(finalize_vs(-150.0), Ok(-150.0));
    }

    // ── Category-aware validation + landing-rate (rotorcraft / seaplane) ──

    fn cat_sample(
        at: DateTime<Utc>,
        agl_ft: f32,
        on_ground: bool,
        vs_fpm: f32,
        g_force: f32,
    ) -> TouchdownWindowSample {
        TouchdownWindowSample {
            at,
            vs_fpm,
            g_force,
            on_ground,
            agl_ft,
            msl_ft: Some(agl_ft + 500.0),
            heading_true_deg: 0.0,
            groundspeed_kt: 0.0,
            indicated_airspeed_kt: 0.0,
            true_airspeed_kt: 0.0,
            lat: 0.0,
            lon: 0.0,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            gear_normal_force_n: None,
            total_weight_kg: Some(1100.0),
        }
    }

    /// 1200 ms of on-surface samples from `edge` (AGL≈1 ft, near-zero V/S, no
    /// G-spike) plus a matching candidate. `on_ground` toggles the wheeled
    /// (helicopter skids) vs water (seaplane) case.
    fn soft_setdown(
        edge: DateTime<Utc>,
        on_ground: bool,
    ) -> (Vec<TouchdownWindowSample>, TdCandidate) {
        let mut samples = Vec::new();
        let mut t = edge;
        let end = edge + Duration::milliseconds(1200);
        while t <= end {
            samples.push(cat_sample(t, 1.0, on_ground, -3.0, 1.0));
            t = t + Duration::milliseconds(20);
        }
        let cand = TdCandidate {
            edge_sample_index: 0,
            edge_at: edge,
            edge_agl_ft: 1.0,
            edge_vs_fpm: -3.0,
            edge_gear_force_n: None,
            edge_g_force: 1.0,
            edge_total_weight_kg: Some(1100.0),
        };
        (samples, cand)
    }

    #[test]
    fn heli_soft_setdown_rejected_as_fixed_wing_but_validated_as_heli() {
        // Diese Fixture ist bewusst extremer als GSG1249: g_force bleibt
        // die GESAMTEN 1200ms exakt bei 1.0 — also NULL messbare
        // Kraftaenderung, nicht bloss eine schwache. Fuer ein Festfluegel-
        // Flugzeug ist das kein Touchdown-Muster, sondern "schon am Boden,
        // keine neue Bodenberuehrung" (z.B. Rollen). Der v1.7.23-Settle-Pfad
        // (siehe validate_candidate, MSFS-Zweig) verlangt deshalb bewusst
        // ZUSAETZLICH g_force_peak > MIN_SETTLE_G_FORCE_PEAK (1.02) — eine
        // spuerbare, wenn auch schwache Gewichtsuebertragung wie bei
        // GSG1249 (1.05G). Bei exakt 1.0G bleibt es bei der alten
        // 3-von-4-Abstimmung, die hier durchfaellt (kein G-Spike, V/S milder
        // als -10fpm -> nur 2/4) — FixedWing bleibt also FalseEdge, exakt
        // wie vor dem Fix. Fuer Helikopter/Wasserflugzeuge gilt weiterhin
        // die eigene Kategorie-Praesenz-Regel (kein g_force-Erfordernis),
        // weil ein echter Heli-Aufsetzer laut FAA-Handbuch genau SO aussieht.
        let edge = Utc::now();
        let (samples, cand) = soft_setdown(edge, true);
        // Fixed-wing: no G-spike, V/S > -10 → only 2/4 votes AND der neue
        // Settle-Pfad greift nicht (g_force bleibt bei 1.0) → FalseEdge.
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -3.0,
                AircraftCategory::FixedWing,
                None
            ),
            ValidationResult::FalseEdge { .. }
        ));
        // Helicopter: sustained low-AGL + sustained ground = presence ⇒ Validated.
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -3.0,
                AircraftCategory::Helicopter,
                None
            ),
            ValidationResult::Validated { .. }
        ));
    }

    #[test]
    fn seaplane_water_contact_validates_without_on_ground() {
        let edge = Utc::now();
        // on_ground stays FALSE — the water case.
        let (samples, cand) = soft_setdown(edge, false);
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -120.0,
                AircraftCategory::FixedWing,
                None
            ),
            ValidationResult::FalseEdge { .. }
        ));
        // Seaplane: sustained low-AGL (= on the water) carries it without on_ground.
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -120.0,
                AircraftCategory::Seaplane,
                None
            ),
            ValidationResult::Validated { .. }
        ));
    }

    #[test]
    fn seaplane_level_low_pass_not_validated_phantom_guard() {
        // Phantom guard (review finding): a seaplane LINGERING in the water-
        // contact band (sustained low-AGL) WITHOUT a genuine descent — a level
        // low pass / step-taxi skim / glassy go-around — must NOT validate as a
        // water touchdown. With impact V/S ≈ 0 the descent gate rejects it even
        // though the low-AGL window is satisfied.
        let edge = Utc::now();
        let (samples, cand) = soft_setdown(edge, false); // on_ground=false (water)
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -5.0, // near-zero sink = not a landing
                AircraftCategory::Seaplane,
                None
            ),
            ValidationResult::FalseEdge { .. }
        ));
        // A genuine descending water touchdown (clear sink past the gate) DOES
        // validate — the gate distinguishes landing from non-landing low flight.
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -120.0,
                AircraftCategory::Seaplane,
                None
            ),
            ValidationResult::Validated { .. }
        ));
    }

    #[test]
    fn single_low_agl_glitch_tick_not_validated_for_heli() {
        // Phantom protection: one low-AGL tick at the edge, then back to cruise.
        let edge = Utc::now();
        let mut samples = vec![cat_sample(edge, 1.0, true, -3.0, 1.0)];
        let mut t = edge + Duration::milliseconds(20);
        let end = edge + Duration::milliseconds(1200);
        while t <= end {
            samples.push(cat_sample(t, 9000.0, false, 0.0, 1.0));
            t = t + Duration::milliseconds(20);
        }
        let cand = TdCandidate {
            edge_sample_index: 0,
            edge_at: edge,
            edge_agl_ft: 1.0,
            edge_vs_fpm: -3.0,
            edge_gear_force_n: None,
            edge_g_force: 1.0,
            edge_total_weight_kg: Some(1100.0),
        };
        // Not sustained ⇒ low-AGL fails ⇒ FalseEdge even for a helicopter.
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -3.0,
                AircraftCategory::Helicopter,
                None
            ),
            ValidationResult::FalseEdge { .. }
        ));
    }

    #[test]
    fn landing_rate_near_zero_rejected_fixed_wing_accepted_heli() {
        let base = Utc::now();
        let mut samples = Vec::new();
        // Pre-flare window [base-3000, base-500): hovering, V/S = 0.
        let mut t = base - Duration::milliseconds(3000);
        while t < base - Duration::milliseconds(500) {
            samples.push(cat_sample(t, 2.0, false, 0.0, 1.0));
            t = t + Duration::milliseconds(50);
        }
        // Around impact: a gentle -5 fpm settle.
        let mut t = base - Duration::milliseconds(400);
        let end = base + Duration::milliseconds(100);
        while t <= end {
            samples.push(cat_sample(t, 0.5, true, -5.0, 1.0));
            t = t + Duration::milliseconds(20);
        }
        let impact = ImpactFrameResult {
            contact_at: base,
            impact_at: base,
            impact_vs_fpm: -5.0,
            initial_load_peak_n: None,
            initial_load_peak_g: 1.0,
        };
        // Fixed-wing: -5 fpm is above the -10 floor at every tier and the
        // pre-flare window has no sink → rejected.
        assert!(matches!(
            compute_landing_rate(&samples, &impact, AircraftCategory::FixedWing),
            Err(RejectionReason::AllSourcesPositive)
        ));
        // Helicopter: near-zero floor accepts the real -5 fpm impact.
        let heli = compute_landing_rate(&samples, &impact, AircraftCategory::Helicopter)
            .expect("helicopter near-zero landing accepted");
        assert!((heli.vs_fpm - (-5.0)).abs() < 0.01);
    }

    fn touch(vs: f32) -> LowLevelTouch {
        LowLevelTouch {
            at: Utc::now(),
            vs_at_impact_fpm: vs,
            agl_max_ft: 3.0,
            sustained_ms: 200,
        }
    }

    #[test]
    fn hardest_impact_no_bounces_returns_contact() {
        let (vs, src) = compute_hardest_impact(-300.0, &[]);
        assert_eq!(vs, -300.0);
        assert_eq!(src, HardestImpactSource::Contact);
    }

    #[test]
    fn hardest_impact_contact_harder_than_bounce() {
        // PTO 705 Pattern: contact -182, low_level -61 → hardest = -182 (contact)
        let (vs, src) = compute_hardest_impact(-182.0, &[touch(-61.0)]);
        assert_eq!(vs, -182.0);
        assert_eq!(src, HardestImpactSource::Contact);
    }

    #[test]
    fn hardest_impact_bounce_harder_than_contact() {
        // Hard-Bounce-Pattern: contact -200, bounce -600 → hardest = -600 (bounce)
        let (vs, src) = compute_hardest_impact(-200.0, &[touch(-100.0), touch(-600.0)]);
        assert_eq!(vs, -600.0);
        assert_eq!(src, HardestImpactSource::LowLevelTouch(1));
    }

    #[test]
    fn classify_final_landing() {
        let s = EpisodePostContactState {
            max_agl_ft_after_contact: 30.0,
            settled_under_50ft_for_30s: true,
            current_gs_kt: 15.0,
        };
        assert_eq!(classify_episode(s), EpisodeClass::FinalLanding);
    }

    #[test]
    fn classify_touch_and_go_pattern() {
        let s = EpisodePostContactState {
            max_agl_ft_after_contact: 800.0,
            settled_under_50ft_for_30s: false,
            current_gs_kt: 90.0,
        };
        assert_eq!(classify_episode(s), EpisodeClass::TouchAndGo);
    }

    #[test]
    fn classify_go_around() {
        let s = EpisodePostContactState {
            max_agl_ft_after_contact: 1500.0,
            settled_under_50ft_for_30s: false,
            current_gs_kt: 130.0,
        };
        assert_eq!(classify_episode(s), EpisodeClass::GoAround);
    }

    fn make_sample(at_ms: i64, gear_n: Option<f32>) -> TouchdownWindowSample {
        let at = DateTime::<Utc>::from_timestamp_millis(at_ms).unwrap();
        TouchdownWindowSample {
            at,
            vs_fpm: -200.0,
            g_force: 1.2,
            on_ground: true,
            agl_ft: 1.0,
            msl_ft: Some(1.0 + 500.0),
            heading_true_deg: 0.0,
            groundspeed_kt: 100.0,
            indicated_airspeed_kt: 100.0,
            true_airspeed_kt: 100.0,
            lat: 0.0,
            lon: 0.0,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            gear_normal_force_n: gear_n,
            total_weight_kg: Some(73000.0), // A320-ish → threshold ≈ 21478 N
        }
    }

    #[test]
    fn gear_force_continuous_pass_when_sustained() {
        // 5 samples a 20ms (= 80ms span), alle ueber threshold (50000 N > 21478)
        let samples: Vec<TouchdownWindowSample> = (0..5)
            .map(|i| make_sample(1000 + i * 20, Some(50000.0)))
            .collect();
        let edge_at = samples[0].at;
        let (pass, peak, sustained, count) = evaluate_gear_force_test(&samples, edge_at, 21478.0);
        assert!(pass, "5 consecutive samples should pass");
        assert_eq!(peak, Some(50000.0));
        assert!(sustained.unwrap() >= 60);
        assert_eq!(count, 5);
    }

    #[test]
    fn gear_force_continuous_fail_with_gap_in_middle() {
        // P2-Fix: Sample 0 above, 1 below, 2 above → run-laenge = 1 sample.
        // Vorher (BUG) waere span 0→2 = 40ms = pass. Jetzt: korrekt fail.
        let samples = vec![
            make_sample(1000, Some(50000.0)), // above
            make_sample(1020, Some(100.0)),   // below threshold (gap!)
            make_sample(1040, Some(50000.0)), // above wieder
        ];
        let edge_at = samples[0].at;
        let (pass, _peak, sustained, _count) = evaluate_gear_force_test(&samples, edge_at, 21478.0);
        // Best run hat nur 1 sample bzw 0ms span -> fail
        assert!(
            !pass,
            "gap in middle must NOT count as sustained, got sustained={:?}",
            sustained
        );
    }

    #[test]
    fn gear_force_continuous_pass_when_long_run_after_gap() {
        // Sample 0 above (single), gap, dann 5 samples sustained → pass
        let mut samples = vec![
            make_sample(1000, Some(50000.0)), // single above
            make_sample(1020, Some(100.0)),   // gap
        ];
        for i in 0..5 {
            samples.push(make_sample(1100 + i * 20, Some(50000.0)));
        }
        let edge_at = samples[0].at;
        let (pass, _peak, _sustained, _count) =
            evaluate_gear_force_test(&samples, edge_at, 21478.0);
        assert!(pass, "long sustained run after gap should pass");
    }

    /// 60 samples (1180 ms) with the given `gear_ns` supplying the first N
    /// entries' `gear_normal_force_n` (the rest `None`) — enough span to
    /// satisfy sustained-ground (500 ms) and low-AGL persistence (1000 ms)
    /// on their own, so only the gear_force test's own data density varies.
    fn hard_landing_with_sparse_gear_force(
        gear_ns: &[Option<f32>],
    ) -> (Vec<TouchdownWindowSample>, TdCandidate) {
        let samples: Vec<TouchdownWindowSample> = (0..60)
            .map(|i| {
                let gear_n = gear_ns.get(i as usize).copied().unwrap_or(None);
                make_sample(1000 + i * 20, gear_n)
            })
            .collect();
        let edge_at = samples[0].at;
        let cand = TdCandidate {
            edge_sample_index: 0,
            edge_at,
            edge_agl_ft: 1.0,
            edge_vs_fpm: -1200.0,
            edge_gear_force_n: gear_ns.first().copied().flatten(),
            edge_g_force: 1.2,
            edge_total_weight_kg: Some(73000.0),
        };
        (samples, cand)
    }

    // v0.19.x QS (backlog "X-Plane gear-force MUST-PASS defeated by
    // <2-sample windows"): a data-sparse gear_force window used to be
    // treated exactly like "genuinely no impact force" — both forced
    // FalseEdge. Reproduces a genuine hard landing (g-spike, sustained
    // ground, sustained low-AGL, clear negative V/S — every OTHER vote
    // passes) whose window happens to carry only ONE gear_normal_force_n
    // sample (e.g. an FPS stall/telemetry reconnect right at touchdown).

    #[test]
    fn sparse_gear_force_window_falls_back_to_voting_instead_of_discarding_a_real_hard_landing() {
        let (samples, cand) = hard_landing_with_sparse_gear_force(&[Some(50000.0)]);
        match validate_candidate(
            &cand,
            &samples,
            SimKind::XPlane12,
            -1200.0,
            AircraftCategory::FixedWing,
            None,
        ) {
            ValidationResult::Validated { result } => {
                assert_eq!(
                    result.gear_force_sample_count_in_window, 1,
                    "only one gear_force sample was ever provided"
                );
            }
            ValidationResult::FalseEdge { reason, .. } => panic!(
                "a genuine hard landing must not be discarded just because gear_force \
                 data was sparse — got FalseEdge({reason:?})"
            ),
        }
    }

    #[test]
    fn zero_gear_force_samples_in_window_also_falls_back_to_voting() {
        // The gear_force dataref exists elsewhere in the buffer (checked via
        // `any_gear_force_data`) but happens to carry nothing in THIS
        // specific window — same "not enough data" class as one sample.
        let (mut samples, cand) = hard_landing_with_sparse_gear_force(&[]);
        // Give the buffer a gear_force reading far outside the 500ms window
        // so `any_gear_force_data` is true but the window itself is empty.
        samples.push(make_sample(1000 + 800 * 20, Some(50000.0)));
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::XPlane12,
                -1200.0,
                AircraftCategory::FixedWing,
                None
            ),
            ValidationResult::Validated { .. }
        ));
    }

    #[test]
    fn sparse_gear_force_window_still_rejects_a_genuine_non_landing_via_voting() {
        // The fallback is 4-of-4 voting, same strictness as the "no
        // gear_force dataref at all" path — a sparse window must NOT
        // become an easier bar to clear than having no data whatsoever.
        // Only 2 of the other 3 votes pass here (no sustained ground —
        // on_ground toggles off), so even with a sparse gear_force window
        // this must still FalseEdge.
        let mut samples: Vec<TouchdownWindowSample> = (0..60)
            .map(|i| {
                let mut s = make_sample(1000 + i * 20, if i == 0 { Some(50000.0) } else { None });
                s.on_ground = i < 5; // breaks sustained-ground almost immediately
                s
            })
            .collect();
        samples[0].g_force = 0.9; // no G-spike either
        let edge_at = samples[0].at;
        let cand = TdCandidate {
            edge_sample_index: 0,
            edge_at,
            edge_agl_ft: 1.0,
            edge_vs_fpm: -1200.0,
            edge_gear_force_n: Some(50000.0),
            edge_g_force: 0.9,
            edge_total_weight_kg: Some(73000.0),
        };
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::XPlane12,
                -1200.0,
                AircraftCategory::FixedWing,
                None
            ),
            ValidationResult::FalseEdge { .. }
        ));
    }

    #[test]
    fn gear_force_sample_count_in_window_reflects_raw_data_density_not_threshold() {
        // Below-threshold samples still count toward the density figure —
        // it measures "how much data do we have", not "how much passed".
        let samples = vec![
            make_sample(1000, Some(100.0)), // below threshold
            make_sample(1020, Some(200.0)), // below threshold
        ];
        let edge_at = samples[0].at;
        let (pass, _peak, _sustained, count) = evaluate_gear_force_test(&samples, edge_at, 21478.0);
        assert!(!pass, "both samples are below threshold");
        assert_eq!(count, 2, "both are still real, finite data points");
    }

    #[test]
    fn classify_pending_low_agl_not_settled_yet() {
        let s = EpisodePostContactState {
            max_agl_ft_after_contact: 20.0,
            settled_under_50ft_for_30s: false,
            current_gs_kt: 80.0,
        };
        assert_eq!(classify_episode(s), EpisodeClass::Pending);
    }

    // v0.19.x FIX: an empty sample window (a telemetry stall right at the
    // touchdown edge) must FAIL, not silently pass with a claimed
    // full-duration confirmation built from zero evidence.

    #[test]
    fn low_agl_persistence_fails_on_a_totally_empty_window() {
        let edge_at = DateTime::<Utc>::from_timestamp_millis(1000).unwrap();
        let (pass, ms) = evaluate_low_agl_persistence(&[], edge_at, TIEFFLUG_SPIELRAUM_FT);
        assert!(
            !pass,
            "zero samples must not confirm 1000ms of low-AGL persistence"
        );
        assert_eq!(ms, 0);
    }

    #[test]
    fn low_agl_persistence_still_passes_with_real_no_violation_data() {
        // Regression guard: the fix must not turn a GENUINE pass (samples
        // present, none of them above 5 ft) into a failure.
        let samples: Vec<TouchdownWindowSample> =
            (0..5).map(|i| make_sample(1000 + i * 100, None)).collect();
        let edge_at = samples[0].at;
        let (pass, ms) = evaluate_low_agl_persistence(&samples, edge_at, TIEFFLUG_SPIELRAUM_FT);
        assert!(pass, "real samples with no violation must still pass");
        assert_eq!(ms, 1000);
    }

    #[test]
    fn sustained_ground_fails_on_a_totally_empty_window() {
        let edge_at = DateTime::<Utc>::from_timestamp_millis(1000).unwrap();
        let (pass, ms) = evaluate_sustained_ground(&[], edge_at);
        assert!(
            !pass,
            "zero samples must not confirm 500ms of sustained ground contact"
        );
        assert_eq!(ms, 0);
    }

    #[test]
    fn sustained_ground_still_passes_with_real_data() {
        // Regression guard: real on_ground=true samples spanning >= 500ms
        // must still pass — the fix only changes the EMPTY-window case.
        let samples: Vec<TouchdownWindowSample> =
            (0..6).map(|i| make_sample(1000 + i * 100, None)).collect();
        let edge_at = samples[0].at;
        let (pass, ms) = evaluate_sustained_ground(&samples, edge_at);
        assert!(pass, "500ms of real on_ground samples must pass");
        assert!(ms >= 500);
    }

    // ── v1.7.23 "Settle path" fix — Referenzfaelle ─────────────────────────
    //
    // GSG1249 (EDDW→EDHE, 2026-09-09): der reale Flug, der den Fehler
    // aufgedeckt hat. PTO 705 und DAH 3181 sind die historischen
    // Referenzfaelle aus docs/spec/historical/touchdown-forensics-v2.md,
    // die durch diesen Fix NICHT regressieren duerfen.

    /// MSFS-Sample fuer die Settle-Path-Tests: kein gear_normal_force_n
    /// (MSFS liefert das nicht), sonst frei parametrisierbar.
    fn msfs_sample(
        at_ms: i64,
        agl_ft: f32,
        on_ground: bool,
        vs_fpm: f32,
        g_force: f32,
    ) -> TouchdownWindowSample {
        TouchdownWindowSample {
            at: DateTime::<Utc>::from_timestamp_millis(at_ms).unwrap(),
            vs_fpm,
            g_force,
            on_ground,
            agl_ft,
            msl_ft: Some(agl_ft + 500.0),
            heading_true_deg: 0.0,
            groundspeed_kt: 60.0,
            indicated_airspeed_kt: 55.0,
            true_airspeed_kt: 55.0,
            lat: 0.0,
            lon: 0.0,
            pitch_deg: 3.0,
            bank_deg: 0.0,
            gear_normal_force_n: None,
            total_weight_kg: Some(1250.0), // GSG1249-Groessenordnung (leichtes GA-Muster)
        }
    }

    fn msfs_candidate(
        edge_at_ms: i64,
        edge_agl_ft: f32,
        edge_vs_fpm: f32,
        edge_g_force: f32,
    ) -> TdCandidate {
        TdCandidate {
            edge_sample_index: 0,
            edge_at: DateTime::<Utc>::from_timestamp_millis(edge_at_ms).unwrap(),
            edge_agl_ft,
            edge_vs_fpm,
            edge_gear_force_n: None,
            edge_g_force,
            edge_total_weight_kg: Some(1250.0),
        }
    }

    #[test]
    fn gsg1249_soft_real_landing_now_confirmed_via_settle_path() {
        // Aufsetz-G-Kraft an der Kippgrenze (nicht > 1.05 -> g_force_pass
        // FAELLT, aber > 1.02 -> spuerbare, echte Kraftaenderung, kein
        // Null-Rauschen), Aufsetz-Sinkrate milder als die -10fpm-
        // Testschwelle (vs_negative_pass FAELLT) — nur 2 von 4 Stimmen.
        // Vorher: FalseEdge. Boden-Wahrheit (durchgehend on_ground=true,
        // agl<5ft, 1200ms lang) ist aber eindeutig -> muss jetzt ueber den
        // Settle-Pfad bestaetigt werden.
        let edge = 0_i64;
        let mut samples = Vec::new();
        let mut t = edge;
        while t <= 1200 {
            samples.push(msfs_sample(t, 1.5, true, -8.0, 1.04));
            t += 20;
        }
        let cand = msfs_candidate(edge, 1.5, -8.0, 1.04);
        let result = validate_candidate(
            &cand,
            &samples,
            SimKind::Msfs2024,
            -8.0, // impact_frame_vs: milder als -10 -> vs_negative_pass FALSE
            AircraftCategory::FixedWing,
            None,
        );
        match result {
            ValidationResult::Validated { result } => {
                assert!(
                    !result.g_force_pass.unwrap(),
                    "g_force sollte hier knapp durchfallen"
                );
                assert!(
                    !result.vs_negative_pass,
                    "vs_negative sollte hier knapp durchfallen"
                );
                assert!(result.low_agl_persistence_pass);
                assert!(result.sustained_ground_pass.unwrap());
            }
            ValidationResult::FalseEdge { reason, .. } => panic!(
                "eine echte, sanfte Landung mit eindeutiger Boden-Wahrheit darf nicht \
                 verworfen werden — got FalseEdge({reason:?})"
            ),
        }
    }

    #[test]
    fn pto705_short_real_contact_still_confirmed_via_vote_fallback() {
        // PTO 705 (Spec §6.3): erster echter Bodenkontakt nur ~300ms, dann
        // bricht on_ground kurz ab (Touch-and-Go-Muster) — sustained_ground
        // FAELLT (< 500ms). low_agl bleibt aber die vollen 1000ms unten,
        // g_force-Spitze UND vs_negative bestehen deutlich (harter Touch).
        // Settle-Pfad greift NICHT (sustained fehlt) — muss weiterhin ueber
        // die alte 3-von-4-Abstimmung bestaetigt werden. Keine Regression.
        let edge = 0_i64;
        let mut samples = Vec::new();
        let mut t = edge;
        while t <= 1200 {
            let on_ground = t <= 300; // Kontakt bricht nach 300ms ab
            samples.push(msfs_sample(t, 1.5, on_ground, -20.0, 1.25));
            t += 20;
        }
        let cand = msfs_candidate(edge, 1.5, -182.0, 1.25);
        let result = validate_candidate(
            &cand,
            &samples,
            SimKind::Msfs2024,
            -182.0, // klarer harter Sink, weit unter -10fpm
            AircraftCategory::FixedWing,
            None,
        );
        match result {
            ValidationResult::Validated { result } => {
                assert!(
                    !result.sustained_ground_pass.unwrap(),
                    "sustained sollte hier durchfallen (307ms-Muster)"
                );
                assert!(result.g_force_pass.unwrap());
                assert!(result.vs_negative_pass);
                assert!(result.low_agl_persistence_pass);
            }
            ValidationResult::FalseEdge { reason, .. } => panic!(
                "PTO 705 (kurzer aber echter Bodenkontakt) darf durch diesen Fix nicht \
                 regressieren — got FalseEdge({reason:?})"
            ),
        }
    }

    #[test]
    fn dah3181_float_skim_stays_rejected() {
        // DAH 3181 (Spec §6.4): 44ms-Float-Streifschuss, danach wieder
        // abgehoben (steigt), keine G-Kraft-Spitze, positive Sinkrate
        // (steigt statt sinkt). Weder Settle- noch Vote-Pfad duerfen das
        // als Touchdown durchlassen.
        let edge = 0_i64;
        let mut samples = Vec::new();
        // Kurzer Bodenkontakt (44ms), dann steigt es weg.
        samples.push(msfs_sample(0, 1.0, true, -1.0, 1.0));
        samples.push(msfs_sample(20, 1.0, true, -1.0, 1.0));
        samples.push(msfs_sample(44, 1.0, true, -1.0, 1.0));
        let mut t = 60_i64;
        while t <= 1200 {
            samples.push(msfs_sample(t, 30.0, false, 104.0, 1.0)); // steigt weg
            t += 20;
        }
        let cand = msfs_candidate(edge, 1.0, -1.0, 1.0);
        let result = validate_candidate(
            &cand,
            &samples,
            SimKind::Msfs2024,
            104.0, // positiv = steigt, kein Sinken
            AircraftCategory::FixedWing,
            None,
        );
        assert!(
            matches!(result, ValidationResult::FalseEdge { .. }),
            "ein Float-Streifschuss mit positiver Sinkrate darf NIE als Touchdown gelten"
        );
    }

    #[test]
    fn telemetry_gap_at_candidate_instant_is_labelled_insufficient_not_voted_down() {
        // Eine Telemetrie-Luecke genau am Kandidaten-Zeitpunkt (z.B. FPS-
        // Stall/Reconnect) darf nicht als "Boden-Wahrheit hat wirklich nicht
        // bestanden" durchlaufen, sondern muss als eigener, ehrlicher Grund
        // erkennbar sein.
        let edge = 0_i64;
        let samples = vec![msfs_sample(edge, 1.0, true, -20.0, 1.2)]; // nur 1 Sample
        let cand = msfs_candidate(edge, 1.0, -20.0, 1.2);
        let result = validate_candidate(
            &cand,
            &samples,
            SimKind::Msfs2024,
            -20.0,
            AircraftCategory::FixedWing,
            None,
        );
        match result {
            ValidationResult::FalseEdge { reason, .. } => {
                assert!(
                    matches!(reason, FalseEdgeReason::InsufficientTelemetry),
                    "ein duennes Fenster muss als InsufficientTelemetry erkennbar sein, nicht als InsufficientVoteScore, got {reason:?}"
                );
            }
            ValidationResult::Validated { .. } => {
                panic!("ein einzelnes Sample darf keine Boden-Wahrheit bestaetigen")
            }
        }
    }

    #[test]
    fn sparse_early_samples_with_gap_do_not_confirm_via_settle_path() {
        // Codex-QS-Fund (Vereinheitlichung 09/2026): 2 fruehe Samples
        // (0ms/20ms, beide on_ground/low-AGL) erfuellen technisch
        // MIN_GROUND_TRUTH_SAMPLES UND `evaluate_low_agl_persistence`/
        // `evaluate_sustained_ground`s "keine Verletzung gesehen"-PASS —
        // obwohl der Rest der behaupteten 1000ms/500ms-Fenster gar nicht
        // beobachtet wurde (z.B. ein DAH3181-artiger Streifschuss, gefolgt
        // von einer Telemetrie-Luecke, in der das Flugzeug tatsaechlich
        // wieder abgehoben haben koennte). Der Settle-Pfad ersetzt die
        // physikalische Abstimmung komplett und darf sich deshalb NICHT
        // auf so duenne Abdeckung verlassen.
        //
        // g_force/vs bewusst wie im GSG1249-Fall knapp UNTER den strikten
        // Schwellen (1.04 statt >1.05, -8.0 statt <-10.0) gewaehlt, damit
        // NUR der Settle-Pfad ueberhaupt in Frage kommt (g_force_pass und
        // vs_negative_pass bleiben false -> die alte 3-von-4-Abstimmung
        // kommt mangels dritter Stimme gar nicht erst auf 3 und ist hier
        // NICHT die Fehlerquelle, die dieser Test prueft).
        let edge = 0_i64;
        let samples = vec![
            msfs_sample(0, 1.0, true, -8.0, 1.04),
            msfs_sample(20, 1.0, true, -8.0, 1.04),
            // Danach: Telemetrie-Luecke bis 1200ms — absichtlich KEINE
            // weiteren Samples, um die duenne Abdeckung zu simulieren.
        ];
        let cand = msfs_candidate(edge, 1.0, -8.0, 1.04);
        let result = validate_candidate(
            &cand,
            &samples,
            SimKind::Msfs2024,
            -8.0,
            AircraftCategory::FixedWing,
            None,
        );
        assert!(
            matches!(result, ValidationResult::FalseEdge { .. }),
            "2 fruehe Samples mit Luecke danach duerfen den Settle-Pfad nicht ausloesen, got {result:?}"
        );
    }

    #[test]
    fn internal_gap_between_samples_does_not_confirm_via_settle_path() {
        // Codex-QS-Fund (Vereinheitlichung 09/2026, Abschlusspruefung): die
        // vorige Fassung der Abdeckungs-Pruefung mass nur den Abstand des
        // LETZTEN Samples zum Fensterende. Samples bei 0/400/900ms in einem
        // 1000ms-Fenster haetten das bestanden (900ms liegt nur 100ms vom
        // Ende entfernt, unter MAX_COVERAGE_GAP_MS=200) — obwohl zwischen
        // 400ms und 900ms 500ms lang GAR NICHTS beobachtet wurde. Anders als
        // der Test oben (Luecke NACH den Samples) ist das hier eine Luecke
        // MITTEN im Fenster, mit Wiederaufnahme danach — genau der Fall, den
        // `deckt_fenster_durchgehend_ab` jetzt zusaetzlich abfaengt.
        let edge = 0_i64;
        let samples = vec![
            msfs_sample(0, 1.0, true, -8.0, 1.04),
            msfs_sample(400, 1.0, true, -8.0, 1.04),
            // Luecke 400ms -> 900ms: 500ms lang keine Telemetrie.
            msfs_sample(900, 1.0, true, -8.0, 1.04),
        ];
        let cand = msfs_candidate(edge, 1.0, -8.0, 1.04);
        let result = validate_candidate(
            &cand,
            &samples,
            SimKind::Msfs2024,
            -8.0,
            AircraftCategory::FixedWing,
            None,
        );
        assert!(
            matches!(result, ValidationResult::FalseEdge { .. }),
            "eine 500ms-Luecke mitten im Fenster darf den Settle-Pfad nicht \
             ausloesen, auch wenn das letzte Sample nah am Fensterende liegt, \
             got {result:?}"
        );
    }

    #[test]
    fn threshold_boundary_g_force_exactly_1_05_fails_strict_test_but_settle_path_still_confirms() {
        // g_force_peak == 1.05 (nicht > 1.05) faellt bewusst durch den
        // strikten Test — das ist unveraendert. Bei eindeutiger Boden-
        // Wahrheit rettet der Settle-Pfad die Landung trotzdem.
        let edge = 0_i64;
        let mut samples = Vec::new();
        let mut t = edge;
        while t <= 1200 {
            samples.push(msfs_sample(t, 1.0, true, -30.0, 1.05));
            t += 20;
        }
        let cand = msfs_candidate(edge, 1.0, -30.0, 1.05);
        let result = validate_candidate(
            &cand,
            &samples,
            SimKind::Msfs2024,
            -30.0,
            AircraftCategory::FixedWing,
            None,
        );
        match result {
            ValidationResult::Validated { result } => {
                assert!(
                    !result.g_force_pass.unwrap(),
                    "1.05 ist die Grenze, nicht darueber -> g_force_pass muss false bleiben"
                );
            }
            ValidationResult::FalseEdge { reason, .. } => {
                panic!("Boden-Wahrheit haette das retten muessen, got FalseEdge({reason:?})")
            }
        }
    }

    #[test]
    fn threshold_boundary_vs_exactly_minus_10_fails_strict_test() {
        // -10.0 fpm ist NICHT < -10.0 -> vs_negative_pass bleibt false.
        // Unveraenderte Grenze, nur hier explizit dokumentiert.
        let edge = 0_i64;
        let samples: Vec<TouchdownWindowSample> = (0..60)
            .map(|i| msfs_sample(edge + i * 20, 1.0, true, -10.0, 1.2))
            .collect();
        let cand = msfs_candidate(edge, 1.0, -10.0, 1.2);
        let result = validate_candidate(
            &cand,
            &samples,
            SimKind::Msfs2024,
            -10.0,
            AircraftCategory::FixedWing,
            None,
        );
        // Settle-Pfad greift hier ohnehin (low_agl+sustained bestehen ueber
        // die volle Laenge) -> Validated, aber vs_negative_pass selbst muss
        // false bleiben (die Grenze wurde nicht aufgeweicht).
        match result {
            ValidationResult::Validated { result } => {
                assert!(
                    !result.vs_negative_pass,
                    "-10.0 ist die Grenze, nicht darunter"
                );
            }
            ValidationResult::FalseEdge { reason, .. } => {
                panic!("Boden-Wahrheit haette das retten muessen, got FalseEdge({reason:?})")
            }
        }
    }

    // ─── Bewertbarkeit: reicht die Aufzeichnung? ─────────────────────────
    //
    // Anlass CFG 2090 (EDDF→KPDX, A339, X-Plane 12, 12.09.2026): 97 Punkte
    // und Note A+ für eine Landung, deren Aufsetzmoment nicht aufgezeichnet
    // wurde. Zwischen der letzten Probe in der Luft (6,4 ft) und dem ersten
    // Bodenkontakt lagen 0,92 s ohne jede Messung; im ganzen Fenster von
    // fünf Sekunden standen zehn Proben statt der üblichen 127.

    fn bp(at_ms: i64, agl_ft: f32, on_ground: bool, vs_fpm: f32) -> TouchdownWindowSample {
        use chrono::TimeZone;
        cat_sample(
            Utc.timestamp_opt(1_700_000_000, 0).unwrap() + chrono::Duration::milliseconds(at_ms),
            agl_ft,
            on_ground,
            vs_fpm,
            1.0,
        )
    }

    fn kontakt() -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    /// Sauber abgetastete Landung: alle 33 ms eine Probe, wie im Bestand üblich.
    fn dichte_proben() -> Vec<TouchdownWindowSample> {
        (-1200..=200)
            .step_by(33)
            .map(|ms| bp(ms, if ms < 0 { 5.0 } else { 0.5 }, ms >= 0, -150.0))
            .collect()
    }

    #[test]
    fn dichte_aufzeichnung_ist_bewertbar() {
        assert_eq!(pruefe_bewertbarkeit(&dichte_proben(), kontakt()), Ok(()));
    }

    #[test]
    fn die_luecke_im_aufsetzmoment_sperrt_die_bewertung() {
        // Die echte Probenfolge von CFG 2090, auf das Fenster bezogen:
        // −4,7 s / −3,5 s / −2,6 s / −1,6 s ... dann 0,92 s nichts, dann Boden.
        let proben = vec![
            bp(-2650, 11.2, false, -520.0),
            bp(-1480, 6.9, false, -458.0),
            bp(-920, 6.4, false, 26.0),
            bp(0, 0.7, true, -82.0),
            bp(83, 0.98, true, -10.4),
            bp(113, 0.98, true, -10.4),
        ];
        let fehlt = pruefe_bewertbarkeit(&proben, kontakt()).unwrap_err();
        assert!(
            fehlt.groesste_luecke_ms >= 900,
            "die 0,92-s-Lücke muss gefunden werden, gemessen: {} ms",
            fehlt.groesste_luecke_ms
        );
        assert!(fehlt.proben < MIN_BEWERTUNGS_PROBEN);
    }

    #[test]
    fn die_beiden_bedingungen_greifen_je_fuer_sich() {
        // Codex-Abnahme 12.09.2026: Der erste Grenztest prüfte 200 gegen
        // 201 ms — aber beide Reihen hatten weniger als zwölf Proben, also
        // entschied in Wahrheit die Probenzahl. Hier steht jede Bedingung
        // für sich, mit der jeweils anderen sicher erfüllt.

        // Dicht genug (20 Proben), eine einzelne Lücke von 201 ms.
        let mut mit_luecke: Vec<TouchdownWindowSample> = (-1000..=-600)
            .step_by(20)
            .map(|ms| bp(ms, 5.0, false, -200.0))
            .collect();
        mit_luecke.extend(
            (-399..=100)
                .step_by(20)
                .map(|ms| bp(ms, 2.0, ms >= 0, -140.0)),
        );
        assert!(
            mit_luecke.len() > MIN_BEWERTUNGS_PROBEN,
            "Probenzahl ist erfüllt"
        );
        let fehlt = pruefe_bewertbarkeit(&mit_luecke, kontakt()).unwrap_err();
        assert_eq!(
            fehlt.groesste_luecke_ms, 201,
            "nur die Lücke entscheidet hier"
        );

        // Umgekehrt: keine Lücke über 200 ms, aber zu wenige Proben.
        let zu_wenige: Vec<_> = (-1000..=100)
            .step_by(200)
            .map(|ms| bp(ms, 4.0, ms >= 0, -180.0))
            .collect();
        assert!(zu_wenige.len() < MIN_BEWERTUNGS_PROBEN);
        let fehlt = pruefe_bewertbarkeit(&zu_wenige, kontakt()).unwrap_err();
        assert!(
            fehlt.groesste_luecke_ms <= MAX_BEWERTUNGS_LUECKE_MS,
            "die Lücke allein wäre in Ordnung: {} ms",
            fehlt.groesste_luecke_ms
        );
        assert_eq!(
            fehlt.proben,
            zu_wenige.len(),
            "hier entscheidet die Probenzahl"
        );
    }

    #[test]
    fn genau_an_der_grenze_bleibt_die_bewertung_erhalten() {
        // 200 ms Abstand ist erlaubt, 201 ms nicht — beide Zweige getrennt
        // geprüft, damit eine Verschiebung der Konstante auffällt.
        let gerade_noch: Vec<_> = (-1000..=100)
            .step_by(MAX_BEWERTUNGS_LUECKE_MS as usize)
            .map(|ms| bp(ms, 3.0, ms >= 0, -140.0))
            .collect();
        assert_eq!(
            pruefe_bewertbarkeit(&gerade_noch, kontakt()).is_ok(),
            gerade_noch.len() >= MIN_BEWERTUNGS_PROBEN,
            "bei 200 ms darf nur noch die Probenzahl den Ausschlag geben"
        );

        let zu_grob: Vec<_> = (-1000..=100)
            .step_by(MAX_BEWERTUNGS_LUECKE_MS as usize + 1)
            .map(|ms| bp(ms, 3.0, ms >= 0, -140.0))
            .collect();
        assert!(pruefe_bewertbarkeit(&zu_grob, kontakt()).is_err());
    }

    #[test]
    fn zu_wenige_proben_sperren_auch_ohne_grosse_luecke() {
        // Gleichmässig verteilt, keine einzelne Lücke über 200 ms — aber
        // insgesamt zu grob. Genau der Fall, den eine reine Lückenprüfung
        // übersieht.
        let proben: Vec<_> = (-1000..=100)
            .step_by(150)
            .map(|ms| bp(ms, 4.0, ms >= 0, -200.0))
            .collect();
        assert!(proben.len() < MIN_BEWERTUNGS_PROBEN);
        let fehlt = pruefe_bewertbarkeit(&proben, kontakt()).unwrap_err();
        assert!(fehlt.groesste_luecke_ms <= MAX_BEWERTUNGS_LUECKE_MS);
        assert_eq!(fehlt.proben, proben.len());
    }

    #[test]
    fn eine_luecke_am_fensterrand_zaehlt_mit() {
        // Alle Proben dicht — aber die erste kommt erst 600 ms nach
        // Fensterbeginn. Dann fehlt die halbe Flare, und genau dort
        // entscheidet sich die Sinkrate.
        let proben: Vec<_> = (-400..=100)
            .step_by(20)
            .map(|ms| bp(ms, 4.0, ms >= 0, -200.0))
            .collect();
        let fehlt = pruefe_bewertbarkeit(&proben, kontakt()).unwrap_err();
        assert!(
            fehlt.groesste_luecke_ms >= 600,
            "die Randlücke muss zählen, gemessen: {} ms",
            fehlt.groesste_luecke_ms
        );
    }

    #[test]
    fn proben_ausserhalb_des_fensters_helfen_nicht() {
        // Dichte Aufzeichnung, aber erst ab 300 ms nach dem Kontakt — das
        // Ausrollen. Über die Landung sagt sie nichts.
        let proben: Vec<_> = (300..=2000)
            .step_by(20)
            .map(|ms| bp(ms, 0.4, true, -20.0))
            .collect();
        assert!(pruefe_bewertbarkeit(&proben, kontakt()).is_err());
    }

    #[test]
    fn unbrauchbare_werte_taeuschen_keine_abdeckung_vor() {
        // Proben mit NaN tragen nichts zur Messung bei. Mitgezählt würden
        // sie eine dichte Reihe vortäuschen, aus der nichts zu lesen ist.
        // Gegenprobe zuerst: dieselbe Reihe mit gültigen Werten ist bewertbar.
        assert_eq!(pruefe_bewertbarkeit(&dichte_proben(), kontakt()), Ok(()));

        // Alle unbrauchbar machen, dann drei in der Fenstermitte wieder
        // gültig — die ersten Proben der Reihe liegen noch vor dem Fenster
        // und würden ohnehin nicht zählen.
        let mut proben = dichte_proben();
        for p in proben.iter_mut() {
            p.vs_fpm = f32::NAN;
        }
        let behalten = 3;
        let mitte = proben.len() / 2;
        for p in proben.iter_mut().skip(mitte).take(behalten) {
            p.vs_fpm = -150.0;
        }
        let fehlt = pruefe_bewertbarkeit(&proben, kontakt()).unwrap_err();
        assert_eq!(
            fehlt.proben, behalten,
            "nur die Proben mit gültigen Werten dürfen zählen"
        );
    }

    #[test]
    fn doppelte_zeitstempel_zaehlen_einmal() {
        // Ein hängender Simulator liefert denselben Zustand mehrfach. Der
        // Sampler schreibt jede Runde eine Probe — die Zeitstempel wiederholen
        // sich dabei nicht, wohl aber in Aufzeichnungen aus Fremdquellen.
        let mut proben = vec![bp(-500, 5.0, false, -300.0); 40];
        proben.push(bp(0, 0.5, true, -120.0));
        let fehlt = pruefe_bewertbarkeit(&proben, kontakt()).unwrap_err();
        assert_eq!(
            fehlt.proben, 2,
            "vierzig gleiche Zeitstempel sind eine Probe"
        );
    }

    #[test]
    fn ein_leeres_fenster_ist_nie_bewertbar() {
        let fehlt = pruefe_bewertbarkeit(&[], kontakt()).unwrap_err();
        assert_eq!(fehlt.proben, 0);
        assert_eq!(
            fehlt.groesste_luecke_ms,
            BEWERTUNGS_FENSTER_VOR_MS + BEWERTUNGS_FENSTER_NACH_MS
        );
    }

    // ─── Bodenhöhe als Bezug der Tiefflug-Prüfung (Befund DLH 880) ────────

    /// DLH 880 (Sven M, 15.09.2026, Fenix A321, MSFS 2024): Kontakt bei
    /// 11,9 ft, danach Einfedern auf ~9,1 ft, Sinkrate im Kontakt −50 fpm,
    /// G-Spitze 1,01. Das Flugzeug steht mit ~9 ft auf dem Boden.
    fn dlh880_fenster() -> (TdCandidate, Vec<TouchdownWindowSample>) {
        let edge = 0_i64;
        let mut samples = Vec::new();
        let mut t = edge;
        while t <= 1200 {
            // 11,9 ft im Kontakt, linear auf 9,9 ft nach 1,2 s
            let agl = 11.9 - 2.0 * (t as f32 / 1200.0);
            samples.push(msfs_sample(t, agl, true, -50.0, 1.01));
            t += 20;
        }
        (msfs_candidate(edge, 11.9, -50.0, 1.01), samples)
    }

    fn referenz_mit(agl_ft: f32, anzahl: u32) -> BodenhoehenReferenz {
        let mut r = BodenhoehenReferenz::default();
        for _ in 0..anzahl {
            r.beobachte(agl_ft);
        }
        r
    }

    #[test]
    fn dlh880_ohne_bodenhoehe_bleibt_wie_bisher_verworfen() {
        // Gegenprobe: genau der Live-Befund. Ohne Messung gilt die alte
        // absolute Grenze — Tiefflug und G fallen durch, 2 von 4 Stimmen.
        let (cand, samples) = dlh880_fenster();
        let r = validate_candidate(
            &cand,
            &samples,
            SimKind::Msfs2024,
            -50.57,
            AircraftCategory::FixedWing,
            None,
        );
        match r {
            ValidationResult::FalseEdge { reason, result } => {
                assert!(matches!(reason, FalseEdgeReason::InsufficientVoteScore));
                assert!(!result.low_agl_persistence_pass);
                assert_eq!(result.low_agl_grenze_ft, Some(5.0));
            }
            ValidationResult::Validated { .. } => panic!("ohne Bodenhöhe darf sich nichts ändern"),
        }
    }

    #[test]
    fn dlh880_mit_gemessener_bodenhoehe_wird_erkannt() {
        let (cand, samples) = dlh880_fenster();
        let boden = referenz_mit(9.1, BODENHOEHE_MIN_PROBEN).bodenhoehe_ft();
        let r = validate_candidate(
            &cand,
            &samples,
            SimKind::Msfs2024,
            -50.57,
            AircraftCategory::FixedWing,
            boden,
        );
        match r {
            ValidationResult::Validated { result } => {
                assert!(result.low_agl_persistence_pass);
                assert!(result.sustained_ground_pass.unwrap());
                assert!(result.vs_negative_pass);
                assert!(
                    !result.g_force_pass.unwrap(),
                    "G bleibt knapp darunter — erkannt über die Abstimmung"
                );
                let grenze = result.low_agl_grenze_ft.unwrap();
                assert!(
                    (grenze - 14.125).abs() < 0.01,
                    "Grenze = Bodenhöhe + 5 ft, war {grenze}"
                );
            }
            ValidationResult::FalseEdge { reason, .. } => {
                panic!("echte weiche Landung mit hohem Bezugspunkt muss erkannt werden — got {reason:?}")
            }
        }
    }

    #[test]
    fn flackern_in_der_luft_bleibt_trotz_bodenhoehe_verworfen() {
        // Schutz gegen Phantom-Aufsetzer bleibt: Bodenkontakt-Signal flackert
        // in 50 ft über Grund. Bodenhöhe 9 ft → Grenze 14 ft, 50 ft liegt weit
        // darüber; G und Sinkrate unauffällig.
        let edge = 0_i64;
        let mut samples = Vec::new();
        let mut t = edge;
        while t <= 1200 {
            samples.push(msfs_sample(t, 50.0, t <= 600, -5.0, 1.0));
            t += 20;
        }
        let cand = msfs_candidate(edge, 50.0, -5.0, 1.0);
        let boden = referenz_mit(9.0, BODENHOEHE_MIN_PROBEN).bodenhoehe_ft();
        let r = validate_candidate(
            &cand,
            &samples,
            SimKind::Msfs2024,
            -5.0,
            AircraftCategory::FixedWing,
            boden,
        );
        assert!(
            matches!(r, ValidationResult::FalseEdge { .. }),
            "Flackern in 50 ft darf nicht als Landung gelten"
        );
    }

    #[test]
    fn bodenhoehe_gilt_erst_ab_genug_proben_und_ist_robust_gegen_ausreisser() {
        let mut r = referenz_mit(9.0, BODENHOEHE_MIN_PROBEN - 1);
        assert_eq!(r.bodenhoehe_ft(), None, "zu wenige Proben");
        r.beobachte(9.0);
        let b = r.bodenhoehe_ft().unwrap();
        assert!(
            (b - 9.125).abs() < 0.01,
            "Median in der Klassenmitte, war {b}"
        );

        // Ausreißer beim Laden (RYR 2: −148 ft) und Unsinn oberhalb der
        // Obergrenze zählen gar nicht; einzelne falsche Werte im gültigen
        // Bereich verschieben den Median nicht.
        let vorher = r.anzahl();
        r.beobachte(-148.0);
        r.beobachte(f32::NAN);
        r.beobachte(BODENHOEHE_MAX_FT + 1.0);
        assert_eq!(r.anzahl(), vorher);
        for _ in 0..50 {
            r.beobachte(25.0);
        }
        let b2 = r.bodenhoehe_ft().unwrap();
        assert!(
            (b2 - 9.125).abs() < 0.01,
            "Median bleibt beim Rollwert, war {b2}"
        );
    }

    #[test]
    fn tiefflug_grenze_ist_nie_strenger_als_vorher_und_gedeckelt() {
        assert_eq!(tiefflug_grenze_ft(None), 5.0);
        assert_eq!(
            tiefflug_grenze_ft(Some(-3.0)),
            5.0,
            "negative Bodenhöhe macht nicht strenger"
        );
        assert_eq!(tiefflug_grenze_ft(Some(f32::NAN)), 5.0);
        assert_eq!(tiefflug_grenze_ft(Some(9.0)), 14.0);
        assert_eq!(
            tiefflug_grenze_ft(Some(500.0)),
            BODENHOEHE_MAX_FT + TIEFFLUG_SPIELRAUM_FT
        );
    }

    #[test]
    fn hubschrauber_und_wasserflugzeuge_behalten_die_absolute_grenze() {
        // Präsenzweg ohne Sinkraten-/G-Anker: Kufen 12 ft über Grund,
        // Bodenkontakt-Signal an. Mit gemessener "Bodenhöhe" 9 ft (etwa vom
        // Start auf einem Dach) darf das nicht als Landung durchgehen.
        let edge = 0_i64;
        let mut samples = Vec::new();
        let mut t = edge;
        while t <= 1200 {
            samples.push(msfs_sample(t, 12.0, true, -3.0, 1.0));
            t += 20;
        }
        let cand = msfs_candidate(edge, 12.0, -3.0, 1.0);
        let boden = referenz_mit(9.0, BODENHOEHE_MIN_PROBEN).bodenhoehe_ft();
        for kat in [AircraftCategory::Helicopter, AircraftCategory::Seaplane] {
            match validate_candidate(&cand, &samples, SimKind::Msfs2024, -3.0, kat, boden) {
                ValidationResult::FalseEdge { result, .. } => {
                    assert_eq!(
                        result.low_agl_grenze_ft,
                        Some(TIEFFLUG_SPIELRAUM_FT),
                        "{kat:?}"
                    );
                }
                ValidationResult::Validated { .. } => {
                    panic!("{kat:?}: Bodenhöhe darf die Präsenzprüfung nicht aufweichen")
                }
            }
        }
    }

    #[test]
    fn proben_ueber_der_obergrenze_zaehlen_nicht() {
        // Kein Kappen auf 22 ft: Ein Flugzeug (oder eine Fehlmessung) jenseits
        // der Obergrenze bekommt gar keine Bodenhöhe und damit die alte Grenze.
        let r = referenz_mit(BODENHOEHE_MAX_FT + 3.0, BODENHOEHE_MIN_PROBEN * 2);
        assert_eq!(r.bodenhoehe_ft(), None);
        assert_eq!(tiefflug_grenze_ft(r.bodenhoehe_ft()), TIEFFLUG_SPIELRAUM_FT);
    }

    #[test]
    fn nur_ruhige_bodenproben_zaehlen_zur_bodenhoehe() {
        assert!(ist_rollprobe(true, 12.0, false, false));
        assert!(
            ist_rollprobe(true, 0.0, false, false),
            "Stehen am Gate zählt"
        );
        assert!(!ist_rollprobe(false, 12.0, false, false), "in der Luft nie");
        assert!(
            !ist_rollprobe(true, BODENHOEHE_MAX_GS_KT, false, false),
            "Startlauf/Ausrollen nicht"
        );
        assert!(!ist_rollprobe(true, 5.0, true, false), "Pause nicht");
        assert!(!ist_rollprobe(true, 5.0, false, true), "Versetzen nicht");
        assert!(!ist_rollprobe(true, f32::NAN, false, false));
    }

    #[test]
    fn bodenhoehe_ueberlebt_speichern_und_laden() {
        let r = referenz_mit(9.1, BODENHOEHE_MIN_PROBEN);
        let json = serde_json::to_string(&r).unwrap();
        let zurueck: BodenhoehenReferenz = serde_json::from_str(&json).unwrap();
        assert_eq!(zurueck, r);
        assert_eq!(zurueck.bodenhoehe_ft(), r.bodenhoehe_ft());
        // Alte Sicherung ohne Feld / leeres Objekt → keine Messung, kein Absturz.
        let leer: BodenhoehenReferenz = serde_json::from_str("{}").unwrap();
        assert_eq!(leer.bodenhoehe_ft(), None);
    }
}
