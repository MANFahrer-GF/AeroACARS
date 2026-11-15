    //! 3.1.2 (Feld-Hänger 15.11.2026): Kein Beenden-Schritt darf auf dem
    //! Hauptfaden warten. Die Wächter lesen den Quelltext; die verbotenen
    //! Wörter stehen nur hier, nicht in `lib.rs`/`beenden.rs` — kein
    //! Selbsttreffer.

    /// Was auf dem Hauptfaden wartet: `block_on`, die drei Sicher-Schritte,
    /// Hoppie/MQTT, Pausen und Kanal-Warten.
    const WARTEN: &[&str] = &[
        "block_on",
        "activity_log_abwarten",
        "flugprotokoll_abwarten",
        "acars_abwarten",
        "hoppie::shutdown",
        ".await",
        "sleep(",
        "recv",
        ".join(",
    ];

    /// Körper ab `marker` bis zur passenden schließenden Klammer, ohne
    /// Kommentarzeilen.
    fn koerper(quelle: &str, marker: &str) -> String {
        let start = quelle
            .find(marker)
            .unwrap_or_else(|| panic!("{marker} nicht mehr gefunden — Test anpassen, nicht löschen"));
        let rest = &quelle[start..];
        let auf = rest.find('{').expect("öffnende Klammer");
        let mut tiefe = 0usize;
        let mut ende = rest.len();
        for (i, c) in rest[auf..].char_indices() {
            match c {
                '{' => tiefe += 1,
                '}' => {
                    tiefe -= 1;
                    if tiefe == 0 {
                        ende = auf + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest[..ende]
            .lines()
            .filter(|z| !z.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn ohne_warten(k: &str, wo: &str) {
        for w in WARTEN {
            assert!(!k.contains(w), "{wo} enthält `{w}` — der Hauptfaden darf beim Beenden nicht warten");
        }
    }

    #[test]
    fn run_callback_wartet_nicht() {
        let k = koerper(crate::quelle_gesamt::PRODUKTION, ".run(|app_handle, event|");
        assert!(k.contains("beenden::exit_requested("), "ExitRequested geht nicht mehr an beenden.rs");
        ohne_warten(&k, "Der .run-Callback");
    }

    const SRC: &str = include_str!("../beenden.rs");

    /// Text von `von` bis zum nächsten `bis` (ohne dieses), oder bis zum Ende.
    fn abschnitt<'a>(k: &'a str, von: &str, bis: &str) -> &'a str {
        let start = k.find(von).unwrap_or_else(|| panic!("{von} nicht mehr gefunden — Test anpassen, nicht löschen"));
        let rest = &k[start + von.len()..];
        &rest[..rest.find(bis).unwrap_or(rest.len())]
    }

    #[test]
    fn exit_requested_wartet_nicht() {
        let k = koerper(SRC, "pub(crate) fn exit_requested(");
        ohne_warten(&k, "beenden::exit_requested");
        // Alles, was der Hauptfaden beim Beenden aufruft — auch eine Ebene tiefer.
        for f in [
            "fn durchlassen(",
            "fn fenster_ausblenden(",
            "fn aufraeumen_starten(",
            "fn notbremse_starten(",
            "pub(crate) fn uebernehmen(",
            "pub(crate) fn beenden_uebernehmen(",
            "pub(crate) fn neu_starten(",
            "fn phase_setzen(",
        ] {
            ohne_warten(&koerper(SRC, f), f);
        }
        assert!(
            !k.contains("beenden_beginnt"),
            "exit_requested meldet `beenden_beginnt` selbst — nur `durchlassen` darf das, sonst schweigt der Wächter beim Aufhalten"
        );
    }

    #[test]
    fn erster_wunsch_wird_aufgehalten() {
        let k = koerper(SRC, "pub(crate) fn exit_requested(");
        let zweig = abschnitt(&k, "Entscheidung::AufhaltenUndAufraeumen =>", "Entscheidung::");
        assert!(zweig.contains("api.prevent_exit()"), "erstes ExitRequested wird nicht aufgehalten:\n{zweig}");
        assert!(zweig.contains("aufraeumen_starten("), "erstes ExitRequested startet kein Aufräumen");
        // H3: erst loggen, dann ausblenden.
        let log = zweig.find("tracing::info!").expect("Logzeile „aufgehalten“");
        let weg = zweig.find("fenster_ausblenden(").expect("Fenster ausblenden");
        assert!(log < weg, "Logzeile „aufgehalten“ steht nach dem Ausblenden");
        let weiter = abschnitt(&k, "Entscheidung::Aufhalten =>", "Entscheidung::");
        assert!(weiter.contains("api.prevent_exit()"), "weitere Wünsche während des Aufräumens gehen durch");
    }

    #[test]
    fn aufraeumen_endet_mit_beenden_oder_neustart() {
        let k = koerper(SRC, "fn aufraeumen(app");
        let schluss = abschnitt(&k, "phase_setzen(phase_nach_dem_aufraeumen(", "\u{0}");
        assert!(schluss.contains("app.exit("), "nach dem Aufräumen beendet niemand die App");
        assert!(schluss.contains("app.request_restart()"), "nach dem Aufräumen startet niemand neu");
        assert!(k.contains("sichern_und_abmelden("), "aufraeumen sichert nicht mehr");
    }

    #[test]
    fn notbremse_braucht_keine_sperre() {
        // S1: Die Notbremse liest die Phase aus dem Atomic; keine Sperre,
        // die der Hauptfaden oder eine stockende Logzeile halten könnte.
        assert!(!SRC.contains("Mutex"), "beenden.rs hat wieder eine Sperre");
        let k = koerper(SRC, "fn notbremse(app");
        assert!(!k.contains(".lock("), "Notbremse nimmt eine Sperre");
        // Log und GlitchTip nur im Hilfsfaden, mit Frist.
        let vor_meldung = abschnitt(&k, "let phase = phase();", "spawn(");
        assert!(!vor_meldung.contains("tracing::"), "Notbremse loggt vor dem Hilfsfaden");
        assert!(k.contains("recv_timeout(NOTBREMSE_MELDEFRIST)"), "Notbremse wartet ohne Frist auf die Meldung");
        let nach_meldung = abschnitt(&k, "recv_timeout(NOTBREMSE_MELDEFRIST)", "\u{0}");
        assert!(nach_meldung.contains("std::process::exit("), "Notbremse beendet nicht hart");
        assert!(!nach_meldung.contains("tracing::"), "Notbremse loggt nach der Meldefrist synchron");
    }

    #[test]
    fn fristen_kommen_aus_den_konstanten() {
        // S3: Jeder Schritt ist über `schrittfrist` an die Gesamtfrist gebunden.
        let k = koerper(SRC, "fn sichern_und_abmelden(");
        for aufruf in [
            "activity_log_abwarten(schrittfrist(FRIST_AKTIVITAET, start.elapsed()))",
            "flugprotokoll_abwarten_hoechstens(schrittfrist(FRIST_FLUGPROTOKOLL, start.elapsed()))",
            "acars_abwarten_beim_beenden(schrittfrist(FRIST_ACARS, start.elapsed()))",
            "schrittfrist(FRIST_HOPPIE_MQTT.saturating_sub(start.elapsed()), start.elapsed())",
        ] {
            assert!(k.contains(aufruf), "fehlt: {aufruf}");
        }
        for (marker, frei) in [
            ("fn activity_log_abwarten(", "Duration::from_"),
            ("fn acars_abwarten_beim_beenden(", "Duration::from_"),
        ] {
            assert!(
                !koerper(crate::quelle_gesamt::PRODUKTION, marker).contains(frei),
                "{marker} hat wieder eine eigene feste Frist"
            );
        }
    }

    #[test]
    fn fenster_bleibt_beim_beenden_weg() {
        // H4: Tray „Anzeigen“, Tray-Linksklick, Zweitstart, Deck, Hülle.
        let tray = koerper(crate::quelle_gesamt::PRODUKTION, "fn build_tray_icon(");
        assert!(
            abschnitt(&tray, "\"show\" =>", "\"quit\" =>").contains("beenden::fenster_zeigen_erlaubt()"),
            "Tray „Anzeigen“ holt das Fenster beim Beenden zurück"
        );
        assert!(
            abschnitt(&tray, ".on_tray_icon_event(", "\u{0}").contains("beenden::fenster_zeigen_erlaubt()"),
            "Tray-Linksklick holt das Fenster beim Beenden zurück"
        );
        let zweit = koerper(crate::quelle_gesamt::PRODUKTION, "tauri_plugin_single_instance::init(");
        let pruef = zweit.find("beenden::fenster_zeigen_erlaubt()").expect("Zweitstart prüft das Beenden nicht");
        assert!(pruef < zweit.find(".show()").expect("show"), "Zweitstart zeigt vor der Prüfung");
        const DECK: &str = include_str!("../panel_server.rs");
        assert!(
            abschnitt(DECK, "DeckBefehl::Fenster(seite) =>", "Ok(())").contains("fenster_zeigen_erlaubt()"),
            "Deck-Taste holt das Fenster beim Beenden zurück"
        );
        const HUELLE: &str = include_str!("../huelle.rs");
        assert!(koerper(HUELLE, "fn fenster_zeigen(").contains("fenster_zeigen_erlaubt()"));
        assert!(koerper(HUELLE, "impl Drop for Verborgen").contains("fenster_zeigen_erlaubt()"));
    }

    #[test]
    fn windows_update_raeumt_vor_dem_installationsprogramm_auf() {
        // H6: Unter Windows endet der Prozess im Updater mit `process::exit`.
        const HUELLE: &str = include_str!("../huelle.rs");
        let k = koerper(HUELLE, "pub(crate) async fn update_installieren(");
        let haken = abschnitt(&k, ".on_before_exit(", ".build()");
        assert!(haken.contains("beenden::vor_dem_installieren("), "kein Aufräumen vor dem Installationsprogramm");
        assert!(haken.contains("cleanup_before_exit()"), "eingebauter Haken des Updaters fehlt");
        assert!(koerper(SRC, "pub(crate) fn vor_dem_installieren(").contains("sichern_und_abmelden("));
    }

    #[test]
    fn tray_takt_ruft_nichts_am_tray_direkt() {
        // S2: Vom Tokio-Worker nur `run_on_main_thread` ohne Warten; kein
        // Menü-Neubau, Setter nur in dessen Closure.
        const TRAY: &str = include_str!("../lib_tray.rs");
        let k = koerper(TRAY, "fn spawn_tray_updater(");
        assert!(!k.contains("set_menu("), "Tray-Takt setzt das Menü neu");
        assert!(!k.contains("build_tray_menu("), "Tray-Takt baut das Menü neu");
        let marker = "run_on_main_thread(";
        let start = k.find(marker).expect("Tray-Takt geht nicht mehr über run_on_main_thread");
        let auf = start + marker.len() - 1;
        let mut tiefe = 0usize;
        let mut ende = k.len();
        for (i, c) in k[auf..].char_indices() {
            match c {
                '(' => tiefe += 1,
                ')' => {
                    tiefe -= 1;
                    if tiefe == 0 {
                        ende = auf + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let closure = &k[auf..ende];
        let rest = format!("{}{}", &k[..start], &k[ende..]);
        for setter in ["set_tooltip(", "set_icon(", "set_text("] {
            assert!(closure.contains(setter), "{setter} fehlt in der run_on_main_thread-Closure");
            assert!(!rest.contains(setter), "{setter} läuft im Tray-Takt außerhalb von run_on_main_thread");
        }
        assert_eq!(k.matches(marker).count(), 1, "zweiter run_on_main_thread-Aufruf — Wächter anpassen");
    }

    #[test]
    fn laufmarkierung_faellt_nur_bei_exit() {
        // Ein aufgehaltenes ExitRequested ist kein Ende; griffe danach die
        // Notbremse, muss die Markierung noch liegen.
        let k = koerper(crate::quelle_gesamt::PRODUKTION, "fn beendet_den_lauf_sauber(");
        assert!(k.contains("RunEvent::Exit"));
        assert!(!k.contains("ExitRequested"), "Laufmarkierung fiele schon beim aufgehaltenen ExitRequested");
    }
