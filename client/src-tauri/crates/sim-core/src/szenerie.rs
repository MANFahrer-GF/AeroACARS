//! Was ein Flughafen in der Szenerie des Simulators hergibt.
//!
//! # Warum die Typen hier stehen
//!
//! Beide Simulatoren liefern dieselbe Sache aus verschiedenen Quellen:
//! X-Plane aus der installierten `apt.dat`, MSFS über die
//! SimConnect-Facility-Schnittstelle. Die **Auswertung** danach ist
//! identisch — Kurs, Breite, Länge, Schwellen, Belag in die Navdaten
//! übernehmen, mit denselben Riegeln.
//!
//! Stünden die Typen in einem der beiden Adapter, müsste der andere von
//! ihm abhängen. Also stehen sie hier, wo beide ohnehin hinschauen.
//!
//! ⚠ **Koordinaten sind immer (Breite, Länge).** Am 28.08.2026 stand im
//! X-Plane-Leser einmal (Länge, Breite) für Rollwege und (Breite, Länge)
//! für Bahnen; der Abnehmer verwarf daraufhin 75.610 von 86.674 Bahnen
//! als „liegt woanders". Ein vertauschtes Paar sieht wie eine gültige
//! Koordinate aus — nur die Reihenfolge festzuschreiben hilft.

/// Ein Bahnende, so wie die Szenerie es beschreibt.
#[derive(Debug, Clone, PartialEq)]
pub struct SzenerieBahn {
    /// Bezeichner dieses Endes, etwa `"27R"`.
    pub bezeichner: String,
    /// Wahrer Kurs in Grad, in Landerichtung dieses Endes.
    pub kurs_grad: f64,
    /// Breite der befestigten Fläche in Metern.
    pub breite_m: f64,
    /// Länge zwischen den beiden Schwellen in Metern.
    pub laenge_m: f64,
    /// Versetzte Schwelle an diesem Ende, in Metern.
    pub versetzte_schwelle_m: f64,
    /// Koordinaten dieses Endes, **(Breite, Länge)**.
    pub schwelle: (f64, f64),
    /// Koordinaten des gegenüberliegenden Endes, **(Breite, Länge)**.
    pub gegenende: (f64, f64),
    /// Belagsschlüssel im Format der X-Plane-`apt.dat`
    /// (1 = Asphalt, 2 = Beton, 3 = Gras, …).
    ///
    /// MSFS liefert eine eigene Aufzählung; ihr Adapter rechnet sie auf
    /// diese Schlüssel um, damit die Auswertung eine Sprache spricht.
    pub belag_code: u8,
}

/// Ein benanntes Rollwegstück.
#[derive(Debug, Clone, PartialEq)]
pub struct SzenerieRollweg {
    pub name: String,
    /// **(Breite, Länge)** je Punkt.
    pub punkte: Vec<(f64, f64)>,
}

/// Was ein Flughafen in der Szenerie hergibt.
#[derive(Debug, Clone, Default)]
pub struct SzenerieFlughafen {
    pub icao: String,
    pub bahnen: Vec<SzenerieBahn>,
    pub rollwege: Vec<SzenerieRollweg>,
    /// Woher die Angaben stammen — Dateipfad bei X-Plane, `"msfs"` bei
    /// MSFS. Steht im Bericht und in der Fehlersuche: Ein Add-on-Platz
    /// sieht anders aus als der globale.
    pub quelle: String,
}

/// Wie weit die Szenerie-Abfrage gekommen ist.
///
/// Bewusst ein eigener Typ statt eines `bool`: Die drei Fehlerfaelle
/// verlangen verschiedene Antworten. "Abgelehnt" heisst, der Simulator
/// kennt die Abfrage nicht (falsche Fassung?), "keine Antwort" heisst,
/// sie kam nicht rechtzeitig, "ohne Bahnen" heisst, der Platz war leer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SzenerieDiagnose {
    /// Es wurde nie gefragt — kein Ziel bekannt, oder kein MSFS.
    #[default]
    NichtAngefordert,
    /// Anfrage gestellt, Antwort steht noch aus.
    Angefordert,
    /// SimConnect hat die Anfrage zurueckgewiesen (Grund im Text).
    Abgelehnt(String),
    /// Vollstaendige Lieferung eingetroffen.
    Geliefert {
        icao: String,
        bahnen: usize,
        rollwege: usize,
    },
}

impl SzenerieDiagnose {
    /// Kurzwort fuer den Flug — muss ohne Erklaerung lesbar sein.
    ///
    /// ⚠ Bei einer Lieferung stehen die ZAHLEN dabei. Der erste Entwurf
    /// hat sie weggekuerzt, und genau daran blieb die Untersuchung am
    /// 29.08.2026 haengen: Zwei Fluege mit MSFS 2024 meldeten
    /// `ohne_bahnen` — die Antwort kam also an, aber ohne eine einzige
    /// Bahn. Ob dabei ROLLWEGE ankamen, haette entschieden, wo der
    /// Fehler sitzt: Kommen sie, funktionieren Untersaetze grundsaetzlich
    /// und nur das Bahn-Raster ist falsch. Kommen sie nicht, scheitert
    /// jeder Untersatz. Die Zahl fehlte, also war es nicht zu sagen.
    ///
    /// Eine Diagnose, die eine Stufe zu grob ist, beantwortet die Frage
    /// bis kurz vor dem Ziel.
    pub fn kurz(&self) -> String {
        match self {
            Self::NichtAngefordert => "nicht_angefordert".to_string(),
            Self::Angefordert => "keine_antwort".to_string(),
            Self::Abgelehnt(_) => "abgelehnt".to_string(),
            Self::Geliefert {
                bahnen: 0,
                rollwege,
                ..
            } => format!("ohne_bahnen(rollwege={rollwege})"),
            Self::Geliefert {
                bahnen, rollwege, ..
            } => format!("geliefert(bahnen={bahnen},rollwege={rollwege})"),
        }
    }
}

#[cfg(test)]
mod szenerie_diagnose_tests {
    use super::SzenerieDiagnose;

    #[test]
    fn eine_lieferung_nennt_ihre_zahlen() {
        // ⚠ Ohne die Rollwegzahl bleibt offen, ob nur das Bahn-Raster
        // falsch ist oder jeder Untersatz scheitert. Genau daran blieb
        // die Untersuchung am 29.08.2026 haengen.
        let d = SzenerieDiagnose::Geliefert {
            icao: "LKTB".to_string(),
            bahnen: 0,
            rollwege: 243,
        };
        let s = d.kurz();
        assert!(s.starts_with("ohne_bahnen"), "{s}");
        assert!(s.contains("243"), "die Rollwegzahl fehlt: {s}");
    }

    #[test]
    fn eine_vollstaendige_lieferung_nennt_beide_zahlen() {
        let d = SzenerieDiagnose::Geliefert {
            icao: "EDDH".to_string(),
            bahnen: 4,
            rollwege: 118,
        };
        let s = d.kurz();
        assert!(s.contains("bahnen=4"), "{s}");
        assert!(s.contains("rollwege=118"), "{s}");
    }

    #[test]
    fn die_stummen_faelle_bleiben_einzelne_woerter() {
        // Sie tragen keine Zahlen — und duerfen auch keine erfinden.
        assert_eq!(SzenerieDiagnose::NichtAngefordert.kurz(), "nicht_angefordert");
        assert_eq!(SzenerieDiagnose::Angefordert.kurz(), "keine_antwort");
        assert_eq!(
            SzenerieDiagnose::Abgelehnt("egal".to_string()).kurz(),
            "abgelehnt"
        );
    }
}
