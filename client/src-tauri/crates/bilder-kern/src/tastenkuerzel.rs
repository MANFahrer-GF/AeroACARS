//! Tastenkuerzel der Fensteraufnahme (`bild_aufnehmen`, Vorgabe Strg+Shift+S).
//!
//! Text wie „Ctrl+Shift+S“; „Strg“, „Umschalt“ und „Win“ werden verstanden.
//! Mindestens ein Modifikator ist Pflicht, sonst waere eine einzelne
//! Buchstabentaste im ganzen System belegt.

use crate::Fehler;

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taste {
    /// Bits wie bei `RegisterHotKey` (MOD_ALT ...).
    pub mods: u32,
    /// Windows-Tastencode (`A`-`Z` = 0x41.., `0`-`9` = 0x30.., `F1` = 0x70..).
    pub vk: u32,
    /// Einheitliche Schreibweise, z. B. `Ctrl+Shift+S`.
    pub text: String,
}

pub fn lesen(roh: &str) -> Result<Taste, Fehler> {
    let fehler = || Fehler::Eingabe(format!("Tastenkuerzel {roh:?} nicht lesbar (Beispiel: Ctrl+Shift+S)"));
    let mut mods = 0u32;
    let mut taste: Option<(u32, String)> = None;
    for teil in roh.split('+').map(str::trim) {
        if teil.is_empty() {
            return Err(fehler());
        }
        let klein = teil.to_ascii_lowercase();
        match klein.as_str() {
            "ctrl" | "strg" | "control" => mods |= MOD_CONTROL,
            "shift" | "umschalt" => mods |= MOD_SHIFT,
            "alt" => mods |= MOD_ALT,
            "win" | "super" | "meta" | "windows" => mods |= MOD_WIN,
            _ => {
                if taste.is_some() {
                    return Err(fehler());
                }
                let b = klein.as_bytes();
                let t = if b.len() == 1 && b[0].is_ascii_lowercase() {
                    (u32::from(b[0].to_ascii_uppercase()), teil.to_ascii_uppercase())
                } else if b.len() == 1 && b[0].is_ascii_digit() {
                    (u32::from(b[0]), teil.to_string())
                } else if let Some(n) = klein.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=24).contains(n)) {
                    (0x70 + n - 1, format!("F{n}"))
                } else {
                    return Err(fehler());
                };
                taste = Some(t);
            }
        }
    }
    let (vk, name) = taste.ok_or_else(fehler)?;
    if mods == 0 {
        return Err(Fehler::Eingabe("Ein Tastenkuerzel braucht mindestens Strg, Alt, Umschalt oder Win".into()));
    }
    let mut text = String::new();
    for (bit, n) in [(MOD_CONTROL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Win")] {
        if mods & bit != 0 {
            text.push_str(n);
            text.push('+');
        }
    }
    text.push_str(&name);
    Ok(Taste { mods, vk, text })
}

/// Die beiden Kennungen fuer `RegisterHotKey`: die neue Kombi wird unter der
/// freien angemeldet, solange die alte noch gilt.
pub const HOTKEY_IDS: [i32; 2] = [0x4141, 0x4142];

/// Auftrag an den Hotkey-Faden; Tasten als `(mods, vk)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Befehl {
    /// Sofort umstellen (`None` = freigeben).
    Setzen(Option<(u32, u32)>),
    /// Neue Kombi zusaetzlich anmelden, die alte bleibt.
    Vorbereiten((u32, u32)),
    /// Die vorbereitete Kombi gilt, die alte wird abgemeldet.
    Bestaetigen,
    /// Die vorbereitete Kombi wird abgemeldet, die alte gilt weiter.
    Verwerfen,
}

/// Was der Hotkey-Faden gerade bei Windows angemeldet hat.
#[derive(Debug, Default)]
pub struct Belegung {
    gehalten: Option<(i32, u32, u32)>,
    vorbereitet: Option<(i32, u32, u32)>,
}

impl Belegung {
    /// Fuehrt einen Auftrag aus. Die alte Kombi geht erst, wenn die neue steht; die schon
    /// gehaltene gilt als Erfolg (nochmal anmelden scheiterte an uns selbst).
    pub fn ausfuehren<E>(
        &mut self,
        befehl: Befehl,
        mut anmelden: impl FnMut(i32, u32, u32) -> Result<(), E>,
        mut abmelden: impl FnMut(i32),
    ) -> Result<(), E> {
        match befehl {
            Befehl::Setzen(None) => {
                self.verwerfen(&mut abmelden);
                if let Some((id, _, _)) = self.gehalten.take() {
                    abmelden(id);
                }
            }
            Befehl::Setzen(Some(neu)) => {
                self.vorbereiten(neu, &mut anmelden, &mut abmelden)?;
                self.bestaetigen(&mut abmelden);
            }
            Befehl::Vorbereiten(neu) => self.vorbereiten(neu, &mut anmelden, &mut abmelden)?,
            Befehl::Bestaetigen => self.bestaetigen(&mut abmelden),
            Befehl::Verwerfen => self.verwerfen(&mut abmelden),
        }
        Ok(())
    }

    fn vorbereiten<E>(&mut self, neu: (u32, u32), anmelden: &mut impl FnMut(i32, u32, u32) -> Result<(), E>, abmelden: &mut impl FnMut(i32)) -> Result<(), E> {
        self.verwerfen(abmelden); // ein liegengebliebener Versuch darf nicht weiter gelten
        if self.gehalten.map(|(_, m, v)| (m, v)) == Some(neu) {
            return Ok(());
        }
        let id = if self.gehalten.map(|(id, _, _)| id) == Some(HOTKEY_IDS[0]) { HOTKEY_IDS[1] } else { HOTKEY_IDS[0] };
        anmelden(id, neu.0, neu.1)?;
        self.vorbereitet = Some((id, neu.0, neu.1));
        Ok(())
    }

    fn bestaetigen(&mut self, abmelden: &mut impl FnMut(i32)) {
        if let Some(neu) = self.vorbereitet.take() {
            if let Some((id, _, _)) = self.gehalten.replace(neu) {
                abmelden(id);
            }
        }
    }

    fn verwerfen(&mut self, abmelden: &mut impl FnMut(i32)) {
        if let Some((id, _, _)) = self.vorbereitet.take() {
            abmelden(id);
        }
    }
}

/// Was das Protokoll ueber einen Anmeldeversuch wissen muss (nur Zustandswechsel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Meldung {
    Nichts,
    Fehlschlag(String),
    /// Nach so vielen Wiederholungen doch noch angemeldet.
    Nachgeholt(u32),
}

/// Stand des Tastenkuerzels: welche Kombi gelten soll, ob sie angemeldet ist und
/// was zuletzt schiefging. `setzen` ist die Anmeldung beim Hotkey-Faden.
#[derive(Debug, Default)]
pub struct Anmeldung {
    soll: Option<Taste>,
    angemeldet: bool,
    fehler: Option<String>,
    versuche: u32,
}

impl Anmeldung {
    pub fn aktiv(&self) -> bool {
        self.angemeldet
    }

    pub fn fehler(&self) -> Option<&str> {
        self.fehler.as_deref()
    }

    /// Der Hotkey-Faden laeuft nicht: nichts zu wiederholen.
    pub fn dienst_fehlt(&mut self, fehler: &Fehler) {
        self.fehler = Some(fehler.meldung().to_string());
    }

    /// Anmelden beim Programmstart. Eine unlesbare Kombi wird nie wiederholt.
    pub fn starten(&mut self, text: &str, setzen: impl FnMut(&Taste) -> Result<(), Fehler>) -> Meldung {
        match lesen(text) {
            Ok(t) => {
                self.soll = Some(t);
                self.nachholen(setzen)
            }
            Err(e) => {
                self.fehler = Some(e.meldung().to_string());
                Meldung::Fehlschlag(e.meldung().to_string())
            }
        }
    }

    /// Versucht die gueltige Kombi anzumelden, solange sie noch nicht angemeldet ist
    /// (das alte Programm haelt sie nach einem Update noch einen Moment fest).
    pub fn nachholen(&mut self, mut setzen: impl FnMut(&Taste) -> Result<(), Fehler>) -> Meldung {
        let Some(t) = self.soll.clone().filter(|_| !self.angemeldet) else { return Meldung::Nichts };
        match setzen(&t) {
            Ok(()) => {
                self.angemeldet = true;
                self.fehler = None;
                if self.versuche == 0 {
                    Meldung::Nichts
                } else {
                    Meldung::Nachgeholt(std::mem::take(&mut self.versuche))
                }
            }
            Err(e) => {
                self.versuche = self.versuche.saturating_add(1);
                let neu = self.fehler.as_deref() != Some(e.meldung());
                self.fehler = Some(e.meldung().to_string());
                if neu {
                    Meldung::Fehlschlag(e.meldung().to_string())
                } else {
                    Meldung::Nichts
                }
            }
        }
    }

    /// Wechselt auf die Kombi `neu`: `vorbereiten` meldet sie zusaetzlich an, dann
    /// `speichern`, dann `abschliessen(gespeichert)` (die alte aufgeben oder die neue
    /// zuruecknehmen). Ist `neu` vergeben, wird nichts gespeichert und die alte bleibt.
    pub fn aendern(
        &mut self,
        neu: &str,
        vorbereiten: impl FnOnce(&Taste) -> Result<(), Fehler>,
        speichern: impl FnOnce() -> Result<(), Fehler>,
        abschliessen: impl FnOnce(bool),
    ) -> Result<(), Fehler> {
        let taste = lesen(neu)?;
        vorbereiten(&taste).map_err(|e| {
            let bleibt = self.soll.as_ref().filter(|_| self.angemeldet).map(|a| format!("; {} bleibt aktiv", a.text)).unwrap_or_default();
            Fehler::Eingabe(format!("{} nicht uebernommen: {}{bleibt}", taste.text, e.meldung()))
        })?;
        if let Err(e) = speichern() {
            abschliessen(false);
            return Err(e);
        }
        abschliessen(true);
        self.soll = Some(taste);
        self.angemeldet = true;
        self.fehler = None;
        self.versuche = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vorgabe_und_schreibweisen() {
        let t = lesen("Ctrl+Shift+S").unwrap();
        assert_eq!((t.mods, t.vk, t.text.as_str()), (MOD_CONTROL | MOD_SHIFT, 0x53, "Ctrl+Shift+S"));
        assert_eq!(lesen(" strg + umschalt + s ").unwrap(), t);
        assert_eq!(lesen("Shift+Ctrl+S").unwrap().text, "Ctrl+Shift+S");
        let f = lesen("Alt+F9").unwrap();
        assert_eq!((f.mods, f.vk, f.text.as_str()), (MOD_ALT, 0x78, "Alt+F9"));
        assert_eq!(lesen("Win+1").unwrap().vk, 0x31);
    }

    #[test]
    fn ohne_modifikator_oder_unlesbar_scheitert() {
        for schlecht in ["S", "F9", "", "Ctrl", "Ctrl+", "Ctrl+Shift", "Ctrl+S+D", "Ctrl+Ä", "Ctrl+F25", "Ctrl+F0", "Ctrl++S", "Ctrl+Space"] {
            assert!(lesen(schlecht).is_err(), "{schlecht:?}");
        }
    }

    /// Windows im Kleinen: eine Kombi hat genau einen Halter.
    #[derive(Default)]
    struct System {
        fremd: Vec<(u32, u32)>,
        eigen: Vec<(i32, u32, u32)>,
        ablauf: Vec<String>,
    }

    fn befehl(b: &mut Belegung, s: &std::cell::RefCell<System>, was: Befehl) -> Result<(), u32> {
        b.ausfuehren(
            was,
            |id, m, v| {
                let mut s = s.borrow_mut();
                if s.fremd.contains(&(m, v)) || s.eigen.iter().any(|e| (e.1, e.2) == (m, v)) {
                    return Err(1409);
                }
                s.eigen.push((id, m, v));
                s.ablauf.push(format!("an {v}"));
                Ok(())
            },
            |id| {
                let mut s = s.borrow_mut();
                let v = s.eigen.iter().find(|e| e.0 == id).map(|e| e.2).unwrap();
                s.eigen.retain(|e| e.0 != id);
                s.ablauf.push(format!("ab {v}"));
            },
        )
    }

    fn umstellen(b: &mut Belegung, s: &std::cell::RefCell<System>, neu: Option<(u32, u32)>) -> Result<(), u32> {
        befehl(b, s, Befehl::Setzen(neu))
    }

    #[test]
    fn neue_kombi_steht_bevor_die_alte_geht() {
        let s = std::cell::RefCell::new(System::default());
        let mut b = Belegung::default();
        umstellen(&mut b, &s, Some((2, 83))).unwrap();
        umstellen(&mut b, &s, Some((1, 120))).unwrap();
        assert_eq!(s.borrow().ablauf, ["an 83", "an 120", "ab 83"]);
        assert_eq!(s.borrow().eigen.len(), 1);
    }

    #[test]
    fn vergebene_kombi_laesst_die_alte_stehen_und_folgewechsel_gehen() {
        let s = std::cell::RefCell::new(System { fremd: vec![(1, 120)], ..Default::default() });
        let mut b = Belegung::default();
        umstellen(&mut b, &s, Some((2, 83))).unwrap();
        assert_eq!(umstellen(&mut b, &s, Some((1, 120))), Err(1409));
        assert_eq!(s.borrow().eigen.iter().map(|e| e.2).collect::<Vec<_>>(), [83], "die alte gilt weiter");
        umstellen(&mut b, &s, Some((2, 84))).unwrap();
        assert_eq!(s.borrow().eigen.iter().map(|e| e.2).collect::<Vec<_>>(), [84]);
    }

    #[test]
    fn gehaltene_kombi_ist_erfolg_ohne_aufruf_und_none_gibt_frei() {
        let s = std::cell::RefCell::new(System::default());
        let mut b = Belegung::default();
        umstellen(&mut b, &s, Some((2, 83))).unwrap();
        umstellen(&mut b, &s, Some((2, 83))).expect("nochmal dieselbe scheitert nicht an uns selbst");
        assert_eq!(s.borrow().ablauf, ["an 83"]);
        umstellen(&mut b, &s, None).unwrap();
        assert!(s.borrow().eigen.is_empty());
        umstellen(&mut b, &s, None).unwrap();
        assert_eq!(s.borrow().ablauf.len(), 2, "nichts gehalten, nichts abzumelden");
    }

    fn belegt(_: &Taste) -> Result<(), Fehler> {
        Err(Fehler::Eingabe("Tastenkuerzel ist schon belegt (Windows-Fehler 1409)".into()))
    }

    #[test]
    fn nach_update_wird_wiederholt_bis_das_alte_programm_weg_ist() {
        let mut a = Anmeldung::default();
        assert!(matches!(a.starten("Ctrl+Shift+S", belegt), Meldung::Fehlschlag(_)));
        assert!(!a.aktiv());
        assert!(a.fehler().unwrap().contains("1409"));
        assert_eq!(a.nachholen(belegt), Meldung::Nichts, "dieselbe Meldung nur einmal im Protokoll");
        assert_eq!(a.nachholen(|_| Ok(())), Meldung::Nachgeholt(2));
        assert!(a.aktiv());
        assert_eq!(a.fehler(), None);
        let mut gerufen = false;
        assert_eq!(
            a.nachholen(|_| {
                gerufen = true;
                Ok(())
            }),
            Meldung::Nichts
        );
        assert!(!gerufen, "angemeldet: nichts mehr zu tun");
    }

    #[test]
    fn beim_ersten_versuch_erfolgreich_und_ohne_gueltige_kombi_keine_wiederholung() {
        let mut a = Anmeldung::default();
        assert_eq!(a.starten("Alt+F9", |_| Ok(())), Meldung::Nichts);
        assert!(a.aktiv());
        let mut kaputt = Anmeldung::default();
        assert!(matches!(kaputt.starten("S", |_| panic!("unlesbar wird nie angemeldet")), Meldung::Fehlschlag(_)));
        assert_eq!(kaputt.nachholen(|_| panic!("unlesbar wird nie wiederholt")), Meldung::Nichts);
        assert!(!kaputt.aktiv() && kaputt.fehler().is_some());
        let mut ohne_dienst = Anmeldung::default();
        ohne_dienst.dienst_fehlt(&Fehler::Io("Hotkey-Faden startet nicht".into()));
        assert_eq!(ohne_dienst.nachholen(|_| panic!("ohne Dienst keine Wiederholung")), Meldung::Nichts);
        assert!(!ohne_dienst.aktiv() && ohne_dienst.fehler().is_some());
    }

    /// Anmeldung und Belegung zusammen, wie im Programm.
    struct Aufbau {
        a: Anmeldung,
        b: std::cell::RefCell<Belegung>,
        s: std::cell::RefCell<System>,
    }

    impl Aufbau {
        fn neu(fremd: Vec<(u32, u32)>) -> Self {
            Aufbau { a: Anmeldung::default(), b: Default::default(), s: std::cell::RefCell::new(System { fremd, ..Default::default() }) }
        }

        fn start(&mut self, text: &str) -> Meldung {
            let (b, s) = (&self.b, &self.s);
            self.a.starten(text, |t| Self::auftrag(b, s, Befehl::Setzen(Some((t.mods, t.vk)))))
        }

        fn auftrag(b: &std::cell::RefCell<Belegung>, s: &std::cell::RefCell<System>, was: Befehl) -> Result<(), Fehler> {
            befehl(&mut b.borrow_mut(), s, was).map_err(|c| Fehler::Eingabe(format!("schon belegt (Windows-Fehler {c})")))
        }

        fn nachholen(&mut self) -> Meldung {
            let (b, s) = (&self.b, &self.s);
            self.a.nachholen(|t| Self::auftrag(b, s, Befehl::Setzen(Some((t.mods, t.vk)))))
        }

        fn aendern(&mut self, neu: &str, speichern: Result<(), Fehler>) -> Result<(), Fehler> {
            let (b, s) = (&self.b, &self.s);
            self.a.aendern(
                neu,
                |t| Self::auftrag(b, s, Befehl::Vorbereiten((t.mods, t.vk))),
                || {
                    s.borrow_mut().ablauf.push("speichern".into());
                    speichern
                },
                |ok| Self::auftrag(b, s, if ok { Befehl::Bestaetigen } else { Befehl::Verwerfen }).unwrap(),
            )
        }

        fn ablauf(&self) -> Vec<String> {
            self.s.borrow().ablauf.clone()
        }

        fn angemeldet(&self) -> Vec<u32> {
            self.s.borrow().eigen.iter().map(|e| e.2).collect()
        }
    }

    const ALT_F9: (u32, u32) = (MOD_ALT, 0x78);

    #[test]
    fn wechsel_laeuft_anmelden_speichern_alte_abmelden() {
        let mut x = Aufbau::neu(vec![]);
        x.start("Ctrl+Shift+S");
        x.aendern("Alt+F9", Ok(())).unwrap();
        assert_eq!(x.ablauf(), ["an 83", "an 120", "speichern", "ab 83"]);
        assert_eq!(x.angemeldet(), [120]);
        assert!(x.a.aktiv());
    }

    #[test]
    fn vergebene_neue_kombi_wird_nicht_gespeichert_und_die_alte_bleibt_aktiv() {
        let mut x = Aufbau::neu(vec![ALT_F9]);
        x.start("Ctrl+Shift+S");
        let e = x.aendern("Alt+F9", Ok(())).unwrap_err();
        assert!(e.meldung().contains("Alt+F9") && e.meldung().contains("Ctrl+Shift+S bleibt aktiv"), "{}", e.meldung());
        assert_eq!(x.ablauf(), ["an 83"], "nichts gespeichert, nichts abgemeldet");
        assert_eq!(x.angemeldet(), [83]);
        assert!(x.a.aktiv());
        assert_eq!(x.a.fehler(), None, "die alte Kombi laeuft, der Fehler gehoert zur Antwort");
        assert_eq!(x.nachholen(), Meldung::Nichts);
    }

    #[test]
    fn scheitert_das_speichern_geht_nur_die_neue_wieder_weg() {
        let mut x = Aufbau::neu(vec![]);
        x.start("Ctrl+Shift+S");
        assert!(x.aendern("Alt+F9", Err(Fehler::Io("Platte voll".into()))).is_err());
        assert_eq!(x.ablauf(), ["an 83", "an 120", "speichern", "ab 120"], "die alte wurde nie abgemeldet");
        assert_eq!(x.angemeldet(), [83]);
        assert!(x.a.aktiv());
        x.aendern("Alt+F9", Ok(())).expect("ein spaeterer Wechsel geht");
        assert_eq!(x.angemeldet(), [120]);
    }

    #[test]
    fn scheitert_das_speichern_ohne_alte_kombi_bleibt_nichts_stehen() {
        let mut x = Aufbau::neu(vec![]);
        assert!(matches!(x.start("S"), Meldung::Fehlschlag(_)));
        assert!(x.aendern("Alt+F9", Err(Fehler::Io("Platte voll".into()))).is_err());
        assert!(x.angemeldet().is_empty());
        assert!(!x.a.aktiv());
        assert_eq!(x.nachholen(), Meldung::Nichts, "nichts Gueltiges zu wiederholen");
    }

    #[test]
    fn liegengebliebener_versuch_wird_beim_naechsten_auftrag_abgemeldet() {
        let s = std::cell::RefCell::new(System::default());
        let mut b = Belegung::default();
        umstellen(&mut b, &s, Some((2, 83))).unwrap();
        befehl(&mut b, &s, Befehl::Vorbereiten((1, 120))).unwrap(); // Zeitueberschreitung, keiner bestaetigt
        umstellen(&mut b, &s, Some((2, 84))).unwrap();
        assert_eq!(s.borrow().eigen.iter().map(|e| e.2).collect::<Vec<_>>(), [84]);
        befehl(&mut b, &s, Befehl::Vorbereiten((1, 120))).unwrap();
        befehl(&mut b, &s, Befehl::Verwerfen).unwrap();
        befehl(&mut b, &s, Befehl::Bestaetigen).unwrap();
        assert_eq!(s.borrow().eigen.iter().map(|e| e.2).collect::<Vec<_>>(), [84], "verworfen heisst verworfen");
    }

    #[test]
    fn vorbereiten_der_gehaltenen_kombi_ist_kein_fehler() {
        let s = std::cell::RefCell::new(System::default());
        let mut b = Belegung::default();
        umstellen(&mut b, &s, Some((2, 83))).unwrap();
        befehl(&mut b, &s, Befehl::Vorbereiten((2, 83))).unwrap();
        befehl(&mut b, &s, Befehl::Bestaetigen).unwrap();
        assert_eq!(s.borrow().ablauf, ["an 83"]);
    }

    #[test]
    fn wechsel_beendet_eine_laufende_wiederholung() {
        let mut x = Aufbau::neu(vec![(MOD_CONTROL | MOD_SHIFT, 0x53)]);
        assert!(matches!(x.start("Ctrl+Shift+S"), Meldung::Fehlschlag(_)));
        x.aendern("Alt+F9", Ok(())).unwrap();
        assert!(x.a.aktiv());
        assert_eq!(x.a.fehler(), None);
        assert_eq!(x.nachholen(), Meldung::Nichts);
        assert_eq!(x.angemeldet(), [120]);
    }

    #[test]
    fn vergebene_kombi_waehrend_der_wiederholung_laesst_die_gespeicherte_weiterlaufen() {
        let alt = (MOD_CONTROL | MOD_SHIFT, 0x53);
        let mut x = Aufbau::neu(vec![alt, ALT_F9]);
        x.start("Ctrl+Shift+S");
        let e = x.aendern("Alt+F9", Ok(())).unwrap_err();
        assert!(!e.meldung().contains("bleibt aktiv"), "es ist ja keine aktiv: {}", e.meldung());
        x.s.borrow_mut().fremd.clear(); // das alte Programm ist weg
        assert!(matches!(x.nachholen(), Meldung::Nachgeholt(_)));
        assert_eq!(x.angemeldet(), [83], "wiederholt wird die Kombi aus der Datei");
        assert!(x.a.aktiv());
    }
}
