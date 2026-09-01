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
        assert_eq!(
            SzenerieDiagnose::NichtAngefordert.kurz(),
            "nicht_angefordert"
        );
        assert_eq!(SzenerieDiagnose::Angefordert.kurz(), "keine_antwort");
        assert_eq!(
            SzenerieDiagnose::Abgelehnt("egal".to_string()).kurz(),
            "abgelehnt"
        );
    }
}

// ---------------------------------------------------------------------
// Auftragsbuch
// ---------------------------------------------------------------------

/// Was der Simulator zu einem Platz gesagt hat.
#[derive(Debug, Clone, PartialEq)]
pub enum Auftragszustand {
    /// Angemeldet, aber noch nicht gestellt.
    Offen,
    /// Gestellt, Antwort steht aus (Zeitpunkt in Millisekunden).
    Laeuft { seit_ms: i64 },
    /// Vollstaendige Lieferung eingetroffen.
    Geliefert,
    /// SimConnect hat die Anfrage zurueckgewiesen.
    Abgelehnt,
}

#[derive(Debug, Clone)]
struct Auftrag {
    zustand: Auftragszustand,
    versuche: u8,
    auskunft: Option<SzenerieFlughafen>,
    grund: Option<String>,
    /// Rangfolge: kleiner heisst frueher dran. Das Ausweichziel steht
    /// vor dem geplanten, weil dort gelandet wird.
    ///
    /// ⚠ Ohne das entschied die alphabetische Ordnung der Ablage. Die
    /// Reihenfolge, die der Aufrufer aufstellt, ging dabei verloren —
    /// bei EDDF/LEZL kam Frankfurt zuerst dran, obwohl in Sevilla
    /// gelandet wird (QS-Befund 3, 01.09.2026).
    rang: u8,
    /// Kennung des letzten gestellten Auftrags.
    letzte_id: u32,
}

/// Wie lange auf eine Antwort gewartet wird, bevor neu gefragt wird.
///
/// ⚠ Der Grund fuer diese ganze Klasse: Bei EDDF→LEZL am 01.09.2026 war
/// das Ziel **einmal** gefragt worden — am Gate in Frankfurt, 1.400 km
/// entfernt. Es kam keine Antwort, und niemand fragte je wieder. Beim
/// Aufsetzen lag deshalb die Szenerie des STARTflughafens vor, der
/// Vergleich fiel aus, und am Flug stand `auskunft_ohne_vergleich`.
pub const WARTEZEIT_MS: i64 = 60_000;

/// Wie oft ein Platz hoechstens gefragt wird.
///
/// Zehn Versuche im Minutentakt decken jeden Anflug ab. Ein Platz, den
/// der Simulator nach zehn Minuten in Reichweite nicht kennt, kennt er
/// nicht.
pub const HOECHSTVERSUCHE: u8 = 10;

/// Wer welchen Platz gefragt hat, und was zurueckkam.
///
/// # Warum ein Buch und kein Platz
///
/// Bis v1.7.13 hielt der Adapter **eine** Auskunft und **einen** Wunsch.
/// Das hatte drei Folgen, die zusammen den MSFS-Weg lahmlegten:
///
/// 1. Start und Ziel wurden in einer Schleife hintereinander angemeldet;
///    jede Anmeldung loeschte die vorherige Antwort. Wer zuletzt kam,
///    gewann.
/// 2. Eine Lieferung wurde mit dem **gerade aktuellen Wunsch**
///    beschriftet, nicht mit dem Platz, den sie beschreibt. Traf die
///    Antwort des Startflughafens ein, nachdem das Ziel angemeldet war,
///    trug sie den Namen des Ziels — plausible Zahlen, falscher Platz.
/// 3. `if wunsch == icao { return }` verhinderte jeden zweiten Versuch.
///    "Unterwegs", "erledigt" und "gescheitert" waren derselbe Zustand.
///
/// Das Buch trennt die Plaetze und beschriftet jede Lieferung mit dem
/// Platz, der wirklich gefragt wurde.
/// Wie viele vergebene Kennungen zurueckverfolgt werden.
///
/// ⚠ Der Grund fuer diese Zahl: Eine Antwort, die nach der Wartezeit
/// eintrifft, muss noch ihrem Platz zugeordnet werden koennen — sonst
/// wird sie dem inzwischen laufenden Auftrag zugeschlagen. Genau dieser
/// Fehler ist in v1.7.14 zunaechst nur verschoben statt behoben worden
/// (QS-Befund 1, 01.09.2026).
pub const KENNUNGEN_GEDAECHTNIS: usize = 16;

#[derive(Debug, Clone, Default)]
pub struct Auftragsbuch {
    auftraege: std::collections::BTreeMap<String, Auftrag>,
    laeuft: Option<String>,
    /// Fortlaufende Kennung. Jeder VERSUCH bekommt eine eigene — nicht
    /// jeder Platz.
    naechste_id: u32,
    /// Kennung → Platz, aelteste vorn.
    kennungen: std::collections::VecDeque<(u32, String)>,
}

impl Auftragsbuch {
    pub fn neu() -> Self {
        Self::default()
    }

    /// Einen Platz anmelden. Mehrfach aufzurufen ist ausdruecklich
    /// erlaubt — genau darueber laeuft die Wiederholung im Anflug.
    pub fn wunsch(&mut self, icao: &str) {
        self.wunsch_mit_rang(icao, 0);
    }

    /// Wie `wunsch`, aber mit Rangfolge — kleiner heisst frueher dran.
    ///
    /// Ein bereits eingetragener Platz behaelt den BESSEREN Rang: Wird
    /// das geplante Ziel spaeter zum Ausweichziel, rueckt es vor; ein
    /// erneutes Anmelden als geplantes Ziel darf es nicht zurueckstufen.
    pub fn wunsch_mit_rang(&mut self, icao: &str, rang: u8) {
        let icao = icao.trim().to_ascii_uppercase();
        if icao.len() != 4 {
            return;
        }
        self.auftraege
            .entry(icao)
            .and_modify(|a| a.rang = a.rang.min(rang))
            .or_insert(Auftrag {
                zustand: Auftragszustand::Offen,
                versuche: 0,
                auskunft: None,
                grund: None,
                rang,
                letzte_id: 0,
            });
    }

    /// Der naechste Platz, den der Verbindungsfaden fragen soll.
    ///
    /// Gibt nichts zurueck, solange eine Anfrage laeuft und die
    /// Wartezeit nicht um ist — SimConnect beantwortet immer nur eine.
    pub fn naechster(&mut self, jetzt_ms: i64) -> Option<String> {
        // Laeuft eine und ist noch Zeit? Dann nichts Neues.
        if let Some(laufend) = self.laeuft.clone() {
            if let Some(a) = self.auftraege.get(&laufend) {
                if let Auftragszustand::Laeuft { seit_ms } = a.zustand {
                    if jetzt_ms - seit_ms < WARTEZEIT_MS {
                        return None;
                    }
                }
            }
            // Wartezeit um: der Platz darf wieder in die Reihe.
            self.laeuft = None;
        }
        let kandidat = self
            .auftraege
            .iter()
            .filter(|(_, a)| a.versuche < HOECHSTVERSUCHE)
            .filter(|(_, a)| {
                matches!(
                    a.zustand,
                    Auftragszustand::Offen | Auftragszustand::Laeuft { .. }
                )
            })
            // Wer am laengsten nicht dran war, zuerst.
            // ⚠ Rang VOR dem Namen. Sonst entscheidet das Alphabet, und
            // die Rangfolge des Aufrufers ist wirkungslos.
            .min_by_key(|(icao, a)| (a.versuche, a.rang, (*icao).clone()))
            .map(|(icao, _)| icao.clone())?;
        Some(kandidat)
    }

    /// Festhalten, dass die Anfrage wirklich gestellt wurde.
    ///
    /// Gibt die **Kennung dieses Versuchs** zurueck. Jeder Versuch
    /// bekommt eine eigene; damit laesst sich eine eintreffende Antwort
    /// ihrem Platz zuordnen, auch wenn inzwischen ein anderer laeuft.
    #[must_use]
    pub fn gestellt(&mut self, icao: &str, jetzt_ms: i64) -> u32 {
        let icao = icao.trim().to_ascii_uppercase();
        self.naechste_id = self.naechste_id.wrapping_add(1);
        let id = self.naechste_id;
        if let Some(a) = self.auftraege.get_mut(&icao) {
            a.zustand = Auftragszustand::Laeuft { seit_ms: jetzt_ms };
            a.versuche = a.versuche.saturating_add(1);
            a.letzte_id = id;
        }
        self.kennungen.push_back((id, icao.clone()));
        while self.kennungen.len() > KENNUNGEN_GEDAECHTNIS {
            self.kennungen.pop_front();
        }
        self.laeuft = Some(icao);
        id
    }

    /// Zu welchem Platz eine Kennung gehoert.
    pub fn platz_zu_kennung(&self, id: u32) -> Option<String> {
        self.kennungen
            .iter()
            .rev()
            .find(|(k, _)| *k == id)
            .map(|(_, icao)| icao.clone())
    }

    /// Welcher Platz gerade beantwortet wird.
    ///
    /// ⚠ Damit wird die Lieferung beschriftet — NICHT mit dem zuletzt
    /// angemeldeten Wunsch. Das war Fehler 2 oben.
    pub fn laufender(&self) -> Option<String> {
        self.laeuft.clone()
    }

    /// Eine Lieferung ueber die **Kennung ihres Versuchs** ablegen.
    ///
    /// ⚠ Das ist der einzige Weg, der eine verspaetete Antwort richtig
    /// zuordnet. Nach der Wartezeit gibt das Buch den naechsten Auftrag
    /// heraus; kommt die alte Antwort dann noch, gehoert sie IHREM
    /// Platz — nicht dem, der gerade laeuft. Der Rueckgabewert nennt
    /// den Platz, unter dem sie abgelegt wurde, oder `None`, wenn die
    /// Kennung unbekannt ist (dann wird verworfen, nicht geraten).
    pub fn geliefert_zu_kennung(&mut self, id: u32, auskunft: SzenerieFlughafen) -> Option<String> {
        let icao = self.platz_zu_kennung(id)?;
        self.geliefert(&icao, auskunft);
        Some(icao)
    }

    /// Eine Zurueckweisung ueber die Kennung festhalten.
    pub fn abgelehnt_zu_kennung(&mut self, id: u32, grund: String) -> Option<String> {
        let icao = self.platz_zu_kennung(id)?;
        self.abgelehnt(&icao, grund);
        Some(icao)
    }

    /// Eine vollstaendige Lieferung ablegen.
    pub fn geliefert(&mut self, icao: &str, auskunft: SzenerieFlughafen) {
        let icao = icao.trim().to_ascii_uppercase();
        let eintrag = self.auftraege.entry(icao.clone()).or_insert(Auftrag {
            zustand: Auftragszustand::Offen,
            versuche: 0,
            auskunft: None,
            grund: None,
            rang: 0,
            letzte_id: 0,
        });
        eintrag.zustand = Auftragszustand::Geliefert;
        eintrag.auskunft = Some(auskunft);
        if self.laeuft.as_deref() == Some(icao.as_str()) {
            self.laeuft = None;
        }
    }

    /// Eine Zurueckweisung festhalten.
    pub fn abgelehnt(&mut self, icao: &str, grund: String) {
        let icao = icao.trim().to_ascii_uppercase();
        if let Some(a) = self.auftraege.get_mut(&icao) {
            a.zustand = Auftragszustand::Abgelehnt;
            a.grund = Some(grund);
        }
        if self.laeuft.as_deref() == Some(icao.as_str()) {
            self.laeuft = None;
        }
    }

    /// Die Auskunft zu genau diesem Platz — oder nichts.
    ///
    /// ⚠ Kein Rueckfall auf "irgendeine". Die Auskunft des
    /// Startflughafens fuer das Ziel auszugeben war der Fehler, der
    /// diese Klasse ausgeloest hat.
    pub fn auskunft(&self, icao: &str) -> Option<&SzenerieFlughafen> {
        self.auftraege
            .get(&icao.trim().to_ascii_uppercase())
            .and_then(|a| a.auskunft.as_ref())
    }

    /// Zustand eines Platzes — fuer die Diagnose am Flug.
    pub fn zustand(&self, icao: &str) -> Option<Auftragszustand> {
        self.auftraege
            .get(&icao.trim().to_ascii_uppercase())
            .map(|a| a.zustand.clone())
    }

    /// Wie oft dieser Platz schon gefragt wurde.
    pub fn versuche(&self, icao: &str) -> u8 {
        self.auftraege
            .get(&icao.trim().to_ascii_uppercase())
            .map(|a| a.versuche)
            .unwrap_or(0)
    }

    /// Kurzwort zum Zustand EINES Platzes.
    ///
    /// ⚠ Die Diagnose des Adapters war global: Jede Anfrage, Ablehnung
    /// und Lieferung ueberschrieb denselben Wert, und am Flug stand der
    /// Zustand des zuletzt bearbeiteten Platzes — nicht der des Ziels
    /// (QS-Befund 4, 01.09.2026).
    pub fn diagnose(&self, icao: &str) -> String {
        let icao_gross = icao.trim().to_ascii_uppercase();
        match self.auftraege.get(&icao_gross) {
            None => format!("nie_gefragt({icao_gross})"),
            Some(a) => match &a.zustand {
                Auftragszustand::Offen => format!("angemeldet({icao_gross})"),
                Auftragszustand::Laeuft { .. } => {
                    format!("unterwegs({icao_gross}, versuch={})", a.versuche)
                }
                Auftragszustand::Abgelehnt => format!(
                    "abgelehnt({icao_gross}, {})",
                    a.grund.as_deref().unwrap_or("ohne Grund")
                ),
                Auftragszustand::Geliefert => {
                    let (bahnen, rollwege) = a
                        .auskunft
                        .as_ref()
                        .map(|x| (x.bahnen.len(), x.rollwege.len()))
                        .unwrap_or((0, 0));
                    format!("geliefert({icao_gross}, bahnen={bahnen}, rollwege={rollwege})")
                }
            },
        }
    }

    /// Grund einer Zurueckweisung.
    pub fn ablehnungsgrund(&self, icao: &str) -> Option<String> {
        self.auftraege
            .get(&icao.trim().to_ascii_uppercase())
            .and_then(|a| a.grund.clone())
    }
}

#[cfg(test)]
mod auftragsbuch_tests {
    use super::*;

    fn auskunft(icao: &str, bahnen: usize) -> SzenerieFlughafen {
        SzenerieFlughafen {
            icao: icao.to_string(),
            bahnen: (0..bahnen)
                .map(|i| SzenerieBahn {
                    bezeichner: format!("{:02}", i + 1),
                    kurs_grad: 0.0,
                    breite_m: 45.0,
                    laenge_m: 3000.0,
                    versetzte_schwelle_m: 0.0,
                    schwelle: (0.0, 0.0),
                    gegenende: (0.0, 0.0),
                    belag_code: 1,
                })
                .collect(),
            rollwege: Vec::new(),
            quelle: "msfs".into(),
        }
    }

    /// Fehler 1: Zwei Anmeldungen hintereinander — beide muessen dran
    /// kommen.
    ///
    /// ⚠ Genau das ging bis v1.7.13 verloren: Die zweite Anmeldung
    /// loeschte die erste Antwort, und gefragt wurde nur eine.
    #[test]
    fn start_und_ziel_verdraengen_sich_nicht() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        b.wunsch("LEZL");

        let erster = b.naechster(0).expect("erster Auftrag");
        let _ = b.gestellt(&erster, 0);
        b.geliefert(&erster, auskunft(&erster, 4));

        let zweiter = b.naechster(1_000).expect("zweiter Auftrag");
        assert_ne!(
            zweiter, erster,
            "derselbe Platz zweimal — der andere fiel aus"
        );
        let _ = b.gestellt(&zweiter, 1_000);
        b.geliefert(&zweiter, auskunft(&zweiter, 1));

        assert!(b.auskunft("EDDF").is_some());
        assert!(b.auskunft("LEZL").is_some());
    }

    /// Fehler 2: Die Lieferung traegt den Platz, der GEFRAGT wurde.
    ///
    /// ⚠ Der Adapter beschriftete sie mit dem zuletzt angemeldeten
    /// Wunsch. Traf die Antwort des Startflughafens ein, nachdem das
    /// Ziel angemeldet war, hiess Frankfurt dann Sevilla — plausible
    /// Zahlen, falscher Platz. Der Vergleich haette gegen die falsche
    /// Bahn gemessen, ohne dass etwas anschlaegt.
    #[test]
    fn eine_spaete_lieferung_wird_nicht_umbenannt() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        let laufend = b.naechster(0).expect("Auftrag");
        let _ = b.gestellt(&laufend, 0);

        // Waehrend die Antwort unterwegs ist, meldet der Flug das Ziel an.
        b.wunsch("LEZL");

        // Der Verbindungsfaden beschriftet mit `laufender()`, nicht mit
        // dem neuesten Wunsch.
        assert_eq!(b.laufender().as_deref(), Some("EDDF"));
        let traeger = b.laufender().expect("laufender Auftrag");
        b.geliefert(&traeger, auskunft(&traeger, 4));

        assert_eq!(b.auskunft("EDDF").map(|a| a.bahnen.len()), Some(4));
        assert!(
            b.auskunft("LEZL").is_none(),
            "die Frankfurter Antwort wurde dem Ziel zugeschlagen"
        );
    }

    /// Fehler 3: Nach einem Fehlschlag wird wieder gefragt.
    ///
    /// ⚠ `if wunsch == icao { return }` warf "unterwegs", "erledigt" und
    /// "gescheitert" in einen Topf. Ein Ziel, das am Gate nicht
    /// antwortete, wurde nie wieder gefragt — auch nicht im Anflug, wo
    /// seine Szenerie geladen ist.
    #[test]
    fn ein_stummer_platz_wird_im_anflug_erneut_gefragt() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let a = b.naechster(0).expect("erster Versuch");
        let _ = b.gestellt(&a, 0);

        // Kurz danach: nichts Neues, die Antwort darf noch kommen.
        assert_eq!(b.naechster(WARTEZEIT_MS - 1), None);

        // Nach der Wartezeit erneut.
        assert_eq!(b.naechster(WARTEZEIT_MS).as_deref(), Some("LEZL"));
        let _ = b.gestellt("LEZL", WARTEZEIT_MS);
        assert_eq!(b.versuche("LEZL"), 2);
    }

    /// Aber nicht endlos.
    #[test]
    fn nach_zehn_versuchen_ist_schluss() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let mut t = 0;
        for _ in 0..HOECHSTVERSUCHE {
            let a = b.naechster(t).expect("Versuch");
            let _ = b.gestellt(&a, t);
            t += WARTEZEIT_MS;
        }
        assert_eq!(b.versuche("LEZL"), HOECHSTVERSUCHE);
        assert_eq!(b.naechster(t), None, "elfter Versuch");
    }

    /// Eine gelieferte Auskunft wird nicht noch einmal gefragt.
    #[test]
    fn ein_gelieferter_platz_kommt_nicht_wieder_dran() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        let _ = b.gestellt("EDDF", 0);
        b.geliefert("EDDF", auskunft("EDDF", 4));
        assert_eq!(b.naechster(10 * WARTEZEIT_MS), None);
    }

    /// Eine Zurueckweisung wird nicht wiederholt — und der Grund bleibt.
    #[test]
    fn eine_zurueckweisung_wird_nicht_wiederholt() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("XXXX");
        let _ = b.gestellt("XXXX", 0);
        b.abgelehnt("XXXX", "unbekannter Platz".into());
        assert_eq!(b.naechster(10 * WARTEZEIT_MS), None);
        assert_eq!(b.zustand("XXXX"), Some(Auftragszustand::Abgelehnt));
        assert_eq!(
            b.ablehnungsgrund("XXXX").as_deref(),
            Some("unbekannter Platz")
        );
    }

    /// Und keine Auskunft wird fuer einen anderen Platz ausgegeben.
    ///
    /// ⚠ Das ist der Kern des Vorfalls vom 01.09.2026: Beim Aufsetzen in
    /// Sevilla lag die Szenerie Frankfurts vor. Ein Rueckfall auf
    /// "irgendeine" waere hier bequem und genau falsch.
    #[test]
    fn kein_rueckfall_auf_irgendeinen_platz() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        let _ = b.gestellt("EDDF", 0);
        b.geliefert("EDDF", auskunft("EDDF", 4));
        assert!(b.auskunft("LEZL").is_none());
    }

    /// ⚠ **Die Folge, an der v1.7.14 zuerst gescheitert ist.**
    ///
    /// ```text
    /// EDDF angefordert
    /// 60-s-Timeout
    /// LEZL angefordert
    /// EDDF-Antwort trifft verspaetet ein
    /// ```
    ///
    /// Die verspaetete Antwort gehoert EDDF — nicht dem Auftrag, der
    /// gerade laeuft. Die erste Fassung des Umbaus beschriftete sie mit
    /// `laufender()` und haette sie als LEZL abgelegt: derselbe Fehler
    /// wie vorher, nur um die Wartezeit verschoben.
    #[test]
    fn eine_verspaetete_antwort_gehoert_ihrem_eigenen_platz() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        b.wunsch("LEZL");

        let erst = b.naechster(0).expect("erster Auftrag");
        let id_erst = b.gestellt(&erst, 0);

        // Wartezeit um, nichts gekommen — der naechste ist dran.
        let zweit = b.naechster(WARTEZEIT_MS).expect("zweiter Auftrag");
        assert_ne!(zweit, erst);
        let id_zweit = b.gestellt(&zweit, WARTEZEIT_MS);
        assert_ne!(id_erst, id_zweit, "beide Versuche teilen sich eine Kennung");
        assert_eq!(b.laufender().as_deref(), Some(zweit.as_str()));

        // JETZT kommt die Antwort des ERSTEN.
        let abgelegt = b
            .geliefert_zu_kennung(id_erst, auskunft(&erst, 4))
            .expect("Kennung unbekannt");
        assert_eq!(abgelegt, erst, "die alte Antwort wurde umbenannt");
        assert_eq!(b.auskunft(&erst).map(|a| a.bahnen.len()), Some(4));
        assert!(
            b.auskunft(&zweit).is_none(),
            "die verspaetete Antwort wurde dem laufenden Auftrag zugeschlagen"
        );
    }

    /// Eine Kennung, die das Buch nie vergeben hat, wird verworfen.
    ///
    /// ⚠ Nicht geraten. Ohne Zuordnung ist die einzig richtige Antwort
    /// „weg damit" — eine Auskunft unbekannter Herkunft ist schlimmer
    /// als keine.
    #[test]
    fn eine_unbekannte_kennung_wird_verworfen() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        let _ = b.gestellt("EDDF", 0);
        assert_eq!(b.geliefert_zu_kennung(9999, auskunft("EDDF", 4)), None);
        assert!(b.auskunft("EDDF").is_none());
    }

    /// Und die Rangfolge des Aufrufers ueberlebt die Ablage.
    ///
    /// ⚠ Ohne Rang entschied das Alphabet: EDDF kam vor LEZL, obwohl in
    /// Sevilla gelandet wird. Der alte Test pruefte nur die LISTE der
    /// Ziele, nicht die tatsaechliche Anfragefolge (QS-Befund 3).
    #[test]
    fn die_rangfolge_bestimmt_die_anfragefolge() {
        let mut b = Auftragsbuch::neu();
        // Ausweichziel (Rang 0) alphabetisch HINTER dem geplanten.
        b.wunsch_mit_rang("LEMG", 0);
        b.wunsch_mit_rang("EDDF", 1);
        assert_eq!(
            b.naechster(0).as_deref(),
            Some("LEMG"),
            "das Alphabet hat die Rangfolge ueberstimmt"
        );
    }

    /// Ein besserer Rang zieht einen vorhandenen Eintrag nach vorn.
    #[test]
    fn ein_ausweichziel_rueckt_vor() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("LEMG", 1); // erst als geplantes Ziel
        b.wunsch_mit_rang("EDDF", 1);
        b.wunsch_mit_rang("LEMG", 0); // dann als Ausweichziel
        assert_eq!(b.naechster(0).as_deref(), Some("LEMG"));
    }

    /// Die Diagnose gilt je Platz, nicht global.
    #[test]
    fn die_diagnose_gilt_je_platz() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        b.wunsch("LEZL");
        let id = b.gestellt("EDDF", 0);
        b.geliefert_zu_kennung(id, auskunft("EDDF", 4))
            .expect("Kennung");
        let _ = b.gestellt("LEZL", 1_000);

        assert_eq!(b.diagnose("EDDF"), "geliefert(EDDF, bahnen=4, rollwege=0)");
        assert_eq!(b.diagnose("LEZL"), "unterwegs(LEZL, versuch=1)");
        assert_eq!(b.diagnose("LEMG"), "nie_gefragt(LEMG)");

        b.abgelehnt("LEZL", "unbekannter Platz".into());
        assert_eq!(b.diagnose("LEZL"), "abgelehnt(LEZL, unbekannter Platz)");
    }

    /// Der Reihe nach: Wer weniger Versuche hat, kommt zuerst.
    #[test]
    fn der_seltener_gefragte_kommt_zuerst() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        let _ = b.gestellt("EDDF", 0);
        // EDDF hat einen Versuch, LEZL keinen.
        b.wunsch("LEZL");
        assert_eq!(b.naechster(WARTEZEIT_MS).as_deref(), Some("LEZL"));
    }
}
