//! v3 (Etappe 1b): Fachliche Grenzwerte aus dem Laufzeit-Paket (Pflichtenheft
//! P-3.55, P-3.65).
//!
//! Die Zahlen stehen nur in `bundle/grenzwerte/<bereich>.json`, nie im
//! Rust-Code. Je Bereich eine Struktur ohne Vorgabewerte und mit
//! `deny_unknown_fields`: Ein falscher Feldname ist ein Compile-Fehler, ein
//! fehlendes oder fremdes Feld im Paket ein Lesefehler — dann gilt der ganze
//! eingebaute Stand (fail-closed, P-3.21). Neue Felder brauchen eine neue
//! Faehigkeit `grenzwerte-n` (P-2.15).

use std::sync::OnceLock;

use serde::Deserialize;

/// Ein Wert mit Einheit und Herkunft im Code von v2.1.24 (P-3.55). Gelesen
/// wird nur `wert`; `einheit` und `quelle` sind Pflicht, damit jede Zahl
/// nachvollziehbar bleibt.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Grenzwert<T> {
    pub wert: T,
    #[allow(dead_code)]
    pub einheit: String,
    #[allow(dead_code)]
    pub quelle: String,
}

/// Stand- und Rollweg-Erkennung (`stands.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Staende {
    pub stand_capture_radius_m: Grenzwert<f64>,
    pub stand_name_radius_m: Grenzwert<f64>,
    pub stand_name_vorsprung_m: Grenzwert<f64>,
    pub rollweg_mittellinie_m: Grenzwert<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct Grenzwerte {
    pub staende: Staende,
}

/// Bereich -> Datei im Paket.
pub(crate) const DATEIEN: &[&str] = &["grenzwerte/staende.json"];

static GRENZWERTE: OnceLock<Grenzwerte> = OnceLock::new();

/// Die geltenden Grenzwerte. Ohne Laufzeit-Paket (z. B. in Tests) die
/// eingebauten.
pub(crate) fn grenzwerte() -> &'static Grenzwerte {
    GRENZWERTE.get_or_init(|| {
        lesen(|p| crate::laufzeit_paket::eingebaute_datei(p).map(<[u8]>::to_vec))
            .expect("eingebaute Grenzwerte lesbar (Test grenzwerte_eingebaut_lesbar)")
    })
}

/// Setzt die Grenzwerte aus dem Laufzeit-Paket. `false`, wenn schon welche
/// gelesen wurden.
pub(crate) fn setzen(g: Grenzwerte) -> bool {
    GRENZWERTE.set(g).is_ok()
}

/// Liest alle Bereiche ueber `datei(pfad)`.
pub(crate) fn lesen(datei: impl Fn(&str) -> Option<Vec<u8>>) -> Result<Grenzwerte, String> {
    fn bereich<T: serde::de::DeserializeOwned>(
        datei: &impl Fn(&str) -> Option<Vec<u8>>,
        pfad: &str,
    ) -> Result<T, String> {
        let bytes = datei(pfad).ok_or_else(|| format!("{pfad} fehlt"))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{pfad}: {e}"))
    }
    Ok(Grenzwerte { staende: bereich(&datei, "grenzwerte/staende.json")? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grenzwerte_eingebaut_lesbar() {
        lesen(|p| crate::laufzeit_paket::eingebaute_datei(p).map(<[u8]>::to_vec)).unwrap();
    }

    /// Die Tabelle traegt dieselben Werte wie die Konstanten in v2.1.24
    /// (Fundstellen in `bundle/grenzwerte/staende.json`). Eine bewusste
    /// Aenderung passt diesen Test im selben Commit an.
    #[test]
    fn grenzwerte_staende_wie_2_1_24() {
        let s = &grenzwerte().staende;
        assert_eq!(s.stand_capture_radius_m.wert, 60.0); // stands.rs:51
        assert_eq!(s.stand_name_radius_m.wert, 60.0); // stands.rs:64
        assert_eq!(s.stand_name_vorsprung_m.wert, 10.0); // stands.rs:69
        assert_eq!(s.rollweg_mittellinie_m.wert, 3.0); // stands.rs:326
    }

    #[test]
    fn fremdes_oder_fehlendes_feld_wird_abgelehnt() {
        let echt = crate::laufzeit_paket::eingebaute_datei("grenzwerte/staende.json").unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(echt).unwrap();
        v["tippfehler_radius_m"] = serde_json::json!({"wert": 1.0, "einheit": "m", "quelle": "x"});
        let fremd = serde_json::to_vec(&v).unwrap();
        assert!(lesen(|_| Some(fremd.clone())).is_err());
        let mut v: serde_json::Value = serde_json::from_slice(echt).unwrap();
        v.as_object_mut().unwrap().remove("stand_name_radius_m");
        let fehlt = serde_json::to_vec(&v).unwrap();
        assert!(lesen(|_| Some(fehlt.clone())).is_err());
    }
}
