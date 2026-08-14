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
//!    Flugzeug steht genau am Bahnrand. Die Breite liegt für nahezu alle
//!    Bahnen der Navdaten vor; fehlt sie, wird sichtbar nicht bewertet
//!    (die Metertabelle als Rückfallebene wäre genau die Unfairness,
//!    die wir gerade abgeschafft haben).
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

use crate::SubScoreEntry;

/// Anteil an der halben Bahnbreite → Punkte. 1,0 = Bahnrand.
/// Kalibriert am Korpus: p50 = 0,17 · p90 = 0,51 · p95 = 0,71.
/// 97,5 % aller Landungen liegen unter 1,0 (= innerhalb der Bahn); volle
/// Punkte gibt es aber erst unter 0,25, was rund zwei Drittel erreichen.
/// Die Achse bewertet also jede Landung, trifft hart aber nur Ausreißer.
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
/// …und zusaetzlich mindestens so viele Meter daneben. Ein Kartenfehler ist
/// ein absolutes Phaenomen (EDHE: ~60 m, unabhaengig von der Bahnbreite) —
/// der Anteil allein waere auf einer 18-m-Buschpiste schon bei 18 m erfuellt
/// und wuerde dort jede echte Neben-die-Piste-Landung entschuldigen
/// (QS-Befund v1.6.2).
const GEOMETRIE_FRAGWUERDIG_AB_METER: f32 = 40.0;

/// Breiteste Bahn der Welt liegt bei rund 80 m. Alles darüber ist ein
/// Datenfehler — und ein unendlicher oder absurd großer Wert machte den
/// Versatz-Anteil beliebig klein, also die Achse zum stillen
/// 100-Punkte-Geschenk (QS-Befund v1.6.2).
const MAX_PLAUSIBLE_BAHNBREITE_M: f32 = 120.0;

/// Jenseits davon ist nicht der Pilot schief, sondern die Zuordnung falsch:
/// bei ~180° wurde das andere Bahnende gematcht (Landung nahe der Bahnmitte,
/// Divert). Eine tadellose Landung bekäme sonst 15 Punkte und den Hinweis
/// „schräg aufgesetzt" (QS-Befund v1.6.2).
const MAX_BEWERTBARE_KURSABWEICHUNG_DEG: f32 = 90.0;

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
    /// `true` bei Drehflüglern und Wasserflugzeugen. Für sie gibt es keine
    /// Bahnachse, an der man sich ausrichten müsste — ein Helikopter setzt
    /// regulär quer auf. Ohne dieses Gate bekäme er 15 Punkte und den
    /// Ratschlag „mit dem Seitenruder ausrichten" (QS-Befund v1.6.2,
    /// dieselbe Lücke wie bei den Anflug-Streuungen in v0.15.21).
    pub nicht_konventionell: bool,
}

/// Kleinster Winkel zwischen zwei Kursen, 0–180°.
fn kursdifferenz(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 { 360.0 - d } else { d }
}

/// Toleranz beim Stufenvergleich. QS-Befund v1.6.2: 16,9125 m Versatz auf
/// einer 45,1-m-Bahn ergibt rechnerisch exakt 0,75 — in f32 aber 0,75000006
/// und damit die naechstschlechtere Stufe. Der Pilot laese „75 %" und
/// bekaeme das Band, das erst DARUEBER beginnt. Genau die krummen
/// Navdaten-Breiten (45,1 · 39,9 · 60,96 m) treffen das.
const STUFEN_TOLERANZ: f32 = 1e-4;

fn punkte_aus_stufen(wert: f32, stufen: &[(f32, u8)], darueber: u8) -> u8 {
    for (grenze, punkte) in stufen {
        if wert <= *grenze + STUFEN_TOLERANZ {
            return *punkte;
        }
    }
    darueber
}

/// Die geprüften Messwerte `(Versatz-Anteil, Kursabweichung)` — oder der
/// Grund, warum nicht bewertet werden kann.
///
/// **Warum ein `Result` und keine `Option<&str>` (QS-Befund v1.6.2).** Der
/// erste Wurf prüfte die Vorbedingungen hier und packte die Werte danach
/// mit `.expect("durch skip_grund abgesichert")` aus. Diese Invariante hielt
/// bei NaN NICHT: `is_none_or(|w| w <= 0.0)` ist für `Some(NaN)` **false**
/// (kein Überspringen), während die Berechnung `if w > 0.0` bei NaN
/// ebenfalls **false** ergibt (kein Wert) — die beiden Prädikate sind unter
/// NaN keine Komplemente, und genau dazwischen panickte der Score-Bau. Ein
/// Absturz beim Einreichen kostet den ganzen Flug. Jetzt gibt es nur noch
/// EINEN Ort, an dem die Werte entstehen, und er kann nicht lügen.
///
/// Reihenfolge wie beim Bremsweg-Score: erst Voraussetzungen (die
/// spezifischere Aussage), dann fehlende Daten, dann Plausibilität.
fn gepruefte_werte(input: &AlignmentInput) -> Result<(f32, f32), &'static str> {
    if input.nicht_konventionell {
        return Err("not_applicable_for_category");
    }
    if input.airport_source.as_deref() != Some("runway_match") {
        return Err("alignment_off_airport");
    }
    if input.runway_geometry_trusted != Some(true) {
        return Err("alignment_untrusted_geometry");
    }
    // `is_finite` statt `is_some`: ein NaN-Messwert ist keine Messung. Er
    // rutschte sonst durch jeden Stufenvergleich (NaN <= x ist immer false)
    // und landete als HÄRTESTE Note im Score — das Gegenteil des Versprechens
    // „fehlende Größen bestrafen nie".
    let offset = match input.centerline_offset_m {
        Some(o) if o.is_finite() => o.abs(),
        _ => return Err("missing_centerline_offset"),
    };
    // Ohne Breite wäre nur die absolute Meterzahl da — und die ist je nach
    // Bahn etwas völlig anderes. Lieber nicht bewerten als falsch bewerten.
    // Obergrenze, weil eine unendliche oder absurde Breite den Anteil gegen
    // null drückt und die Achse zum stillen 100-Punkte-Geschenk machte.
    let breite = match input.runway_width_m {
        Some(w) if w.is_finite() && w > 0.0 && w <= MAX_PLAUSIBLE_BAHNBREITE_M => w,
        _ => return Err("missing_runway_width"),
    };
    // Der Bahnkurs fehlt im OurAirports-Ersatzpfad. Ohne ihn ließe sich nur
    // der Versatz bewerten — dann verlöre die Achse aber genau die Größe,
    // die den Datenfehler von der Fehllandung trennt (siehe unten).
    let kurs = match input.runway_true_course_deg {
        Some(c) if c.is_finite() => c,
        _ => return Err("missing_runway_course"),
    };
    let heading = match input.heading_true_deg {
        Some(h) if h.is_finite() => h,
        _ => return Err("missing_heading"),
    };

    let anteil = offset / (breite / 2.0);
    let kurs_abw = kursdifferenz(heading, kurs);

    // Falsches Bahnende gematcht (Landung nahe der Bahnmitte, Divert): die
    // Kursabweichung springt auf ~180°. Das ist kein schiefes Aufsetzen,
    // sondern eine Fehlzuordnung — dieselbe Klasse wie der EDHE-Fall.
    if kurs_abw > MAX_BEWERTBARE_KURSABWEICHUNG_DEG {
        return Err("implausible_runway_geometry");
    }
    // Geometrie fragwürdig: weit daneben (relativ UND absolut), aber sauber
    // ausgerichtet.
    if anteil >= GEOMETRIE_FRAGWUERDIG_AB_ANTEIL
        && offset >= GEOMETRIE_FRAGWUERDIG_AB_METER
        && kurs_abw <= GEOMETRIE_FRAGWUERDIG_MAX_KURS_DEG
    {
        return Err("implausible_runway_geometry");
    }
    Ok((anteil, kurs_abw))
}

/// Der Ausrichtungs-Sub-Score. Die schlechtere der beiden Größen bestimmt
/// die Punktzahl — wer die Mittellinie trifft, aber 15° schräg steht, hat
/// genauso ein Problem wie umgekehrt.
pub fn sub_alignment(input: &AlignmentInput) -> SubScoreEntry {
    let (anteil, kurs_abw) = match gepruefte_werte(input) {
        Ok(werte) => werte,
        Err(grund) => {
            return SubScoreEntry::skipped("alignment", "landing.sub.alignment", grund)
        }
    };
    let offset_m = anteil * input.runway_width_m.unwrap_or_default() / 2.0;

    let p_versatz = punkte_aus_stufen(anteil, &VERSATZ_STUFEN, VERSATZ_DARUEBER);
    let p_kurs = punkte_aus_stufen(kurs_abw, &KURS_STUFEN, KURS_DARUEBER);
    let punkte = p_versatz.min(p_kurs);

    // Die Begründung nennt die Größe, die den Ausschlag gab — sonst rät der
    // Pilot, woran es lag. Bei Gleichstand gewinnt die Kursabweichung nur,
    // wenn sie wirklich die schlechtere ist (strikt kleiner).
    let grund = if punkte >= 100 {
        "aligned_on_centerline"
    } else if p_kurs < p_versatz {
        "crooked_touchdown"
    } else if anteil > 1.0 {
        "off_runway_surface"
    } else {
        "off_centerline"
    };

    // `value` bleibt sprachneutral (Zahlen + Einheiten), `extra` bleibt LEER:
    // seit v0.12.0 gilt „kein hartkodiertes Deutsch im Crate" — die Strings
    // wandern sonst in den gespeicherten Datensatz und erscheinen auf jeder
    // Oberfläche, auch der englischen und italienischen (QS-Befund v1.6.2).
    SubScoreEntry::scored(
        "alignment",
        "landing.sub.alignment",
        punkte,
        format!("{offset_m:.0} m · {kurs_abw:.0}°"),
        grund,
        crate::band_from_points(punkte),
    )
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
            nicht_konventionell: false,
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
            (AlignmentInput { airport_source: Some("nearest".into()), ..basis() }, "alignment_off_airport"),
            (AlignmentInput { runway_geometry_trusted: None, ..basis() }, "alignment_untrusted_geometry"),
            (AlignmentInput { runway_geometry_trusted: Some(false), ..basis() }, "alignment_untrusted_geometry"),
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
    fn drehfluegler_werden_nicht_auf_bahnachse_bewertet() {
        let e = sub_alignment(&AlignmentInput {
            nicht_konventionell: true,
            heading_true_deg: Some(160.0),
            ..basis()
        });
        assert!(e.skipped);
        assert_eq!(e.reason.as_deref(), Some("not_applicable_for_category"));
    }

    #[test]
    fn schmale_buschpiste_wird_nicht_entschuldigt() {
        // 18-m-Piste, 20 m daneben, sauber ausgerichtet: relativ waere das
        // ueber der Zweifelsschwelle (Anteil 2,2), absolut sind 20 m aber
        // kein Kartenfehler — das ist eine echte Neben-die-Piste-Landung.
        let e = sub_alignment(&AlignmentInput {
            centerline_offset_m: Some(20.0),
            runway_width_m: Some(18.0),
            ..basis()
        });
        assert!(!e.skipped, "20 m auf schmaler Piste ist kein Kartenfehler");
        assert_eq!(e.points, 15);
    }

    #[test]
    fn kursdifferenz_ueber_den_nordpunkt() {
        assert!((kursdifferenz(359.0, 1.0) - 2.0).abs() < 1e-4);
        assert!((kursdifferenz(1.0, 359.0) - 2.0).abs() < 1e-4);
        assert!((kursdifferenz(90.0, 270.0) - 180.0).abs() < 1e-4);
        assert!((kursdifferenz(70.0, 70.0)).abs() < 1e-4);
    }

    #[test]
    fn nan_und_unendlich_werden_nie_bewertet() {
        // QS-Befund v1.6.2, mit cargo test reproduziert: NaN rutschte durch
        // jeden Stufenvergleich und landete als HÄRTESTE Note im Score —
        // eine NaN-Bahnbreite brachte den Score-Bau sogar zum Absturz.
        let faelle: [(AlignmentInput, &str); 5] = [
            (AlignmentInput { runway_width_m: Some(f32::NAN), ..basis() }, "missing_runway_width"),
            (AlignmentInput { runway_width_m: Some(f32::INFINITY), ..basis() }, "missing_runway_width"),
            (AlignmentInput { centerline_offset_m: Some(f32::NAN), ..basis() }, "missing_centerline_offset"),
            (AlignmentInput { heading_true_deg: Some(f32::NAN), ..basis() }, "missing_heading"),
            (AlignmentInput { runway_true_course_deg: Some(f32::NAN), ..basis() }, "missing_runway_course"),
        ];
        for (input, erwartet) in faelle {
            let e = sub_alignment(&input);
            assert!(e.skipped, "{erwartet}: NaN/Inf darf nie bewertet werden");
            assert_eq!(e.reason.as_deref(), Some(erwartet));
        }
    }

    #[test]
    fn falsches_bahnende_wird_nicht_als_schraeglandung_bestraft() {
        // Matcht der Runway-Match das andere Bahnende (Landung nahe der
        // Bahnmitte, Divert), springt die Kursabweichung auf ~180°. Das ist
        // eine Fehlzuordnung, kein schiefes Aufsetzen.
        let e = sub_alignment(&AlignmentInput {
            heading_true_deg: Some(250.0),
            runway_true_course_deg: Some(70.0),
            ..basis()
        });
        assert!(e.skipped);
        assert_eq!(e.reason.as_deref(), Some("implausible_runway_geometry"));
    }

    #[test]
    fn bandgrenzen_liegen_auf_der_besseren_seite() {
        // Krumme Navdaten-Breiten (45,1 m) treffen die Grenzen nicht exakt:
        // 16,9125 m ergibt rechnerisch 0,75, in f32 aber 0,75000006. Ohne
        // Toleranz fiele der Pilot bei angezeigten „75 %" ins nächste Band.
        let e = sub_alignment(&AlignmentInput {
            centerline_offset_m: Some(16.912_5),
            runway_width_m: Some(45.1),
            ..basis()
        });
        assert_eq!(e.points, 65, "exakte Bandgrenze gehoert ins bessere Band");
        // Und die Grenzen selbst, auf einer glatten Bahn:
        for (anteil, erwartet) in [(0.25f32, 100u8), (0.50, 85), (0.75, 65), (1.00, 40)] {
            let e = sub_alignment(&AlignmentInput {
                centerline_offset_m: Some(anteil * 22.5),
                runway_width_m: Some(45.0),
                ..basis()
            });
            assert_eq!(e.points, erwartet, "Versatz-Anteil {anteil}");
        }
        for (grad, erwartet) in [(3.0f32, 100u8), (6.0, 85), (10.0, 65), (15.0, 40)] {
            let e = sub_alignment(&AlignmentInput {
                heading_true_deg: Some(70.0 + grad),
                ..basis()
            });
            assert_eq!(e.points, erwartet, "Kursabweichung {grad}");
        }
    }

    #[test]
    fn geometrie_schutz_greift_genau_an_seinen_grenzen() {
        // Beide Konstanten einklemmen, damit eine „Feinjustierung" nicht
        // stillschweigend eine ganze Fallklasse umdeutet.
        let bei = |anteil: f32, kurs: f32| {
            sub_alignment(&AlignmentInput {
                centerline_offset_m: Some(anteil * 22.5),
                runway_width_m: Some(45.0),
                heading_true_deg: Some(70.0 + kurs),
                ..basis()
            })
        };
        assert!(bei(2.01, 4.99).skipped, "weit daneben + sauber = Kartenfehler");
        assert!(!bei(1.99, 4.99).skipped, "knapp darunter wird bewertet");
        assert!(!bei(2.01, 5.01).skipped, "weit daneben + schief = echter Fehler");
    }

    #[test]
    fn stufen_sind_monoton() {
        // Mehr Abweichung darf nie mehr Punkte geben.
        let mut vorher = 101u8;
        for anteil in [0.0f32, 0.2, 0.3, 0.6, 0.8, 1.0, 1.5, 1.9] {
            let e = sub_alignment(&AlignmentInput {
                centerline_offset_m: Some(anteil * 22.5),
                runway_width_m: Some(45.0),
                // Kurs PERFEKT: sonst deckelt die Kursstufe die Punkte und
                // die Versatz-Leiter wird nie wirklich geprueft (QS-Befund —
                // der erste Wurf dieses Tests war damit hohl).
                heading_true_deg: Some(70.0),
                runway_true_course_deg: Some(70.0),
                ..basis()
            });
            // Ueber dem Geometrie-Schutz (Anteil >= 2 bei sauberem Kurs)
            // wird bewusst nicht mehr bewertet — dort endet die Leiter.
            if e.skipped {
                assert!(anteil >= GEOMETRIE_FRAGWUERDIG_AB_ANTEIL);
                continue;
            }
            assert!(e.points <= vorher, "Anteil {anteil}: {} > {vorher}", e.points);
            assert!(e.points >= 15);
            vorher = e.points;
        }
    }
}
