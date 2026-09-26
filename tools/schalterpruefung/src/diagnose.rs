//! Lesewege und Diagnose — plattformunabhängig.
//!
//! Befund aus den echten Läufen vom 26.09. (A380 13:13, A350 13:46): Keine
//! einzige der ~2000–2400 über MobiFlight-Zusatzclients gelesenen INI_-LVars
//! hat sich je bewegt — auch nicht `L:INI_LIGHTS_STROBE`, die das AAO-Skript
//! des Piloten nachweislich schreibt und auf die das Flugzeug im selben
//! Messschritt reagierte (B:AIRLINER_LIGHTS_EXT_STROBE, A:LIGHT STROBE).
//! Alle Werte wurden GELIEFERT (Ruhemessung: 0 nicht geliefert), sie blieben
//! nur konstant. Der MobiFlight-Leseweg ist für diese Namen also unbrauchbar;
//! die genaue Ursache im Modul ließ sich aus dem Quelltext allein nicht
//! belegen (siehe `sim.rs`, Kopf von `lvars_direkt`).
//!
//! Neuer Leseweg: ALLE LVars direkt per SimConnect-Datendefinition
//! (`AddToDataDefinition("L:<Name>", "Number")`) — derselbe Weg, auf dem der
//! AeroACARS-Client seit Langem INI_-LVars liest
//! (`client/src-tauri/crates/sim-msfs/src/adapter/telemetry.rs`, z. B.
//! `L:INI_ap1_on`). MobiFlight liefert nur noch die Namensliste und liest zur
//! Gegenprobe eine kleine Stichprobe mit („MF:L:…"), damit der nächste Lauf
//! zeigt, welcher Weg lebt.

use serde::Serialize;

use crate::auswertung::Werte;

/// Diese vier werden in jedem Lauf zu Beginn ausgelesen (beide Wege).
pub const STICHPROBE: &[&str] = &[
    "INI_LIGHTS_STROBE",
    "INI_SEATBELTS_SWITCH",
    "INI_TCAS_ON_CPT",
    "INI_APU_MASTER_SWITCH",
];

/// Präfix für Variablen, die über MobiFlight gelesen werden (Gegenprobe).
pub const MF_PRAEFIX: &str = "MF:L:";

#[derive(Debug, Clone, PartialEq)]
pub struct Lesewege {
    /// Direkt per SimConnect: MobiFlight-Liste ∪ Zusatzliste ∪ Stichprobe.
    pub direkt: Vec<String>,
    /// Gegenprobe über MobiFlight: Stichprobe + die ersten Namen der
    /// MobiFlight-Liste (die dort nachweislich funktionierten).
    pub mf_gegenprobe: Vec<String>,
    /// Zusatz-Namen, die nicht schon in der MobiFlight-Liste standen.
    pub zusatz_neu: usize,
}

pub fn lesewege(mf_liste: &[String], zusatz: &[String]) -> Lesewege {
    let stichprobe: Vec<String> = STICHPROBE.iter().map(|s| s.to_string()).collect();
    let (mit_zusatz, zusatz_neu) = crate::mobiflight::zusammenfuehren(mf_liste, zusatz);
    let (direkt, _) = crate::mobiflight::zusammenfuehren(&mit_zusatz, &stichprobe);
    let mut mf_gegenprobe = stichprobe;
    mf_gegenprobe.extend(mf_liste.iter().take(10).cloned());
    Lesewege {
        direkt,
        mf_gegenprobe,
        zusatz_neu,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GruppenDiagnose {
    pub gruppe: String,
    pub abonniert: usize,
    pub geliefert: usize,
    pub ungleich_null: usize,
    /// Viele Variablen geliefert, aber ALLE 0 — das Muster des Fehlers
    /// vom 26.09.
    pub verdaechtig: bool,
}

/// Je Gruppe (in Messreihenfolge, `(Name, Anzahl)`): wie viele Variablen
/// geliefert wurden und wie viele davon ≠ 0 waren.
pub fn gruppen(gruppen: &[(String, usize)], werte: &Werte) -> Vec<GruppenDiagnose> {
    let mut start = 0;
    gruppen
        .iter()
        .map(|(name, n)| {
            let teil = werte.get(start..start + n).unwrap_or(&[]);
            start += n;
            let geliefert = teil.iter().filter(|w| w.is_some()).count();
            let ungleich_null = teil
                .iter()
                .filter(|w| matches!(w, Some(x) if *x != 0.0))
                .count();
            GruppenDiagnose {
                gruppe: name.clone(),
                abonniert: *n,
                geliefert,
                ungleich_null,
                verdaechtig: geliefert >= 50 && ungleich_null == 0,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Probe {
    pub variable: String,
    pub wert: Option<f64>,
}

/// Anfangswerte der Stichprobe auf beiden Wegen.
pub fn stichprobe(variablen: &[String], werte: &Werte) -> Vec<Probe> {
    let mut v = Vec::new();
    for n in STICHPROBE {
        for name in [format!("L:{n}"), format!("{MF_PRAEFIX}{n}")] {
            let wert = variablen
                .iter()
                .position(|x| *x == name)
                .and_then(|i| werte.get(i).copied().flatten());
            v.push(Probe {
                variable: name,
                wert,
            });
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skripte;

    /// Rot gegen den Stand vor diesem Fix: Dort liefen alle Zusatz-Namen
    /// über MobiFlight-Clients (`zusammenfuehren` → `abonnieren`), und genau
    /// dieser Weg lieferte im echten Lauf nur konstante Werte. Jetzt muss jede
    /// Zusatz-LVar — insbesondere L:INI_LIGHTS_STROBE — direkt gelesen werden.
    #[test]
    fn zusatzliste_laeuft_direkt_nicht_ueber_mobiflight() {
        let mf: Vec<String> = (0..1000).map(|i| format!("p42_var_{i}")).collect();
        let zusatz = skripte::zusatzliste("a380");
        let w = lesewege(&mf, &zusatz);
        assert!(w.direkt.iter().any(|n| n == "INI_LIGHTS_STROBE"));
        for z in &zusatz {
            assert!(w.direkt.contains(z), "{z} fehlt im Direktweg");
        }
        // MobiFlight nur noch Gegenprobe: klein, Stichprobe vorn.
        assert!(w.mf_gegenprobe.len() <= 14);
        assert_eq!(w.mf_gegenprobe[0], "INI_LIGHTS_STROBE");
        assert_eq!(w.zusatz_neu, zusatz.len());
        // Stichprobe ist immer direkt dabei, auch beim allgemeinen Skript.
        let g = lesewege(&mf, &[]);
        for s in STICHPROBE {
            assert!(g.direkt.iter().any(|n| n == s));
        }
        assert_eq!(g.direkt.len(), 1004);
    }

    #[test]
    fn gruppendiagnose_erkennt_das_muster_vom_26_09() {
        // Gruppe 1 lebt (p42 zappelt), Gruppe 2: 150 geliefert, alle 0.
        let mut w: Werte = vec![Some(0.0); 150];
        w[3] = Some(1.0);
        w.extend(vec![Some(0.0); 150]);
        w.extend(vec![None; 5]);
        let g = gruppen(
            &[("SP7".into(), 150), ("SP8".into(), 150), ("leer".into(), 5)],
            &w,
        );
        assert_eq!(g[0].ungleich_null, 1);
        assert!(!g[0].verdaechtig);
        assert!(g[1].verdaechtig);
        assert_eq!(
            (g[1].abonniert, g[1].geliefert, g[1].ungleich_null),
            (150, 150, 0)
        );
        assert_eq!((g[2].geliefert, g[2].verdaechtig), (0, false));
    }

    #[test]
    fn stichprobe_beide_wege() {
        let v: Vec<String> = vec![
            "L:INI_LIGHTS_STROBE".into(),
            "MF:L:INI_LIGHTS_STROBE".into(),
            "L:INI_APU_MASTER_SWITCH".into(),
        ];
        let w: Werte = vec![Some(1.0), Some(0.0), Some(0.0)];
        let p = stichprobe(&v, &w);
        assert_eq!(p.len(), 8);
        assert_eq!(
            p[0],
            Probe {
                variable: "L:INI_LIGHTS_STROBE".into(),
                wert: Some(1.0)
            }
        );
        assert_eq!(p[1].wert, Some(0.0));
        assert_eq!(p[2].wert, None); // SEATBELTS nicht beobachtet
    }
}
