//! v1.7.35 — Sprit-Auswertung **ohne Note**.
//!
//! # Warum es diese Datei gibt
//!
//! Die Achse „OFP-Treue" verglich den geplanten mit dem tatsaechlichen
//! Streckenverbrauch und zog bei Mehrverbrauch Punkte ab. Am Bestand
//! belegt (DLH370, 17.09.2026, und alle A380-Fluege nach EDDM): Der
//! Mehrverbrauch entsteht zum Grossteil im Anflug — Radarfuehrung,
//! Zwischenhoehen, Transitions — und den bestimmt die Flugsicherung,
//! nicht der Pilot. Der Pilot bewegt am Verbrauch 2–3 %, ATC bis ueber
//! 25 % (EUROCONTROL 2020). Kein reales Airline-Programm und keine
//! grosse VA-Plattform benotet den Einzelflug nach Verbrauch.
//!
//! Deshalb: **Der Sprit verlaesst die Note.** Diese Auswertung erklaert
//! dem Piloten, was passiert ist — in Zahlen, ohne Urteil:
//!
//! 1. Verbrauch bis zum Sinkflug gegen Plan (das, was der Pilot steuert)
//! 2. Verbrauch im Sinkflug und Anflug gegen Plan (das, was ATC steuert)
//! 3. Zeit unterhalb einer Schwelle ueber Platzhoehe (die Radarfuehrung)
//! 4. Die Sprit-Leiter des OFP mit der Landemarke: Contingency ist per
//!    Definition das Budget fuer Wind, Routing und ATC (ICAO Annex 6);
//!    Extra ist der Puffer des Piloten. Beides zu nutzen ist planmaessig.
//! 5. Final Reserve — die einzige Sprit-Groesse, die wirklich zaehlt.
//!    Unterschreitung ist ein Hinweis (gelb), nie eine Note, nie rot,
//!    nie eine Blockierung (Thomas, 17.09.2026).
//!
//! # Eine Stelle rechnet
//!
//! Der Client ruft [`auswerten`] einmal beim Abschluss der Landung auf und
//! schreibt das Ergebnis in den PIREP-Payload. Client-Oberflaeche,
//! PDF-Bericht und die Live-Webapp **zeigen** dieses Ergebnis nur an —
//! kein Fallback, kein Nachrechnen. Fehlt ein Wert, steht dort „—".

use serde::{Deserialize, Serialize};

/// Alles, was die Auswertung braucht. Jeder Wert ist optional: Manual-
/// Fluege haben kein OFP, Altbestand-Resumes kennen die Navlog-Felder
/// nicht, ein Neustart mitten im Flug kann den Vergleichspunkt verpasst
/// haben. Fehlende Eingaben fuehren zu fehlenden Ausgaben — nie zu 0.
#[derive(Debug, Clone, Default)]
pub struct SpritEingang {
    /// Geplanter Trip-Verbrauch (OFP `enroute_burn`).
    pub planned_burn_kg: Option<f32>,
    /// Geplanter Verbrauch bis zum Vergleichspunkt (Navlog `fuel_totalused`
    /// am TOD-Fix).
    pub plan_bis_vergleichspunkt_kg: Option<f32>,
    /// Geplante Reststrecke ab dem Vergleichspunkt bis zum Ziel (NM).
    pub plan_rest_nm: Option<f32>,
    /// Tankstand beim Abheben.
    pub takeoff_fuel_kg: Option<f32>,
    /// Tankstand am Vergleichspunkt — dort, wo noch `plan_rest_nm` vor dem
    /// Flugzeug lag. Vom Client im Flug festgehalten.
    pub sprit_bei_vergleichspunkt_kg: Option<f32>,
    /// Tankstand beim Aufsetzen.
    pub landing_fuel_kg: Option<f32>,
    /// Geflogene Strecke ab dem Vergleichspunkt bis zum Aufsetzen (NM).
    pub strecke_ab_vergleichspunkt_nm: Option<f32>,
    /// Zeit unterhalb der Schwelle (s), Pausen und Neustart-Luecken bereits
    /// abgezogen.
    pub zeit_unter_schwelle_s: Option<f32>,
    /// Die Schwelle selbst (ft MSL), zur Anzeige.
    pub schwelle_ft: Option<f32>,
    /// OFP-Sprit-Leiter.
    pub planned_taxi_kg: Option<f32>,
    pub planned_contingency_kg: Option<f32>,
    pub planned_alternate_kg: Option<f32>,
    pub planned_reserve_kg: Option<f32>,
    pub planned_extra_kg: Option<f32>,
    pub planned_block_fuel_kg: Option<f32>,
    /// Ausweichflug: eingereichtes Ziel != geplantes Ziel. Dann wurde der Plan
    /// nicht geflogen — Phasen und Plan-Reststrecke gegen ihn zu zeigen waere
    /// eine Aussage ueber einen Flug, den es nicht gab. Die alte Achse hatte
    /// dafuer eine ausdrueckliche Regel; die gilt hier weiter.
    pub ausweichflug: bool,
    /// `false`, wenn der Tankstand beim Aufsetzen nicht zum Verlauf passt
    /// (Auslesefehler, nur zwei von vier Tanks, Sprung). Dann gibt es
    /// „nicht pruefbar" statt einer falschen Warnung.
    pub tank_plausibel: bool,
    /// v1.7.36: Der geplante Trip laut DEMSELBEN Navlog, aus dem auch
    /// `plan_bis_vergleichspunkt_kg` stammt (Sprit am letzten Fix).
    ///
    /// Der Anflug-Plan ist „Trip minus Plan bis zum Messpunkt". Kommen beide
    /// aus verschiedenen Quellen — OFP-Kopf und Navlog, oder ein Routen-
    /// Update im Flug, das nur die Wegpunkte ersetzt —, summieren sich die
    /// Phasen nicht mehr auf den Plan, und der Schnitt driftet unbemerkt.
    /// `None` bei Altbestand: dann gilt `planned_burn_kg` wie bisher.
    pub plan_trip_navlog_kg: Option<f32>,
    /// v1.7.36: Tankstand, als die Triebwerke anliefen, und als sie
    /// ausgingen. Beide an ein Ereignis gerastet (`sprit_boden_marken`).
    pub engine_start_fuel_kg: Option<f32>,
    pub engine_off_fuel_kg: Option<f32>,
    /// v1.7.36: Die Aufzeichnung begann erst IN DER LUFT — Client mitten im
    /// Flug gestartet, die FSM hat das Abheben nie gesehen und der
    /// Airborne-Rescue hat den Abhebe-Tankstand beim Einstieg gesetzt.
    ///
    /// Dann ist `takeoff_fuel_kg` der Tankstand am EINSTIEG, und „bis
    /// Sinkflug" darf nur das Stueck ab dort gegen den Plan ab dort rechnen.
    /// Sonst stuende ein kurzes Ist-Stueck gegen den Plan ab dem Abflug, und
    /// der Flug saehe um Tonnen sparsamer aus, als er war.
    pub einstieg_in_der_luft: bool,
    /// Der Plan-Verbrauch bis zum Einstiegsort (`sprit_plan_am_ort`).
    /// `None` bei einem Einstieg in der Luft heisst: Der Ort lag nicht auf
    /// der Route — dann gibt es „bis Sinkflug" nicht, statt einer geratenen
    /// Zahl. Ohne Einstieg in der Luft ohne Bedeutung.
    pub plan_bis_einstieg_kg: Option<f32>,
}

/// Der Abstand zur Final Reserve passend zum Status: bei „unterschritten"
/// mindestens −1 kg, bei „intakt" nie negativ, bei „nicht pruefbar" keiner.
///
/// Ohne die Klemme stuende bei knapper Unterschreitung (4 915,6 gegen
/// 4 916,4 kg — gerundet gleich) „UNTERSCHRITTEN … − 0 kg" und im Hinweis
/// „0 kg darueber gelandet" (QS v1.7.37).
fn abstand_nach_status(status: &Reserve, roh: f32) -> Option<f32> {
    match status {
        Reserve::NichtPruefbar { .. } => None,
        Reserve::Unterschritten { .. } => Some(roh.min(-1.0)),
        Reserve::Intakt { .. } => Some(roh.max(0.0)),
    }
}

/// Der Abstand zur Final Reserve einer FERTIGEN Auswertung — auch fuer eine,
/// die vor v1.7.37 eingefroren wurde und das Feld nicht traegt (Update
/// zwischen Landung und Einreichen). Dieselbe Regel wie `reserveAbstand`
/// in client/src/lib/sprit.ts.
pub fn reserve_abstand(a: &SpritAuswertung) -> Option<f32> {
    if let Some(v) = a.reserve_abstand_kg {
        return abstand_nach_status(&a.reserve, v);
    }
    abstand_nach_status(&a.reserve, a.landing_fuel_kg? - a.reserve_kg?)
}

/// Genutzte Contingency einer FERTIGEN Auswertung, in kg — auch fuer eine
/// aus der Zeit vor v1.7.38 ohne das Feld. Dann gilt dieselbe Rechnung aus
/// Leiter und Landesprit wie in `auswerten`: Plan-Landestand (Skala minus
/// Taxi minus Trip) minus Landesprit, auf die Contingency begrenzt.
/// Dieselbe Regel wie `contingencyGenutzt` in client/src/lib/sprit.ts.
///
/// `None`, wenn die Auswertung dazu nichts sagt (kein OFP, Tank unplausibel —
/// dann ist auch `contingency_verbraucht` leer).
pub fn contingency_genutzt(a: &SpritAuswertung) -> Option<f32> {
    a.contingency_verbraucht?;
    if let Some(v) = a.contingency_genutzt_kg {
        return Some(v);
    }
    let l = a.leiter.as_ref()?;
    let ldg = a.landing_fuel_kg?;
    let posten =
        l.taxi_kg + l.trip_kg + l.contingency_kg + l.alternate_kg + l.reserve_kg + l.extra_kg;
    let skala = l.block_kg.max(posten) + l.uebertankung_kg - l.untertankung_kg;
    let mehr = (skala - l.taxi_kg - l.trip_kg - ldg).max(0.0);
    Some(mehr.clamp(0.0, l.contingency_kg).round())
}

/// Rollen nach der Landung: Landesprit minus Tankstand beim Abstellen.
///
/// Eigene Funktion, weil sie ZWEIMAL gebraucht wird und dieselbe Zahl
/// liefern muss: in `auswerten` und beim Nachtragen, wenn die Triebwerke
/// erst nach dem Einfrieren der Auswertung ausgehen. Dort wird nur dieses
/// eine Feld nachgetragen — alles andere bleibt, wie es beim Aufsetzen
/// gerechnet wurde (QS-Befund E2, 18.09.2026: eine vollstaendige Neubildung
/// hatte die Rollstrecke zum Stand in die Anflugstrecke geschoben).
pub fn rollen_nach_landung(
    landing_fuel_kg: Option<f32>,
    engine_off_fuel_kg: Option<f32>,
) -> Option<f32> {
    match (nicht_negativ(landing_fuel_kg), positiv(engine_off_fuel_kg)) {
        (Some(ldg), Some(aus)) if ldg > aus => Some((ldg - aus).round()),
        _ => None,
    }
}

/// Eine Flugphase: was gebraucht wurde, was geplant war, Abweichung in
/// Prozent (eine Nachkommastelle, gerundet — Anzeige und Wert sind
/// dieselbe Zahl).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    pub ist_kg: f32,
    pub plan_kg: f32,
    pub abweichung_pct: f32,
    /// Soll die Hauptzahl in Kilogramm stehen statt in Prozent?
    ///
    /// # Warum das hier entschieden wird
    ///
    /// Ueber 60 % sagt ein Prozentwert nichts mehr — der Anflug-Plan ist im
    /// Median nur 6,5 % des Trips, und dort ergeben schon 129 kg
    /// Mehrverbrauch +47 %. Die Regel selbst ist eine Zeile, aber sie stand
    /// bis v1.7.35 an VIER Stellen: im Client, in der Live-Uebersicht, im
    /// Bericht und in den PIREP-Feldern fuer die GSG-Webseite. Aendert
    /// jemand eine, driftet die vierte lautlos — und der Pilot liest im
    /// Client „−12 kg" und auf der Webseite „−8,3 %".
    ///
    /// Deshalb entscheidet es die Rechnung, und die Anzeigen folgen.
    ///
    /// `#[serde(default)]`: Datensaetze der Fassungen 1 und 2 tragen das Feld
    /// nicht. Ohne Rueckfall waere das ein harter Lesefehler — und
    /// `storage::read_all` legte die GANZE `landings.json` als
    /// `.corrupt-…` beiseite (QS-Befund F1, 18.09.2026: derselbe Fehler, der
    /// am selben Tag schon einmal an `SpritAuswertung` behoben wurde).
    /// Fuer Altbestand heisst `false` „Prozent" — die Anzeige des alten
    /// Datensatzes bleibt dann, wie sie war.
    #[serde(default)]
    pub als_kg: bool,
}

/// Stand der Final Reserve beim Aufsetzen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Reserve {
    /// Landesprit ueber der geplanten Final Reserve. `quote_pct` = 100 ·
    /// Landesprit / Reserve.
    Intakt { quote_pct: f32 },
    /// Landesprit unter der geplanten Final Reserve. Gelb, nie rot.
    Unterschritten { quote_pct: f32 },
    /// Kein OFP, kein Landesprit oder Tankwert unplausibel. Auch der
    /// Rueckfall fuer einen Datensatz, der das Feld nicht traegt.
    NichtPruefbar { grund: String },
}

impl Default for Reserve {
    fn default() -> Self {
        Reserve::NichtPruefbar {
            grund: "unbekannt".into(),
        }
    }
}

/// Das Badge im Kopf der Landung. Nur drei Farben, nie rot, keine Zahl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Badge {
    Gruen,
    Gelb,
    /// Auch der Rueckfall: „keine Aussage" ist die ehrliche Antwort, wenn
    /// ein Datensatz das Feld nicht traegt.
    #[default]
    Grau,
}

/// Die Sprit-Leiter des OFP, wie sie der Dispatcher plant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Leiter {
    pub taxi_kg: f32,
    pub trip_kg: f32,
    pub contingency_kg: f32,
    pub alternate_kg: f32,
    pub reserve_kg: f32,
    pub extra_kg: f32,
    pub block_kg: f32,
    /// v1.7.36: Was im geplanten Block steckt, aber in keinem der sechs
    /// Posten — ETOPS-, Minimum- oder Zusatzsprit, den SimBrief getrennt
    /// ausweist. Ohne dieses Stueck ergaeben die Posten weniger als den
    /// Block, und jede Marke saesse um genau diesen Betrag falsch.
    #[serde(default)]
    pub sonstiges_kg: f32,
    /// v1.7.36: Mehr getankt als geplant — der Tank beim Anlassen ueber dem
    /// Plan-Block (ohne Anlass-Marke: Abhebe-Tankstand plus Plan-Taxi). Bis dahin rechnete die ANZEIGE das selbst aus, entgegen
    /// der Zusage „nur gerendert, nichts nachgerechnet" (QS-Befund V6). Und
    /// es fehlte, war die Leiter nicht genau der Block. Jetzt eine Zahl, an
    /// einer Stelle.
    #[serde(default)]
    pub uebertankung_kg: f32,
    /// v1.7.36: Weniger getankt als geplant — der Tank beim Anlassen unter
    /// dem Plan-Block, hoechstens bis zur Hoehe des Extra.
    ///
    /// Was nie an Bord war, kann auch nicht „genutzt" worden sein. Es fehlt
    /// am Extra, dem frei waehlbaren Teil der Betankung: `extra_getankt_kg`
    /// ist um genau diesen Betrag kleiner, und die Grafik zeichnet das Extra
    /// entsprechend kuerzer. Ohne das erschien die Untertankung als „Extra
    /// genutzt" und konnte „Contingency verbraucht" ausloesen, waehrend die
    /// Zeile „getankt" die volle Plan-Menge nannte (QS Runde 3).
    #[serde(default)]
    pub untertankung_kg: f32,
}

/// Das Ergebnis. Wird 1:1 in den Payload geschrieben und ueberall nur
/// gerendert. `fassung` steigt, wenn sich die Bedeutung eines Feldes
/// aendert, damit Anzeigen alte Datensaetze richtig lesen.
///
/// # Warum `#[serde(default)]` am ganzen Typ
///
/// `fassung` verspricht, dass eine spaetere Fassung einen aelteren Datensatz
/// noch lesen kann. Ohne `default` haelt serde das nicht: ein fehlendes Feld
/// ist ein harter Fehler. `LandingRecord.sprit` traegt zwar `default`, aber
/// das greift nur, wenn das ganze Objekt fehlt — nicht, wenn es da ist und
/// ein Feld vermisst. Und `storage::read_all` liest `landings.json` als EINEN
/// `Vec<LandingRecord>`: ein einziger Datensatz aelterer Fassung wuerde die
/// gesamte lokale Landungs-Historie in den Fehlerzweig kippen und als
/// `landings.json.corrupt-…` beiseitelegen. Genau das ist beim Sprung von
/// Fassung 1 auf 2 (`takeoff_fuel_kg`) moeglich geworden.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpritAuswertung {
    pub fassung: u8,
    pub bis_sinkflug: Option<Phase>,
    pub anflug: Option<Phase>,
    pub zeit_unter_schwelle_min: Option<f32>,
    pub schwelle_ft: Option<f32>,
    pub strecke_anflug_nm: Option<f32>,
    pub plan_strecke_anflug_nm: Option<f32>,
    pub reserve: Reserve,
    pub reserve_kg: Option<f32>,
    /// v1.7.37: Landesprit minus Final Reserve in kg — positiv darueber,
    /// negativ darunter. `None`, wenn die Reserve nicht pruefbar ist.
    ///
    /// Die Anzeige zeigte bis dahin die Quote („Reserve 732 kg (338 %)"),
    /// und das las sich, als haette die RESERVE 338 %. Ein Pilot denkt in
    /// Kilogramm Abstand zur Reserve (Thomas, BIT348, 18.09.2026). Die
    /// Quote bleibt im Datensatz fuer aeltere Anzeigen.
    pub reserve_abstand_kg: Option<f32>,
    /// Tankstand beim Abheben. Steht hier, weil er sich aus den uebrigen
    /// Feldern nicht rekonstruieren laesst: Bleibt der Mehrverbrauch unter
    /// der Contingency, taucht er in `extra_genutzt_kg` gar nicht auf. Die
    /// Anzeige braucht ihn fuer die Abhebe-Marke der Sprit-Leiter.
    pub takeoff_fuel_kg: Option<f32>,
    pub landing_fuel_kg: Option<f32>,
    pub extra_getankt_kg: Option<f32>,
    pub extra_genutzt_kg: Option<f32>,
    pub extra_ungenutzt_kg: Option<f32>,
    pub contingency_verbraucht: Option<bool>,
    /// v1.7.38: Wie viel der Contingency genutzt wurde, in kg (0 … geplant).
    ///
    /// `contingency_verbraucht` sagt nur, ob sie GANZ aufgebraucht ist. Alles
    /// darunter hiess in der Anzeige „unberuehrt" — bei Flug #1417 waren aber
    /// 86 von 238 kg genutzt, und die Landemarke stand sichtbar im gelben
    /// Block (Thomas, 19.09.2026).
    pub contingency_genutzt_kg: Option<f32>,
    pub alternate_und_reserve_intakt: Option<bool>,
    pub leiter: Option<Leiter>,
    /// v1.7.36: Rollen vor dem Start, gegen den geplanten Taxi-Anteil.
    ///
    /// Erst damit ist der Flug lueckenlos abgedeckt: Bis v1.7.35 begann die
    /// Auswertung beim Abheben und endete beim Aufsetzen — bei einem A380
    /// blieben so vierstellige Kilogramm unerwaehnt.
    pub rollen_vor_start: Option<Phase>,
    /// Rollen nach der Landung — **ohne Plan**, nur die Zahl.
    ///
    /// SimBrief plant genau EINEN Taxi-Block, und der gilt dem Weg zum
    /// Start; `planned_burn_kg` beginnt beim Abheben. Fuer den Weg zurueck
    /// zum Stand gibt es also nichts zu vergleichen. Das ist kein Mangel,
    /// sondern die Wahrheit ueber das OFP — und es passt zur Hausform
    /// „zeigen, nicht benoten".
    pub rollen_nach_landung_kg: Option<f32>,
    /// v1.7.36: Die Aufzeichnung begann in der Luft. `takeoff_fuel_kg` ist
    /// dann der Tankstand beim EINSTIEG — die Anzeige beschriftet die Marke
    /// entsprechend, statt „abgehoben mit" zu behaupten.
    pub einstieg_in_der_luft: bool,
    pub badge: Badge,
    /// v1.7.40: Sprit Wegpunkt für Wegpunkt — SimBrief an Bord gegen den
    /// Tankstand beim Überflug, mit Hochrechnung auf die Landung. Leer bei
    /// Flügen ohne Navlog-Werte und bei Altbestand. Nur Anzeige, keine Note.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub wegpunkte: Vec<Wegpunkt>,
}

/// Farbe einer Zeile der Wegpunkt-Tabelle. **Keine Note** — sie fliesst in
/// keine Bewertung ein. Sie sagt, wie viel Luft im Tank bleibt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ampel {
    /// Die Hochrechnung auf die Landung liegt im Plan (Contingency deckt).
    Gruen,
    /// Die Contingency wird aufgebraucht, das OFP-Minimum hält.
    Gelb,
    /// Unter dem OFP-Minimum — jetzt oder hochgerechnet bei der Landung.
    Rot,
}

/// Wie ein Wegpunkt zu seinem Ist-Wert kam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WegpunktZustand {
    /// Noch nicht erreicht — nur der Plan.
    #[default]
    Offen,
    /// Überflogen, Tankstand gemessen.
    Gemessen,
    /// Nicht überflogen (Direct, Radarführung). Der Ist-Wert ist aus den
    /// gemessenen Nachbarn gerechnet — keine Messung.
    Uebersprungen,
}

/// Eine Zeile der Tabelle „Sprit · Wegpunkt für Wegpunkt".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wegpunkt {
    pub ident: String,
    /// Geplante Höhe (ft), zur Anzeige.
    pub hoehe_ft: Option<f32>,
    /// SimBrief `fuel_plan_onboard`, kg.
    pub plan_an_bord_kg: Option<f32>,
    /// SimBrief `fuel_min_onboard`, kg.
    pub min_an_bord_kg: Option<f32>,
    pub zustand: WegpunktZustand,
    /// Zeitpunkt des Überflugs (Unix-Millisekunden).
    pub zeit_ms: Option<i64>,
    /// Tankstand beim Überflug — bei `Uebersprungen` gerechnet.
    pub ist_an_bord_kg: Option<f32>,
    /// Hochrechnung: So viel wäre bei der Landung noch im Tank.
    pub landung_hochgerechnet_kg: Option<f32>,
    pub ampel: Option<Ampel>,
}

/// Der Fuel-Check an einem Wegpunkt, wie ihn eine Crew macht (EFOB):
/// Tankstand jetzt, minus der geplante Rest, hochgerechnet mit dem bisher
/// gemessenen Verhältnis von Ist- zu Plan-Verbrauch.
///
/// Thomas (19.09.2026) hat das gegen „Contingency bisher" gewählt, weil es
/// nach vorn schaut: Ein hoher Verbrauch fällt auf, bevor er die Reserve
/// erreicht. Eine feste Prozentschwelle hat er verworfen — kleine Muster
/// liegen ohnehin 9–15 % über SimBrief und wären dauernd gelb.
///
/// # Warum der Rest NICHT hochskaliert wird
///
/// Bis 20.09.2026 wurde der geplante Restverbrauch mit dem bisher
/// gemessenen Ist-zu-Plan-Verhaeltnis multipliziert. Das erfindet Zahlen,
/// weil die Phasen verschieden teuer sind: Steigen kostet ein Vielfaches
/// derselben Strecke im Reiseflug. Bei DLH #1439 (A380, EDDM) ergab das
/// bei BRADD "−3748 kg bei der Landung" (rot) und zwei Fixe spaeter
/// "+20 209 kg" (gruen) — ein Sprung von 24 Tonnen im ruhigen Reiseflug.
/// Den Faktor stattdessen erst ab TOC zu bilden half nicht: Dann stand
/// dort 30 893 kg. Das Flugzeug selbst sagte 23,3 t.
///
/// Der SimBrief-Plan kennt Sinkflug, Wind und Stufen bereits; unsere
/// Multiplikation weiss davon nichts. Also genau die Crew-Rechnung:
/// Tankstand jetzt minus dem GEPLANTEN Rest. Die bisherige Abweichung
/// steckt in `ist_hier` und wird als Betrag weitergetragen, nicht
/// gestreckt. Fuer denselben Flug: 23 429 / 23 436 / 23 805 kg gegen
/// 23,3 t im FMS (Thomas, 20.09.2026).
///
/// Gibt `None`, wenn Plan-Werte fehlen.
pub fn fuel_check(
    ist_hier: f32,
    plan_hier: f32,
    plan_landung: f32,
    min_landung: Option<f32>,
    min_hier: Option<f32>,
    contingency_kg: Option<f32>,
) -> Option<(f32, Ampel)> {
    if !(ist_hier.is_finite() && plan_hier.is_finite() && plan_landung.is_finite()) {
        return None;
    }
    let plan_rest = (plan_hier - plan_landung).max(0.0);
    let hoch = ist_hier - plan_rest;
    let unter_min = min_landung.is_some_and(|m| hoch < m) || min_hier.is_some_and(|m| ist_hier < m);
    let ampel = if unter_min {
        Ampel::Rot
    } else if hoch < plan_landung - contingency_kg.unwrap_or(0.0).max(0.0) {
        Ampel::Gelb
    } else {
        Ampel::Gruen
    };
    Some((hoch, ampel))
}

/// Füllt Hochrechnung und Ampel für die Zeilen und rechnet übersprungene
/// Wegpunkte aus ihren gemessenen Nachbarn nach.
///
/// Erwartet die Zeilen in Flugreihenfolge; die erste ist der Abflug, die
/// letzte das Ziel (deren Plan-Wert die Plan-Landung ist).
pub fn wegpunkte_auswerten(zeilen: &mut [Wegpunkt], contingency_kg: Option<f32>) {
    let Some(letzte) = zeilen.last() else { return };
    let Some(plan_landung) = letzte.plan_an_bord_kg else {
        return;
    };
    let min_landung = letzte.min_an_bord_kg;

    // Übersprungene: Abweichung vom Plan zwischen den gemessenen Nachbarn
    // linear übertragen, gewichtet nach dem Plan-Verbrauch dazwischen.
    let gemessen: Vec<usize> = (0..zeilen.len())
        .filter(|&i| {
            zeilen[i].zustand == WegpunktZustand::Gemessen && zeilen[i].ist_an_bord_kg.is_some()
        })
        .collect();

    for i in 0..zeilen.len() {
        if zeilen[i].zustand != WegpunktZustand::Uebersprungen {
            continue;
        }
        zeilen[i].ist_an_bord_kg = None;
        let vor = gemessen.iter().rev().find(|&&g| g < i).copied();
        let nach = gemessen.iter().find(|&&g| g > i).copied();
        let (Some(v), Some(n)) = (vor, nach) else {
            continue;
        };
        let (Some(pv), Some(pn), Some(ph)) = (
            zeilen[v].plan_an_bord_kg,
            zeilen[n].plan_an_bord_kg,
            zeilen[i].plan_an_bord_kg,
        ) else {
            continue;
        };
        let dv = zeilen[v].ist_an_bord_kg.unwrap() - pv;
        let dn = zeilen[n].ist_an_bord_kg.unwrap() - pn;
        let anteil = if (pv - pn).abs() > 1.0 {
            ((pv - ph) / (pv - pn)).clamp(0.0, 1.0)
        } else {
            0.5
        };
        zeilen[i].ist_an_bord_kg = Some(ph + dv + (dn - dv) * anteil);
    }

    // KEIN Mindestabstand mehr, bevor gerechnet wird.
    //
    // Der Riegel (ein Zehntel des Trips) war die Notbremse gegen die
    // Hochskalierung: In den ersten Minuten stand der Startschub gegen ein
    // winziges Planstueck, und aus drei Minuten Steigflug wurden bei
    // DLH #1439 (20.09.2026) 929 kg Landesprit hochgerechnet. Seit die
    // Skalierung weg ist, gibt es dieses Rauschen nicht mehr: Tankstand
    // minus geplanter Rest ist ab dem ersten Ueberflug stabil. Beim selben
    // Flug traefe ACK — 26 Minuten nach dem Abheben — den FMS-Wert auf
    // 130 kg genau. Den haette der Riegel verschwiegen.
    for z in zeilen.iter_mut() {
        z.landung_hochgerechnet_kg = None;
        z.ampel = None;
        if z.zustand == WegpunktZustand::Offen {
            continue;
        }
        let (Some(ist), Some(plan)) = (z.ist_an_bord_kg, z.plan_an_bord_kg) else {
            continue;
        };
        if let Some((hoch, ampel)) = fuel_check(
            ist,
            plan,
            plan_landung,
            min_landung,
            z.min_an_bord_kg,
            contingency_kg,
        ) {
            z.landung_hochgerechnet_kg = Some(hoch.round());
            z.ampel = Some(ampel);
        }
    }
}

/// Die Fassung dieser Auswertung — **Diagnose, keine Weiche.**
///
/// Sie sagt einem Leser (Mensch oder Werkzeug), nach welcher Regel die
/// Zahlen entstanden sind. Der Code verzweigt NICHT auf sie; ein aelterer
/// Datensatz bleibt lesbar, weil `#[serde(default)]` am Typ steht, nicht
/// weil hier eine Fallunterscheidung haengt. Wer das aendern will, muss
/// zuerst die Anzeigen anfassen — heute wertet sie niemand aus.
///
/// # Was die Fassungen bedeuten
///
/// * **1** (v1.7.35, erster Wurf): `plan_kg` beider Phasen stammt vom
///   TOD-Fix des Navlogs.
/// * **2** (v1.7.35, nach der Abnahme): `takeoff_fuel_kg` kam dazu. Ohne das
///   Feld rechnete die Anzeige den Abhebe-Tankstand zurueck — und lag bei
///   DLH 370 um 3 204 kg daneben, weil der Mehrverbrauch bei unverbrauchter
///   Contingency in keinem der uebrigen Felder steckt.
/// * **3** (v1.7.36): `plan_kg` und `plan_strecke_anflug_nm` gehoeren jetzt
///   zum GEMESSENEN Ort, nicht mehr zum TOD-Fix (`sprit_plan_am_ort`).
///   Derselbe Flug bekommt unter Fassung 2 und 3 verschiedene Zahlen — bei
///   14 von 39 Fluegen lag der Messpunkt unter 80 % der Plan-Reststrecke,
///   und dort wurde ein kurzes Ist-Stueck gegen einen vollen Plan
///   gerechnet. Dazu kommen `rollen_vor_start` und
///   `rollen_nach_landung_kg`: der Flug ist erst damit lueckenlos
///   abgedeckt, von den Triebwerken an bis zu den Triebwerken aus. Die
///   Leiter traegt `sonstiges_kg` (Block ueber den sechs Posten) und
///   `uebertankung_kg` (Tank beim Anlassen ueber dem Block), und der
///   Anflug-Plan rechnet gegen den Trip desselben Navlogs
///   (`plan_trip_navlog_kg`) statt gegen den OFP-Kopf. Bei einem Einstieg
///   in der Luft zaehlt „bis Sinkflug" erst ab dem Einstiegsort.
pub const SPRIT_AUSWERTUNG_FASSUNG: u8 = 3;

/// Unterhalb dieses geplanten Rollverbrauchs ist der Prozentwert ohne
/// Aussage. Ein Bizjet plant 60 kg Taxi; bei 20 kg Plan waeren 10 kg
/// Unterschied schon 50 %.
const MIN_PLAN_ROLLEN_KG: f32 = 30.0;

/// Ab dieser Abweichung sagt ein Prozentwert nichts mehr — dann steht die
/// Differenz in Kilogramm. Siehe `Phase::als_kg`.
const MAX_LESBARER_PROZENTWERT: f32 = 60.0;

/// Unterhalb dieses geplanten Anflugverbrauchs ist der Prozentwert ohne
/// Aussage (Division durch fast null).
///
/// 15 kg statt der urspruenglichen 50: Die Korpus-Pruefung hat gezeigt, dass
/// 50 kg rund 17 % aller Fluege die Anflugphase gekostet haetten — praktisch
/// die gesamte GA- und Bizjet-Flotte (E55P, BE58, DA40, PC12 …). Genau diesen
/// Mustern waere dann nur die andere Phase geblieben.
const MIN_PLAN_ANFLUG_KG: f32 = 15.0;

/// Mindestgroesse fuer die Phase „bis Sinkflug". Bei 18 kg Planverbrauch
/// (AC11, PA24, DA40 im Bestand) macht 1 kg Ablesefehler schon 7 % aus —
/// dreistellige Prozentwerte sind dort normal und sagen nichts. Dann lieber
/// keine Phase; Reserve, Leiter und Extra bleiben sichtbar.
const MIN_PLAN_BIS_TOD_KG: f32 = 120.0;

fn runde1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

fn pct(ist: f32, plan: f32) -> f32 {
    runde1((ist - plan) / plan * 100.0)
}

fn positiv(v: Option<f32>) -> Option<f32> {
    v.filter(|x| x.is_finite() && *x > 0.0)
}

fn nicht_negativ(v: Option<f32>) -> Option<f32> {
    v.filter(|x| x.is_finite() && *x >= 0.0)
}

/// Die eine Rechnung. Siehe Modulkopf.
pub fn auswerten(e: &SpritEingang) -> SpritAuswertung {
    let takeoff = positiv(e.takeoff_fuel_kg);
    let landing = nicht_negativ(e.landing_fuel_kg);
    let vergleich = positiv(e.sprit_bei_vergleichspunkt_kg);
    let plan_bis = positiv(e.plan_bis_vergleichspunkt_kg);
    let planned_burn = positiv(e.planned_burn_kg);
    let reserve = positiv(e.planned_reserve_kg);

    // ---- Phasen: nur bei plausiblem Tank, vollstaendigen Werten und
    // tatsaechlich geflogenem Plan ----
    let (bis_sinkflug, anflug) = if e.tank_plausibel && !e.ausweichflug {
        // Beim Einstieg in der Luft zaehlt der Plan erst ab dem Einstiegsort.
        let plan_bis_phase = if e.einstieg_in_der_luft {
            match (plan_bis, nicht_negativ(e.plan_bis_einstieg_kg)) {
                (Some(vp), Some(ein)) if vp > ein => Some(vp - ein),
                _ => None,
            }
        } else {
            plan_bis
        };
        let bis = match (takeoff, vergleich, plan_bis_phase) {
            (Some(to), Some(vp), Some(plan)) if to > vp && plan >= MIN_PLAN_BIS_TOD_KG => {
                Some(Phase {
                    // Prozent aus DENSELBEN Zahlen, die daneben stehen — sonst
                    // passen angezeigter Wert und angezeigte Abweichung nicht
                    // zusammen.
                    ist_kg: (to - vp).round(),
                    plan_kg: plan.round(),
                    abweichung_pct: pct((to - vp).round(), plan.round()),
                    // „bis Sinkflug" bleibt IMMER in Prozent: Der Plan-Anteil
                    // ist dort gross genug, dass die Quote etwas aussagt. Sonst
                    // saehe die GA-Flotte hier Kilogramm und anderswo Prozent.
                    als_kg: false,
                })
            }
            _ => None,
        };
        // Trip aus demselben Navlog wie `plan_bis` — sonst driftet der Schnitt.
        let trip_fuer_phasen = positiv(e.plan_trip_navlog_kg).or(planned_burn);
        let an = match (vergleich, landing, trip_fuer_phasen, plan_bis) {
            // `burn - plan` ist der geplante Anflugverbrauch. Liegt der TOD
            // kurz vor dem Ziel, wird er winzig und jede Abweichung ergibt
            // dreistellige Prozentwerte ohne Aussage — dann lieber nichts.
            (Some(vp), Some(ldg), Some(burn), Some(plan))
                if vp > ldg && burn - plan >= MIN_PLAN_ANFLUG_KG =>
            {
                Some(Phase {
                    ist_kg: (vp - ldg).round(),
                    plan_kg: (burn - plan).round(),
                    abweichung_pct: pct((vp - ldg).round(), (burn - plan).round()),
                    als_kg: pct((vp - ldg).round(), (burn - plan).round()).abs()
                        > MAX_LESBARER_PROZENTWERT,
                })
            }
            _ => None,
        };
        (bis, an)
    } else {
        (None, None)
    };

    // ---- Rollen, vor dem Start und nach der Landung ----
    //
    // Beide Marken sind an ein Ereignis gerastet, nicht laufend gefuehrt —
    // sonst stuende hier bei jedem zweiten Flug Unsinn (siehe
    // `sprit_boden_marken`).
    let rollen_vor_start = match (
        positiv(e.engine_start_fuel_kg),
        takeoff,
        positiv(e.planned_taxi_kg),
    ) {
        (Some(an), Some(to), Some(plan)) if an > to && plan >= MIN_PLAN_ROLLEN_KG => Some(Phase {
            ist_kg: (an - to).round(),
            plan_kg: plan.round(),
            abweichung_pct: pct((an - to).round(), plan.round()),
            // Beim Rollen ist der Plan klein (ein Bizjet plant 60 kg), aber
            // die Abweichung auch — hier bleibt Prozent lesbar.
            als_kg: false,
        }),
        _ => None,
    };
    let rollen_nach_landung_kg = rollen_nach_landung(e.landing_fuel_kg, e.engine_off_fuel_kg);

    // ---- Reserve ----
    let reserve_status = match (reserve, landing) {
        _ if !e.tank_plausibel => Reserve::NichtPruefbar {
            grund: "tank_unplausibel".into(),
        },
        (None, _) => Reserve::NichtPruefbar {
            grund: "kein_ofp".into(),
        },
        (_, None) => Reserve::NichtPruefbar {
            grund: "kein_landesprit".into(),
        },
        (Some(res), Some(ldg)) => {
            // Deckel: Bei Kleinflugzeugen ergeben sich Quoten wie 1 298 %
            // (BE60: 19 kg Reserve, 247 kg gelandet). Die Zahl waere richtig
            // und trotzdem ohne Aussage.
            let quote = runde1((ldg / res * 100.0).min(999.0));
            if ldg >= res {
                Reserve::Intakt { quote_pct: quote }
            } else {
                Reserve::Unterschritten { quote_pct: quote }
            }
        }
    };
    // Aus den GERUNDETEN Werten, die auch angezeigt werden — dann geht die
    // Rechnung in der Anzeige immer auf (gelandet − Reserve = Abstand), und
    // ein Datensatz ohne dieses Feld bekommt exakt dieselbe Zahl
    // (`reserve_abstand`, `reserveAbstand` in sprit.ts).
    let reserve_abstand_kg = match (reserve, landing) {
        (Some(res), Some(ldg)) => abstand_nach_status(&reserve_status, ldg.round() - res.round()),
        _ => None,
    };
    let badge = match reserve_status {
        Reserve::Intakt { .. } => Badge::Gruen,
        Reserve::Unterschritten { .. } => Badge::Gelb,
        Reserve::NichtPruefbar { .. } => Badge::Grau,
    };

    // ---- Leiter, Contingency, Extra ----
    let leiter = match (
        planned_burn,
        nicht_negativ(e.planned_contingency_kg),
        nicht_negativ(e.planned_alternate_kg),
        reserve,
        nicht_negativ(e.planned_extra_kg),
        positiv(e.planned_block_fuel_kg),
    ) {
        (Some(trip), Some(cont), Some(alt), Some(res), Some(extra), Some(block)) => {
            let taxi = nicht_negativ(e.planned_taxi_kg).unwrap_or(0.0);
            let posten = taxi + trip + cont + alt + res + extra;
            // Was der Block mehr enthaelt als die sechs Posten.
            let sonstiges = (block - posten).max(0.0);
            // Was an Bord war, aber nicht im Plan-Block.
            //
            // Am besten gemessen am Tankstand beim ANLASSEN: Dann ist die
            // Skala der Leiter (Block plus Uebertankung) genau der Tank beim
            // Anlassen, und Abhebe- und Lande-Marke sitzen auf ihren echten
            // Tankstaenden — gleich, ob mehr oder weniger gerollt wurde als
            // geplant. Mit „Abheben plus Plan-Taxi" wich die Marke um genau
            // den Unterschied im Rollsprit ab, und weniger Rollen erschien
            // als Uebertankung (QS-Vorschlag V-a, 18.09.2026). Der Rueckfall
            // bleibt fuer Fluege ohne Anlass-Marke.
            let tank_beim_anlassen = match (positiv(e.engine_start_fuel_kg), takeoff) {
                (Some(an), _) => Some(an),
                (None, Some(to)) if !e.einstieg_in_der_luft => Some(to + taxi),
                _ => None,
            };
            // Ueber- und Untertankung gegen DIESELBE Groesse, die die Grafik
            // als Plan-Stapel zeichnet: den Block, oder die Posten, wenn das
            // OFP mehr in die Posten plant als in den Block. Dann ist die
            // Skala (Stapel + Ueber − Unter) genau der Tank beim Anlassen,
            // und die Abhebe-Marke sitzt auf ihrem echten Wert (QS Runde 3).
            let stapel = block.max(posten);
            let uebertankung = tank_beim_anlassen
                .map(|t| (t - stapel).max(0.0))
                .unwrap_or(0.0);
            // Siehe `Leiter::untertankung_kg`.
            let untertankung = tank_beim_anlassen
                .map(|t| (stapel - t).max(0.0).min(extra))
                .unwrap_or(0.0);
            Some(Leiter {
                taxi_kg: taxi.round(),
                trip_kg: trip.round(),
                contingency_kg: cont.round(),
                alternate_kg: alt.round(),
                reserve_kg: res.round(),
                extra_kg: extra.round(),
                block_kg: block.round(),
                sonstiges_kg: sonstiges.round(),
                uebertankung_kg: uebertankung.round(),
                untertankung_kg: untertankung.round(),
            })
        }
        _ => None,
    };

    let (
        extra_getankt,
        extra_genutzt,
        extra_ungenutzt,
        contingency_verbraucht,
        contingency_genutzt_kg,
        alt_res_intakt,
    ) = match (&leiter, landing, e.tank_plausibel) {
        (Some(l), Some(ldg), true) => {
            // Was nach Rollen und Trip uebrig bleiben sollte — gemessen
            // an der SKALA DER LEITER: Block plus Uebertankung, also der
            // Tank beim Anlassen (bei Untertankung der Block).
            //
            // Bis v1.7.36-Entwurf stand hier „Abhebe-Tankstand minus
            // Trip". Das war fuer sich richtig, aber eine andere Rechnung
            // als die Grafik: Wer weniger rollte als geplant, bekam die
            // Ersparnis als Uebertankung gezeichnet und als Reserve fuer
            // den Trip gerechnet, und die Landemarke lag um genau diese
            // Differenz neben der Zeile „Extra ungenutzt" (QS-Vorschlag
            // V-a, 18.09.2026). Mit derselben Skala auf beiden Seiten
            // trifft die Marke die Zeile immer — und seit v1.7.36 zaehlt
            // der Flug ohnehin vom Anlassen an, nicht vom Abheben.
            //
            // Beim Einstieg in der Luft gibt es keine Anlass-Marke; die
            // Skala ist dann der Block, und der Plan-Landestand ist der
            // des OFP.
            // Dieselbe Skala wie die Grafik: Plant ein OFP mehr in die
            // Posten als in den Block, zaehlen die Posten.
            let posten = l.taxi_kg
                + l.trip_kg
                + l.contingency_kg
                + l.alternate_kg
                + l.reserve_kg
                + l.extra_kg;
            let skala = l.block_kg.max(posten) + l.uebertankung_kg - l.untertankung_kg;
            let plan_landing = (skala - l.taxi_kg - l.trip_kg).max(0.0);
            let mehr = (plan_landing - ldg).max(0.0);
            let cont_genutzt = mehr.clamp(0.0, l.contingency_kg).round();
            // „Verbraucht" folgt dem GERUNDETEN Wert, den die Anzeige
            // nennt — sonst stuende „aufgebraucht — alle 238 kg" neben
            // einem Kennzeichen „nicht verbraucht" (QS v1.7.38).
            let cont_verbraucht = l.contingency_kg > 0.0 && cont_genutzt >= l.contingency_kg;
            // Was vom Extra tatsaechlich an Bord war.
            let extra_an_bord = (l.extra_kg - l.untertankung_kg).max(0.0);
            let genutzt = (mehr - l.contingency_kg).clamp(0.0, extra_an_bord).round();
            (
                Some(extra_an_bord),
                Some(genutzt),
                Some((extra_an_bord - genutzt).round()),
                Some(cont_verbraucht),
                Some(cont_genutzt),
                Some(ldg >= l.alternate_kg + l.reserve_kg),
            )
        }
        _ => (None, None, None, None, None, None),
    };

    SpritAuswertung {
        fassung: SPRIT_AUSWERTUNG_FASSUNG,
        bis_sinkflug,
        anflug,
        zeit_unter_schwelle_min: nicht_negativ(e.zeit_unter_schwelle_s).map(|s| runde1(s / 60.0)),
        schwelle_ft: positiv(e.schwelle_ft).map(|v| v.round()),
        strecke_anflug_nm: positiv(e.strecke_ab_vergleichspunkt_nm).map(|v| v.round()),
        // Nach einem Ausweichflug gab es diesen Plan nicht — die geflogene
        // Strecke bleibt eine Tatsache, der Plan dazu nicht.
        plan_strecke_anflug_nm: if e.ausweichflug {
            None
        } else {
            positiv(e.plan_rest_nm).map(|v| v.round())
        },
        reserve: reserve_status,
        reserve_kg: reserve.map(|v| v.round()),
        reserve_abstand_kg,
        takeoff_fuel_kg: takeoff.map(|v| v.round()),
        landing_fuel_kg: landing.map(|v| v.round()),
        extra_getankt_kg: extra_getankt,
        extra_genutzt_kg: extra_genutzt,
        extra_ungenutzt_kg: extra_ungenutzt,
        contingency_verbraucht,
        contingency_genutzt_kg,
        alternate_und_reserve_intakt: alt_res_intakt,
        leiter,
        rollen_vor_start,
        rollen_nach_landung_kg,
        einstieg_in_der_luft: e.einstieg_in_der_luft,
        badge,
        // Die Tabelle füllt der Client (`sprit_wegpunkt_zeilen`) — hier
        // fehlt ihm die Route.
        wegpunkte: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Sprit je Wegpunkt ────────────────────────────────────────────

    fn wp(
        ident: &str,
        plan: f32,
        min: f32,
        ist: Option<f32>,
        zustand: WegpunktZustand,
    ) -> Wegpunkt {
        Wegpunkt {
            ident: ident.into(),
            plan_an_bord_kg: Some(plan),
            min_an_bord_kg: Some(min),
            ist_an_bord_kg: ist,
            zustand,
            ..Default::default()
        }
    }

    #[test]
    fn fuel_check_rechnet_den_mehrverbrauch_auf_die_landung_hoch() {
        // Jetzt Plan 5480, Ist 5360 — 120 kg unter Plan. Rest bis zur
        // Landung laut Plan 960 kg → 5360 − 960 = 4400. Die Abweichung
        // wird als BETRAG weitergetragen, nicht gestreckt: 4520 − 120.
        let (hoch, ampel) = fuel_check(
            5360.0,
            5480.0,
            4520.0,
            Some(3790.0),
            Some(4400.0),
            Some(160.0),
        )
        .unwrap();
        assert!((hoch - 4400.0).abs() < 3.0, "{hoch}");
        // 4400 liegt ueber 4520 − 160 = 4360: die Contingency deckt es.
        assert_eq!(ampel, Ampel::Gruen);
    }

    #[test]
    fn fuel_check_gruen_wenn_die_contingency_deckt() {
        let (_, ampel) = fuel_check(
            5440.0,
            5480.0,
            4520.0,
            Some(3790.0),
            None,
            Some(160.0),
        )
        .unwrap();
        assert_eq!(ampel, Ampel::Gruen);
    }

    #[test]
    fn fuel_check_weniger_verbraucht_ist_immer_gruen() {
        // Mehr im Tank als geplant — nichts daran ist schlimm (Thomas).
        let (_, ampel) = fuel_check(
            5700.0,
            5480.0,
            4520.0,
            Some(3790.0),
            Some(4400.0),
            Some(0.0),
        )
        .unwrap();
        assert_eq!(ampel, Ampel::Gruen);
    }

    #[test]
    fn fuel_check_rot_unter_dem_ofp_minimum() {
        // Jetzt schon unter dem Minimum an diesem Wegpunkt.
        let (_, a) = fuel_check(
            4300.0,
            5480.0,
            4520.0,
            Some(3790.0),
            Some(4400.0),
            Some(160.0),
        )
        .unwrap();
        assert_eq!(a, Ampel::Rot);
        // Oder hochgerechnet bei der Landung darunter: 4700 − 960 = 3740.
        let (h, a) = fuel_check(
            4700.0,
            5480.0,
            4520.0,
            Some(3790.0),
            Some(4000.0),
            Some(160.0),
        )
        .unwrap();
        assert!(h < 3790.0, "{h}");
        assert_eq!(a, Ampel::Rot);
    }

    #[test]
    fn fuel_check_kurz_nach_dem_abheben_bleibt_ruhig() {
        // 40 kg Plan-Verbrauch, 20 kg darunter. Frueher machte das
        // Verhaeltnis daraus Rauschen; jetzt steht schlicht der Plan
        // minus 20 kg da.
        let (hoch, _) = fuel_check(
            7640.0,
            7660.0,
            4520.0,
            None,
            None,
            None,
        )
        .unwrap();
        assert!((hoch - (7640.0 - 3140.0)).abs() < 0.5, "{hoch}");
    }

    #[test]
    fn uebersprungener_wegpunkt_wird_aus_den_nachbarn_gerechnet_nicht_gemessen() {
        let mut z = vec![
            wp(
                "EDDL",
                7700.0,
                5000.0,
                Some(7700.0),
                WegpunktZustand::Gemessen,
            ),
            wp(
                "KORED",
                5930.0,
                4700.0,
                Some(5870.0),
                WegpunktZustand::Gemessen,
            ),
            wp(
                "ADEKA",
                5710.0,
                4600.0,
                None,
                WegpunktZustand::Uebersprungen,
            ),
            wp(
                "RESMI",
                5480.0,
                4400.0,
                Some(5360.0),
                WegpunktZustand::Gemessen,
            ),
            wp("LUMAS", 5330.0, 4300.0, None, WegpunktZustand::Offen),
            wp("LEPA", 4520.0, 3790.0, None, WegpunktZustand::Offen),
        ];
        wegpunkte_auswerten(&mut z, Some(160.0));
        // Abweichung −60 bei KORED, −120 bei RESMI; ADEKA liegt nach Plan-
        // Verbrauch knapp in der Mitte → etwa −89.
        let adeka = z[2].ist_an_bord_kg.unwrap();
        assert!((adeka - (5710.0 - 89.0)).abs() < 2.0, "{adeka}");
        assert_eq!(
            z[2].zustand,
            WegpunktZustand::Uebersprungen,
            "bleibt als übersprungen gekennzeichnet"
        );
        // Offene Zeilen bekommen keine Ampel und keinen Ist-Wert.
        assert!(z[4].ampel.is_none() && z[4].ist_an_bord_kg.is_none());
        // Gemessene schon.
        assert!(z[3].ampel.is_some() && z[3].landung_hochgerechnet_kg.is_some());
    }

    #[test]
    fn der_steigflug_faerbt_den_reiseflug_nicht_mehr_rot() {
        // DLH #1439 am 20.09.2026, echte Zahlen aus Thomas' Bild.
        // Der Steigflug kostete 4.545 kg mehr als geplant. Auf die ganze
        // Reststrecke hochgerechnet ergab das bei BRADD "-3748 kg bei der
        // Landung" — rot —, zwei Fixe spaeter "+20 209 kg" — gruen.
        // Mit TOC als Faktor-Basis steht eine ruhige Reihe da.
        let mut z = vec![
            wp("BETTE", 103417.0, 20000.0, Some(107282.0), WegpunktZustand::Gemessen),
            wp("ACK", 97262.0, 20000.0, Some(96582.0), WegpunktZustand::Gemessen),
            wp("TOC", 97221.0, 20000.0, Some(96548.0), WegpunktZustand::Gemessen),
            wp("BRADD", 92992.0, 20000.0, Some(92688.0), WegpunktZustand::Gemessen),
            wp("PORTI", 77551.0, 20000.0, Some(78884.0), WegpunktZustand::Gemessen),
            wp("EDDM", 24109.0, 20000.0, None, WegpunktZustand::Offen),
        ];
        wegpunkte_auswerten(&mut z, Some(2000.0));

        // Das Flugzeug selbst sagte 23,3 t bei der Landung. Was im
        // Steigflug zu viel wegging, fehlt weiter im Tank — es wird nur
        // nicht mehr auf die Reststrecke hochskaliert.
        const FMS: f32 = 23300.0;
        let ack = z[1].landung_hochgerechnet_kg.expect("ACK wird hochgerechnet");
        let brad = z[3].landung_hochgerechnet_kg.expect("BRADD wird hochgerechnet");
        let porti = z[4].landung_hochgerechnet_kg.expect("PORTI wird hochgerechnet");
        // Die alte Rechnung lag bei BRADD 27 t daneben und war negativ.
        for (name, wert) in [("ACK", ack), ("BRADD", brad)] {
            assert!(
                (wert - FMS).abs() < 600.0,
                "{name} soll nahe am FMS-Wert liegen: {wert} statt {FMS}",
            );
        }
        // PORTI ist die schwaechste Zeile — aber immer noch im Rahmen,
        // waehrend die alte Rechnung dort 20 209 kg sagte.
        assert!((porti - FMS).abs() < 2500.0, "PORTI: {porti}");
        // Der eigentliche Fehler war der SPRUNG: 24 t zwischen zwei Zeilen
        // desselben ruhigen Reiseflugs.
        assert!(
            (porti - brad).abs() < 2000.0,
            "zwei Fixe im Reiseflug duerfen nicht um {} kg auseinanderliegen",
            (porti - brad).abs(),
        );
        assert_eq!(z[3].ampel, Some(Ampel::Gruen), "BRADD war nie in Not");
    }

    #[test]
    fn die_ersten_minuten_rechnen_jetzt_mit() {
        // DLH #1439: 292 kg mehr getankt, Startschub gegen ein winziges
        // Planstück. Mit der Hochskalierung standen hier drei ROTE Zeilen,
        // die sich ab dem vierten Fix von selbst erledigten; dagegen half
        // nur, die ersten Zeilen ganz zu verschweigen. Ohne Skalierung
        // rechnen sie mit — und bleiben von allein ruhig.
        let mut z = vec![
            wp(
                "DER24",
                9491.0,
                8000.0,
                Some(9783.0),
                WegpunktZustand::Gemessen,
            ),
            wp(
                "GEMMA",
                9245.0,
                7800.0,
                Some(9377.0),
                WegpunktZustand::Gemessen,
            ),
            wp(
                "TEA",
                8384.0,
                7000.0,
                Some(8466.0),
                WegpunktZustand::Gemessen,
            ),
            wp("EDDF", 3791.0, 2800.0, None, WegpunktZustand::Offen),
        ];
        wegpunkte_auswerten(&mut z, Some(300.0));
        // Jede gemessene Zeile bekommt ihre Zahl, schon 246 kg nach dem
        // Abheben. Der Riegel haette die beiden ersten verschwiegen.
        for i in 0..3 {
            assert!(
                z[i].landung_hochgerechnet_kg.is_some() && z[i].ampel.is_some(),
                "Zeile {i} sollte gerechnet werden",
            );
            assert_ne!(z[i].ampel, Some(Ampel::Rot), "Zeile {i} war nie in Not");
        }
        // Und sie stehen ruhig beieinander: 4.083 / 3.923 / 3.873 kg.
        let werte: Vec<f32> = (0..3)
            .map(|i| z[i].landung_hochgerechnet_kg.unwrap())
            .collect();
        let spanne = werte.iter().cloned().fold(f32::MIN, f32::max)
            - werte.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spanne < 300.0, "Spanne {spanne} kg ueber die ersten Fixe");
    }

    #[test]
    fn ein_kurzer_flug_bekommt_trotzdem_eine_ampel() {
        // Abnahme 20.09.2026: Mit einer absoluten Untergrenze von 150 kg
        // blieb ein Trip von 300 kg zur Haelfte stumm, ein Trip unter
        // 150 kg bis zur Landung. Genau die kleinen Muster fliegen aber
        // am dichtesten an ihren Reserven.
        let mut z = vec![
            wp("EDDL", 900.0, 400.0, Some(900.0), WegpunktZustand::Gemessen),
            wp(
                "MITTE",
                800.0,
                330.0,
                Some(760.0),
                WegpunktZustand::Gemessen,
            ),
            wp("EDDK", 700.0, 260.0, None, WegpunktZustand::Offen),
        ];
        wegpunkte_auswerten(&mut z, Some(40.0));
        assert!(
            z[1].ampel.is_some(),
            "kurzer Flug ohne Ampel — 200 kg Trip, 100 kg Plan-Verbrauch"
        );
        assert!(z[1].landung_hochgerechnet_kg.is_some());
    }

    #[test]
    fn einstieg_in_der_luft_bekommt_seine_ampel() {
        // Client erst im Reiseflug gestartet: Der Abflug trägt keine
        // Messung. Seit dem Wegfall der Hochskalierung (20.09.2026) hängt
        // die Rechnung an keinem Startpunkt mehr — der Tankstand hier
        // gegen den geplanten Rest, fertig. Der Test hält fest, dass ein
        // Einstieg in der Luft trotzdem eine Ampel bekommt und die
        // Contingency-Grenze dort genauso greift.
        let mut z = vec![
            wp("EDDL", 8000.0, 1500.0, None, WegpunktZustand::Offen),
            wp(
                "KORED",
                5000.0,
                1400.0,
                Some(5000.0),
                WegpunktZustand::Gemessen,
            ),
            wp(
                "RESMI",
                4000.0,
                1300.0,
                Some(3850.0),
                WegpunktZustand::Gemessen,
            ),
            wp("LEPA", 2000.0, 1300.0, None, WegpunktZustand::Offen),
        ];
        wegpunkte_auswerten(&mut z, Some(100.0));
        // 3.850 − 2.000 geplanter Rest = 1.850, unter 2.000 − 100.
        assert_eq!(z[2].landung_hochgerechnet_kg, Some(1850.0));
        assert_eq!(z[2].ampel, Some(Ampel::Gelb));
        assert!(z[0].ampel.is_none(), "der Abflug wurde nicht gemessen");
    }

    #[test]
    fn uebersprungen_ohne_gemessenen_nachfolger_bleibt_leer() {
        // Keine Zahl erfinden, wenn es danach keine Messung gibt.
        let mut z = vec![
            wp(
                "EDDL",
                7700.0,
                5000.0,
                Some(7700.0),
                WegpunktZustand::Gemessen,
            ),
            wp(
                "ADEKA",
                5710.0,
                4600.0,
                None,
                WegpunktZustand::Uebersprungen,
            ),
            wp("LEPA", 4520.0, 3790.0, None, WegpunktZustand::Offen),
        ];
        wegpunkte_auswerten(&mut z, Some(160.0));
        assert!(z[1].ist_an_bord_kg.is_none());
        assert!(z[1].ampel.is_none());
    }

    #[test]
    fn altbestand_ohne_wegpunkte_bleibt_lesbar_und_sendet_nichts() {
        let a: SpritAuswertung = serde_json::from_str(r#"{"fassung":3}"#).unwrap();
        assert!(a.wegpunkte.is_empty());
        let j = serde_json::to_value(&a).unwrap();
        assert!(
            j.get("wegpunkte").is_none(),
            "leere Tabelle wird nicht gesendet"
        );
    }

    /// DLH370, 17.09.2026, ESSA→EDDM, A380 D-AIMK — die echten Zahlen aus
    /// Track und OFP. Sie stehen so im Entwurf, den Thomas freigegeben hat.
    fn dlh370() -> SpritEingang {
        SpritEingang {
            planned_burn_kg: Some(21_218.0),
            plan_bis_vergleichspunkt_kg: Some(19_353.0),
            // Echte Navlog-Summe nach dem TOD-Fix aus dem OFP dieses Fluges
            // (die 191 hier waren frueher handgeschrieben und falsch).
            plan_rest_nm: Some(139.0),
            takeoff_fuel_kg: Some(40_919.0),
            sprit_bei_vergleichspunkt_kg: Some(22_231.0),
            landing_fuel_kg: Some(16_770.0),
            strecke_ab_vergleichspunkt_nm: Some(182.0),
            zeit_unter_schwelle_s: Some(948.0),
            schwelle_ft: Some(9_487.0),
            planned_taxi_kg: Some(998.0),
            planned_contingency_kg: Some(1_061.0),
            planned_alternate_kg: Some(8_273.0),
            planned_reserve_kg: Some(4_916.0),
            planned_extra_kg: Some(5_178.0),
            planned_block_fuel_kg: Some(41_644.0),
            // Die echten Boden-Marken dieses Fluges: angelassen mit
            // 41 644 kg, abgestellt mit 16 121 kg.
            engine_start_fuel_kg: Some(41_644.0),
            engine_off_fuel_kg: Some(16_121.0),
            einstieg_in_der_luft: false,
            plan_bis_einstieg_kg: None,
            // Kein Navlog-Trip: Der Golden-Test deckt damit den Rueckfall auf
            // den OFP-Trip ab. Den Navlog-Weg prueft
            // `sprit_phasen_summieren_sich_auf_den_navlog_trip`.
            plan_trip_navlog_kg: None,
            tank_plausibel: true,
            ausweichflug: false,
        }
    }

    /// Ein Datensatz der Fassung 1 — ohne `takeoff_fuel_kg` — muss lesbar
    /// bleiben.
    ///
    /// Sonst kippt `storage::read_all` die GANZE `landings.json` in den
    /// Fehlerzweig (sie wird als ein einziger `Vec<LandingRecord>` gelesen),
    /// und die komplette lokale Landungs-Historie verschwindet aus der
    /// Oberflaeche. Der Test faellt, sobald jemand `#[serde(default)]` an
    /// `SpritAuswertung` entfernt oder ein neues Feld ohne Rueckfall ergaenzt.
    #[test]
    fn sprit_auswertung_aeltere_fassung_bleibt_lesbar() {
        // Genau die Felder der Fassung 1, kein `takeoff_fuel_kg`.
        let alt = r#"{
            "fassung": 1,
            "bis_sinkflug": { "ist_kg": 18688.0, "plan_kg": 19353.0, "abweichung_pct": -3.4 },
            "anflug": null,
            "zeit_unter_schwelle_min": 15.8,
            "schwelle_ft": 9487.0,
            "strecke_anflug_nm": 182.0,
            "plan_strecke_anflug_nm": 139.0,
            "reserve": { "status": "intakt", "quote_pct": 341.1 },
            "reserve_kg": 4916.0,
            "landing_fuel_kg": 16770.0,
            "extra_getankt_kg": 5178.0,
            "extra_genutzt_kg": 1870.0,
            "extra_ungenutzt_kg": 3308.0,
            "contingency_verbraucht": false,
            "alternate_und_reserve_intakt": true,
            "leiter": null,
            "badge": "gruen"
        }"#;
        let a: SpritAuswertung = serde_json::from_str(alt).expect("Fassung 1 muss lesbar sein");
        assert_eq!(
            a.fassung, 1,
            "die Fassung bleibt stehen, sie wird nicht hochgelogen"
        );
        assert_eq!(a.takeoff_fuel_kg, None, "das fehlende Feld wird zu None");
        // Gegenprobe: Die uebrigen Werte kommen unveraendert an — der
        // Rueckfall darf nicht alles auf Default ziehen.
        assert_eq!(a.landing_fuel_kg, Some(16_770.0));
        assert_eq!(a.plan_strecke_anflug_nm, Some(139.0));
        assert_eq!(a.badge, Badge::Gruen);
        assert_eq!(a.reserve, Reserve::Intakt { quote_pct: 341.1 });
    }

    /// Eine Phase der Fassungen 1 und 2 traegt kein `als_kg` — sie muss
    /// trotzdem lesbar sein, und zwar als „Prozent" (so wurde sie damals
    /// angezeigt).
    #[test]
    fn sprit_phase_ohne_als_kg_bleibt_lesbar() {
        let alt = r#"{"ist_kg": 5461.0, "plan_kg": 1865.0, "abweichung_pct": 192.8}"#;
        let p: Phase = serde_json::from_str(alt).expect("Phase ohne als_kg muss lesbar sein");
        assert!(!p.als_kg);
        assert_eq!(
            p.ist_kg, 5_461.0,
            "Gegenprobe: die uebrigen Werte kommen an"
        );
    }

    /// Gegenprobe zur Rueckwaertstoleranz: Ein voellig leeres Objekt ergibt
    /// den ehrlichen Leerstand — Grau und „nicht pruefbar", nicht Gruen.
    #[test]
    fn sprit_auswertung_leeres_objekt_ist_grau_und_nicht_pruefbar() {
        let a: SpritAuswertung = serde_json::from_str("{}").expect("leer muss lesbar sein");
        assert_eq!(a.badge, Badge::Grau);
        assert!(matches!(a.reserve, Reserve::NichtPruefbar { .. }));
        assert_eq!(a.fassung, 0, "keine Fassung behauptet, wo keine stand");
    }

    #[test]
    fn sprit_auswertung_dlh370_phasen() {
        let a = auswerten(&dlh370());
        let bis = a.bis_sinkflug.expect("bis Sinkflug");
        assert_eq!(bis.ist_kg, 18_688.0);
        assert_eq!(bis.plan_kg, 19_353.0);
        assert_eq!(bis.abweichung_pct, -3.4);
        let an = a.anflug.expect("Anflug");
        assert_eq!(an.ist_kg, 5_461.0);
        assert_eq!(an.plan_kg, 1_865.0);
        assert_eq!(an.abweichung_pct, 192.8);
        assert_eq!(a.zeit_unter_schwelle_min, Some(15.8));
        assert_eq!(a.strecke_anflug_nm, Some(182.0));
        assert_eq!(a.plan_strecke_anflug_nm, Some(139.0));
    }

    #[test]
    fn sprit_auswertung_dlh370_reserve_und_extra() {
        let a = auswerten(&dlh370());
        assert_eq!(a.reserve, Reserve::Intakt { quote_pct: 341.1 });
        assert_eq!(a.reserve_abstand_kg, Some(11_854.0), "16 770 − 4 916");
        assert_eq!(a.badge, Badge::Gruen);
        assert_eq!(a.extra_getankt_kg, Some(5_178.0));
        // Vom Anlassen an gerechnet (Tank 41 644 = Block): geplant gelandet
        // mit 41 644 − 998 Taxi − 21 218 Trip = 19 428, tatsaechlich 16 770.
        // 2 658 mehr, davon traegt die Contingency 1 061, das Extra 1 597.
        // Bis v1.7.36-Entwurf standen hier 1 870 / 3 308 — dort zaehlte die
        // Rollersparnis von 273 kg als Uebertankung (QS-Vorschlag V-a).
        assert_eq!(a.extra_genutzt_kg, Some(1_597.0));
        assert_eq!(a.extra_ungenutzt_kg, Some(3_581.0));
        assert_eq!(a.contingency_verbraucht, Some(true));
        assert_eq!(a.alternate_und_reserve_intakt, Some(true));
        let l = a.leiter.expect("Leiter");
        assert_eq!(l.block_kg, 41_644.0);
        assert_eq!(
            l.taxi_kg + l.trip_kg + l.contingency_kg + l.alternate_kg + l.reserve_kg + l.extra_kg,
            41_644.0
        );
    }

    /// EDDH→EFHK, A350: 1 835 kg gelandet bei 2 647 kg Final Reserve.
    /// Gelb, 69 % — und nichts weiter. Keine Note, keine Blockierung.
    #[test]
    fn sprit_auswertung_unter_reserve_ist_gelb() {
        let mut e = dlh370();
        e.planned_reserve_kg = Some(2_647.0);
        e.landing_fuel_kg = Some(1_835.0);
        let a = auswerten(&e);
        assert_eq!(a.reserve, Reserve::Unterschritten { quote_pct: 69.3 });
        assert_eq!(a.badge, Badge::Gelb);
        assert_eq!(a.alternate_und_reserve_intakt, Some(false));
    }

    /// Manual-Flug ohne OFP: keine Plan-Werte → Grau, keine Phasen, keine
    /// Leiter. Der Landesprit selbst bleibt sichtbar.
    #[test]
    fn sprit_auswertung_ohne_ofp_ist_grau() {
        let e = SpritEingang {
            takeoff_fuel_kg: Some(3_000.0),
            landing_fuel_kg: Some(1_200.0),
            tank_plausibel: true,
            ..Default::default()
        };
        let a = auswerten(&e);
        assert_eq!(a.badge, Badge::Grau);
        assert_eq!(
            a.reserve,
            Reserve::NichtPruefbar {
                grund: "kein_ofp".into()
            }
        );
        assert!(a.bis_sinkflug.is_none() && a.anflug.is_none() && a.leiter.is_none());
        assert_eq!(a.landing_fuel_kg, Some(1_200.0));
    }

    /// Unplausibler Tankwert (Auslesefehler): lieber „nicht pruefbar" als
    /// eine falsche Warnung — und keine Phasen aus falschen Zahlen.
    #[test]
    fn sprit_auswertung_tank_unplausibel_warnt_nicht() {
        let mut e = dlh370();
        e.landing_fuel_kg = Some(7.0);
        e.tank_plausibel = false;
        let a = auswerten(&e);
        assert_eq!(a.badge, Badge::Grau);
        assert_eq!(
            a.reserve,
            Reserve::NichtPruefbar {
                grund: "tank_unplausibel".into()
            }
        );
        assert!(a.bis_sinkflug.is_none() && a.anflug.is_none());
        assert_eq!(a.extra_genutzt_kg, None);
    }

    /// Wer sparsamer landet als geplant, hat Contingency und Extra nicht
    /// angeruehrt — genutzt 0, ungenutzt alles.
    #[test]
    fn sprit_auswertung_sparsam_laesst_extra_ganz() {
        let mut e = dlh370();
        e.landing_fuel_kg = Some(20_000.0);
        let a = auswerten(&e);
        assert_eq!(a.contingency_verbraucht, Some(false));
        assert_eq!(a.extra_genutzt_kg, Some(0.0));
        assert_eq!(a.extra_ungenutzt_kg, Some(5_178.0));
        assert!(matches!(a.reserve, Reserve::Intakt { .. }));
    }

    /// Neustart mitten im Flug, Vergleichspunkt verpasst: Phasen fehlen,
    /// Reserve und Leiter bleiben — die Sektion zeigt, was sie hat.
    #[test]
    fn sprit_auswertung_ohne_vergleichspunkt_zeigt_den_rest() {
        let mut e = dlh370();
        e.sprit_bei_vergleichspunkt_kg = None;
        let a = auswerten(&e);
        assert!(a.bis_sinkflug.is_none() && a.anflug.is_none());
        assert_eq!(a.badge, Badge::Gruen);
        assert!(a.leiter.is_some());
        assert_eq!(a.extra_genutzt_kg, Some(1_597.0));
    }

    /// Die Grenzen MIN_PLAN_ANFLUG_KG (15) und MIN_PLAN_BIS_TOD_KG (120)
    /// genau an ihrer Kante — vorher war keine von beiden abgesteckt.
    #[test]
    fn sprit_auswertung_grenzen_genau_getroffen() {
        let bau = |plan_trip: f32, plan_bis: f32| SpritEingang {
            planned_burn_kg: Some(plan_trip),
            plan_bis_vergleichspunkt_kg: Some(plan_bis),
            takeoff_fuel_kg: Some(5_000.0),
            sprit_bei_vergleichspunkt_kg: Some(4_000.0),
            landing_fuel_kg: Some(3_500.0),
            tank_plausibel: true,
            ..Default::default()
        };
        // bis Sinkflug: 120 kg Plan zaehlt, 119 nicht.
        assert!(auswerten(&bau(200.0, 120.0)).bis_sinkflug.is_some());
        assert!(auswerten(&bau(200.0, 119.0)).bis_sinkflug.is_none());
        // Anflug: 15 kg Plan zaehlt (200 − 185), 14 nicht.
        assert!(auswerten(&bau(200.0, 185.0)).anflug.is_some());
        assert!(auswerten(&bau(200.0, 186.0)).anflug.is_none());
    }

    /// Ein Flug mit rund 100 kg Plan-Trip verliert „bis Sinkflug", behaelt
    /// aber den Anflug — der Fall, den der Pruefer als unerklaert bemaengelt
    /// hat. Der Test haelt fest, dass das kein Unfall ist.
    #[test]
    fn sprit_auswertung_kleiner_flug_behaelt_nur_den_anflug() {
        let e = SpritEingang {
            planned_burn_kg: Some(100.0),
            plan_bis_vergleichspunkt_kg: Some(80.0), // 20 kg Anflugplan
            takeoff_fuel_kg: Some(300.0),
            sprit_bei_vergleichspunkt_kg: Some(215.0),
            landing_fuel_kg: Some(190.0),
            tank_plausibel: true,
            ..Default::default()
        };
        let a = auswerten(&e);
        assert!(a.bis_sinkflug.is_none(), "80 kg Plan sind zu klein");
        assert!(a.anflug.is_some(), "20 kg Anflugplan sind messbar");
    }

    /// Wer mehr tankt als geplant, landet hoeher — das darf nicht als
    /// „Extra unangetastet" durchgehen.
    #[test]
    fn sprit_auswertung_extra_erkennt_tankern() {
        let mut e = dlh370();
        e.engine_start_fuel_kg = Some(44_644.0); // 3 t ueber Plan getankt
        e.takeoff_fuel_kg = Some(43_919.0);
        e.landing_fuel_kg = Some(19_770.0); // entsprechend hoeher gelandet
        let a = auswerten(&e);
        assert_eq!(a.leiter.as_ref().map(|l| l.uebertankung_kg), Some(3_000.0));
        // Ohne Korrektur waere hier „nichts verbraucht" herausgekommen.
        assert_eq!(a.extra_genutzt_kg, Some(1_597.0));
    }

    /// Die Landemarke der Grafik und die Zeile „Extra ungenutzt" nennen
    /// DIESELBE Zahl — gleich ob mehr oder weniger gerollt wurde als
    /// geplant, ob unter- oder uebertankt (QS-Vorschlag V-a, 18.09.2026).
    ///
    /// Die Grafik stapelt von rechts Reserve, Alternate, Uebertankung,
    /// Zusatzsprit und Extra; die Landemarke steht beim Landesprit. Was vom
    /// Extra in der Grafik uebrig ist, ist also der Landesprit minus alles
    /// rechts davon, auf das Extra begrenzt.
    #[test]
    fn sprit_landemarke_trifft_die_zeile_extra_ungenutzt() {
        for (anlassen, abheben) in [
            (41_644.0, 40_919.0), // DLH 370: 273 kg weniger gerollt
            (41_644.0, 40_346.0), // 300 kg mehr gerollt
            (41_300.0, 40_302.0), // 344 kg untertankt
            (44_644.0, 43_646.0), // 3 t uebertankt
        ] {
            for landung in [15_000.0, 16_770.0, 18_000.0] {
                let mut e = dlh370();
                e.engine_start_fuel_kg = Some(anlassen);
                e.takeoff_fuel_kg = Some(abheben);
                e.landing_fuel_kg = Some(landung);
                let a = auswerten(&e);
                let l = a.leiter.clone().expect("Leiter");
                let rechts = l.reserve_kg + l.alternate_kg + l.uebertankung_kg + l.sonstiges_kg;
                // Die Grafik zeichnet nur das Extra, das an Bord war.
                let grafik = (landung - rechts)
                    .clamp(0.0, l.extra_kg - l.untertankung_kg)
                    .round();
                assert_eq!(
                    a.extra_ungenutzt_kg,
                    Some(grafik),
                    "Anlassen {anlassen}, Abheben {abheben}, Landung {landung}"
                );
            }
        }
    }

    /// Untertankung: Was nie an Bord war, erscheint nicht als „genutzt" und
    /// loest keine „Contingency verbraucht" aus — und die Zeile „getankt"
    /// nennt, was wirklich an Bord war (QS Runde 3).
    #[test]
    fn sprit_untertankung_zaehlt_nicht_als_genutzt() {
        let mut e = dlh370();
        e.engine_start_fuel_kg = Some(41_300.0); // 344 kg unter Block
        e.takeoff_fuel_kg = Some(40_302.0); // 998 kg gerollt, wie geplant
                                            // Trip genau nach Plan: 40 302 − 21 218.
        e.landing_fuel_kg = Some(19_084.0);
        let a = auswerten(&e);
        let l = a.leiter.clone().expect("Leiter");
        assert_eq!(l.untertankung_kg, 344.0);
        assert_eq!(l.uebertankung_kg, 0.0);
        assert_eq!(a.extra_getankt_kg, Some(4_834.0), "5 178 − 344");
        assert_eq!(
            a.extra_genutzt_kg,
            Some(0.0),
            "planmaessig geflogen, nichts genutzt"
        );
        assert_eq!(a.extra_ungenutzt_kg, Some(4_834.0));
        assert_eq!(a.contingency_verbraucht, Some(false));
        // Mehr als das ganze Extra fehlt: gedeckelt, nie negativ.
        e.engine_start_fuel_kg = Some(30_000.0);
        e.takeoff_fuel_kg = Some(29_002.0);
        e.landing_fuel_kg = Some(7_784.0);
        let a = auswerten(&e);
        assert_eq!(a.leiter.as_ref().map(|l| l.untertankung_kg), Some(5_178.0));
        assert_eq!(a.extra_getankt_kg, Some(0.0));
    }

    /// Plant das OFP mehr in die Posten als in den Block, ist die Skala der
    /// Grafik trotzdem der Tank beim Anlassen — ueber-, unter- und genau
    /// betankt (QS Runde 3).
    #[test]
    fn sprit_skala_ist_der_tank_beim_anlassen_auch_bei_posten_ueber_block() {
        for anlassen in [40_500.0_f32, 41_644.0, 43_000.0] {
            let mut e = dlh370();
            e.planned_block_fuel_kg = Some(40_000.0); // Posten: 41 644
            e.engine_start_fuel_kg = Some(anlassen);
            e.takeoff_fuel_kg = Some(anlassen - 725.0);
            let l = auswerten(&e).leiter.expect("Leiter");
            let skala = l.block_kg.max(
                l.taxi_kg
                    + l.trip_kg
                    + l.contingency_kg
                    + l.alternate_kg
                    + l.reserve_kg
                    + l.extra_kg,
            ) + l.uebertankung_kg
                - l.untertankung_kg;
            assert_eq!(
                skala, anlassen,
                "Skala {skala} statt Tank beim Anlassen {anlassen}"
            );
        }
    }

    /// Einstieg in der Luft: „bis Sinkflug" rechnet nur das Stueck ab dem
    /// Einstiegsort — gegen den Plan ab dort, nicht ab dem Abflug.
    #[test]
    fn sprit_einstieg_in_der_luft_rechnet_ab_dem_einstieg() {
        let mut e = dlh370();
        e.einstieg_in_der_luft = true;
        e.engine_start_fuel_kg = None;
        e.plan_bis_einstieg_kg = Some(10_000.0);
        e.takeoff_fuel_kg = Some(31_500.0);
        let bis = auswerten(&e).bis_sinkflug.expect("bis Sinkflug");
        assert_eq!(
            bis.plan_kg, 9_353.0,
            "19 353 bis zum Messpunkt − 10 000 bis zum Einstieg"
        );
        assert_eq!(bis.ist_kg, 9_269.0, "31 500 − 22 231");
        // Liegt der Einstiegsort nicht auf der Route, gibt es keinen Wert —
        // statt eines kurzen Ist-Stuecks gegen den ganzen Plan.
        e.plan_bis_einstieg_kg = None;
        assert!(auswerten(&e).bis_sinkflug.is_none());
        // Der Anflug haengt nicht am Einstieg.
        assert!(auswerten(&e).anflug.is_some());
        // Und die Anzeige erfaehrt es — fuer die Beschriftung der Marke.
        assert!(auswerten(&e).einstieg_in_der_luft);
        assert!(!auswerten(&dlh370()).einstieg_in_der_luft);
    }

    /// Der Abstand zur Final Reserve: positiv darueber, negativ darunter,
    /// leer, wenn nicht pruefbar.
    #[test]
    fn sprit_reserve_abstand_in_kg() {
        let mut e = dlh370();
        e.landing_fuel_kg = Some(4_796.0);
        let a = auswerten(&e);
        assert!(matches!(a.reserve, Reserve::Unterschritten { .. }));
        assert_eq!(a.reserve_abstand_kg, Some(-120.0));
        e.tank_plausibel = false;
        assert_eq!(
            auswerten(&e).reserve_abstand_kg,
            None,
            "nicht pruefbar → kein Abstand"
        );
        e.tank_plausibel = true;
        e.planned_reserve_kg = None;
        assert_eq!(
            auswerten(&e).reserve_abstand_kg,
            None,
            "ohne OFP-Reserve kein Abstand"
        );
    }

    /// Knapp unterschritten, gerundet gleich: nie „− 0 kg". Und eine
    /// Auswertung ohne das Feld (vor v1.7.37 eingefroren) bekommt denselben
    /// Abstand ueber `reserve_abstand`.
    #[test]
    fn sprit_reserve_abstand_klemme_und_altbestand() {
        let mut e = dlh370();
        e.planned_reserve_kg = Some(4_916.4);
        e.landing_fuel_kg = Some(4_915.6);
        let a = auswerten(&e);
        assert!(matches!(a.reserve, Reserve::Unterschritten { .. }));
        assert_eq!(
            a.reserve_abstand_kg,
            Some(-1.0),
            "gerundet gleich, trotzdem darunter"
        );
        let mut alt = auswerten(&dlh370());
        alt.reserve_abstand_kg = None;
        assert_eq!(
            reserve_abstand(&alt),
            Some(11_854.0),
            "Altbestand aus den gespeicherten Werten"
        );
        alt.reserve = Reserve::NichtPruefbar {
            grund: "kein_ofp".into(),
        };
        assert_eq!(reserve_abstand(&alt), None);
    }

    /// Flug #1417 (19.09.2026): 86 von 238 kg Contingency genutzt — die
    /// Anzeige sagte „unberuehrt". Und eine Auswertung ohne das Feld
    /// bekommt ueber `contingency_genutzt` dieselbe Zahl.
    #[test]
    fn sprit_contingency_genutzt_in_kg() {
        let e = SpritEingang {
            planned_burn_kg: Some(2_115.0),
            planned_taxi_kg: Some(227.0),
            planned_contingency_kg: Some(238.0),
            planned_alternate_kg: Some(1_927.0),
            planned_reserve_kg: Some(1_130.0),
            planned_extra_kg: Some(0.0),
            planned_block_fuel_kg: Some(5_637.0),
            engine_start_fuel_kg: Some(5_628.0),
            takeoff_fuel_kg: Some(5_444.0),
            landing_fuel_kg: Some(3_209.0),
            tank_plausibel: true,
            ..Default::default()
        };
        let a = auswerten(&e);
        assert_eq!(
            a.contingency_verbraucht,
            Some(false),
            "nicht GANZ verbraucht"
        );
        assert_eq!(
            a.contingency_genutzt_kg,
            Some(86.0),
            "5 637 − 227 − 2 115 − 3 209"
        );
        let mut alt = a.clone();
        alt.contingency_genutzt_kg = None;
        assert_eq!(
            contingency_genutzt(&alt),
            Some(86.0),
            "Altbestand: dieselbe Zahl"
        );
        // Grenzen: unberuehrt 0, ganz verbraucht = geplant.
        let mut sparsam = e.clone();
        sparsam.landing_fuel_kg = Some(3_400.0);
        assert_eq!(auswerten(&sparsam).contingency_genutzt_kg, Some(0.0));
        let mut viel = e.clone();
        viel.landing_fuel_kg = Some(2_900.0);
        let v = auswerten(&viel);
        assert_eq!(v.contingency_genutzt_kg, Some(238.0));
        assert_eq!(v.contingency_verbraucht, Some(true));
        // Rundungsrand: 0,3 kg unter der Contingency — Text und
        // Kennzeichen sagen dasselbe („aufgebraucht").
        let mut knapp = e.clone();
        knapp.landing_fuel_kg = Some(3_295.0 - 237.7);
        let k = auswerten(&knapp);
        assert_eq!(k.contingency_genutzt_kg, Some(238.0));
        assert_eq!(k.contingency_verbraucht, Some(true));
        // Ohne Aussage keine Zahl.
        let mut leer = a.clone();
        leer.contingency_verbraucht = None;
        leer.contingency_genutzt_kg = None;
        assert_eq!(contingency_genutzt(&leer), None);
    }

    /// Rollen nach der Landung: eine Funktion, zwei Wege — dieselbe Zahl.
    #[test]
    fn sprit_rollen_nach_landung_eine_funktion() {
        assert_eq!(
            rollen_nach_landung(Some(16_770.0), Some(16_121.0)),
            Some(649.0)
        );
        assert_eq!(auswerten(&dlh370()).rollen_nach_landung_kg, Some(649.0));
        // Nachgetankt oder toter Sensor: kein Wert.
        assert_eq!(rollen_nach_landung(Some(16_770.0), Some(17_000.0)), None);
        assert_eq!(rollen_nach_landung(Some(16_770.0), Some(0.0)), None);
        assert_eq!(rollen_nach_landung(None, Some(16_121.0)), None);
    }

    /// Nach einem Ausweichflug wurde der Plan nicht geflogen: keine Phasen,
    /// keine Plan-Reststrecke. Die geflogenen Zahlen bleiben (QS-Befund P2-4).
    #[test]
    fn sprit_auswertung_divert_zeigt_keine_planphasen() {
        let mut e = dlh370();
        e.ausweichflug = true;
        let a = auswerten(&e);
        assert!(a.bis_sinkflug.is_none() && a.anflug.is_none());
        assert_eq!(a.plan_strecke_anflug_nm, None);
        // Tatsachen bleiben: Reserve, Landesprit, geflogene Strecke.
        assert!(matches!(a.reserve, Reserve::Intakt { .. }));
        assert_eq!(a.landing_fuel_kg, Some(16_770.0));
        assert_eq!(a.strecke_anflug_nm, Some(182.0));
    }

    /// Ein winziger geplanter Anflugverbrauch (TOD kurz vor dem Ziel) ergibt
    /// keinen Prozentwert, sondern keinen — sonst stuenden dreistellige
    /// Zahlen ohne Aussage da.
    #[test]
    fn sprit_auswertung_winziger_anflugplan_ergibt_keine_phase() {
        let mut e = dlh370();
        // 8 kg Anflugplan — unter MIN_PLAN_ANFLUG_KG (15). Die Grenze liegt
        // bewusst niedrig, damit die GA- und Bizjet-Flotte ihre Anflugphase
        // behaelt; darunter ist die Zahl aber nicht mehr messbar.
        e.plan_bis_vergleichspunkt_kg = Some(21_210.0);
        let a = auswerten(&e);
        assert!(a.anflug.is_none());
        assert!(a.bis_sinkflug.is_some());
    }

    /// Ein winziger Plan bis TOD (Kleinflugzeug: 18 kg Trip) ergibt keine
    /// Phase — 1 kg Ablesefehler waeren dort 7 % (Korpus-Befund P3).
    #[test]
    fn sprit_auswertung_winziger_plan_ergibt_keine_phase() {
        let e = SpritEingang {
            planned_burn_kg: Some(18.0),
            plan_bis_vergleichspunkt_kg: Some(14.0),
            takeoff_fuel_kg: Some(90.0),
            sprit_bei_vergleichspunkt_kg: Some(76.0),
            landing_fuel_kg: Some(72.0),
            tank_plausibel: true,
            ..Default::default()
        };
        let a = auswerten(&e);
        assert!(
            a.bis_sinkflug.is_none(),
            "winziger Plan darf keine Phase ergeben"
        );
        assert!(a.anflug.is_none());
        // Der Landesprit bleibt eine Tatsache.
        assert_eq!(a.landing_fuel_kg, Some(72.0));
    }

    /// Ein Bizjet mit kleinem, aber messbarem Anflugplan behaelt die Phase —
    /// die frueheren 50 kg haetten 17 % aller Fluege die Phase gekostet.
    #[test]
    fn sprit_auswertung_bizjet_behaelt_die_anflugphase() {
        let e = SpritEingang {
            planned_burn_kg: Some(900.0),
            plan_bis_vergleichspunkt_kg: Some(870.0), // 30 kg Anflugplan
            takeoff_fuel_kg: Some(1_400.0),
            sprit_bei_vergleichspunkt_kg: Some(520.0),
            landing_fuel_kg: Some(470.0),
            tank_plausibel: true,
            ..Default::default()
        };
        let a = auswerten(&e);
        assert!(a.anflug.is_some(), "30 kg Anflugplan sind messbar");
        assert!(a.bis_sinkflug.is_some());
    }

    /// Reserve-Quoten von Kleinflugzeugen werden gedeckelt — 1 298 % waere
    /// richtig und trotzdem ohne Aussage (Korpus-Befund P7).
    #[test]
    fn sprit_auswertung_reserve_quote_gedeckelt() {
        let e = SpritEingang {
            planned_reserve_kg: Some(19.0),
            landing_fuel_kg: Some(247.0),
            takeoff_fuel_kg: Some(300.0),
            tank_plausibel: true,
            ..Default::default()
        };
        match auswerten(&e).reserve {
            Reserve::Intakt { quote_pct } => assert_eq!(quote_pct, 999.0),
            andere => panic!("erwartet Intakt, war {andere:?}"),
        }
    }

    /// Der Abhebe-Tankstand steht im Ergebnis und ist nicht geraten.
    #[test]
    fn sprit_auswertung_nennt_den_abhebe_tankstand() {
        let a = auswerten(&dlh370());
        assert_eq!(a.takeoff_fuel_kg, Some(40_919.0));
        assert_eq!(a.landing_fuel_kg, Some(16_770.0));
        // Und die Differenz ist der Trip-Verbrauch — die Aussage, die die
        // beiden Marken der Sprit-Leiter machen.
        let verbrauch = a.takeoff_fuel_kg.unwrap() - a.landing_fuel_kg.unwrap();
        assert_eq!(verbrauch, 24_149.0);
    }

    /// Das Ergebnis ist das Wire-Format: Enum-Tags und Feldnamen sind
    /// Vertrag mit Webapp und Client-Oberflaeche.
    #[test]
    fn sprit_auswertung_wire_format() {
        let a = auswerten(&dlh370());
        let json = serde_json::to_string(&a).expect("json");
        assert!(
            json.contains("\"reserve\":{\"status\":\"intakt\",\"quote_pct\":341.1}"),
            "{json}"
        );
        assert!(json.contains("\"badge\":\"gruen\""), "{json}");
        // Gegen die Konstante, nicht gegen eine abgeschriebene Zahl — sonst
        // muss dieser Test bei jedem Fassungssprung von Hand nachgezogen
        // werden, und genau das wurde beim Sprung auf 3 vergessen.
        assert!(
            json.contains(&format!("\"fassung\":{SPRIT_AUSWERTUNG_FASSUNG}")),
            "{json}"
        );
        let zurueck: SpritAuswertung = serde_json::from_str(&json).expect("roundtrip");
        assert_eq!(zurueck, a);
    }
}
