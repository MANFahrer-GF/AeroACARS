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
