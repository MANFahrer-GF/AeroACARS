//! AeroACARS landing-score Single-Source-of-Truth Crate.
//!
//! v0.7.1 Phase 0: 1:1-Port von `client/src/lib/landingScoring.ts`.
//! Per Spec docs/spec/v0.7.1-landing-ux-fairness.md §3.1: alle
//! Konsumenten (Backend lib.rs, Tauri-Frontend, aeroacars-live
//! webapp + monitor) nutzen identische Sub-Scores aus dieser
//! Crate. UI rendert ausschliesslich die hier produzierten
//! `SubScoreEntry`-Werte aus dem PIREP-Payload — KEIN Recompute.
//!
//! Phase 0 portiert die Legacy-Algorithmen 1:1 (Goldenset blockiert
//! Drift > 0.5 Punkte gegen TS). Phase 2 fuehrt die Asymmetrie und
//! Skip-Logik ein (F2/F3). Phase 3 baut sub_stability auf 4-Faktor-
//! Voting um (F7-B).

use serde::{Deserialize, Serialize};

/// 05.10.2026: Abfangen (Flare) über die Höhe — Messung und Teilnote.
pub mod abfangen;
/// Lernpaket AP4/AP5 (29.09.2026): Datenform der Anflug-Forensik (keine Note).
pub mod anflug_forensik;
pub mod anflug_urteil;
pub mod belag;
pub mod gate;
/// v1.7.35: Sprit-Auswertung ohne Note — siehe `sprit.rs`.
pub mod sprit;
pub mod spurweite;
pub mod sub_alignment;
pub mod sub_bahndisziplin;
pub mod sub_bounces;
pub mod sub_fuel;
pub mod sub_g_force;
pub mod sub_landing_rate;
pub mod sub_loadsheet;
pub mod sub_rollout;
pub mod sub_stability;
pub mod sub_touchdown_point;
pub mod vref;

/// Score-Band — 1:1 aus TS `Band`. NICHT umbenennen, bestehende UI
/// erwartet exakt diese Werte (siehe Spec §5.4 K1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Band {
    Good,
    Ok,
    Bad,
    Skipped,
}

impl Band {
    /// Wire-Format als String fuer JSON/Serde — matched die TS-
    /// Enum-Werte 1:1 ("good" | "ok" | "bad" | "skipped").
    pub fn as_str(self) -> &'static str {
        match self {
            Band::Good => "good",
            Band::Ok => "ok",
            Band::Bad => "bad",
            Band::Skipped => "skipped",
        }
    }
}

/// Konvertiert Score-Punkte → Band. Identisch zu TS `band(points)`
/// in landingScoring.ts:138-142.
pub fn band_from_points(points: u8) -> Band {
    if points >= 75 {
        Band::Good
    } else if points >= 45 {
        Band::Ok
    } else {
        Band::Bad
    }
}

/// Sub-Score Wire-Format. Spec §5.4 (P1.5-A): voll ausgebaut auf
/// alle Render-Felder, damit Web/Monitor ohne Recompute bit-identisch
/// rendern koennen.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubScoreEntry {
    /// Stabile Schluessel — siehe TS-Type SubScore["key"] +
    /// neue v0.7.1-Schluessel "loadsheet" (F1) + "flare" (F6).
    pub key: String,
    /// 0-100, gerundet
    pub score: u8,
    /// Alias fuer score — bestehende UI nutzt .points
    pub points: u8,
    /// "good" | "ok" | "bad" | "skipped" (siehe Band)
    pub band: String,
    /// i18n-Key z.B. "landing.sub.fuel"
    pub label_key: String,
    /// formatiert: "-191 fpm", "1.32 g", "+5.2 %"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// i18n-Key z.B. "landing.rat.smooth_touchdown"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale_key: Option<String>,
    /// i18n-Key z.B. "landing.tip.firm_touchdown"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip_key: Option<String>,
    /// true wenn Sub-Score nicht bewertet wurde (z.B. VFR ohne ZFW)
    pub skipped: bool,
    /// Skip-Reason ("no_planned_burn", "no_actual_burn", ...)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Optionale Warnung (z.B. "planned_burn_may_be_off" bei
    /// implausibel-hohem Minderverbrauch — Phase 2/F3)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// v0.10.0 (#runway-utilization-score): Zusatz-Display-Zeilen für die
    /// UI-Card (z.B. „davon ~520 m Float vor Aufsetzen", „Bahn: YMML 16,
    /// LDA 3657 m"). Renderer alter Versionen ignorieren das Feld
    /// schweigend (forward-compat). Renderer v0.10+ die ein Payload ohne
    /// `extra` bekommen, behandeln es als leere Liste (`#[serde(default)]`).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub extra: Vec<String>,
    /// Lernpaket AP2 (29.09.2026): der gemessene Zahlenwert hinter der
    /// Teilnote, wo eine spaetere Regel ihn braucht — bisher nur `g_force`
    /// (G auf der Referenzkette, dieselbe Zahl, aus der die Teilnote
    /// entsteht). `value` ist fuer die Anzeige formatiert und taugt dafuer
    /// nicht. Alte Payloads ohne das Feld lesen es als `None`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub messwert: Option<f32>,
    /// Score-Version 19: nur an `stability` — die Pruefungen des Stable Gate
    /// mit Messwert und Grenze, damit jede Oberflaeche den Grund des Urteils
    /// nennt („Gleitpfad 1,7 Dots, gut unter 1"). Alte Payloads: leer.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub gate: Vec<anflug_urteil::Pruefpunkt>,
    /// Abnahme 06.10.2026: Übersetzungsschlüssel des Werttextes samt Zahlen
    /// (schon formatiert, ohne „-0"). `value` bleibt der deutsche Text für
    /// ältere Empfänger; die Anzeige baut daraus den Text in ihrer Sprache —
    /// sonst stand „m hinter der Schwelle" auch in der englischen Oberfläche.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub value_key: Option<String>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty", default)]
    pub value_params: std::collections::BTreeMap<String, String>,
}

/// Zahl mit `stellen` Nachkommastellen, ohne „-0" (Abnahme 06.10.2026).
pub fn zahl_text(v: impl Into<f64>, stellen: usize) -> String {
    let v: f64 = v.into();
    let s = format!("{v:.stellen$}");
    if s.trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        s.trim_start_matches('-').to_string()
    } else {
        s
    }
}

/// Wie `zahl_text`, positive Werte mit „+".
pub fn vorzeichen_text(v: impl Into<f64>, stellen: usize) -> String {
    let s = zahl_text(v, stellen);
    if !s.starts_with('-') && s.chars().any(|c| c.is_ascii_digit() && c != '0') {
        format!("+{s}")
    } else {
        s
    }
}

/// QS 06.10.2026: Jeder Werttext, den ein Test erzeugt, muss in allen drei
/// Sprachen existieren und genau die mitgegebenen Zahlen als Platzhalter
/// tragen. Ohne diese Prüfung bliebe ein umbenannter Schlüssel oder
/// Parameter überall grün — die Anzeige zeigte dann den rohen Schlüssel
/// oder `{{m}}`.
#[cfg(test)]
fn wert_text_pruefen(schluessel: &str, werte: &std::collections::BTreeMap<String, String>) {
    use std::collections::BTreeSet;
    use std::sync::OnceLock;
    static SPRACHEN: OnceLock<Vec<(&'static str, serde_json::Value)>> = OnceLock::new();
    let sprachen = SPRACHEN.get_or_init(|| {
        [
            ("de", include_str!("../../../../src/locales/de/common.json")),
            ("en", include_str!("../../../../src/locales/en/common.json")),
            ("it", include_str!("../../../../src/locales/it/common.json")),
        ]
        .into_iter()
        .map(|(l, roh)| (l, serde_json::from_str(roh).expect("Sprachdatei lesbar")))
        .collect()
    });
    let erwartet: BTreeSet<&str> = werte.keys().map(String::as_str).collect();
    for (sprache, daten) in sprachen {
        let text = schluessel
            .split('.')
            .try_fold(daten, |k, teil| k.get(teil))
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{sprache}: Werttext {schluessel} fehlt"));
        let platzhalter: BTreeSet<&str> = text
            .split("{{")
            .skip(1)
            .filter_map(|r| r.split_once("}}").map(|(name, _)| name.trim()))
            .collect();
        assert_eq!(
            platzhalter, erwartet,
            "{sprache}: Platzhalter von {schluessel} passen nicht zu den Zahlen"
        );
    }
}

impl SubScoreEntry {
    /// Hilfs-Konstruktor fuer skipped Sub-Scores.
    pub fn skipped(key: &str, label_key: &str, reason: &str) -> Self {
        Self {
            key: key.to_string(),
            score: 0,
            points: 0,
            band: Band::Skipped.as_str().to_string(),
            label_key: label_key.to_string(),
            value: None,
            rationale_key: None,
            tip_key: None,
            skipped: true,
            reason: Some(reason.to_string()),
            warning: None,
            extra: Vec::new(),
            messwert: None,
            gate: Vec::new(),
            value_key: None,
            value_params: std::collections::BTreeMap::new(),
        }
    }

    /// Hilfs-Konstruktor fuer bewertete Sub-Scores. Setzt
    /// `points = score`, `band` aus `band_from_points`, `tip_key
    /// = label_key + "tip." + rationale` Konvention.
    pub fn scored(
        key: &str,
        label_key: &str,
        score: u8,
        value: String,
        rationale: &str,
        band: Band,
    ) -> Self {
        Self {
            key: key.to_string(),
            score,
            points: score,
            band: band.as_str().to_string(),
            label_key: label_key.to_string(),
            value: Some(value),
            rationale_key: Some(format!("landing.rat.{}", rationale)),
            tip_key: Some(format!("landing.tip.{}", rationale)),
            skipped: false,
            reason: None,
            warning: None,
            extra: Vec::new(),
            messwert: None,
            gate: Vec::new(),
            value_key: None,
            value_params: std::collections::BTreeMap::new(),
        }
    }

    /// v0.10.0 Builder: hängt Display-Extra-Zeilen an (LE12). Wird vom
    /// `sub_rollout_v2` genutzt um Float-/Bahn-Info unter die Rationale
    /// zu setzen. No-op wenn `extra` leer ist.
    pub fn with_extra(mut self, extra: Vec<String>) -> Self {
        self.extra = extra;
        self
    }

    /// v0.10.0 Builder: setzt das Warning-Feld (existiert bereits im
    /// Wire-Format, kriegt in v0.10.0 nur einen neuen Wert
    /// `pre_displaced_threshold`). `None` löscht eine evtl. vorher
    /// gesetzte Warning.
    pub fn with_warning(mut self, warning: Option<String>) -> Self {
        self.warning = warning;
        self
    }

    /// Abnahme 06.10.2026: Werttext als Schlüssel + Zahlen (siehe `value_key`).
    pub fn mit_wert_text(mut self, schluessel: &str, werte: &[(&str, String)]) -> Self {
        self.value_key = Some(schluessel.to_string());
        self.value_params = werte
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        #[cfg(test)]
        wert_text_pruefen(schluessel, &self.value_params);
        self
    }

    /// Lernpaket AP2 Builder: haengt den Messwert an (siehe `messwert`).
    pub fn mit_messwert(mut self, messwert: f32) -> Self {
        self.messwert = Some(messwert);
        self
    }
}

/// Eingabe-Struktur fuer `compute_sub_scores`. Spiegel der TS-Signatur
/// von `computeSubScores(p)` in landingScoring.ts:218-238 plus die
/// in Phase 2+ benoetigten Felder fuer F1 (loadsheet) und F6 (flare).
///
/// Felder mit `Option<>` werden als "nicht vorhanden" interpretiert
/// und produzieren entweder den Default-Pfad (z.B. `bounce_count
/// .unwrap_or(0)`) oder einen skipped Sub-Score (Phase 2: fuel/
/// loadsheet).
#[derive(Debug, Clone, Default)]
pub struct LandingScoringInput {
    pub vs_fpm: Option<f32>,
    /// Roher 50-Hz-Einzelframe-G-Peak. **Forensik / Backward-Fallback** —
    /// `sub_g_force` scort dies NICHT mehr direkt, sondern `scored_g_load`
    /// falls vorhanden (v0.12.3 LE7/LE8).
    pub peak_g_load: Option<f32>,
    /// v0.12.3 (LE8): EMA-geglätteter Fenster-Peak (FOQA-Methode). Wenn
    /// gesetzt, scort `sub_g_force` diesen Wert; sonst Fallback auf
    /// `peak_g_load`. `None` bei pre-v0.12.3-Callern → identisches
    /// Alt-Verhalten (Roh-Peak).
    pub scored_g_load: Option<f32>,
    pub bounce_count: Option<u32>,
    pub approach_vs_stddev_fpm: Option<f32>,
    pub approach_bank_stddev_deg: Option<f32>,
    /// v1.9.16: die sieben Gate-Messwerte hinter dem STABLE/PARTIAL/UNSTABLE-
    /// Urteil — siehe `anflug_urteil`. Leer (Altbestand, Tests) = kein Urteil,
    /// kein Deckel auf der Stabilitaetsachse.
    pub anflug: anflug_urteil::AnflugWerte,
    pub rollout_distance_m: Option<f32>,
    pub fuel_efficiency_pct: Option<f32>,
    // Phase 2 (F1 + F2 + F3): VFR/ZFW + Fuel-Asymmetrie
    pub planned_zfw_kg: Option<f32>,
    pub planned_tow_kg: Option<f32>,
    pub planned_burn_kg: Option<f32>,
    pub actual_trip_burn_kg: Option<f32>,
    /// Wurde ausgewichen — also woanders gelandet als geplant?
    ///
    /// ⚠ Dann ist die OFP-Treue nicht bewertbar: Es wurde eine ANDERE
    /// Strecke geflogen als die geplante, und ein Vergleich gegen den
    /// urspruenglichen Verbrauch misst den Umweg, nicht den Piloten.
    /// `None` = unbekannt, dann wird wie bisher verglichen.
    pub diverted: Option<bool>,
    /// v1.7.32 — Zusatzarbeit fuer die OFP-Treue: tatsaechlich geflogene
    /// Strecke, Luftlinie Start→Ziel und Zahl der Durchstartmanoever.
    /// Siehe `sub_fuel`s Regelblock. Leer = keine Gutschrift (Alt-Caller).
    pub geflogene_strecke_nm: Option<f32>,
    pub plan_strecke_nm: Option<f32>,
    pub durchstarts: u32,
    /// Wurde die Bahngeometrie gegen die SZENERIE des Simulators
    /// geprueft und uebernommen?
    ///
    /// ⚠ Das entscheidet, ob eine schraeg laufende Rollspur unserer
    /// Achse angelastet wird oder dem Piloten. Ist die Geometrie
    /// bestaetigt, ist sie nicht mehr verdaechtig — dann ist eine Spur
    /// am Bahnrand genau das, was bewertet werden soll.
    pub bahn_geometrie_aus_szenerie: Option<bool>,
    // Phase 3 hook (Flare-Sub-Score kommt in Phase 3/F6).
    pub flare_quality_score: Option<u8>,
    /// 05.10.2026: Abfangen über die Höhe (`abfangen::messen`). `None` =
    /// keine Messung (Altbestand) — dann gibt es die Achse nicht.
    pub abfangen: Option<abfangen::Abfangen>,
    /// v0.7.17 (N-002): ICAO type designator des geflogenen
    /// Aircraft (z.B. "A320", "B738", "C172"). Wird vom `sub_rollout`-
    /// Score genutzt um die Bahn-Auslastung-Schwellen aircraft-
    /// kategorie-abhaengig zu staffeln — vorher feuerte jeder Airliner
    /// einen „long_rollout"-Score (25 Pkt) selbst bei voellig normalen
    /// 2 km auf einer 3 km Bahn. None heisst „unbekannt" → konservative
    /// (medium-light) Schwellen werden genutzt.
    pub aircraft_icao: Option<String>,
    // ─── v0.10.0 Runway-Utilization-Score (Spec docs/spec/v0.10.0-
    // runway-utilization-score.md) ──────────────────────────────────
    //
    // Wenn ALLE folgenden Felder Some sind UND alle Skip-Gates pass
    // (siehe sub_rollout::sub_rollout_v2), wird der NEUE Algorithmus
    // (LDA-basiert, td+rollout/lda) verwendet und im PIREP/Touchdown
    // mit `score_algorithm_version` markiert (v0.16.21: Some(4) nach
    // MSFS-Touchdown-V/S-g-Delag; v0.12.0–v0.16.20: Some(3) Float-
    // Toleranz-Refinement; vor v0.12.0: Some(2)). Wenn
    // eines fehlt → Skip mit konkretem Reason (KEIN Fallback auf v1).
    //
    // Backward-Compat: Wenn aufrufender Code die Felder None lässt,
    // ruft `compute_sub_scores` weiterhin den alten `sub_rollout` als
    // Fallback (= identisch zu v0.9.x-Verhalten).
    pub td_distance_from_threshold_m: Option<f64>,
    pub landing_float_distance_m: Option<f32>,
    pub runway_length_m: Option<f32>,
    pub runway_displaced_threshold_ft: Option<i32>,
    /// v1.7.0 — Ziel-Markierung ab der Lande-Schwelle, aus
    /// `runway_assessment::classify_aim`. Bewusst als Eingabe statt hier
    /// gerechnet: die Regel (300 m / 400 m ab 2400 m Bahnlänge) lebt an genau
    /// einer Stelle.
    pub aim_point_m: Option<f64>,
    /// v1.7.0 Bahndisziplin — groesster seitlicher Versatz ueber den
    /// gewerteten Rollweg, in Metern von der Mittellinie.
    pub bahn_max_querversatz_m: Option<f64>,
    /// v1.7.0 — Strecke jenseits des Bahnendes, `None`/0 = kein Overrun.
    pub bahn_overrun_m: Option<f64>,
    /// v1.7.0 — Positionsproben im Messfenster.
    pub bahn_proben: Option<usize>,
    /// Winkel der Rollspur zur Bahnachse in Grad — Szenerie gegen Navdaten.
    pub bahn_achsen_abweichung_grad: Option<f64>,
    /// Kreuzt die Rollspur im gewerteten Fenster die Mittellinie?
    pub bahn_achsen_kreuzt_mitte: Option<bool>,
    /// Grösster Betrag der Querlage im gewerteten Fenster, in Metern.
    pub bahn_achsen_groesster_betrag_m: Option<f64>,
    /// v1.7.0 — Spurweite aus der **Flugzeugdatei**, falls gelesen.
    ///
    /// # Warum die Bewertung sie kennen muss
    ///
    /// Seit Schritt 11 der Bauliste liest der Client die Spurweite bei
    /// Bedarf aus der `.acf` bzw. `flight_model.cfg` des geladenen
    /// Musters. Sie hat Vorrang vor der Typtabelle, weil sie das
    /// tatsaechlich geflogene Add-on beschreibt und nicht das Realmuster.
    ///
    /// Bis zur QS-Runde 20 kam dieser Wert **nur in der Anzeige** an: Die
    /// Bewertung rechnete unverdrossen mit `spurweite_m(icao)`. Bei einem
    /// Add-on, dessen Fahrwerk vom Realmuster abweicht, zeigte die Grafik
    /// damit einen anderen Randabstand, als die Note benutzte — und die
    /// Herkunftsangabe daneben behauptete „aus der Flugzeugdatei".
    ///
    /// `None` heisst „keine Datei gelesen", nicht „keine Spurweite": Dann
    /// gilt die Tabelle.
    pub fahrwerk_spurweite_m: Option<f64>,
    /// v1.7.0 — Bahnbelag als Rohangabe (OurAirports `runways.surface`).
    pub runway_surface: Option<String>,
    /// v1.7.0 — Ende der Aufsetzzone ab der Schwelle, aus `classify_tdz`.
    /// `None` = Bahn unter 1200 m, hat laut Annex 14 keine Zonenmarkierung.
    pub tdz_end_m: Option<f64>,
    pub pre_displaced_threshold: Option<bool>,
    pub runway_geometry_trusted: Option<bool>,
    pub airport_source: Option<String>,
    pub runway_match_icao: Option<String>,
    pub runway_match_ident: Option<String>,
    // ─── v1.6.2 Ausrichtungs-Score (score_algorithm_version 6) ────────
    //
    // Siehe `sub_alignment` — bewertet WO und wie gerade aufgesetzt wurde.
    // Alle vier Felder muessen vorliegen, sonst wird die Achse sichtbar
    // als „nicht bewertet" ausgewiesen statt zu bestrafen.
    pub runway_match_centerline_offset_m: Option<f32>,
    /// Bahnbreite in Metern. Ohne sie ist der Versatz nicht einzuordnen —
    /// dieselben 26 m sind auf einer 61-m-Bahn harmlos und auf einer
    /// 45-m-Bahn der Bahnrand.
    pub runway_width_m: Option<f32>,
    pub landing_heading_true_deg: Option<f32>,
    pub runway_true_course_deg: Option<f32>,
    /// Drehflügler/Wasserflugzeug — siehe `sub_alignment::AlignmentInput`.
    pub nicht_konventionell: bool,
    /// Rechtweisender Wind beim Aufsetzen (Seitenwind-Kompensation).
    pub landing_wind_direction_deg: Option<f32>,
    pub landing_wind_speed_kt: Option<f32>,
    /// Geschwindigkeit über Grund beim Aufsetzen.
    pub landing_groundspeed_kt: Option<f32>,
}

/// Berechnet alle Sub-Scores.
///
/// v0.7.1 Phase 2: nutzt `sub_fuel_v0_7_1` (F2 Hard-Gate + F3
/// Asymmetrie) und `sub_loadsheet` (F1 VFR-Skip). Stability bleibt
/// im 2-Faktor-Modus bis Phase 3 F7-B aktiviert (siehe Spec §5.5
/// Backward-Compat-Test 7.2.1).
/// v0.10.0 — Sentinel: ist mindestens ein v2-Feld gesetzt? Wenn ja
/// laufen wir den neuen `sub_rollout_v2`-Pfad (auch wenn der dann
/// skipped). Wenn nein → alter `sub_rollout` (Backward-Compat für
/// pre-v0.10-Caller wie Bin-Tools oder Tests).
///
/// Wir prüfen die DENOMINATOR-Bezogenen Felder (Bahn-Länge / Geometry-
/// Trust / Airport-Source) — die sind erst ab v0.8.x überhaupt da. Wenn
/// hier nichts steht, ist der Caller definitiv pre-v0.10.
fn scoring_input_has_v2_fields(input: &LandingScoringInput) -> bool {
    input.runway_length_m.is_some()
        || input.runway_geometry_trusted.is_some()
        || input.airport_source.is_some()
        || input.td_distance_from_threshold_m.is_some()
}

pub fn compute_sub_scores(input: &LandingScoringInput) -> Vec<SubScoreEntry> {
    let mut out = Vec::with_capacity(8);

    // v0.20.2: Fehlt die Sinkrate, wird die Achse SICHTBAR als "nicht bewertet"
    // ausgewiesen — sie darf nicht stillschweigend aus der Bewertung fallen.
    //
    // Bis v0.20.1 lieferte die Kanonik immer irgendeinen Wert (notfalls einen
    // falschen aus einem Glitch-Sample). Seit sie unplausible Werte verwirft,
    // kann sie `None` liefern — und dann fehlte hier der Sub-Score komplett.
    // Folge: das Aggregat waere nur noch aus G-Kraft, Hopsern, Anflug, Bahn und
    // Sprit gebildet worden. Ein Flug, dessen LANDERATE gar nicht messbar war,
    // haette einen gut aussehenden Gesamtscore bekommen, ohne dass der Pilot
    // etwas davon merkt. Die wichtigste Achse still wegzulassen ist schlimmer
    // als die falsche Zahl, die wir gerade abgeschafft haben.
    match input.vs_fpm {
        Some(vs) => out.push(sub_landing_rate::sub_landing_rate(vs)),
        None => out.push(SubScoreEntry::skipped(
            "landing_rate",
            "landing.sub.landing_rate",
            "landing_rate_unmeasurable",
        )),
    }
    // v0.12.3 (LE8): score the EMA-smoothed `scored_g_load` when present;
    // fall back to the raw `peak_g_load` for pre-v0.12.3 callers.
    if let Some(g) = input.scored_g_load.or(input.peak_g_load) {
        out.push(sub_g_force::sub_g_force(g));
    }
    out.push(sub_bounces::sub_bounces(input.bounce_count.unwrap_or(0)));

    let pruefung = anflug_urteil::anflug_pruefung(&input.anflug);
    let urteil = anflug_urteil::urteil_aus(&pruefung);
    if let Some(mut stab) = sub_stability::sub_stability_legacy(
        input.approach_vs_stddev_fpm,
        input.approach_bank_stddev_deg,
        urteil,
    ) {
        stab.gate = pruefung;
        out.push(stab);
    } else if scoring_input_has_v2_fields(input) && !input.nicht_konventionell {
        // v1.9.16: Anflug nicht gemessen (zu wenig Samples im Gate). Die
        // Achse bleibt sichtbar als „nicht bewertet" und traegt die Marke,
        // an der `master_deckel` die Bestnote sperrt: ohne Messung keine
        // 100 Punkte.
        //
        // Score-Version 18 (Cloud-QS 05.10.2026): Fehlt nur die Streuung,
        // liegt aber ein Urteil vor (eigene Stichprobe), zaehlt das Urteil —
        // sonst kaeme ein PARTIAL/UNSTABLE-Anflug mit Deckel 97 davon.
        // Je Grund ein eigener Aufruf mit Literal: `i18nSchluessel.test.ts`
        // liest die Skip-Gruende per Muster aus dem Quelltext.
        let mut stab = match urteil {
            Some(anflug_urteil::AnflugUrteil::Partial) => {
                SubScoreEntry::skipped("stability", "landing.sub.stability", "anflug_partial")
            }
            Some(anflug_urteil::AnflugUrteil::Unstable) => {
                SubScoreEntry::skipped("stability", "landing.sub.stability", "anflug_unstable")
            }
            _ => {
                SubScoreEntry::skipped("stability", "landing.sub.stability", ANFLUG_NICHT_GEMESSEN)
            }
        };
        stab.gate = pruefung;
        out.push(stab);
    }
    // v0.10.0 (#runway-utilization-score): Wenn die v2-Datenlage da ist,
    // wird der neue LDA-basierte Sub-Score gerechnet (auch bei
    // Skip-Outcomes wie `missing_td_distance`). Der Caller markiert die
    // Records dann mit `score_algorithm_version` (v0.16.21: Some(4) nach
    // MSFS-Touchdown-V/S-g-Delag; v0.12.0–v0.16.20: Some(3) Float-Toleranz-
    // Refinement; vor v0.12.0: Some(2)).
    //
    // Wenn keines der v2-Felder gesetzt ist (= Caller ist nicht-migriert
    // oder Test-Fixture ohne Touchdown-Forensik), fallen wir auf den
    // alten meter-only `sub_rollout` zurück damit pre-v0.10 Code-Pfade
    // (Bin-Tools, alte Tests) unverändert weiterlaufen.
    // v1.7.0: Die Bahn-Achse bewertet nicht mehr die AUSLASTUNG, sondern die
    // DISZIPLIN — nur noch, was ohne Kontextwissen eindeutig falsch ist. Die
    // Begruendung steht in `sub_bahndisziplin`; kurz: 80 % der alten Abzuege
    // trafen Landungen ohne jedes Reserve-Problem.
    //
    // Der alte `sub_rollout_v2` bleibt vorerst im Baum, wird aber nicht mehr
    // aufgerufen. Er faellt, sobald die Anzeige umgestellt ist — bis dahin
    // dient er als Vergleichsmassstab beim Nachrechnen ueber den Korpus.
    if scoring_input_has_v2_fields(input) {
        out.push(sub_bahndisziplin::sub_bahndisziplin(
            &sub_bahndisziplin::BahndisziplinInput {
                max_querversatz_m: input.bahn_max_querversatz_m,
                bahnbreite_m: input.runway_width_m.map(|w| w as f64),
                // Dieselbe Rangfolge wie in der Anzeige (`bahn_felder`):
                // die Flugzeugdatei zuerst, die Typtabelle als Rueckfall.
                spurweite_m: input
                    .fahrwerk_spurweite_m
                    .or_else(|| spurweite::spurweite_m(input.aircraft_icao.as_deref())),
                overrun_m: input.bahn_overrun_m,
                belag: Some(belag::belag_aus_angabe(input.runway_surface.as_deref())),
                airport_source: match input.airport_source.as_deref() {
                    Some("runway_match") => Some("runway_match"),
                    _ => None,
                },
                runway_geometry_trusted: input.runway_geometry_trusted,
                bahn_geometrie_aus_szenerie: input.bahn_geometrie_aus_szenerie,
                proben: input.bahn_proben,
                // Der Winkel wird dort gerechnet, wo die Spur liegt
                // (lib.rs), nicht hier — die Achse bekommt das Ergebnis.
                achsen_abweichung_grad: input.bahn_achsen_abweichung_grad,
                achsen_kreuzt_mitte: input.bahn_achsen_kreuzt_mitte,
                achsen_groesster_betrag_m: input.bahn_achsen_groesster_betrag_m,
            },
        ));
    }

    // v1.7.0: Der alte meter-only `sub_rollout` bleibt fuer nicht-migrierte
    // Aufrufer (Bin-Tools, alte Fixtures ohne Touchdown-Forensik).
    if !scoring_input_has_v2_fields(input) {
        if let Some(ro) =
            sub_rollout::sub_rollout(input.rollout_distance_m, input.aircraft_icao.as_deref())
        {
            out.push(ro);
        }
    }

    // v1.6.2: Ausrichtung — WO und wie gerade wurde aufgesetzt. Laeuft nur
    // im v2-Datenpfad, weil sie dieselbe Bahn-Geometrie braucht wie der
    // Bremsweg-Score; ohne die Felder erscheint sie gar nicht (statt als
    // „nicht bewertet"), damit alte Aufrufer und Test-Fixturen
    // unveraendert 7 Achsen sehen.
    if scoring_input_has_v2_fields(input) {
        out.push(sub_alignment::sub_alignment(
            &sub_alignment::AlignmentInput {
                centerline_offset_m: input.runway_match_centerline_offset_m,
                runway_width_m: input.runway_width_m,
                heading_true_deg: input.landing_heading_true_deg,
                runway_true_course_deg: input.runway_true_course_deg,
                airport_source: input.airport_source.clone(),
                runway_geometry_trusted: input.runway_geometry_trusted,
                nicht_konventionell: input.nicht_konventionell,
                wind_direction_deg: input.landing_wind_direction_deg,
                wind_speed_kt: input.landing_wind_speed_kt,
                bezugsgeschwindigkeit_kt: input.landing_groundspeed_kt,
            },
        ));
    }

    // v1.7.0: Aufsetzpunkt — WO laengs aufgesetzt wurde. Wie die Ausrichtung
    // nur im v2-Datenpfad, weil dieselbe Bahn-Geometrie gebraucht wird. Die
    // nutzbare Laenge wird hier EINMAL gebildet (Bahnlaenge minus versetzte
    // Schwelle) und ist dieselbe Groesse, die `sub_rollout_v2` verwendet.
    if scoring_input_has_v2_fields(input) {
        let lda_m = input.runway_length_m.map(|len| {
            let displaced = input.runway_displaced_threshold_ft.unwrap_or(0) as f64 * 0.3048;
            len as f64 - displaced
        });
        out.push(sub_touchdown_point::sub_touchdown_point(
            &sub_touchdown_point::TouchdownPointInput {
                td_distance_from_threshold_m: input.td_distance_from_threshold_m,
                aim_point_m: input.aim_point_m,
                tdz_end_m: input.tdz_end_m,
                lda_m,
                airport_source: match input.airport_source.as_deref() {
                    Some("runway_match") => Some("runway_match"),
                    _ => None,
                },
                runway_geometry_trusted: input.runway_geometry_trusted,
            },
        ));
    }

    // 05.10.2026: Abfangen — wie lange vom ersten 50-ft-Durchgang (im
    // letzten Anflug) bis zum Aufsetzen. Ohne Messung keine Achse (siehe
    // `abfangen.rs`). Nicht für Hubschrauber/Wasserflugzeuge (QS 06.10.2026):
    // Schwebeflug ist dort das Verfahren, kein zu langes Abfangen — wie die
    // Anflug- und Ausrichtungsbewertung.
    if !input.nicht_konventionell {
        if let Some(s) =
            abfangen::sub_abfangen(input.abfangen.as_ref(), input.aircraft_icao.as_deref())
        {
            out.push(s);
        }
    }

    // ⚠ `sub_fuel` wird NICHT MEHR bewertet (v1.7.35).
    //
    // # Warum die Achse raus ist
    //
    // Sie verglich den geplanten mit dem tatsaechlichen Streckenverbrauch.
    // Am Bestand belegt (DLH370, 17.09.2026; alle A380-Fluege nach EDDM):
    // Der Mehrverbrauch entsteht zum Grossteil im Anflug — Radarfuehrung,
    // Zwischenhoehen, Transitions — und den bestimmt die Flugsicherung.
    // Ein Pilot, der exakt nach Plan flog (Hoehe, Route, Gewicht), verlor
    // 45 Punkte fuer 15,8 Minuten unter 8 000 ft. Der Pilot bewegt am
    // Verbrauch 2–3 %, ATC bis ueber 25 % (EUROCONTROL 2020). Kein reales
    // Airline-Programm und keine grosse VA-Plattform benotet den
    // Einzelflug nach Verbrauch.
    //
    // (Thomas, 17.09.2026: „Ich soll ja nicht bestraft werden, wenn ich
    // richtig fliege.")
    //
    // Der Sprit bleibt sichtbar — als Erklaerung, nicht als Note: siehe
    // `sprit::auswerten` (Phasen, Zeit unter Schwelle, Sprit-Leiter,
    // Final Reserve). Der Wächter `compute_sub_scores_never_emits_fuel`
    // stellt sicher, dass die Achse nicht still zurueckkommt. Das Gewicht
    // `"fuel" => 1.0` in `aggregate_master_score` bleibt als ruhender
    // Eintrag stehen, wie bei `loadsheet` und `flare`.
    //
    // `sub_fuel.rs` selbst bleibt: Altbestand-Datensaetze tragen die Achse
    // in ihren gespeicherten `sub_scores`, und die Tests dort dokumentieren
    // die Regeln der Fassungen 13–15.

    // ⚠ `sub_loadsheet` wird NICHT MEHR bewertet (v1.7.12).
    //
    // # Warum die Achse raus ist
    //
    // Sie verglich das geplante Abfluggewicht — und darauf hat der Pilot
    // keinen Einfluss. Das Gewicht steht fest, bevor der Flug beginnt: Es
    // kommt aus dem OFP, und der Simulator laedt es meist automatisch.
    // Eine Achse, die etwas bewertet, das man nicht beeinflussen kann,
    // ist keine Bewertung.
    //
    // (Thomas, 30.08.2026: „TOW und Gewichte — gehoert das in eine
    // Landebewertung? Das kann ich doch nicht beeinflussen.")
    //
    // ⚠ Dazu kam, dass sie faktisch nie etwas aussagte: Sie gab seit
    // Phase 2 KONSTANT 100 zurueck, sobald geplantes ZFW und TOW
    // vorlagen — ein Platzhalter, der nie zu Ende gebaut wurde. Das war
    // nicht neutral: Eine Achse mit konstant 100 und Gewicht 1 zieht
    // JEDEN Score nach oben (bei sonst 80 Punkten auf 81,25) und
    // verwaessert die Achsen, die wirklich messen.
    //
    // Der Sprit bleibt dagegen drin: Wie schnell geflogen wird, auf
    // welcher Hoehe, ob Umwege genommen werden — das entscheidet der
    // Pilot sehr wohl.
    //
    // Die Zahlen selbst bleiben im Payload und in der Anzeige erhalten;
    // sie sind interessant, nur eben keine Note. Der Test
    // `compute_sub_scores_never_emits_loadsheet` haelt das fest.

    out
}

/// Master-Score-Aggregation — gewichteter Mittelwert. Spec §5.5
/// (P1.4-B): bestehende Gewichte beibehalten, skipped Sub-Scores
/// aus Summe UND Gewichtssumme entfernen.
///
/// 1:1-Spiegel von TS `aggregateSubScores` in landingScoring.ts:292-309
/// plus Phase-1-Erweiterung um neue Sub-Score-Schluessel "loadsheet"
/// (Gewicht 1) und "flare" (Gewicht 1) — siehe Spec §5.5 Tabelle.
pub fn aggregate_master_score(subs: &[SubScoreEntry]) -> Option<u8> {
    let mittel = gewichtetes_mittel(subs)?;
    let abzug =
        landungs_abzug(subs).map_or(0, |(_, p)| p) + anflug_abzug(subs).map_or(0, |(_, p)| p);
    Some(mittel.saturating_sub(abzug))
}

/// Gewichteter Mittelwert aller bewerteten Teile, OHNE Deckel. `None` ohne
/// Sinkrate (siehe unten). Getrennt, damit [`master_deckel_wirksam`] den
/// Mittelwert direkt mit der Obergrenze vergleichen kann.
fn gewichtetes_mittel(subs: &[SubScoreEntry]) -> Option<u8> {
    if subs.is_empty() {
        return None;
    }
    // v0.20.2: Ohne Sinkrate gibt es KEINE Landungsbewertung.
    //
    // `skipped` fliegt aus Summe UND Gewichtssumme — und die Sinkrate traegt das
    // hoechste Gewicht (3). Fehlt sie, wurde der Master trotzdem gebildet, nur
    // eben aus den uebrigen Achsen. Ein Flug ganz ohne Messwerte kam so auf
    // **100 Punkte**, allein weil "keine Hopser" als perfekt zaehlt (der
    // Goldenset-Test `empty_input_returns_only_bounces_loadsheet_fuel` hat genau
    // das festgeschrieben).
    //
    // Seit die Kanonik unplausible Landeraten verwirft, ist das kein
    // theoretischer Fall mehr: ein Glitch-Flug haette eine glaenzende Note
    // bekommen, gerade WEIL seine Landung nicht messbar war. Eine
    // Landungsbewertung ohne die Sinkrate der Landung ist keine Bewertung —
    // dann lieber gar keine Note als eine geschenkte.
    let has_landing_rate = subs.iter().any(|s| s.key == "landing_rate" && !s.skipped);
    if !has_landing_rate {
        return None;
    }
    let mut sum: f32 = 0.0;
    let mut wsum: f32 = 0.0;
    for s in subs {
        if s.skipped {
            continue; // Skip aus Summe UND Gewichtssumme
        }
        let w = match s.key.as_str() {
            "landing_rate" => 3.0,
            "g_force" => 3.0,
            "bounces" => 2.0,
            "stability" => 2.0,
            "rollout" => 1.0,
            // v1.6.2: Gewicht 1 wie Bremsweg/Sprit/Ladepapiere. Bewusst
            // NICHT hoeher: die Achse trifft nur 2,5 % der Landungen
            // ueberhaupt, soll dort aber spuerbar sein. Explizit gelistet,
            // damit sie nicht still ueber den `_`-Default mitlaeuft.
            "alignment" => 1.0,
            // v1.7.0: Aufsetzpunkt. Gewicht 1 — bewusst konservativ, obwohl
            // die Achse fachlich mehr wiegt als der Bremsweg. Grund: Sie
            // ersetzt eine Bewertung, die bisher INDIREKT ueber `rollout`
            // (Gewicht 1) lief. Mit Gewicht 1 verschiebt sich der Gesamtscore
            // fuer eine unveraenderte Landung minimal; mit 2 oder 3 waere der
            // Umbau ein Bruch fuer jeden Piloten. Anheben ist jederzeit
            // moeglich — dann aber bewusst und ueber den Korpus nachgerechnet.
            // Explizit gelistet, damit sie nicht still ueber den `_`-Default
            // mitlaeuft.
            "touchdown_point" => 1.0,
            // 05.10.2026: Abfangen — Gewicht 2 (Entscheidung Thomas). Mit
            // dem Aufsetzpunkt zusammen kostet langes Schweben damit etwa so
            // viel, wie die weichere Sinkrate (Gewicht 3) einbringt.
            "abfangen" => 2.0,
            "fuel" => 1.0,
            // ⚠ Gewicht bleibt stehen, die Achse wird aber nicht mehr
            // ausgegeben (v1.7.12, siehe `compute_sub_scores`). Gleiches
            // Muster wie `flare`: Ein Waechter stellt sicher, dass das
            // ruhende Gewicht nicht still wieder aktiv wird.
            "loadsheet" => 1.0,
            "flare" => 1.0, // NEU v0.7.1
            _ => 1.0,       // unbekannt → default 1
        };
        sum += s.score as f32 * w;
        wsum += w;
    }
    if wsum > 0.0 {
        Some((sum / wsum).round() as u8)
    } else {
        None
    }
}

/// Die Abzuege, die die Gesamtnote bestimmt haben, als Kennungen mit
/// Endung `_abzug`, verbunden mit `+` (Landung zuerst, dann Anflug) — z. B.
/// `vor_der_schwelle_abzug+anflug_unstable_abzug`. `None` ohne Abzug oder
/// ohne Gesamtnote. Feld `landing_score_deckel` im Datensatz.
///
/// Name aus der Zeit der Deckel (Score-Version 18–20) — alte Datensaetze
/// tragen dort noch Deckel-Kennungen ohne `_abzug`.
pub fn master_deckel_wirksam(subs: &[SubScoreEntry]) -> Option<String> {
    gewichtetes_mittel(subs)?;
    let teile: Vec<String> = [landungs_abzug(subs), anflug_abzug(subs)]
        .into_iter()
        .flatten()
        .map(|(grund, _)| format!("{grund}_abzug"))
        .collect();
    (!teile.is_empty()).then(|| teile.join("+"))
}

/// Ab dieser G-Last gilt die Landung als hart ([`ABZUG_HART_PUNKTE`]).
/// Schwelle uebernommen aus vmsACARS 3 (`LandingScorer`, 1,75 g / 2,6 g).
pub const DECKEL_HART_G: f32 = 1.75;
/// Ab dieser G-Last Ueberlast ([`ABZUG_UEBERLAST_PUNKTE`]).
pub const DECKEL_UEBERLAST_G: f32 = 2.6;

/// Score-Version 21 (06.10.2026, Thomas: „die Deckel sind zu hart, eher
/// Abzug von den Punkten"): Statt die Gesamtnote auf eine feste Zahl zu
/// setzen, zieht jeder Grund Punkte vom gewichteten Mittel ab. Der feste
/// Deckel machte jede Landung mit demselben Fehler gleich (Korpus 60 Tage:
/// 14 gefaehrliche Ereignisse alle exakt 40, 58 gute Landungen nach
/// instabilem Anflug alle exakt 45); mit dem Abzug zaehlt der Rest der
/// Landung weiter.
///
/// Von den Gruenden der LANDUNG zaehlt nur der groesste — eine harte
/// Landung vor der Schwelle ist ein Ereignis, nicht drei (gestapelt kam
/// LNK861 auf 0). Der Anflug-Abzug kommt dazu.
pub const ABZUG_UEBERLAST_PUNKTE: u8 = 50;
pub const ABZUG_HART_PUNKTE: u8 = 25;
/// Vor der Schwelle, Overrun, Raeder neben der Bahn, zwei oder mehr Hopser.
pub const ABZUG_GEFAHR_PUNKTE: u8 = 30;
/// Schwaechster Teil „schlecht" (unter 45 Punkten).
pub const ABZUG_TEIL_SCHLECHT_PUNKTE: u8 = 10;
/// Schwaechster Teil „mittel" (45–74 Punkte).
pub const ABZUG_TEIL_MITTEL_PUNKTE: u8 = 5;

pub const GEFAHR_VOR_DER_SCHWELLE: &str = "vor_der_schwelle";
pub const GEFAHR_OVERRUN: &str = "overrun";
pub const GEFAHR_NEBEN_DER_BAHN: &str = "neben_der_bahn";
pub const GEFAHR_MEHRFACH_HOPSER: &str = "mehrfach_hopser";

/// Der groesste Abzug aus der Landung selbst: G-Last, gefaehrliches
/// Ereignis, schwaechster Teil. Bei Gleichstand diese Reihenfolge.
///
/// Massgeblich fuer die G-Last ist der Messwert der G-Teilnote (X-Plane dort
/// schon auf MSFS umgerechnet). Ohne bewertete G-Teilnote kein G-Abzug: Ein
/// fehlender Messwert darf nie eine Strafe ausloesen.
pub fn landungs_abzug(subs: &[SubScoreEntry]) -> Option<(&'static str, u8)> {
    let g_abzug = subs
        .iter()
        .find(|s| s.key == "g_force" && !s.skipped)
        .and_then(|s| s.messwert)
        .filter(|g| g.is_finite())
        .and_then(|g| {
            if g >= DECKEL_UEBERLAST_G {
                Some(("ueberlast", ABZUG_UEBERLAST_PUNKTE))
            } else if g >= DECKEL_HART_G {
                Some(("harte_landung", ABZUG_HART_PUNKTE))
            } else {
                None
            }
        });
    [
        g_abzug,
        gefahr_grund(subs).map(|g| (g, ABZUG_GEFAHR_PUNKTE)),
        teil_abzug(subs),
    ]
    .into_iter()
    .flatten()
    .reduce(|best, d| if d.1 > best.1 { d } else { best })
}

/// Vor der Schwelle aufgesetzt, Overrun, Raeder neben der Bahn, zwei oder
/// mehr Hopser. Erkannt an der Begruendung der Teilnote bzw. (Achsenablage)
/// am Messwert, nie an der Punktzahl allein.
fn gefahr_grund(subs: &[SubScoreEntry]) -> Option<&'static str> {
    let rat = |s: &SubScoreEntry| {
        s.rationale_key
            .as_deref()
            .and_then(|k| k.strip_prefix("landing.rat."))
            .unwrap_or("")
            .to_string()
    };
    subs.iter().filter(|s| !s.skipped).find_map(|s| {
        let grund = match (s.key.as_str(), rat(s).as_str()) {
            ("touchdown_point", "pre_threshold") => GEFAHR_VOR_DER_SCHWELLE,
            ("rollout", "overrun") => GEFAHR_OVERRUN,
            ("rollout", "off_pavement") => GEFAHR_NEBEN_DER_BAHN,
            ("alignment", _) if s.messwert.is_some_and(|a| a > 1.0) => GEFAHR_NEBEN_DER_BAHN,
            ("bounces", "two_bounces" | "many_bounces") => GEFAHR_MEHRFACH_HOPSER,
            _ => return None,
        };
        Some(grund)
    })
}

/// Der schwaechste Teil: unter 45 Punkten [`ABZUG_TEIL_SCHLECHT_PUNKTE`],
/// unter 75 [`ABZUG_TEIL_MITTEL_PUNKTE`] (die Stabilitaetsachse nur ohne
/// Anflug-Abzug). Die Kennung nennt den Teil
/// (`teil_mittel_touchdown_point`), damit der Bericht sagen kann, WAS
/// Punkte gekostet hat.
fn teil_abzug(subs: &[SubScoreEntry]) -> Option<(&'static str, u8)> {
    // Traegt der Anflug schon einen Abzug, zaehlt die Stabilitaetsachse hier
    // nicht mit — sonst kostete derselbe Anflug zweimal (RYR73: Achse 45 als
    // schwaechster Teil −5 plus UNSTABLE −15).
    let anflug_schon = anflug_abzug(subs).is_some();
    let schwaechster = subs
        .iter()
        .filter(|s| !s.skipped && s.score < 75)
        .filter(|s| !(anflug_schon && s.key == "stability"))
        .min_by_key(|s| s.score)?;
    let schlecht = schwaechster.score < 45;
    let grund = match (schlecht, schwaechster.key.as_str()) {
        (true, "landing_rate") => "teil_schlecht_landing_rate",
        (true, "g_force") => "teil_schlecht_g_force",
        (true, "bounces") => "teil_schlecht_bounces",
        (true, "stability") => "teil_schlecht_stability",
        (true, "rollout") => "teil_schlecht_rollout",
        (true, "alignment") => "teil_schlecht_alignment",
        (true, "touchdown_point") => "teil_schlecht_touchdown_point",
        (true, "abfangen") => "teil_schlecht_abfangen",
        (true, _) => "teil_schlecht",
        (false, "landing_rate") => "teil_mittel_landing_rate",
        (false, "g_force") => "teil_mittel_g_force",
        (false, "bounces") => "teil_mittel_bounces",
        (false, "stability") => "teil_mittel_stability",
        (false, "rollout") => "teil_mittel_rollout",
        (false, "alignment") => "teil_mittel_alignment",
        (false, "touchdown_point") => "teil_mittel_touchdown_point",
        (false, "abfangen") => "teil_mittel_abfangen",
        (false, _) => "teil_mittel",
    };
    Some((
        grund,
        if schlecht {
            ABZUG_TEIL_SCHLECHT_PUNKTE
        } else {
            ABZUG_TEIL_MITTEL_PUNKTE
        },
    ))
}

/// Marke der Stabilitaetsachse: Anflug im Gate nicht gemessen.
pub const ANFLUG_NICHT_GEMESSEN: &str = "anflug_nicht_gemessen";
/// Kennungen der Gesamtnoten-Deckel in Score-Version 18–20 (PARTIAL ≤ 80,
/// UNSTABLE ≤ 45). Werden nicht mehr vergeben; alte Datensaetze tragen sie
/// noch, ihr Text muss stimmen bleiben.
pub const ANFLUG_PARTIAL_GESAMT: &str = "anflug_partial_gesamt";
pub const ANFLUG_UNSTABLE_GESAMT: &str = "anflug_unstable_gesamt";

/// Anflug nicht STABLE: PARTIAL −5, UNSTABLE −15; ohne Gate-Messung −3
/// (ohne Messung keine Bestnote, aber auch nicht nachweislich schlecht).
/// Die Stabilitaetsachse selbst bleibt zusaetzlich begrenzt (≤ 80 / ≤ 45).
pub const ABZUG_ANFLUG_PARTIAL_PUNKTE: u8 = 5;
pub const ABZUG_ANFLUG_UNSTABIL_PUNKTE: u8 = 15;
pub const ABZUG_ANFLUG_OFFEN_PUNKTE: u8 = 3;

/// Marke der Stabilitaetsachse (bei uebersprungener Achse der Grund).
fn anflug_marke(subs: &[SubScoreEntry]) -> Option<&str> {
    let stab = subs.iter().find(|s| s.key == "stability")?;
    if stab.skipped {
        stab.reason.as_deref()
    } else {
        stab.warning.as_deref()
    }
}

/// Abzug fuer den Anflug (Score-Version 21).
pub fn anflug_abzug(subs: &[SubScoreEntry]) -> Option<(&'static str, u8)> {
    match anflug_marke(subs)? {
        "anflug_partial" => Some(("anflug_partial", ABZUG_ANFLUG_PARTIAL_PUNKTE)),
        "anflug_unstable" => Some(("anflug_unstable", ABZUG_ANFLUG_UNSTABIL_PUNKTE)),
        "anflug_nicht_gemessen" => Some((ANFLUG_NICHT_GEMESSEN, ABZUG_ANFLUG_OFFEN_PUNKTE)),
        _ => None,
    }
}

/// Landing-Kategorie-Klassifikation — Spiegel von TS `classifyLanding`
/// in landingScoring.ts:280-290. Wird vom Backend lib.rs `LandingScore::
/// classify` parallel verwendet (siehe lib.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LandingCategory {
    Smooth,
    Acceptable,
    Firm,
    Hard,
    Severe,
}

impl LandingCategory {
    pub fn numeric(self) -> u8 {
        match self {
            LandingCategory::Smooth => 100,
            LandingCategory::Acceptable => 80,
            LandingCategory::Firm => 60,
            LandingCategory::Hard => 30,
            LandingCategory::Severe => 0,
        }
    }

    fn order(self) -> u8 {
        match self {
            LandingCategory::Smooth => 0,
            LandingCategory::Acceptable => 1,
            LandingCategory::Firm => 2,
            LandingCategory::Hard => 3,
            LandingCategory::Severe => 4,
        }
    }

    fn worse_of(a: LandingCategory, b: LandingCategory) -> LandingCategory {
        if a.order() >= b.order() {
            a
        } else {
            b
        }
    }

    fn bump_up(self) -> LandingCategory {
        match self {
            LandingCategory::Smooth => LandingCategory::Acceptable,
            LandingCategory::Acceptable => LandingCategory::Firm,
            LandingCategory::Firm => LandingCategory::Hard,
            LandingCategory::Hard | LandingCategory::Severe => LandingCategory::Severe,
        }
    }
}

fn classify_by_vs(peak_vs_fpm: f32) -> LandingCategory {
    let vs = peak_vs_fpm.abs();
    if vs >= sub_landing_rate::T_VS_SEVERE_FPM {
        LandingCategory::Severe
    } else if vs >= sub_landing_rate::T_VS_HARD_FPM {
        LandingCategory::Hard
    } else if vs >= sub_landing_rate::T_VS_FIRM_FPM {
        LandingCategory::Firm
    } else if vs >= sub_landing_rate::T_VS_SMOOTH_FPM {
        LandingCategory::Acceptable
    } else {
        LandingCategory::Smooth
    }
}

fn classify_by_g(peak_g: f32) -> LandingCategory {
    if peak_g >= sub_g_force::T_G_SEVERE {
        LandingCategory::Severe
    } else if peak_g >= sub_g_force::T_G_HARD {
        LandingCategory::Hard
    } else if peak_g >= sub_g_force::T_G_FIRM {
        LandingCategory::Firm
    } else if peak_g >= sub_g_force::T_G_SMOOTH {
        LandingCategory::Acceptable
    } else {
        LandingCategory::Smooth
    }
}

pub fn classify_landing(peak_vs_fpm: f32, peak_g: Option<f32>, bounces: u32) -> LandingCategory {
    let by_vs = classify_by_vs(peak_vs_fpm);
    let by_g = peak_g.map(classify_by_g).unwrap_or(LandingCategory::Smooth);
    // v0.7.17 (B-009): V/S-led classification.
    //
    // Vorher: `worse_of(by_vs, by_g)` — fuehrte zu falschen „Severe"-
    // Klassifikationen bei butterweichen Touchdowns, sobald die Sim-
    // Strut-Compression einen einzelnen G-Spike >2.10 produzierte.
    // Echtes Pilot-Performance-Signal ist die vertikale Sinkrate (V/S)
    // — das was der Pilot beim Aufsetzen fuehlt und was Wartungs-
    // bestimmungen tatsaechlich definieren (-600 fpm = "Hard").
    //
    // Neue Logik: V/S fuehrt. G darf die Klassifikation maximal um
    // EINE Stufe verschlechtern (= Indikator fuer ungewohnt harte
    // Strut-Compression), aber nie alleine zu Severe fuehren bei
    // smoother V/S. Bei echten Hard-Impacts (V/S >= -600 fpm)
    // dominiert das schlechtere Signal wie bisher.
    let mut cat = match (by_vs, by_g) {
        // V/S = Smooth: G kann maximal um eine Stufe runter
        (LandingCategory::Smooth, LandingCategory::Hard)
        | (LandingCategory::Smooth, LandingCategory::Severe) => LandingCategory::Acceptable,
        (LandingCategory::Smooth, _) => LandingCategory::Smooth,

        // V/S = Acceptable: G kann maximal um eine Stufe runter
        (LandingCategory::Acceptable, LandingCategory::Hard)
        | (LandingCategory::Acceptable, LandingCategory::Severe) => LandingCategory::Firm,
        (LandingCategory::Acceptable, _) => LandingCategory::Acceptable,

        // V/S = Firm: G kann maximal um eine Stufe runter
        (LandingCategory::Firm, LandingCategory::Severe) => LandingCategory::Hard,
        (LandingCategory::Firm, _) => LandingCategory::Firm,

        // V/S = Hard/Severe: echter Impact, worse_of greift wie vorher
        (LandingCategory::Hard, _) | (LandingCategory::Severe, _) => {
            LandingCategory::worse_of(by_vs, by_g)
        }
    };
    if bounces > 0 && cat != LandingCategory::Severe {
        cat = cat.bump_up();
    }
    cat
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v0.12.3 (LE8): `sub_g_force` scores `scored_g_load` when present,
    /// else falls back to the raw `peak_g_load`.
    #[test]
    fn sub_g_force_uses_scored_g() {
        let g_value = |input: &LandingScoringInput| -> String {
            compute_sub_scores(input)
                .into_iter()
                .find(|s| s.key == "g_force")
                .and_then(|s| s.value)
                .expect("g_force sub-score present")
        };
        // scored_g present → it is scored, not the raw peak.
        let with_scored = LandingScoringInput {
            peak_g_load: Some(1.95),
            scored_g_load: Some(1.78),
            ..Default::default()
        };
        assert_eq!(g_value(&with_scored), "1.78 G");
        // No scored_g (pre-v0.12.3 caller) → falls back to the raw peak.
        let legacy = LandingScoringInput {
            peak_g_load: Some(1.95),
            scored_g_load: None,
            ..Default::default()
        };
        assert_eq!(g_value(&legacy), "1.95 G");
    }

    #[test]
    fn band_thresholds_match_ts() {
        assert_eq!(band_from_points(100), Band::Good);
        assert_eq!(band_from_points(75), Band::Good);
        assert_eq!(band_from_points(74), Band::Ok);
        assert_eq!(band_from_points(45), Band::Ok);
        assert_eq!(band_from_points(44), Band::Bad);
        assert_eq!(band_from_points(0), Band::Bad);
    }

    /// Lernpaket AP2: Landung mit sehr guten Nebenachsen, aber hartem
    /// Aufsetzen. Ohne Deckel kaeme der Mittelwert auf 70.
    fn harte_landung_mit_guten_nebenachsen(g: f32) -> Vec<SubScoreEntry> {
        vec![
            crate::sub_landing_rate::sub_landing_rate(300.0),
            crate::sub_g_force::sub_g_force(g),
            SubScoreEntry::scored("bounces", "l", 100, "0".into(), "clean_set", Band::Good),
            SubScoreEntry::scored("stability", "l", 100, "-".into(), "clean_set", Band::Good),
            SubScoreEntry::scored("rollout", "l", 100, "-".into(), "clean_set", Band::Good),
            SubScoreEntry::scored(
                "touchdown_point",
                "l",
                100,
                "-".into(),
                "clean_set",
                Band::Good,
            ),
        ]
    }

    #[test]
    fn abzug_ab_175_g() {
        // 1,74 g ist keine harte Landung — der schwache G-Teil (20 Punkte,
        // „schlecht") kostet aber 10.
        let unter = harte_landung_mit_guten_nebenachsen(1.74);
        assert_eq!(
            landungs_abzug(&unter),
            Some(("teil_schlecht_g_force", ABZUG_TEIL_SCHLECHT_PUNKTE)),
            "1,74 g = kein G-Abzug"
        );
        let mittel = gewichtetes_mittel(&unter).unwrap();
        assert_eq!(aggregate_master_score(&unter), Some(mittel - 10));

        let hart = harte_landung_mit_guten_nebenachsen(DECKEL_HART_G);
        assert_eq!(
            landungs_abzug(&hart),
            Some(("harte_landung", ABZUG_HART_PUNKTE))
        );
        let mittel = gewichtetes_mittel(&hart).unwrap();
        assert_eq!(aggregate_master_score(&hart), Some(mittel - 25));
    }

    #[test]
    fn abzug_ueberlast_ab_26_g() {
        let subs = harte_landung_mit_guten_nebenachsen(DECKEL_UEBERLAST_G);
        assert_eq!(
            landungs_abzug(&subs),
            Some(("ueberlast", ABZUG_UEBERLAST_PUNKTE))
        );
        let mittel = gewichtetes_mittel(&subs).unwrap();
        assert_eq!(
            aggregate_master_score(&subs),
            Some(mittel.saturating_sub(50))
        );
    }

    #[test]
    fn abzug_nie_unter_null_und_nur_mit_messwert() {
        let mut schlecht = harte_landung_mit_guten_nebenachsen(2.0);
        for s in schlecht.iter_mut() {
            s.score = 0;
        }
        assert_eq!(aggregate_master_score(&schlecht), Some(0));

        // Ohne Messwert (alte Payloads) kein G-Abzug — ein fehlender Wert
        // darf nie strafen; es bleibt der schwache G-Teil.
        let mut ohne = harte_landung_mit_guten_nebenachsen(2.0);
        ohne[1].messwert = None;
        assert_eq!(
            landungs_abzug(&ohne),
            Some(("teil_schlecht_g_force", ABZUG_TEIL_SCHLECHT_PUNKTE))
        );

        // Uebersprungene G-Teilnote → kein Abzug.
        let mut skip = harte_landung_mit_guten_nebenachsen(2.0);
        skip[1] = SubScoreEntry::skipped("g_force", "l", "insufficient_samples");
        assert_eq!(landungs_abzug(&skip), None);
    }

    #[test]
    fn grund_nennt_den_abzug() {
        let hart = harte_landung_mit_guten_nebenachsen(1.9);
        assert_eq!(
            master_deckel_wirksam(&hart).as_deref(),
            Some("harte_landung_abzug")
        );
        let mut sauber = harte_landung_mit_guten_nebenachsen(1.2);
        sauber[0].score = 100;
        sauber[1].score = 100;
        assert_eq!(master_deckel_wirksam(&sauber), None);
    }

    fn alles_hundert_mit_stabilitaet(stab: SubScoreEntry) -> Vec<SubScoreEntry> {
        let mk =
            |k: &str| SubScoreEntry::scored(k, "l", 100, "x".into(), "very_stable", Band::Good);
        vec![
            mk("landing_rate"),
            mk("g_force"),
            mk("bounces"),
            stab,
            mk("rollout"),
        ]
    }

    /// QAF434 (05.10.2026): Anflug PARTIAL, Achsen wie im PIREP —
    /// Stabilitaet 80, Aufsetzpunkt 85, Rest 100. Unter Score-Version 17
    /// ergab das 96, unter 18–20 der Deckel 80; seit 21 kostet PARTIAL
    /// 5 Punkte: 91.
    #[test]
    fn qaf434_teilweise_stabil_kostet_fuenf_punkte() {
        use crate::anflug_urteil::AnflugUrteil;
        let mk = |k: &str, p: u8| {
            SubScoreEntry::scored(k, "l", p, "x".into(), "very_stable", Band::Good)
        };
        let stab =
            sub_stability::sub_stability_legacy(Some(50.0), Some(0.5), Some(AnflugUrteil::Partial))
                .unwrap();
        assert_eq!(stab.score, 80);
        let subs = vec![
            mk("landing_rate", 100),
            mk("g_force", 100),
            mk("bounces", 100),
            stab,
            mk("rollout", 100),
            mk("alignment", 100),
            mk("touchdown_point", 85),
        ];
        // Gegenprobe: ohne Deckel kaeme das gewichtete Mittel ueber 80.
        let mut ohne = subs.clone();
        ohne[3].warning = None;
        assert_eq!(aggregate_master_score(&ohne), Some(96));
        assert_eq!(aggregate_master_score(&subs), Some(91));
        assert_eq!(
            master_deckel_wirksam(&subs).as_deref(),
            Some("anflug_partial_abzug")
        );
    }

    /// Lag die Stabilitaetsachse schon unter dem Urteilsdeckel (hier 50 aus
    /// der Querneigung), muss der Abzug trotzdem greifen — vorher setzte die
    /// Achse die Marke nur, wenn sie selbst gesenkt wurde.
    #[test]
    fn abzug_auch_wenn_die_achse_schon_niedriger_war() {
        use crate::anflug_urteil::AnflugUrteil;
        let stab =
            sub_stability::sub_stability_legacy(Some(50.0), Some(7.0), Some(AnflugUrteil::Partial))
                .unwrap();
        assert!(stab.score < 80, "Achse {}", stab.score);
        assert_eq!(stab.warning.as_deref(), Some("anflug_partial"));
        let subs = alles_hundert_mit_stabilitaet(stab);
        assert_eq!(anflug_abzug(&subs), Some(("anflug_partial", 5)));
        // Der Abzug geht vom Mittel ab; die schwache Achse (50) kostet nicht
        // noch einmal als „schwaechster Teil".
        assert_eq!(landungs_abzug(&subs), None);
        let mittel = gewichtetes_mittel(&subs).unwrap();
        assert_eq!(aggregate_master_score(&subs), Some(mittel - 5));
    }

    /// Score-Version 21: Der Abzug wirkt auch bei mittelmaessiger Landung
    /// (anders als der fruehere Deckel 80, der unter 80 nichts aenderte) und
    /// wird dann als Grund gemeldet.
    #[test]
    fn abzug_wirkt_auch_unter_achtzig() {
        use crate::anflug_urteil::AnflugUrteil;
        let stab =
            sub_stability::sub_stability_legacy(Some(50.0), Some(0.5), Some(AnflugUrteil::Partial))
                .unwrap();
        let mut subs = alles_hundert_mit_stabilitaet(stab);
        // Alle anderen Teile knapp „gut" (75): Mittel < 80, und seit
        // Score-Version 19 greift auch kein Teil-Deckel dazwischen.
        for s in subs.iter_mut().filter(|s| s.key != "stability") {
            s.score = 75;
        }
        let mittel = gewichtetes_mittel(&subs).unwrap();
        assert!(mittel < 80);
        assert_eq!(aggregate_master_score(&subs), Some(mittel - 5));
        assert_eq!(landungs_abzug(&subs), None);
        assert_eq!(
            master_deckel_wirksam(&subs).as_deref(),
            Some("anflug_partial_abzug")
        );
    }

    /// Cloud-QS 05.10.2026: Urteil vorhanden, Streuung fehlt (eigene
    /// Stichprobe) — vorher „nicht gemessen" mit Deckel 97. Jetzt zaehlt das
    /// Urteil.
    #[test]
    fn urteil_ohne_streuung_deckelt_trotzdem() {
        let input = LandingScoringInput {
            vs_fpm: Some(-150.0),
            runway_length_m: Some(3000.0),
            anflug: anflug_urteil::AnflugWerte {
                stable_config: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };
        let subs = compute_sub_scores(&input);
        let stab = subs.iter().find(|s| s.key == "stability").unwrap();
        assert!(stab.skipped);
        assert_eq!(stab.reason.as_deref(), Some("anflug_partial"));
        assert_eq!(anflug_abzug(&subs), Some(("anflug_partial", 5)));
        // Gegenprobe: ganz ohne Urteil bleibt es „nicht gemessen".
        let ohne = LandingScoringInput {
            anflug: Default::default(),
            ..input
        };
        let subs = compute_sub_scores(&ohne);
        let stab = subs.iter().find(|s| s.key == "stability").unwrap();
        assert_eq!(stab.reason.as_deref(), Some(ANFLUG_NICHT_GEMESSEN));
    }

    /// Anflug nicht STABLE: Abzug (Score-Version 21), die Anzeige nennt den
    /// Grund. Alles andere perfekt: PARTIAL 96 − 5 = 91, UNSTABLE 90 − 15
    /// = 75 (Achse 45, zaehlt nicht nochmal als schwaechster Teil).
    #[test]
    fn nicht_stabiler_anflug_gibt_keine_hundert() {
        use crate::anflug_urteil::AnflugUrteil;
        for (u, note, grund) in [
            (AnflugUrteil::Partial, 91, "anflug_partial_abzug"),
            (AnflugUrteil::Unstable, 75, "anflug_unstable_abzug"),
        ] {
            let stab = sub_stability::sub_stability_legacy(Some(50.0), Some(0.5), Some(u)).unwrap();
            let subs = alles_hundert_mit_stabilitaet(stab);
            assert_eq!(aggregate_master_score(&subs), Some(note), "{grund}");
            assert_eq!(master_deckel_wirksam(&subs).as_deref(), Some(grund));
        }
        // STABLE: voll, kein Hinweis.
        let stab = sub_stability::sub_stability_legacy(
            Some(50.0),
            Some(0.5),
            Some(crate::anflug_urteil::AnflugUrteil::Stable),
        )
        .unwrap();
        let subs = alles_hundert_mit_stabilitaet(stab);
        assert_eq!(aggregate_master_score(&subs), Some(100));
        assert_eq!(master_deckel_wirksam(&subs), None);
    }

    /// RYR73 (06.10.2026): UNSTABLE nach Score-Version 18–20 = 45 trotz
    /// sauberer Landung. Jetzt bestimmt die Landung mit: Achsen wie im PIREP.
    #[test]
    fn ryr73_instabil_mit_guter_landung() {
        use crate::anflug_urteil::AnflugUrteil;
        let mk = |k: &str, p: u8| {
            SubScoreEntry::scored(k, "l", p, "x".into(), "very_stable", Band::Good)
        };
        let stab = sub_stability::sub_stability_legacy(
            Some(164.0),
            Some(0.8),
            Some(AnflugUrteil::Unstable),
        )
        .unwrap();
        assert_eq!(stab.score, 45);
        let subs = vec![
            mk("landing_rate", 80),
            mk("g_force", 80),
            mk("bounces", 100),
            stab,
            mk("rollout", 100),
            mk("alignment", 85),
            mk("touchdown_point", 85),
        ];
        // Mittel 80 − 15 = 65; die Achse (45) kostet nicht noch einmal.
        assert_eq!(gewichtetes_mittel(&subs), Some(80));
        assert_eq!(aggregate_master_score(&subs), Some(65));
        assert_eq!(
            master_deckel_wirksam(&subs).as_deref(),
            Some("anflug_unstable_abzug")
        );
    }

    /// Landungs- und Anflug-Abzug addieren sich; beide werden genannt.
    #[test]
    fn harte_landung_und_anflug_addieren_sich() {
        use crate::anflug_urteil::AnflugUrteil;
        let stab = sub_stability::sub_stability_legacy(
            Some(50.0),
            Some(0.5),
            Some(AnflugUrteil::Unstable),
        )
        .unwrap();
        let mut subs = alles_hundert_mit_stabilitaet(stab);
        let g = subs.iter_mut().find(|s| s.key == "g_force").unwrap();
        g.messwert = Some(1.9);
        let mittel = gewichtetes_mittel(&subs).unwrap();
        assert_eq!(aggregate_master_score(&subs), Some(mittel - 25 - 15));
        assert_eq!(
            master_deckel_wirksam(&subs).as_deref(),
            Some("harte_landung_abzug+anflug_unstable_abzug")
        );
    }

    /// Ohne Gate-Messung keine Bestnote — mit und ohne Streuungswerte.
    #[test]
    fn ohne_anflugmessung_keine_bestnote() {
        let skip = SubScoreEntry::skipped("stability", "l", ANFLUG_NICHT_GEMESSEN);
        let subs = alles_hundert_mit_stabilitaet(skip);
        assert_eq!(aggregate_master_score(&subs), Some(97));
        assert_eq!(
            master_deckel_wirksam(&subs).as_deref(),
            Some("anflug_nicht_gemessen_abzug")
        );

        let stab = sub_stability::sub_stability_legacy(Some(50.0), Some(0.5), None).unwrap();
        let subs = alles_hundert_mit_stabilitaet(stab);
        assert_eq!(aggregate_master_score(&subs), Some(97));

        // Auch bei schwaecherer Landung kostet die fehlende Messung 3.
        let mut schlecht = alles_hundert_mit_stabilitaet(SubScoreEntry::skipped(
            "stability",
            "l",
            ANFLUG_NICHT_GEMESSEN,
        ));
        // Alle Teile 90: Mittel unter 97, kein Teil-Deckel (Score-Version 19).
        for s in schlecht.iter_mut().filter(|s| !s.skipped) {
            s.score = 90;
        }
        assert_eq!(aggregate_master_score(&schlecht), Some(87));
    }

    // ── Score-Version 19: „schwaechster Teil begrenzt" ─────────────────

    /// Gute Landung, in der genau ein Teil ersetzt wird.
    fn gute_landung_mit(teil: SubScoreEntry) -> Vec<SubScoreEntry> {
        let mk = |k: &str| SubScoreEntry::scored(k, "l", 100, "x".into(), "clean_set", Band::Good);
        let mut v = vec![
            mk("landing_rate"),
            mk("g_force"),
            mk("bounces"),
            mk("stability"),
            mk("rollout"),
            mk("alignment"),
            mk("touchdown_point"),
        ];
        let i = v.iter().position(|s| s.key == teil.key).unwrap();
        v[i] = teil;
        v
    }

    fn teil(key: &str, punkte: u8, begruendung: &str) -> SubScoreEntry {
        SubScoreEntry::scored(
            key,
            "l",
            punkte,
            "x".into(),
            begruendung,
            band_from_points(punkte),
        )
    }

    /// QAF419 (24.09.2026): vor der Schwelle aufgesetzt, sonst sauber — war
    /// 92, Score-Version 19–20 Deckel 40, seit 21 −30.
    #[test]
    fn qaf419_vor_der_schwelle_kostet_dreissig() {
        let subs = gute_landung_mit(teil("touchdown_point", 0, "pre_threshold"));
        let mittel = gewichtetes_mittel(&subs).unwrap();
        assert!(mittel >= 90, "ohne Abzug ~92");
        assert_eq!(aggregate_master_score(&subs), Some(mittel - 30));
        assert_eq!(
            master_deckel_wirksam(&subs).as_deref(),
            Some("vor_der_schwelle_abzug")
        );
    }

    /// AIB424 (21.09.2026): Raeder neben der Bahn beim Ausrollen — 91.
    #[test]
    fn aib424_neben_der_bahn_kostet_dreissig() {
        // Der Ausroll-Teil (20) ist auch „schlecht" (−10) — es zaehlt nur der
        // groessere Abzug.
        let subs = gute_landung_mit(teil("rollout", 20, "off_pavement"));
        let mittel = gewichtetes_mittel(&subs).unwrap();
        assert_eq!(aggregate_master_score(&subs), Some(mittel - 30));
        assert_eq!(
            master_deckel_wirksam(&subs).as_deref(),
            Some("neben_der_bahn_abzug")
        );
        let subs = gute_landung_mit(teil("rollout", 0, "overrun"));
        assert_eq!(
            master_deckel_wirksam(&subs).as_deref(),
            Some("overrun_abzug")
        );
    }

    /// Achsenablage: neben der Bahn erkannt am Messwert (Anteil > 1), auch
    /// wenn die Begruendung „schraeg" lautet.
    #[test]
    fn achsenablage_neben_der_bahn_am_messwert() {
        let schraeg = teil("alignment", 15, "crooked_touchdown").mit_messwert(1.3);
        let subs = gute_landung_mit(schraeg);
        assert_eq!(landungs_abzug(&subs), Some((GEFAHR_NEBEN_DER_BAHN, 30)));
        // Gegenprobe: am Rand, aber auf der Bahn (Anteil 0,9) → „schlecht" −10.
        let rand = teil("alignment", 40, "off_centerline").mit_messwert(0.9);
        let subs = gute_landung_mit(rand);
        assert_eq!(landungs_abzug(&subs), Some(("teil_schlecht_alignment", 10)));
    }

    /// Ab zwei Hopsern gefaehrlich, ein Hopser ist „mittel".
    #[test]
    fn hopser_ab_zwei_gefaehrlich() {
        let zwei = gute_landung_mit(teil("bounces", 40, "two_bounces"));
        assert_eq!(landungs_abzug(&zwei), Some((GEFAHR_MEHRFACH_HOPSER, 30)));
        let viele = gute_landung_mit(teil("bounces", 15, "many_bounces"));
        assert_eq!(landungs_abzug(&viele), Some((GEFAHR_MEHRFACH_HOPSER, 30)));
        let einer = gute_landung_mit(teil("bounces", 70, "one_bounce"));
        assert_eq!(landungs_abzug(&einer), Some(("teil_mittel_bounces", 5)));
    }

    /// GSG219 (21.09.2026): lang hinter der Aufsetzzone (55) — war 97,
    /// Score-Version 19–20 Deckel 80, seit 21 −5.
    #[test]
    fn gsg219_langer_aufsetzpunkt_kostet_fuenf() {
        let subs = gute_landung_mit(teil("touchdown_point", 55, "long_touchdown"));
        let mittel = gewichtetes_mittel(&subs).unwrap();
        assert_eq!(aggregate_master_score(&subs), Some(mittel - 5));
        assert_eq!(
            master_deckel_wirksam(&subs).as_deref(),
            Some("teil_mittel_touchdown_point_abzug")
        );
    }

    /// Der schwaechste Teil wird genannt; „gut" (ab 75) begrenzt nichts.
    #[test]
    fn schwaechster_teil_wird_genannt_gute_teile_begrenzen_nicht() {
        let mut subs = gute_landung_mit(teil("alignment", 65, "off_centerline"));
        subs[0] = teil("landing_rate", 45, "hard_landing");
        assert_eq!(landungs_abzug(&subs), Some(("teil_mittel_landing_rate", 5)));
        let alle_gut = gute_landung_mit(teil("touchdown_point", 85, "in_tdz"));
        assert_eq!(landungs_abzug(&alle_gut), None);
        let grenze = gute_landung_mit(teil("alignment", 75, "off_centerline"));
        assert_eq!(landungs_abzug(&grenze), None, "75 ist noch gut");
    }

    /// Score-Version 21: Schwache Stabilitaetsachse bei PARTIAL — der Anflug
    /// kostet einmal (−5), die Achse nicht noch als schwaechster Teil.
    #[test]
    fn schwache_achse_zaehlt_nicht_doppelt() {
        let stab = sub_stability::sub_stability_legacy(
            Some(50.0),
            Some(7.0),
            Some(crate::anflug_urteil::AnflugUrteil::Partial),
        )
        .unwrap();
        assert!(stab.score < 75);
        let subs = gute_landung_mit(stab);
        assert_eq!(landungs_abzug(&subs), None);
        let mittel = gewichtetes_mittel(&subs).unwrap();
        assert_eq!(aggregate_master_score(&subs), Some(mittel - 5));
        assert_eq!(
            master_deckel_wirksam(&subs).as_deref(),
            Some("anflug_partial_abzug")
        );
        // Gegenprobe: STABLE mit schwacher Streuungsachse — dann zaehlt sie.
        let mut stabil = subs.clone();
        let st = stabil.iter_mut().find(|s| s.key == "stability").unwrap();
        st.warning = None;
        assert_eq!(landungs_abzug(&stabil), Some(("teil_mittel_stability", 5)));
    }

    /// Landungs-Abzug (hart) und fehlende Anflugmessung addieren sich.
    #[test]
    fn g_abzug_und_fehlende_messung() {
        let mut subs = harte_landung_mit_guten_nebenachsen(1.9);
        subs.retain(|s| s.key != "stability");
        subs.push(SubScoreEntry::skipped(
            "stability",
            "l",
            ANFLUG_NICHT_GEMESSEN,
        ));
        assert_eq!(
            landungs_abzug(&subs),
            Some(("harte_landung", ABZUG_HART_PUNKTE))
        );
        let mittel = gewichtetes_mittel(&subs).unwrap();
        assert_eq!(aggregate_master_score(&subs), Some(mittel - 25 - 3));
    }

    /// Drehfluegler/Wasserflugzeug haben keine Stabilitaetsachse — dort darf
    /// „nicht gemessen" keine Bestnote sperren.
    #[test]
    fn nicht_konventionell_bekommt_keine_anflug_sperre() {
        let input = LandingScoringInput {
            vs_fpm: Some(-150.0),
            runway_length_m: Some(3000.0),
            nicht_konventionell: true,
            ..Default::default()
        };
        let subs = compute_sub_scores(&input);
        assert!(!subs.iter().any(|s| s.key == "stability"));
        let normal = LandingScoringInput {
            nicht_konventionell: false,
            ..input
        };
        let subs = compute_sub_scores(&normal);
        assert!(subs.iter().any(|s| s.key == "stability" && s.skipped));
    }

    /// QS 06.10.2026: Hubschrauber/Wasserflugzeug — Schwebeflug ist dort
    /// Verfahren, keine Abfang-Teilnote (und damit kein Deckel).
    #[test]
    fn nicht_konventionell_bekommt_keine_abfang_teilnote() {
        let abfangen = abfangen::Abfangen {
            dauer_ab_50ft_s: Some(40.0),
            max_vs_fpm: Some(-20.0),
            ..Default::default()
        };
        let heli = LandingScoringInput {
            vs_fpm: Some(-150.0),
            nicht_konventionell: true,
            abfangen: Some(abfangen),
            ..Default::default()
        };
        assert!(!compute_sub_scores(&heli)
            .iter()
            .any(|s| s.key == "abfangen"));
        // Gegenprobe: konventionell gibt es sie (40 s → 25 Punkte).
        let normal = LandingScoringInput {
            nicht_konventionell: false,
            ..heli
        };
        let s = compute_sub_scores(&normal);
        assert_eq!(
            s.iter().find(|s| s.key == "abfangen").map(|s| s.points),
            Some(25)
        );
    }

    /// QS 06.10.2026: Abfangen zählt mit Gewicht 2 (Entscheidung Thomas) —
    /// fiele die Zeile weg, griffe still der Standard 1.
    #[test]
    fn abfangen_zaehlt_mit_gewicht_zwei() {
        // Echte Schlüssel: der Text-Wächter des Frontends liest auch Rust-Tests.
        let teil = |key: &str, p: u8| {
            SubScoreEntry::scored(
                key,
                "landing.sub.abfangen",
                p,
                String::new(),
                "flare_normal",
                Band::Good,
            )
        };
        // 100×3 (Sinkrate) + 80×2 (Abfangen) = 460 / 5 = 92; mit Gewicht 1: 380/4 = 95.
        let subs = vec![teil("landing_rate", 100), teil("abfangen", 80)];
        assert_eq!(gewichtetes_mittel(&subs), Some(92));
    }

    /// Die Rangfolge der Landungs-Abzuege: Ueberlast > Gefahr > hart >
    /// schwaechster Teil — sonst gewinnt bei Gleichzeitigkeit der falsche.
    #[test]
    fn abzuege_in_der_richtigen_rangfolge() {
        let reihe = [
            ABZUG_UEBERLAST_PUNKTE,
            ABZUG_GEFAHR_PUNKTE,
            ABZUG_HART_PUNKTE,
            ABZUG_TEIL_SCHLECHT_PUNKTE,
            ABZUG_TEIL_MITTEL_PUNKTE,
        ];
        assert!(reihe.windows(2).all(|w| w[0] > w[1]), "{reihe:?}");
    }

    #[test]
    fn aggregate_master_uses_weighted_mean() {
        // landing_rate=100×3 + g_force=80×3 + bounces=70×2 + stability=50×2
        //   = 300 + 240 + 140 + 100 = 780
        // wsum = 3+3+2+2 = 10
        // → 78
        let subs = vec![
            SubScoreEntry::scored(
                "landing_rate",
                "landing.sub.landing_rate",
                100,
                "-50 fpm".into(),
                "smooth_touchdown",
                Band::Good,
            ),
            SubScoreEntry::scored(
                "g_force",
                "landing.sub.g_force",
                80,
                "1.30 G".into(),
                "comfortable_g",
                Band::Good,
            ),
            SubScoreEntry::scored(
                "bounces",
                "landing.sub.bounces",
                70,
                "1".into(),
                "one_bounce",
                Band::Ok,
            ),
            SubScoreEntry::scored(
                "stability",
                "landing.sub.stability",
                50,
                "σ 250 fpm / 4.0°".into(),
                "average_stability",
                Band::Ok,
            ),
        ];
        assert_eq!(gewichtetes_mittel(&subs), Some(78));
        // Schwaechster Teil „mittel" (50, Hopser 70): −5.
        assert_eq!(aggregate_master_score(&subs), Some(73));
    }

    #[test]
    fn aggregate_master_skips_skipped_subs() {
        // Skip senkt nicht den Master-Score
        let subs = vec![
            SubScoreEntry::scored(
                "landing_rate",
                "landing.sub.landing_rate",
                100,
                "-50 fpm".into(),
                "smooth_touchdown",
                Band::Good,
            ),
            SubScoreEntry::skipped("loadsheet", "landing.sub.loadsheet", "no_planned_zfw"),
        ];
        assert_eq!(aggregate_master_score(&subs), Some(100));
    }

    #[test]
    fn classify_landing_bumps_for_bounces() {
        // smooth + 1 bounce → acceptable
        let cat = classify_landing(-100.0, Some(1.10), 1);
        assert_eq!(cat, LandingCategory::Acceptable);
    }

    #[test]
    fn classify_landing_severe_does_not_bump() {
        // severe + bounce bleibt severe
        let cat = classify_landing(-1100.0, None, 1);
        assert_eq!(cat, LandingCategory::Severe);
    }

    #[test]
    fn classify_landing_vs_led_smooth_vs_severe_g() {
        // v0.7.17 (B-009): V/S-led classification.
        // Vorher worse_of(Smooth, Severe) = Severe.
        // Jetzt V/S Smooth fuehrt, G darf maximal 1 Stufe runter → Acceptable.
        let cat = classify_landing(-50.0, Some(2.20), 0);
        assert_eq!(cat, LandingCategory::Acceptable);
    }

    // v0.7.17 (B-009) — V/S-led classification tests
    #[test]
    fn b009_butter_landing_with_sim_strut_spike_stays_smooth() {
        // Real-Tester-Case GSG 105: VS -116 fpm (Smooth) + G 2.30 (Severe)
        // wegen Sim-Strut-Compression. Vorher: worse_of -> Severe.
        // Jetzt: Smooth gefuehrt von V/S, G nur 1-Stufen-Downgrade -> Acceptable.
        let cat = classify_landing(-116.0, Some(2.30), 0);
        assert_eq!(cat, LandingCategory::Acceptable);
    }

    #[test]
    fn b009_smooth_vs_with_hard_g_only_one_step_down() {
        let cat = classify_landing(-150.0, Some(1.80), 0);
        assert_eq!(cat, LandingCategory::Acceptable);
    }

    #[test]
    fn b009_smooth_vs_with_smooth_g_stays_smooth() {
        let cat = classify_landing(-100.0, Some(1.10), 0);
        assert_eq!(cat, LandingCategory::Smooth);
    }

    #[test]
    fn b009_real_hard_impact_still_classified_correctly() {
        // V/S Hard (-700 fpm), G Severe (2.50): worse_of greift, bleibt Severe
        let cat = classify_landing(-700.0, Some(2.50), 0);
        assert_eq!(cat, LandingCategory::Severe);
    }

    #[test]
    fn b009_acceptable_vs_with_severe_g_one_step_down() {
        // V/S Acceptable (-300 fpm), G Severe (2.30) -> Firm
        let cat = classify_landing(-300.0, Some(2.30), 0);
        assert_eq!(cat, LandingCategory::Firm);
    }

    #[test]
    fn b009_firm_vs_with_severe_g_one_step_down() {
        // V/S Firm (-500 fpm), G Severe (2.30) -> Hard
        let cat = classify_landing(-500.0, Some(2.30), 0);
        assert_eq!(cat, LandingCategory::Hard);
    }

    #[test]
    fn b009_bounces_still_bump_up_one_stufe() {
        // Smooth + bounce -> Acceptable (bump_up greift weiterhin)
        let cat = classify_landing(-100.0, Some(1.10), 1);
        assert_eq!(cat, LandingCategory::Acceptable);
    }

    /// v0.16.22 FIX 5 — dormant-flare-weight guardrail. `aggregate_master_
    /// score` carries a live `"flare" => 1.0` weight, but `compute_sub_
    /// scores` must NEVER emit a `"flare"` SubScoreEntry — the flare metric
    /// is forensic/coaching-only and stays OUT of the master score (Landing-
    /// Score is private; no scored flare ranking — see ACARS-focus policy).
    /// This test fails loudly if a future edit accidentally wires flare into
    /// the scored set, so the dormant weight can never silently activate.
    /// Run over a FULLY-populated input so every sub-score branch fires.
    #[test]
    fn compute_sub_scores_never_emits_loadsheet() {
        // ⚠ Die Ladepapier-Achse ist seit v1.7.12 stillgelegt: Sie
        // bewertete das geplante Abfluggewicht, auf das der Pilot keinen
        // Einfluss hat — und gab dabei konstant 100, was jeden Score
        // nach oben zog.
        //
        // Das Gewicht steht noch in der Tabelle. Dieser Waechter stellt
        // sicher, dass es nicht still wieder aktiv wird, wenn jemand die
        // Achse versehentlich wieder ausgibt.
        let rich = voll_besetzter_eingang();
        let subs = compute_sub_scores(&rich);
        assert!(
            !subs.iter().any(|s| s.key == "loadsheet"),
            "die Ladepapier-Achse wird wieder ausgegeben — sie bewertet \
             etwas, das der Pilot nicht beeinflussen kann"
        );
    }

    /// v1.7.35 — Sprit-Waechter. Die OFP-Treue-Achse ist aus der Note
    /// (siehe Kommentar in `compute_sub_scores`). Dieser Test faellt laut
    /// aus, wenn jemand sie wieder ausgibt — ueber einen VOLL besetzten
    /// Eingang, damit sie nicht nur deshalb fehlt, weil Daten fehlten.
    #[test]
    fn compute_sub_scores_never_emits_fuel() {
        let rich = voll_besetzter_eingang();
        let subs = compute_sub_scores(&rich);
        assert!(
            !subs.iter().any(|s| s.key == "fuel"),
            "die Sprit-Achse wird wieder benotet — sie misst zum Grossteil \
             die Flugsicherung, nicht den Piloten (v1.7.35)"
        );
        // Und das ruhende Gewicht `"fuel" => 1.0` zieht nicht mit: Der
        // Master ist der gewichtete Schnitt GENAU der ausgegebenen Achsen.
        // (`assert!(master <= 100)` stand hier frueher — bauartbedingt immer
        // wahr und damit kein Beleg.)
        // Ungedeckelt verglichen: es geht um die Gewichte, nicht um Deckel.
        let master = gewichtetes_mittel(&subs).expect("master") as f32;
        let gewicht = |k: &str| -> f32 {
            match k {
                "landing_rate" | "g_force" => 3.0,
                "bounces" | "stability" => 2.0,
                _ => 1.0,
            }
        };
        let summe: f32 = subs
            .iter()
            .filter(|s| !s.skipped)
            .map(|s| s.score as f32 * gewicht(&s.key))
            .sum();
        let gewichte: f32 = subs
            .iter()
            .filter(|s| !s.skipped)
            .map(|s| gewicht(&s.key))
            .sum();
        assert_eq!(
            master,
            (summe / gewichte).round(),
            "ein ruhendes Gewicht zieht mit"
        );
    }

    /// Ein voll besetzter Eingang — damit in den Waechtern jeder Zweig
    /// feuert und eine Achse nicht nur deshalb fehlt, weil ihr die Daten
    /// fehlten.
    fn voll_besetzter_eingang() -> LandingScoringInput {
        LandingScoringInput {
            // v1.9.16: auch das Anflug-Urteil gehoert zum vollen Eingang.
            anflug: anflug_urteil::AnflugWerte::default(),
            diverted: None,
            // v1.7.32: echte Werte, damit der Waechter auch die
            // Gutschrift-Zweige der OFP-Achse durchlaeuft.
            geflogene_strecke_nm: Some(520.0),
            plan_strecke_nm: Some(500.0),
            durchstarts: 1,
            bahn_geometrie_aus_szenerie: None,
            bahn_achsen_abweichung_grad: None,
            bahn_achsen_kreuzt_mitte: None,
            bahn_achsen_groesster_betrag_m: None,
            // v1.7.0: Aufsetzpunkt-Achse — der Test laeuft ueber einen VOLL
            // besetzten Input, damit jeder Zweig feuert.
            aim_point_m: Some(400.0),
            tdz_end_m: Some(900.0),
            bahn_max_querversatz_m: Some(3.0),
            bahn_overrun_m: None,
            bahn_proben: Some(30),
            fahrwerk_spurweite_m: None,
            runway_surface: Some("ASP".into()),
            vs_fpm: Some(-150.0),
            peak_g_load: Some(1.4),
            scored_g_load: Some(1.3),
            bounce_count: Some(1),
            approach_vs_stddev_fpm: Some(120.0),
            approach_bank_stddev_deg: Some(3.0),
            rollout_distance_m: Some(1500.0),
            fuel_efficiency_pct: Some(2.0),
            planned_zfw_kg: Some(60_000.0),
            planned_tow_kg: Some(72_000.0),
            planned_burn_kg: Some(5_000.0),
            actual_trip_burn_kg: Some(5_100.0),
            // Set the flare-quality input HIGH: if a `flare` sub-score were
            // ever (wrongly) wired on this field, it would emit here.
            flare_quality_score: Some(100),
            abfangen: None,
            aircraft_icao: Some("A320".into()),
            td_distance_from_threshold_m: Some(400.0),
            landing_float_distance_m: Some(200.0),
            runway_length_m: Some(3000.0),
            runway_displaced_threshold_ft: Some(0),
            pre_displaced_threshold: Some(false),
            runway_geometry_trusted: Some(true),
            airport_source: Some("ourairports".into()),
            runway_match_icao: Some("EDDM".into()),
            runway_match_ident: Some("08R".into()),
            runway_match_centerline_offset_m: Some(3.0),
            runway_width_m: Some(60.0),
            landing_heading_true_deg: Some(83.0),
            runway_true_course_deg: Some(82.0),
            nicht_konventionell: false,
            landing_wind_direction_deg: None,
            landing_wind_speed_kt: None,
            landing_groundspeed_kt: None,
        }
    }

    /// Die Falldatei des Gleichheitstests (client/src/lib/landungsFaelle.ts)
    /// fuehrt GSG1709 mit 99 Punkten ohne Deckel: Teilnoten aus dem Flug,
    /// Stabilitaet mit dem v19-Gate (σ 84 fpm / 0,9° → 100). Haelt die
    /// Zahl dort an der Rechnung hier fest.
    #[test]
    fn gsg1709_v19_gesamtnote() {
        let teil = |key: &str, punkte: u8| {
            SubScoreEntry::scored(
                key,
                "x",
                punkte,
                String::new(),
                "x",
                band_from_points(punkte),
            )
        };
        let subs = vec![
            teil("landing_rate", 100),
            teil("g_force", 100),
            teil("bounces", 100),
            teil("stability", 100),
            teil("rollout", 100),
            teil("alignment", 100),
            teil("touchdown_point", 85),
        ];
        assert_eq!(landungs_abzug(&subs), None);
        assert_eq!(aggregate_master_score(&subs), Some(99));
    }

    /// Ende zu Ende (QS 05.10.2026): Die Gefahr-Deckel erkennen ihren
    /// Fall an den Begruendungs-Schluesseln der Teilnoten. Dieser Test
    /// geht vom Eingang aus — benennt eine Teilnote ihren Schluessel um,
    /// faellt der Deckel hier auf, nicht erst im Flug.
    #[test]
    fn gefahr_deckel_greift_vom_eingang_aus() {
        // Aufsetzpunkt und Bahndisziplin werden nur auf erkannter Bahn
        // bewertet — wie im echten Flug.
        let auf_der_bahn = || LandingScoringInput {
            airport_source: Some("runway_match".into()),
            ..voll_besetzter_eingang()
        };
        let sauber = compute_sub_scores(&auf_der_bahn());
        assert_eq!(
            gefahr_grund(&sauber),
            None,
            "der volle Eingang ist ungefaehrlich"
        );

        let faelle: [(LandingScoringInput, &str); 3] = [
            (
                LandingScoringInput {
                    bounce_count: Some(2),
                    ..auf_der_bahn()
                },
                GEFAHR_MEHRFACH_HOPSER,
            ),
            (
                LandingScoringInput {
                    td_distance_from_threshold_m: Some(-40.0),
                    ..auf_der_bahn()
                },
                GEFAHR_VOR_DER_SCHWELLE,
            ),
            (
                LandingScoringInput {
                    bahn_overrun_m: Some(60.0),
                    ..auf_der_bahn()
                },
                GEFAHR_OVERRUN,
            ),
        ];
        for (eingang, grund) in faelle {
            let subs = compute_sub_scores(&eingang);
            assert_eq!(
                landungs_abzug(&subs),
                Some((grund, ABZUG_GEFAHR_PUNKTE)),
                "{grund}"
            );
            let mittel = gewichtetes_mittel(&subs).unwrap();
            let anflug = anflug_abzug(&subs).map_or(0, |(_, p)| p);
            assert_eq!(
                aggregate_master_score(&subs),
                Some(mittel.saturating_sub(30 + anflug)),
                "{grund}"
            );
        }
    }

    #[test]
    fn compute_sub_scores_never_emits_flare() {
        let rich = voll_besetzter_eingang();
        let subs = compute_sub_scores(&rich);
        assert!(
            !subs.iter().any(|s| s.key == "flare"),
            "compute_sub_scores must NEVER emit a `flare` sub-score (flare is forensic-only); got keys {:?}",
            subs.iter().map(|s| s.key.as_str()).collect::<Vec<_>>()
        );
        // Also assert over a bare/default input (no fields) — the empty path
        // must not emit flare either.
        let bare = compute_sub_scores(&LandingScoringInput::default());
        assert!(!bare.iter().any(|s| s.key == "flare"));
    }
}
