//! Ausrichtungs-Sub-Score (v1.6.2, `score_algorithm_version` 6).
//!
//! **Warum es diese Achse gibt.** Der Score maß bis v1.6.1 ausschließlich,
//! WIE sanft aufgesetzt wurde — nie, WO und wie gerade. Auslöser war
//! BTI 243 (EVRA→EDDF 07L, 14.08.2026): 92 Punkte, Note A „smooth", bei
//! 45,7 m Versatz zur Mittellinie einer 45 m breiten Bahn und 15°
//! Kursabweichung. Das Flugzeug setzte gut 23 m jenseits des Asphaltrands
//! auf — und keine der sieben Achsen bemerkte es.
//!
//! Spezifiziert war die Anforderung längst: `docs/spec/requirements.md`
//! §16 (Arrival Centerline Deviation) und §17 (Arrival Heading Deviation).
//!
//! **Zwei bewusste Abweichungen von der Spec-Tabelle:**
//!
//! 1. Die Spec bewertet den Versatz in ABSOLUTEN Metern. Das ist unfair:
//!    26 m auf Chicagos 61-m-Bahn sind sicher auf der Bahn, dieselben 26 m
//!    auf einer 45-m-Bahn sind am Rand. Gemessen am Korpus (915 Landungen)
//!    verschiebt die absolute Tabelle 141 Landungen in die falsche Stufe.
//!    Wir bewerten deshalb den ANTEIL an der halben Bahnbreite: 1,0 = das
//!    Flugzeug steht genau am Bahnrand. Die Breite liegt für alle 85 058
//!    Bahnen der Navdaten vor.
//! 2. Die Spec kennt kein „nicht bewertbar". Wir brauchen es — siehe
//!    `GEOMETRIE_FRAGWUERDIG_*` unten.
//!
//! **Nur Größen, die BEIDE Simulatoren gleichwertig liefern.** Versatz und
//! Kursabweichung stammen aus Position und rechtweisendem Steuerkurs; beide
//! sind in MSFS und X-Plane nativ und gleich definiert. Bewusst NICHT
//! verwendet: der Schiebewinkel (X-Plane liefert ihn nur bei 26 % der
//! Landungen und rechnet ihn anders als MSFS), die Windkomponenten (Frame
//! auf X-Plane ungeklärt) und das VORZEICHEN der Querneigung (MSFS dreht
//! bei der Längsneigung das Vorzeichen und der Code korrigiert das
//! ausdrücklich — bei der Querneigung passiert diese Korrektur nicht, das
//! ist nie verifiziert worden).

use crate::{Band, SubScoreEntry};

/// Anteil an der halben Bahnbreite → Punkte. 1,0 = Bahnrand.
/// Kalibriert am Korpus: p50 = 0,17 · p90 = 0,51 · p95 = 0,71.
/// 97,5 % aller Landungen liegen unter 1,0 — die Achse trifft Ausreißer,
/// nicht den Normalbetrieb.
const VERSATZ_STUFEN: [(f32, u8); 4] = [(0.25, 100), (0.50, 85), (0.75, 65), (1.00, 40)];
const VERSATZ_DARUEBER: u8 = 15;

/// Kursabweichung in Grad → Punkte. Stufen aus `requirements.md` §17
/// unverändert übernommen (dort ist die Tabelle sim-neutral und passt zum
/// Korpus: p50 = 2,0° · p90 = 5,8° · p99 = 12,6°).
const KURS_STUFEN: [(f32, u8); 4] = [(3.0, 100), (6.0, 85), (10.0, 65), (15.0, 40)];
const KURS_DARUEBER: u8 = 15;

/// Ab diesem Versatz-Anteil gilt die Bahngeometrie als fragwürdig —
/// gemeinsam mit [`GEOMETRIE_FRAGWUERDIG_MAX_KURS_DEG`].
///
/// **Der Feldbefund dahinter (EDHE/Uetersen, 13.–14.08.2026).** Vier
/// Landungen wurden dort mit 50–73 m Versatz gemeldet. Die Gegenprobe
/// gegen OpenStreetMap zeigte: die Navigraph-Bahnachse liegt rund 60 m
/// neben der echten Bahn, die Piloten setzten mit 2,7–6,0 m zur
/// tatsächlichen Mittellinie praktisch perfekt auf. Ohne Schutz hätte
/// diese Achse vier fehlerfreie Landungen abgestraft.
///
/// **Die Unterscheidung.** Wer perfekt AUSGERICHTET mehr als eine ganze
/// Bahnbreite neben der Mittellinie aufsetzt, ist mit hoher
/// Wahrscheinlichkeit auf einer real existierenden, aber falsch
/// kartierten Bahn gelandet — nicht im Gras. Wer weit daneben UND schief
/// ist, hat wirklich einen Fehler gemacht: BTI 243 lag bei 2,03 Anteilen
/// und 14,9° und wird deshalb korrekt bewertet, die EDHE-Landungen bei
/// 2,5–3,7 Anteilen und 0,7–4,6° fallen heraus.
const GEOMETRIE_FRAGWUERDIG_AB_ANTEIL: f32 = 2.0;
const GEOMETRIE_FRAGWUERDIG_MAX_KURS_DEG: f32 = 5.0;

/// Eingaben der Ausrichtungs-Bewertung. Alle `Option`, weil jede fehlende
/// Größe zu einem sichtbaren „nicht bewertet" führt — nie zu Punktabzug.
#[derive(Debug, Clone, Default)]
pub struct AlignmentInput {
    /// Seitlicher Versatz zur Bahnmittellinie in Metern, vorzeichenbehaftet
    /// (positiv = rechts). Für die Bewertung zählt nur der Betrag.
    pub centerline_offset_m: Option<f32>,
    /// Bahnbreite in Metern.
    pub runway_width_m: Option<f32>,
    /// Rechtweisender Steuerkurs des Flugzeugs beim Aufsetzen.
    pub heading_true_deg: Option<f32>,
    /// Rechtweisender Bahnkurs. Liegt nur bei Navigraph-Navdaten vor —
    /// im OurAirports-Ersatzpfad ist er `None`, dann wird nicht bewertet.
    pub runway_true_course_deg: Option<f32>,
    /// Wie der Flughafen bestimmt wurde; nur `Some("runway_match")` zählt.
    pub airport_source: Option<String>,
    /// Vertrauensurteil über die Bahngeometrie. `None` gilt als NICHT
    /// vertrauenswürdig (dasselbe Muster wie beim Bremsweg-Score).
    pub runway_geometry_trusted: Option<bool>,
}

/// Kleinster Winkel zwischen zwei Kursen, 0–180°.
fn kursdifferenz(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 { 360.0 - d } else { d }
}

fn punkte_aus_stufen(wert: f32, stufen: &[(f32, u8)], darueber: u8) -> u8 {
    for (grenze, punkte) in stufen {
        if wert <= *grenze {
            return *punkte;
        }
    }
    darueber
}

/// Warum nicht bewertet werden kann — `None`, wenn alles vorliegt.
/// Reihenfolge wie beim Bremsweg-Score: erst Voraussetzungen (die
/// spezifischere Aussage), dann fehlende Daten, dann Plausibilität.
fn skip_grund(input: &AlignmentInput, anteil: Option<f32>, kurs_abw: Option<f32>) -> Option<&'static str> {
    if input.airport_source.as_deref() != Some("runway_match") {
        return Some("off_airport_landing");
    }
    if input.runway_geometry_trusted != Some(true) {
        return Some("untrusted_geometry");
    }
    if input.centerline_offset_m.is_none() {
        return Some("missing_centerline_offset");
    }
    // Ohne Breite wäre nur die absolute Meterzahl da — und die ist je nach
    // Bahn etwas völlig anderes. Lieber nicht bewerten als falsch bewerten.
    if input.runway_width_m.is_none_or(|w| w <= 0.0) {
        return Some("missing_runway_width");
    }
    // Der Bahnkurs fehlt im OurAirports-Ersatzpfad. Ohne ihn ließe sich nur
    // der Versatz bewerten — dann verlöre die Achse aber genau die Größe,
    // die den Datenfehler von der Fehllandung trennt (siehe unten).
    if input.runway_true_course_deg.is_none() {
        return Some("missing_runway_course");
    }
    if input.heading_true_deg.is_none() {
        return Some("missing_heading");
    }
    // Geometrie fragwürdig: weit daneben, aber sauber ausgerichtet.
    if let (Some(a), Some(k)) = (anteil, kurs_abw) {
        if a >= GEOMETRIE_FRAGWUERDIG_AB_ANTEIL && k <= GEOMETRIE_FRAGWUERDIG_MAX_KURS_DEG {
            return Some("implausible_runway_geometry");
        }
    }
    None
}

/// Der Ausrichtungs-Sub-Score. Die schlechtere der beiden Größen bestimmt
/// die Punktzahl — wer die Mittellinie trifft, aber 15° schräg steht, hat
/// genauso ein Problem wie umgekehrt.
pub fn sub_alignment(input: &AlignmentInput) -> SubScoreEntry {
    let anteil = match (input.centerline_offset_m, input.runway_width_m) {
        (Some(o), Some(w)) if w > 0.0 => Some(o.abs() / (w / 2.0)),
        _ => None,
    };
    let kurs_abw = match (input.heading_true_deg, input.runway_true_course_deg) {
        (Some(h), Some(c)) => Some(kursdifferenz(h, c)),
        _ => None,
    };

    if let Some(grund) = skip_grund(input, anteil, kurs_abw) {
        return SubScoreEntry::skipped("alignment", "landing.sub.alignment", grund);
    }

    let anteil = anteil.expect("durch skip_grund abgesichert");
    let kurs_abw = kurs_abw.expect("durch skip_grund abgesichert");
    let offset_m = input.centerline_offset_m.expect("durch skip_grund abgesichert").abs();

    let p_versatz = punkte_aus_stufen(anteil, &VERSATZ_STUFEN, VERSATZ_DARUEBER);
    let p_kurs = punkte_aus_stufen(kurs_abw, &KURS_STUFEN, KURS_DARUEBER);
    let punkte = p_versatz.min(p_kurs);

    // Die Begründung nennt die Größe, die den Ausschlag gab — sonst rät der
    // Pilot, woran es lag.
    let grund = if punkte >= 100 {
        "aligned_on_centerline"
    } else if p_versatz <= p_kurs {
        if anteil > 1.0 { "off_runway_surface" } else { "off_centerline" }
    } else {
        "crooked_touchdown"
    };

    let mut eintrag = SubScoreEntry::scored(
        "alignment",
        "landing.sub.alignment",
        punkte,
        format!("{offset_m:.0} m · {kurs_abw:.0}°"),
        grund,
        crate::band_from_points(punkte),
    );
    // Zusatzzeilen: der nackte Meterwert sagt ohne Bahnbreite wenig.
    eintrag.extra.push(format!(
        "Versatz {:.0} m bei {:.0} m Bahnbreite ({:.0} % bis zum Rand)",
        offset_m,
        input.runway_width_m.unwrap_or(0.0),
        anteil * 100.0
    ));
    eintrag.extra.push(format!("Kursabweichung {kurs_abw:.1}° zur Bahnachse"));
    eintrag
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basis() -> AlignmentInput {
        AlignmentInput {
            centerline_offset_m: Some(0.0),
            runway_width_m: Some(45.0),
            heading_true_deg: Some(70.0),
            runway_true_course_deg: Some(70.0),
            airport_source: Some("runway_match".into()),
            runway_geometry_trusted: Some(true),
        }
    }

    #[test]
    fn mittellinie_und_kurs_perfekt_gibt_volle_punkte() {
        let e = sub_alignment(&basis());
        assert!(!e.skipped);
        assert_eq!(e.points, 100);
        assert_eq!(e.rationale_key.as_deref(), Some("landing.rat.aligned_on_centerline"));
    }

    #[test]
    fn bti243_wird_bewertet_und_faellt_durch() {
        // Der Auslöser-Flug: 45,7 m Versatz auf 45,1 m Bahn (Anteil 2,03),
        // Steuerkurs 84,5° gegen Bahnkurs 69,5° (14,9°). Beide Quellen —
        // Navigraph und OpenStreetMap — sind sich hier auf 0,4 m einig,
        // die Bahn ist also korrekt kartiert.
        let e = sub_alignment(&AlignmentInput {
            centerline_offset_m: Some(-45.7),
            runway_width_m: Some(45.1),
            heading_true_deg: Some(84.487),
            runway_true_course_deg: Some(69.548),
            ..basis()
        });
        assert!(!e.skipped, "korrekt kartierte Bahn muss bewertet werden");
        assert_eq!(e.points, 15);
    }

    #[test]
    fn edhe_falsch_kartierte_bahn_wird_nicht_bewertet() {
        // Feldbefund EDHE 27: rund 60 m Versatz gegen die Navdaten, aber
        // praktisch perfekt ausgerichtet — in Wahrheit lag die Bahnachse
        // der Navdaten daneben, die Landung war sauber.
        let e = sub_alignment(&AlignmentInput {
            centerline_offset_m: Some(-65.4),
            runway_width_m: Some(39.9),
            heading_true_deg: Some(270.7),
            runway_true_course_deg: Some(270.0),
            ..basis()
        });
        assert!(e.skipped, "falsch kartierte Bahn darf niemanden bestrafen");
        assert_eq!(e.reason.as_deref(), Some("implausible_runway_geometry"));
    }

    #[test]
    fn weit_daneben_und_schief_bleibt_bewertet() {
        // Gegenprobe zum EDHE-Schutz: derselbe große Versatz, aber deutlich
        // schräg — das ist ein echter Fehler und muss zählen.
        let e = sub_alignment(&AlignmentInput {
            centerline_offset_m: Some(-65.4),
            runway_width_m: Some(39.9),
            heading_true_deg: Some(282.0),
            runway_true_course_deg: Some(270.0),
            ..basis()
        });
        assert!(!e.skipped);
        assert_eq!(e.points, 15);
    }

    #[test]
    fn breite_bahn_ist_milder_als_schmale() {
        // Derselbe Versatz in Metern, zwei Bahnbreiten: 26 m auf Chicagos
        // 61-m-Bahn ist sicher auf der Bahn, auf 45 m ist es der Rand.
        let breit = sub_alignment(&AlignmentInput {
            centerline_offset_m: Some(26.0),
            runway_width_m: Some(61.0),
            ..basis()
        });
        let schmal = sub_alignment(&AlignmentInput {
            centerline_offset_m: Some(26.0),
            runway_width_m: Some(45.0),
            ..basis()
        });
        assert!(
            breit.points > schmal.points,
            "auf der breiten Bahn muss dieselbe Abweichung milder ausfallen ({} vs {})",
            breit.points,
            schmal.points
        );
    }

    #[test]
    fn schlechtere_der_beiden_groessen_gewinnt() {
        // Mittellinie perfekt, aber 12° schräg → die Kursabweichung zieht.
        let e = sub_alignment(&AlignmentInput {
            centerline_offset_m: Some(0.5),
            heading_true_deg: Some(82.0),
            runway_true_course_deg: Some(70.0),
            ..basis()
        });
        assert_eq!(e.points, 40);
        assert_eq!(e.rationale_key.as_deref(), Some("landing.rat.crooked_touchdown"));
    }

    #[test]
    fn fehlende_daten_werden_nie_zu_punktabzug() {
        let faelle = [
            (AlignmentInput { airport_source: Some("nearest".into()), ..basis() }, "off_airport_landing"),
            (AlignmentInput { runway_geometry_trusted: None, ..basis() }, "untrusted_geometry"),
            (AlignmentInput { runway_geometry_trusted: Some(false), ..basis() }, "untrusted_geometry"),
            (AlignmentInput { centerline_offset_m: None, ..basis() }, "missing_centerline_offset"),
            (AlignmentInput { runway_width_m: None, ..basis() }, "missing_runway_width"),
            (AlignmentInput { runway_width_m: Some(0.0), ..basis() }, "missing_runway_width"),
            (AlignmentInput { runway_true_course_deg: None, ..basis() }, "missing_runway_course"),
            (AlignmentInput { heading_true_deg: None, ..basis() }, "missing_heading"),
        ];
        for (input, erwartet) in faelle {
            let e = sub_alignment(&input);
            assert!(e.skipped, "{erwartet} muss zum Überspringen führen");
            assert_eq!(e.reason.as_deref(), Some(erwartet));
            assert_eq!(e.points, 0, "übersprungene Achsen zählen gar nicht mit");
        }
    }

    #[test]
    fn kursdifferenz_ueber_den_nordpunkt() {
        assert!((kursdifferenz(359.0, 1.0) - 2.0).abs() < 1e-4);
        assert!((kursdifferenz(1.0, 359.0) - 2.0).abs() < 1e-4);
        assert!((kursdifferenz(90.0, 270.0) - 180.0).abs() < 1e-4);
        assert!((kursdifferenz(70.0, 70.0)).abs() < 1e-4);
    }

    #[test]
    fn stufen_sind_monoton() {
        // Mehr Abweichung darf nie mehr Punkte geben.
        let mut vorher = 101u8;
        for anteil in [0.0f32, 0.2, 0.3, 0.6, 0.8, 1.0, 1.5, 3.0] {
            let e = sub_alignment(&AlignmentInput {
                centerline_offset_m: Some(anteil * 22.5),
                runway_width_m: Some(45.0),
                // Kurs immer leicht schräg, damit der Geometrie-Schutz
                // (der sauberen Kurs verlangt) hier nicht dazwischenfunkt.
                heading_true_deg: Some(78.0),
                runway_true_course_deg: Some(70.0),
                ..basis()
            });
            assert!(e.points <= vorher, "Anteil {anteil}: {} > {vorher}", e.points);
            vorher = e.points;
        }
    }
}
