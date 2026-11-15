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

    #[test]
    fn exit_requested_wartet_nicht() {
        const SRC: &str = include_str!("../beenden.rs");
        let k = koerper(SRC, "pub(crate) fn exit_requested(");
        assert!(k.contains("prevent_exit()"), "exit_requested hält das Ende nicht mehr auf");
        ohne_warten(&k, "beenden::exit_requested");
        ohne_warten(&koerper(SRC, "fn durchlassen("), "beenden::durchlassen");
        ohne_warten(&koerper(SRC, "fn fenster_ausblenden("), "beenden::fenster_ausblenden");
    }

    #[test]
    fn laufmarkierung_faellt_nur_bei_exit() {
        // Ein aufgehaltenes ExitRequested ist kein Ende; griffe danach die
        // Notbremse, muss die Markierung noch liegen.
        let k = koerper(crate::quelle_gesamt::PRODUKTION, "fn beendet_den_lauf_sauber(");
        assert!(k.contains("RunEvent::Exit"));
        assert!(!k.contains("ExitRequested"), "Laufmarkierung fiele schon beim aufgehaltenen ExitRequested");
    }
