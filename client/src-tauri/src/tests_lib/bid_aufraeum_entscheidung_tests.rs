    use super::{bid_aufraeum_entscheidung, BidAufraeumen::*};

    /// Alter PIREP auf `flug-a`, vorgefiled um 10:00 (Serveruhr).
    const PIREP: Option<&str> = Some("2026-11-13T10:00:00.000000Z");

    fn bid(id: i64, flug: &str, angelegt: Option<&str>) -> api_client::Bid {
        let zeit = angelegt.map(|t| format!(r#", "created_at": "{t}""#)).unwrap_or_default();
        serde_json::from_str(&format!(
            r#"{{"id": {id}, "user_id": 5, "flight_id": "{flug}",
                "flight": {{"id": "{flug}", "flight_number": "1",
                  "dpt_airport_id": "EDDF", "arr_airport_id": "EDDM"}}{zeit}}}"#
        ))
        .expect("Bid-JSON")
    }

    /// Der Fehler: alte Buchung 4711 ist weg, der Pilot hat `flug-a` neu
    /// gebucht (4800). Weder per `bid_id` noch per `flight_id` loeschen.
    #[test]
    fn neue_buchung_auf_demselben_flug_bleibt_mit_bid_id() {
        let b = [bid(4800, "flug-a", Some("2026-11-13T12:00:00Z"))];
        assert_eq!(bid_aufraeum_entscheidung(Some(4711), Some("flug-a"), Some(&b), None), Erledigt);
    }

    /// Dasselbe ohne `bid_id`: die Buchung ist juenger als der alte PIREP.
    #[test]
    fn neue_buchung_auf_demselben_flug_bleibt_ohne_bid_id() {
        let b = [bid(4800, "flug-a", Some("2026-11-13T12:00:00Z"))];
        assert_eq!(bid_aufraeum_entscheidung(None, Some("flug-a"), Some(&b), PIREP), Erledigt);
    }

    /// Berechtigter Orphan-Fall: die alte Buchung liegt noch — loeschen, mit
    /// `flight_id`-Rueckfall (auf dem Flug liegt nur sie).
    #[test]
    fn alte_buchung_wird_geloescht() {
        let b = [
            bid(4711, "flug-a", Some("2026-11-13T09:00:00Z")),
            bid(4712, "flug-b", Some("2026-11-13T11:00:00Z")),
        ];
        assert_eq!(
            bid_aufraeum_entscheidung(Some(4711), Some("flug-a"), Some(&b), None),
            Loeschen(Some(4711), Some("flug-a".into()))
        );
        assert_eq!(
            bid_aufraeum_entscheidung(None, Some("flug-a"), Some(&b), PIREP),
            Loeschen(Some(4711), Some("flug-a".into())),
            "ohne bid_id: die VOR dem PIREP angelegte Buchung ist die alte"
        );
    }

    /// Keine Buchung mehr auf dem Flug: Ziel erreicht.
    #[test]
    fn nichts_mehr_da_ist_erledigt() {
        let b = [bid(4712, "flug-b", None)];
        assert_eq!(bid_aufraeum_entscheidung(Some(4711), Some("flug-a"), Some(&b), None), Erledigt);
        assert_eq!(bid_aufraeum_entscheidung(None, Some("flug-a"), Some(&b), None), Erledigt);
    }

    /// Bid-Liste nicht abrufbar: per `bid_id` (404 = erledigt, sicher), nie
    /// per `flight_id`; ohne `bid_id` gar nicht.
    #[test]
    fn ohne_bid_liste_nie_ueber_flight_id() {
        assert_eq!(
            bid_aufraeum_entscheidung(Some(4711), Some("flug-a"), None, PIREP),
            Loeschen(Some(4711), None)
        );
        assert_eq!(bid_aufraeum_entscheidung(None, Some("flug-a"), None, PIREP), Liegenlassen);
    }

    /// Ohne `bid_id` und ohne lesbare Zeiten ist die Buchung nicht
    /// zuzuordnen — nichts loeschen.
    #[test]
    fn ohne_zeiten_liegenlassen() {
        let b = [bid(4711, "flug-a", None)];
        assert_eq!(bid_aufraeum_entscheidung(None, Some("flug-a"), Some(&b), PIREP), Liegenlassen);
        let b = [bid(4711, "flug-a", Some("2026-11-13T09:00:00Z"))];
        assert_eq!(bid_aufraeum_entscheidung(None, Some("flug-a"), Some(&b), None), Liegenlassen);
    }

    mod laufender_flug {
        use super::super::bid_trifft_laufenden_flug;

        const LAUFEND: (&str, i64, &str) = ("pirep-neu", 4711, "flug-a");

        /// Pilot hat denselben Flug mit derselben, nie geloeschten Buchung neu
        /// gestartet: Aufraeumen des alten PIREP darf sie nicht anfassen.
        #[test]
        fn gleiche_bid_id_anderer_pirep_trifft() {
            assert!(bid_trifft_laufenden_flug("pirep-alt", Some(4711), None, LAUFEND));
        }

        #[test]
        fn gleiche_flight_id_anderer_pirep_trifft() {
            assert!(bid_trifft_laufenden_flug("pirep-alt", None, Some("flug-a"), LAUFEND));
            assert!(bid_trifft_laufenden_flug("pirep-alt", Some(99), Some("flug-a"), LAUFEND));
        }

        #[test]
        fn derselbe_pirep_oder_anderer_flug_trifft_nicht() {
            assert!(!bid_trifft_laufenden_flug("pirep-neu", Some(4711), Some("flug-a"), LAUFEND));
            assert!(!bid_trifft_laufenden_flug("pirep-alt", Some(99), Some("flug-b"), LAUFEND));
            assert!(!bid_trifft_laufenden_flug("pirep-alt", None, Some(""), ("pirep-neu", 0, "")));
        }
    }

    /// Wiring: Drain und Orphan-Weg loeschen nur noch ueber
    /// `bid_aufraeumen_ausfuehren` (dort Entscheidung, lokale Sperre und
    /// Server-Abfrage, siehe `ablauf`), nie direkt; der nachgereichte PIREP
    /// reiht ein statt direkt zu loeschen. Fenster bis zur naechsten
    /// Top-Level-Funktion.
    #[test]
    fn aufraeumwege_loeschen_nur_ueber_bid_aufraeumen_ausfuehren() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = |start: &str| {
            let ab = SRC.find(start).unwrap_or_else(|| panic!("{start} nicht gefunden — Test anpassen"));
            let rest = &SRC[ab..];
            let ende = ["\nfn ", "\nasync fn ", "\n#[tauri::command]"]
                .iter()
                .filter_map(|m| rest[1..].find(m).map(|i| i + 1))
                .min()
                .unwrap_or(rest.len());
            &rest[..ende]
        };
        for start in ["async fn drain_pending_bid_cleanup(", "async fn flight_cancel_orphan("] {
            let k = koerper(start);
            assert!(k.contains("bid_aufraeumen_ausfuehren("), "{start}: ruft bid_aufraeumen_ausfuehren nicht");
            assert!(k.contains("bid_aufraeumen_lokal_gesperrt("), "{start}: ohne lokale Sperre");
            assert!(!k.contains(".delete_bid("), "{start}: loescht direkt an der Pruefung vorbei");
        }
        let nachreichen = koerper("fn spawn_pirep_queue_worker(");
        assert!(nachreichen.contains("bid_nach_nachreichen_einreihen("), "queued_filed reiht nicht ein");
        assert!(!nachreichen.contains("consume_bid_best_effort("), "queued_filed loescht direkt");
    }

    /// Ablauf gegen einen Mock-Server: `bid_aufraeumen_ausfuehren` ist die
    /// einzige Loeschstelle von Drain und Orphan-Weg. Beide brauchen selbst
    /// einen `AppHandle` (Warteschlange, Zustand) und sind so nicht als
    /// Ganzes testbar; die lokale Sperre kommt hier als Schalter herein.
    mod ablauf {
        use super::super::{bid_aufraeumen_ausfuehren, drain_folge, BidAufraeumErgebnis, DrainFolge};
        use super::{bid, PIREP};
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::{Arc, Mutex};

        type Protokoll = Arc<Mutex<Vec<(String, String, String)>>>;

        /// Mock-Server: protokolliert (Methode, Pfad, Body) in Reihenfolge.
        /// GET `/api/user/pireps?state=0` liefert `laufende` (oder 500, wenn
        /// `None`), DELETE `/api/user/bids` den Status aus `delete`.
        fn server(
            laufende: Option<&'static str>,
            delete: impl Fn(&str) -> u16 + Send + 'static,
        ) -> (api_client::Client, Protokoll) {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let adresse = format!("http://{}", l.local_addr().unwrap());
            let protokoll: Protokoll = Arc::new(Mutex::new(Vec::new()));
            let p = protokoll.clone();
            std::thread::spawn(move || {
                for s in l.incoming() {
                    let Ok(mut s) = s else { break };
                    let mut daten = Vec::new();
                    let mut puffer = [0u8; 8192];
                    let (kopf, body) = loop {
                        let n = s.read(&mut puffer).unwrap_or(0);
                        daten.extend_from_slice(&puffer[..n]);
                        let text = String::from_utf8_lossy(&daten).to_string();
                        if let Some(ende) = text.find("\r\n\r\n") {
                            let laenge: usize = text[..ende]
                                .lines()
                                .find_map(|z| {
                                    let (k, v) = z.split_once(':')?;
                                    k.eq_ignore_ascii_case("content-length").then(|| v.trim().parse().ok())?
                                })
                                .unwrap_or(0);
                            if daten.len() >= ende + 4 + laenge || n == 0 {
                                break (text[..ende].to_string(), text[ende + 4..].to_string());
                            }
                        } else if n == 0 {
                            break (text, String::new());
                        }
                    };
                    let mut erste = kopf.lines().next().unwrap_or("").split_whitespace();
                    let methode = erste.next().unwrap_or("").to_string();
                    let pfad = erste.next().unwrap_or("").to_string();
                    let (status, antwort) = match (methode.as_str(), pfad.as_str()) {
                        ("GET", "/api/user/pireps?state=0") => match laufende {
                            Some(json) => (200, format!(r#"{{"data":{json}}}"#)),
                            None => (500, "{}".to_string()),
                        },
                        ("DELETE", "/api/user/bids") => (delete(&body), r#"{"data":[]}"#.to_string()),
                        _ => (404, "{}".to_string()),
                    };
                    p.lock().unwrap().push((methode, pfad, body));
                    let _ = write!(
                        s,
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{antwort}",
                        antwort.len()
                    );
                }
            });
            let client = api_client::Client::new(api_client::Connection::new(&adresse, "test").unwrap()).unwrap();
            (client, protokoll)
        }

        #[allow(clippy::too_many_arguments)]
        fn aufraeumen(
            client: &api_client::Client,
            bid_id: Option<i64>,
            flight_id: Option<&str>,
            bids: Option<&[api_client::Bid]>,
            pirep_erstellt: Option<&str>,
            lokal_laufend: bool,
        ) -> BidAufraeumErgebnis {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(bid_aufraeumen_ausfuehren(
                    client,
                    "pirep-alt",
                    bid_id,
                    flight_id,
                    bids,
                    pirep_erstellt,
                    || lokal_laufend,
                ))
        }

        fn deletes(p: &Protokoll) -> Vec<String> {
            p.lock().unwrap().iter().filter(|(m, _, _)| m == "DELETE").map(|(_, _, b)| b.clone()).collect()
        }

        fn alte_buchung() -> Vec<api_client::Bid> {
            vec![bid(4711, "flug-a", Some("2026-11-13T09:00:00Z"))]
        }

        /// Gegenstueck zu allen „kein DELETE"-Faellen: freie Bahn — erst
        /// die Server-Abfrage, dann genau ein DELETE.
        #[test]
        fn frei_fragt_server_und_loescht_einmal() {
            let (c, p) = server(Some("[]"), |_| 200);
            let b = alte_buchung();
            let e = aufraeumen(&c, Some(4711), Some("flug-a"), Some(&b), None, false);
            assert!(matches!(e, BidAufraeumErgebnis::Geloescht(Some(4711))), "{e:?}");
            let log = p.lock().unwrap().clone();
            assert_eq!(log.len(), 2, "{log:?}");
            assert_eq!((log[0].0.as_str(), log[0].1.as_str()), ("GET", "/api/user/pireps?state=0"));
            assert_eq!(log[1].0, "DELETE");
            assert!(log[1].2.contains("\"bid_id\":4711"), "{log:?}");
        }

        /// Befund 1: auf demselben Flug laeuft laut Server ein ANDERER
        /// eigener PIREP (zweites Geraet oder Startfenster) — kein DELETE.
        #[test]
        fn laufender_server_pirep_auf_dem_flug_verhindert_delete() {
            let (c, p) = server(Some(r#"[{"id":"pirep-neu","state":0,"flight_id":"flug-a"}]"#), |_| 200);
            let b = alte_buchung();
            let e = aufraeumen(&c, Some(4711), Some("flug-a"), Some(&b), None, false);
            assert!(matches!(e, BidAufraeumErgebnis::LaufenderFlug), "{e:?}");
            assert!(deletes(&p).is_empty(), "{:?}", p.lock().unwrap());
            assert_eq!(drain_folge(&e), DrainFolge::WartetOhneVersuch);
        }

        /// Nur `bid_id` bekannt: der Flug kommt aus der Bid-Liste.
        #[test]
        fn flug_der_bid_id_aus_der_liste_zaehlt() {
            let (c, p) = server(Some(r#"[{"id":"pirep-neu","state":0,"flight_id":"flug-a"}]"#), |_| 200);
            let b = alte_buchung();
            let e = aufraeumen(&c, Some(4711), None, Some(&b), None, false);
            assert!(matches!(e, BidAufraeumErgebnis::LaufenderFlug), "{e:?}");
            assert!(deletes(&p).is_empty());
        }

        /// Kein Flug ermittelbar (Bid-Liste weg, keine flight_id): jeder
        /// andere laufende PIREP sperrt; ebenso einer ohne flight_id.
        #[test]
        fn im_zweifel_sperrt_jeder_andere_laufende_pirep() {
            let (c, p) = server(Some(r#"[{"id":"pirep-neu","state":0,"flight_id":"flug-z"}]"#), |_| 200);
            let e = aufraeumen(&c, Some(4711), None, None, None, false);
            assert!(matches!(e, BidAufraeumErgebnis::LaufenderFlug), "{e:?}");
            let (c2, p2) = server(Some(r#"[{"id":"pirep-neu","state":0}]"#), |_| 200);
            let b = alte_buchung();
            let e2 = aufraeumen(&c2, Some(4711), Some("flug-a"), Some(&b), None, false);
            assert!(matches!(e2, BidAufraeumErgebnis::LaufenderFlug), "{e2:?}");
            assert!(deletes(&p).is_empty() && deletes(&p2).is_empty());
        }

        /// Der aufzuraeumende PIREP selbst und PIREPs anderer Fluege sperren nicht.
        #[test]
        fn derselbe_pirep_oder_anderer_flug_sperrt_nicht() {
            let (c, p) = server(
                Some(r#"[{"id":"pirep-alt","state":0,"flight_id":"flug-a"},{"id":"pirep-x","state":0,"flight_id":"flug-b"}]"#),
                |_| 200,
            );
            let b = alte_buchung();
            let e = aufraeumen(&c, Some(4711), Some("flug-a"), Some(&b), None, false);
            assert!(matches!(e, BidAufraeumErgebnis::Geloescht(Some(4711))), "{e:?}");
            assert_eq!(deletes(&p).len(), 1);
        }

        /// Abfrage scheitert (500): nicht loeschen, Versuch zaehlt.
        #[test]
        fn gescheiterte_abfrage_verhindert_delete() {
            let (c, p) = server(None, |_| 200);
            let b = alte_buchung();
            let e = aufraeumen(&c, Some(4711), Some("flug-a"), Some(&b), None, false);
            assert!(matches!(e, BidAufraeumErgebnis::ServerUnpruefbar), "{e:?}");
            assert!(deletes(&p).is_empty());
            assert_eq!(drain_folge(&e), DrainFolge::VersuchZaehlt);
        }

        /// Lokale Sperre (laufender/startender Flug): gar keine Anfrage.
        #[test]
        fn lokale_sperre_verhindert_jede_anfrage() {
            let (c, p) = server(Some("[]"), |_| 200);
            let b = alte_buchung();
            let e = aufraeumen(&c, Some(4711), Some("flug-a"), Some(&b), None, true);
            assert!(matches!(e, BidAufraeumErgebnis::LaufenderFlug), "{e:?}");
            assert!(p.lock().unwrap().is_empty(), "{:?}", p.lock().unwrap());
            assert_eq!(drain_folge(&e), DrainFolge::WartetOhneVersuch);
        }

        /// Entscheidung `Liegenlassen` (ohne bid_id, Zeiten unlesbar): weder
        /// Abfrage noch DELETE; im Drain zaehlt der Versuch.
        #[test]
        fn liegenlassen_schickt_kein_delete() {
            let (c, p) = server(Some("[]"), |_| 200);
            let b = [bid(4711, "flug-a", None)];
            let e = aufraeumen(&c, None, Some("flug-a"), Some(&b), PIREP, false);
            assert!(matches!(e, BidAufraeumErgebnis::Liegenlassen), "{e:?}");
            assert!(p.lock().unwrap().is_empty());
            assert_eq!(drain_folge(&e), DrainFolge::VersuchZaehlt);
        }

        /// Panel schickt nur `flight_id`: gesendet wird die entschiedene
        /// `bid_id` der alten Buchung aus der Liste, nicht der Panel-Wert.
        #[test]
        fn loeschen_sendet_die_entschiedene_bid_id() {
            let (c, p) = server(Some("[]"), |_| 200);
            let b = alte_buchung();
            let e = aufraeumen(&c, None, Some("flug-a"), Some(&b), PIREP, false);
            assert!(matches!(e, BidAufraeumErgebnis::Geloescht(Some(4711))), "{e:?}");
            let d = deletes(&p);
            assert_eq!(d.len(), 1, "{d:?}");
            assert!(d[0].contains("\"bid_id\":4711"), "{d:?}");
        }

        /// Bid-Liste nicht abrufbar: entschieden ist `bid_id` OHNE
        /// `flight_id`-Rueckfall — auch bei 422 kein flight_id-DELETE.
        #[test]
        fn ohne_bid_liste_kein_flight_id_delete() {
            let (c, p) = server(Some("[]"), |body| if body.contains("bid_id") { 422 } else { 200 });
            let e = aufraeumen(&c, Some(4711), Some("flug-a"), None, None, false);
            assert!(matches!(e, BidAufraeumErgebnis::Fehlgeschlagen(_)), "{e:?}");
            let d = deletes(&p);
            assert_eq!(d.len(), 1, "{d:?}");
            assert!(!d[0].contains("flight_id"), "{d:?}");
        }

        /// 404 auf das DELETE: Buchung schon weg (`delete_bid` meldet Ok),
        /// der Drain entfernt den Eintrag.
        #[test]
        fn delete_404_entfernt_den_eintrag() {
            let (c, _p) = server(Some("[]"), |_| 404);
            let b = alte_buchung();
            let e = aufraeumen(&c, Some(4711), Some("flug-a"), Some(&b), None, false);
            assert_eq!(drain_folge(&e), DrainFolge::Entfernen, "{e:?}");
        }
    }
