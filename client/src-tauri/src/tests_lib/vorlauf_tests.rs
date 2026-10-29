    use super::{vorlauf_einordnen, VorlaufEnde};

    /// Svens Fall: Datei des vorigen Laufs noch da, gleiche Fassung.
    #[test]
    fn gleiche_fassung_ist_ein_abbruch() {
        let datei = "pid=10712\nstarted_at=2026-09-21T07:13:32+00:00\nversion=1.7.41\n";
        assert_eq!(
            vorlauf_einordnen(Some(datei), "1.7.41"),
            VorlaufEnde::Abbruch {
                gestartet: Some("2026-09-21T07:13:32+00:00".into())
            }
        );
    }

    /// Gegenprobe: der Auto-Updater startet mit einer anderen Fassung neu.
    #[test]
    fn andere_fassung_ist_ein_update() {
        let datei = "pid=1\nstarted_at=x\nversion=1.7.41\n";
        assert_eq!(
            vorlauf_einordnen(Some(datei), "1.7.44"),
            VorlaufEnde::Update {
                vorher: "1.7.41".into()
            }
        );
    }

    /// Keine Datei = sauber beendet; alte Datei ohne Fassung = offen lassen,
    /// statt jeden Update-Neustart nach dem Aufspielen als Absturz zu melden.
    #[test]
    fn sauber_und_unbekannt() {
        assert_eq!(vorlauf_einordnen(None, "1.7.47"), VorlaufEnde::Sauber);
        assert_eq!(
            vorlauf_einordnen(Some("pid=1\nstarted_at=x\n"), "1.7.47"),
            VorlaufEnde::Unbekannt
        );
        assert_eq!(
            vorlauf_einordnen(Some("pid=1\nversion=\n"), "1.7.47"),
            VorlaufEnde::Unbekannt
        );
    }

    /// Die geschriebene Datei ist mit der Einordnung lesbar (Rundreise).
    #[test]
    fn geschriebenes_format_wird_gelesen() {
        let inhalt = format!(
            "pid={}\nstarted_at={}\nversion={}\n",
            42,
            "2026-09-21T12:00:00+00:00",
            env!("CARGO_PKG_VERSION")
        );
        assert!(matches!(
            vorlauf_einordnen(Some(&inhalt), env!("CARGO_PKG_VERSION")),
            VorlaufEnde::Abbruch { .. }
        ));
    }
