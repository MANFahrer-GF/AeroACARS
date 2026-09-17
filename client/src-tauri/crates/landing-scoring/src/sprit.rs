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
    /// `false`, wenn der Tankstand beim Aufsetzen nicht zum Verlauf passt
    /// (Auslesefehler, nur zwei von vier Tanks, Sprung). Dann gibt es
    /// „nicht pruefbar" statt einer falschen Warnung.
    pub tank_plausibel: bool,
}

/// Eine Flugphase: was gebraucht wurde, was geplant war, Abweichung in
/// Prozent (eine Nachkommastelle, gerundet — Anzeige und Wert sind
/// dieselbe Zahl).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    pub ist_kg: f32,
    pub plan_kg: f32,
    pub abweichung_pct: f32,
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
    /// Kein OFP, kein Landesprit oder Tankwert unplausibel.
    NichtPruefbar { grund: String },
}

/// Das Badge im Kopf der Landung. Nur drei Farben, nie rot, keine Zahl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Badge {
    Gruen,
    Gelb,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub landing_fuel_kg: Option<f32>,
    pub extra_getankt_kg: Option<f32>,
    pub extra_genutzt_kg: Option<f32>,
    pub extra_ungenutzt_kg: Option<f32>,
    pub contingency_verbraucht: Option<bool>,
    pub alternate_und_reserve_intakt: Option<bool>,
    pub leiter: Option<Leiter>,
    pub badge: Badge,
}

pub const SPRIT_AUSWERTUNG_FASSUNG: u8 = 1;

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

    // ---- Phasen: nur bei plausiblem Tank und vollstaendigen Werten ----
    let (bis_sinkflug, anflug) = if e.tank_plausibel {
        let bis = match (takeoff, vergleich, plan_bis) {
            (Some(to), Some(vp), Some(plan)) if to > vp => Some(Phase {
                ist_kg: (to - vp).round(),
                plan_kg: plan.round(),
                abweichung_pct: pct(to - vp, plan),
            }),
            _ => None,
        };
        let an = match (vergleich, landing, planned_burn, plan_bis) {
            (Some(vp), Some(ldg), Some(burn), Some(plan)) if vp > ldg && burn > plan => {
                Some(Phase {
                    ist_kg: (vp - ldg).round(),
                    plan_kg: (burn - plan).round(),
                    abweichung_pct: pct(vp - ldg, burn - plan),
                })
            }
            _ => None,
        };
        (bis, an)
    } else {
        (None, None)
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
            let quote = runde1(ldg / res * 100.0);
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
                // Geplanter Landesprit = alles, was nach dem Trip uebrig
                // bleiben sollte. Was darunter liegt, wurde „zusaetzlich"
                // gebraucht — zuerst aus Contingency, dann aus Extra.
                let plan_landing = l.contingency_kg + l.alternate_kg + l.reserve_kg + l.extra_kg;
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
        plan_strecke_anflug_nm: positiv(e.plan_rest_nm).map(|v| v.round()),
        reserve: reserve_status,
        reserve_kg: reserve.map(|v| v.round()),
        landing_fuel_kg: landing.map(|v| v.round()),
        extra_getankt_kg: extra_getankt,
        extra_genutzt_kg: extra_genutzt,
        extra_ungenutzt_kg: extra_ungenutzt,
        contingency_verbraucht,
        alternate_und_reserve_intakt: alt_res_intakt,
        leiter,
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
            plan_rest_nm: Some(191.0),
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
            tank_plausibel: true,
        }
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
        assert_eq!(a.plan_strecke_anflug_nm, Some(191.0));
    }

    #[test]
    fn sprit_auswertung_dlh370_reserve_und_extra() {
        let a = auswerten(&dlh370());
        assert_eq!(a.reserve, Reserve::Intakt { quote_pct: 341.1 });
        assert_eq!(a.badge, Badge::Gruen);
        assert_eq!(a.extra_getankt_kg, Some(5_178.0));
        assert_eq!(a.extra_genutzt_kg, Some(1_597.0));
        assert_eq!(a.extra_ungenutzt_kg, Some(3_581.0));
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
        assert_eq!(a.extra_genutzt_kg, Some(1_597.0));
    }

    /// Das Ergebnis ist das Wire-Format: Enum-Tags und Feldnamen sind
    /// Vertrag mit Webapp und Client-Oberflaeche.
    #[test]
    fn sprit_auswertung_wire_format() {
        let a = auswerten(&dlh370());
        let json = serde_json::to_string(&a).expect("json");
        assert!(json.contains("\"reserve\":{\"status\":\"intakt\",\"quote_pct\":341.1}"), "{json}");
        assert!(json.contains("\"badge\":\"gruen\""), "{json}");
        assert!(json.contains("\"fassung\":1"), "{json}");
        let zurueck: SpritAuswertung = serde_json::from_str(&json).expect("roundtrip");
        assert_eq!(zurueck, a);
    }
}
