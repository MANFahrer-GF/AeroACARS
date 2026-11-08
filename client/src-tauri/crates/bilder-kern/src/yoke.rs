//! Yoke- oder Joystick-Knopf als Ausloeser von `bild_aufnehmen` (P-11.30/P-11.33).
//!
//! Hier liegt nur die reine Logik ohne Windows-Aufrufe: der gemerkte Knopf,
//! die Flankenerkennung samt Sperre gegen Prellen, das Wiederfinden des Geraets
//! und das Auslesen des Auftrags aus `bild_quellen_setzen`. Die Abfrage selbst
//! (`winmm`) steht in `plattform/windows.rs`.
//!
//! Die Schnittstelle `winmm` sieht je Geraet hoechstens 32 Knoepfe (Maske
//! `dwButtons`); Knoepfe darueber und Hat-Schalter sind so nicht erreichbar.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Fehler;

/// Hoechstzahl der Knoepfe, die `winmm` je Geraet liefert.
pub const MAX_KNOEPFE: u8 = 32;
/// Mindestabstand zweier Ausloesungen (Prellen, Dauerdruck).
pub const SPERRE_MS: u64 = 1000;
/// Abfragetakt der Knopfpruefung (20 Hz).
pub const TAKT_MS: u64 = 50;

/// Der gemerkte Knopf. Das Geraet wird ueber Hersteller, Produkt und Name
/// gefunden, nie ueber die Joystick-Nummer (die wechselt beim Abstecken).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YokeTaste {
    pub geraet_name: String,
    pub hersteller_id: u16,
    pub produkt_id: u16,
    /// 1 bis [`MAX_KNOEPFE`], wie in der Windows-Spielgeraete-Anzeige.
    pub knopf: u8,
}

impl YokeTaste {
    pub fn pruefen(&self) -> Result<(), Fehler> {
        if !(1..=MAX_KNOEPFE).contains(&self.knopf) {
            return Err(Fehler::Eingabe(format!("Yoke-Knopf {} ausserhalb 1 bis {MAX_KNOEPFE}", self.knopf)));
        }
        if self.geraet_name.is_empty() || self.geraet_name.chars().count() > 64 {
            return Err(Fehler::Eingabe("Yoke-Geraetename leer oder zu lang".into()));
        }
        Ok(())
    }
}

/// Ein angeschlossenes Geraet, wie `winmm` es meldet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Geraet {
    pub nummer: u32,
    pub name: String,
    pub hersteller_id: u16,
    pub produkt_id: u16,
}

/// Die Joystick-Nummer des gemerkten Geraets unter den jetzt angeschlossenen:
/// Hersteller, Produkt **und** Name muessen stimmen; bei gleichen Geraeten das erste.
pub fn geraet_finden(geraete: &[Geraet], taste: &YokeTaste) -> Option<u32> {
    geraete
        .iter()
        .find(|g| g.hersteller_id == taste.hersteller_id && g.produkt_id == taste.produkt_id && g.name == taste.geraet_name)
        .map(|g| g.nummer)
}

/// Merkt das letzte Suchergebnis: der Sekundentakt schreibt nur Wechsel ins Protokoll.
#[derive(Debug, Default)]
pub struct SuchStand {
    letztes: Option<Option<u32>>,
}

impl SuchStand {
    pub fn geaendert(&mut self, treffer: Option<u32>) -> bool {
        let neu = self.letztes != Some(treffer);
        self.letztes = Some(treffer);
        neu
    }
}

/// Protokollzeile zur Suche; beim Nichtfund mit allen Geraeten, die Windows gerade meldet.
pub fn suchbericht(taste: &YokeTaste, treffer: Option<u32>, geraete: &[Geraet]) -> String {
    let gemerkt = format!("{:?} ({:04X}:{:04X}) Knopf {}", taste.geraet_name, taste.hersteller_id, taste.produkt_id, taste.knopf);
    match treffer {
        Some(n) => format!("Yoke-Knopf: Geraet gefunden unter Nummer {n}: {gemerkt}"),
        None => {
            let da: Vec<String> = geraete.iter().map(|g| format!("{}: {:?} ({:04X}:{:04X})", g.nummer, g.name, g.hersteller_id, g.produkt_id)).collect();
            format!("Yoke-Knopf: Geraet nicht gefunden: {gemerkt}; angeschlossen: [{}]", da.join(", "))
        }
    }
}

/// Ist der Knopf (1-basiert) in der Maske gedrueckt?
pub fn knopf_gedrueckt(maske: u32, knopf: u8) -> bool {
    (1..=MAX_KNOEPFE).contains(&knopf) && maske & (1u32 << (knopf - 1)) != 0
}

/// Registry-Schluessel (HKCU) mit dem wirklichen Geraetenamen; `winmm` nennt nur den Treiber.
pub fn oem_schluessel(hersteller_id: u16, produkt_id: u16) -> String {
    format!(r"System\CurrentControlSet\Control\MediaProperties\PrivateProperties\Joystick\OEM\VID_{hersteller_id:04X}&PID_{produkt_id:04X}")
}

/// Anzeigename: der Registry-Name, sonst der von `winmm`; passend zur Grenze aus [`YokeTaste::pruefen`].
pub fn name_waehlen(oem: Option<&str>, winmm: &str) -> String {
    let n = oem.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(winmm).trim();
    n.chars().take(64).collect()
}

/// Der niedrigste Knopf, der jetzt gedrueckt ist und vorher nicht (1-basiert).
pub fn neuer_knopf(vorher: u32, jetzt: u32) -> Option<u8> {
    let neu = jetzt & !vorher;
    (neu != 0).then(|| neu.trailing_zeros() as u8 + 1)
}

/// Flankenerkennung mit Sperre. Ein Aufruf je Abfrage mit dem Zustand des Knopfes
/// (`None` = Geraet gerade nicht lesbar/abgesteckt). Loest nur beim Wechsel von
/// „nicht gedrueckt“ auf „gedrueckt“ aus, hoechstens einmal je [`SPERRE_MS`].
/// Der erste lesbare Zustand nach dem Start oder nach dem Wiederfinden gilt nur
/// als Ausgangslage: ein schon gehaltener Knopf loest nichts aus.
#[derive(Debug, Default)]
pub struct Flanke {
    vorher: Option<bool>,
    letzte_ausloesung_ms: Option<u64>,
}

impl Flanke {
    pub fn pruefen(&mut self, jetzt_ms: u64, gedrueckt: Option<bool>) -> bool {
        let Some(g) = gedrueckt else {
            self.vorher = None; // Geraet weg: beim Wiederfinden neu ansetzen
            return false;
        };
        let flanke = g && self.vorher == Some(false);
        self.vorher = Some(g);
        if !flanke {
            return false;
        }
        if self.letzte_ausloesung_ms.is_some_and(|t| jetzt_ms.saturating_sub(t) < SPERRE_MS) {
            return false;
        }
        self.letzte_ausloesung_ms = Some(jetzt_ms);
        true
    }
}

/// Liest `yokeTaste` aus dem Rohauftrag von `bild_quellen_setzen`: Schluessel fehlt =
/// `None` (unveraendert), `null` = `Some(None)` (loeschen), Objekt = `Some(Some(..))`.
/// (Tauri reicht fehlend und `null` bei `Option` gleich als `None` weiter; deshalb
/// liest der Befehl den Rohauftrag.)
pub fn aus_auftrag(body: &Value) -> Result<Option<Option<YokeTaste>>, Fehler> {
    let Some(v) = body.get("yokeTaste") else { return Ok(None) };
    if v.is_null() {
        return Ok(Some(None));
    }
    let t: YokeTaste = serde_json::from_value(v.clone()).map_err(|e| Fehler::Eingabe(format!("yokeTaste nicht lesbar: {e}")))?;
    t.pruefen()?;
    Ok(Some(Some(t)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn taste() -> YokeTaste {
        YokeTaste { geraet_name: "Honeycomb Alpha Flight Controls".into(), hersteller_id: 0x294B, produkt_id: 0x1900, knopf: 5 }
    }

    fn g(nummer: u32, name: &str, h: u16, p: u16) -> Geraet {
        Geraet { nummer, name: name.into(), hersteller_id: h, produkt_id: p }
    }

    #[test]
    fn flanke_loest_einmal_beim_druecken_aus() {
        let mut f = Flanke::default();
        assert!(!f.pruefen(0, Some(false)));
        assert!(f.pruefen(50, Some(true)), "Druecken");
        assert!(!f.pruefen(100, Some(true)), "Halten");
        assert!(!f.pruefen(150, Some(false)), "Loslassen");
    }

    #[test]
    fn sperre_haelt_prellen_ab_und_gibt_nach_einer_sekunde_frei() {
        let mut f = Flanke::default();
        f.pruefen(0, Some(false));
        assert!(f.pruefen(50, Some(true)));
        assert!(!f.pruefen(100, Some(false)));
        assert!(!f.pruefen(150, Some(true)), "Prellen innerhalb 1 s");
        assert!(!f.pruefen(200, Some(false)));
        assert!(!f.pruefen(1049, Some(true)), "999 ms nach der Ausloesung");
        assert!(!f.pruefen(1050, Some(false)));
        assert!(f.pruefen(1100, Some(true)), "nach 1 s wieder frei");
    }

    #[test]
    fn schon_gehaltener_knopf_und_abgestecktes_geraet() {
        let mut f = Flanke::default();
        assert!(!f.pruefen(0, Some(true)), "beim Start schon gehalten: nur Ausgangslage");
        assert!(!f.pruefen(50, Some(false)));
        assert!(f.pruefen(100, Some(true)));
        // abgesteckt (nicht lesbar) blockiert nichts und loest nichts aus
        assert!(!f.pruefen(2000, None));
        assert!(!f.pruefen(2050, None));
        // Wiederfinden mit gehaltenem Knopf: nur Ausgangslage
        assert!(!f.pruefen(3000, Some(true)));
        assert!(!f.pruefen(3050, Some(false)));
        assert!(f.pruefen(3100, Some(true)), "danach geht es wieder");
    }

    #[test]
    fn geraet_nur_ueber_hersteller_produkt_und_name() {
        let t = taste();
        let liste = [g(0, "Anderes", 1, 2), g(3, &t.geraet_name, t.hersteller_id, t.produkt_id)];
        assert_eq!(geraet_finden(&liste, &t), Some(3), "Nummer ist egal, sie wechselt");
        let umgesteckt = [g(1, &t.geraet_name, t.hersteller_id, t.produkt_id)];
        assert_eq!(geraet_finden(&umgesteckt, &t), Some(1));
        assert_eq!(geraet_finden(&[g(0, &t.geraet_name, t.hersteller_id, 0x1901)], &t), None, "anderes Produkt");
        assert_eq!(geraet_finden(&[g(0, &t.geraet_name, 0x1, t.produkt_id)], &t), None, "anderer Hersteller");
        assert_eq!(geraet_finden(&[g(0, "Honeycomb Bravo", t.hersteller_id, t.produkt_id)], &t), None, "anderer Name");
        assert_eq!(geraet_finden(&[], &t), None);
    }

    #[test]
    fn suche_meldet_nur_wechsel() {
        let mut s = SuchStand::default();
        assert!(s.geaendert(None), "der erste Nichtfund kommt ins Protokoll");
        assert!(!s.geaendert(None), "der Sekundentakt flutet es nicht");
        assert!(s.geaendert(Some(2)));
        assert!(!s.geaendert(Some(2)));
        assert!(s.geaendert(Some(3)), "neue Nummer");
        assert!(s.geaendert(None), "abgesteckt");
    }

    #[test]
    fn suchbericht_nennt_das_gemerkte_und_beim_nichtfund_die_angeschlossenen() {
        let t = taste();
        let gefunden = suchbericht(&t, Some(1), &[]);
        assert!(gefunden.contains("gefunden unter Nummer 1") && gefunden.contains("294B:1900") && gefunden.contains("Knopf 5"), "{gefunden}");
        let liste = [g(0, "Microsoft PC-Joystick-Treiber", 0x294B, 0x1900), g(4, "Pedale", 0x10, 0x20)];
        let fehlt = suchbericht(&t, None, &liste);
        assert!(fehlt.contains("nicht gefunden") && fehlt.contains("Honeycomb Alpha Flight Controls"), "{fehlt}");
        assert!(fehlt.contains(r#"0: "Microsoft PC-Joystick-Treiber" (294B:1900)"#) && fehlt.contains(r#"4: "Pedale" (0010:0020)"#), "{fehlt}");
        assert!(suchbericht(&t, None, &[]).ends_with("angeschlossen: []"));
    }

    #[test]
    fn geraetename_aus_der_registry_sonst_winmm() {
        assert!(oem_schluessel(0x294B, 0x1900).ends_with(r"Joystick\OEM\VID_294B&PID_1900"));
        assert_eq!(name_waehlen(Some(" Honeycomb Alpha "), "Treiber"), "Honeycomb Alpha");
        assert_eq!(name_waehlen(Some(" "), "Treiber"), "Treiber");
        assert_eq!(name_waehlen(None, "Treiber"), "Treiber");
        assert_eq!(name_waehlen(Some(&"x".repeat(80)), "T").chars().count(), 64, "pruefen darf nicht scheitern");
    }

    #[test]
    fn masken_und_neuer_knopf() {
        assert!(knopf_gedrueckt(0b1, 1));
        assert!(knopf_gedrueckt(1 << 31, 32));
        assert!(!knopf_gedrueckt(u32::MAX, 0) && !knopf_gedrueckt(u32::MAX, 33), "ausserhalb 1..=32 nie");
        assert!(!knopf_gedrueckt(0b10, 1));
        assert_eq!(neuer_knopf(0, 0b100), Some(3));
        assert_eq!(neuer_knopf(0b100, 0b100), None, "schon gehalten");
        assert_eq!(neuer_knopf(0b100, 0b101), Some(1));
        assert_eq!(neuer_knopf(0, 1 << 31), Some(32));
        assert_eq!(neuer_knopf(0b1, 0), None, "Loslassen");
    }

    #[test]
    fn auftrag_fehlt_null_objekt_und_kaputt() {
        assert_eq!(aus_auftrag(&json!({"quellen": []})).unwrap(), None);
        assert_eq!(aus_auftrag(&json!({"yokeTaste": null})).unwrap(), Some(None));
        let t = taste();
        let v = json!({"yokeTaste": {"geraet_name": t.geraet_name, "hersteller_id": 0x294B, "produkt_id": 0x1900, "knopf": 5}});
        assert_eq!(aus_auftrag(&v).unwrap(), Some(Some(t)));
        for kaputt in [
            json!({"yokeTaste": {"geraet_name": "x", "hersteller_id": 1, "produkt_id": 2, "knopf": 0}}),
            json!({"yokeTaste": {"geraet_name": "x", "hersteller_id": 1, "produkt_id": 2, "knopf": 33}}),
            json!({"yokeTaste": {"geraet_name": "", "hersteller_id": 1, "produkt_id": 2, "knopf": 1}}),
            json!({"yokeTaste": {"geraet_name": "x", "hersteller_id": 70000, "produkt_id": 2, "knopf": 1}}),
            json!({"yokeTaste": "F5"}),
        ] {
            assert!(aus_auftrag(&kaputt).is_err(), "{kaputt}");
        }
    }
}
