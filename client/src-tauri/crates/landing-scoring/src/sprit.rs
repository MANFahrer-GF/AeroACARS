//! v1.7.35 — Sprit-Auswertung **ohne Note**.
//!
//! # Warum es diese Datei gibt
//!
//! Die Achse „OFP-Treue" verglich den geplanten mit dem tatsaechlichen
//! Streckenverbrauch und zog bei Mehrverbrauch Punkte ab. Am Bestand
//! belegt (DLH370, 17.09.2026, und alle A380-Fluege nach EDDM): Der
//! Mehrverbrauch entsteht zum Grossteil im Anflug — Radarfuehrung,
//! Zwischenhoehen, Transitions — und den bestimmt die Flugsicherung,
//! nicht der Pilot. Der Pilot bewegt am Verbrauch 2–3 %, ATC bis ueber
//! 25 % (EUROCONTROL 2020). Kein reales Airline-Programm und keine
//! grosse VA-Plattform benotet den Einzelflug nach Verbrauch.
//!
//! Deshalb: **Der Sprit verlaesst die Note.** Diese Auswertung erklaert
//! dem Piloten, was passiert ist — in Zahlen, ohne Urteil:
//!
//! 1. Verbrauch bis zum Sinkflug gegen Plan (das, was der Pilot steuert)
//! 2. Verbrauch im Sinkflug und Anflug gegen Plan (das, was ATC steuert)
//! 3. Zeit unterhalb einer Schwelle ueber Platzhoehe (die Radarfuehrung)
//! 4. Die Sprit-Leiter des OFP mit der Landemarke: Contingency ist per
//!    Definition das Budget fuer Wind, Routing und ATC (ICAO Annex 6);
//!    Extra ist der Puffer des Piloten. Beides zu nutzen ist planmaessig.
//! 5. Final Reserve — die einzige Sprit-Groesse, die wirklich zaehlt.
//!    Unterschreitung ist ein Hinweis (gelb), nie eine Note, nie rot,
//!    nie eine Blockierung (Thomas, 17.09.2026).
//!
//! # Eine Stelle rechnet
//!
//! Der Client ruft [`auswerten`] einmal beim Abschluss der Landung auf und
//! schreibt das Ergebnis in den PIREP-Payload. Client-Oberflaeche,
//! PDF-Bericht und die Live-Webapp **zeigen** dieses Ergebnis nur an —
//! kein Fallback, kein Nachrechnen. Fehlt ein Wert, steht dort „—".

use serde::{Deserialize, Serialize};

/// Alles, was die Auswertung braucht. Jeder Wert ist optional: Manual-
/// Fluege haben kein OFP, Altbestand-Resumes kennen die Navlog-Felder
/// nicht, ein Neustart mitten im Flug kann den Vergleichspunkt verpasst
/// haben. Fehlende Eingaben fuehren zu fehlenden Ausgaben — nie zu 0.
#[derive(Debug, Clone, Default)]
pub struct SpritEingang {
    /// Geplanter Trip-Verbrauch (OFP `enroute_burn`).
    pub planned_burn_kg: Option<f32>,
    /// Geplanter Verbrauch bis zum Vergleichspunkt (Navlog `fuel_totalused`
    /// am TOD-Fix).
    pub plan_bis_vergleichspunkt_kg: Option<f32>,
    /// Geplante Reststrecke ab dem Vergleichspunkt bis zum Ziel (NM).
    pub plan_rest_nm: Option<f32>,
    /// Tankstand beim Abheben.
    pub takeoff_fuel_kg: Option<f32>,
    /// Tankstand am Vergleichspunkt — dort, wo noch `plan_rest_nm` vor dem
    /// Flugzeug lag. Vom Client im Flug festgehalten.
    pub sprit_bei_vergleichspunkt_kg: Option<f32>,
    /// Tankstand beim Aufsetzen.
    pub landing_fuel_kg: Option<f32>,
    /// Geflogene Strecke ab dem Vergleichspunkt bis zum Aufsetzen (NM).
    pub strecke_ab_vergleichspunkt_nm: Option<f32>,
    /// Zeit unterhalb der Schwelle (s), Pausen und Neustart-Luecken bereits
    /// abgezogen.
    pub zeit_unter_schwelle_s: Option<f32>,
    /// Die Schwelle selbst (ft MSL), zur Anzeige.
    pub schwelle_ft: Option<f32>,
    /// OFP-Sprit-Leiter.
    pub planned_taxi_kg: Option<f32>,
    pub planned_contingency_kg: Option<f32>,
    pub planned_alternate_kg: Option<f32>,
    pub planned_reserve_kg: Option<f32>,
    pub planned_extra_kg: Option<f32>,
    pub planned_block_fuel_kg: Option<f32>,
    /// Ausweichflug: eingereichtes Ziel != geplantes Ziel. Dann wurde der Plan
    /// nicht geflogen — Phasen und Plan-Reststrecke gegen ihn zu zeigen waere
    /// eine Aussage ueber einen Flug, den es nicht gab. Die alte Achse hatte
    /// dafuer eine ausdrueckliche Regel; die gilt hier weiter.
    pub ausweichflug: bool,
    /// `false`, wenn der Tankstand beim Aufsetzen nicht zum Verlauf passt
    /// (Auslesefehler, nur zwei von vier Tanks, Sprung). Dann gibt es
    /// „nicht pruefbar" statt einer falschen Warnung.
    pub tank_plausibel: bool,
    /// v1.7.36: Tankstand, als die Triebwerke anliefen, und als sie
    /// ausgingen. Beide an ein Ereignis gerastet (`sprit_boden_marken`).
    pub engine_start_fuel_kg: Option<f32>,
    pub engine_off_fuel_kg: Option<f32>,
}

/// Eine Flugphase: was gebraucht wurde, was geplant war, Abweichung in
/// Prozent (eine Nachkommastelle, gerundet — Anzeige und Wert sind
/// dieselbe Zahl).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    pub ist_kg: f32,
    pub plan_kg: f32,
    pub abweichung_pct: f32,
    /// Soll die Hauptzahl in Kilogramm stehen statt in Prozent?
    ///
    /// # Warum das hier entschieden wird
    ///
    /// Ueber 60 % sagt ein Prozentwert nichts mehr — der Anflug-Plan ist im
    /// Median nur 6,5 % des Trips, und dort ergeben schon 129 kg
    /// Mehrverbrauch +47 %. Die Regel selbst ist eine Zeile, aber sie stand
    /// bis v1.7.35 an VIER Stellen: im Client, in der Live-Uebersicht, im
    /// Bericht und in den PIREP-Feldern fuer die GSG-Webseite. Aendert
    /// jemand eine, driftet die vierte lautlos — und der Pilot liest im
    /// Client „−12 kg" und auf der Webseite „−8,3 %".
    ///
    /// Deshalb entscheidet es die Rechnung, und die Anzeigen folgen.
    pub als_kg: bool,
}

/// Stand der Final Reserve beim Aufsetzen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Reserve {
    /// Landesprit ueber der geplanten Final Reserve. `quote_pct` = 100 ·
    /// Landesprit / Reserve.
    Intakt { quote_pct: f32 },
    /// Landesprit unter der geplanten Final Reserve. Gelb, nie rot.
    Unterschritten { quote_pct: f32 },
    /// Kein OFP, kein Landesprit oder Tankwert unplausibel. Auch der
    /// Rueckfall fuer einen Datensatz, der das Feld nicht traegt.
    NichtPruefbar { grund: String },
}

impl Default for Reserve {
    fn default() -> Self {
        Reserve::NichtPruefbar {
            grund: "unbekannt".into(),
        }
    }
}

/// Das Badge im Kopf der Landung. Nur drei Farben, nie rot, keine Zahl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Badge {
    Gruen,
    Gelb,
    /// Auch der Rueckfall: „keine Aussage" ist die ehrliche Antwort, wenn
    /// ein Datensatz das Feld nicht traegt.
    #[default]
    Grau,
}

/// Die Sprit-Leiter des OFP, wie sie der Dispatcher plant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Leiter {
    pub taxi_kg: f32,
    pub trip_kg: f32,
    pub contingency_kg: f32,
    pub alternate_kg: f32,
    pub reserve_kg: f32,
    pub extra_kg: f32,
    pub block_kg: f32,
}

/// Das Ergebnis. Wird 1:1 in den Payload geschrieben und ueberall nur
/// gerendert. `fassung` steigt, wenn sich die Bedeutung eines Feldes
/// aendert, damit Anzeigen alte Datensaetze richtig lesen.
///
/// # Warum `#[serde(default)]` am ganzen Typ
///
/// `fassung` verspricht, dass eine spaetere Fassung einen aelteren Datensatz
/// noch lesen kann. Ohne `default` haelt serde das nicht: ein fehlendes Feld
/// ist ein harter Fehler. `LandingRecord.sprit` traegt zwar `default`, aber
/// das greift nur, wenn das ganze Objekt fehlt — nicht, wenn es da ist und
/// ein Feld vermisst. Und `storage::read_all` liest `landings.json` als EINEN
/// `Vec<LandingRecord>`: ein einziger Datensatz aelterer Fassung wuerde die
/// gesamte lokale Landungs-Historie in den Fehlerzweig kippen und als
/// `landings.json.corrupt-…` beiseitelegen. Genau das ist beim Sprung von
/// Fassung 1 auf 2 (`takeoff_fuel_kg`) moeglich geworden.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpritAuswertung {
    pub fassung: u8,
    pub bis_sinkflug: Option<Phase>,
    pub anflug: Option<Phase>,
    pub zeit_unter_schwelle_min: Option<f32>,
    pub schwelle_ft: Option<f32>,
    pub strecke_anflug_nm: Option<f32>,
    pub plan_strecke_anflug_nm: Option<f32>,
    pub reserve: Reserve,
    pub reserve_kg: Option<f32>,
    /// Tankstand beim Abheben. Steht hier, weil er sich aus den uebrigen
    /// Feldern nicht rekonstruieren laesst: Bleibt der Mehrverbrauch unter
    /// der Contingency, taucht er in `extra_genutzt_kg` gar nicht auf. Die
    /// Anzeige braucht ihn fuer die Abhebe-Marke der Sprit-Leiter.
    pub takeoff_fuel_kg: Option<f32>,
    pub landing_fuel_kg: Option<f32>,
    pub extra_getankt_kg: Option<f32>,
    pub extra_genutzt_kg: Option<f32>,
    pub extra_ungenutzt_kg: Option<f32>,
    pub contingency_verbraucht: Option<bool>,
    pub alternate_und_reserve_intakt: Option<bool>,
    pub leiter: Option<Leiter>,
    /// v1.7.36: Rollen vor dem Start, gegen den geplanten Taxi-Anteil.
    ///
    /// Erst damit ist der Flug lueckenlos abgedeckt: Bis v1.7.35 begann die
    /// Auswertung beim Abheben und endete beim Aufsetzen — bei einem A380
    /// blieben so vierstellige Kilogramm unerwaehnt.
    pub rollen_vor_start: Option<Phase>,
    /// Rollen nach der Landung — **ohne Plan**, nur die Zahl.
    ///
    /// SimBrief plant genau EINEN Taxi-Block, und der gilt dem Weg zum
    /// Start; `planned_burn_kg` beginnt beim Abheben. Fuer den Weg zurueck
    /// zum Stand gibt es also nichts zu vergleichen. Das ist kein Mangel,
    /// sondern die Wahrheit ueber das OFP — und es passt zur Hausform
    /// „zeigen, nicht benoten".
    pub rollen_nach_landung_kg: Option<f32>,
    pub badge: Badge,
}

/// Die Fassung dieser Auswertung — **Diagnose, keine Weiche.**
///
/// Sie sagt einem Leser (Mensch oder Werkzeug), nach welcher Regel die
/// Zahlen entstanden sind. Der Code verzweigt NICHT auf sie; ein aelterer
/// Datensatz bleibt lesbar, weil `#[serde(default)]` am Typ steht, nicht
/// weil hier eine Fallunterscheidung haengt. Wer das aendern will, muss
/// zuerst die Anzeigen anfassen — heute wertet sie niemand aus.
///
/// # Was die Fassungen bedeuten
///
/// * **1** (v1.7.35, erster Wurf): `plan_kg` beider Phasen stammt vom
///   TOD-Fix des Navlogs.
/// * **2** (v1.7.35, nach der Abnahme): `takeoff_fuel_kg` kam dazu. Ohne das
///   Feld rechnete die Anzeige den Abhebe-Tankstand zurueck — und lag bei
///   DLH 370 um 3 204 kg daneben, weil der Mehrverbrauch bei unverbrauchter
///   Contingency in keinem der uebrigen Felder steckt.
/// * **3** (v1.7.36): `plan_kg` und `plan_strecke_anflug_nm` gehoeren jetzt
///   zum GEMESSENEN Ort, nicht mehr zum TOD-Fix (`sprit_plan_am_ort`).
///   Derselbe Flug bekommt unter Fassung 2 und 3 verschiedene Zahlen — bei
///   14 von 39 Fluegen lag der Messpunkt unter 80 % der Plan-Reststrecke,
///   und dort wurde ein kurzes Ist-Stueck gegen einen vollen Plan
///   gerechnet. Dazu kommen `rollen_vor_start` und
///   `rollen_nach_landung_kg`: der Flug ist erst damit lueckenlos
///   abgedeckt, von den Triebwerken an bis zu den Triebwerken aus.
pub const SPRIT_AUSWERTUNG_FASSUNG: u8 = 3;

/// Unterhalb dieses geplanten Rollverbrauchs ist der Prozentwert ohne
/// Aussage. Ein Bizjet plant 60 kg Taxi; bei 20 kg Plan waeren 10 kg
/// Unterschied schon 50 %.
const MIN_PLAN_ROLLEN_KG: f32 = 30.0;

/// Ab dieser Abweichung sagt ein Prozentwert nichts mehr — dann steht die
/// Differenz in Kilogramm. Siehe `Phase::als_kg`.
const MAX_LESBARER_PROZENTWERT: f32 = 60.0;

/// Unterhalb dieses geplanten Anflugverbrauchs ist der Prozentwert ohne
/// Aussage (Division durch fast null).
///
/// 15 kg statt der urspruenglichen 50: Die Korpus-Pruefung hat gezeigt, dass
/// 50 kg rund 17 % aller Fluege die Anflugphase gekostet haetten — praktisch
/// die gesamte GA- und Bizjet-Flotte (E55P, BE58, DA40, PC12 …). Genau diesen
/// Mustern waere dann nur die andere Phase geblieben.
const MIN_PLAN_ANFLUG_KG: f32 = 15.0;

/// Mindestgroesse fuer die Phase „bis Sinkflug". Bei 18 kg Planverbrauch
/// (AC11, PA24, DA40 im Bestand) macht 1 kg Ablesefehler schon 7 % aus —
/// dreistellige Prozentwerte sind dort normal und sagen nichts. Dann lieber
/// keine Phase; Reserve, Leiter und Extra bleiben sichtbar.
const MIN_PLAN_BIS_TOD_KG: f32 = 120.0;

fn runde1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

fn pct(ist: f32, plan: f32) -> f32 {
    runde1((ist - plan) / plan * 100.0)
}

fn positiv(v: Option<f32>) -> Option<f32> {
    v.filter(|x| x.is_finite() && *x > 0.0)
}

fn nicht_negativ(v: Option<f32>) -> Option<f32> {
    v.filter(|x| x.is_finite() && *x >= 0.0)
}

/// Die eine Rechnung. Siehe Modulkopf.
pub fn auswerten(e: &SpritEingang) -> SpritAuswertung {
    let takeoff = positiv(e.takeoff_fuel_kg);
    let landing = nicht_negativ(e.landing_fuel_kg);
    let vergleich = positiv(e.sprit_bei_vergleichspunkt_kg);
    let plan_bis = positiv(e.plan_bis_vergleichspunkt_kg);
    let planned_burn = positiv(e.planned_burn_kg);
    let reserve = positiv(e.planned_reserve_kg);

    // ---- Phasen: nur bei plausiblem Tank, vollstaendigen Werten und
    // tatsaechlich geflogenem Plan ----
    let (bis_sinkflug, anflug) = if e.tank_plausibel && !e.ausweichflug {
        let bis = match (takeoff, vergleich, plan_bis) {
            (Some(to), Some(vp), Some(plan)) if to > vp && plan >= MIN_PLAN_BIS_TOD_KG => Some(Phase {
                // Prozent aus DENSELBEN Zahlen, die daneben stehen — sonst
                // passen angezeigter Wert und angezeigte Abweichung nicht
                // zusammen.
                ist_kg: (to - vp).round(),
                plan_kg: plan.round(),
                abweichung_pct: pct((to - vp).round(), plan.round()),
                // „bis Sinkflug" bleibt IMMER in Prozent: Der Plan-Anteil
                // ist dort gross genug, dass die Quote etwas aussagt. Sonst
                // saehe die GA-Flotte hier Kilogramm und anderswo Prozent.
                als_kg: false,
            }),
            _ => None,
        };
        let an = match (vergleich, landing, planned_burn, plan_bis) {
            // `burn - plan` ist der geplante Anflugverbrauch. Liegt der TOD
            // kurz vor dem Ziel, wird er winzig und jede Abweichung ergibt
            // dreistellige Prozentwerte ohne Aussage — dann lieber nichts.
            (Some(vp), Some(ldg), Some(burn), Some(plan))
                if vp > ldg && burn - plan >= MIN_PLAN_ANFLUG_KG =>
            {
                Some(Phase {
                    ist_kg: (vp - ldg).round(),
                    plan_kg: (burn - plan).round(),
                    abweichung_pct: pct((vp - ldg).round(), (burn - plan).round()),
                    als_kg: pct((vp - ldg).round(), (burn - plan).round()).abs()
                        > MAX_LESBARER_PROZENTWERT,
                })
            }
            _ => None,
        };
        (bis, an)
    } else {
        (None, None)
    };

    // ---- Rollen, vor dem Start und nach der Landung ----
    //
    // Beide Marken sind an ein Ereignis gerastet, nicht laufend gefuehrt —
    // sonst stuende hier bei jedem zweiten Flug Unsinn (siehe
    // `sprit_boden_marken`).
    let rollen_vor_start = match (positiv(e.engine_start_fuel_kg), takeoff, positiv(e.planned_taxi_kg)) {
        (Some(an), Some(to), Some(plan)) if an > to && plan >= MIN_PLAN_ROLLEN_KG => Some(Phase {
            ist_kg: (an - to).round(),
            plan_kg: plan.round(),
            abweichung_pct: pct((an - to).round(), plan.round()),
            // Beim Rollen ist der Plan klein (ein Bizjet plant 60 kg), aber
            // die Abweichung auch — hier bleibt Prozent lesbar.
            als_kg: false,
        }),
        _ => None,
    };
    let rollen_nach_landung_kg = match (landing, positiv(e.engine_off_fuel_kg)) {
        (Some(ldg), Some(aus)) if ldg > aus => Some((ldg - aus).round()),
        _ => None,
    };

    // ---- Reserve ----
    let reserve_status = match (reserve, landing) {
        _ if !e.tank_plausibel => Reserve::NichtPruefbar {
            grund: "tank_unplausibel".into(),
        },
        (None, _) => Reserve::NichtPruefbar {
            grund: "kein_ofp".into(),
        },
        (_, None) => Reserve::NichtPruefbar {
            grund: "kein_landesprit".into(),
        },
        (Some(res), Some(ldg)) => {
            // Deckel: Bei Kleinflugzeugen ergeben sich Quoten wie 1 298 %
            // (BE60: 19 kg Reserve, 247 kg gelandet). Die Zahl waere richtig
            // und trotzdem ohne Aussage.
            let quote = runde1((ldg / res * 100.0).min(999.0));
            if ldg >= res {
                Reserve::Intakt { quote_pct: quote }
            } else {
                Reserve::Unterschritten { quote_pct: quote }
            }
        }
    };
    let badge = match reserve_status {
        Reserve::Intakt { .. } => Badge::Gruen,
        Reserve::Unterschritten { .. } => Badge::Gelb,
        Reserve::NichtPruefbar { .. } => Badge::Grau,
    };

    // ---- Leiter, Contingency, Extra ----
    let leiter = match (
        planned_burn,
        nicht_negativ(e.planned_contingency_kg),
        nicht_negativ(e.planned_alternate_kg),
        reserve,
        nicht_negativ(e.planned_extra_kg),
        positiv(e.planned_block_fuel_kg),
    ) {
        (Some(trip), Some(cont), Some(alt), Some(res), Some(extra), Some(block)) => Some(Leiter {
            taxi_kg: nicht_negativ(e.planned_taxi_kg).unwrap_or(0.0).round(),
            trip_kg: trip.round(),
            contingency_kg: cont.round(),
            alternate_kg: alt.round(),
            reserve_kg: res.round(),
            extra_kg: extra.round(),
            block_kg: block.round(),
        }),
        _ => None,
    };

    let (extra_getankt, extra_genutzt, extra_ungenutzt, contingency_verbraucht, alt_res_intakt) =
        match (&leiter, landing, e.tank_plausibel) {
            (Some(l), Some(ldg), true) => {
                // Was nach dem Trip uebrig bleiben sollte — gemessen am
                // TATSAECHLICHEN Tankstand beim Abheben, nicht an der
                // geplanten Betankung. Wer mehr tankt als geplant, landet
                // hoeher; ohne diese Korrektur bekaeme er „Contingency und
                // Extra unangetastet", obwohl er mehr verbraucht hat. Und
                // umgekehrt: DLH370 hob mit 40 919 kg statt geplanter
                // 40 646 kg ab — 273 kg, die sonst falsch zugeordnet wuerden.
                let plan_landing = match takeoff {
                    Some(to) => (to - l.trip_kg).max(0.0),
                    None => l.contingency_kg + l.alternate_kg + l.reserve_kg + l.extra_kg,
                };
                let mehr = (plan_landing - ldg).max(0.0);
                let cont_verbraucht = l.contingency_kg > 0.0 && mehr >= l.contingency_kg;
                let genutzt = (mehr - l.contingency_kg).clamp(0.0, l.extra_kg).round();
                (
                    Some(l.extra_kg),
                    Some(genutzt),
                    Some((l.extra_kg - genutzt).round()),
                    Some(cont_verbraucht),
                    Some(ldg >= l.alternate_kg + l.reserve_kg),
                )
            }
            _ => (None, None, None, None, None),
        };

    SpritAuswertung {
        fassung: SPRIT_AUSWERTUNG_FASSUNG,
        bis_sinkflug,
        anflug,
        zeit_unter_schwelle_min: nicht_negativ(e.zeit_unter_schwelle_s).map(|s| runde1(s / 60.0)),
        schwelle_ft: positiv(e.schwelle_ft).map(|v| v.round()),
        strecke_anflug_nm: positiv(e.strecke_ab_vergleichspunkt_nm).map(|v| v.round()),
        // Nach einem Ausweichflug gab es diesen Plan nicht — die geflogene
        // Strecke bleibt eine Tatsache, der Plan dazu nicht.
        plan_strecke_anflug_nm: if e.ausweichflug {
            None
        } else {
            positiv(e.plan_rest_nm).map(|v| v.round())
        },
        reserve: reserve_status,
        reserve_kg: reserve.map(|v| v.round()),
        takeoff_fuel_kg: takeoff.map(|v| v.round()),
        landing_fuel_kg: landing.map(|v| v.round()),
        extra_getankt_kg: extra_getankt,
        extra_genutzt_kg: extra_genutzt,
        extra_ungenutzt_kg: extra_ungenutzt,
        contingency_verbraucht,
        alternate_und_reserve_intakt: alt_res_intakt,
        leiter,
        rollen_vor_start,
        rollen_nach_landung_kg,
        badge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DLH370, 17.09.2026, ESSA→EDDM, A380 D-AIMK — die echten Zahlen aus
    /// Track und OFP. Sie stehen so im Entwurf, den Thomas freigegeben hat.
    fn dlh370() -> SpritEingang {
        SpritEingang {
            planned_burn_kg: Some(21_218.0),
            plan_bis_vergleichspunkt_kg: Some(19_353.0),
            // Echte Navlog-Summe nach dem TOD-Fix aus dem OFP dieses Fluges
            // (die 191 hier waren frueher handgeschrieben und falsch).
            plan_rest_nm: Some(139.0),
            takeoff_fuel_kg: Some(40_919.0),
            sprit_bei_vergleichspunkt_kg: Some(22_231.0),
            landing_fuel_kg: Some(16_770.0),
            strecke_ab_vergleichspunkt_nm: Some(182.0),
            zeit_unter_schwelle_s: Some(948.0),
            schwelle_ft: Some(9_487.0),
            planned_taxi_kg: Some(998.0),
            planned_contingency_kg: Some(1_061.0),
            planned_alternate_kg: Some(8_273.0),
            planned_reserve_kg: Some(4_916.0),
            planned_extra_kg: Some(5_178.0),
            planned_block_fuel_kg: Some(41_644.0),
            // Die echten Boden-Marken dieses Fluges: angelassen mit
            // 41 644 kg, abgestellt mit 16 121 kg.
            engine_start_fuel_kg: Some(41_644.0),
            engine_off_fuel_kg: Some(16_121.0),
            tank_plausibel: true,
            ausweichflug: false,
        }
    }

    /// Ein Datensatz der Fassung 1 — ohne `takeoff_fuel_kg` — muss lesbar
    /// bleiben.
    ///
    /// Sonst kippt `storage::read_all` die GANZE `landings.json` in den
    /// Fehlerzweig (sie wird als ein einziger `Vec<LandingRecord>` gelesen),
    /// und die komplette lokale Landungs-Historie verschwindet aus der
    /// Oberflaeche. Der Test faellt, sobald jemand `#[serde(default)]` an
    /// `SpritAuswertung` entfernt oder ein neues Feld ohne Rueckfall ergaenzt.
    #[test]
    fn sprit_auswertung_aeltere_fassung_bleibt_lesbar() {
        // Genau die Felder der Fassung 1, kein `takeoff_fuel_kg`.
        let alt = r#"{
            "fassung": 1,
            "bis_sinkflug": { "ist_kg": 18688.0, "plan_kg": 19353.0, "abweichung_pct": -3.4 },
            "anflug": null,
            "zeit_unter_schwelle_min": 15.8,
            "schwelle_ft": 9487.0,
            "strecke_anflug_nm": 182.0,
            "plan_strecke_anflug_nm": 139.0,
            "reserve": { "status": "intakt", "quote_pct": 341.1 },
            "reserve_kg": 4916.0,
            "landing_fuel_kg": 16770.0,
            "extra_getankt_kg": 5178.0,
            "extra_genutzt_kg": 1870.0,
            "extra_ungenutzt_kg": 3308.0,
            "contingency_verbraucht": false,
            "alternate_und_reserve_intakt": true,
            "leiter": null,
            "badge": "gruen"
        }"#;
        let a: SpritAuswertung = serde_json::from_str(alt).expect("Fassung 1 muss lesbar sein");
        assert_eq!(a.fassung, 1, "die Fassung bleibt stehen, sie wird nicht hochgelogen");
        assert_eq!(a.takeoff_fuel_kg, None, "das fehlende Feld wird zu None");
        // Gegenprobe: Die uebrigen Werte kommen unveraendert an — der
        // Rueckfall darf nicht alles auf Default ziehen.
        assert_eq!(a.landing_fuel_kg, Some(16_770.0));
        assert_eq!(a.plan_strecke_anflug_nm, Some(139.0));
        assert_eq!(a.badge, Badge::Gruen);
        assert_eq!(a.reserve, Reserve::Intakt { quote_pct: 341.1 });
    }

    /// Gegenprobe zur Rueckwaertstoleranz: Ein voellig leeres Objekt ergibt
    /// den ehrlichen Leerstand — Grau und „nicht pruefbar", nicht Gruen.
    #[test]
    fn sprit_auswertung_leeres_objekt_ist_grau_und_nicht_pruefbar() {
        let a: SpritAuswertung = serde_json::from_str("{}").expect("leer muss lesbar sein");
        assert_eq!(a.badge, Badge::Grau);
        assert!(matches!(a.reserve, Reserve::NichtPruefbar { .. }));
        assert_eq!(a.fassung, 0, "keine Fassung behauptet, wo keine stand");
    }

    #[test]
    fn sprit_auswertung_dlh370_phasen() {
        let a = auswerten(&dlh370());
        let bis = a.bis_sinkflug.expect("bis Sinkflug");
        assert_eq!(bis.ist_kg, 18_688.0);
        assert_eq!(bis.plan_kg, 19_353.0);
        assert_eq!(bis.abweichung_pct, -3.4);
        let an = a.anflug.expect("Anflug");
        assert_eq!(an.ist_kg, 5_461.0);
        assert_eq!(an.plan_kg, 1_865.0);
        assert_eq!(an.abweichung_pct, 192.8);
        assert_eq!(a.zeit_unter_schwelle_min, Some(15.8));
        assert_eq!(a.strecke_anflug_nm, Some(182.0));
        assert_eq!(a.plan_strecke_anflug_nm, Some(139.0));
    }

    #[test]
    fn sprit_auswertung_dlh370_reserve_und_extra() {
        let a = auswerten(&dlh370());
        assert_eq!(a.reserve, Reserve::Intakt { quote_pct: 341.1 });
        assert_eq!(a.badge, Badge::Gruen);
        assert_eq!(a.extra_getankt_kg, Some(5_178.0));
        assert_eq!(a.extra_genutzt_kg, Some(1_870.0));
        assert_eq!(a.extra_ungenutzt_kg, Some(3_308.0));
        assert_eq!(a.contingency_verbraucht, Some(true));
        assert_eq!(a.alternate_und_reserve_intakt, Some(true));
        let l = a.leiter.expect("Leiter");
        assert_eq!(l.block_kg, 41_644.0);
        assert_eq!(l.taxi_kg + l.trip_kg + l.contingency_kg + l.alternate_kg + l.reserve_kg + l.extra_kg, 41_644.0);
    }

    /// EDDH→EFHK, A350: 1 835 kg gelandet bei 2 647 kg Final Reserve.
    /// Gelb, 69 % — und nichts weiter. Keine Note, keine Blockierung.
    #[test]
    fn sprit_auswertung_unter_reserve_ist_gelb() {
        let mut e = dlh370();
        e.planned_reserve_kg = Some(2_647.0);
        e.landing_fuel_kg = Some(1_835.0);
        let a = auswerten(&e);
        assert_eq!(a.reserve, Reserve::Unterschritten { quote_pct: 69.3 });
        assert_eq!(a.badge, Badge::Gelb);
        assert_eq!(a.alternate_und_reserve_intakt, Some(false));
    }

    /// Manual-Flug ohne OFP: keine Plan-Werte → Grau, keine Phasen, keine
    /// Leiter. Der Landesprit selbst bleibt sichtbar.
    #[test]
    fn sprit_auswertung_ohne_ofp_ist_grau() {
        let e = SpritEingang {
            takeoff_fuel_kg: Some(3_000.0),
            landing_fuel_kg: Some(1_200.0),
            tank_plausibel: true,
            ..Default::default()
        };
        let a = auswerten(&e);
        assert_eq!(a.badge, Badge::Grau);
        assert_eq!(
            a.reserve,
            Reserve::NichtPruefbar { grund: "kein_ofp".into() }
        );
        assert!(a.bis_sinkflug.is_none() && a.anflug.is_none() && a.leiter.is_none());
        assert_eq!(a.landing_fuel_kg, Some(1_200.0));
    }

    /// Unplausibler Tankwert (Auslesefehler): lieber „nicht pruefbar" als
    /// eine falsche Warnung — und keine Phasen aus falschen Zahlen.
    #[test]
    fn sprit_auswertung_tank_unplausibel_warnt_nicht() {
        let mut e = dlh370();
        e.landing_fuel_kg = Some(7.0);
        e.tank_plausibel = false;
        let a = auswerten(&e);
        assert_eq!(a.badge, Badge::Grau);
        assert_eq!(
            a.reserve,
            Reserve::NichtPruefbar { grund: "tank_unplausibel".into() }
        );
        assert!(a.bis_sinkflug.is_none() && a.anflug.is_none());
        assert_eq!(a.extra_genutzt_kg, None);
    }

    /// Wer sparsamer landet als geplant, hat Contingency und Extra nicht
    /// angeruehrt — genutzt 0, ungenutzt alles.
    #[test]
    fn sprit_auswertung_sparsam_laesst_extra_ganz() {
        let mut e = dlh370();
        e.landing_fuel_kg = Some(20_000.0);
        let a = auswerten(&e);
        assert_eq!(a.contingency_verbraucht, Some(false));
        assert_eq!(a.extra_genutzt_kg, Some(0.0));
        assert_eq!(a.extra_ungenutzt_kg, Some(5_178.0));
        assert!(matches!(a.reserve, Reserve::Intakt { .. }));
    }

    /// Neustart mitten im Flug, Vergleichspunkt verpasst: Phasen fehlen,
    /// Reserve und Leiter bleiben — die Sektion zeigt, was sie hat.
    #[test]
    fn sprit_auswertung_ohne_vergleichspunkt_zeigt_den_rest() {
        let mut e = dlh370();
        e.sprit_bei_vergleichspunkt_kg = None;
        let a = auswerten(&e);
        assert!(a.bis_sinkflug.is_none() && a.anflug.is_none());
        assert_eq!(a.badge, Badge::Gruen);
        assert!(a.leiter.is_some());
        assert_eq!(a.extra_genutzt_kg, Some(1_870.0));
    }


    /// Die Grenzen MIN_PLAN_ANFLUG_KG (15) und MIN_PLAN_BIS_TOD_KG (120)
    /// genau an ihrer Kante — vorher war keine von beiden abgesteckt.
    #[test]
    fn sprit_auswertung_grenzen_genau_getroffen() {
        let bau = |plan_trip: f32, plan_bis: f32| SpritEingang {
            planned_burn_kg: Some(plan_trip),
            plan_bis_vergleichspunkt_kg: Some(plan_bis),
            takeoff_fuel_kg: Some(5_000.0),
            sprit_bei_vergleichspunkt_kg: Some(4_000.0),
            landing_fuel_kg: Some(3_500.0),
            tank_plausibel: true,
            ..Default::default()
        };
        // bis Sinkflug: 120 kg Plan zaehlt, 119 nicht.
        assert!(auswerten(&bau(200.0, 120.0)).bis_sinkflug.is_some());
        assert!(auswerten(&bau(200.0, 119.0)).bis_sinkflug.is_none());
        // Anflug: 15 kg Plan zaehlt (200 − 185), 14 nicht.
        assert!(auswerten(&bau(200.0, 185.0)).anflug.is_some());
        assert!(auswerten(&bau(200.0, 186.0)).anflug.is_none());
    }

    /// Ein Flug mit rund 100 kg Plan-Trip verliert „bis Sinkflug", behaelt
    /// aber den Anflug — der Fall, den der Pruefer als unerklaert bemaengelt
    /// hat. Der Test haelt fest, dass das kein Unfall ist.
    #[test]
    fn sprit_auswertung_kleiner_flug_behaelt_nur_den_anflug() {
        let e = SpritEingang {
            planned_burn_kg: Some(100.0),
            plan_bis_vergleichspunkt_kg: Some(80.0), // 20 kg Anflugplan
            takeoff_fuel_kg: Some(300.0),
            sprit_bei_vergleichspunkt_kg: Some(215.0),
            landing_fuel_kg: Some(190.0),
            tank_plausibel: true,
            ..Default::default()
        };
        let a = auswerten(&e);
        assert!(a.bis_sinkflug.is_none(), "80 kg Plan sind zu klein");
        assert!(a.anflug.is_some(), "20 kg Anflugplan sind messbar");
    }

    /// Wer mehr tankt als geplant, landet hoeher — das darf nicht als
    /// „Extra unangetastet" durchgehen.
    #[test]
    fn sprit_auswertung_extra_erkennt_tankern() {
        let mut e = dlh370();
        e.takeoff_fuel_kg = Some(43_919.0); // 3 t ueber Plan getankt
        e.landing_fuel_kg = Some(19_770.0); // entsprechend hoeher gelandet
        let a = auswerten(&e);
        // Ohne Korrektur waere hier „nichts verbraucht" herausgekommen.
        assert_eq!(a.extra_genutzt_kg, Some(1_870.0));
    }

    /// Nach einem Ausweichflug wurde der Plan nicht geflogen: keine Phasen,
    /// keine Plan-Reststrecke. Die geflogenen Zahlen bleiben (QS-Befund P2-4).
    #[test]
    fn sprit_auswertung_divert_zeigt_keine_planphasen() {
        let mut e = dlh370();
        e.ausweichflug = true;
        let a = auswerten(&e);
        assert!(a.bis_sinkflug.is_none() && a.anflug.is_none());
        assert_eq!(a.plan_strecke_anflug_nm, None);
        // Tatsachen bleiben: Reserve, Landesprit, geflogene Strecke.
        assert!(matches!(a.reserve, Reserve::Intakt { .. }));
        assert_eq!(a.landing_fuel_kg, Some(16_770.0));
        assert_eq!(a.strecke_anflug_nm, Some(182.0));
    }

    /// Ein winziger geplanter Anflugverbrauch (TOD kurz vor dem Ziel) ergibt
    /// keinen Prozentwert, sondern keinen — sonst stuenden dreistellige
    /// Zahlen ohne Aussage da.
    #[test]
    fn sprit_auswertung_winziger_anflugplan_ergibt_keine_phase() {
        let mut e = dlh370();
        // 8 kg Anflugplan — unter MIN_PLAN_ANFLUG_KG (15). Die Grenze liegt
        // bewusst niedrig, damit die GA- und Bizjet-Flotte ihre Anflugphase
        // behaelt; darunter ist die Zahl aber nicht mehr messbar.
        e.plan_bis_vergleichspunkt_kg = Some(21_210.0);
        let a = auswerten(&e);
        assert!(a.anflug.is_none());
        assert!(a.bis_sinkflug.is_some());
    }


    /// Ein winziger Plan bis TOD (Kleinflugzeug: 18 kg Trip) ergibt keine
    /// Phase — 1 kg Ablesefehler waeren dort 7 % (Korpus-Befund P3).
    #[test]
    fn sprit_auswertung_winziger_plan_ergibt_keine_phase() {
        let e = SpritEingang {
            planned_burn_kg: Some(18.0),
            plan_bis_vergleichspunkt_kg: Some(14.0),
            takeoff_fuel_kg: Some(90.0),
            sprit_bei_vergleichspunkt_kg: Some(76.0),
            landing_fuel_kg: Some(72.0),
            tank_plausibel: true,
            ..Default::default()
        };
        let a = auswerten(&e);
        assert!(a.bis_sinkflug.is_none(), "winziger Plan darf keine Phase ergeben");
        assert!(a.anflug.is_none());
        // Der Landesprit bleibt eine Tatsache.
        assert_eq!(a.landing_fuel_kg, Some(72.0));
    }

    /// Ein Bizjet mit kleinem, aber messbarem Anflugplan behaelt die Phase —
    /// die frueheren 50 kg haetten 17 % aller Fluege die Phase gekostet.
    #[test]
    fn sprit_auswertung_bizjet_behaelt_die_anflugphase() {
        let e = SpritEingang {
            planned_burn_kg: Some(900.0),
            plan_bis_vergleichspunkt_kg: Some(870.0), // 30 kg Anflugplan
            takeoff_fuel_kg: Some(1_400.0),
            sprit_bei_vergleichspunkt_kg: Some(520.0),
            landing_fuel_kg: Some(470.0),
            tank_plausibel: true,
            ..Default::default()
        };
        let a = auswerten(&e);
        assert!(a.anflug.is_some(), "30 kg Anflugplan sind messbar");
        assert!(a.bis_sinkflug.is_some());
    }

    /// Reserve-Quoten von Kleinflugzeugen werden gedeckelt — 1 298 % waere
    /// richtig und trotzdem ohne Aussage (Korpus-Befund P7).
    #[test]
    fn sprit_auswertung_reserve_quote_gedeckelt() {
        let e = SpritEingang {
            planned_reserve_kg: Some(19.0),
            landing_fuel_kg: Some(247.0),
            takeoff_fuel_kg: Some(300.0),
            tank_plausibel: true,
            ..Default::default()
        };
        match auswerten(&e).reserve {
            Reserve::Intakt { quote_pct } => assert_eq!(quote_pct, 999.0),
            andere => panic!("erwartet Intakt, war {andere:?}"),
        }
    }

    /// Der Abhebe-Tankstand steht im Ergebnis und ist nicht geraten.
    #[test]
    fn sprit_auswertung_nennt_den_abhebe_tankstand() {
        let a = auswerten(&dlh370());
        assert_eq!(a.takeoff_fuel_kg, Some(40_919.0));
        assert_eq!(a.landing_fuel_kg, Some(16_770.0));
        // Und die Differenz ist der Trip-Verbrauch — die Aussage, die die
        // beiden Marken der Sprit-Leiter machen.
        let verbrauch = a.takeoff_fuel_kg.unwrap() - a.landing_fuel_kg.unwrap();
        assert_eq!(verbrauch, 24_149.0);
    }

    /// Das Ergebnis ist das Wire-Format: Enum-Tags und Feldnamen sind
    /// Vertrag mit Webapp und Client-Oberflaeche.
    #[test]
    fn sprit_auswertung_wire_format() {
        let a = auswerten(&dlh370());
        let json = serde_json::to_string(&a).expect("json");
        assert!(json.contains("\"reserve\":{\"status\":\"intakt\",\"quote_pct\":341.1}"), "{json}");
        assert!(json.contains("\"badge\":\"gruen\""), "{json}");
        assert!(json.contains("\"fassung\":2"), "{json}");
        let zurueck: SpritAuswertung = serde_json::from_str(&json).expect("roundtrip");
        assert_eq!(zurueck, a);
    }
}
