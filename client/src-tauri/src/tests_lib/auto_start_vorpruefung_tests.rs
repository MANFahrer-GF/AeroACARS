    use super::*;

    fn bid(simbrief: &str) -> Bid {
        serde_json::from_str(&format!(
            r#"{{"id": 5719, "user_id": 22, "flight_id": "f1",
                "flight": {{"id": "f1", "flight_number": "219",
                  "dpt_airport_id": "TNCM", "arr_airport_id": "TFFG"{simbrief}}}}}"#
        ))
        .expect("Bid-JSON")
    }

    /// Der Fall vom 21.09.: Bid gebucht, OFP noch nicht fertig.
    #[test]
    fn bid_ohne_ofp_hat_kein_flugzeug() {
        assert_eq!(bid_flugzeug_id(&bid("")), None);
        assert_eq!(
            bid_flugzeug_id(&bid(r#", "simbrief": {"id": "sb1"}"#)),
            None,
            "OFP-Eintrag ohne aircraft_id ist ebenfalls kein Flugzeug"
        );
        assert_eq!(
            bid_flugzeug_id(&bid(r#", "simbrief": {"id": "sb1", "aircraft_id": 1046}"#)),
            Some(1046)
        );
    }

    #[test]
    fn pause_gilt_nur_fuer_den_gescheiterten_bid_und_nur_120_s() {
        let jetzt = Utc::now();
        let f = (
            jetzt - chrono::Duration::seconds(20),
            5719,
            "not_at_departure".to_string(),
        );
        assert_eq!(
            auto_start_pause_rest(5719, Some(&f), jetzt),
            Some((
                crate::grenzwerte::grenzwerte().cockpit.auto_start_fail_cooldown_secs.wert - 20,
                "not_at_departure".to_string()
            ))
        );
        assert_eq!(auto_start_pause_rest(5720, Some(&f), jetzt), None);
        assert_eq!(auto_start_pause_rest(5719, None, jetzt), None);
        let alt = (
            jetzt - chrono::Duration::seconds(crate::grenzwerte::grenzwerte().cockpit.auto_start_fail_cooldown_secs.wert),
            5719,
            "x".to_string(),
        );
        assert_eq!(auto_start_pause_rest(5719, Some(&alt), jetzt), None);
    }

    #[test]
    fn ohne_ofp_sagt_worauf_gewartet_wird() {
        let (code, titel, text) = auto_start_hinweis(&AutoStartBeobachtung {
            ohne_ofp: Some("219".into()),
            naechster_nm: Some(0.2),
            ..Default::default()
        });
        assert_eq!(code, "ofp_missing");
        assert!(titel.contains("SimBrief-OFP"));
        assert!(text.contains("Bid 219"), "{text}");
    }

    /// Frueher: „Bid ? wurde diese Session schon mal auto-gestartet" —
    /// waehrend in Wahrheit die Pause nach einem Fehler lief.
    #[test]
    fn pause_nennt_bid_rest_und_fehler_statt_fragezeichen() {
        let (code, _, text) = auto_start_hinweis(&AutoStartBeobachtung {
            pause: Some((5719, 95, "not_at_departure".into())),
            ..Default::default()
        });
        assert_eq!(code, "retry_pending");
        assert!(text.contains("5719") && text.contains("95 s"), "{text}");
        assert!(text.contains("not_at_departure"), "{text}");
        assert!(!text.contains('?'), "{text}");
    }

    #[test]
    fn konkretester_grund_gewinnt() {
        let alles = AutoStartBeobachtung {
            typ_passt_nicht: Some(("219".into(), "C24R".into(), "B738".into())),
            ohne_ofp: Some("220".into()),
            pause: Some((1, 10, "x".into())),
            schon_gestartet: Some(2),
            start_laeuft_bid: None,
            naechster_nm: Some(0.1),
        };
        assert_eq!(auto_start_hinweis(&alles).0, "aircraft_mismatch");
        let ohne_typ = AutoStartBeobachtung {
            typ_passt_nicht: None,
            ..alles
        };
        assert_eq!(auto_start_hinweis(&ohne_typ).0, "ofp_missing");
        let nur_pause = AutoStartBeobachtung {
            pause: Some((1, 10, "x".into())),
            schon_gestartet: Some(2),
            ..Default::default()
        };
        assert_eq!(auto_start_hinweis(&nur_pause).0, "retry_pending");
        let (code, _, text) = auto_start_hinweis(&AutoStartBeobachtung {
            naechster_nm: Some(12.34),
            ..Default::default()
        });
        assert_eq!(code, "no_bid_match");
        assert!(text.contains("12.3 nm"), "{text}");
    }

    /// Jeder Code, den `auto_start_hinweis` liefern kann, braucht einen
    /// Banner-Text in allen drei Sprachen — sonst zeigt das Banner nur den
    /// allgemeinen Ersatztext. Die Codes erzeugt die Funktion selbst, die
    /// FAELLE (ein Feld je Zweig) stehen aber hier: Wer einen Zweig ergaenzt,
    /// muss hier einen Fall ergaenzen. Die Laengen-Pruefung faengt nur
    /// doppelte Codes, keinen vergessenen Zweig.
    #[test]
    fn jeder_hinweis_code_hat_einen_banner_text() {
        let faelle = [
            AutoStartBeobachtung {
                typ_passt_nicht: Some(("1".into(), "A".into(), "B".into())),
                ..Default::default()
            },
            AutoStartBeobachtung {
                ohne_ofp: Some("1".into()),
                ..Default::default()
            },
            AutoStartBeobachtung {
                pause: Some((1, 1, "x".into())),
                ..Default::default()
            },
            AutoStartBeobachtung {
                schon_gestartet: Some(1),
                ..Default::default()
            },
            AutoStartBeobachtung {
                schon_gestartet: Some(1),
                start_laeuft_bid: Some(1),
                ..Default::default()
            },
            AutoStartBeobachtung::default(),
        ];
        let mut codes: std::collections::BTreeSet<&str> =
            faelle.iter().map(|b| auto_start_hinweis(b).0).collect();
        assert_eq!(codes.len(), faelle.len(), "jeder Fall eigener Code");
        codes.insert(AUTO_START_SIM_WARTET);
        for (sprache, json) in [
            ("de", include_str!("../../../src/locales/de/common.json")),
            ("en", include_str!("../../../src/locales/en/common.json")),
            ("it", include_str!("../../../src/locales/it/common.json")),
        ] {
            let v: serde_json::Value = serde_json::from_str(json).expect("Locale-JSON");
            for code in &codes {
                let text = v["bids"]["auto_start_skip"][*code].as_str().unwrap_or("");
                assert!(
                    !text.trim().is_empty(),
                    "{sprache}: bids.auto_start_skip.{code} fehlt"
                );
            }
        }
    }

    fn sim(lat: f64, lon: f64, paused: bool, titel: &str) -> SimSnapshot {
        let mut s = SimSnapshot::default();
        s.lat = lat;
        s.lon = lon;
        s.paused = paused;
        s.aircraft_title = Some(titel.to_string());
        s
    }

    /// Nachgestellt nach dem Log vom 19.09.2026 (MSFS lädt, Weltkarte
    /// pausiert, Teleports, Pause-Flackern). Vorher feuerte der Auto-Start
    /// mitten darin; jetzt erst nach 20 s Ruhe am Gate.
    #[test]
    fn ladevorgang_sperrt_bis_der_sim_20_s_ruhig_ist() {
        let t0 = Utc::now();
        let s = |sek: i64| t0 + chrono::Duration::seconds(sek);
        let mut r = SimRuhe::default();
        // Weltkarte: pausiert, irgendwo in der Luft.
        assert!(r.pruefen(&sim(50.0, 8.0, true, "A380"), s(0)).is_some());
        // Kurz unpausiert, aber Teleport um Tausende Kilometer.
        let g = r.pruefen(&sim(19.09, 72.87, false, "A380"), s(3));
        assert!(g.as_deref().unwrap_or("").contains("springt"), "{g:?}");
        // Pause-Flackern am Zielplatz setzt die Uhr jedes Mal zurück.
        assert!(r.pruefen(&sim(19.09, 72.87, false, "A380"), s(6)).is_some());
        assert!(r.pruefen(&sim(19.09, 72.87, true, "A380"), s(9)).is_some());
        // Ab jetzt ruhig: erst nach 20 s bereit, nicht vorher.
        for t in (12..32).step_by(3) {
            assert!(
                r.pruefen(&sim(19.09, 72.87, false, "A380"), s(t)).is_some(),
                "t={t}"
            );
        }
        assert_eq!(r.pruefen(&sim(19.09, 72.87, false, "A380"), s(33)), None);
        // Und bleibt bereit, solange nichts passiert.
        assert_eq!(
            r.pruefen(&sim(19.0901, 72.8701, false, "A380"), s(36)),
            None
        );
    }

    /// Normalfall: Pilot sitzt schon kalt am Gate, der Sim läuft ruhig —
    /// der Auto-Start ist nach 20 s bereit, Rollen am Platz stört nicht.
    #[test]
    fn ruhiger_sim_am_gate_ist_nach_20_s_bereit() {
        let t0 = Utc::now();
        let mut r = SimRuhe::default();
        assert!(r.pruefen(&sim(18.04, -63.11, false, "C24R"), t0).is_some());
        let t = t0 + chrono::Duration::seconds(crate::grenzwerte::grenzwerte().cockpit.auto_start_sim_ruhe_secs.wert);
        assert_eq!(r.pruefen(&sim(18.0401, -63.1101, false, "C24R"), t), None);
    }

    #[test]
    fn flugzeugwechsel_und_slew_setzen_die_ruhe_zurueck() {
        let t0 = Utc::now();
        let s = |sek: i64| t0 + chrono::Duration::seconds(sek);
        let mut r = SimRuhe::default();
        r.pruefen(&sim(18.0, -63.0, false, "C24R"), s(0));
        assert_eq!(r.pruefen(&sim(18.0, -63.0, false, "C24R"), s(25)), None);
        let g = r.pruefen(&sim(18.0, -63.0, false, "B738"), s(28));
        assert!(g.as_deref().unwrap_or("").contains("gewechselt"), "{g:?}");
        let mut slew = sim(18.0, -63.0, false, "B738");
        slew.slew_mode = true;
        assert!(r.pruefen(&slew, s(60)).is_some());
        assert!(r.pruefen(&sim(18.0, -63.0, false, "B738"), s(63)).is_some());
    }

    fn bid_mit(id: i64, flugzeug: Option<i64>) -> Bid {
        let sb = flugzeug
            .map(|f| format!(r#", "simbrief": {{"id": "sb", "aircraft_id": {f}}}"#))
            .unwrap_or_default();
        let mut b = bid(&sb);
        b.id = id;
        b
    }

    /// Der Fall vom 21.09.: am Platz, ohne OFP → nie `TypPruefen`, also
    /// weder Claim noch flight_start noch Pause.
    #[test]
    fn am_platz_ohne_ofp_wird_nicht_gefeuert() {
        assert_eq!(
            auto_start_bid_vorpruefung(&bid_mit(5719, None), None, true, None),
            BidVorpruefung::OhneOfp
        );
        assert_eq!(
            auto_start_bid_vorpruefung(&bid_mit(5719, Some(1046)), None, true, None),
            BidVorpruefung::TypPruefen { flugzeug_id: 1046 }
        );
    }

    /// Codex-Abnahme Runde 2: Aufraeumen darf nur den EIGENEN Eintrag
    /// loeschen — ein fremder Start bleibt markiert. Und es muss auch nach
    /// einer Panik eines anderen Halters (vergifteter Mutex) gehen.
    #[test]
    fn freigeben_nur_den_eigenen_eintrag() {
        let platz = Mutex::new(Some(5724));
        freigeben_wenn_eigen(&platz, 99);
        assert_eq!(
            *platz.lock().unwrap(),
            Some(5724),
            "fremder Eintrag geloescht"
        );
        freigeben_wenn_eigen(&platz, 5724);
        assert_eq!(*platz.lock().unwrap(), None);

        let vergiftet = std::sync::Arc::new(Mutex::new(Some(7)));
        let v2 = vergiftet.clone();
        let _ = std::thread::spawn(move || {
            let _g = v2.lock().unwrap();
            panic!("Halter stirbt");
        })
        .join();
        assert!(vergiftet.is_poisoned());
        freigeben_wenn_eigen(&vergiftet, 7);
        assert_eq!(*vergiftet.lock().unwrap_or_else(|e| e.into_inner()), None);
    }

    /// Normales Ende: nur der Marker. Panik/Abbruch: Marker, Anspruch UND
    /// Pause — sonst Neustart desselben Bids nach 3 s.
    #[test]
    fn aufraeumen_nach_normalem_ende_und_nach_panik() {
        let jetzt = Utc::now();
        let marker = Mutex::new(Some(5724));
        let anspruch = Mutex::new(Some(5724));
        let pause = Mutex::new(None);
        auto_start_aufraeumen(&marker, &anspruch, &pause, 5724, true, jetzt);
        assert_eq!(*marker.lock().unwrap(), None);
        assert_eq!(
            *anspruch.lock().unwrap(),
            Some(5724),
            "den regelt der normale Weg"
        );
        assert!(pause.lock().unwrap().is_none());

        let marker = Mutex::new(Some(5724));
        auto_start_aufraeumen(&marker, &anspruch, &pause, 5724, false, jetzt);
        assert_eq!(*marker.lock().unwrap(), None);
        assert_eq!(*anspruch.lock().unwrap(), None);
        let p = pause.lock().unwrap().clone().expect("Pause gesetzt");
        assert_eq!((p.1, p.2.as_str()), (5724, "abgebrochen"));
        // Und die Pause greift wirklich im Watcher.
        assert!(auto_start_pause_rest(5724, Some(&p), jetzt).is_some());

        // Fremde Eintraege bleiben unangetastet.
        let marker = Mutex::new(Some(1));
        let anspruch = Mutex::new(Some(1));
        auto_start_aufraeumen(&marker, &anspruch, &pause, 5724, false, jetzt);
        assert_eq!(*marker.lock().unwrap(), Some(1));
        assert_eq!(*anspruch.lock().unwrap(), Some(1));
    }

    /// Log Thomas 21.09.2026 (AIB424): waehrend flight_start lief, stand
    /// „schon mal auto-gestartet" im Protokoll. Jetzt „Start läuft" — und
    /// NUR, solange der Start wirklich laeuft.
    #[test]
    fn laufender_start_heisst_start_laeuft() {
        let (code, titel, text) = auto_start_hinweis(&AutoStartBeobachtung {
            schon_gestartet: Some(5724),
            start_laeuft_bid: Some(5724),
            // Andere Bids duerfen das nicht verdraengen.
            typ_passt_nicht: Some(("1".into(), "A".into(), "B".into())),
            ..Default::default()
        });
        assert_eq!(code, "start_running");
        assert!(titel.contains("Start läuft"), "{titel}");
        assert!(text.contains("5724"), "{text}");
        // Ohne laufenden Start bleibt es beim bisherigen Hinweis.
        let (code, _, _) = auto_start_hinweis(&AutoStartBeobachtung {
            schon_gestartet: Some(5724),
            ..Default::default()
        });
        assert_eq!(code, "bid_already_started");
        // Laeuft der Auto-Start eines ANDEREN Bids, ist dieser nicht „am
        // Starten" (Codex-Abnahme 22.09.2026: vorher globales Signal).
        let (code, _, _) = auto_start_hinweis(&AutoStartBeobachtung {
            schon_gestartet: Some(5724),
            start_laeuft_bid: Some(99),
            ..Default::default()
        });
        assert_eq!(code, "bid_already_started");
        // Laeuft ein Start, aber fuer keinen beanspruchten Bid, gibt es
        // nichts „Laufendes" zu melden.
        let (code, _, _) = auto_start_hinweis(&AutoStartBeobachtung {
            start_laeuft_bid: Some(5724),
            ..Default::default()
        });
        assert_eq!(code, "no_bid_match");
    }

    #[test]
    fn pause_zaehlt_nur_am_platz_und_schon_gestartet_zuerst() {
        let pause = || Some((90, "not_at_departure".to_string()));
        let b = bid_mit(5719, Some(1046));
        assert_eq!(
            auto_start_bid_vorpruefung(&b, None, false, pause()),
            BidVorpruefung::NichtAmPlatz,
            "fern vom Platz kein „neuer Versuch gleich\""
        );
        assert_eq!(
            auto_start_bid_vorpruefung(&b, None, true, pause()),
            BidVorpruefung::Pause {
                rest_s: 90,
                code: "not_at_departure".into()
            }
        );
        assert_eq!(
            auto_start_bid_vorpruefung(&b, Some(5719), true, pause()),
            BidVorpruefung::SchonGestartet
        );
        // Ein anderer Bid ist nicht „schon gestartet".
        assert_eq!(
            auto_start_bid_vorpruefung(&b, Some(1), true, None),
            BidVorpruefung::TypPruefen { flugzeug_id: 1046 }
        );
    }

    /// Banner und Protokoll getrennt: gleicher Grund → Protokoll erst nach
    /// der Frist (oder nie, wenn keine gesetzt), neuer Grund → sofort.
    #[test]
    fn protokoll_drosselt_gleiche_gruende_und_meldet_neue_sofort() {
        let t0 = Utc::now();
        let s = |sek: i64| t0 + chrono::Duration::seconds(sek);
        let mut p = HinweisProtokoll::default();
        assert!(p.protokollieren("ofp_missing", s(0), Some(60)));
        assert!(!p.protokollieren("ofp_missing", s(3), Some(60)));
        assert!(!p.protokollieren("ofp_missing", s(57), Some(60)));
        assert!(p.protokollieren("ofp_missing", s(60), Some(60)));
        assert!(p.protokollieren("retry_pending", s(63), Some(60)));
        assert!(p.protokollieren(AUTO_START_SIM_WARTET, s(66), None));
        assert!(!p.protokollieren(AUTO_START_SIM_WARTET, s(600), None));
    }

    /// Codex-Nachprüfung: Kommt der Titel beim Hochfahren erst nach 20 s,
    /// durfte der Auto-Start sofort feuern. Das erste Erscheinen startet die
    /// Ruhe neu.
    #[test]
    fn spaet_erscheinender_titel_startet_die_ruhe_neu() {
        let t0 = Utc::now();
        let s = |sek: i64| t0 + chrono::Duration::seconds(sek);
        let mut r = SimRuhe::default();
        let mut ohne = sim(47.0, 11.0, false, "");
        ohne.aircraft_title = None;
        for t in (0..=30).step_by(3) {
            r.pruefen(&ohne, s(t));
        }
        let g = r.pruefen(&sim(47.0, 11.0, false, "C172"), s(33));
        assert!(g.as_deref().unwrap_or("").contains("gewechselt"), "{g:?}");
        // Ruhe beginnt mit dem nächsten ruhigen Takt (36) neu: bei 54 noch
        // nicht bereit, bei 56 schon.
        assert!(r.pruefen(&sim(47.0, 11.0, false, "C172"), s(36)).is_some());
        assert!(r.pruefen(&sim(47.0, 11.0, false, "C172"), s(54)).is_some());
        assert_eq!(r.pruefen(&sim(47.0, 11.0, false, "C172"), s(56)), None);
    }

    /// X-Plane: Die Web-API liefert mal keinen Titel. Das darf die Ruhe
    /// nicht zurücksetzen, sonst feuert der Auto-Start nie.
    #[test]
    fn kurz_fehlender_titel_ist_kein_flugzeugwechsel() {
        let t0 = Utc::now();
        let s = |sek: i64| t0 + chrono::Duration::seconds(sek);
        let mut r = SimRuhe::default();
        r.pruefen(&sim(47.0, 11.0, false, "C172"), s(0));
        let mut ohne = sim(47.0, 11.0, false, "");
        ohne.aircraft_title = None;
        for t in [3, 9, 15] {
            r.pruefen(&ohne, s(t));
            r.pruefen(&sim(47.0, 11.0, false, "C172"), s(t + 3));
        }
        assert_eq!(r.pruefen(&sim(47.0, 11.0, false, "C172"), s(21)), None);
    }
