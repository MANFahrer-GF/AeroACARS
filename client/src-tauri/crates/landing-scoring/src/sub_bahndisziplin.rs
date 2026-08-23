//! Bahndisziplin — **blieb das Flugzeug auf der Bahn?**
//!
//! # Was diese Achse ersetzt
//!
//! Bis v1.7.0 rechnete die Bahn-Achse *genutzte Bahnstrecke ÷ nutzbare Länge*
//! und schloss daraus auf Sicherheit. Wie viel Bahn ein Pilot nutzt, hängt aber
//! an Umständen, die in unseren Daten nicht vorkommen: der Anweisung des Lotsen,
//! dem Verkehr hinter ihm, der Lage der Abrollbahnen. Ein `long rollout` von ATC
//! macht langes Rollen zur Pflicht — und niemand bremst dann auf 40 kt ab, um
//! den Rest der Bahn im Schritttempo zu kriechen.
//!
//! Gemessen über 765 Landungen des Bestands: **80 % der Abzüge trafen Landungen
//! ohne jedes Reserve-Problem.** Umgekehrt bekamen drei objektiv knappe
//! Landungen volle Punktzahl, weil die Geschwindigkeit in der Rechnung gar nicht
//! vorkam.
//!
//! # Was sie stattdessen bewertet
//!
//! Nur, was **ohne Kontextwissen eindeutig falsch** ist:
//!
//! * ein Rad neben der befestigten Fläche,
//! * über das Bahnende hinausgerollt.
//!
//! Alles, was auf der Bahn an Rollstrategie geschieht — Ausrollstrecke,
//! Ausfahrtenwahl, Bremsstärke — bekommt volle Punktzahl. Am Bestand schlägt die
//! Achse damit bei rund 2 % der Landungen an. Das ist gewollt.
//!
//! Der **Aufsetzpunkt** gehört ausdrücklich nicht hierher, sondern in
//! `sub_touchdown_point` — auch „vor der Schwelle". Sonst zahlt der Pilot
//! zweimal für dieselbe Sache.
//!
//! # Die Skala ist geliehen, nicht erfunden
//!
//! Bewertet wird der Anteil des äusseren Hauptrades an der **halben
//! Bahnbreite** — dieselbe Grösse wie in `sub_alignment`, wo `1,0` bedeutet,
//! dass das Flugzeug am Bahnrand steht. Diese Skala ist über 915 Landungen
//! gemessen und fair über alle Bahnbreiten. Ein Pilot soll nicht zwei Maßstäbe
//! für dieselbe Sache lernen müssen.
//!
//! Die Stufen sind gegenüber der Ausrichtung um eine Position gemildert, weil
//! hier ein **Maximum über eine Strecke** steht und nicht ein einzelner Moment.

use crate::belag::Belag;
use crate::{Band, SubScoreEntry};

/// Obergrenze „mittig" — bis hierhin volle Punktzahl.
const ANTEIL_MITTIG: f64 = 0.75;
/// Obergrenze „weit aussen, aber sicher".
const ANTEIL_AUSSEN: f64 = 0.90;

/// Toleranz an der Bahnkante, **zugunsten des Piloten**.
///
/// Erst wenn das äussere Rad mehr als diesen Betrag jenseits der Kante liegt,
/// gilt es als „neben der Bahn".
///
/// **Warum es sie braucht — gemessen an MPH 9, 885 m:**
///
/// | Bahnquelle | Versatz | äusseres Rad | Kante | Differenz |
/// |---|---|---|---|---|
/// | Navigraph | 18,39 m | 23,74 m | 22,55 m | **+1,19 m** |
/// | OpenStreetMap | 17,29 m | 22,64 m | 22,55 m | **+0,09 m** |
///
/// Dieselbe Landung, dieselben Positionsdaten — **35 Punkte Unterschied**,
/// allein durch die Wahl der Bahnquelle. Dazu ist die Bahnbreite eine gerundete
/// Angabe und die Spurweite stammt aus einer Typtabelle. Ohne diese Toleranz
/// entscheidet die Datenquelle über die Note, nicht der Pilot.
const KANTEN_TOLERANZ_M: f64 = 1.5;

/// Eingabe der Bahndisziplin-Achse.
#[derive(Debug, Clone, Copy, Default)]
pub struct BahndisziplinInput {
    /// Grösster Betrag des seitlichen Versatzes über den gewerteten Rollweg,
    /// in Metern von der Mittellinie. Wird im App-Crate aus den Positionsproben
    /// gebildet (siehe Spec §5.2 zum Messfenster).
    pub max_querversatz_m: Option<f64>,
    /// Breite der befestigten Fläche in Metern.
    pub bahnbreite_m: Option<f64>,
    /// Spurweite des Hauptfahrwerks, aus `spurweite::spurweite_m`.
    pub spurweite_m: Option<f64>,
    /// Strecke jenseits des Bahnendes, falls dort noch Fahrt war. `None` oder
    /// `0` = kein Overrun.
    pub overrun_m: Option<f64>,
    /// Belag der Bahn — auf Unbefestigtem entfällt die seitliche Bewertung.
    pub belag: Option<Belag>,
    /// Muss `Some("runway_match")` sein, sonst Skip.
    pub airport_source: Option<&'static str>,
    /// Muss `Some(true)` sein, sonst Skip.
    pub runway_geometry_trusted: Option<bool>,
    /// Anzahl der Positionsproben im Messfenster. Unter 3 ist die Aussage
    /// nicht belastbar.
    pub proben: Option<usize>,
}

/// Bewertet die Bahndisziplin.
///
/// # Bänder
///
/// | Lage des äusseren Rades | Punkte |
/// |---|---|
/// | bis 75 % der halben Bahnbreite | 100 |
/// | bis 90 % | 85 |
/// | bis zur Kante (plus Toleranz) | 55 |
/// | darüber — Rad neben der Bahn | 20 |
/// | **über das Bahnende hinaus** | 0 |
///
/// Ein Overrun **überschreibt alles** — er ist der einzige Fall, der auch dann
/// zählt, wenn die seitliche Bewertung ausgesetzt ist (Graspiste, fehlende
/// Spurweite). Wer über das Bahnende hinausrollt, tut das auf jedem Belag.
pub fn sub_bahndisziplin(input: &BahndisziplinInput) -> SubScoreEntry {
    const KEY: &str = "rollout"; // Schlüssel bleibt, damit alte Anzeigen nicht brechen
    const LABEL: &str = "landing.sub.runway_discipline";

    // ── Vorbedingungen ───────────────────────────────────────────────
    if input.airport_source != Some("runway_match") {
        return SubScoreEntry::skipped(KEY, LABEL, "off_airport_landing");
    }
    if input.runway_geometry_trusted != Some(true) {
        return SubScoreEntry::skipped(KEY, LABEL, "untrusted_geometry");
    }

    // ── Overrun zuerst: gilt unabhängig von Belag und Spurweite ──────
    // Wer über das Bahnende hinausrollt, tut das auf jedem Untergrund. Diese
    // Prüfung darf nicht hinter den seitlichen Skips liegen, sonst verschwindet
    // der schwerste Fall ausgerechnet dort, wo die Daten dünn sind.
    if let Some(over) = input.overrun_m.filter(|m| *m > 0.0 && m.is_finite()) {
        return SubScoreEntry::scored(
            KEY,
            LABEL,
            0,
            format!("{over:.0} m über das Bahnende hinaus"),
            "overrun",
            Band::Bad,
        );
    }

    // ── Seitliche Bewertung: nur auf befestigten Bahnen ──────────────
    let belag = input.belag.unwrap_or(Belag::Unbekannt);
    if !belag.seitlich_bewertbar() {
        return SubScoreEntry::skipped(KEY, LABEL, belag.skip_grund());
    }
    let Some(breite) = input.bahnbreite_m.filter(|b| (10.0..=120.0).contains(b)) else {
        return SubScoreEntry::skipped(KEY, LABEL, "runway_width_unknown");
    };
    let Some(spur) = input.spurweite_m.filter(|s| (1.0..=20.0).contains(s)) else {
        return SubScoreEntry::skipped(KEY, LABEL, "track_width_unknown");
    };
    if input.proben.is_some_and(|n| n < 3) {
        return SubScoreEntry::skipped(KEY, LABEL, "insufficient_samples");
    }
    let Some(versatz) = input.max_querversatz_m.filter(|v| v.is_finite()) else {
        return SubScoreEntry::skipped(KEY, LABEL, "missing_lateral_track");
    };

    // ── Lage des äusseren Rades ──────────────────────────────────────
    let halbe = breite / 2.0;
    let aussenkante_m = versatz.abs() + spur / 2.0;
    let anteil = aussenkante_m / halbe;
    let rand_abstand_m = halbe - aussenkante_m;

    let wert = format!(
        "{:.1} m Versatz · äußeres Rad {:.1} m von der Mitte · Rand {:+.1} m",
        versatz.abs(),
        aussenkante_m,
        rand_abstand_m
    );

    let (punkte, band, grund) = if anteil <= ANTEIL_MITTIG {
        (100u8, Band::Good, "centered")
    } else if anteil <= ANTEIL_AUSSEN {
        (85, Band::Good, "outboard")
    } else if rand_abstand_m >= -KANTEN_TOLERANZ_M {
        // Innerhalb der Toleranz — die Datenlage gibt "eindeutig daneben"
        // nicht her. Siehe KANTEN_TOLERANZ_M.
        (55, Band::Ok, "edge_reached")
    } else {
        (20, Band::Bad, "off_pavement")
    };

    SubScoreEntry::scored(KEY, LABEL, punkte, wert, grund, band)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EHAM 06: 45,1 m breit, MD-11 mit 10,7 m Spurweite.
    fn eham06(versatz_m: f64) -> BahndisziplinInput {
        BahndisziplinInput {
            max_querversatz_m: Some(versatz_m),
            bahnbreite_m: Some(45.1),
            spurweite_m: Some(10.7),
            overrun_m: None,
            belag: Some(Belag::Befestigt),
            airport_source: Some("runway_match"),
            runway_geometry_trusted: Some(true),
            proben: Some(30),
        }
    }

    #[test]
    fn mph9_beide_bahnquellen_ergeben_dasselbe() {
        // Das ist der Grund für die Kantentoleranz. Ohne sie ergaeben die
        // beiden Quellen 20 gegen 55 Punkte fuer dieselbe Landung.
        let navigraph = sub_bahndisziplin(&eham06(18.39));
        let osm = sub_bahndisziplin(&eham06(17.29));
        assert_eq!(
            navigraph.points, osm.points,
            "Navigraph {} gegen OSM {} — die Datenquelle darf die Note nicht entscheiden",
            navigraph.points, osm.points
        );
        assert_eq!(navigraph.points, 55);
        assert_eq!(
            navigraph.rationale_key.as_deref(),
            Some("landing.rat.edge_reached")
        );
    }

    #[test]
    fn baender_der_reihe_nach() {
        // halbe Breite 22,55 m; aeusseres Rad = Versatz + 5,35 m.
        // 75 % -> 16,91 m Rad -> Versatz 11,56
        // 90 % -> 20,30 m Rad -> Versatz 14,95
        for (versatz, erwartet, grund) in [
            (0.0, 100u8, "centered"),
            (11.0, 100, "centered"),
            (12.0, 85, "outboard"),
            (14.5, 85, "outboard"),
            (15.5, 55, "edge_reached"),
            (18.7, 55, "edge_reached"), // Rand -1,5 m: noch in der Toleranz
            (20.0, 20, "off_pavement"), // Rand -2,8 m: eindeutig daneben
        ] {
            let r = sub_bahndisziplin(&eham06(versatz));
            assert_eq!(r.points, erwartet, "bei {versatz} m Versatz");
            assert_eq!(
                r.rationale_key.as_deref(),
                Some(format!("landing.rat.{grund}").as_str()),
                "bei {versatz} m Versatz"
            );
        }
    }

    #[test]
    fn vorzeichen_egal_es_zaehlt_der_betrag() {
        // Links und rechts sind gleich schlimm.
        assert_eq!(
            sub_bahndisziplin(&eham06(20.0)).points,
            sub_bahndisziplin(&eham06(-20.0)).points
        );
    }

    #[test]
    fn overrun_ueberschreibt_alles() {
        // Auch bei perfekter Mittellage.
        let mut i = eham06(0.0);
        i.overrun_m = Some(35.0);
        let r = sub_bahndisziplin(&i);
        assert_eq!(r.points, 0);
        assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.overrun"));
        assert!(r.value.unwrap_or_default().contains("35 m über das Bahnende"));
    }

    #[test]
    fn overrun_zaehlt_auch_ohne_spurweite_und_auf_gras() {
        // Der schwerste Fall darf nicht ausgerechnet dort verschwinden,
        // wo die Datenlage duenn ist.
        let mut i = eham06(0.0);
        i.overrun_m = Some(20.0);
        i.spurweite_m = None;
        i.belag = Some(Belag::Unbefestigt);
        let r = sub_bahndisziplin(&i);
        assert_eq!(r.points, 0, "Overrun gilt auf jedem Untergrund");
        assert!(!r.skipped);
    }

    #[test]
    fn graspiste_wird_seitlich_nicht_bewertet() {
        let mut i = eham06(25.0); // waere auf Asphalt "neben der Bahn"
        i.belag = Some(Belag::Unbefestigt);
        let r = sub_bahndisziplin(&i);
        assert!(r.skipped);
        assert_eq!(r.reason.as_deref(), Some("unpaved_runway"));
        assert_eq!(r.points, 0, "Skip erzeugt keine Note");
    }

    #[test]
    fn datenmangel_wird_uebersprungen_nie_bestraft() {
        for (bau, grund) in [
            (
                BahndisziplinInput { airport_source: None, ..eham06(5.0) },
                "off_airport_landing",
            ),
            (
                BahndisziplinInput { runway_geometry_trusted: Some(false), ..eham06(5.0) },
                "untrusted_geometry",
            ),
            (
                BahndisziplinInput { bahnbreite_m: None, ..eham06(5.0) },
                "runway_width_unknown",
            ),
            (
                BahndisziplinInput { bahnbreite_m: Some(500.0), ..eham06(5.0) },
                "runway_width_unknown",
            ),
            (
                BahndisziplinInput { spurweite_m: None, ..eham06(5.0) },
                "track_width_unknown",
            ),
            (
                BahndisziplinInput { proben: Some(2), ..eham06(5.0) },
                "insufficient_samples",
            ),
            (
                BahndisziplinInput { max_querversatz_m: None, ..eham06(5.0) },
                "missing_lateral_track",
            ),
            (
                BahndisziplinInput { belag: Some(Belag::Unbekannt), ..eham06(5.0) },
                "surface_unknown",
            ),
        ] {
            let r = sub_bahndisziplin(&bau);
            assert!(r.skipped, "muss uebersprungen werden: {grund}");
            assert_eq!(r.reason.as_deref(), Some(grund));
            assert_eq!(r.points, 0, "Skip darf keine Note erzeugen");
        }
    }

    #[test]
    fn schmale_bahn_wird_strenger_ohne_sonderregel() {
        // Dieselben 8 m Versatz auf einer 23-m-Bahn (Code C) sind etwas
        // ganz anderes als auf 45 m. Genau dafuer ist die Anteilsskala da.
        let mut schmal = eham06(8.0);
        schmal.bahnbreite_m = Some(23.0);
        schmal.spurweite_m = Some(5.72); // 737 statt MD-11
        let r = sub_bahndisziplin(&schmal);
        // Rad bei 8 + 2,86 = 10,86 von 11,5 halber Breite = 94 %
        assert_eq!(r.points, 55, "auf schmaler Bahn ist das die Kante");

        let breit = sub_bahndisziplin(&eham06(8.0));
        assert_eq!(breit.points, 100, "auf 45 m ist derselbe Versatz mittig");
    }
}

#[cfg(test)]
mod kette {
    use crate::*;

    fn mph9_eingabe() -> LandingScoringInput {
        LandingScoringInput {
            vs_fpm: Some(-339.0),
            td_distance_from_threshold_m: Some(327.0),
            rollout_distance_m: Some(1979.0),
            runway_length_m: Some(3439.0),
            runway_width_m: Some(45.1),
            runway_displaced_threshold_ft: Some(820),
            aim_point_m: Some(400.0),
            tdz_end_m: Some(900.0),
            runway_geometry_trusted: Some(true),
            airport_source: Some("runway_match".into()),
            aircraft_icao: Some("MD11".into()),
            runway_surface: Some("ASP".into()),
            bahn_max_querversatz_m: Some(18.39),
            bahn_proben: Some(30),
            // Ausrichtungs-Achse braucht eigene Felder — ohne sie erscheint
            // sie gar nicht, und der Test unten prueft ja gerade, dass die
            // drei Bahn-bezogenen Achsen NEBENEINANDER stehen.
            runway_match_centerline_offset_m: Some(-1.04),
            landing_heading_true_deg: Some(57.8),
            runway_true_course_deg: Some(58.06),
            ..Default::default()
        }
    }

    /// Die neue Achse muss in der Kette stehen — und die alte darf nicht
    /// mehr mitlaufen, sonst haette der Pilot zwei Bahn-Noten.
    #[test]
    fn disziplin_ersetzt_die_auslastung() {
        let scores = compute_sub_scores(&mph9_eingabe());
        let bahn: Vec<_> = scores.iter().filter(|s| s.key == "rollout").collect();
        assert_eq!(bahn.len(), 1, "genau eine Bahn-Achse, nicht zwei");
        let b = bahn[0];
        assert_eq!(
            b.label_key, "landing.sub.runway_discipline",
            "es muss die Disziplin-Achse sein, nicht die Auslastung"
        );
        // 18,39 m Versatz + 5,35 m halbe Spur = 23,74 m gegen 22,55 m Kante,
        // also 1,19 m drueber — innerhalb der 1,5-m-Toleranz.
        assert_eq!(b.points, 55);
        assert_eq!(b.rationale_key.as_deref(), Some("landing.rat.edge_reached"));
    }

    /// Die Spurweite muss ueber den Typ gefunden werden — ohne sie entfaellt
    /// die seitliche Bewertung, und genau das war der MPH-9-Fehler.
    #[test]
    fn ohne_typ_wird_seitlich_nicht_bewertet() {
        let mut e = mph9_eingabe();
        e.aircraft_icao = None;
        let scores = compute_sub_scores(&e);
        let b = scores.iter().find(|s| s.key == "rollout").expect("Achse");
        assert!(b.skipped, "ohne Spurweite kein Urteil ueber ein Rad");
        assert_eq!(b.reason.as_deref(), Some("track_width_unknown"));
        assert_eq!(b.points, 0, "Skip erzeugt keine Note");
    }

    /// Neun Achsen statt acht — die Zahl gehoert festgehalten, weil jede
    /// weitere den Gesamtscore aller Piloten verschiebt.
    #[test]
    fn neun_achsen_bei_voller_datenlage() {
        let scores = compute_sub_scores(&mph9_eingabe());
        let bewertet: Vec<&str> = scores
            .iter()
            .filter(|s| !s.skipped)
            .map(|s| s.key.as_str())
            .collect();
        assert!(
            bewertet.contains(&"touchdown_point"),
            "Aufsetzpunkt fehlt: {bewertet:?}"
        );
        assert!(bewertet.contains(&"rollout"), "Bahndisziplin fehlt: {bewertet:?}");
        assert!(bewertet.contains(&"alignment"), "Ausrichtung fehlt: {bewertet:?}");
    }
}
