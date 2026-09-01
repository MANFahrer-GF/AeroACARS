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
    /// Kennung des Versuchs, dessen LIEFERUNG gespeichert ist.
    ///
    /// ⚠ Ohne diese Zahl kann eine aeltere Antwort desselben Platzes
    /// eine neuere ueberschreiben:
    ///
    /// ```text
    /// LEZL Versuch 1 am Gate
    /// LEZL Versuch 2 im Anflug
    /// Versuch 2 liefert vollstaendige Daten
    /// Versuch 1 liefert verspaetet leere Daten   <- ueberschreibt
    /// ```
    ///
    /// Beide Kennungen zeigen RICHTIG auf LEZL — die Zuordnung stimmt
    /// also. Was fehlte, war die REIHENFOLGE (QS-Befund 1, zweite
    /// Runde). Dasselbe gilt fuer eine alte Ausnahme, die einen neueren
    /// Erfolg nachtraeglich auf „abgelehnt" zuruecksetzt.
    ergebnis_id: u32,
    /// Kennung des Versuchs, dessen FEHLSCHLAG festgehalten ist.
    ///
    /// ⚠ Getrennt von der Lieferung. Mit einer gemeinsamen Zahl
    /// entwertete ein spaeterer Fehlversuch eine aeltere, brauchbare
    /// Lieferung: Die Diagnose sagte „abgelehnt", `auskunft()` gab
    /// trotzdem Daten heraus, und eine danach eintreffende vollstaendige
    /// Lieferung wurde als „ueberholt" verworfen (QS-Befund 5, dritte
    /// Runde).
    fehler_id: u32,
}

/// Wie lange auf eine Antwort gewartet wird, bevor neu gefragt wird.
///
/// ⚠ Der Grund fuer diese ganze Klasse: Bei EDDF→LEZL am 01.09.2026 war
/// das Ziel **einmal** gefragt worden — am Gate in Frankfurt, 1.400 km
/// entfernt. Es kam keine Antwort, und niemand fragte je wieder. Beim
/// Aufsetzen lag deshalb die Szenerie des STARTflughafens vor, der
/// Vergleich fiel aus, und am Flug stand `auskunft_ohne_vergleich`.
pub const WARTEZEIT_MS: i64 = 60_000;

/// Wie oft ein Platz **je Abschnitt** hoechstens gefragt wird.
///
/// ⚠ **JE ABSCHNITT, nicht je Flug.** Der erste Entwurf zaehlte ueber
/// den ganzen Flug, und im Pruefstand stand dazu „zehn Versuche im
/// Minutentakt decken jeden Anflug ab". Das war falsch, und zwar
/// gefaehrlich falsch:
///
/// ```text
/// 03:50  Flugbeginn, Start/Ziel/Ausweich angemeldet
/// ~04:10 zehn Versuche fuer LEZL verbraucht — am Gate in Frankfurt
/// 07:29  Landung in Sevilla, nie wieder gefragt
/// ```
///
/// Der Vorrat war nach zwanzig Minuten am Boden aufgebraucht, 1.400 km
/// vom Ziel entfernt, und das spaetere Anmelden im Anflug aendert nur
/// den Rang — nicht die Versuchszahl. Damit waere GENAU der Fehler
/// zurueckgekommen, den diese Fassung behebt: am Gate zehnmal gefragt,
/// im Anflug kein einziges Mal (QS-Befund 1, dritte Runde, P0).
///
/// `neues_versuchsfenster` oeffnet den Vorrat neu — beim Eintritt in den
/// Anflug, wo die Szenerie des Ziels geladen ist.
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

/// Rang eines Platzes, der gerade kein Ziel ist.
///
/// Schlechter als jeder Rang, den die Ernte vergibt — er darf mitlaufen,
/// aber niemandem den Vortritt nehmen.
pub const RANG_UNBETEILIGT: u8 = 200;

/// Hoechste Auftragskennung, die noch vergeben wird.
///
/// ⚠ Der Abstand nach oben ist Absicht: Der Adapter bildet die
/// Anfragekennung als `BASIS + Auftragskennung` und die Basis ist 1000.
/// Ohne Reserve liefe SIE ueber, tausend Schritte bevor die
/// Auftragskennung es taete — und zwei Anfragen truegen dieselbe
/// Kennung.
///
/// `saturating_add` allein reichte nicht: Bei `u32::MAX` haette es
/// dieselbe Kennung unbegrenzt weiterverwendet, und `eroeffnen` haette
/// jedes Mal den Sammler der vorigen Anfrage ersetzt. Die Zusicherung
/// „die Vergabe steht still" war damit nicht wahr (QS-Befund 5, vierte
/// Runde).
pub const KENNUNG_HOECHST: u32 = u32::MAX - 2000;

#[derive(Debug, Clone, Default)]
pub struct Auftragsbuch {
    auftraege: std::collections::BTreeMap<String, Auftrag>,
    laeuft: Option<String>,
    /// Fortlaufende Kennung. Jeder VERSUCH bekommt eine eigene — nicht
    /// jeder Platz.
    naechste_id: u32,
    /// Gesetzt, wenn der Simulator ein Feld der Definition abgelehnt
    /// hat: (Feldname, Grund).
    ///
    /// ⚠ Ein harter Zustand. Das SDK meldet eine ungueltige
    /// Felddefinition als asynchronen `DATA_ERROR`; danach ist der
    /// gesamte Facility-Weg unbrauchbar, nicht nur ein Feld. Wer hier
    /// weiterfragt, sammelt Antworten, die niemand deuten kann.
    definition_fehler: Option<(String, String)>,
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
                ergebnis_id: 0,
                fehler_id: 0,
            });
    }

    /// Der naechste Platz, den der Verbindungsfaden fragen soll.
    ///
    /// Gibt nichts zurueck, solange eine Anfrage laeuft und die
    /// Wartezeit nicht um ist — SimConnect beantwortet immer nur eine.
    pub fn naechster(&mut self, jetzt_ms: i64) -> Option<String> {
        // ⚠ Ist die Felddefinition zurueckgewiesen, hat Fragen keinen
        // Sinn mehr — der Simulator kennt eines ihrer Felder nicht, und
        // JEDE Antwort waere unbrauchbar. Frueher wurde der Feldfehler
        // nur protokolliert und der Weg lief weiter: Auftraege meldeten
        // weiter „unterwegs" oder sogar „geliefert", obwohl die
        // Definition nachweislich abgelehnt war (QS-Befund 4, dritte
        // Runde).
        if self.definition_fehler.is_some() {
            return None;
        }
        // ⚠ Kennungen aufgebraucht: lieber nichts fragen als zwei
        // Anfragen mit derselben Kennung.
        if self.naechste_id >= KENNUNG_HOECHST {
            return None;
        }
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
            // ⚠ RANG ZUERST, dann die Versuchszahl.
            //
            // Vorher stand die Versuchszahl vorn. Dann verliert der
            // tatsaechliche Landeplatz (Rang 0, ein Versuch) gegen einen
            // beliebigen vorgemerkten Platz (Rang 200, null Versuche) —
            // und jeder Wiederholungsversuch fuer das Ziel wartet zwei
            // weitere Minuten, mit Start und Ausweichplatz noch laenger.
            // „Der tatsaechliche Platz zuerst" war damit keine
            // Zusicherung, sondern ein Wunsch (QS-Befund 3, dritte
            // Runde).
            .min_by_key(|(icao, a)| (a.rang, a.versuche, (*icao).clone()))
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
        // ⚠ `saturating_add`, nicht `wrapping_add`: Nach einem Umlauf
        // waere jeder Vergleich „neuer als" falsch, und eine uralte
        // Antwort gaelte als die juengste. Bei einem Versuch je Minute
        // wird die Grenze in vier Milliarden Minuten erreicht; laeuft
        // sie doch voll, steht die Vergabe still, statt falsch zu
        // ordnen (QS-Befund 5, dritte Runde, P3).
        self.naechste_id = self.naechste_id.saturating_add(1);
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

    /// Einen neuen Versuchsvorrat oeffnen — ohne Ergebnisse zu verlieren.
    ///
    /// ⚠ Fuer den Eintritt in den Anflug. Was schon geliefert ist,
    /// bleibt; was abgelehnt wurde, bleibt abgelehnt (ein Platz, den der
    /// Simulator nicht kennt, wird ihm durch Nachfragen nicht bekannt);
    /// nur die noch offenen Plaetze bekommen ihre Versuche zurueck.
    ///
    /// Gibt zurueck, fuer wie viele Plaetze das galt — damit die
    /// Aufrufstelle es protokollieren kann, statt es zu behaupten.
    pub fn neues_versuchsfenster(&mut self) -> usize {
        let mut betroffen = 0;
        for a in self.auftraege.values_mut() {
            if matches!(
                a.zustand,
                Auftragszustand::Offen | Auftragszustand::Laeuft { .. }
            ) {
                a.versuche = 0;
                a.zustand = Auftragszustand::Offen;
                betroffen += 1;
            }
        }
        self.laeuft = None;
        betroffen
    }

    /// Alles vergessen — neuer Flug, neue Verbindung, neuer Simulator.
    ///
    /// ⚠ Das Buch hatte die Lebensdauer des ADAPTERS: einmal angelegt,
    /// nie geleert. Damit ueberdauerten verbrauchte Versuche, dauerhafte
    /// Ablehnungen, gelieferte Szenerie eines frueheren Fluges und
    /// laengst belanglose Plaetze samt ihren Raengen. Nach einem
    /// Simulator-Neustart oder einem Wechsel zwischen MSFS 2020 und 2024
    /// galt alte Szenerie als „geliefert" und wurde nie erneut
    /// angefordert (QS-Befund 2, dritte Runde).
    ///
    /// Der Anfragezustand gehoert dem Flug und der Verbindung. Eine
    /// Datenablage darf laenger leben — dieses Buch ist keine.
    pub fn zuruecksetzen(&mut self) {
        self.auftraege.clear();
        self.kennungen.clear();
        self.laeuft = None;
        // ⚠ Der Definitionsfehler bleibt STEHEN.
        //
        // Die Felddefinition wird je VERBINDUNG registriert, nicht je
        // Flug. Loeschte ein Flugwechsel den Fehler, fragte dieselbe
        // Verbindung mit derselben abgelehnten Definition munter weiter
        // — ohne sie je neu registriert zu haben (QS-Befund 2, vierte
        // Runde). Ihn loescht nur `verbindung_zuruecksetzen`.
        // ⚠ `naechste_id` NICHT zuruecksetzen. Antworten des alten
        // Kontextes koennen noch unterwegs sein; wuerden die Kennungen
        // wieder bei 1 beginnen, traefe eine davon einen neuen Auftrag.
    }

    /// Alles vergessen, einschliesslich des Definitionsfehlers.
    ///
    /// ⚠ Nur bei einer NEUEN VERBINDUNG. Dort wird die Felddefinition
    /// neu registriert, also darf der alte Fehler nicht weiterwirken.
    /// Bei einem blossen Flugwechsel waere das falsch: Die Definition
    /// ist dieselbe, ihr Fehler auch.
    pub fn verbindung_zuruecksetzen(&mut self) {
        self.zuruecksetzen();
        self.definition_fehler = None;
    }

    /// Die Raenge der derzeit gueltigen Ziele SETZEN.
    ///
    /// ⚠ Setzen, nicht verbessern. `wunsch_mit_rang` kennt nur das
    /// Minimum — ein Platz, der einmal Rang 0 hatte, behielt ihn fuer
    /// immer. Wechselt das erkannte Ausweichziel, konkurrierte der alte
    /// Kandidat weiter mit dem aktuellen Landeplatz und verzoegerte
    /// dessen Versuche; `neues_versuchsfenster` weckte ihn zusaetzlich
    /// wieder auf (QS-Befund 4, vierte Runde).
    ///
    /// Plaetze, die nicht in der Liste stehen, fallen auf
    /// `RANG_UNBETEILIGT` zurueck. Ihre Ergebnisse bleiben unberuehrt —
    /// nur ihr Vorrang endet.
    pub fn raenge_setzen(&mut self, ziele: &[(String, u8)]) {
        for (icao, auftrag) in self.auftraege.iter_mut() {
            auftrag.rang = ziele
                .iter()
                .find(|(z, _)| z.eq_ignore_ascii_case(icao))
                .map(|(_, r)| *r)
                .unwrap_or(RANG_UNBETEILIGT);
        }
    }

    /// Der Simulator hat ein Feld der Definition abgelehnt.
    ///
    /// Danach ist der ganze Facility-Weg unbrauchbar: `naechster` gibt
    /// nichts mehr heraus, und die Diagnose sagt, welches Feld es war.
    pub fn definition_abgelehnt(&mut self, feld: String, grund: String) {
        self.definition_fehler = Some((feld, grund));
        self.laeuft = None;
    }

    /// Welches Feld der Definition abgelehnt wurde, falls eines.
    pub fn definitionsfehler(&self) -> Option<(String, String)> {
        self.definition_fehler.clone()
    }

    /// Nur fuer Tests: der Rang eines Platzes.
    #[doc(hidden)]
    pub fn rang_fuer_test(&self, icao: &str) -> Option<u8> {
        self.auftraege
            .get(&icao.trim().to_ascii_uppercase())
            .map(|a| a.rang)
    }

    /// Nur fuer Tests: den Kennungszaehler ans Ende setzen.
    ///
    /// ⚠ Ein Test, der bis dorthin zaehlt, laeuft vier Milliarden
    /// Schleifen — das war der erste Entwurf, und er blockierte den
    /// Testlauf. Ein Zugang ist hier ehrlicher als eine Schleife, die
    /// nie endet.
    #[doc(hidden)]
    pub fn kennung_setzen_fuer_test(&mut self, id: u32) {
        self.naechste_id = id;
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
        // ⚠ Reihenfolge, nicht Ankunft — aber NUR gegen andere
        // Lieferungen. Ein Fehlversuch darf eine brauchbare Lieferung
        // nicht abwehren, auch wenn er neuer ist.
        if self
            .auftraege
            .get(&icao)
            .is_some_and(|a| a.ergebnis_id > id)
        {
            return None;
        }
        self.geliefert(&icao, auskunft);
        if let Some(a) = self.auftraege.get_mut(&icao) {
            a.ergebnis_id = id;
        }
        Some(icao)
    }

    /// Eine Zurueckweisung ueber die Kennung festhalten.
    pub fn abgelehnt_zu_kennung(&mut self, id: u32, grund: String) -> Option<String> {
        let icao = self.platz_zu_kennung(id)?;
        // ⚠ Eine alte Ausnahme darf keinen neueren Fehlschlag
        // ueberschreiben — und KEINE Lieferung entwerten, egal wie alt.
        // Der Simulator meldet Zurueckweisungen asynchron; sie treffen
        // regelmaessig ein, nachdem ein anderer Versuch laengst geliefert
        // hat. Eine brauchbare Auskunft bleibt brauchbar.
        if self.auftraege.get(&icao).is_some_and(|a| a.fehler_id > id) {
            return None;
        }
        self.abgelehnt(&icao, grund);
        if let Some(a) = self.auftraege.get_mut(&icao) {
            a.fehler_id = id;
        }
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
            ergebnis_id: 0,
            fehler_id: 0,
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
            a.grund = Some(grund);
            // ⚠ Der Zustand faellt NUR, wenn nichts Brauchbares da ist.
            // Sonst behauptete die Diagnose „abgelehnt", waehrend
            // `auskunft()` weiter Daten herausgibt — zwei Aussagen ueber
            // denselben Platz, die sich widersprechen.
            if a.auskunft.is_none() {
                a.zustand = Auftragszustand::Abgelehnt;
            }
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
        // ⚠ Der Definitionsfehler schlaegt alles. Ein Platz kann nicht
        // „unterwegs" sein, wenn der Weg selbst zu ist.
        if let Some((feld, grund)) = &self.definition_fehler {
            return format!("definition_abgelehnt({feld}, {grund})");
        }
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

    /// ⚠ **Die Folge aus QS-Runde 2, Befund 1.**
    ///
    /// ```text
    /// LEZL Versuch 1 am Gate
    /// LEZL Versuch 2 im Anflug
    /// Versuch 2 liefert vollstaendige Daten
    /// Versuch 1 liefert verspaetet leere Daten
    /// ```
    ///
    /// Beide Kennungen zeigen RICHTIG auf LEZL — die Zuordnung aus
    /// Runde 1 stimmt also. Was fehlte, war die Reihenfolge: Die alte
    /// Antwort ueberschrieb die neue mit einem leeren Flughafen.
    #[test]
    fn eine_aeltere_antwort_ueberschreibt_keine_neuere() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let id1 = b.gestellt("LEZL", 0);
        let id2 = b.gestellt("LEZL", WARTEZEIT_MS);

        // Der zweite Versuch liefert vollstaendig.
        assert_eq!(
            b.geliefert_zu_kennung(id2, auskunft("LEZL", 2)).as_deref(),
            Some("LEZL")
        );
        // Der erste liefert verspaetet und LEER.
        assert_eq!(
            b.geliefert_zu_kennung(id1, auskunft("LEZL", 0)),
            None,
            "die alte Lieferung wurde angenommen"
        );
        assert_eq!(
            b.auskunft("LEZL").map(|a| a.bahnen.len()),
            Some(2),
            "die alte Lieferung hat die neue ueberschrieben"
        );
    }

    /// Und eine alte Ausnahme stuft einen neueren Erfolg nicht zurueck.
    ///
    /// ⚠ Der Simulator meldet eine Zurueckweisung asynchron. Sie kann
    /// eintreffen, nachdem der naechste Versuch laengst geliefert hat.
    #[test]
    fn eine_aeltere_ausnahme_stuft_keinen_erfolg_zurueck() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let id1 = b.gestellt("LEZL", 0);
        let id2 = b.gestellt("LEZL", WARTEZEIT_MS);

        b.geliefert_zu_kennung(id2, auskunft("LEZL", 2))
            .expect("Lieferung");

        // ⚠ Die Ausnahme wird FESTGEHALTEN — ihr Grund ist eine
        // Tatsache ueber ihren Versuch. Was sie NICHT darf: die
        // brauchbare Lieferung entwerten.
        b.abgelehnt_zu_kennung(id1, "zu spaet".into());
        assert_eq!(
            b.zustand("LEZL"),
            Some(Auftragszustand::Geliefert),
            "der Erfolg wurde nachtraeglich zurueckgestuft"
        );
        assert!(
            b.auskunft("LEZL").is_some(),
            "die Auskunft ist mit der Ausnahme verschwunden"
        );
        assert!(
            b.diagnose("LEZL").starts_with("geliefert("),
            "die Diagnose sagt abgelehnt, waehrend auskunft() Daten \
             herausgibt — zwei Aussagen, die sich widersprechen"
        );
    }

    /// ⚠ QS-Befund 5, dritte Runde: Erfolg und Fehler brauchen GETRENNTE
    /// Ordnung.
    ///
    /// Mit einer gemeinsamen Zahl setzte eine neuere Ausnahme sie hoch —
    /// und eine danach eintreffende, aeltere, aber VOLLSTAENDIGE
    /// Lieferung wurde als „ueberholt" verworfen. Der Flug haette dann
    /// gar keine Szenerie, obwohl eine brauchbare eingetroffen war.
    #[test]
    fn eine_ausnahme_wehrt_keine_lieferung_ab() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let id1 = b.gestellt("LEZL", 0);
        let id2 = b.gestellt("LEZL", WARTEZEIT_MS);

        // Der ZWEITE Versuch scheitert …
        b.abgelehnt_zu_kennung(id2, "abgelehnt".into());
        // … und der ERSTE liefert danach doch noch vollstaendig.
        assert_eq!(
            b.geliefert_zu_kennung(id1, auskunft("LEZL", 2)).as_deref(),
            Some("LEZL"),
            "die brauchbare Lieferung wurde von einem fremden Fehlversuch \
             abgewehrt"
        );
        assert_eq!(b.auskunft("LEZL").map(|a| a.bahnen.len()), Some(2));
        assert_eq!(b.zustand("LEZL"), Some(Auftragszustand::Geliefert));
    }

    /// Eine Ausnahme zum NEUESTEN Versuch gilt aber sehr wohl.
    ///
    /// ⚠ Sonst waere die Reihenfolge-Sperre ein Maulkorb: Ein Platz,
    /// den der Simulator wirklich nicht kennt, bliebe „unterwegs".
    #[test]
    fn eine_ausnahme_zum_neuesten_versuch_gilt() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("XXXX");
        let _ = b.gestellt("XXXX", 0);
        let id2 = b.gestellt("XXXX", WARTEZEIT_MS);
        assert_eq!(
            b.abgelehnt_zu_kennung(id2, "unbekannter Platz".into())
                .as_deref(),
            Some("XXXX")
        );
        assert_eq!(b.zustand("XXXX"), Some(Auftragszustand::Abgelehnt));
    }

    /// ⚠ **QS-Befund 1 der dritten Runde (P0): der Vorrat war am Gate
    /// verbraucht.**
    ///
    /// ```text
    /// 03:50  Flugbeginn, LEZL angemeldet
    /// ~04:10 zehn Versuche verbraucht — noch in Frankfurt
    /// 07:29  Landung in Sevilla
    /// ```
    ///
    /// Das spaetere Anmelden im Anflug aendert nur den RANG, nicht die
    /// Versuchszahl. Ohne ein neues Fenster waere genau der Fehler
    /// zurueck, den diese Fassung behebt: am Gate zehnmal gefragt, im
    /// Anflug kein einziges Mal.
    #[test]
    fn im_anflug_gibt_es_einen_neuen_versuchsvorrat() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let mut t = 0;
        for _ in 0..HOECHSTVERSUCHE {
            let a = b.naechster(t).expect("Versuch am Gate");
            let _ = b.gestellt(&a, t);
            t += WARTEZEIT_MS;
        }
        // Vorrat leer — und der Flug ist noch Stunden vom Ziel entfernt.
        assert_eq!(b.naechster(t), None);
        // Erneutes Anmelden hilft NICHT. Genau das war der Befund.
        b.wunsch_mit_rang("LEZL", 0);
        assert_eq!(
            b.naechster(t),
            None,
            "Anmelden setzt die Versuche zurueck — dann ist die Sperre wirkungslos"
        );

        // Eintritt in den Anflug: neues Fenster.
        assert_eq!(b.neues_versuchsfenster(), 1);
        assert_eq!(
            b.naechster(t).as_deref(),
            Some("LEZL"),
            "im Anflug wurde kein einziges Mal mehr gefragt"
        );
    }

    /// Ein neues Fenster verliert keine Ergebnisse.
    ///
    /// ⚠ Was geliefert ist, bleibt; was abgelehnt wurde, bleibt
    /// abgelehnt. Ein Platz, den der Simulator nicht kennt, wird ihm
    /// durch Nachfragen nicht bekannt.
    #[test]
    fn ein_neues_fenster_verliert_keine_ergebnisse() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        b.wunsch("XXXX");
        b.wunsch("LEZL");
        let id_eddf = b.gestellt("EDDF", 0);
        b.geliefert_zu_kennung(id_eddf, auskunft("EDDF", 4))
            .expect("Lieferung");
        let id_xxxx = b.gestellt("XXXX", 1_000);
        b.abgelehnt_zu_kennung(id_xxxx, "unbekannt".into())
            .expect("Ablehnung");

        assert_eq!(b.neues_versuchsfenster(), 1, "nur LEZL war offen");
        assert!(b.auskunft("EDDF").is_some());
        assert_eq!(b.zustand("XXXX"), Some(Auftragszustand::Abgelehnt));
        assert_eq!(b.naechster(2_000).as_deref(), Some("LEZL"));
    }

    /// ⚠ QS-Befund 2 der dritten Runde: Das Buch gehoert dem Flug und
    /// der Verbindung, nicht dem Adapter.
    ///
    /// Es wurde einmal angelegt und nie geleert. Nach einem
    /// Simulator-Neustart oder einem Wechsel zwischen MSFS 2020 und 2024
    /// galt die Szenerie des vorigen Fluges als „geliefert" und wurde
    /// nie erneut angefordert.
    #[test]
    fn ein_kontextwechsel_loescht_den_anfragezustand() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        let id = b.gestellt("EDDF", 0);
        b.geliefert_zu_kennung(id, auskunft("EDDF", 4))
            .expect("Lieferung");

        b.zuruecksetzen();

        assert!(b.auskunft("EDDF").is_none(), "alte Szenerie ueberlebte");
        assert_eq!(b.zustand("EDDF"), None, "alter Zustand ueberlebte");
        assert_eq!(b.versuche("EDDF"), 0);
        // ⚠ Der DEFINITIONSfehler gehoert nicht dem Flug, sondern der
        // Verbindung — siehe `ein_flugwechsel_behaelt_den_definitionsfehler`.
    }

    /// ⚠ QS-Befund 2 der vierten Runde: Der Definitionsfehler ueberlebt
    /// einen Flugwechsel — aber nicht eine neue Verbindung.
    ///
    /// Die Felddefinition wird je VERBINDUNG registriert. Loeschte ein
    /// Flugwechsel den Fehler, fragte dieselbe Verbindung mit derselben
    /// abgelehnten Definition weiter, ohne sie je neu registriert zu
    /// haben.
    #[test]
    fn ein_flugwechsel_behaelt_den_definitionsfehler() {
        let mut b = Auftragsbuch::neu();
        b.definition_abgelehnt("WIDTH".into(), "DATA_ERROR".into());

        b.zuruecksetzen(); // neuer Flug
        assert!(
            b.definitionsfehler().is_some(),
            "der Flugwechsel hat den Definitionsfehler geloescht — dieselbe \
             Verbindung fragt wieder mit derselben abgelehnten Definition"
        );
        b.wunsch("LEZL");
        assert_eq!(b.naechster(0), None, "es wird trotzdem gefragt");

        b.verbindung_zuruecksetzen(); // neue Verbindung, Definition neu
        assert!(b.definitionsfehler().is_none());
        b.wunsch("LEZL");
        assert_eq!(b.naechster(0).as_deref(), Some("LEZL"));
    }

    /// ⚠ QS-Befund 4 der vierten Runde: Raenge werden GESETZT, nicht nur
    /// verbessert.
    ///
    /// `wunsch_mit_rang` kennt nur das Minimum. Wechselt das erkannte
    /// Ausweichziel, behielte der alte Kandidat seinen Rang 0 und
    /// konkurrierte weiter mit dem aktuellen Landeplatz — und
    /// `neues_versuchsfenster` weckte ihn zusaetzlich wieder auf.
    /// ⚠ Der veraltete Kandidat steht ALPHABETISCH VORN — sonst prueft
    /// der Test nichts.
    ///
    /// Mein erster Entwurf nahm LEMG als Altlast und LEBL als neues
    /// Ziel. Damit gewinnt LEBL auch dann, wenn die Raenge nur
    /// verbessert statt gesetzt werden: Bei gleichem Rang entscheidet
    /// das Alphabet, und LEBL steht vor LEMG. Die Gegenprobe
    /// („Raenge nur verbessern") blieb prompt gruen.
    ///
    /// Mit EDDF als Altlast kann nur die Ruecksetzung auf
    /// `RANG_UNBETEILIGT` das richtige Ergebnis liefern.
    #[test]
    fn ein_alter_kandidat_verliert_seinen_vorrang() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDF", 0); // frueher erkanntes Ziel, alphabetisch vorn
        b.wunsch_mit_rang("LEZL", 1);

        // Das tatsaechliche Ziel ist jetzt LEZL, EDDF ist unbeteiligt.
        b.raenge_setzen(&[("LEZL".into(), 0)]);

        assert_eq!(
            b.naechster(0).as_deref(),
            Some("LEZL"),
            "der alte Kandidat hat den aktuellen Landeplatz verdraengt"
        );
        assert_eq!(b.rang_fuer_test("EDDF"), Some(RANG_UNBETEILIGT));
    }

    /// Und ein zurueckgestufter Platz verliert seine Ergebnisse NICHT.
    ///
    /// ⚠ Nur sein Vorrang endet. Wer eine gelieferte Szenerie beim
    /// Umsortieren wegwirft, muss sie neu holen — und hat im
    /// Zweifelsfall keine Versuche mehr dafuer.
    #[test]
    fn ein_zurueckgestufter_platz_behaelt_seine_auskunft() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("LEMG", 0);
        let id = b.gestellt("LEMG", 0);
        b.geliefert_zu_kennung(id, auskunft("LEMG", 1))
            .expect("Lieferung");

        b.raenge_setzen(&[("LEZL".into(), 0)]);
        assert!(b.auskunft("LEMG").is_some(), "die Auskunft ging verloren");
    }

    /// ⚠ QS-Befund 5 der vierten Runde: Am Ende des Zahlenraums steht
    /// die Vergabe WIRKLICH still.
    ///
    /// `saturating_add` allein haette dieselbe Kennung unbegrenzt
    /// weiterverwendet — und `Lieferungen::eroeffnen` haette jedes Mal
    /// den Sammler der vorigen Anfrage ersetzt.
    #[test]
    fn am_ende_des_zahlenraums_wird_nichts_mehr_vergeben() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        assert!(b.naechster(0).is_some(), "normal wird vergeben");

        b.kennung_setzen_fuer_test(KENNUNG_HOECHST);
        assert_eq!(
            b.naechster(0),
            None,
            "es werden weiter Kennungen vergeben — zwei Anfragen koennten \
             dieselbe tragen"
        );
    }

    /// ⚠ Aber die Kennungen laufen weiter.
    ///
    /// Antworten des alten Kontextes koennen noch unterwegs sein. Fingen
    /// die Kennungen wieder bei 1 an, traefe eine davon einen neuen
    /// Auftrag — und legte fremde Szenerie unter dessen Namen ab.
    #[test]
    fn nach_dem_ruecksetzen_beginnen_die_kennungen_nicht_neu() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        let alt = b.gestellt("EDDF", 0);
        b.zuruecksetzen();
        b.wunsch("LEZL");
        let neu = b.gestellt("LEZL", 1_000);
        assert!(neu > alt, "die Kennungen haben von vorn begonnen");
    }

    /// ⚠ QS-Befund 4 der dritten Runde: Ein abgelehntes Feld schliesst
    /// den ganzen Weg.
    ///
    /// Vorher wurde der Feldfehler nur protokolliert, und die
    /// Zustandsmaschine lief weiter — Auftraege meldeten „unterwegs"
    /// oder sogar „geliefert", obwohl die Definition nachweislich
    /// abgelehnt war.
    #[test]
    fn ein_abgelehntes_feld_schliesst_den_weg() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        assert!(b.naechster(0).is_some());

        b.definition_abgelehnt("WIDTH".into(), "DATA_ERROR".into());

        assert_eq!(
            b.naechster(WARTEZEIT_MS * 5),
            None,
            "es wird weiter gefragt, obwohl die Definition abgelehnt ist"
        );
        assert_eq!(
            b.diagnose("LEZL"),
            "definition_abgelehnt(WIDTH, DATA_ERROR)",
            "die Diagnose verschweigt den Definitionsfehler"
        );
        assert_eq!(
            b.definitionsfehler(),
            Some(("WIDTH".to_string(), "DATA_ERROR".to_string()))
        );
    }

    /// ⚠ QS-Befund 3 der dritten Runde: Der Rang steht VOR der
    /// Versuchszahl.
    ///
    /// Die frueheren Rangtests benutzten ueberall dieselbe Versuchszahl
    /// und sahen die Umkehrung deshalb nicht: Ein tatsaechliches Ziel
    /// mit Rang 0 und einem Versuch verlor gegen einen belanglosen
    /// vorgemerkten Platz mit Rang 200 und null Versuchen.
    #[test]
    fn der_rang_schlaegt_die_versuchszahl() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDF", 200); // Vormerkung, nie gefragt
        b.wunsch_mit_rang("LEZL", 0); // tatsaechliches Ziel
        let _ = b.gestellt("LEZL", 0); // hat damit EINEN Versuch mehr

        assert_eq!(
            b.naechster(WARTEZEIT_MS).as_deref(),
            Some("LEZL"),
            "ein belangloser Platz hat das tatsaechliche Ziel verdraengt"
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

    /// ⚠ QS-Befund 3 der zweiten Runde: mit der VORBELEGUNG aus dem
    /// echten Ablauf.
    ///
    /// Der fruehere Test begann mit frischen Eintraegen. Im Betrieb
    /// meldet der Flugbeginn aber Start, Ziel und Ausweichplatz
    /// GEMEINSAM an — die Stelle kennt die Rollen nicht. Trug sie alle
    /// mit Rang 0 ein, war die Rangfolge tot: `min` kann einen Rang nie
    /// verschlechtern, das geplante Ziel behielt seine 0, und bei
    /// gleicher Versuchszahl entschied wieder das Alphabet.
    ///
    /// Deshalb merkt die fruehe Anmeldung mit einem SCHLECHTEN Rang vor.
    #[test]
    fn die_vormerkung_neutralisiert_die_rangfolge_nicht() {
        let mut b = Auftragsbuch::neu();
        // Flugbeginn: alle drei gemeinsam, ohne Rollen.
        for platz in ["EDDF", "LEZL", "LEMG"] {
            b.wunsch_mit_rang(platz, 200);
        }
        // Die Ernte traegt jeden Durchlauf die richtige Rangfolge nach.
        b.wunsch_mit_rang("LEMG", 0); // Ausweichziel
        b.wunsch_mit_rang("LEZL", 1); // geplantes Ziel

        assert_eq!(
            b.naechster(0).as_deref(),
            Some("LEMG"),
            "die Vormerkung hat die Rangfolge ueberstimmt"
        );
    }

    /// ⚠ Und die Gegenprobe dazu: Mit Rang 0 vorgemerkt gewinnt das
    /// Alphabet — genau der gemeldete Fehler.
    #[test]
    fn eine_vormerkung_mit_rang_null_waere_der_fehler() {
        let mut b = Auftragsbuch::neu();
        for platz in ["EDDF", "LEZL", "LEMG"] {
            b.wunsch_mit_rang(platz, 0);
        }
        b.wunsch_mit_rang("LEMG", 0);
        b.wunsch_mit_rang("LEZL", 1);
        // EDDF steht alphabetisch vorn und hat immer noch Rang 0.
        assert_eq!(
            b.naechster(0).as_deref(),
            Some("EDDF"),
            "wenn das nicht mehr stimmt, ist die Vorbelegung anders — \
             dann gehoert dieser Test angepasst, nicht geloescht"
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
