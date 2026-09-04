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

/// Eine Parkposition (Gate/Rampe/Stand), so wie die Szenerie sie kennt —
/// vom Szenerie-Entwickler gepflegt, nicht aus OpenStreetMap.
///
/// # Warum das eine eigene, sim-agnostische Quelle ist
///
/// X-Plane traegt jede Rampenstart-Position in derselben `apt.dat`, aus
/// der auch die Bahnen kommen (Zeilencode `1300`). MSFS liefert sie ueber
/// dieselbe SimConnect-Facility-Schnittstelle wie Bahnen und Rollwege,
/// nur eine weitere Gruppe (`TAXI_PARKING`). Beide sind damit die ERSTE
/// Instanz — die tatsaechliche Szenerie, die der Pilot gerade sieht —,
/// nicht eine dritte, unabhaengige Karte wie OpenStreetMap.
///
/// Ein fehlender Name ist kein Fehler: eine Position ohne Namen zaehlt
/// bei der Naehe-Frage trotzdem als „an einem Stand", nur ohne
/// Beschriftung — dieselbe Regel wie bei OSM-Parkpositionen
/// (`stands::ParkingStand`).
#[derive(Debug, Clone, PartialEq)]
pub struct SzenerieStand {
    pub name: Option<String>,
    /// **(Breite, Länge)**.
    pub lat: f64,
    pub lon: f64,
}

/// Was ein Flughafen in der Szenerie hergibt.
#[derive(Debug, Clone, Default)]
pub struct SzenerieFlughafen {
    pub icao: String,
    pub bahnen: Vec<SzenerieBahn>,
    pub rollwege: Vec<SzenerieRollweg>,
    pub staende: Vec<SzenerieStand>,
    /// Woher die Angaben stammen — Dateipfad bei X-Plane, `"msfs"` bei
    /// MSFS. Steht im Bericht und in der Fehlersuche: Ein Add-on-Platz
    /// sieht anders aus als der globale.
    pub quelle: String,
}

// ⚠ Hier stand `SzenerieDiagnose` — eine ZWEITE Zustandsquelle neben
// dem Auftragsbuch. Sie ist ersatzlos gestrichen.
//
// Sie wurde an fuenf Stellen im Adapter getrennt fortgeschrieben und
// hatte einen oeffentlichen Getter. Beide Quellen widersprachen sich
// schon: Bei einem synchronen Fehler stand global „abgelehnt", im Buch
// `Wartet`; bei einer voruebergehenden Ausnahme blieb global
// „angefordert"; und bei mehreren Flughaefen beschrieb sie den ZULETZT
// bearbeiteten Platz, waehrend der Schnappschuss das Ernteziel meint.
//
// Der Flug las bereits den Schnappschuss, es entstand also kein
// falscher Bericht — aber die Aussage „der Riss kann nicht neu
// verdrahtet werden" stimmte nicht, solange der Getter den Rueckweg
// anbot (QS-Befund, zehnte Runde).
//
// Das Wort `ohne_bahnen` ist in `Auftragsbuch::diagnose` uebernommen,
// weil der Bestand danach durchsuchbar ist.

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
    /// Voruebergehend gescheitert — gesperrt bis `bis_ms`.
    ///
    /// ⚠ Ohne diesen Zustand verbrannte ein voruebergehender Fehler den
    /// ganzen Abschnittsvorrat in einer halben Sekunde: `freigeben`
    /// setzte den Auftrag sofort auf `Offen`, der Verteiler laeuft alle
    /// 50 ms, und nach zehn Durchlaeufen waren alle zehn Versuche weg
    /// (QS-Befund 1, siebte Runde, P0).
    ///
    /// Bei `TOO_MANY_REQUESTS` verschaerfte die Wiederholung sogar genau
    /// den Zustand, den sie beheben soll — die Ausnahme bedeutet, dass
    /// die Hoechstzahl gleichzeitiger Anfragen erreicht ist.
    Wartet { bis_ms: i64 },
    /// Der Abschnittsvorrat ist aufgebraucht.
    ///
    /// ⚠ Ein ECHTER Zustand, keine zweite Sicht auf `Wartet`.
    ///
    /// Der erste Entwurf liess den Eintrag auf `Wartet` stehen und
    /// liess nur `diagnose()` „erschoepft" behaupten. Damit sagten
    /// `zustand()` und `diagnose()` wieder Verschiedenes ueber denselben
    /// Auftrag — genau die Konstruktion, aus der in dieser Serie
    /// mehrfach ein Befund wurde. Und weil die Sonderregel auch `Laeuft`
    /// umfasste, galt schon der ZEHNTE, noch laufende Versuch als
    /// erschoepft, obwohl seine Antwort noch kommen konnte (QS-Befund 2,
    /// neunte Runde).
    ///
    /// Ein neues Anflugfenster setzt ihn wieder auf `Offen`.
    Erschoepft,
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

/// Alles, was der Abholer ueber einen Platz wissen muss — aus EINEM
/// Zugriff.
///
/// ⚠ Warum als Bündel und nicht als drei Getter:
///
/// 1. Der Abholer liest die alte Auskunft A aus Generation 1.
/// 2. Der Verbindungsfaden erhoeht auf Generation 2 und leert das Buch.
/// 3. Der Abholer liest Generation 2.
/// 4. Die Flugkopie wird auf Generation 2 entwertet.
/// 5. Und danach wird A wieder eingesetzt — scheinbar als Generation 2.
/// 6. Der naechste Durchlauf sieht keinen Wechsel mehr.
///
/// Genau die Auskunft, welche die Generation entwerten sollte, ersteht
/// dauerhaft wieder auf (QS-Befund 1, neunte Runde). Dasselbe gilt fuer
/// die Diagnose: Sie koennte aus einem anderen Kontext stammen als die
/// Auskunft, die sie beschreibt.
#[derive(Debug, Clone)]
pub struct Schnappschuss {
    pub generation: u32,
    /// Auskunft und ihr Stand — oder nichts.
    pub auskunft: Option<(SzenerieFlughafen, u32)>,
    pub diagnose: String,
    /// Womit sich der Simulator gemeldet hat.
    ///
    /// ⚠ Sie liegt IM BUCH, nicht in einem zweiten Mutex daneben.
    ///
    /// Vorher hielt der Adapter sie getrennt, und der Schnappschuss nahm
    /// zwei Sperren nacheinander. Ein Waechter belegte dann nur die
    /// REIHENFOLGE (Buch leeren vor Kennung leeren vor Registrierung) —
    /// nicht die Atomarität. Wer den Buch-Griff dazwischen fallen
    /// laesst, bekommt wieder neue Generation mit alter Kennung, und der
    /// Waechter bliebe gruen (QS-Befund 3, zwoelfte Runde).
    ///
    /// Ein Feld im Buch braucht keine Reihenfolge, die man bewachen
    /// muss.
    pub kennung: Option<String>,
}

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

/// Wie lange ein Platz nach einem voruebergehenden Fehler ruht.
///
/// ⚠ Kuerzer als `WARTEZEIT_MS`, weil die Anfrage gar nicht erst
/// hinausging oder sofort abgewiesen wurde — aber lang genug, dass zehn
/// Versuche ueber **fuenfundvierzig Sekunden** laufen statt ueber eine
/// halbe. Das ist der ganze Zweck: Der Verteiler laeuft alle 50 ms.
pub const RUECKZUG_MS: i64 = 5_000;

#[derive(Debug, Clone, Default)]
pub struct Auftragsbuch {
    auftraege: std::collections::BTreeMap<String, Auftrag>,
    // ⚠ Hier stand `laeuft: Option<String>` — der Platz, dessen Anfrage
    // laeuft. Er ist ERSATZLOS gestrichen.
    //
    // Er hielt dieselbe Tatsache ein zweites Mal: Der Zustand steht im
    // Eintrag (`Laeuft`), und er stand nochmal global. Aus dem Riss
    // zwischen beiden sind in sechs QS-Runden ZWEI Befunde entstanden —
    // „laufender() sagt niemand, diagnose() sagt unterwegs" und die nur
    // halb ausgefuehrte Freigabe. Jede Freigabestelle musste an zwei
    // Stellen denken, und jede neue vergass eine.
    //
    // `laufender()` liest den laufenden Auftrag jetzt aus den
    // Eintraegen. Es kann keinen Widerspruch mehr geben, weil es nur
    // noch eine Quelle gibt.
    /// Fortlaufende Kennung. Jeder VERSUCH bekommt eine eigene — nicht
    /// jeder Platz.
    naechste_id: u32,
    /// Womit sich der Simulator gemeldet hat — siehe `Schnappschuss`.
    kennung: Option<String>,
    /// Zaehlt jede neue Verbindung und jeden Definitionsfehler.
    ///
    /// ⚠ Die Standnummer allein reicht nicht: Sie loest nur den Fall,
    /// dass nach dem Wechsel tatsaechlich eine neue Lieferung eintrifft.
    /// Kommt keine — andere Simulatorfassung, gescheiterte
    /// Registrierung, abgelehnte Definition, stummer Zielplatz —, bleibt
    /// die Kopie der ALTEN Verbindung am Flug und wird beim Aufsetzen
    /// benutzt. Das Buch gibt in diesem Zustand bewusst `None` heraus;
    /// die bereits kopierte Auskunft umging den Riegel (QS-Befund 2,
    /// achte Runde).
    ///
    /// Die Generation macht den Wechsel sichtbar, auch ohne Lieferung.
    generation: u32,
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
        //
        // ⚠ Aus den EINTRAEGEN gelesen, nicht aus einem zweiten Zeiger.
        let laufend = self.auftraege.iter().find_map(|(icao, a)| match a.zustand {
            Auftragszustand::Laeuft { seit_ms } => Some((icao.clone(), seit_ms)),
            _ => None,
        });
        if let Some((icao, seit_ms)) = laufend {
            if jetzt_ms - seit_ms < WARTEZEIT_MS {
                return None;
            }
            // Wartezeit um: der Platz darf wieder in die Reihe — oder
            // ist erschoepft, wenn das sein letzter Versuch war.
            let neu = if self.versuche(&icao) >= HOECHSTVERSUCHE {
                Auftragszustand::Erschoepft
            } else {
                Auftragszustand::Offen
            };
            self.zustand_setzen(&icao, neu);
        }
        let kandidat = self
            .auftraege
            .iter()
            .filter(|(_, a)| a.versuche < HOECHSTVERSUCHE)
            .filter(|(_, a)| match a.zustand {
                Auftragszustand::Offen | Auftragszustand::Laeuft { .. } => true,
                // ⚠ Ruhend: erst nach Ablauf wieder Kandidat. Und NUR
                // dieser Platz ruht — andere duerfen weiter.
                Auftragszustand::Wartet { bis_ms } => jetzt_ms >= bis_ms,
                _ => false,
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
        // ⚠ Keinen Auftrag erfinden. Gibt es den Platz nicht (mehr),
        // darf auch keine Kennung und kein `laeuft` entstehen — sonst
        // traegt eine spaetere Lieferung einen Flughafen ein, den dieses
        // Buch nie gefragt hat.
        if !self.auftraege.contains_key(&icao) {
            return 0;
        }
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
        self.neues_versuchsfenster_mit_schutz(&[], &[])
    }

    /// Wie `neues_versuchsfenster`, aber ein Platz aus `geschuetzt` bleibt
    /// unberuehrt — auch ein LAUFENDER Auftrag fuer einen geschuetzten
    /// Platz wird NICHT zurueckgesetzt. AUSNAHME: steht derselbe Platz
    /// AUCH in `ziele`, gewinnt das Ziel — er bekommt sein Fenster wie
    /// jeder andere Anflug-Kandidat.
    ///
    /// ⚠ v1.7.17 Runde 2 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): `anflug_ausrichten_mit_schutz` (Runde 1) schuetzte nur
    /// die EINE Freigabestelle innerhalb dieser Funktion selbst — aber sie
    /// ruft am Ende, wenn `fenster` gesetzt ist, DIESE Funktion auf, die
    /// UNABHAENGIG davon JEDEN `Laeuft`-Auftrag zuruecksetzt, geschuetzt
    /// oder nicht. Bei jedem Eintritt in Sinkflug/Anflug/Endanflug haette
    /// ein gerade laufender Abflug-Auftrag also doch zurueckgesetzt werden
    /// koennen — derselbe Fehler wie in Runde 1, nur ueber den ZWEITEN
    /// Reset-Pfad statt den ersten. Zwei Reset-Mechanismen in derselben
    /// Funktion brauchen denselben Schutz, nicht nur einer davon.
    ///
    /// ⚠ v1.7.17 Runde 3 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): Der Runde-2-Schutz nahm `geschuetzt` allein — bei
    /// einem Rueckflug zum Startflughafen (oder jedem Flug, bei dem der
    /// Abflugplatz ZUGLEICH ein Anflug-Ziel ist, z. B. ein Ausweichziel)
    /// steht derselbe ICAO in BEIDEN Listen. Ohne Vorrang fuer `ziele`
    /// haette der Schutz einen echten Anflug-Kandidaten sein Fenster
    /// gekostet — genau das, was `raenge_setzen_mit_schutz` und die
    /// Freigabestelle in `anflug_ausrichten_mit_schutz` fuer ihre eigenen
    /// Mechanismen schon richtig machen (Ziel schlaegt Schutz). Dieselbe
    /// Rangordnung fehlte hier.
    ///
    /// ⚠ v1.7.17 Runde 4 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): Der Runde-3-Zielvorrang war zu grob — er setzte auch
    /// einen GERADE ECHT LAUFENDEN Auftrag (`Laeuft`) zurueck, sobald der
    /// Platz zugleich Ziel war. Genau das ist der Fehler, den Runde 1 fuer
    /// den reinen Schutzfall schon verhindert hat: eine echte, unterwegs
    /// befindliche SimConnect-Anfrage darf nicht doppelt gestellt werden,
    /// bevor ihre Antwort da ist.
    ///
    /// ⚠ v1.7.17 Runde 5 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): Runde 4 liess einen solchen Platz KOMPLETT unberuehrt
    /// — aber damit verlor er auch die Fenster-GUTSCHRIFT selbst. War die
    /// laufende Anfrage zufaellig schon der LETZTE erlaubte Versuch, galt
    /// sie bei einem spaeteren Fehlschlag sofort als erschoepft, ohne dass
    /// je ein neues Fenster fuer sie gewirkt haette — der eigentliche
    /// Landeplatz haette dann fuer den Rest des Anflugs ohne Szenerie
    /// dastehen koennen. Die Loesung trennt beide Anliegen: der ZUSTAND
    /// bleibt `Laeuft` (keine doppelte Anfrage), aber der VERSUCHSZAEHLER
    /// wird zurueckgesetzt — scheitert dieser Versuch danach, hat der
    /// Platz wieder ein volles Kontingent, statt sofort erschoepft zu
    /// gelten.
    pub fn neues_versuchsfenster_mit_schutz(
        &mut self,
        ziele: &[(String, u8)],
        geschuetzt: &[String],
    ) -> usize {
        let mut betroffen = 0;
        for (icao, a) in self.auftraege.iter_mut() {
            let ist_geschuetzt = geschuetzt.iter().any(|g| g.eq_ignore_ascii_case(icao));
            if ist_geschuetzt {
                let ist_ziel = ziele.iter().any(|(z, _)| z.eq_ignore_ascii_case(icao));
                if !ist_ziel {
                    continue;
                }
                if matches!(a.zustand, Auftragszustand::Laeuft { .. }) {
                    // Fenster-Gutschrift OHNE doppelte Anfrage — siehe
                    // Funktionsdoku (Runde 5).
                    a.versuche = 0;
                    betroffen += 1;
                    continue;
                }
            }
            // ⚠ Auch RUHENDE Plaetze. Ein Phasenwechsel ist ein echtes
            // Ereignis, kein Tick — die Ruhezeit aus einem
            // voruebergehenden Fehler darf ihn nicht ueberdauern.
            if matches!(
                a.zustand,
                Auftragszustand::Offen
                    | Auftragszustand::Laeuft { .. }
                    | Auftragszustand::Wartet { .. }
                    | Auftragszustand::Erschoepft
            ) {
                a.versuche = 0;
                a.zustand = Auftragszustand::Offen;
                betroffen += 1;
            }
        }
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
        // ⚠ Die Kennung gehoert der Verbindung — sie faellt MIT, und
        // zwar unter derselben Sperre. Genau darum liegt sie hier.
        self.kennung = None;
        self.generation = self.generation.saturating_add(1);
    }

    /// Die laufende Generation — siehe `generation`.
    pub fn generation(&self) -> u32 {
        self.generation
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
        self.raenge_setzen_mit_schutz(ziele, &[]);
    }

    /// Wie `raenge_setzen`, aber `geschuetzt` behaelt seinen Rang statt auf
    /// `RANG_UNBETEILIGT` zu fallen.
    ///
    /// ⚠ v1.7.17: Das Buch traegt seit der Abflug-Ernte AUCH Auftraege,
    /// die gar keine Anflug-Kandidaten sind (der Abflugplatz). Ohne diesen
    /// Schutz setzte JEDER Anflug-Tick — und der laeuft ab dem ERSTEN
    /// Tick des Fluges, nicht erst ab dem Sinkflug — den Abflug-Rang
    /// zurueck auf 200, noch bevor die Abflug-Ernte ihn auf 0 verbessern
    /// konnte: eine Verbesserung, die im selben Tick-Durchlauf schon
    /// wieder rueckgaengig gemacht wurde (externe Gegenpruefung, Codex,
    /// adversarial, 04.09.2026).
    pub fn raenge_setzen_mit_schutz(&mut self, ziele: &[(String, u8)], geschuetzt: &[String]) {
        for (icao, auftrag) in self.auftraege.iter_mut() {
            if let Some((_, r)) = ziele.iter().find(|(z, _)| z.eq_ignore_ascii_case(icao)) {
                auftrag.rang = *r;
            } else if geschuetzt.iter().any(|g| g.eq_ignore_ascii_case(icao)) {
                // Rang unveraendert — dieser Platz gehoert nicht zu DIESER
                // Zielliste, ist aber anderswo aktiv verwaltet (siehe Doku
                // oben).
            } else {
                auftrag.rang = RANG_UNBETEILIGT;
            }
        }
    }

    /// Der Simulator hat ein Feld der Definition abgelehnt.
    ///
    /// Danach ist der ganze Facility-Weg unbrauchbar: `naechster` gibt
    /// nichts mehr heraus, und die Diagnose sagt, welches Feld es war.
    pub fn definition_abgelehnt(&mut self, feld: String, grund: String) {
        self.definition_fehler = Some((feld, grund));
        // ⚠ Auch das ist ein Generationswechsel: Was unter dieser
        // Definition entstanden ist, gilt nicht mehr — auch das, was
        // schon am Flug liegt.
        self.generation = self.generation.saturating_add(1);
        // ⚠ Auch den laufenden Auftrag beenden — sonst haelt er die
        // Reihe, obwohl nie wieder gefragt wird.
        if let Some(icao) = self.laufender() {
            self.zustand_setzen(&icao, Auftragszustand::Offen);
        }
    }

    /// Welches Feld der Definition abgelehnt wurde, falls eines.
    pub fn definitionsfehler(&self) -> Option<(String, String)> {
        self.definition_fehler.clone()
    }

    /// Auswaehlen UND stellen in EINEM Zug.
    ///
    /// ⚠ `naechster` und `gestellt` getrennt aufzurufen ist ein Riss:
    /// Der Aufrufer nimmt die Sperre zweimal, und dazwischen kann ein
    /// Flugwechsel das Buch leeren. `gestellt` legte dann trotzdem
    /// Kennung und `laeuft` an — fuer einen Auftrag, den es nicht mehr
    /// gibt. Eine spaetere Lieferung setzte den Flughafen des ALTEN
    /// Fluges ueber `geliefert` wieder ins neue Buch (QS-Befund 5,
    /// fuenfte Runde).
    pub fn naechsten_stellen(&mut self, jetzt_ms: i64) -> Option<(String, u32)> {
        let icao = self.naechster(jetzt_ms)?;
        let id = self.gestellt(&icao, jetzt_ms);
        Some((icao, id))
    }

    /// Fenster, Zielmenge und Raenge in EINEM Zug setzen.
    ///
    /// ⚠ Vorher waren das drei Buchoperationen mit drei getrennten
    /// Sperren: Fenster oeffnen, Ziele einzeln anmelden, danach die
    /// Raenge korrigieren. Dazwischen konnte der Verbindungsfaden noch
    /// einen veralteten Rang-0-Platz auswaehlen — und der lief dann bis
    /// zu 60 Sekunden, weil die spaetere Rangkorrektur einen LAUFENDEN
    /// Auftrag nicht beendet. Im Endanflug ist das der restliche Anflug
    /// (QS-Befund 2, fuenfte Runde).
    ///
    /// Gibt zurueck, fuer wie viele Plaetze ein neuer Vorrat geoeffnet
    /// wurde.
    pub fn anflug_ausrichten(&mut self, ziele: &[(String, u8)], fenster: bool) -> usize {
        self.anflug_ausrichten_mit_schutz(ziele, &[], fenster)
    }

    /// Wie `anflug_ausrichten`, aber `geschuetzt` wird von BEIDEN
    /// Aufraeumschritten ausgenommen: der Rang faellt nicht auf
    /// `RANG_UNBETEILIGT` zurueck (siehe `raenge_setzen_mit_schutz`), und
    /// ein laufender Auftrag fuer einen geschuetzten Platz wird NICHT
    /// vorzeitig auf `Offen` zurueckgesetzt, nur weil er kein
    /// Anflug-Ziel ist.
    ///
    /// ⚠ v1.7.17: Ohne die zweite Haelfte haette der Abflug-Auftrag zwar
    /// seinen Rang behalten, waere aber trotzdem JEDEN Anflug-Tick
    /// vorzeitig aus `Laeuft` gerissen worden, sobald er — wie jeder
    /// andere Platz auch — kein Anflug-Ziel ist. Das haette dieselbe
    /// Wirkung wie die Rangzuruecksetzung gehabt: eine echte, laufende
    /// SimConnect-Anfrage koennte doppelt gestellt werden, bevor ihre
    /// Antwort da ist (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026).
    pub fn anflug_ausrichten_mit_schutz(
        &mut self,
        ziele: &[(String, u8)],
        geschuetzt: &[String],
        fenster: bool,
    ) -> usize {
        for (icao, rang) in ziele {
            self.wunsch_mit_rang(icao, *rang);
        }
        self.raenge_setzen_mit_schutz(ziele, geschuetzt);
        // ⚠ Einen laufenden Auftrag, der kein Ziel mehr ist UND nicht
        // geschuetzt, SOFORT freigeben. Sonst blockiert er die Reihe, bis
        // die Wartezeit um ist — und genau die 60 Sekunden fehlen dann
        // dem Ziel.
        if let Some(laufend) = self.laufender() {
            let ist_ziel = ziele.iter().any(|(z, _)| z.eq_ignore_ascii_case(&laufend));
            let ist_geschuetzt = geschuetzt.iter().any(|g| g.eq_ignore_ascii_case(&laufend));
            if !ist_ziel && !ist_geschuetzt {
                self.zustand_setzen(&laufend, Auftragszustand::Offen);
            }
        }
        if fenster {
            // ⚠ Runde 2: die geschuetzte Variante — siehe deren Doku, warum
            // die ungeschuetzte hier derselbe Fehler waere, nur ueber den
            // zweiten Reset-Pfad. Runde 3: `ziele` MIT uebergeben, sonst
            // verliert ein Abflugplatz, der zugleich Anflug-Ziel ist
            // (Rueckflug/Ausweichziel), sein Fenster an den Schutz.
            self.neues_versuchsfenster_mit_schutz(ziele, geschuetzt)
        } else {
            0
        }
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
        if id == 0 {
            return None;
        }
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
        self.auftraege
            .iter()
            .find(|(_, a)| matches!(a.zustand, Auftragszustand::Laeuft { .. }))
            .map(|(icao, _)| icao.clone())
    }

    /// Einen Auftrag beenden und in einen neuen Zustand ueberfuehren.
    ///
    /// ⚠ ALLE Freigabewege gehen hier durch — voruebergehender Fehler,
    /// veraltetes Ziel, neues Versuchsfenster, Lieferung, Ablehnung.
    /// Vorher hatte jeder seine eigene Fassung, und jede musste an den
    /// Eintrag UND an den globalen Zeiger denken.
    fn zustand_setzen(&mut self, icao: &str, neu: Auftragszustand) {
        if let Some(a) = self.auftraege.get_mut(icao) {
            a.zustand = neu;
        }
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

    /// Einen laufenden Auftrag wieder freigeben, ohne ihn abzulehnen.
    ///
    /// ⚠ Fuer voruebergehende Fehler. Der Platz bleibt offen und kommt
    /// wieder an die Reihe; nur die laufende Anfrage gilt als beendet,
    /// damit nicht die volle Wartezeit verstreicht.
    pub fn freigeben_zu_kennung(&mut self, id: u32, jetzt_ms: i64) -> Option<String> {
        let icao = self.platz_zu_kennung(id)?;
        // ⚠ NUR die neueste Kennung darf freigeben.
        //
        // Kommt die Ausnahme zu Versuch 1 erst an, waehrend Versuch 2
        // laeuft, setzte sie dessen Zustand auf `Offen` und loeschte den
        // laufenden Auftrag — Versuch 3 begaenne sofort, waehrend
        // Versuch 2 noch unterwegs ist (QS-Befund 3, sechste Runde).
        if self.auftraege.get(&icao).is_some_and(|a| a.letzte_id != id) {
            return None;
        }
        // ⚠ NICHT `Offen`. Sonst kommt der Platz beim naechsten
        // Verteilerdurchlauf sofort wieder dran — 50 ms spaeter. Ein
        // ruhender Platz haelt dabei KEINEN anderen auf: `naechster`
        // sucht den laufenden Auftrag, und ruhend ist nicht laufend.
        if matches!(
            self.auftraege.get(&icao).map(|a| &a.zustand),
            Some(Auftragszustand::Laeuft { .. })
        ) {
            // ⚠ War das der letzte Versuch, ist der Vorrat WEG — und der
            // Zustand sagt das auch. Eine Ruhezeit, nach der nie wieder
            // gefragt wird, waere eine Luege.
            let neu = if self.versuche(&icao) >= HOECHSTVERSUCHE {
                Auftragszustand::Erschoepft
            } else {
                Auftragszustand::Wartet {
                    bis_ms: jetzt_ms + RUECKZUG_MS,
                }
            };
            self.zustand_setzen(&icao, neu);
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
    }

    /// Die Auskunft zu genau diesem Platz — oder nichts.
    ///
    /// ⚠ Kein Rueckfall auf "irgendeine". Die Auskunft des
    /// Startflughafens fuer das Ziel auszugeben war der Fehler, der
    /// diese Klasse ausgeloest hat.
    pub fn auskunft(&self, icao: &str) -> Option<&SzenerieFlughafen> {
        // ⚠ Ist die Definition abgelehnt, wird NICHTS mehr herausgegeben
        // — auch nichts, was vorher eintraf.
        //
        // Die Sperre schloss bisher nur die Anfragen. Eine Auskunft, die
        // unter einer nachweislich ungueltigen Definition entstanden ist,
        // hat ein anderes Raster als erwartet; sie ist nicht „etwas
        // besser als nichts", sondern falsch (QS-Befund 2, sechste
        // Runde).
        if self.definition_fehler.is_some() {
            return None;
        }
        self.auftraege
            .get(&icao.trim().to_ascii_uppercase())
            .and_then(|a| a.auskunft.as_ref())
    }

    /// Generation, Auskunft, Stand und Diagnose — EIN Zugriff.
    ///
    /// ⚠ Der einzige Weg fuer den Abholer. Siehe `Schnappschuss`.
    pub fn schnappschuss(&self, icao: &str) -> Schnappschuss {
        Schnappschuss {
            generation: self.generation,
            auskunft: self.auskunft_mit_stand(icao),
            diagnose: self.diagnose(icao),
            kennung: self.kennung.clone(),
        }
    }

    /// Womit sich der Simulator gemeldet hat.
    pub fn kennung_setzen(&mut self, kennung: Option<String>) {
        self.kennung = kennung;
    }

    /// Auskunft UND Stand in einem Zug.
    ///
    /// ⚠ Getrennt gelesen ist das ein Riss — und zwar genau der, den
    /// dieses Blatt als Fehlerklasse fuehrt:
    ///
    /// 1. alte Auskunft wird gelesen,
    /// 2. das Buch speichert eine neue mit Stand 2,
    /// 3. der Abholer liest Stand 2,
    /// 4. die ALTE Auskunft liegt am Flug und traegt Stand 2,
    /// 5. die echte neue wird spaeter wegen gleichen Standes abgewiesen.
    ///
    /// Der Flug behaelt die alte Auskunft dauerhaft, und niemand sieht
    /// es (QS-Befund 1, achte Runde).
    pub fn auskunft_mit_stand(&self, icao: &str) -> Option<(SzenerieFlughafen, u32)> {
        let icao = icao.trim().to_ascii_uppercase();
        if self.definition_fehler.is_some() {
            return None;
        }
        let a = self.auftraege.get(&icao)?;
        a.auskunft.as_ref().map(|x| (x.clone(), a.ergebnis_id))
    }

    /// Der Stand der gespeicherten Lieferung — die Kennung des Versuchs,
    /// aus dem sie stammt.
    ///
    /// ⚠ Damit kann der Abnehmer entscheiden, ob seine Kopie veraltet
    /// ist. Vorher kopierte er nur, wenn noch GAR NICHTS mit diesem
    /// ICAO am Flug lag — eine neuere Lieferung desselben Platzes kam
    /// nie an, und nach einem Verbindungswechsel blieb die alte Kopie
    /// stehen, obwohl das Buch frisch abgefragt hatte (QS-Befund 2,
    /// siebte Runde).
    pub fn lieferungsstand(&self, icao: &str) -> u32 {
        self.auftraege
            .get(&icao.trim().to_ascii_uppercase())
            .map(|a| a.ergebnis_id)
            .unwrap_or(0)
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
            // ⚠ Keine Sonderregel mehr. `Erschoepft` ist ein echter
            // Zustand; die Diagnose liest ihn, statt ihn abzuleiten.
            Some(a) => match &a.zustand {
                Auftragszustand::Offen => format!("angemeldet({icao_gross})"),
                Auftragszustand::Laeuft { .. } => {
                    format!("unterwegs({icao_gross}, versuch={})", a.versuche)
                }
                Auftragszustand::Wartet { bis_ms } => {
                    format!("ruht({icao_gross}, bis={bis_ms}, versuch={})", a.versuche)
                }
                Auftragszustand::Erschoepft => {
                    format!("erschoepft({icao_gross}, versuche={})", a.versuche)
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
                    // ⚠ „ohne_bahnen" bleibt ein EIGENES Wort.
                    //
                    // Es traegt Bedeutung aus der Untersuchung vom
                    // 29.08.2026: Zwei MSFS-2024-Fluege meldeten es, die
                    // Antwort kam also an — nur ohne eine einzige Bahn.
                    // Ob dabei ROLLWEGE ankamen, entschied, wo der
                    // Fehler sitzt. Der Bestand ist danach durchsuchbar;
                    // das Wort einfach in „geliefert" aufgehen zu lassen
                    // wuerde diese Suche brechen.
                    if bahnen == 0 {
                        format!("ohne_bahnen({icao_gross}, rollwege={rollwege})")
                    } else {
                        format!("geliefert({icao_gross}, bahnen={bahnen}, rollwege={rollwege})")
                    }
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
            staende: Vec::new(),
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
    /// ⚠ QS-Befund 2, neunte Runde: Der ZEHNTE, noch laufende Versuch
    /// ist nicht erschoepft.
    ///
    /// Die erste Fassung leitete „erschoepft" nur in der Diagnose ab —
    /// und schloss `Laeuft` mit ein. Direkt nach dem Absenden galt
    /// deshalb `zustand = Laeuft`, `diagnose = erschoepft`, obwohl die
    /// Antwort noch kommen konnte.
    #[test]
    fn der_letzte_laufende_versuch_ist_noch_unterwegs() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let mut t = 0i64;
        let mut letzte_id = 0;
        for _ in 0..HOECHSTVERSUCHE {
            let (_, id) = b.naechsten_stellen(t).expect("Versuch");
            letzte_id = id;
            if b.versuche("LEZL") < HOECHSTVERSUCHE {
                b.freigeben_zu_kennung(id, t);
            }
            t += RUECKZUG_MS;
        }
        // Der zehnte laeuft noch.
        assert_eq!(b.versuche("LEZL"), HOECHSTVERSUCHE);
        assert!(
            matches!(b.zustand("LEZL"), Some(Auftragszustand::Laeuft { .. })),
            "der zehnte Versuch laeuft nicht mehr: {:?}",
            b.zustand("LEZL")
        );
        assert!(
            b.diagnose("LEZL").starts_with("unterwegs("),
            "der noch laufende Versuch gilt als erschoepft: {}",
            b.diagnose("LEZL")
        );

        // ERST nach seinem Fehlschlag ist der Vorrat weg.
        b.freigeben_zu_kennung(letzte_id, t);
        assert_eq!(b.zustand("LEZL"), Some(Auftragszustand::Erschoepft));
        assert!(b.diagnose("LEZL").starts_with("erschoepft("));

        // Und ein neues Anflugfenster macht ihn wieder frei.
        assert_eq!(b.neues_versuchsfenster(), 1);
        assert_eq!(b.zustand("LEZL"), Some(Auftragszustand::Offen));
        assert_eq!(b.versuche("LEZL"), 0);
    }

    /// Auch ein AUSGELAUFENER letzter Versuch endet in `Erschoepft`.
    ///
    /// ⚠ Nicht nur der Fehlschlag: Kommt gar keine Antwort, laeuft die
    /// Wartezeit ab — und danach darf der Eintrag nicht auf `Offen`
    /// stehen, als koennte noch einmal gefragt werden.
    #[test]
    fn ein_ausgelaufener_letzter_versuch_ist_erschoepft() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let mut t = 0i64;
        for _ in 0..HOECHSTVERSUCHE {
            b.naechsten_stellen(t).expect("Versuch");
            t += WARTEZEIT_MS;
        }
        // Der letzte laeuft ins Leere.
        assert_eq!(b.naechsten_stellen(t + WARTEZEIT_MS), None);
        assert_eq!(b.zustand("LEZL"), Some(Auftragszustand::Erschoepft));
    }

    /// ⚠ QS-Befund, zehnte Runde: Es gibt nur EINE Diagnosequelle.
    ///
    /// Neben dem Buch lief eine globale `SzenerieDiagnose` mit, an fuenf
    /// Stellen getrennt fortgeschrieben und mit oeffentlichem Getter.
    /// Beide widersprachen sich schon: synchroner Fehler → global
    /// „abgelehnt", im Buch `Wartet`; voruebergehende Ausnahme → global
    /// blieb „angefordert"; bei mehreren Plaetzen beschrieb sie den
    /// ZULETZT bearbeiteten, der Schnappschuss aber das Ernteziel.
    ///
    /// Sie ist gestrichen. Was bleibt, ist dieser Test: Die Diagnose
    /// nennt IMMER den gefragten Platz.
    #[test]
    fn die_diagnose_nennt_immer_den_gefragten_platz() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        b.wunsch("LEZL");
        let (erst, id) = b.naechsten_stellen(0).expect("Auftrag");
        b.geliefert_zu_kennung(id, auskunft(&erst, 4))
            .expect("Lieferung");

        // ⚠ Der ANDERE Platz darf nicht die Diagnose des ersten erben.
        let anderer = if erst == "EDDF" { "LEZL" } else { "EDDF" };
        assert!(
            b.diagnose(anderer).contains(anderer),
            "die Diagnose von {anderer} nennt einen fremden Platz: {}",
            b.diagnose(anderer)
        );
        assert!(
            !b.diagnose(anderer).starts_with("geliefert("),
            "der ungefragte Platz gilt als geliefert"
        );
    }

    /// ⚠ Und `ohne_bahnen` bleibt ein eigenes Wort.
    ///
    /// Es traegt Bedeutung aus der Untersuchung vom 29.08.2026: Die
    /// Antwort KAM an, nur ohne eine einzige Bahn — und ob dabei
    /// Rollwege ankamen, entschied, wo der Fehler sitzt. Der Bestand ist
    /// nach diesem Wort durchsuchbar.
    #[test]
    fn eine_lieferung_ohne_bahnen_heisst_weiter_ohne_bahnen() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LKTB");
        let (_, id) = b.naechsten_stellen(0).expect("Auftrag");
        let mut leer = auskunft("LKTB", 0);
        leer.rollwege = (0..243)
            .map(|_| SzenerieRollweg {
                name: "A".to_string(),
                punkte: Vec::new(),
            })
            .collect();
        b.geliefert_zu_kennung(id, leer).expect("Lieferung");

        let d = b.diagnose("LKTB");
        assert!(d.starts_with("ohne_bahnen("), "die Meldung lautet: {d}");
        assert!(d.contains("LKTB"), "der Platz fehlt: {d}");
        assert!(d.contains("243"), "die Rollwegzahl fehlt: {d}");
    }

    /// ⚠ QS-Befund 1, neunte Runde: EIN Schnappschuss statt drei Getter.
    #[test]
    fn der_schnappschuss_ist_in_sich_stimmig() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let (_, id) = b.naechsten_stellen(0).expect("Versuch");
        b.geliefert_zu_kennung(id, auskunft("LEZL", 2))
            .expect("Lieferung");

        let s = b.schnappschuss("LEZL");
        let (a, stand) = s.auskunft.clone().expect("Auskunft");
        assert_eq!(a.bahnen.len(), 2);
        assert_eq!(stand, id);
        assert_eq!(s.generation, b.generation());
        assert!(s.diagnose.starts_with("geliefert("));

        // Nach einem Verbindungswechsel: neue Generation, KEINE Auskunft
        // — und beides aus demselben Zugriff.
        b.verbindung_zuruecksetzen();
        let s2 = b.schnappschuss("LEZL");
        assert!(s2.generation > s.generation);
        assert!(
            s2.auskunft.is_none(),
            "die alte Auskunft haengt an der neuen Generation"
        );
        assert!(s2.diagnose.starts_with("nie_gefragt("));
    }

    /// ⚠ QS-Befund 1, achte Runde: Auskunft und Stand aus EINEM Zugriff.
    ///
    /// Getrennt gelesen kann dazwischen eine neue Lieferung eintreffen —
    /// dann traegt die ALTE Auskunft den NEUEN Stand, und die echte neue
    /// wird spaeter wegen gleichen Standes abgewiesen.
    #[test]
    fn auskunft_und_stand_gehoeren_zusammen() {
        // ⚠ BEIDE Versuche stellen, BEVOR einer liefert — ein
        // gelieferter Platz wird nicht erneut gefragt. Genau so
        // entstehen im Betrieb zwei ausstehende Antworten desselben
        // Platzes.
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let (_, id1) = b.naechsten_stellen(0).expect("Versuch 1");
        let (_, id2) = b.naechsten_stellen(WARTEZEIT_MS).expect("Versuch 2");

        b.geliefert_zu_kennung(id1, auskunft("LEZL", 1))
            .expect("Lieferung 1");
        let (a1, s1) = b.auskunft_mit_stand("LEZL").expect("Auskunft 1");
        assert_eq!(a1.bahnen.len(), 1);

        b.geliefert_zu_kennung(id2, auskunft("LEZL", 3))
            .expect("Lieferung 2");
        let (a2, s2) = b.auskunft_mit_stand("LEZL").expect("Auskunft 2");

        assert!(s2 > s1, "der Stand waechst nicht");
        assert_eq!(
            a2.bahnen.len(),
            3,
            "der neue Stand gehoert zur ALTEN Auskunft"
        );
    }

    /// ⚠ QS-Befund 2, achte Runde: Der Wechsel ist auch OHNE Lieferung
    /// sichtbar.
    #[test]
    fn ein_verbindungswechsel_erhoeht_die_generation() {
        let mut b = Auftragsbuch::neu();
        let g0 = b.generation();
        b.zuruecksetzen();
        assert_eq!(
            b.generation(),
            g0,
            "ein Flugwechsel ist keine neue Verbindung"
        );
        b.verbindung_zuruecksetzen();
        assert!(b.generation() > g0, "die Generation blieb stehen");
    }

    /// Und ein Definitionsfehler ebenso — was unter einer ungueltigen
    /// Definition entstanden ist, gilt nicht mehr.
    #[test]
    fn ein_definitionsfehler_erhoeht_die_generation() {
        let mut b = Auftragsbuch::neu();
        let g0 = b.generation();
        b.definition_abgelehnt("WIDTH".into(), "DATA_ERROR".into());
        assert!(b.generation() > g0);
        assert!(b.auskunft_mit_stand("LEZL").is_none());
    }

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

    /// ⚠ QS-Befund 2, fuenfte Runde: Fenster, Ziele und Raenge in EINEM
    /// Zug — und ein laufender Auftrag, der kein Ziel mehr ist, wird
    /// sofort freigegeben.
    ///
    /// Vorher waren das drei Buchoperationen. Dazwischen konnte der
    /// Verbindungsfaden einen veralteten Rang-0-Platz auswaehlen; die
    /// spaetere Rangkorrektur beendet einen LAUFENDEN Auftrag nicht, und
    /// er blockierte die Reihe bis zu 60 Sekunden. Im Endanflug ist das
    /// der restliche Anflug.
    #[test]
    fn das_ausrichten_gibt_einen_veralteten_auftrag_sofort_frei() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDF", 0);
        let (laufend, _) = b.naechsten_stellen(0).expect("erster Auftrag");
        assert_eq!(laufend, "EDDF");
        assert_eq!(b.laufender().as_deref(), Some("EDDF"));

        // Das tatsaechliche Ziel ist jetzt LEZL — EDDF ist keins mehr.
        b.anflug_ausrichten(&[("LEZL".into(), 0)], false);

        assert_eq!(
            b.laufender(),
            None,
            "der veraltete Auftrag laeuft weiter und blockiert die Reihe"
        );
        assert_eq!(
            b.naechsten_stellen(1).map(|(i, _)| i).as_deref(),
            Some("LEZL"),
            "das aktuelle Ziel muss ohne Wartezeit drankommen"
        );
    }

    /// Ein laufender Auftrag, der WEITER Ziel ist, bleibt stehen.
    ///
    /// ⚠ Sonst faengt jede Ausrichtung die laufende Anfrage neu an und
    /// die Wartezeit liefe nie ab.
    #[test]
    fn das_ausrichten_stoert_einen_gueltigen_auftrag_nicht() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("LEZL", 0);
        let _ = b.naechsten_stellen(0).expect("Auftrag");
        b.anflug_ausrichten(&[("LEZL".into(), 0)], false);
        assert_eq!(b.laufender().as_deref(), Some("LEZL"));
    }

    /// ⚠ QS-Befund 5, fuenfte Runde: Auswaehlen und Stellen sind EIN
    /// Zug.
    ///
    /// Getrennt waren sie ein Riss: Zwischen `naechster` und `gestellt`
    /// kann ein Flugwechsel das Buch leeren. `gestellt` legte dann
    /// trotzdem Kennung und `laeuft` an, und eine spaetere Lieferung
    /// setzte den Flughafen des ALTEN Fluges wieder ins neue Buch.
    #[test]
    fn ein_leeres_buch_stellt_keinen_auftrag() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        // Flugwechsel genau zwischen Auswahl und Stellen.
        b.zuruecksetzen();
        assert_eq!(
            b.gestellt("EDDF", 0),
            0,
            "fuer einen Platz, den es nicht mehr gibt, entstand eine Kennung"
        );
        assert_eq!(b.laufender(), None);
        assert_eq!(b.platz_zu_kennung(0), None, "Kennung 0 gilt als ungueltig");
        assert_eq!(
            b.geliefert_zu_kennung(0, auskunft("EDDF", 4)),
            None,
            "eine Lieferung des alten Fluges landete im neuen Buch"
        );
        assert!(b.auskunft("EDDF").is_none());
    }

    /// ⚠ QS-Befund 3, fuenfte Runde: Ein voruebergehender Fehler sperrt
    /// den Platz nicht aus.
    ///
    /// Ein abgelehnter Platz bleibt auch bei einem neuen Fenster
    /// geschlossen — eine Ueberlast haette ihn damit fuer den Rest des
    /// Fluges ausgesperrt.
    #[test]
    fn ein_voruebergehender_fehler_sperrt_den_platz_nicht_aus() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let (_, id) = b.naechsten_stellen(0).expect("Auftrag");

        b.freigeben_zu_kennung(id, 0);

        // ⚠ Dieser Test verlangte bis zur siebten Runde das GEGENTEIL:
        // „nach einem voruebergehenden Fehler muss SOFORT wieder gefragt
        // werden duerfen". Das war der P0 — der Verteiler laeuft alle
        // 50 ms, und nach zehn Durchlaeufen war der ganze
        // Abschnittsvorrat in einer halben Sekunde verbrannt. Bei
        // `TOO_MANY_REQUESTS` verschaerfte die Wiederholung sogar genau
        // den Zustand, den sie beheben soll.
        assert!(
            matches!(b.zustand("LEZL"), Some(Auftragszustand::Wartet { .. })),
            "der Platz ruht nicht: {:?}",
            b.zustand("LEZL")
        );
        assert_eq!(
            b.naechsten_stellen(1),
            None,
            "1 ms nach dem Fehler wird schon wieder gefragt"
        );
        assert_eq!(
            b.naechsten_stellen(RUECKZUG_MS - 1),
            None,
            "kurz vor Ablauf der Ruhezeit wird schon wieder gefragt"
        );
        assert_eq!(
            b.naechsten_stellen(RUECKZUG_MS).map(|(i, _)| i).as_deref(),
            Some("LEZL"),
            "nach der Ruhezeit wird nicht wieder gefragt"
        );
    }

    /// ⚠ Und ein ruhender Platz haelt KEINEN anderen auf.
    ///
    /// Sonst waere die Ruhezeit eine Vollsperre: Das eigentliche Ziel
    /// muesste warten, weil der Startflughafen sich verschluckt hat.
    #[test]
    fn ein_ruhender_platz_blockiert_die_anderen_nicht() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDF", 0);
        b.wunsch_mit_rang("LEZL", 1);
        let (erst, id) = b.naechsten_stellen(0).expect("erster Auftrag");
        assert_eq!(erst, "EDDF");

        b.freigeben_zu_kennung(id, 0);

        assert_eq!(
            b.naechsten_stellen(1).map(|(i, _)| i).as_deref(),
            Some("LEZL"),
            "der ruhende Platz sperrt die ganze Reihe"
        );
    }

    /// ⚠ Zehn Versuche dauern jetzt 45 Sekunden, nicht eine halbe.
    ///
    /// Das ist die Rechnung, die dem P0 zugrunde lag: Verteiler alle
    /// 50 ms, zehn Versuche — ohne Ruhezeit war der Vorrat nach 500 ms
    /// weg. Mit `RUECKZUG_MS` starten sie bei 0, 5, … 45 Sekunden.
    ///
    /// ⚠⚠ Dieser Test hiess `..._dauern_minuten`. Das war schlicht
    /// falsch gerechnet — 9 × 5 s sind 45 s, keine Minuten. Ein
    /// Testname, der mehr behauptet als die Sache hergibt, ist eine
    /// Aussage ueber das Verhalten wie jede andere (QS-Nebenbefund,
    /// achte Runde).
    #[test]
    fn zehn_voruebergehende_fehler_dauern_fuenfundvierzig_sekunden() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let mut t = 0i64;
        let mut versuche = 0;
        let mut letzter = 0i64;
        // Der Verteiler klopft alle 50 ms an.
        while t < 10 * RUECKZUG_MS {
            if let Some((_, id)) = b.naechsten_stellen(t) {
                versuche += 1;
                letzter = t;
                b.freigeben_zu_kennung(id, t);
            }
            t += 50;
        }
        assert_eq!(
            versuche,
            HOECHSTVERSUCHE as usize,
            "in {} ms wurden {versuche} Versuche verbraucht",
            10 * RUECKZUG_MS
        );
        // ⚠ Die Zahl im Namen NACHRECHNEN, nicht behaupten: neun
        // Ruhezeiten zwischen zehn Versuchen.
        assert_eq!(
            letzter,
            (HOECHSTVERSUCHE as i64 - 1) * RUECKZUG_MS,
            "der zehnte Versuch faellt nicht auf die erwartete Marke"
        );
        assert_eq!(letzter, 45_000, "45 Sekunden, wie der Name sagt");
        // ⚠ Und danach meldet die Diagnose ERSCHOEPFT, nicht „ruht".
        assert!(
            b.diagnose("LEZL").starts_with("erschoepft("),
            "nach dem letzten Versuch meldet die Diagnose weiter: {}",
            b.diagnose("LEZL")
        );
        // Und die ersten zehn Durchlaeufe (500 ms) duerfen nicht reichen.
        let mut b2 = Auftragsbuch::neu();
        b2.wunsch("LEZL");
        let mut t2 = 0i64;
        let mut v2 = 0;
        while t2 < 500 {
            if let Some((_, id)) = b2.naechsten_stellen(t2) {
                v2 += 1;
                b2.freigeben_zu_kennung(id, t2);
            }
            t2 += 50;
        }
        assert_eq!(
            v2, 1,
            "in einer halben Sekunde wurden {v2} Versuche verbrannt"
        );
    }

    /// ⚠ QS-Befund 3, sechste Runde: Nur die NEUESTE Kennung darf
    /// freigeben.
    ///
    /// Kommt die transiente Ausnahme zu Versuch 1 erst an, waehrend
    /// Versuch 2 laeuft, setzte sie dessen Zustand auf `Offen` und
    /// loeschte den laufenden Auftrag — Versuch 3 begaenne sofort,
    /// waehrend Versuch 2 noch unterwegs ist.
    #[test]
    fn eine_alte_ausnahme_beendet_den_neueren_versuch_nicht() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("LEZL");
        let (_, id1) = b.naechsten_stellen(0).expect("Versuch 1");
        let (_, id2) = b.naechsten_stellen(WARTEZEIT_MS).expect("Versuch 2");
        assert_ne!(id1, id2);

        // Die alte Ausnahme trifft ein — sie darf nichts tun.
        assert_eq!(
            b.freigeben_zu_kennung(id1, 0),
            None,
            "die alte Ausnahme hat den laufenden Versuch beendet"
        );
        assert_eq!(
            b.laufender().as_deref(),
            Some("LEZL"),
            "der laufende Auftrag wurde geloescht"
        );
        assert_eq!(
            b.naechsten_stellen(WARTEZEIT_MS + 1),
            None,
            "ein dritter Versuch startet, waehrend der zweite laeuft"
        );

        // Die Ausnahme zum NEUESTEN Versuch wirkt sehr wohl — der Platz
        // ruht danach, statt sofort wieder dranzukommen.
        assert_eq!(
            b.freigeben_zu_kennung(id2, WARTEZEIT_MS).as_deref(),
            Some("LEZL")
        );
        assert_eq!(
            b.zustand("LEZL"),
            Some(Auftragszustand::Wartet {
                bis_ms: WARTEZEIT_MS + RUECKZUG_MS
            })
        );
    }

    /// ⚠ QS-Befund 5, sechste Runde: Freigeben heisst BEIDE Haelften.
    ///
    /// `anflug_ausrichten` loeschte nur `laeuft`. Der Eintrag blieb auf
    /// `Laeuft` stehen — `laufender()` sagte „niemand", `diagnose(ICAO)`
    /// weiter „unterwegs". Zwei Aussagen ueber denselben Platz, die sich
    /// widersprechen.
    #[test]
    fn ein_freigegebener_auftrag_meldet_nicht_mehr_unterwegs() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDF", 0);
        let _ = b.naechsten_stellen(0).expect("Auftrag");

        b.anflug_ausrichten(&[("LEZL".into(), 0)], false);

        assert_eq!(b.laufender(), None);
        assert_eq!(
            b.zustand("EDDF"),
            Some(Auftragszustand::Offen),
            "der Eintrag steht weiter auf Laeuft"
        );
        assert!(
            b.diagnose("EDDF").starts_with("angemeldet("),
            "die Diagnose meldet weiter unterwegs: {}",
            b.diagnose("EDDF")
        );
    }

    /// ⚠ QS-Befund 2, sechste Runde: Ein Definitionsfehler schliesst
    /// auch die DATENSEITE.
    ///
    /// Die Sperre hielt bisher nur die Anfragen an. Eine Auskunft, die
    /// unter einer nachweislich ungueltigen Definition entstanden ist,
    /// hat ein anderes Raster als erwartet — sie ist nicht „etwas besser
    /// als nichts", sondern falsch.
    #[test]
    fn ein_definitionsfehler_gibt_keine_auskunft_mehr_heraus() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDF");
        let (_, id) = b.naechsten_stellen(0).expect("Auftrag");
        b.geliefert_zu_kennung(id, auskunft("EDDF", 4))
            .expect("Lieferung");
        assert!(b.auskunft("EDDF").is_some(), "vorher da");

        b.definition_abgelehnt("WIDTH".into(), "DATA_ERROR".into());

        assert!(
            b.auskunft("EDDF").is_none(),
            "eine unter ungueltiger Definition entstandene Auskunft wird \
             weiter herausgegeben"
        );
        // Und nach einer neuen Verbindung — Definition neu registriert —
        // ist die Sperre weg.
        b.verbindung_zuruecksetzen();
        assert!(b.definitionsfehler().is_none());
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

    /// v1.7.17 (externe Gegenpruefung, Codex, adversarial, 04.09.2026):
    /// `raenge_setzen` faellt fuer JEDEN Platz ausserhalb der Zielliste
    /// auf `RANG_UNBETEILIGT` zurueck — auch fuer einen Platz, der aus
    /// einem GANZ ANDEREN Grund im selben Buch steht (der Abflugplatz,
    /// der nie Anflug-Ziel ist). `geschuetzt` muss das verhindern.
    #[test]
    fn geschuetzter_platz_behaelt_seinen_rang() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDS", 0); // Abflugplatz, wie am Flugbeginn
        let ziele = vec![("LEPA".to_string(), 0)];
        b.raenge_setzen_mit_schutz(&ziele, &["EDDS".to_string()]);
        assert_eq!(
            b.rang_fuer_test("EDDS"),
            Some(0),
            "geschuetzter Platz darf nicht auf RANG_UNBETEILIGT fallen"
        );
    }

    /// Ohne Schutz faellt derselbe Platz zurueck — Gegenprobe zur
    /// Zusicherung oben, direkt als Test statt nur als Mutation.
    #[test]
    fn ungeschuetzter_platz_faellt_auf_rang_unbeteiligt() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDS", 0);
        let ziele = vec![("LEPA".to_string(), 0)];
        b.raenge_setzen_mit_schutz(&ziele, &[]);
        assert_eq!(b.rang_fuer_test("EDDS"), Some(RANG_UNBETEILIGT));
    }

    /// Der eigentliche Runde-1-Fall: ein LAUFENDER Auftrag fuer einen
    /// geschuetzten, aber nicht im Ziel stehenden Platz darf NICHT
    /// vorzeitig auf `Offen` zurueckgesetzt werden — sonst kann der
    /// Verbindungsfaden eine zweite Anfrage stellen, bevor die erste
    /// beantwortet ist.
    #[test]
    fn laufender_geschuetzter_auftrag_wird_nicht_vorzeitig_freigegeben() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDS", 0);
        let _ = b.gestellt("EDDS", 0); // EDDS ist jetzt "Laeuft"
        assert_eq!(b.laufender().as_deref(), Some("EDDS"));

        let ziele = vec![("LEPA".to_string(), 0)];
        b.anflug_ausrichten_mit_schutz(&ziele, &["EDDS".to_string()], false);

        assert_eq!(
            b.laufender().as_deref(),
            Some("EDDS"),
            "ein geschuetzter laufender Auftrag darf nicht vorzeitig freigegeben werden"
        );
    }

    /// Gegenprobe zur Zusicherung oben: OHNE Schutz wird derselbe
    /// laufende Auftrag sofort freigegeben — das bestehende, gewollte
    /// Verhalten fuer echte Anflug-Kandidaten, die aus der Zielliste
    /// gefallen sind.
    #[test]
    fn laufender_ungeschuetzter_auftrag_wird_freigegeben() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDS", 0);
        let _ = b.gestellt("EDDS", 0);
        assert_eq!(b.laufender().as_deref(), Some("EDDS"));

        let ziele = vec![("LEPA".to_string(), 0)];
        b.anflug_ausrichten_mit_schutz(&ziele, &[], false);

        assert_eq!(
            b.laufender(),
            None,
            "ohne Schutz muss der laufende Auftrag freigegeben werden (bestehendes Verhalten)"
        );
    }

    /// v1.7.17 Runde 2 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): Runde 1 pruefte nur `fenster=false`. Mit `fenster=true`
    /// — der Fall bei jedem Phasenwechsel in Sinkflug/Anflug/Endanflug —
    /// griff ein ZWEITER, unabhaengiger Reset-Pfad (`neues_versuchsfenster`),
    /// der JEDEN `Laeuft`-Auftrag zuruecksetzt, auch geschuetzte. Der
    /// Runde-1-Schutz in `anflug_ausrichten_mit_schutz` selbst reichte
    /// nicht, weil er nur die ERSTE Freigabestelle abdeckte.
    #[test]
    fn laufender_geschuetzter_auftrag_uebersteht_auch_ein_neues_fenster() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDS", 0);
        let _ = b.gestellt("EDDS", 0);
        assert_eq!(b.laufender().as_deref(), Some("EDDS"));

        let ziele = vec![("LEPA".to_string(), 0)];
        // fenster=true — das war die Luecke, die Runde 1 nicht deckte.
        b.anflug_ausrichten_mit_schutz(&ziele, &["EDDS".to_string()], true);

        assert_eq!(
            b.laufender().as_deref(),
            Some("EDDS"),
            "ein geschuetzter laufender Auftrag muss auch ein neues Versuchsfenster ueberstehen"
        );
    }

    /// Gegenprobe: OHNE Schutz reisst ein neues Fenster den laufenden
    /// Auftrag heraus — bestehendes, gewolltes Verhalten fuer echte
    /// Anflug-Kandidaten.
    #[test]
    fn laufender_ungeschuetzter_auftrag_wird_von_neuem_fenster_zurueckgesetzt() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDS", 0);
        let _ = b.gestellt("EDDS", 0);
        let ziele = vec![("LEPA".to_string(), 0)];
        b.anflug_ausrichten_mit_schutz(&ziele, &[], true);
        assert_eq!(b.laufender(), None);
    }

    /// v1.7.17 Runde 3 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): Rueckflug zum Startflughafen (oder jeder Flug mit dem
    /// Abflugplatz als Ausweichziel) — derselbe ICAO steht in `ziele` UND
    /// `geschuetzt`. Ein erschoepfter Auftrag fuer diesen Platz muss sein
    /// neues Fenster bekommen wie jeder andere echte Anflug-Kandidat; der
    /// Schutz darf ihn nicht ausklammern, nur weil er zufaellig auch der
    /// Abflugplatz ist.
    #[test]
    fn ziel_das_zugleich_abflugplatz_ist_bekommt_sein_fenster() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDS");
        let mut t = 0i64;
        for _ in 0..HOECHSTVERSUCHE {
            let (_, id) = b.naechsten_stellen(t).expect("Versuch");
            b.freigeben_zu_kennung(id, t);
            t += RUECKZUG_MS;
        }
        assert_eq!(b.zustand("EDDS"), Some(Auftragszustand::Erschoepft));

        let ziele = vec![("EDDS".to_string(), 0)];
        let geschuetzt = vec!["EDDS".to_string()];
        let betroffen = b.anflug_ausrichten_mit_schutz(&ziele, &geschuetzt, true);

        assert_eq!(
            betroffen, 1,
            "EDDS ist zugleich Ziel und geschuetzt — das Ziel muss gewinnen"
        );
        assert_eq!(
            b.zustand("EDDS"),
            Some(Auftragszustand::Offen),
            "ein Abflugplatz, der auch Anflug-Ziel ist, darf sein Fenster nicht verlieren"
        );
    }

    /// Gegenprobe zur vorigen: OHNE die Ziel-Praeferenz (alter Runde-2-
    /// Stand) bliebe der Platz faelschlich erschoepft.
    #[test]
    fn schutz_allein_ohne_zielvorrang_wuerde_das_fenster_verweigern() {
        let mut b = Auftragsbuch::neu();
        b.wunsch("EDDS");
        let mut t = 0i64;
        for _ in 0..HOECHSTVERSUCHE {
            let (_, id) = b.naechsten_stellen(t).expect("Versuch");
            b.freigeben_zu_kennung(id, t);
            t += RUECKZUG_MS;
        }
        let geschuetzt = vec!["EDDS".to_string()];
        // Ohne Ziel-Praeferenz zu pruefen: leere Zielliste, nur Schutz.
        let betroffen = b.neues_versuchsfenster_mit_schutz(&[], &geschuetzt);
        assert_eq!(betroffen, 0, "reiner Schutz ohne Ziel oeffnet kein Fenster");
        assert_eq!(b.zustand("EDDS"), Some(Auftragszustand::Erschoepft));
    }

    /// v1.7.17 Runde 4 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): Steht derselbe ICAO in `ziele` UND `geschuetzt` UND ist
    /// er GERADE ECHT AKTIV (`Laeuft`), darf der Zielvorrang (Runde 3) ihn
    /// TROTZDEM nicht zuruecksetzen — sonst koennte eine zweite,
    /// ueberlappende Anfrage fuer denselben Platz losgeschickt werden,
    /// bevor die erste Antwort da ist (derselbe Fehler, den Runde 1 fuer
    /// den reinen Schutzfall schon verhindert hat). Zielvorrang gilt nur
    /// fuer die RUHENDEN Zustaende — siehe den Erschoepft-Fall oben.
    #[test]
    fn ziel_das_zugleich_abflugplatz_ist_bleibt_geschuetzt_solange_es_echt_laeuft() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDS", 0);
        let _ = b.gestellt("EDDS", 0);
        assert!(matches!(
            b.zustand("EDDS"),
            Some(Auftragszustand::Laeuft { .. })
        ));

        let ziele = vec![("EDDS".to_string(), 0)];
        let geschuetzt = vec!["EDDS".to_string()];
        b.anflug_ausrichten_mit_schutz(&ziele, &geschuetzt, true);

        assert!(
            matches!(b.zustand("EDDS"), Some(Auftragszustand::Laeuft { .. })),
            "eine echt laufende Anfrage fuer denselben Platz darf nicht doppelt gestellt \
             werden, auch wenn er zugleich Ziel und geschuetzt ist"
        );
        assert_eq!(
            b.versuche("EDDS"),
            0,
            "die Fenster-Gutschrift (Runde 5) muss trotzdem wirken — sonst verliert ein \
             laufender letzter Versuch sein Kontingent fuer immer"
        );
    }

    /// v1.7.17 Runde 5 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): War die laufende Anfrage bereits der LETZTE erlaubte
    /// Versuch, darf ein spaeterer Fehlschlag NICHT sofort in Erschoepft
    /// muenden — die Fenster-Gutschrift (Versuchszaehler auf 0) muss
    /// wirken, obwohl der Zustand waehrend des Fensters `Laeuft` blieb.
    #[test]
    fn ein_echt_laufender_letzter_versuch_bekommt_die_fenster_gutschrift_trotzdem() {
        let mut b = Auftragsbuch::neu();
        b.wunsch_mit_rang("EDDS", 0);
        let mut t = 0i64;
        let mut letzte_id = 0;
        for _ in 0..HOECHSTVERSUCHE {
            let (_, id) = b.naechsten_stellen(t).expect("Versuch");
            letzte_id = id;
            if b.versuche("EDDS") < HOECHSTVERSUCHE {
                b.freigeben_zu_kennung(id, t);
            }
            t += RUECKZUG_MS;
        }
        assert_eq!(b.versuche("EDDS"), HOECHSTVERSUCHE);
        assert!(matches!(
            b.zustand("EDDS"),
            Some(Auftragszustand::Laeuft { .. })
        ));

        let ziele = vec![("EDDS".to_string(), 0)];
        let geschuetzt = vec!["EDDS".to_string()];
        b.anflug_ausrichten_mit_schutz(&ziele, &geschuetzt, true);

        assert!(
            matches!(b.zustand("EDDS"), Some(Auftragszustand::Laeuft { .. })),
            "die echt laufende Anfrage darf nicht doppelt gestellt werden"
        );
        assert!(
            b.versuche("EDDS") < HOECHSTVERSUCHE,
            "der Versuchszaehler muss zurueckgesetzt sein, sonst verliert der letzte \
             Versuch seine Fenster-Gutschrift fuer immer"
        );

        // Scheitert dieser (jetzt gutgeschriebene) Versuch danach, muss
        // noch ein weiterer moeglich sein statt sofortiger Erschoepfung.
        b.freigeben_zu_kennung(letzte_id, t);
        assert!(
            matches!(b.zustand("EDDS"), Some(Auftragszustand::Wartet { .. })),
            "nach der Fenster-Gutschrift muss ein Fehlschlag noch einen weiteren Versuch \
             erlauben, nicht sofortige Erschoepfung: {:?}",
            b.zustand("EDDS")
        );
    }
}
