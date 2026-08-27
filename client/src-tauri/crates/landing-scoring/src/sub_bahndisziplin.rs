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
///
/// # Warum 2,1 m und nicht mehr 1,5
///
/// Seit der QS am 23.08.2026 misst die Rechnung bis zur **Reifen-Aussenkante**
/// statt bis zur Bein-Mitte (`aussenkante_halb_aus_spur`). Beide Werte der
/// Tabelle oben ruecken damit um eine halbe Radpaketbreite nach aussen — bei
/// MPH 9 um 0,55 m auf +1,74 m und +0,64 m.
///
/// Mit der alten Toleranz von 1,5 m faellt der Fall wieder auseinander, für
/// den sie gebaut wurde: Navigraph 20 Punkte, OpenStreetMap 55. Die
/// **Differenz** zwischen den Quellen ist unveraendert 1,10 m; nur die
/// Schwelle lag danach mitten in ihr.
///
/// Die Toleranz deckt jetzt beides:
///
/// * die gemessene Differenz der Bahnquellen (1,10 m bei MPH 9), und
/// * den Naeherungsfehler des Radpaket-Zuschlags (bis 0,55 m, weil er nach
///   Baugroesse geschaetzt wird und keine Herstellerangabe je Muster ist).
///
/// Am Korpus nachgerechnet: siehe den Test `korpus_kein_regress`.
const KANTEN_TOLERANZ_M: f64 = 2.1;

/// Ab wie weit jenseits der Kante die Messung als **fragwürdig** gilt.
///
/// Ein Rad kann neben der Bahn laufen — aber nicht 30 m daneben, ohne dass das
/// Flugzeug längst im Gelände stünde. Wer dort landet, hat kein
/// Bahndisziplin-Problem, sondern die Messung hat eins: falsch zugeordnete
/// Parallelbahn, unsauberer Bahn-Match, kaputte Geometrie.
///
/// **Gemessen am Korpus (802 Landungen, 23.08.2026):** Die Extremfälle waren
/// 513 m Versatz auf EDDH 15, 56,9 m auf LGKO 32 und 52,6 m auf EDDL 23L — eine
/// Bahn mit Parallelbahn. Ohne diese Schranke bekämen genau diese Piloten
/// 20 Punkte für einen Fehler, den nicht sie gemacht haben.
///
/// Jenseits der Schranke wird **übersprungen**, nicht bewertet — nach demselben
/// Grundsatz wie überall: Datenmangel darf nie zur härteren Note führen.
const MESSUNG_FRAGWUERDIG_AB_M: f64 = 30.0;

/// Ab welchem Winkel zur Bahnachse die ACHSE nicht stimmen kann.
///
/// # Der Fall, der das erzwungen hat
///
/// FACT 19 (Kapstadt), 24.08.2026, A340-600 in X-Plane: Der Bericht sagte
/// „Aufsetzen 24,6 m links" auf einer 61 m breiten Bahn und „grösster
/// Versatz 35,3 m links" — ein Rad weit im Gras. Das Bildschirmfoto des
/// Piloten zeigt die Maschine mittig auf der Bahn.
///
/// Nachgerechnet stimmte die Zahl: Gegen die Navdaten-Achse WAR die
/// Maschine 24,6 m links. Nur läuft die Rollspur auf dem geraden Teil
/// **1,95° zu dieser Achse** — und ein rollendes Flugzeug folgt der
/// aufgemalten Mittellinie. Also ist die Achse falsch, nicht die Spur:
/// Die X-Plane-Szenerie von FACT ist gegenüber dem AIRAC-Stand verdreht.
/// Auf 3201 m Bahnlänge macht das 109 m Querfehler.
///
/// **Gemessen an 12 Landungen desselben Tages** (Winkel der
/// Ausgleichsgeraden über den Teil vor dem Ausschwenken):
///
/// ```text
/// Median 0,29°  ·  alle ausser FACT unter 0,66°  ·  FACT 1,55°
/// ```
///
/// Ein Grad lässt beiden Seiten Luft: gut das Dreifache des Normalfalls,
/// deutlich unter dem Störfall.
///
/// Jenseits davon wird **übersprungen**, nicht bewertet — nach demselben
/// Grundsatz wie überall: Datenmangel darf nie zur härteren Note führen,
/// und ein Szenerie-Versatz ist kein Pilotenfehler.
const ACHSE_FRAGWUERDIG_AB_GRAD: f64 = 1.0;

/// Der Winkel der Rollspur zur Bahnachse, in Grad.
///
/// Ausgleichsgerade über die Punkte bis `bis_laengs_m` — also über den
/// Teil, auf dem das Flugzeug noch der Bahn folgt. Danach beginnt das
/// Ausschwenken, und das ist kein Achsenfehler, sondern eine Ausfahrt.
///
/// `None`, wenn zu wenige Punkte da sind oder alle auf derselben
/// Längsposition liegen (dann hat die Gerade keine Steigung).
pub fn achsen_abweichung_grad(
    proben: &[(f64, f64)],
    bis_laengs_m: f64,
) -> Option<f64> {
    achsen_befund(proben, bis_laengs_m).map(|b| b.winkel_grad)
}

/// Was die Rollspur im gewerteten Fenster über die Achse aussagt.
///
/// Der blosse Winkel reicht nicht, um „unsere Achse ist falsch" von „das
/// Flugzeug fuhr schräg" zu unterscheiden — siehe `achse_fragwuerdig`.
#[derive(Debug, Clone, Copy)]
pub struct AchsenBefund {
    /// Winkel der Ausgleichsgeraden zur Bahnachse, in Grad.
    pub winkel_grad: f64,
    /// Wechselt die Querlage im Fenster das Vorzeichen?
    pub kreuzt_mitte: bool,
    /// Grösster Betrag der Querlage im Fenster, in Metern.
    pub groesster_betrag_m: f64,
}

pub fn achsen_befund(proben: &[(f64, f64)], bis_laengs_m: f64) -> Option<AchsenBefund> {
    let auf: Vec<(f64, f64)> = proben
        .iter()
        .copied()
        .filter(|(lg, qr)| *lg <= bis_laengs_m && lg.is_finite() && qr.is_finite())
        .collect();
    // Unter zehn Punkten ist eine Gerade Zufall.
    if auf.len() < 10 {
        return None;
    }
    let n = auf.len() as f64;
    let sx: f64 = auf.iter().map(|(x, _)| *x).sum();
    let sy: f64 = auf.iter().map(|(_, y)| *y).sum();
    let sxx: f64 = auf.iter().map(|(x, _)| x * x).sum();
    let sxy: f64 = auf.iter().map(|(x, y)| x * y).sum();
    let nenner = n * sxx - sx * sx;
    if nenner.abs() < 1e-9 {
        return None;
    }
    let steigung = (n * sxy - sx * sy) / nenner;
    if !steigung.is_finite() {
        return None;
    }
    let kleinste = auf.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
    let groesste = auf.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);
    Some(AchsenBefund {
        winkel_grad: steigung.atan().to_degrees(),
        kreuzt_mitte: kleinste < 0.0 && groesste > 0.0,
        groesster_betrag_m: kleinste.abs().max(groesste.abs()),
    })
}

/// Wie weit von der Mitte darf eine kreuzende Spur wandern und trotzdem
/// als Manöver gelten — als Anteil der BAHNBREITE.
///
/// 0,25 heisst: Die Spur blieb in der mittleren Hälfte der Bahn. Das ist
/// keine angepasste Zahl, sondern eine geometrische Aussage — wer die
/// Mittellinie kreuzt und dabei die mittlere Hälfte nie verlässt, kann
/// nicht Opfer einer verdrehten Achse sein: Eine Drehung, die über den
/// Rollweg mehr als ein Grad ausmacht, traegt das Flugzeug weit aus der
/// Mitte heraus und wechselt dabei nicht das Vorzeichen.
///
/// Am Korpus (46 Landungen mit Rollspur, 37 Plätze, 27.08.2026):
///
/// ```text
/// EDDM  9,2 m / 60 m = 0,15   kreuzt   -> Manöver, wird bewertet
/// SLVR 12,9 m / 45 m = 0,29   kreuzt   -> bleibt uebersprungen
/// FACT 35,3 m / 61 m = 0,58   kreuzt   -> bleibt uebersprungen
/// EDHE 45,7 m / 45 m = 1,02   einseitig -> bleibt uebersprungen
/// ```
const MANOEVER_ANTEIL_BREITE: f64 = 0.25;

/// Bis wohin die Ausgleichsgerade fuer den Achsenwinkel gelegt wird.
///
/// # Warum das eine eigene Funktion ist
///
/// Weil die Reihenfolge der eigentliche Fehler war und niemand sie sah.
/// Am Aufrufer stand `kante.or(raeum)` — die KANTE zuerst, also die
/// Stelle, an der das Flugzeug die Bahn VERLASSEN hat. Die Gerade lief
/// damit ueber das ganze Ausschwenken zur Ausfahrt und stand schraeg;
/// gemeldet wurde „Szenerie-Versatz", an Plaetzen mit Standardszenerie
/// wie EDDK und EGBB nachweislich falsch.
///
/// Als Ausdruck am Aufrufer konnte kein Test das festhalten. Als
/// Funktion schon — siehe `fenster_tests`.
///
/// # Die Rangfolge
///
/// 1. **Messfensterende** — der Teil, auf dem das Flugzeug noch der Bahn
///    folgt. Genau das, was diese Rechnung braucht.
/// 2. **Ausschwenken** (`raeum`) — ab hier haengt die seitliche Lage an
///    der Anweisung des Lotsen, aber die Bahn ist noch nicht verlassen.
/// 3. **Kante** — nur als Notnagel. Besser eine Gerade ueber zu viel als
///    gar keine Aussage.
pub fn achsen_fenster_bis_m(
    mess_ende_laengs_m: Option<f64>,
    raeum_laengs_m: Option<f64>,
    kante_laengs_m: Option<f64>,
) -> Option<f64> {
    mess_ende_laengs_m
        .or(raeum_laengs_m)
        .or(kante_laengs_m)
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// Ist die Achse fragwürdig — oder fuhr das Flugzeug einfach schräg?
///
/// # Warum das getrennt werden muss
///
/// Die Meldung an den Piloten lautet „Szenerie-Versatz". Das ist eine
/// Aussage über die DATEN, und sie war am 27.08.2026 bei EDDK, EDDM und
/// EGBB nachweislich falsch — Standard-Szenerie an bestens kartierten
/// Plätzen. Dort fuhr das Flugzeug glatt über die Mittellinie:
///
/// ```text
/// EDDM 08R:  1544 m: -5,5 m  ->  1695 m: 0,0 m  ->  1793 m: +9,2 m
/// ```
///
/// Ein verdrehter Bahnbezug sieht anders aus: Der Versatz waechst
/// einseitig und wird gross (FACT 24,6 -> 35,3 m). Deshalb zwei
/// Bedingungen statt einer.
pub fn achse_fragwuerdig(befund: AchsenBefund, bahnbreite_m: Option<f64>) -> bool {
    if befund.winkel_grad.abs() <= ACHSE_FRAGWUERDIG_AB_GRAD {
        return false;
    }
    if let Some(breite) = bahnbreite_m.filter(|b| *b > 0.0) {
        if befund.kreuzt_mitte && befund.groesster_betrag_m <= MANOEVER_ANTEIL_BREITE * breite {
            return false;
        }
    }
    true
}

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
    /// Winkel der Rollspur zur Bahnachse, in Grad — siehe
    /// `ACHSE_FRAGWUERDIG_AB_GRAD`. `None` = nicht bestimmbar.
    pub achsen_abweichung_grad: Option<f64>,
    /// Wechselt die Querlage im gewerteten Fenster das Vorzeichen?
    ///
    /// Zusammen mit `achsen_groesster_betrag_m` trennt das ein Manöver von
    /// einem echten Achsenfehler — siehe `achse_fragwuerdig`.
    pub achsen_kreuzt_mitte: Option<bool>,
    /// Grösster Betrag der Querlage im gewerteten Fenster, in Metern.
    pub achsen_groesster_betrag_m: Option<f64>,
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

    // Stimmt die ACHSE? Ein rollendes Flugzeug folgt der Mittellinie —
    // läuft die Spur schräg dazu, ist unsere Achse falsch, nicht die Spur.
    // Siehe `ACHSE_FRAGWUERDIG_AB_GRAD` (FACT 19, 24.08.2026).
    if let Some(winkel) = input.achsen_abweichung_grad {
        let befund = AchsenBefund {
            winkel_grad: winkel,
            // Fehlen die beiden Begleitwerte (alte Datensaetze), bleibt es
            // beim reinen Winkel — dann ist `kreuzt_mitte` falsch und die
            // Ausnahme greift nicht. Lieber uebersprungen als zu Unrecht
            // benotet.
            kreuzt_mitte: input.achsen_kreuzt_mitte.unwrap_or(false),
            groesster_betrag_m: input.achsen_groesster_betrag_m.unwrap_or(f64::INFINITY),
        };
        if achse_fragwuerdig(befund, Some(breite)) {
            return SubScoreEntry::skipped(KEY, LABEL, "runway_axis_mismatch");
        }
    }

    // Plausibilität vor Bewertung: siehe MESSUNG_FRAGWUERDIG_AB_M.
    if versatz.abs() > halbe + MESSUNG_FRAGWUERDIG_AB_M {
        return SubScoreEntry::skipped(KEY, LABEL, "implausible_lateral_track");
    }

    // Die Aussenkante des aeusseren REIFENS, nicht die Bein-Mitte.
    //
    // Die Spurweite in den Herstellerangaben misst von Bein-Mitte zu
    // Bein-Mitte; der aeussere Rand des aeussersten Rades liegt noch eine
    // halbe Radpaketbreite weiter draussen. Fuer die Frage „lief ein Rad
    // neben der befestigten Flaeche" zaehlt genau dieser Rand.
    //
    // `aussenkante_halb_aus_spur` war gebaut und wurde bis zur QS am
    // 23.08.2026 **nirgends aufgerufen** — Bewertung und Anzeige rechneten
    // beide mit `spur / 2.0`.
    //
    // Am Korpus gemessen (781 Landungen mit Muster und Bahnbreite):
    // fuenf wechseln das Band, alle in die strengere Richtung, alle
    // Grenzfaelle — etwa 0,13 m Randabstand statt −0,32 m bei einer 737
    // mit 17,0 m Versatz auf einer 40-m-Bahn. Genau dort kommt es darauf
    // an, ob das Rad noch auf dem Asphalt stand.
    let aussenkante_m = versatz.abs() + crate::spurweite::aussenkante_halb_aus_spur(spur);
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
            runway_geometry_trusted: Some(true),            achsen_kreuzt_mitte: None,
            achsen_groesster_betrag_m: None,
            achsen_abweichung_grad: None,
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
        // halbe Breite 22,55 m; aeusseres RAD = Versatz + 5,90 m.
        //
        // Die 5,90 sind die halbe Spurweite (5,35) plus die halbe
        // Radpaketbreite (0,55): Gemessen wird bis zum aeusseren Rand des
        // aeussersten Reifens, nicht bis zur Bein-Mitte. Bis zur QS am
        // 23.08.2026 stand hier 5,35 — `aussenkante_halb_aus_spur` war
        // gebaut und wurde nirgends aufgerufen.
        //
        // Die Bandgrenzen ruecken dadurch um 0,55 m nach innen:
        // 75 % -> 16,91 m Rad -> Versatz 11,01 (war 11,56)
        // 90 % -> 20,30 m Rad -> Versatz 14,40 (war 14,95)
        //
        // Die Faelle liegen bewusst knapp beidseits jeder Grenze — ein
        // Test in der Bandmitte haette die Verschiebung nicht bemerkt.
        for (versatz, erwartet, grund) in [
            (0.0, 100u8, "centered"),
            (10.9, 100, "centered"),      // Anteil 0,745
            (11.2, 85, "outboard"),       // Anteil 0,758
            (14.2, 85, "outboard"),       // Anteil 0,891
            (14.6, 55, "edge_reached"),   // Anteil 0,909
            (18.4, 55, "edge_reached"),   // Rand -1,74 m: in der Toleranz
            (19.0, 20, "off_pavement"),   // Rand -2,35 m: darueber hinaus
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
    fn unmoegliche_messwerte_werden_nicht_bestraft() {
        // Aus dem Korpus: 513 m Versatz auf EDDH 15, 56,9 m auf LGKO 32,
        // 52,6 m auf EDDL 23L (Bahn mit Parallelbahn). Das ist kein Rad
        // neben der Bahn, das ist ein Bahn-Match-Fehler.
        for versatz in [52.6, 56.9, 513.0, -60.0] {
            let r = sub_bahndisziplin(&eham06(versatz));
            assert!(r.skipped, "{versatz} m ist keine Landung, sondern ein Messfehler");
            assert_eq!(r.reason.as_deref(), Some("implausible_lateral_track"));
            assert_eq!(r.points, 0, "Skip erzeugt keine Note");
        }
        // Direkt darunter wird weiter bewertet — die Schranke ist grosszuegig,
        // nicht willkuerlich.
        let grenzfall = sub_bahndisziplin(&eham06(50.0));
        assert!(!grenzfall.skipped, "50 m liegen noch innerhalb der Schranke");
    }

    #[test]
    fn datenmangel_wird_uebersprungen_nie_bestraft() {
        for (bau, grund) in [
            (
                BahndisziplinInput { airport_source: None, ..eham06(5.0)
        },
                "off_airport_landing",
            ),
            (
                BahndisziplinInput { runway_geometry_trusted: Some(false), ..eham06(5.0)
        },
                "untrusted_geometry",
            ),
            (
                BahndisziplinInput { bahnbreite_m: None, ..eham06(5.0)
        },
                "runway_width_unknown",
            ),
            (
                BahndisziplinInput { bahnbreite_m: Some(500.0), ..eham06(5.0)
        },
                "runway_width_unknown",
            ),
            (
                BahndisziplinInput { spurweite_m: None, ..eham06(5.0)
        },
                "track_width_unknown",
            ),
            (
                BahndisziplinInput { proben: Some(2), ..eham06(5.0)
        },
                "insufficient_samples",
            ),
            (
                BahndisziplinInput { max_querversatz_m: None, ..eham06(5.0)
        },
                "missing_lateral_track",
            ),
            (
                BahndisziplinInput { belag: Some(Belag::Unbekannt), ..eham06(5.0)
        },
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
        // 18,39 m Versatz + 5,90 m bis zur Reifen-Aussenkante = 24,29 m
        // gegen 22,55 m Kante, also 1,74 m drueber — innerhalb der
        // 2,1-m-Toleranz.
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

#[cfg(test)]
mod achse_tests {
    use super::*;

    /// Eine Spur bauen: Start-Querlage, Steigung in m je 100 m, n Punkte.
    fn spur(start_quer: f64, steigung_pro_100m: f64, n: usize) -> Vec<(f64, f64)> {
        (0..n)
            .map(|i| {
                let x = 1000.0 + i as f64 * 10.0;
                (x, start_quer + (x - 1000.0) * steigung_pro_100m / 100.0)
            })
            .collect()
    }

    #[test]
    fn winkel_wird_ueber_das_uebergebene_fenster_gerechnet() {
        // Gerade bis 1250 m, danach knickt sie scharf weg — genau das, was
        // die Ausfahrt tut. Das Fenster muss darueber entscheiden.
        let mut p = spur(0.0, 0.5, 26); // 0,5 m je 100 m ≈ 0,29°
        for i in 0..30 {
            let x = 1260.0 + i as f64 * 10.0;
            p.push((x, 1.25 + (x - 1250.0) * 0.30));
        }
        let eng = achsen_befund(&p, 1250.0).unwrap();
        let weit = achsen_befund(&p, 1560.0).unwrap();
        assert!(eng.winkel_grad.abs() < 0.5, "eng: {}", eng.winkel_grad);
        assert!(weit.winkel_grad.abs() > 5.0, "weit: {}", weit.winkel_grad);
    }

    #[test]
    fn kreuzende_spur_mit_kleinen_betraegen_ist_ein_manoever() {
        // EDDM 08R, 27.08.2026: -5,5 m -> +9,2 m ueber 250 m, Bahn 60 m.
        let p: Vec<(f64, f64)> = (0..26)
            .map(|i| (1544.0 + i as f64 * 10.0, -5.5 + i as f64 * 0.588))
            .collect();
        let b = achsen_befund(&p, 1794.0).unwrap();
        assert!(b.winkel_grad.abs() > ACHSE_FRAGWUERDIG_AB_GRAD);
        assert!(b.kreuzt_mitte);
        assert!(b.groesster_betrag_m < 15.0);
        assert!(
            !achse_fragwuerdig(b, Some(60.0)),
            "EDDM darf nicht als Szenerie-Versatz gelten"
        );
    }

    #[test]
    fn einseitig_und_gross_bleibt_ein_achsenfehler() {
        // FACT, der Fall, fuer den die Pruefung gebaut wurde: 24,6 -> 35,3 m
        // auf 61 m Breite, immer dieselbe Seite.
        // 1,95 Grad — der gemessene Winkel von FACT, nicht geschaetzt:
        // tan(1,95 Grad) * 20 m = 0.681 m je Schritt.
        let p: Vec<(f64, f64)> = (0..40)
            .map(|i| (500.0 + i as f64 * 20.0, 24.6 + i as f64 * 0.681))
            .collect();
        let b = achsen_befund(&p, 1300.0).unwrap();
        assert!(!b.kreuzt_mitte);
        assert!(
            achse_fragwuerdig(b, Some(61.0)),
            "FACT muss uebersprungen bleiben"
        );
    }

    #[test]
    fn kreuzend_aber_weit_draussen_bleibt_ein_achsenfehler() {
        // Die Ausnahme darf nicht dadurch aufgehen, dass die Spur irgendwo
        // einmal die Mitte streift. Nur die mittlere Haelfte zaehlt.
        let p: Vec<(f64, f64)> = (0..30)
            .map(|i| (1000.0 + i as f64 * 10.0, -20.0 + i as f64 * 1.6))
            .collect();
        let b = achsen_befund(&p, 1290.0).unwrap();
        assert!(b.kreuzt_mitte);
        assert!(b.groesster_betrag_m > 0.25 * 45.0);
        assert!(achse_fragwuerdig(b, Some(45.0)));
    }

    #[test]
    fn ohne_bahnbreite_bleibt_es_beim_winkel() {
        // Kein Mass, keine Ausnahme — lieber uebersprungen als zu Unrecht
        // benotet.
        let p: Vec<(f64, f64)> = (0..20)
            .map(|i| (1000.0 + i as f64 * 10.0, -3.0 + i as f64 * 0.4))
            .collect();
        let b = achsen_befund(&p, 1190.0).unwrap();
        assert!(b.winkel_grad.abs() > ACHSE_FRAGWUERDIG_AB_GRAD);
        assert!(achse_fragwuerdig(b, None));
    }

    #[test]
    fn gerade_spur_ist_nie_fragwuerdig() {
        let p = spur(2.0, 0.0, 20);
        let b = achsen_befund(&p, 1190.0).unwrap();
        assert!(!achse_fragwuerdig(b, Some(45.0)));
        assert!(!b.kreuzt_mitte);
    }

    #[test]
    fn alter_datensatz_ohne_begleitwerte_wird_uebersprungen() {
        // `achsen_kreuzt_mitte` und `achsen_groesster_betrag_m` fehlen —
        // dann darf die Manoever-Ausnahme NICHT greifen.
        let input = BahndisziplinInput {
            max_querversatz_m: Some(9.0),
            bahnbreite_m: Some(60.0),
            spurweite_m: Some(7.6),
            belag: Some(Belag::Befestigt),
            airport_source: Some("runway_match"),
            runway_geometry_trusted: Some(true),
            proben: Some(40),
            achsen_abweichung_grad: Some(3.4),
            achsen_kreuzt_mitte: None,
            achsen_groesster_betrag_m: None,
            ..Default::default()
        };
        let e = sub_bahndisziplin(&input);
        assert_eq!(e.reason.as_deref(), Some("runway_axis_mismatch"));
    }

    #[test]
    fn eddm_wird_mit_begleitwerten_bewertet_statt_uebersprungen() {
        let input = BahndisziplinInput {
            max_querversatz_m: Some(9.2),
            bahnbreite_m: Some(60.0),
            spurweite_m: Some(7.6),
            belag: Some(Belag::Befestigt),
            airport_source: Some("runway_match"),
            runway_geometry_trusted: Some(true),
            proben: Some(40),
            achsen_abweichung_grad: Some(3.39),
            achsen_kreuzt_mitte: Some(true),
            achsen_groesster_betrag_m: Some(9.2),
            ..Default::default()
        };
        let e = sub_bahndisziplin(&input);
        assert_ne!(e.reason.as_deref(), Some("runway_axis_mismatch"));
    }
}


#[cfg(test)]
mod fenster_tests {
    use super::*;

    #[test]
    fn das_messfenster_hat_vorrang_vor_allem() {
        // Der Fehler, der das alles ausgeloest hat: Stand die Kante vorn,
        // lief die Gerade ueber das Ausschwenken.
        assert_eq!(
            achsen_fenster_bis_m(Some(1796.0), Some(2084.0), Some(2494.0)),
            Some(1796.0)
        );
    }

    #[test]
    fn ohne_messfenster_gilt_das_ausschwenken_nicht_die_kante() {
        assert_eq!(
            achsen_fenster_bis_m(None, Some(2084.0), Some(2494.0)),
            Some(2084.0)
        );
    }

    #[test]
    fn die_kante_ist_nur_der_notnagel() {
        assert_eq!(achsen_fenster_bis_m(None, None, Some(2494.0)), Some(2494.0));
    }

    #[test]
    fn ohne_alles_gibt_es_keine_aussage() {
        assert_eq!(achsen_fenster_bis_m(None, None, None), None);
    }

    #[test]
    fn unsinnige_werte_zaehlen_nicht_als_fenster() {
        assert_eq!(achsen_fenster_bis_m(Some(0.0), Some(2084.0), None), None);
        assert_eq!(achsen_fenster_bis_m(Some(f64::NAN), None, None), None);
    }
}
