//! Liegt ein sichtbares Fenster ueber dem Simulator? (Befund 6)
//!
//! Der `BitBlt`-Rueckfall der Fensteraufnahme kopiert das **Bildschirmrechteck**
//! des Simulators, also auch alles, was darueber liegt (Mail, Chat, Passwort).
//! Er darf deshalb nur laufen, wenn kein sichtbares Fenster in der
//! Z-Reihenfolge ueber dem Simulator dessen Rechteck schneidet. Die Windows-
//! Teile sammeln die Fenster ueber dem Simulator (Kette `GW_HWNDPREV`) und
//! lassen weg, was nicht zaehlt (unsichtbar, minimiert, vollstaendig
//! durchsichtig, weggeblendet, eigene Fenster des Simulators); die Entscheidung
//! selbst ist diese reine Funktion.

/// Rechteck in Bildschirmkoordinaten; `rechts`/`unten` sind exklusiv (wie `RECT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rechteck {
    pub links: i32,
    pub oben: i32,
    pub rechts: i32,
    pub unten: i32,
}

impl Rechteck {
    pub fn neu(links: i32, oben: i32, rechts: i32, unten: i32) -> Self {
        Self { links, oben, rechts, unten }
    }

    pub fn leer(&self) -> bool {
        self.rechts <= self.links || self.unten <= self.oben
    }

    /// Haben beide eine gemeinsame Flaeche (blosses Beruehren zaehlt nicht)?
    pub fn schneidet(&self, anderes: &Rechteck) -> bool {
        !self.leer() && !anderes.leer() && self.links < anderes.rechts && anderes.links < self.rechts && self.oben < anderes.unten && anderes.oben < self.unten
    }
}

/// Schneidet irgendeines der Fenster, die ueber dem Simulator liegen, dessen Rechteck?
pub fn verdeckt(simulator: &Rechteck, darueber: &[Rechteck]) -> bool {
    darueber.iter().any(|r| simulator.schneidet(r))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIM: Rechteck = Rechteck { links: 0, oben: 0, rechts: 2560, unten: 1440 };

    #[test]
    fn nichts_darueber_ist_frei() {
        assert!(!verdeckt(&SIM, &[]));
    }

    #[test]
    fn eine_ecke_genuegt_auch_wenn_die_mitte_frei_bleibt() {
        // Das war die Luecke der alten Mittelpunktpruefung.
        let mail = Rechteck::neu(2400, 1300, 3000, 1600);
        assert!(!mail.schneidet(&Rechteck::neu(1280, 720, 1281, 721)));
        assert!(verdeckt(&SIM, &[mail]));
    }

    #[test]
    fn fenster_daneben_oder_nur_beruehrend_verdeckt_nicht() {
        assert!(!verdeckt(&SIM, &[Rechteck::neu(2560, 0, 3000, 1440)]), "grenzt rechts an");
        assert!(!verdeckt(&SIM, &[Rechteck::neu(-500, 0, 0, 1440)]), "grenzt links an");
        assert!(!verdeckt(&SIM, &[Rechteck::neu(0, 1440, 2560, 1500)]), "grenzt unten an");
        assert!(!verdeckt(&SIM, &[Rechteck::neu(3000, 0, 4000, 500)]), "anderer Monitor");
    }

    #[test]
    fn leere_rechtecke_zaehlen_nicht_ein_grosses_darueber_verdeckt_alles() {
        assert!(!verdeckt(&SIM, &[Rechteck::neu(100, 100, 100, 900)]));
        assert!(verdeckt(&SIM, &[Rechteck::neu(-10, -10, 5000, 5000)]));
        assert!(!SIM.schneidet(&Rechteck::neu(10, 10, 5, 5)));
    }

    #[test]
    fn ein_treffer_unter_mehreren_reicht() {
        let alle = [Rechteck::neu(5000, 0, 6000, 100), Rechteck::neu(10, 10, 20, 20), Rechteck::neu(9000, 0, 9100, 100)];
        assert!(verdeckt(&SIM, &alle));
        assert!(!verdeckt(&SIM, &[alle[0], alle[2]]));
    }
}
