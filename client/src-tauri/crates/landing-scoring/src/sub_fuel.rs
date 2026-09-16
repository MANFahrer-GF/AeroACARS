//! Fuel Sub-Score.
//!
//! Phase 0 (jetzt): 1:1-Port von TS `subFuel` in landingScoring.ts:207-216.
//!   - Symmetrische Schwelle (Math.abs(efficiency)).
//!   - Returns `None` wenn efficiency_pct nicht verfuegbar ist.
//!
//! Phase 2/F2+F3 (spaeter): wird durch `sub_fuel_v0_7_1` ersetzt mit
//!   - Hard-Gate: kein planned_burn → skipped (kein Fallback)
//!   - Asymmetrie: Minderverbrauch nicht bestrafen
//!   - Label-Wechsel "Spritverbrauch" → "OFP-Treue"
//!
//! v1.6.7 (score_algorithm_version 7→8): die Asymmetrie war nur halb
//! umgesetzt. Der Kopf sagte „Minderverbrauch wird nicht bestraft", die
//! Tabelle zog ab 5 % Minderverbrauch trotzdem 5 Punkte ab (95 statt 100)
//! — der Widerspruch stammt 1:1 aus dem Spec-Entwurf (docs/spec/historical/
//! v0.7.1-landing-ux-fairness.md §F3, „nie Strafe" direkt ueber `score(95,
//! ...)`). Ausloeser war EWG 2047 (EDDH→EDDS, 16.08.2026): 2047,8 statt
//! 2163 kg geplant = -5,3 %, also 0,32 Prozentpunkte ueber der Grenze —
//! sieben Kilo mehr Verbrauch haetten die volle Punktzahl gegeben.
//! Jetzt: Minderverbrauch bis 15 % = 100 Punkte. Erst jenseits der 15 %
//! bleibt es bei 85 + Warnung — und auch das ist keine Strafe fuers
//! Sparen, sondern der Hinweis, dass eher der Plan als der Flug falsch war.
//!
//! Phase 0 behaelt die Legacy-Funktion fuer Goldenset-Tests.
//! Phase 2 fuegt `sub_fuel_v0_7_1` hinzu — wird ab v0.7.1 verwendet.

use crate::{Band, SubScoreEntry};

/// v0.7.1 Phase 2 (F2 + F3): Fuel-Score mit Hard-Gate + Asymmetrie.
///
/// F2: kein planned_burn → skipped (KEIN Fallback)
///     kein actual_trip_burn → skipped
/// F3: efficiency = (actual - planned) / planned * 100
///     Mehrverbrauch (efficiency > 0): wie Legacy bestraft
///     Minderverbrauch (efficiency <= 0): NIE bestraft — bis 15 % unter
///       Plan volle 100 Punkte (v1.6.7; vorher 95 ab 5 % unter Plan)
///     Starker Minderverbrauch (>15% under): 85 + Warning
///       "planned_burn_may_be_off" — Zweifel am Plan, nicht am Piloten
/// Label-Aenderung: "Spritverbrauch" → "OFP-Treue" (i18n key bleibt
/// `landing.sub.fuel`, der String dahinter aendert sich in Phase 3).
/// Toleranzboden als Anteil des Abfluggewichts.
///
/// # Warum ein Boden ueberhaupt, und warum kein fester
///
/// Ein reiner Prozentsatz bestraft KURZE Fluege systematisch: Rollzeit,
/// ein Vektor, eine Warteschleife kosten immer etwa dasselbe, laufen bei
/// einem kurzen Flug aber gegen einen kleinen Nenner.
///
/// Gemeldet an einem Flug mit 208 t Abfluggewicht und nur 4,78 t
/// geplantem Verbrauch (knapp eine Stunde fuer ein Grossraumflugzeug):
/// 400 kg mehr = **+8 %** und damit 55 statt 100 Punkte. Dieselben 400 kg
/// auf einem Zwoelfstundenflug waeren 0,4 % und volle Punktzahl.
///
/// Ein FESTER Boden waere aber genauso falsch: 400 kg sind fuer eine
/// C172 mehr als ihr ganzer Reiseverbrauch. Deshalb waechst er mit dem
/// Abfluggewicht — 0,2 % davon entsprechen grob ein paar Minuten Flug,
/// unabhaengig von der Groesse:
///
///   208 t  →  416 kg      57 t  →  114 kg      1,1 t  →  2,2 kg
///
/// Ohne bekanntes Abfluggewicht bleibt es beim reinen Prozentsatz.
const TOLERANZ_ANTEIL_VOM_TOW: f32 = 0.002;

// ─── v1.7.32: Umweg und Verfahren gehen nicht mehr auf die Note ──────
//
// # Der Anlass
//
// THY 1068 (LRCK→LTFM, 16.09.2026): Plan 1929 kg, geflogen 2189 kg =
// +13,5 % → 55 Punkte. Geflogen wurden 277 NM bei 186 NM Luftlinie, also
// rund 90 NM Mehrweg. Der Mehrverbrauch war die Folge von Vektoren, nicht
// von schlechter Flugführung.
//
// # Was der Bestand sagt (1118 Flüge mit OFP-Daten, Recorder)
//
//   172 Flüge (15,4 %) verloren Punkte — davon 76 % mit mindestens 10 %
//   Mehrweg gegenüber der Luftlinie (Median 17,3 % gegen 11,7 % bei den
//   Flügen ohne Abzug). Und: 22 von 42 Flügen MIT Durchstartmanöver
//   verloren Punkte, also die Hälfte gegen 15 % im Schnitt. Wer
//   durchstartet, tut das Richtige und wurde dafür bestraft.
//
// # Die Regel
//
// Bewertet wird der Mehrverbrauch NACH Abzug der Arbeit, die der Pilot
// nicht zu verantworten hat:
//
//   1. Eine geplante Route ist immer länger als die Luftlinie (typisch
//      5-15 %). GRUNDKORRIDOR gilt deshalb als eingeplant; erst der
//      Mehrweg DARÜBER wird gutgeschrieben. Sonst bekäme der Pilot
//      Strecke gutgeschrieben, die längst im Plan steckt.
//   2. Zusatzmeilen zählen nur mit ZUSATZMEILE_ANTEIL des
//      Schnittverbrauchs: Rollen, Start und Steigflug hängen nicht an
//      der Strecke (externe QS, Codex 16.09.2026).
//   3. Durchstarten wird in KILOGRAMM gutgeschrieben, nicht in Prozent —
//      ein Prozentsatz wäre auf Langstrecke großzügig und auf
//      Kurzstrecke zu knapp.
//   4. Warteschleifen brauchen keine eigene Erkennung: Sie erzeugen
//      Strecke und laufen damit in Punkt 1 ein. Eine echte,
//      zeitbasierte Erkennung bleibt der nächste Ausbau.
//   5. Oberhalb von UMWEG_DECKEL lässt sich ein ATC-Vektor nicht mehr
//      von einem selbst gewählten Umweg unterscheiden → die Achse sagt
//      „nicht bewertbar" statt eine Note zu erfinden. Dasselbe bei
//      Rundflügen und sehr kurzen Strecken (Luftlinie ≈ 0 ergibt
//      absurde Verhältnisse — im Bestand ein Flug mit 11696 %).
//
// Die Bänder sind mitgewandert (3/8/15/25 statt 5/10/20/35): Wer die
// Bezugsgröße entschärft und die Grenzen lässt, entkernt die Achse —
// gemessen am Bestand bleiben so 12,3 % Abzüge statt 15,4 %, aber sie
// treffen die Richtigen.

/// Anteil, um den eine geplante Route typisch länger ist als die
/// Luftlinie. Wird NICHT gutgeschrieben.
const GRUNDKORRIDOR: f32 = 0.10;
/// Anteil des Schnittverbrauchs, mit dem eine Zusatzmeile zählt.
const ZUSATZMEILE_ANTEIL: f32 = 0.80;
/// Gutschrift je Durchstartmanöver, in kg je Tonne Abfluggewicht
/// (A320 ≈ 220 kg, Großraumjet ≈ 750 kg).
const DURCHSTART_KG_JE_TONNE: f32 = 3.0;
/// Ab hier ist der Mehrweg nicht mehr zurechenbar → nicht bewertbar.
const UMWEG_DECKEL: f32 = 0.50;
/// Unter dieser Planstrecke gibt es keinen brauchbaren Bezug
/// (Rundflug, Platzrunde).
const MIN_PLAN_STRECKE_NM: f32 = 25.0;

/// Was der Flug an Zusatzarbeit hatte, die der Pilot nicht zu
/// verantworten hat — zusammen mit dem, was davon gutgeschrieben wird.
#[derive(Debug, Clone, Copy, Default)]
pub struct ZusatzArbeit {
    /// Tatsächlich geflogene Strecke.
    pub geflogene_strecke_nm: Option<f32>,
    /// Geplante Streckenlaenge aus dem Flugplan (phpVMS).
    pub plan_strecke_nm: Option<f32>,
    /// Zahl der Durchstartmanöver.
    pub durchstarts: u32,
}

pub fn sub_fuel_v0_7_1(
    planned_burn_kg: Option<f32>,
    actual_trip_burn_kg: Option<f32>,
    planned_tow_kg: Option<f32>,
    diverted: Option<bool>,
) -> SubScoreEntry {
    sub_fuel_v1_7_32(
        planned_burn_kg,
        actual_trip_burn_kg,
        planned_tow_kg,
        diverted,
        ZusatzArbeit::default(),
    )
}

/// OFP-Treue mit Gutschrift für Mehrweg und Durchstartmanöver — siehe
/// den Block über [`GRUNDKORRIDOR`] für Anlass, Zahlen und Regel.
pub fn sub_fuel_v1_7_32(
    planned_burn_kg: Option<f32>,
    actual_trip_burn_kg: Option<f32>,
    planned_tow_kg: Option<f32>,
    diverted: Option<bool>,
    zusatz: ZusatzArbeit,
) -> SubScoreEntry {
    // ⚠ Nach einem Ausweichflug ist die OFP-Treue nicht bewertbar.
    //
    // Es wurde eine ANDERE Strecke geflogen als die geplante. Ein
    // Vergleich gegen den urspruenglichen Verbrauch misst den Umweg,
    // nicht den Piloten — und faellt je nach Richtung als "gespart" oder
    // "verschwendet" aus, beides ohne Aussage.
    //
    // (Thomas, 30.08.2026: „starker Minderverbrauch kann ja nicht sein,
    // oder nur bei Divert." Genau so — deshalb hier ueberspringen statt
    // eine Zahl zu erfinden.)
    if diverted == Some(true) {
        return SubScoreEntry::skipped("fuel", "landing.sub.fuel", "diverted");
    }
    let Some(planned) = planned_burn_kg else {
        return SubScoreEntry::skipped("fuel", "landing.sub.fuel", "no_planned_burn");
    };
    if planned <= 0.0 {
        return SubScoreEntry::skipped("fuel", "landing.sub.fuel", "no_planned_burn");
    }
    let Some(actual) = actual_trip_burn_kg else {
        return SubScoreEntry::skipped("fuel", "landing.sub.fuel", "no_actual_burn");
    };

    // ── Zusatzarbeit, die nicht auf die Note geht ────────────────────
    let roh = ((actual - planned) / planned) * 100.0;
    let mut erklaerung: Vec<String> = Vec::new();
    let mut gutschrift_kg = 0.0_f32;
    // Ohne Plandistanz gibt es keine Mehrweg-Gutschrift — dann gelten die
    // ALTEN, milderen Bänder. Sonst träfen die gestrafften Grenzen genau
    // die Flüge, denen der Ausgleich fehlt: Ein Neustart mitten im Flug
    // oder ein Altbestand-Resume würde bestraft (externe QS, Codex
    // 16.09.2026).
    let mit_streckenbezug = zusatz
        .plan_strecke_nm
        .is_some_and(|v| v >= MIN_PLAN_STRECKE_NM)
        && zusatz.geflogene_strecke_nm.is_some_and(|v| v > 0.0);

    if let (Some(geflogen), Some(plan_strecke)) = (
        zusatz.geflogene_strecke_nm.filter(|v| *v > 0.0),
        zusatz.plan_strecke_nm.filter(|v| *v > 0.0),
    ) {
        if plan_strecke < MIN_PLAN_STRECKE_NM {
            // Rundflug/Platzrunde: ohne brauchbare Plandistanz kein Bezug.
            return SubScoreEntry::skipped("fuel", "landing.sub.fuel", "kein_streckenbezug");
        }
        // ⚠ Der Deckel gilt gegen die PLANSTRECKE selbst. Gegen die schon
        // um den Grundkorridor vergroesserte Strecke gerechnet spraeche die
        // Anzeige von „mehr als 50 %", waehrend erst bei 65 % ueber Plan
        // uebersprungen wuerde (externe QS, Codex 16.09.2026).
        let umweg = geflogen / plan_strecke - 1.0;
        if umweg > UMWEG_DECKEL {
            return SubScoreEntry::skipped("fuel", "landing.sub.fuel", "umweg_zu_gross");
        }
        // Der Grundkorridor gilt NUR fuer die Gutschrift: bis dahin steckt
        // der Mehrweg im Plan.
        let eingeplant_nm = plan_strecke * (1.0 + GRUNDKORRIDOR);
        let zusatz_nm = (geflogen - eingeplant_nm).max(0.0);
        if zusatz_nm > 0.0 {
            // ⚠ Schnittverbrauch gegen die PLANSTRECKE. Mit der
            // vergroesserten Strecke im Nenner waeren es effektiv nur
            // 0,8/1,1 = 0,73 statt der gewollten 0,8 (externe QS, Codex).
            let kg = zusatz_nm * (planned / plan_strecke) * ZUSATZMEILE_ANTEIL;
            gutschrift_kg += kg;
            erklaerung.push(format!("{zusatz_nm:.0} NM Mehrweg (−{kg:.0} kg)"));
        }
    }
    // ⚠ Ohne bekanntes Abfluggewicht gibt es KEINE Durchstart-Gutschrift.
    // Ein pauschaler Ersatzwert (frueher 70 t = 210 kg je Durchstart) ist
    // fuer einen Manual-/VFR-Flug ohne OFP absurd — bei 60 kg Planverbrauch
    // haette er den ganzen Verbrauch ueberkompensiert (externe QS, Codex
    // 16.09.2026). Lieber keine Gutschrift als eine erfundene.
    if zusatz.durchstarts > 0 {
        if let Some(tonnen) = planned_tow_kg.filter(|t| *t > 0.0).map(|t| t / 1000.0) {
            let kg = zusatz.durchstarts as f32 * DURCHSTART_KG_JE_TONNE * tonnen;
            gutschrift_kg += kg;
            erklaerung.push(format!("{}× Durchstarten (−{kg:.0} kg)", zusatz.durchstarts));
        }
    }

    // ⚠ Die Gutschrift kann den Mehrverbrauch nur AUSGLEICHEN, nie
    // unterbieten. Ohne Deckel wurde aus einem normalen Flug fälschlich
    // „sehr sparsam" samt Hinweis „der Plan könnte falsch sein" — bei
    // THY 1068 standen 597 kg Gutschrift gegen 260 kg Mehrverbrauch
    // (externe QS, Codex 16.09.2026). Sparen soll der Pilot selbst; die
    // Gutschrift nimmt ihm nur die fremde Zusatzarbeit ab.
    let mehrverbrauch = (actual - planned).max(0.0);
    let angerechnet_kg = gutschrift_kg.min(mehrverbrauch);
    // Die Erklärung sagt, was WIRKLICH angerechnet wurde — nicht, was
    // rechnerisch möglich gewesen wäre. Wer ohnehin unter Plan lag,
    // braucht keine Gutschrift und soll auch keine angezeigt bekommen.
    if angerechnet_kg <= 0.0 {
        erklaerung.clear();
    } else if angerechnet_kg < gutschrift_kg {
        // JEDE Teilanrechnung wird benannt — ohne Schwelle. `angerechnet`
        // ist das Minimum aus Gutschrift und Mehrverbrauch, bei voller
        // Anrechnung ist die Differenz exakt 0; eine Toleranz braucht es
        // hier also nicht, und sie hat nur Faelle verschluckt (externe
        // QS, Codex 16.09.2026).
        erklaerung.push(format!("auf {angerechnet_kg:.0} kg Mehrverbrauch begrenzt"));
    }
    let actual = actual - angerechnet_kg;
    // ⚠ Gerundet wird EINMAL, vor der Bandentscheidung — sonst stehen
    // +2,96 % und +3,04 % beide als „+3.0%" da und bekommen 100 bzw. 80
    // Punkte (externe QS, Codex 16.09.2026).
    let efficiency = (((actual - planned) / planned) * 1000.0).round() / 10.0;
    let kern = if efficiency > 0.0 {
        format!("+{:.1}%", efficiency)
    } else {
        format!("{:.1}%", efficiency)
    };
    // Die Anzeige erklärt sich selbst: roher Wert, was gutgeschrieben
    // wurde und wofür, und was am Ende bewertet wird.
    let value = if erklaerung.is_empty() {
        kern
    } else {
        format!(
            "{kern} bewertet · roh {}{:.1}% · {}",
            if roh > 0.0 { "+" } else { "" },
            roh,
            erklaerung.join(" · ")
        )
    };

    // ⚠ Der Toleranzboden gilt VOR den Prozentbaendern.
    let toleranz_kg = planned_tow_kg
        .filter(|t| *t > 0.0)
        .map(|t| t * TOLERANZ_ANTEIL_VOM_TOW)
        .unwrap_or(0.0);
    if efficiency > 0.0 && (actual - planned) <= toleranz_kg {
        return SubScoreEntry::scored(
            "fuel",
            "landing.sub.fuel",
            100,
            value,
            "on_plan",
            Band::Good,
        );
    }

    if efficiency > 0.0 {
        // Mehrverbrauch — score-relevant wie Legacy
        // ⚠ Band auf +5 % geweitet (vorher +2 %).
        //
        // Gemessen ueber 412 Fluege seit dem 1. Juli: **45 % aller
        // Grossraum-Landungen** und 21 % der schmaleren lagen ueber +2 %
        // und verloren Punkte. Eine Schwelle, die die Mehrheit trifft,
        // misst kein Koennen mehr, sondern Rauschen — in der Luftfahrt
        // gilt eine OFP-Genauigkeit von ±5 % als gut.
        //
        // Wind weicht vom Prognosewert ab, ATC vektort, Steigprofile
        // unterscheiden sich. Nichts davon ist dem Piloten anzulasten.
        if efficiency < if mit_streckenbezug { 3.0 } else { 5.0 } {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                100,
                value,
                "on_plan",
                Band::Good,
            )
        } else if efficiency < if mit_streckenbezug { 8.0 } else { 10.0 } {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                80,
                value,
                "near_plan",
                Band::Good,
            )
        } else if efficiency < if mit_streckenbezug { 15.0 } else { 20.0 } {
            SubScoreEntry::scored("fuel", "landing.sub.fuel", 55, value, "off_plan", Band::Ok)
        } else if efficiency < if mit_streckenbezug { 25.0 } else { 35.0 } {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                25,
                value,
                "very_off_plan",
                Band::Bad,
            )
        } else {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                5,
                value,
                "way_off_plan",
                Band::Bad,
            )
        }
    } else {
        // Minderverbrauch (efficiency <= 0) — KEIN Penalty. v1.6.7: das
        // gilt jetzt auch zwischen 5 % und 15 % unter Plan (vorher 95).
        // Die Rationale bleibt getrennt, damit der Pilot „Effizient
        // (Minderverbrauch)" liest und nicht „Auf Plan" — gleiche Punkte,
        // ehrlichere Aussage.
        let under = efficiency.abs();
        if under < 5.0 {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                100,
                value,
                "on_plan",
                Band::Good,
            )
        } else if under < 15.0 {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                100,
                value,
                "efficient",
                Band::Good,
            )
        } else {
            // ⚠ Auch starker Minderverbrauch kostet KEINE Punkte mehr.
            //
            // Vorher: 85 Punkte ab 15 % unter Plan. Der Kopf dieser Datei
            // sagt seit jeher „Minderverbrauch wird nie bestraft" — und
            // die Tabelle tat es trotzdem, nur eine Stufe weiter hinten.
            // Zum zweiten Mal derselbe Widerspruch (v1.6.7 hat ihn schon
            // einmal zwischen 5 % und 15 % beseitigt).
            //
            // Sparen darf nichts kosten. Der Hinweis, dass eher der PLAN
            // als der Flug falsch war, bleibt als Warnung erhalten — aber
            // als Information, nicht als Abzug.
            let mut entry = SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                100,
                value,
                "very_efficient",
                Band::Good,
            );
            entry.warning = Some("planned_burn_may_be_off".to_string());
            entry
        }
    }
}

pub fn sub_fuel_legacy(efficiency_pct: Option<f32>) -> Option<SubScoreEntry> {
    let pct = efficiency_pct?;
    let dev = pct.abs();
    let value = if pct > 0.0 {
        format!("+{:.1}%", pct)
    } else {
        format!("{:.1}%", pct)
    };

    let entry = if dev < 2.0 {
        SubScoreEntry::scored(
            "fuel",
            "landing.sub.fuel",
            100,
            value,
            "on_plan",
            Band::Good,
        )
    } else if dev < 5.0 {
        SubScoreEntry::scored(
            "fuel",
            "landing.sub.fuel",
            80,
            value,
            "near_plan",
            Band::Good,
        )
    } else if dev < 10.0 {
        SubScoreEntry::scored("fuel", "landing.sub.fuel", 55, value, "off_plan", Band::Ok)
    } else if dev < 20.0 {
        SubScoreEntry::scored(
            "fuel",
            "landing.sub.fuel",
            25,
            value,
            "very_off_plan",
            Band::Bad,
        )
    } else {
        SubScoreEntry::scored(
            "fuel",
            "landing.sub.fuel",
            5,
            value,
            "way_off_plan",
            Band::Bad,
        )
    };
    Some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(pct: f32) -> (u8, String) {
        let s = sub_fuel_legacy(Some(pct)).unwrap();
        (s.points, s.rationale_key.unwrap())
    }

    #[test]
    fn none_returns_none() {
        assert!(sub_fuel_legacy(None).is_none());
    }

    #[test]
    fn ts_table_match_symmetric() {
        // Phase-0 Legacy: Math.abs → -5% gleich +5%
        assert_eq!(run(0.0), (100, "landing.rat.on_plan".into()));
        assert_eq!(run(1.99), (100, "landing.rat.on_plan".into()));
        assert_eq!(run(-1.99), (100, "landing.rat.on_plan".into()));
        assert_eq!(run(2.0), (80, "landing.rat.near_plan".into()));
        assert_eq!(run(-2.0), (80, "landing.rat.near_plan".into()));
        assert_eq!(run(4.99), (80, "landing.rat.near_plan".into()));
        assert_eq!(run(5.0), (55, "landing.rat.off_plan".into()));
        assert_eq!(run(-7.5), (55, "landing.rat.off_plan".into()));
        assert_eq!(run(10.0), (25, "landing.rat.very_off_plan".into()));
        assert_eq!(run(-15.0), (25, "landing.rat.very_off_plan".into()));
        assert_eq!(run(20.0), (5, "landing.rat.way_off_plan".into()));
        assert_eq!(run(-30.0), (5, "landing.rat.way_off_plan".into()));
    }

    #[test]
    fn value_format_matches_ts() {
        assert_eq!(sub_fuel_legacy(Some(5.2)).unwrap().value.unwrap(), "+5.2%");
        assert_eq!(sub_fuel_legacy(Some(-5.2)).unwrap().value.unwrap(), "-5.2%");
        assert_eq!(sub_fuel_legacy(Some(0.0)).unwrap().value.unwrap(), "0.0%");
    }

    // ─── v0.7.1 sub_fuel_v0_7_1 (F2 Hard-Gate + F3 Asymmetrie) ────────

    #[test]
    fn v0_7_1_hard_gate_no_planned() {
        let s = sub_fuel_v0_7_1(None, Some(5000.0), None, None);
        assert!(s.skipped);
        assert_eq!(s.reason.as_deref(), Some("no_planned_burn"));
        assert_eq!(s.score, 0); // skipped → 0 — wird via aggregate ignoriert
    }

    #[test]
    fn v0_7_1_hard_gate_no_actual() {
        let s = sub_fuel_v0_7_1(Some(5000.0), None, None, None);
        assert!(s.skipped);
        assert_eq!(s.reason.as_deref(), Some("no_actual_burn"));
    }

    #[test]
    fn v0_7_1_hard_gate_zero_planned() {
        let s = sub_fuel_v0_7_1(Some(0.0), Some(5000.0), None, None);
        assert!(s.skipped);
        assert_eq!(s.reason.as_deref(), Some("no_planned_burn"));
    }

    #[test]
    fn v0_7_1_overburn_punished() {
        // v1.7.12: +7 % gibt jetzt 80 statt 55 — das 100-Punkte-Band
        // reicht bis +5 %, siehe `ofp_treue_baender_v1_7_12`.
        let s = sub_fuel_v0_7_1(Some(5000.0), Some(5350.0), None, None); // +7%
        assert_eq!(s.score, 80);
        // Der Begruendungstext wandert mit: +7 % heisst jetzt "nahe am
        // Plan", nicht mehr "abseits". Die Texte sind gegen die neuen
        // Baender geprueft — 5 / 10 / 20 / 35 %.
        assert_eq!(s.rationale_key.as_deref(), Some("landing.rat.near_plan"));
        assert!(s.warning.is_none());
    }

    #[test]
    fn v0_7_1_underburn_not_punished() {
        // v1.6.7: -10% Minderverbrauch → 100 (efficient), KEIN Warning.
        // Vorher 95 — der Abzug widersprach der eigenen Doku.
        let s = sub_fuel_v0_7_1(Some(5000.0), Some(4500.0), None, None);
        assert_eq!(s.score, 100);
        assert_eq!(s.rationale_key.as_deref(), Some("landing.rat.efficient"));
        assert!(s.warning.is_none());
    }

    /// v1.6.7 — der Fall, der die Aenderung ausgeloest hat: EWG 2047
    /// (EDDH→EDDS, 16.08.2026). Geplant 2163 kg, verbrannt 2047,83 kg =
    /// -5,32 %. Lag 0,32 Prozentpunkte hinter der alten 5-%-Grenze und
    /// kostete deshalb 5 Punkte auf der Achse (98 statt 100 gesamt).
    #[test]
    fn v1_6_7_ewg2047_underburn_scores_full() {
        let s = sub_fuel_v0_7_1(Some(2163.0), Some(2047.8259), None, None);
        assert_eq!(s.score, 100);
        assert_eq!(s.rationale_key.as_deref(), Some("landing.rat.efficient"));
        assert_eq!(s.value.as_deref(), Some("-5.3%"));
        assert!(s.warning.is_none());
    }

    /// Bandgrenzen des Minderverbrauchs am Stueck — inklusive der Stelle,
    /// an der es NICHT mehr 100 gibt. Ohne diesen Test kann die
    /// 15-%-Warnschwelle still mitwandern, wenn jemand die Zahlen
    /// anfasst.
    #[test]
    fn v1_6_7_underburn_band_edges() {
        // ⚠ v1.7.32: gerundet wird EINMAL, vor der Bandentscheidung —
        // sonst zeigt die Karte „-5.0%" und bewertet -4,95 %. Dieser Fall
        // liegt jetzt also im Sparen-Band; die Punktzahl ist dieselbe,
        // nur die Begruendung heisst ehrlicherweise „effizient".
        let s = sub_fuel_v0_7_1(Some(1000.0), Some(950.5), None, None); // -4,95 % → -5,0 %
        assert_eq!(
            (s.score, s.rationale_key.as_deref(), s.value.as_deref()),
            (100, Some("landing.rat.efficient"), Some("-5.0%"))
        );
        // eine Zehntelstelle darunter bleibt „auf Plan"
        let s = sub_fuel_v0_7_1(Some(1000.0), Some(951.0), None, None); // -4,9 %
        assert_eq!(
            (s.score, s.rationale_key.as_deref()),
            (100, Some("landing.rat.on_plan"))
        );
        // exakt auf der alten Grenze — hier stand vorher die 95
        let s = sub_fuel_v0_7_1(Some(1000.0), Some(950.0), None, None); // -5,0 %
        assert_eq!(
            (s.score, s.rationale_key.as_deref()),
            (100, Some("landing.rat.efficient"))
        );
        // kurz vor der Warnschwelle — weiterhin volle Punktzahl
        // (v1.7.32: -14,9 % statt -14,95 %, weil einmal gerundet wird —
        // -14,95 % zeigt die Karte als „-15.0%" und meint dann auch das.)
        let s = sub_fuel_v0_7_1(Some(1000.0), Some(851.0), None, None); // -14,9 %
        assert_eq!(
            (s.score, s.rationale_key.as_deref()),
            (100, Some("landing.rat.efficient"))
        );
        assert!(s.warning.is_none());
        // ⚠ v1.7.12: Ab hier zweifelt die Bewertung am Plan, nicht am
        // Piloten — und sagt das jetzt als HINWEIS statt als Abzug.
        // Vorher 85 Punkte; Sparen darf nichts kosten (Thomas,
        // 30.08.2026). Siehe `sparen_kostet_nie_punkte`.
        let s = sub_fuel_v0_7_1(Some(1000.0), Some(850.0), None, None); // -15,0 %
        assert_eq!(
            (s.score, s.rationale_key.as_deref()),
            (100, Some("landing.rat.very_efficient"))
        );
        assert_eq!(s.warning.as_deref(), Some("planned_burn_may_be_off"));
    }

    /// Mehrverbrauch bleibt unangetastet — die Aenderung ist einseitig.
    #[test]
    fn ofp_treue_baender_v1_7_32() {
        // ⚠ Die Baender wurden am 30.08.2026 geweitet. Anlass: Gemessen
        // ueber 412 Fluege lagen **45 % aller Grossraum-Landungen** und
        // 21 % der schmaleren ueber der alten +2-%-Grenze und verloren
        // Punkte. Eine Schwelle, die die Mehrheit trifft, misst kein
        // Koennen mehr, sondern Rauschen — in der Luftfahrt gilt eine
        // OFP-Genauigkeit von ±5 % als gut.
        //
        // TOW hier None, damit NUR die Prozentbaender geprueft werden;
        // der Toleranzboden hat eigene Tests.
        let f =
            |geplant: f32, echt: f32| sub_fuel_v0_7_1(Some(geplant), Some(echt), None, None).score;
        //
        // ⚠ v1.7.32: Baender wieder gestrafft auf 3/8/15/25. Das gehoert
        // zur Gutschrift fuer Mehrweg und Durchstarten: Wer die
        // Bezugsgroesse entschaerft und die Grenzen laesst, entkernt die
        // Achse (Codex, 16.09.2026: „2,5 % Abzuege und Ø 99,0 Punkte sind
        // kaum noch eine Bewertung"). Am Bestand gemessen bleiben mit
        // Gutschrift + neuen Baendern 12,3 % Abzuege statt 15,4 % — sie
        // treffen aber die Richtigen. Die Werte hier sind OHNE Gutschrift
        // (keine Streckenangabe), pruefen also die reinen Baender.
        // OHNE Streckenangabe gelten weiter die alten, milderen Baender
        // (5/10/20/35). Externe QS (Codex, 16.09.2026): Die gestrafften
        // Grenzen sind das Gegengewicht zur Gutschrift — wer sie ohne
        // Gutschrift bekommt (alte Resume-Datei, fehlende Plandistanz),
        // wuerde doppelt bestraft.
        assert_eq!(f(1000.0, 1049.0), 100); // +4,9 %
        assert_eq!(f(1000.0, 1053.0), 80); // +5,3 %
        assert_eq!(f(1000.0, 1150.0), 55); // +15,0 %
        assert_eq!(f(1000.0, 1250.0), 25); // +25,0 %
        assert_eq!(f(1000.0, 1400.0), 5); // +40,0 %

        // MIT Streckenangabe (und damit moeglicher Gutschrift) gelten die
        // gestrafften Baender 3/8/15/25. Hier ohne Mehrweg, damit nur die
        // Grenzen geprueft werden.
        let g = |geplant: f32, echt: f32| {
            sub_fuel_v1_7_32(
                Some(geplant),
                Some(echt),
                None,
                None,
                mit(500.0, 500.0, 0),
            )
            .score
        };
        assert_eq!(g(1000.0, 1029.0), 100); // +2,9 %
        assert_eq!(g(1000.0, 1049.0), 80); // +4,9 %
        assert_eq!(g(1000.0, 1120.0), 55); // +12,0 %
        assert_eq!(g(1000.0, 1200.0), 25); // +20,0 %
        assert_eq!(g(1000.0, 1400.0), 5); // +40,0 %
    }

    #[test]
    fn sparen_kostet_nie_punkte() {
        // ⚠ Der Kopf dieser Datei sagt seit jeher „Minderverbrauch wird
        // nie bestraft" — und die Tabelle tat es trotzdem, zuletzt mit
        // 85 Punkten ab 15 % unter Plan. Zum ZWEITEN Mal derselbe
        // Widerspruch (v1.6.7 hat ihn zwischen 5 % und 15 % beseitigt).
        let f = |geplant: f32, echt: f32| sub_fuel_v0_7_1(Some(geplant), Some(echt), None, None);
        for unter in [1.0_f32, 5.0, 15.0, 30.0, 60.0] {
            let e = f(1000.0, 1000.0 * (1.0 - unter / 100.0));
            assert_eq!(e.score, 100, "-{unter} % kostete Punkte");
        }
        // Der HINWEIS bleibt — als Information, nicht als Abzug.
        assert!(f(1000.0, 300.0).warning.is_some());
    }

    #[test]
    fn der_toleranzboden_waechst_mit_dem_flugzeug() {
        // ⚠ Der gemeldete Fall: 208 t Abfluggewicht, 4,78 t geplanter
        // Verbrauch (knapp eine Stunde fuer ein Grossraumflugzeug),
        // 400 kg mehr = +8,4 %. Das kostete 55 statt 100 Punkte, obwohl
        // 400 kg dort ein Rollweg mehr sind.
        assert_eq!(
            sub_fuel_v0_7_1(Some(4780.0), Some(5180.0), Some(208_400.0), None).score,
            100,
            "400 kg auf einem 208-t-Flugzeug sind kein Mehrverbrauch"
        );
        // Ohne bekanntes Abfluggewicht bleibt es beim Prozentsatz —
        // und ohne Streckenangabe bei den alten Baendern: +8,4 % → 80.
        assert_eq!(
            sub_fuel_v0_7_1(Some(4780.0), Some(5180.0), None, None).score,
            80
        );
        // ⚠ Und er darf NICHT fest sein: Fuer eine C172 (1,1 t) waeren
        // 400 kg mehr als ihr ganzer Reiseverbrauch.
        assert_eq!(
            sub_fuel_v0_7_1(Some(60.0), Some(120.0), Some(1_100.0), None).score,
            5,
            "eine Verdopplung bei einer C172 muss sichtbar bleiben"
        );
    }

    #[test]
    fn nach_einem_divert_wird_nicht_bewertet() {
        // Es wurde eine ANDERE Strecke geflogen. Ein Vergleich gegen den
        // urspruenglichen Plan misst den Umweg, nicht den Piloten.
        let e = sub_fuel_v0_7_1(Some(5000.0), Some(2000.0), Some(70_000.0), Some(true));
        assert!(e.skipped, "nach einem Divert darf nicht bewertet werden");
        assert_eq!(e.reason.as_deref(), Some("diverted"));
    }

    #[test]
    fn v0_7_1_strong_underburn_warns() {
        // v1.7.12: -25 % gibt volle Punktzahl PLUS Hinweis. Der Hinweis
        // bleibt wichtig — ein so starker Minderverbrauch ist bei einem
        // planmaessigen Flug kaum moeglich, da stimmt eher der Plan
        // nicht (oder es war ein Divert, der eigene Zweig davor).
        let s = sub_fuel_v0_7_1(Some(5000.0), Some(3750.0), None, None);
        assert_eq!(s.score, 100);
        assert_eq!(
            s.rationale_key.as_deref(),
            Some("landing.rat.very_efficient")
        );
        assert_eq!(s.warning.as_deref(), Some("planned_burn_may_be_off"));
    }

    #[test]
    fn v0_7_1_on_plan() {
        // Exact match → 100 (on_plan)
        let s = sub_fuel_v0_7_1(Some(5000.0), Some(5000.0), None, None);
        assert_eq!(s.score, 100);
        assert_eq!(s.rationale_key.as_deref(), Some("landing.rat.on_plan"));
    }

    // ─── v1.7.32: Mehrweg und Durchstarten gehen nicht auf die Note ───

    fn mit(geflogen: f32, luftlinie: f32, durchstarts: u32) -> ZusatzArbeit {
        ZusatzArbeit {
            geflogene_strecke_nm: Some(geflogen),
            plan_strecke_nm: Some(luftlinie),
            durchstarts,
        }
    }

    /// Der Anlassfall: THY 1068 (LRCK→LTFM, 16.09.2026). Plan 1929 kg,
    /// geflogen 2189 kg = +13,5 % → vorher 55 Punkte. Geflogen wurden
    /// 277 NM bei 186,5 NM Plandistanz.
    #[test]
    fn thy1068_vektoren_kosten_keine_punkte_mehr() {
        let e = sub_fuel_v1_7_32(
            Some(1929.0),
            Some(2189.1),
            Some(59122.0),
            None,
            mit(277.3, 186.5, 0),
        );
        assert_eq!(e.score, 100, "90 NM Mehrweg sind nicht dem Piloten anzulasten");
        let text = e.value.clone().unwrap();
        // Die Anzeige erklärt sich selbst — Wert, Rohwert, Grund.
        assert!(text.contains("roh +13.5%"), "{text}");
        assert!(text.contains("NM Mehrweg"), "{text}");
    }

    /// Ohne Mehrweg bleibt derselbe Mehrverbrauch ein Abzug — sonst
    /// misst die Achse nichts mehr.
    #[test]
    fn mehrverbrauch_ohne_grund_kostet_weiter_punkte() {
        let e = sub_fuel_v1_7_32(
            Some(1929.0),
            Some(2189.1),
            Some(59122.0),
            None,
            mit(190.0, 186.5, 0),
        );
        assert_eq!(e.score, 55, "+13,5 % ohne Umweg bleiben ein Abzug");
    }

    /// Der Grundkorridor: die geplante Route ist immer länger als die
    /// Luftlinie. Bis 10 % gibt es KEINE Gutschrift.
    #[test]
    fn grundkorridor_wird_nicht_gutgeschrieben() {
        let knapp = sub_fuel_v1_7_32(
            Some(1000.0),
            Some(1120.0),
            None,
            None,
            mit(109.0, 100.0, 0),
        );
        assert_eq!(knapp.score, 55, "9 % Mehrweg gelten als eingeplant");
        let drueber = sub_fuel_v1_7_32(
            Some(1000.0),
            Some(1120.0),
            None,
            None,
            mit(135.0, 100.0, 0),
        );
        assert_eq!(drueber.score, 100, "25 % Mehrweg werden gutgeschrieben");
    }

    /// Durchstarten ist gutes Handwerk und darf nichts kosten. Die
    /// Gutschrift wächst mit dem Flugzeug, nicht mit dem Planverbrauch.
    #[test]
    fn durchstarten_wird_in_kilogramm_gutgeschrieben() {
        let a320 = sub_fuel_v1_7_32(
            Some(3000.0),
            Some(3240.0),
            Some(73_000.0),
            None,
            mit(500.0, 500.0, 1),
        );
        assert_eq!(a320.score, 100, "+8 % auf einem A320 = ein Durchstarten");
        assert!(a320.value.clone().unwrap().contains("Durchstarten"));
        // Zwei Durchstarter auf einem Großraumjet: entsprechend mehr.
        let heavy = sub_fuel_v1_7_32(
            Some(20_000.0),
            Some(21_500.0),
            Some(250_000.0),
            None,
            mit(2000.0, 2000.0, 2),
        );
        assert_eq!(heavy.score, 100);
    }

    /// Ab 50 % Mehrweg lässt sich ein ATC-Vektor nicht mehr von einem
    /// selbst gewählten Umweg unterscheiden → keine erfundene Note.
    #[test]
    fn extremer_umweg_ist_nicht_bewertbar() {
        let e = sub_fuel_v1_7_32(Some(1000.0), Some(1500.0), None, None, mit(200.0, 100.0, 0));
        assert_eq!(e.band, "skipped");
        assert_eq!(e.reason.as_deref(), Some("umweg_zu_gross"));
    }

    /// Rundflug/Platzrunde: die Luftlinie taugt nicht als Bezug (im
    /// Bestand ein Flug mit rechnerisch 11696 % Mehrweg).
    #[test]
    fn rundflug_hat_keinen_streckenbezug() {
        let e = sub_fuel_v1_7_32(Some(200.0), Some(260.0), None, None, mit(80.0, 2.0, 0));
        assert_eq!(e.band, "skipped");
        assert_eq!(e.reason.as_deref(), Some("kein_streckenbezug"));
    }

    /// Ohne Streckenangabe (Alt-Client, Adopt/Resume) rechnet die Achse
    /// wie zuvor — nur eben mit den gestrafften Bändern.
    #[test]
    fn ohne_streckenangabe_keine_gutschrift() {
        let e = sub_fuel_v1_7_32(Some(1000.0), Some(1120.0), None, None, ZusatzArbeit::default());
        assert_eq!(e.score, 55);
        assert_eq!(e.value.as_deref(), Some("+12.0%"));
    }

    /// Die Gutschrift gleicht nur aus, sie macht niemanden sparsam.
    /// Externe QS (Codex, 16.09.2026): Bei THY 1068 standen 597 kg
    /// Gutschrift gegen 260 kg Mehrverbrauch — daraus wurde „sehr
    /// sparsam" samt Hinweis „der Plan könnte falsch sein".
    #[test]
    fn gutschrift_macht_niemanden_sparsam() {
        let e = sub_fuel_v1_7_32(
            Some(1929.0),
            Some(2189.1),
            Some(59122.0),
            None,
            mit(277.3, 186.5, 0),
        );
        assert_eq!(e.score, 100);
        assert_eq!(e.rationale_key.as_deref(), Some("landing.rat.on_plan"));
        assert!(e.warning.is_none(), "kein falscher Plan-Zweifel");
        assert_eq!(e.value.as_deref().map(|v| v.starts_with("0.0%")), Some(true));
    }

    /// Wer WIRKLICH sparsam war, bleibt sparsam — der Deckel greift nur
    /// gegen die Gutschrift, nicht gegen den Piloten.
    #[test]
    fn echtes_sparen_bleibt_sichtbar() {
        let e = sub_fuel_v1_7_32(
            Some(1000.0),
            Some(880.0),
            None,
            None,
            mit(150.0, 100.0, 0),
        );
        assert_eq!(e.score, 100);
        assert_eq!(e.rationale_key.as_deref(), Some("landing.rat.efficient"));
        assert_eq!(e.value.as_deref(), Some("-12.0%"));
    }

    /// Der Hinweis auf die Begrenzung — genau der Mechanismus, der zuletzt
    /// nachgebessert wurde (unabhaengige QS, 16.09.2026: bis dahin
    /// unassertiert).
    #[test]
    fn begrenzte_gutschrift_sagt_es_auch() {
        // 40 NM Mehrweg = 320 kg moegliche Gutschrift, aber nur 100 kg
        // Mehrverbrauch → angerechnet werden 100, und das steht da.
        let e = sub_fuel_v1_7_32(
            Some(1000.0),
            Some(1100.0),
            None,
            None,
            mit(150.0, 100.0, 0),
        );
        let text = e.value.clone().unwrap();
        assert!(text.contains("auf 100 kg Mehrverbrauch begrenzt"), "{text}");

        // Volle Anrechnung: kein Begrenzungshinweis.
        let voll = sub_fuel_v1_7_32(
            Some(1000.0),
            Some(1400.0),
            None,
            None,
            mit(150.0, 100.0, 0),
        );
        let text = voll.value.clone().unwrap();
        assert!(text.contains("NM Mehrweg"), "{text}");
        assert!(!text.contains("begrenzt"), "{text}");
    }

    /// Die beiden Grenzwerte selbst — nicht nur deutlich darueber oder
    /// darunter (unabhaengige QS, 16.09.2026).
    #[test]
    fn grenzwerte_genau_getroffen() {
        // Genau 25 NM Plandistanz: noch bewertbar.
        let auf_der_grenze = sub_fuel_v1_7_32(
            Some(100.0),
            Some(105.0),
            None,
            None,
            mit(25.0, MIN_PLAN_STRECKE_NM, 0),
        );
        assert_ne!(auf_der_grenze.band, "skipped");
        // Eine Zehntelmeile darunter: kein Streckenbezug.
        let darunter = sub_fuel_v1_7_32(
            Some(100.0),
            Some(105.0),
            None,
            None,
            mit(25.0, MIN_PLAN_STRECKE_NM - 0.1, 0),
        );
        assert_eq!(darunter.reason.as_deref(), Some("kein_streckenbezug"));

        // Genau 50 % Mehrweg: noch bewertbar (der Deckel greift erst
        // DARUEBER).
        let genau_deckel = sub_fuel_v1_7_32(
            Some(1000.0),
            Some(1200.0),
            None,
            None,
            mit(100.0 * (1.0 + UMWEG_DECKEL), 100.0, 0),
        );
        assert_ne!(genau_deckel.band, "skipped");
        let drueber = sub_fuel_v1_7_32(
            Some(1000.0),
            Some(1200.0),
            None,
            None,
            mit(100.0 * (1.0 + UMWEG_DECKEL) + 0.5, 100.0, 0),
        );
        assert_eq!(drueber.reason.as_deref(), Some("umweg_zu_gross"));
    }
}
