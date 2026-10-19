//! Boarding-Musik (2.1.1 G, Entscheidungen Thomas 19.10.2026 Nr. 6–15):
//! wann sie spielt, was sie spielt und ob ein Lied angefragt wird. Pure
//! Logik; die Wiedergabe macht der Host.
//!
//! - Linienflug: Start mit der Boarding-Begrüßung (gemeldet, gespielt oder
//!   abgewählt), Schleife bis „Türen zu“.
//! - Bizjet und Fracht (keine Begrüßung): Start mit der Phase Boarding, Ende
//!   beim Beacon bzw. Pushback — nur eigene Musik, nie ein erzeugtes Lied.
//! - Eigene Musik für die Airline des Flugs (sonst „alle“) geht vor; dann
//!   wird kein Lied angefragt.

use sim_core::FlightPhase;

use crate::flugart::Flugart;
use crate::planer::rang;

/// Ordnername für Musik, die für alle Airlines gilt (Design 19.10.2026:
/// Unterordner je Airline, dazu `Alle/`).
pub const ALLE: &str = "Alle";

/// Was der Host je Takt über den Flug weiß.
#[derive(Debug, Clone, Copy, Default)]
pub struct MusikTakt {
    pub phase: FlightPhase,
    pub am_boden: bool,
    /// Beacon an; `None` = meldet das Flugzeug nicht.
    pub beacon: Option<bool>,
    pub pushback_aktiv: bool,
    /// Linienflug: Die Boarding-Begrüßung ist gemeldet (auch abgewählt oder
    /// schon vor einem Neustart gelaufen).
    pub begruessung: bool,
}

/// Das Zeitfenster der Musik auf einem Flug. „Türen zu“ wie im Planer:
/// Beacon geht am Boden an, sonst Pushback oder eine spätere Phase. Danach
/// kommt auf diesem Flug keine Musik mehr.
#[derive(Debug, Clone, Default)]
pub struct MusikFenster {
    letztes_beacon: Option<bool>,
    tueren_zu: bool,
}

impl MusikFenster {
    /// Soll jetzt Musik laufen?
    pub fn tick(&mut self, flugart: Flugart, t: &MusikTakt) -> bool {
        let r = rang(t.phase);
        let beacon_an = t.am_boden && t.beacon == Some(true) && self.letztes_beacon == Some(false);
        self.letztes_beacon = t.beacon;
        if beacon_an || t.pushback_aktiv || r >= rang(FlightPhase::Pushback) {
            self.tueren_zu = true;
        }
        if self.tueren_zu || t.phase != FlightPhase::Boarding {
            return false;
        }
        match flugart {
            Flugart::Linie => t.begruessung,
            Flugart::Bizjet | Flugart::Aus => true,
        }
    }

    pub fn tueren_zu(&self) -> bool {
        self.tueren_zu
    }
}

/// Welche eigene Musik gilt für diesen Flug: der Ordner der Airline, sonst
/// „Alle“. `ordner` = vorhandene Ordner mit mindestens einem Stück.
pub fn eigener_ordner<'a>(airline_icao: &str, ordner: &'a [String]) -> Option<&'a str> {
    let icao = airline_icao.trim().to_ascii_uppercase();
    ordner
        .iter()
        .find(|o| !icao.is_empty() && o.eq_ignore_ascii_case(&icao))
        .or_else(|| ordner.iter().find(|o| o.as_str() == ALLE))
        .map(String::as_str)
}

/// Wird ein Boarding-Lied angefragt? Nur Linienflüge, nur mit Haken
/// „Boarding-Musik“ an (Nr. 15), und nur ohne eigene Musik (G2).
pub fn lied_anfragen(flugart: Flugart, haken: bool, eigene_musik: bool) -> bool {
    flugart == Flugart::Linie && haken && !eigene_musik
}

/// Zufallsreihenfolge der eigenen Stücke (Nr. 13), danach von vorn in
/// derselben Reihenfolge. `saat` je Flug, damit ein Neustart dieselbe Folge
/// spielt.
pub fn reihenfolge(anzahl: usize, saat: u64) -> Vec<usize> {
    let mut v: Vec<usize> = (0..anzahl).collect();
    // xorshift64*: genügt für eine Spielfolge, ohne neue Abhängigkeit.
    let mut x = saat | 1;
    for i in (1..v.len()).rev() {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let z = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let j = (z % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
    v
}

/// Erlaubte Formate eigener Musik (Pflichtenheft G2).
pub const FORMATE: [&str; 6] = ["mp3", "m4a", "aac", "wav", "flac", "ogg"];

pub fn format_erlaubt(dateiname: &str) -> bool {
    dateiname
        .rsplit_once('.')
        .is_some_and(|(_, e)| FORMATE.iter().any(|f| f.eq_ignore_ascii_case(e)))
}

/// Gültiger Ordnername: „Alle“ oder eine ICAO-Airline-Kennung (2–4
/// Buchstaben/Ziffern). Hält Pfade wie `../` aus der Ablage.
pub fn airline_gueltig(name: &str) -> bool {
    name == ALLE || ((2..=4).contains(&name.len()) && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use FlightPhase::*;

    fn takt(phase: FlightPhase, beacon: bool, begruessung: bool) -> MusikTakt {
        MusikTakt { phase, am_boden: true, beacon: Some(beacon), pushback_aktiv: false, begruessung }
    }

    /// Peter: Musik erst mit der Begrüßung, Schleife bis „Türen zu“ (Beacon).
    #[test]
    fn linie_von_der_begruessung_bis_tueren_zu() {
        let mut f = MusikFenster::default();
        assert!(!f.tick(Flugart::Linie, &takt(Boarding, false, false)), "vor der Begrüßung still");
        assert!(f.tick(Flugart::Linie, &takt(Boarding, false, true)));
        assert!(f.tick(Flugart::Linie, &takt(Boarding, false, true)));
        assert!(!f.tick(Flugart::Linie, &takt(Boarding, true, true)), "Beacon = Türen zu");
        assert!(!f.tick(Flugart::Linie, &takt(Boarding, false, true)), "danach nie wieder auf diesem Flug");
    }

    /// Bizjet/Fracht: ab Phase Boarding bis Beacon bzw. Pushback.
    #[test]
    fn bizjet_und_fracht_ab_boarding_bis_pushback() {
        for art in [Flugart::Bizjet, Flugart::Aus] {
            let mut f = MusikFenster::default();
            assert!(!f.tick(art, &takt(Preflight, false, false)));
            assert!(f.tick(art, &takt(Boarding, false, false)), "{art:?}");
            assert!(!f.tick(art, &takt(Pushback, false, false)), "{art:?}");
        }
        // Beacon schon beim Laden an: keine Flanke, Ende erst beim Pushback.
        let mut f = MusikFenster::default();
        assert!(f.tick(Flugart::Aus, &takt(Boarding, true, false)));
        let mut pb = takt(Boarding, true, false);
        pb.pushback_aktiv = true;
        assert!(!f.tick(Flugart::Aus, &pb));
    }

    #[test]
    fn airline_vor_alle() {
        let o = vec!["Alle".to_string(), "DLH".to_string()];
        assert_eq!(eigener_ordner("dlh", &o), Some("DLH"));
        assert_eq!(eigener_ordner("KLM", &o), Some("Alle"));
        assert_eq!(eigener_ordner("KLM", &["DLH".to_string()]), None);
        assert_eq!(eigener_ordner("", &["DLH".to_string()]), None);
    }

    /// Nr. 11, 15, G2: Lied nur für Linie, mit Haken, ohne eigene Musik.
    #[test]
    fn lied_nur_wenn_nichts_eigenes_und_haken_an() {
        assert!(lied_anfragen(Flugart::Linie, true, false));
        assert!(!lied_anfragen(Flugart::Linie, true, true), "eigene Musik");
        assert!(!lied_anfragen(Flugart::Linie, false, false), "Haken aus");
        assert!(!lied_anfragen(Flugart::Bizjet, true, false));
        assert!(!lied_anfragen(Flugart::Aus, true, false));
    }

    #[test]
    fn zufallsreihenfolge_jedes_stueck_genau_einmal_und_stabil() {
        let r = reihenfolge(7, 42);
        let mut s = r.clone();
        s.sort();
        assert_eq!(s, (0..7).collect::<Vec<_>>());
        assert_eq!(r, reihenfolge(7, 42), "gleiche Saat, gleiche Folge");
        assert!((0..20u64).any(|saat| reihenfolge(7, saat) != (0..7).collect::<Vec<_>>()), "nicht immer die Ablage-Reihenfolge");
        assert!(reihenfolge(0, 1).is_empty());
    }

    #[test]
    fn formate_und_ordnernamen() {
        assert!(format_erlaubt("Mein Lied.MP3") && format_erlaubt("a.m4a") && format_erlaubt("b.flac"));
        assert!(!format_erlaubt("bild.png") && !format_erlaubt("ohne"));
        assert!(airline_gueltig("DLH") && airline_gueltig("Alle") && airline_gueltig("4Y"));
        assert!(!airline_gueltig("../x") && !airline_gueltig("dlh") && !airline_gueltig("ABCDE"));
    }
}
