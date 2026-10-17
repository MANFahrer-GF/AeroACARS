//! Tageszeit und Ortszeit aus dem SimBrief-Plan (Entscheidung Thomas
//! 17.10.2026: die Sim-Uhr wird NICHT benutzt — sie kann beliebig stehen).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tageszeit {
    Morgen,
    Mittag,
    Nachmittag,
    Abend,
    Nacht,
}

/// Ortszeit in Minuten seit Mitternacht (0..1440) aus UTC-Epoch-Sekunden
/// und dem UTC-Versatz des Flughafens in Minuten.
pub fn ortszeit_minuten(epoch_utc: i64, utc_versatz_min: i32) -> u32 {
    let sek = epoch_utc + i64::from(utc_versatz_min) * 60;
    (sek.rem_euclid(86_400) / 60) as u32
}

/// Morgen 5–11, Mittag 11–14, Nachmittag 14–18, Abend 18–22, Nacht 22–5.
pub fn tageszeit(ortszeit_min: u32) -> Tageszeit {
    match ortszeit_min / 60 {
        5..=10 => Tageszeit::Morgen,
        11..=13 => Tageszeit::Mittag,
        14..=17 => Tageszeit::Nachmittag,
        18..=21 => Tageszeit::Abend,
        _ => Tageszeit::Nacht,
    }
}

/// Planzeiten des Flugs, alle in UTC-Epoch-Sekunden.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Planzeiten {
    pub off_block: Option<i64>,
    pub on_block: Option<i64>,
    /// UTC-Versatz am Abflug bzw. Ziel in Minuten (aus dem OFP). Ohne
    /// Versatz gibt es keine Ortszeit und keinen Tageszeit-Gruß.
    pub versatz_abflug_min: Option<i32>,
    pub versatz_ziel_min: Option<i32>,
}

impl Planzeiten {
    /// Tageszeit für die Ansagen bis zum Reiseflug (Ortszeit Abflug).
    pub fn tageszeit_abflug(&self) -> Option<Tageszeit> {
        Some(tageszeit(ortszeit_minuten(self.off_block?, self.versatz_abflug_min?)))
    }

    /// Tageszeit für die Ansagen ab dem Sinkflug (Ortszeit Ziel).
    pub fn tageszeit_ziel(&self) -> Option<Tageszeit> {
        Some(tageszeit(ortszeit_minuten(self.on_block?, self.versatz_ziel_min?)))
    }

    /// Erwartete Ankunft = Plan + gemessene Abweichung (Minuten), damit sie
    /// zur Verspätungsansage passt. Als Ortszeit am Ziel in Minuten.
    pub fn ankunft_ortszeit_min(&self, abweichung_min: i32) -> Option<u32> {
        let an = self.on_block? + i64::from(abweichung_min) * 60;
        Some(ortszeit_minuten(an, self.versatz_ziel_min?))
    }

    /// Zeitverschiebung Abflug → Ziel in Minuten (für „Uhren eine Stunde vor“).
    pub fn zeitverschiebung_min(&self) -> Option<i32> {
        Some(self.versatz_ziel_min? - self.versatz_abflug_min?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grenzen_der_tageszeiten() {
        assert_eq!(tageszeit(4 * 60 + 59), Tageszeit::Nacht);
        assert_eq!(tageszeit(5 * 60), Tageszeit::Morgen);
        assert_eq!(tageszeit(10 * 60 + 59), Tageszeit::Morgen);
        assert_eq!(tageszeit(11 * 60), Tageszeit::Mittag);
        assert_eq!(tageszeit(14 * 60), Tageszeit::Nachmittag);
        assert_eq!(tageszeit(18 * 60), Tageszeit::Abend);
        assert_eq!(tageszeit(22 * 60), Tageszeit::Nacht);
        assert_eq!(tageszeit(0), Tageszeit::Nacht);
    }

    #[test]
    fn ortszeit_ueber_mitternacht_und_negativer_versatz() {
        // 2026-10-17 23:30 UTC, Dubai +4 h → 03:30
        let t = 1_792_279_800; // 23:30 UTC
        assert_eq!(ortszeit_minuten(t, 240), 3 * 60 + 30);
        // New York −4 h → 19:30
        assert_eq!(ortszeit_minuten(t, -240), 19 * 60 + 30);
    }

    #[test]
    fn plan_bestimmt_tageszeit_nicht_die_sim_uhr() {
        let p = Planzeiten {
            off_block: Some(1_792_279_800), // 23:30 UTC
            on_block: Some(1_792_279_800 + 6 * 3600),
            versatz_abflug_min: Some(120),  // FRA Sommerzeit → 01:30
            versatz_ziel_min: Some(240),    // DXB → 09:30
        };
        assert_eq!(p.tageszeit_abflug(), Some(Tageszeit::Nacht));
        assert_eq!(p.tageszeit_ziel(), Some(Tageszeit::Morgen));
        assert_eq!(p.zeitverschiebung_min(), Some(120));
        assert_eq!(p.ankunft_ortszeit_min(-10), Some(9 * 60 + 20));
    }

    #[test]
    fn ohne_zeitzone_keine_tageszeit() {
        let p = Planzeiten { off_block: Some(0), ..Default::default() };
        assert_eq!(p.tageszeit_abflug(), None);
    }
}
