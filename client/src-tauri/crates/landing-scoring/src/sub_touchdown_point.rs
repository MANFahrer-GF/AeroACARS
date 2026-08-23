//! Aufsetzpunkt-Achse — **wo längs** auf der Bahn aufgesetzt wurde.
//!
//! # Warum es diese Achse gibt
//!
//! Bis v1.7.0 hatte der Aufsetzpunkt **keine eigene Achse**. Er floss
//! ausschliesslich über die Bahn-Achse ein, und zwar indirekt: Ein später
//! Aufsetzpunkt verbrauchte Bahn, die Bahn-Achse sah eine höhere Auslastung
//! und zog dafür Punkte ab.
//!
//! Mit dem Umbau der Bahn-Achse zur reinen Disziplin-Prüfung (nur noch
//! eindeutige Fehler: neben der Bahn, über das Bahnende, vor der Schwelle)
//! verschwände diese Bewertung ersatzlos — „1200 m hinter der Schwelle
//! aufgesetzt" wäre dann gleichwertig mit „in der Aufsetzzone". Deshalb bekommt
//! der Aufsetzpunkt eine eigene Achse.
//!
//! # Warum er sich gut bewerten lässt
//!
//! Anders als die Ausrollstrecke hängt der Aufsetzpunkt an **nichts**, was wir
//! nicht kennen. Keine Lotsenanweisung, kein Verkehr, keine Ausfahrtenlage. Die
//! Bezugsgrössen sind normiert:
//!
//! * **Aufsetzzone** — ICAO Annex 14: die ersten 900 m, auf kurzen Bahnen das
//!   erste Drittel; unter 1200 m Bahnlänge gibt es gar keine Markierung.
//! * **Ziel-Markierung (Aim-Point)** — FAA AIM 8-9-1: 400 m auf Bahnen ab
//!   2400 m, sonst 300 m.
//!
//! Beide Werte werden **nicht hier gerechnet**, sondern als Eingabe übergeben.
//! Die Regeln stehen in `runway_assessment::{classify_aim, classify_tdz}` im
//! App-Crate, und dort sollen sie bleiben — eine zweite Implementierung wäre
//! genau die Fehlerklasse, die dieses Release an anderer Stelle gerade beseitigt
//! (siehe `docs/spec/v1.7.0-bahndisziplin.md` §9).
//!
//! # Vor der Schwelle
//!
//! Aufsetzen vor der Lande-Schwelle ist regelwidrig, nicht bloss ungünstig: Der
//! Bereich davor trägt keine Tragfähigkeit für Landungen und ist als
//! „Pre-Threshold" markiert. Das gehört **hierher** und nicht zur
//! Bahndisziplin — sonst zahlt der Pilot zweimal für dieselbe Sache.

use crate::{Band, SubScoreEntry};

/// Halbe Breite des Vollpunkt-Fensters um die Ziel-Markierung.
///
/// **Setzung, nicht Norm.** Die Aufsetzzone und der Aim-Point sind normiert,
/// dieser Korridor ist es nicht. Gewählt in Anlehnung an die vorhandene
/// `AimClass::Perfect`-Schwelle (±60 m) — aber grosszügiger, weil hier eine
/// *Note* daran hängt und nicht nur eine Beschriftung. Gehört in der Demo an
/// echten Fällen geprüft, bevor er festgeschrieben wird.
const AIM_FENSTER_M: f64 = 150.0;

/// Eingabe der Aufsetzpunkt-Achse.
///
/// Alle Längen in Metern, gemessen ab der **Lande-Schwelle** (nicht ab dem
/// Bahnanfang) — dieselbe Bezugsgrösse wie überall sonst im Bahn-Modul.
#[derive(Debug, Clone, Copy, Default)]
pub struct TouchdownPointInput {
    /// Aufsetzpunkt ab der Lande-Schwelle. **Negativ = vor der Schwelle.**
    pub td_distance_from_threshold_m: Option<f64>,
    /// Ziel-Markierung ab der Schwelle, aus `runway_assessment::classify_aim`.
    pub aim_point_m: Option<f64>,
    /// Ende der Aufsetzzone ab der Schwelle, aus `classify_tdz`.
    /// `None` bedeutet: Diese Bahn hat keine Aufsetzzone (unter 1200 m) —
    /// dann entfällt das `in_tdz`-Band, nicht die ganze Achse.
    pub tdz_end_m: Option<f64>,
    /// Nutzbare Bahnlänge (LDA) ab der Lande-Schwelle.
    pub lda_m: Option<f64>,
    /// Muss `Some("runway_match")` sein, sonst Skip.
    pub airport_source: Option<&'static str>,
    /// Muss `Some(true)` sein, sonst Skip.
    pub runway_geometry_trusted: Option<bool>,
}

/// Bewertet den Aufsetzpunkt.
///
/// # Bänder
///
/// | Lage | Punkte |
/// |---|---|
/// | innerhalb ±150 m um die Ziel-Markierung | 100 |
/// | sonst in der Aufsetzzone | 85 |
/// | hinter der Zone, vor der Bahnmitte | 55 |
/// | hinter der Bahnmitte | 25 |
/// | vor der Lande-Schwelle | 0 |
///
/// # Grundsatz bei Datenmangel
///
/// Fehlt eine Bezugsgrösse, wird **übersprungen** — mit sprechendem Grund, nie
/// mit einer Null. Ein fehlender Wert darf keine schlechte Note erzeugen; genau
/// dieser Fehler hat MPH 9 auf der Bahn-Achse 25 Punkte gekostet.
pub fn sub_touchdown_point(input: &TouchdownPointInput) -> SubScoreEntry {
    const KEY: &str = "touchdown_point";
    const LABEL: &str = "landing.sub.touchdown_point";

    // ── Vorbedingungen ───────────────────────────────────────────────
    if input.airport_source != Some("runway_match") {
        return SubScoreEntry::skipped(KEY, LABEL, "off_airport_landing");
    }
    if input.runway_geometry_trusted != Some(true) {
        return SubScoreEntry::skipped(KEY, LABEL, "untrusted_geometry");
    }
    let Some(td) = input.td_distance_from_threshold_m else {
        return SubScoreEntry::skipped(KEY, LABEL, "missing_td_distance");
    };
    let Some(lda) = input.lda_m.filter(|l| *l > 300.0) else {
        return SubScoreEntry::skipped(KEY, LABEL, "invalid_geometry");
    };
    if !td.is_finite() {
        return SubScoreEntry::skipped(KEY, LABEL, "invalid_geometry");
    }

    // ── Vor der Schwelle: regelwidrig, unabhängig von allem anderen ──
    if td < 0.0 {
        return SubScoreEntry::scored(
            KEY,
            LABEL,
            0,
            format!("{:.0} m vor der Schwelle", td.abs()),
            "pre_threshold",
            Band::Bad,
        );
    }

    let aim = input.aim_point_m.filter(|a| a.is_finite() && *a > 0.0);
    let tdz = input.tdz_end_m.filter(|t| t.is_finite() && *t > 0.0);

    // Anzeige: Aufsetzpunkt und, wenn bekannt, der Abstand zur Markierung.
    let wert = match aim {
        Some(a) => format!("{td:.0} m · Ziel {a:.0} m · Δ {:+.0} m", td - a),
        None => format!("{td:.0} m hinter der Schwelle"),
    };

    // ── Bänder ───────────────────────────────────────────────────────
    let (punkte, band, grund) = if aim.is_some_and(|a| (td - a).abs() <= AIM_FENSTER_M) {
        (100u8, Band::Good, "on_aim")
    } else if tdz.is_some_and(|t| td <= t) {
        (85, Band::Good, "in_tdz")
    } else if td <= lda / 2.0 {
        (55, Band::Ok, "long_touchdown")
    } else {
        (25, Band::Bad, "very_long_touchdown")
    };

    SubScoreEntry::scored(KEY, LABEL, punkte, wert, grund, band)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EHAM 06 wie bei MPH 9: 3189 m nutzbar, Aim bei 400 m, Zone bis 900 m.
    fn eham06(td_m: f64) -> TouchdownPointInput {
        TouchdownPointInput {
            td_distance_from_threshold_m: Some(td_m),
            aim_point_m: Some(400.0),
            tdz_end_m: Some(900.0),
            lda_m: Some(3189.0),
            airport_source: Some("runway_match"),
            runway_geometry_trusted: Some(true),
        }
    }

    #[test]
    fn mph9_liegt_im_zielfenster() {
        // 327 m gegen Ziel 400 m = 73 m davor, also innerhalb der 150 m.
        let r = sub_touchdown_point(&eham06(327.13));
        assert_eq!(r.points, 100);
        assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.on_aim"));
        let wert = r.value.unwrap_or_default();
        assert!(wert.contains("327 m"), "Anzeige: {wert}");
        assert!(wert.contains("-73 m"), "Delta muss vorzeichenbehaftet sein: {wert}");
    }

    #[test]
    fn baender_der_reihe_nach() {
        // Zielfenster 250..550, Zone bis 900, Bahnmitte bei 1594,5.
        for (td, erwartet, grund) in [
            (400.0, 100u8, "on_aim"),           // genau auf der Markierung
            (550.0, 100, "on_aim"),             // Rand des Fensters
            (551.0, 85, "in_tdz"),              // knapp daneben, aber in der Zone
            (899.0, 85, "in_tdz"),              // Ende der Zone
            (901.0, 55, "long_touchdown"),      // dahinter
            (1594.0, 55, "long_touchdown"),     // knapp vor der Bahnmitte
            (1600.0, 25, "very_long_touchdown"),// dahinter
        ] {
            let r = sub_touchdown_point(&eham06(td));
            assert_eq!(r.points, erwartet, "bei {td} m erwartet {erwartet} PT");
            assert_eq!(
                r.rationale_key.as_deref(),
                Some(format!("landing.rat.{grund}").as_str()),
                "bei {td} m"
            );
        }
    }

    #[test]
    fn vor_der_schwelle_ist_null_und_zwar_immer() {
        // Auch wenn alle anderen Werte perfekt aussehen.
        let r = sub_touchdown_point(&eham06(-12.0));
        assert_eq!(r.points, 0);
        assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.pre_threshold"));
        assert!(r.value.unwrap_or_default().contains("12 m vor der Schwelle"));
    }

    #[test]
    fn kurze_bahn_ohne_aufsetzzone() {
        // Unter 1200 m gibt es keine Aufsetzzonen-Markierung (Annex 14).
        // Dann entfaellt das in_tdz-Band, nicht die ganze Achse.
        let mut i = eham06(500.0);
        i.tdz_end_m = None;
        i.lda_m = Some(1100.0);
        i.aim_point_m = Some(300.0); // kurze Bahn -> 300 m
        let r = sub_touchdown_point(&i);
        // 500 gegen Ziel 300 = 200 m dahinter, ausserhalb des Fensters,
        // keine Zone -> vor der Bahnmitte (550) -> long_touchdown.
        assert_eq!(r.points, 55);
        assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.long_touchdown"));
    }

    #[test]
    fn datenmangel_wird_uebersprungen_nie_bestraft() {
        for (bau, grund) in [
            (
                TouchdownPointInput { airport_source: None, ..eham06(400.0) },
                "off_airport_landing",
            ),
            (
                TouchdownPointInput { runway_geometry_trusted: Some(false), ..eham06(400.0) },
                "untrusted_geometry",
            ),
            (
                TouchdownPointInput { td_distance_from_threshold_m: None, ..eham06(400.0) },
                "missing_td_distance",
            ),
            (
                TouchdownPointInput { lda_m: None, ..eham06(400.0) },
                "invalid_geometry",
            ),
            (
                TouchdownPointInput { lda_m: Some(50.0), ..eham06(400.0) },
                "invalid_geometry",
            ),
            (
                TouchdownPointInput {
                    td_distance_from_threshold_m: Some(f64::NAN),
                    ..eham06(400.0)
                },
                "invalid_geometry",
            ),
        ] {
            let r = sub_touchdown_point(&bau);
            assert!(r.skipped, "muss uebersprungen werden: {grund}");
            assert_eq!(r.reason.as_deref(), Some(grund));
            assert_eq!(r.points, 0, "Skip darf keine Note erzeugen");
        }
    }

    #[test]
    fn ohne_zielmarkierung_faellt_es_auf_die_zone_zurueck() {
        // Kein Aim-Point bekannt: das on_aim-Band entfaellt, die Zone traegt.
        let mut i = eham06(400.0);
        i.aim_point_m = None;
        let r = sub_touchdown_point(&i);
        assert_eq!(r.points, 85);
        assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.in_tdz"));
        assert!(r.value.unwrap_or_default().contains("400 m hinter der Schwelle"));
    }
}

#[cfg(test)]
mod kette {
    use crate::*;

    /// Die Achse muss in `compute_sub_scores` tatsächlich auftauchen.
    /// Ohne diesen Test wäre sie gebaut, getestet — und nie aufgerufen.
    #[test]
    fn achse_erscheint_in_der_kette() {
        let input = LandingScoringInput {
            vs_fpm: Some(-200.0),
            td_distance_from_threshold_m: Some(327.0),
            rollout_distance_m: Some(1979.0),
            runway_length_m: Some(3439.0),
            runway_displaced_threshold_ft: Some(820),
            aim_point_m: Some(400.0),
            tdz_end_m: Some(900.0),
            runway_geometry_trusted: Some(true),
            airport_source: Some("runway_match".into()),
            aircraft_icao: Some("MD11".into()),
            ..Default::default()
        };
        let scores = compute_sub_scores(&input);
        let eintrag = scores
            .iter()
            .find(|s| s.key == "touchdown_point")
            .expect("Aufsetzpunkt-Achse fehlt in compute_sub_scores");
        assert_eq!(eintrag.points, 100, "MPH 9 liegt im Zielfenster");
        assert!(!eintrag.skipped);
    }
}
