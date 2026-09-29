//! Anflug-Forensik ohne Note — Datenform (Lernpaket AP4/AP5, 29.09.2026).
//!
//! Nur die Structs, keine Rechnung (die steht im Client,
//! `src-tauri/src/anflug_forensik.rs`). Sie liegen in diesem Crate, weil
//! `storage` (LandingRecord) und `aeroacars-mqtt` (PirepPayload) beide
//! darauf zugreifen — eine Definition, eine JSON-Form. Keine Unternote,
//! kein Gate und kein Deckel liest diese Werte.

use serde::{Deserialize, Serialize};

/// Lernpaket AP4 (29.09.2026): Abweichung vom Gleitpfad der tatsaechlich
/// gelandeten Bahn, geometrisch aus Position und Hoehe — nicht mehr nur
/// Soll-V/S gegen Ist-V/S. Reine Forensik, keine Note.
///
/// Vorzeichen: **positiv = ueber dem Pfad**. Ein Dot = 0,35° × θ/3 (bei 3°
/// genau vmsACARS: 0,35°, zwei Dots = Vollausschlag 0,7°).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AnflugGleitpfad {
    /// Woher der Bezugspfad kommt: `navigraph_ils` (Bahn hat ein ILS),
    /// `navigraph_bahn` (Navigraph-Bahn ohne ILS, Winkel der Bahn) oder
    /// `angenommen_3grad` (keine Navdaten-Bahn — dann auch keine Werte).
    pub quelle: String,
    /// Verwendeter Gleitwinkel in Grad. `None` ohne Bahn — dann wurde
    /// nichts gerechnet, und kein Wert soll eine Rechnung vortaeuschen.
    #[serde(default)]
    pub winkel_deg: Option<f32>,
    /// Verwendete Schwellenueberflughoehe (TCH) in Fuss, `None` ohne Bahn.
    #[serde(default)]
    pub tch_ft: Option<f32>,
    /// `true`, wenn die Navdaten keine TCH fuehren und 50 ft angenommen
    /// wurden (der Server schreibt dann 0).
    #[serde(default)]
    pub tch_angenommen: bool,
    /// Groesse eines Dots in Grad (0,35° × θ/3), `None` ohne Bahn.
    #[serde(default)]
    pub grad_je_dot: Option<f32>,
    /// Hoehenbezug des Pfads: `navigraph` (Schwellenhoehe der Navdaten)
    /// oder `sim_boden` (Bodenhoehe des Simulators an der Schwelle, wenn
    /// sie um mehr als 20 ft abweicht oder die Navdaten keine nennen).
    #[serde(default)]
    pub hoehenbezug: Option<String>,
    /// Schwellenhoehe laut Navdaten in Fuss MSL.
    #[serde(default)]
    pub schwellenhoehe_navigraph_ft: Option<f32>,
    /// Bodenhoehe des Simulators an der Schwelle (msl − agl), Fuss MSL.
    #[serde(default)]
    pub sim_boden_ft: Option<f32>,
    /// Um so viele Fuss wurde die Schwelle der Navdaten Richtung Bahn
    /// verschoben (versetzte Schwelle, soweit nicht schon in der Geometrie).
    #[serde(default)]
    pub versatz_ft: Option<f32>,
    /// Warum es keine Werte gibt: `keine_bahn`, `schwellenhoehe_fehlt`
    /// oder `keine_proben`. `None`, wenn gerechnet wurde.
    #[serde(default)]
    pub grund_ohne_werte: Option<String>,
    /// 1000 bis 200 ft ueber der Schwelle.
    #[serde(default)]
    pub gesamt: Option<GleitpfadTor>,
    /// 1000 bis 500 ft ueber der Schwelle.
    #[serde(default)]
    pub tor_1000_500: Option<GleitpfadTor>,
    /// 500 bis 200 ft ueber der Schwelle.
    #[serde(default)]
    pub tor_500_200: Option<GleitpfadTor>,
}

/// Kennwerte der Gleitpfad-Abweichung in einem Hoehenband.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GleitpfadTor {
    /// Anzahl Proben im Band (vor der Schwelle, im Gleitwegsektor).
    pub proben: u32,
    /// Mittlere |Abweichung| in Dots.
    pub mittel_abs_dots: f32,
    /// Groesste Abweichung in Dots, mit Vorzeichen (+ = ueber dem Pfad).
    pub max_dots: f32,
    /// Hoehenabweichung DERSELBEN Probe in Fuss, mit Vorzeichen.
    pub max_abw_ft: f32,
    /// Hoechste ausgewertete Probe im Band, ft ueber der Schwelle — zeigt,
    /// ob das Band wirklich von oben an erfasst ist.
    #[serde(default)]
    pub oberste_hoehe_ft: Option<f32>,
}

/// Lernpaket AP5 (29.09.2026): Anflugruhe — nur Hinweis, keine Note.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AnflugRuhe {
    /// Bezug der Hoehenbaender: `schwelle` (Navdaten-Schwellenhoehe),
    /// `sim_boden` (Bodenhoehe des Simulators an der Schwelle) oder `platz`
    /// (Platzhoehe, wenn beides fehlt).
    pub hoehenbezug: String,
    #[serde(default)]
    pub tor_1000_500: Option<RuheTor>,
    #[serde(default)]
    pub tor_500_200: Option<RuheTor>,
}

/// Kennwerte der Anflugruhe in einem Hoehenband. Jeder Wert ist `None`,
/// wenn er sich in diesem Band nicht verlaesslich messen liess.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RuheTor {
    pub proben: u32,
    /// Zeit zwischen erster und letzter Probe im Band, Sekunden.
    pub dauer_s: f32,
    /// Hoechste Probe im Band, ft ueber dem Bezug.
    #[serde(default)]
    pub oberste_hoehe_ft: Option<f32>,
    /// Wie oft die Pfadabweichung die Seite gewechselt hat (ueber/unter
    /// dem Pfad), mit Totband ±0,1 Dot. `None` ohne Gleitpfad (AP4).
    #[serde(default)]
    pub pfad_vorzeichenwechsel: Option<u32>,
    /// Streuung der Nickrate in °/s.
    #[serde(default)]
    pub nick_unruhe_deg_s: Option<f32>,
    /// Streuung der Rollrate in °/s.
    #[serde(default)]
    pub roll_unruhe_deg_s: Option<f32>,
    /// Richtungswechsel des Schubs (mittleres N1) je Minute, Totband 2 %.
    #[serde(default)]
    pub schub_umkehr_pro_min: Option<f32>,
    /// Warum `schub_umkehr_pro_min` fehlt: `kein_n1` (Simulator liefert
    /// kein N1 — X-Plane, Kolben) oder `zu_kurz` (N1 da, aber < 5 Proben
    /// oder < 10 s im Band).
    #[serde(default)]
    pub schub_grund: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tor() -> GleitpfadTor {
        GleitpfadTor {
            proben: 88,
            mittel_abs_dots: 0.42,
            max_dots: -1.27,
            max_abw_ft: -41.4,
            oberste_hoehe_ft: Some(996.0),
        }
    }

    fn ruhe_tor() -> RuheTor {
        RuheTor {
            proben: 57,
            dauer_s: 56.3,
            oberste_hoehe_ft: Some(996.0),
            pfad_vorzeichenwechsel: Some(3),
            nick_unruhe_deg_s: Some(0.84),
            roll_unruhe_deg_s: Some(1.26),
            schub_umkehr_pro_min: Some(4.27),
            schub_grund: None,
        }
    }

    /// Beide Bloecke gehen in den PIREP-Payload (Entscheid Thomas
    /// 29.09.2026). Der MQTT-Touchdown liegt knapp unter 10 KB; der PIREP
    /// hat mehr Luft, trotzdem gilt fuer diese Forensik ein eigenes Budget:
    /// voll befuellt unter 1 KB. Wer ein Feld ergaenzt, sieht hier, was es
    /// kostet. Die Werte sind so gerundet, wie der Client sie ablegt
    /// (`anflug_forensik.rs`: Dots/Raten 0,01, Fuss/Sekunden 0,1, Hoehen
    /// ganze Fuss) — ungerundete f32 kosteten rund 80 Bytes mehr.
    #[test]
    fn voll_befuellt_bleiben_beide_bloecke_unter_einem_kilobyte() {
        let g = AnflugGleitpfad {
            quelle: "navigraph_ils".into(),
            winkel_deg: Some(3.0),
            tch_ft: Some(50.0),
            tch_angenommen: false,
            grad_je_dot: Some(0.35),
            hoehenbezug: Some("sim_boden".into()),
            schwellenhoehe_navigraph_ft: Some(1487.0),
            sim_boden_ft: Some(1_519.4),
            versatz_ft: Some(0.0),
            grund_ohne_werte: None,
            gesamt: Some(tor()),
            tor_1000_500: Some(tor()),
            tor_500_200: Some(tor()),
        };
        let r = AnflugRuhe {
            hoehenbezug: "sim_boden".into(),
            tor_1000_500: Some(ruhe_tor()),
            tor_500_200: Some(ruhe_tor()),
        };
        let g_len = serde_json::to_string(&g).unwrap().len();
        let r_len = serde_json::to_string(&r).unwrap().len();
        println!("anflug_gleitpfad {g_len} B, anflug_ruhe {r_len} B");
        assert!(g_len + r_len < 1024, "{g_len} + {r_len} Bytes");
    }
}
