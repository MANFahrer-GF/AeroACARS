    use super::*;

    /// [`BUNDLE_IDENTIFIER`] dupliziert einen Wert aus `tauri.conf.json`,
    /// weil `init_tracing()` vor dem ersten `AppHandle` läuft. Ohne diesen
    /// Test würde ein Umbenennen des Identifiers die Logdateien lautlos in
    /// ein anderes Verzeichnis wandern lassen als `activity_log.json` und
    /// `run_active.lock` — und beim Absturz-Nachsehen schaut dann jeder in
    /// den falschen Ordner.
    #[test]
    fn log_dir_identifier_matches_tauri_conf() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).expect("tauri.conf.json");
        assert_eq!(
            conf["identifier"].as_str(),
            Some(BUNDLE_IDENTIFIER),
            "BUNDLE_IDENTIFIER muss dem identifier aus tauri.conf.json entsprechen"
        );
    }

    /// Die Logdatei liegt neben den anderen Laufzeitdateien, nicht in einem
    /// eigenen Zweig — sonst wird sie beim Aufräumen übersehen.
    #[test]
    fn log_dir_sits_under_the_app_config_dir() {
        let Some(dir) = log_dir() else { return };
        let expected_parent = dirs::config_dir().unwrap().join(BUNDLE_IDENTIFIER);
        assert_eq!(dir.parent(), Some(expected_parent.as_path()));
        assert!(dir.ends_with(LOG_SUBDIR));
    }

    /// Der eigentliche Zweck der Übung: es muss wirklich eine Datei mit
    /// Inhalt entstehen. Prüft zugleich, dass jede Zeile SOFORT auf der
    /// Platte landet — ein gepufferter Schreiber würde beim Absturz genau
    /// die letzten Zeilen verlieren, also die einzigen, die zählen.
    #[test]
    fn appender_writes_through_immediately() {
        use std::io::Write;

        let dir = std::env::temp_dir().join(format!("aeroacars-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut appender = build_log_appender(&dir).expect("appender should build");
        writeln!(appender, "erste zeile").expect("write");

        // Ohne Flush, ohne Drop des Appenders: der Inhalt muss schon da sein.
        let written: String = std::fs::read_dir(&dir)
            .expect("log dir")
            .filter_map(|e| e.ok())
            .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
            .collect();
        assert!(
            written.contains("erste zeile"),
            "Logdatei war leer — der Schreiber puffert, statt durchzuschreiben. Inhalt: {written:?}"
        );

        drop(appender);
        let _ = std::fs::remove_dir_all(&dir);
    }
