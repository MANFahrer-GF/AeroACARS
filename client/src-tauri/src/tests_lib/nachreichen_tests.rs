    use super::{
        abfrage_bloecke, fehlende_einreihen, fehlt_auf_dem_server, nachreich_kandidaten, protokoll_ist_eingereicht,
        LokalesProtokoll,
    };
    use aeroacars_mqtt::pirep_status::PirepPruefstatus;
    use std::time::Duration;

    fn p(id: &str, stunden: u64, eingereicht: bool) -> LokalesProtokoll {
        LokalesProtokoll {
            pirep_id: id.into(),
            alter: Duration::from_secs(stunden * 3600),
            eingereicht,
        }
    }

    const WOCHE: Duration = Duration::from_secs(7 * 24 * 3600);

    #[test]
    fn nur_eingereichte_und_juengste_zuerst() {
        let liste = [
            p("alt", 30, true),
            p("neu", 2, true),
            p("abgebrochen", 1, false),
        ];
        assert_eq!(
            nachreich_kandidaten(&liste, None, WOCHE, 50),
            vec!["neu", "alt"]
        );
    }

    /// Gegenprobe: der laufende Flug und zu alte Protokolle bleiben weg,
    /// die Obergrenze greift.
    #[test]
    fn laufender_flug_zu_alt_und_obergrenze() {
        let liste = [
            p("aktiv", 1, true),
            p("uralt", 24 * 8, true),
            p("a", 2, true),
            p("b", 3, true),
        ];
        assert_eq!(
            nachreich_kandidaten(&liste, Some("aktiv"), WOCHE, 1),
            vec!["a"]
        );
    }

    /// P-7.23: 30 Tage, ohne Stueckgrenze — 120 Kandidaten gehen in drei
    /// Abfragen, keiner faellt hinten runter, doppelte nur einmal, Diagnose
    /// zuerst. Gegenprobe: ein 31 Tage altes Protokoll bleibt draussen.
    #[test]
    fn abgleich_ohne_stueckgrenze_ueber_dreissig_tage() {
        assert_eq!(super::NACHREICHEN_MAX_ALTER, Duration::from_secs(30 * 24 * 3600));
        let flug: Vec<String> = (0..120).map(|i| format!("F{i}")).collect();
        let diag = vec!["D1".to_string(), "F5".to_string()];
        let b = abfrage_bloecke(&diag, &flug);
        assert_eq!(b.iter().map(Vec::len).collect::<Vec<_>>(), [50, 50, 21]);
        assert_eq!(&b[0][..2], ["D1", "F5"]);
        let alle: std::collections::HashSet<&String> = b.iter().flatten().collect();
        assert_eq!(alle.len(), 121);
        let liste = [p("neu", 24 * 29, true), p("zu_alt", 24 * 31, true)];
        assert_eq!(nachreich_kandidaten(&liste, None, super::NACHREICHEN_MAX_ALTER, usize::MAX), vec!["neu"]);
    }

    /// P-7.23 (Codex 30.10.): Alle als fehlend gemeldeten, eingereichten
    /// Protokolle landen in der Warteschlange — auch ueber 100; vorhandene und
    /// abgebrochene nicht.
    #[test]
    fn fehlende_werden_alle_eingereiht() {
        let d = std::env::temp_dir().join(format!("aeroacars-fehlende-einreihen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut stand: Vec<PirepPruefstatus> = (0..120).map(|i| status(&format!("F{i}"), Some(false))).collect();
        stand.push(status("DA", Some(true)));
        stand.push(status("ABGEBROCHEN", Some(false)));
        let flug: Vec<String> = (0..120).map(|i| format!("F{i}")).chain(["DA".to_string()]).collect();
        assert_eq!(fehlende_einreihen(&d, &stand, &flug, "7"), 120);
        let offen = crate::upload_warteschlange::offene_protokolle(&d);
        assert_eq!(offen.len(), 120);
        assert!(!offen.contains("DA") && !offen.contains("ABGEBROCHEN"));
        let _ = std::fs::remove_dir_all(&d);
    }

    fn status(id: &str, da: Option<bool>) -> PirepPruefstatus {
        PirepPruefstatus {
            pirep_id: id.into(),
            known: true,
            flug_log_vorhanden: da,
            diagnose_log_vorhanden: None,
            score_trust_level: None,
            requires_review: false,
            review_state: None,
            review_decision: None,
            reason_codes: vec![],
            reviewed_at: None,
        }
    }

    /// Nur ein ausdrueckliches `false` loest einen Upload aus — ein alter
    /// Server ohne das Feld bekommt nicht alles noch einmal.
    #[test]
    fn nur_ausdruecklich_fehlende_werden_nachgereicht() {
        let s = [
            status("da", Some(true)),
            status("fehlt", Some(false)),
            status("alt", None),
        ];
        assert_eq!(fehlt_auf_dem_server(&s), vec!["fehlt"]);
    }

    /// Das Feld kommt vom Server als `flug_log_vorhanden`; fehlt es, ist es
    /// `None` (aelterer Recorder).
    #[test]
    fn feld_wird_gelesen_und_fehlt_tolerant() {
        let mit: PirepPruefstatus =
            serde_json::from_str(r#"{"pirep_id":"x","known":false,"flug_log_vorhanden":false}"#)
                .unwrap();
        assert_eq!(mit.flug_log_vorhanden, Some(false));
        let ohne: PirepPruefstatus =
            serde_json::from_str(r#"{"pirep_id":"x","known":true}"#).unwrap();
        assert_eq!(ohne.flug_log_vorhanden, None);
    }

    /// Nach einem unmoeglichen Sprung gibt die App NICHT mehr von selbst ab
    /// — sonst ginge ein Flug wie MSC1588 (22.09.2026) mit voller Strecke
    /// und ohne je gemessene Landung raus. Der Pilot entscheidet im Banner.
    ///
    /// Waechter am Quelltext: Ein Verhaltenstest braeuchte einen laufenden
    /// Flug samt Sim-Snapshots. Am Zeilenanfang gesucht, sonst findet der
    /// Test seinen eigenen Suchtext (CI 23.09.2026).
    #[test]
    fn auto_file_setzt_bei_unmoeglichem_sprung_aus() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let start = SRC
            .find(concat!("\nfn auto_file_", "versuchen("))
            .expect("Auto-File-Funktion nicht mehr gefunden — Test anpassen, nicht loeschen");
        // Bis zur schliessenden Klammer DIESES Zweigs (Einrueckung 16),
        // nicht bis zu einem festen Byte-Offset: lib.rs enthaelt Nicht-ASCII,
        // ein fester Offset kann mitten in ein Zeichen fallen und den Test
        // mit einem Slice-Panic roeten. Ausserdem endete das Fenster mit
        // 3000 Bytes mitten im Heartbeat-Code dahinter — ein kuenftiger
        // zweiter `flight_end`-Aufruf dort haette die Aussage entwertet
        // (Cloud-QS 23.09.2026, zweite Runde).
        let rest = &SRC[start..];
        let zweig_ende = rest
            .find("\n}\n")
            .expect("Ende der Auto-File-Funktion nicht gefunden — Test anpassen, nicht loeschen");
        let zweig = &rest[..zweig_ende];
        // Genau EIN Abgabe-Aufruf im Fenster: sonst prueft die
        // Reihenfolge-Aussage weiter unten den falschen.
        assert_eq!(
            zweig.matches("flight_end(").count(),
            1,
            "Fenster passt nicht mehr auf den Auto-File-Zweig"
        );
        // Die Entscheidung (Sperre, Kabine, Merker) steht seit DAL667 in
        // `auto_file_entscheiden` und ist dort mit Verhaltenstests belegt
        // (`auto_file_entscheidet_unabhaengig_von_pause_und_sprung`). Hier
        // nur: Sie wird VOR der Abgabe gefragt.
        let sprung = zweig
            .find("auto_file_entscheiden(&mut st")
            .expect("Auto-File fragt die Entscheidung nicht mehr");
        let ent_start = SRC
            .find(concat!("\nfn auto_file_", "entscheiden("))
            .expect("auto_file_entscheiden nicht gefunden — Test anpassen, nicht loeschen");
        let ent = &SRC[ent_start..ent_start + SRC[ent_start..].find("\n}\n").unwrap()];
        assert!(
            ent.contains("abgabe_sperre(st)"),
            "Auto-File fragt die Abgabe-Sperre nicht mehr"
        );
        let abgabe = zweig
            .find("flight_end(app_af.clone()")
            .expect("Auto-File-Aufruf nicht mehr gefunden");
        assert!(
            sprung < abgabe,
            "die Sprung-Pruefung muss VOR der automatischen Abgabe stehen"
        );
        // Der ZWEITE Weg, auf dem die App von selbst einreicht: File-First
        // in `flight_cancel`. Er hat MSC1588 nicht ausgeloest, aber der
        // Resume-Banner ruft ihn ohne `force` — ohne diese Bedingung reicht
        // „Flug verwerfen" den Flug ein (Cloud-QS 23.09.2026).
        let cancel = SRC
            .find("\nasync fn flight_cancel(")
            .expect("flight_cancel nicht gefunden — Test anpassen, nicht loeschen");
        let cancel_ende = SRC[cancel + 1..]
            .find("\nasync fn ")
            .map(|i| cancel + 1 + i)
            .unwrap_or(SRC.len());
        let file_first = &SRC[cancel..cancel_ende];
        let bedingung = file_first
            .find("abgabe_sperre(&stats).is_none()")
            .expect("File-First fragt die Abgabe-Sperre nicht mehr — es reicht sonst selbst ein");
        let aufruf = file_first
            .find("flight_end(app.clone()")
            .expect("File-First-Aufruf nicht mehr gefunden");
        assert!(
            bedingung < aufruf,
            "die Pruefung muss VOR der Abgabe stehen"
        );
    }

    /// Wo ein Sprung erkannt wird, muss er auch in die LISTE — sonst
    /// ueberschreibt der zweite Sprung den ersten still, und die Notiz
    /// nennt nur den letzten (Cloud-QS 23.09.2026, dritte Runde).
    ///
    /// Seit DAL667 (07.11.2026) laeuft jede Erkennung ueber
    /// `sprung_vermerken`, das zusaetzlich Zeitpunkt und „nach der Ankunft"
    /// stempelt. Ein Erkennungsweg, der das Feld wieder direkt setzt, haette
    /// keinen Stempel und sperrte einen fertigen Flug — darum: genau EINE
    /// Setzstelle, und alle drei Erkennungswege rufen den Helfer. Das
    /// Verhalten selbst belegt `sprung_vermerken_haengt_an_und_stempelt`.
    #[test]
    fn jede_erkennung_haengt_den_sprung_an_die_liste() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let nadel = concat!("stats.resume_discontinuity = ", "Some(");
        let stellen: Vec<usize> = SRC.match_indices(nadel).map(|(i, _)| i).collect();
        assert_eq!(stellen.len(), 1, "Sprung wird an mehr als einer Stelle gesetzt: {stellen:?}");
        let aufrufe = SRC.matches(concat!("sprung_", "vermerken(&mut stats, ")).count();
        assert!(aufrufe >= 3, "nicht alle Erkennungswege rufen sprung_vermerken: {aufrufe}");
        for i in &stellen {
            let rest = &SRC[*i..];
            // Eng: bis zum Ende der naechsten Anweisung. Ein weites Fenster
            // faende das Schlagwort irgendwo weiter unten und koennte nicht
            // mehr rot werden (Cloud-QS 23.09.2026, zweite Runde).
            let zeilen: Vec<&str> = rest.lines().take(4).collect();
            let fenster = zeilen.join("\n");
            assert!(
                fenster.contains("resume_spruenge.push(")
                    // Test-Literale bauen den Wert direkt, ohne Erkennung.
                    || fenster.contains("ResumeDiscontinuity {"),
                "Sprung wird erkannt, aber nicht angehaengt: {fenster}"
            );
        }
    }

    /// Die Einheitentests rufen `compute_resume_discontinuity` selbst mit
    /// Ziel und Luecke auf. Uebergaeben die ECHTEN Aufrufstellen dort `None`,
    /// waere die ganze Regel wirkungslos — und alle Tests blieben gruen
    /// (Cloud-QS 23.09.2026).
    #[test]
    fn die_echten_aufrufe_geben_ziel_und_luecke_mit() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        // v3 (Etappe 0): Die Tests (`resume_discontinuity_tests`) liegen in
        // `tests_lib/`, `lib.rs` enthaelt nur noch Produktion — das
        // Ausblenden des Testmoduls entfaellt.
        assert!(
            !SRC.contains("\nmod resume_discontinuity_tests {"),
            "das Testmodul steht wieder eingebettet in lib.rs — dann zaehlt es hier mit"
        );
        // Der Suchtext getrennt, sonst findet der Test SICH SELBST — die
        // Falle hat in dieser Datei schon zweimal zugeschlagen.
        let nadel = concat!("compute_resume_", "discontinuity(");
        let aufrufe: Vec<&str> = SRC
            .match_indices(nadel)
            .map(|(i, _)| {
                let rest = &SRC[i..];
                // Bis zum Ende der Anweisung, nicht bis zu einem festen
                // Offset (die Datei enthaelt Nicht-ASCII).
                &rest[..rest.find(';').map(|e| e + 1).unwrap_or(rest.len())]
            })
            // Die Definition selbst ist kein Aufruf.
            .filter(|a| !a.contains("prev: &PausedSnapshot"))
            .collect();
        assert!(aufrufe.len() >= 2, "Aufrufstellen: {aufrufe:#?}");
        for a in &aufrufe {
            assert!(
                a.contains("ziel"),
                "ohne Zielkoordinaten schlaegt die Sprung-Regel nie an: {a}"
            );
            assert!(
                a.contains("duration_secs") || a.contains("luecke_secs"),
                "ohne Luecke gilt jede Strecke als fliegbar: {a}"
            );
        }
    }

    /// GAF 9655 (23.09.2026): Der Simulator stuerzte 1,4 nm vor der Bahn
    /// ab, kam am Boden am Ziel zurueck, die Phasen liefen im Stand bis
    /// „Arrived" durch — und die App reichte ein, ohne je ein Aufsetzen
    /// gesehen zu haben. Kein Sprung nach den alten Regeln: Position kaum
    /// verschoben, Sprit gesunken, Ziel schon vorher in Reichweite.
    #[test]
    fn unterbrochen_ohne_gemessene_landung_fragt_den_piloten() {
        use super::{abgabe_sperre, AbgabeSperre, FlightStats, PauseReason, PauseSegment, Utc};
        let mut stats = FlightStats::new();
        // Ein Flug ohne alles: Die App darf einreichen.
        assert_eq!(abgabe_sperre(&stats), None);

        // Kurzer Aussetzer (Ladebildschirm) zaehlt nicht.
        stats.pause_segments.push(PauseSegment {
            started_at: Utc::now(),
            ended_at: Utc::now(),
            duration_secs: 8,
            reason: PauseReason::SimDisconnect,
            drift_nm: None,
            altitude_delta_ft: None,
            fuel_delta_kg: None,
        });
        assert_eq!(abgabe_sperre(&stats), None);

        // Elf Minuten ohne Simulator, danach keine gemessene Landung.
        stats.pause_segments.push(PauseSegment {
            started_at: Utc::now(),
            ended_at: Utc::now(),
            duration_secs: 674,
            reason: PauseReason::SimDisconnect,
            drift_nm: None,
            altitude_delta_ft: None,
            fuel_delta_kg: None,
        });
        assert_eq!(abgabe_sperre(&stats), Some(AbgabeSperre::LandungFehlt));

        // Zwei App-Neustarts: erst 15 min (Absturz), dann 20 s (Update).
        // Der zweite darf den ersten NICHT vergessen machen — `resume_luecke_secs`
        // steht dann auf 20, die laengste Luecke bleibt bei 900
        // (Cloud-QS 23.09.2026, dritte Runde).
        let mut zwei_neustarts = FlightStats::new();
        zwei_neustarts.resume_luecke_secs = Some(20);
        zwei_neustarts.resume_gap_minutes = Some(0);
        assert_eq!(abgabe_sperre(&zwei_neustarts), None, "nur der kurze");
        zwei_neustarts.resume_luecke_max_secs = Some(900);
        assert_eq!(
            abgabe_sperre(&zwei_neustarts),
            Some(AbgabeSperre::LandungFehlt),
            "der lange erste Neustart zaehlt weiter"
        );

        // Mit gemessener Sinkrate ist alles in Ordnung — eine lange Pause
        // allein macht keinen Verdacht.
        stats.landing_rate_fpm = Some(-180.0);
        assert_eq!(abgabe_sperre(&stats), None);

        // Der Sprung wiegt schwerer und wird zuerst genannt.
        stats.resume_discontinuity = Some(super::ResumeDiscontinuity {
            drift_nm: 240.0,
            altitude_delta_ft: -20000.0,
            fuel_delta_kg: 0.0,
            both_grounded: false,
            ziel_vorher_nm: None,
            ziel_nachher_nm: None,
            danach_am_boden: true,
            luecke_secs: Some(600),
            nur_bestaetigt: false,
            am: None,
            nach_ankunft: false,
        });
        assert_eq!(abgabe_sperre(&stats), Some(AbgabeSperre::Sprung));
    }

    /// Die Kette um `auto_file_versuchen`: Takt-Aufruf in `Arrived` UND der
    /// Merker gegen Doppelabgabe. Ohne den Takt bliebe ein Flug haengen,
    /// wenn die Landebewertung erst nach dem Wechsel fertig wird; ohne den
    /// Merker reichte die App bei jedem Takt erneut ein
    /// (Cloud-QS 23.09.2026, dritte Runde).
    #[test]
    fn auto_file_kette_takt_und_merker() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        // Suchtexte getrennt — sonst findet der Test sich selbst.
        let takt = concat!("auto_file_versuchen(&app, &flight, ", "false)");
        let wechsel = concat!("auto_file_versuchen(&app, &flight, ", "true)");
        assert_eq!(
            SRC.matches(takt).count(),
            1,
            "Takt-Aufruf fehlt oder doppelt"
        );
        assert_eq!(
            SRC.matches(wechsel).count(),
            1,
            "Wechsel-Aufruf fehlt oder doppelt"
        );
        // Der Takt-Aufruf darf nur in `Arrived` laufen.
        let i = SRC.find(takt).unwrap();
        let davor = &SRC[SRC[..i]
            .rfind('\n')
            .map(|z| z.saturating_sub(120))
            .unwrap_or(0)..i];
        assert!(
            davor.contains("FlightPhase::Arrived"),
            "Takt-Aufruf ohne Arrived-Bedingung: {davor}"
        );

        // DAL667 (07.11.2026): Der Takt-Aufruf steht VOR dem ersten
        // `continue` der Streamer-Schleife. Hinter Pause, `was_just_resumed`
        // oder „kein Snapshot" erreichte ihn der Takt nie, und der gelandete
        // Flug wurde nicht abgegeben.
        let schleife = SRC
            .find(concat!("\nfn spawn_position_", "streamer("))
            .and_then(|s| SRC[s..].find("        loop {").map(|l| s + l))
            .expect("Streamer-Schleife nicht gefunden — Test anpassen, nicht loeschen");
        let erstes_continue = schleife
            + SRC[schleife..].find("continue;").expect("kein continue in der Schleife");
        assert!(
            schleife < i && i < erstes_continue,
            "der Abgabe-Versuch muss vor dem ersten `continue` der Streamer-Schleife stehen"
        );

        // Merker: Die Reihenfolge Pruefen → Setzen steht in
        // `auto_file_entscheiden` (Verhalten: `auto_file_entscheidet_*`),
        // und `auto_file_versuchen` gibt erst danach ab.
        let start = SRC
            .find(concat!("\nfn auto_file_", "versuchen("))
            .expect("auto_file_versuchen nicht gefunden");
        let koerper = &SRC[start..start + SRC[start..].find("\n}\n").unwrap()];
        let entscheiden = koerper
            .find("auto_file_entscheiden(")
            .expect("Entscheidung wird nicht mehr gefragt");
        let abgabe = koerper.find("flight_end(").expect("Abgabe fehlt");
        assert!(entscheiden < abgabe, "Merker-Reihenfolge stimmt nicht");
    }

    /// Das Aktivitaetsprotokoll schreibt die GUELTIGE Phase, nicht die der
    /// alten Logik. Vorher sprang es im Stufensinkflug auf „Cruise",
    /// waehrend phpVMS „Descent" bekam (Feldbefund 23.09.2026).
    #[test]
    fn aktivitaetsprotokoll_schreibt_die_gueltige_phase() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        // Suchtexte getrennt — sonst findet der Test sich selbst.
        let alt = concat!("format!(\"Phase: {:?}\", ", "new_phase)");
        let neu = concat!("format!(\"Phase: {:?}\", ", "effective_phase_this_tick)");
        assert_eq!(
            SRC.matches(alt).count(),
            0,
            "die alte Phase steht wieder im Protokoll"
        );
        assert_eq!(
            SRC.matches(neu).count(),
            1,
            "die gueltige Phase fehlt im Protokoll"
        );
        // Und sie haengt am Wechsel der gueltigen Phase, nicht am v1-Wechsel:
        // Ein v1-Block davor muss geschlossen sein, bevor die Zeile kommt.
        // Gezaehlt wird die Klammertiefe — ein umschliessender Block liegt
        // VOR dem Wechsel-Block und waere in dessen Ausschnitt unsichtbar.
        let i = SRC.find(neu).unwrap();
        let bis_zeile = &SRC[..i];
        let wechsel = bis_zeile
            .rfind(concat!("if effective_", "phase_changed {"))
            .expect("Wechsel-Block fehlt");
        if let Some(v1) = bis_zeile.rfind(concat!("if let Some(new_phase) = ", "phase_change")) {
            assert_eq!(
                SRC[v1..wechsel].matches('{').count(),
                SRC[v1..wechsel].matches('}').count(),
                "die Protokollzeile steht im v1-Block"
            );
        }
    }

    /// Das Resume-Gate schaltet erst scharf, wenn der Simulator ruhig und
    /// brauchbar liefert — nicht beim ersten frischen Wert. Beim ersten Wert
    /// war es am 24.09.2026 (OCN 712) das MSFS-Menue: anderes Flugzeug, 0 kg,
    /// anderer Ort. Daraus wurden drei „Spruenge" und ein falscher Durchstart.
    #[test]
    fn resume_gate_wartet_auf_ruhigen_simulator() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let start = SRC
            .find(concat!("\nfn spawn_resume_", "sim_gate("))
            .expect("Resume-Gate nicht gefunden — Test anpassen, nicht loeschen");
        let koerper = &SRC[start..start + SRC[start..].find("\n}\n").unwrap()];
        // Das Verhalten pruefen die Tests an `resume_gate_takt` selbst; hier
        // nur, dass das Gate diese Entscheidung auch FRAGT, bevor es scharf
        // schaltet — und nicht an ihr vorbei.
        let takt = koerper
            .find(concat!("resume_gate_", "takt(&mut ruhe"))
            .expect("Gate fragt resume_gate_takt nicht mehr");
        let scharf = koerper
            .find(concat!("spawn_position_", "streamer("))
            .expect("Gate schaltet nicht mehr scharf");
        assert!(takt < scharf);
        assert_eq!(
            koerper
                .matches(concat!("spawn_position_", "streamer("))
                .count(),
            1,
            "ein zweiter Weg zum Scharfschalten"
        );
    }

    /// Der manuelle Einreichweg darf nicht die Tuer sein, durch die ein
    /// unterbrochener Flug ohne Vermerk durchgeht (Cloud-QS 23.09.2026).
    #[test]
    fn manueller_weg_vermerkt_den_sprung_ebenfalls() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let start = SRC
            .find("\nasync fn flight_end_manual(")
            .expect("flight_end_manual nicht gefunden — Test anpassen, nicht loeschen");
        let ende = SRC[start + 1..]
            .find("\n#[tauri::command]")
            .map(|i| start + 1 + i)
            .unwrap_or(SRC.len());
        let koerper = &SRC[start..ende];
        assert!(
            koerper.contains("sprung_notiz(&spruenge, None, &notes)"),
            "flight_end_manual vermerkt den Sprung nicht mehr"
        );
        assert!(
            koerper.contains("landung_fehlt_notiz(None, &notes)"),
            "flight_end_manual vermerkt die fehlende Landung nicht mehr"
        );
        // Und derselbe Vermerk im normalen Weg — sonst haette nur der
        // manuelle Weg ihn (Cloud-QS 23.09.2026, zweite Runde).
        // ⚠ Suchtext getrennt: Wörtlich geschrieben findet ihn der Wächter
        // `kein_zweiter_filing_weg_am_file_endpunkt_vorbei` in DIESEM Test
        // und schneidet sein Fenster ab hier — er wurde dadurch rot
        // (Cloud-QS 23.09.2026, zweite Runde).
        let ende_start = SRC
            .find(concat!("\nasync fn ", "flight_end("))
            .expect("flight_end nicht gefunden — Test anpassen, nicht loeschen");
        let ende_ende = SRC[ende_start + 1..]
            .find(concat!("\nasync ", "fn "))
            .map(|i| ende_start + 1 + i)
            .unwrap_or(SRC.len());
        let flight_end_koerper = &SRC[ende_start..ende_ende];
        assert!(
            flight_end_koerper.contains("landung_fehlt_notiz(sprung_begruendung.as_deref()"),
            "flight_end vermerkt die fehlende Landung nicht mehr"
        );
    }

    /// Abgebrochene Fluege liefern das DIAGNOSE-Log — genau deren
    /// technische Zeilen fehlten uns (Joel, 22.09.2026). Das Flugprotokoll
    /// bleibt trotzdem aussen vor, sonst entstuende im Import eine Sitzung
    /// fuer einen Flug, den phpVMS so nie gesehen hat.
    #[test]
    fn diagnose_nimmt_auch_abgebrochene_flugprotokolle() {
        let liste = [
            p("eingereicht", 2, true),
            p("abgebrochen", 3, false),
            p("zu_alt", 48, false),
        ];
        let tag_und_nacht = super::NACHREICHEN_DIAGNOSE_MAX_ALTER;
        assert_eq!(
            super::diagnose_kandidaten(&liste, None, tag_und_nacht, 50),
            vec!["eingereicht", "abgebrochen"]
        );
        // Der laufende Flug bleibt aussen vor.
        assert_eq!(
            super::diagnose_kandidaten(&liste, Some("eingereicht"), tag_und_nacht, 50),
            vec!["abgebrochen"]
        );
        // Gegenprobe: Fuers Flugprotokoll zaehlt der abgebrochene weiter nicht.
        assert_eq!(
            nachreich_kandidaten(&liste, None, tag_und_nacht, 50),
            vec!["eingereicht"]
        );
    }

    /// Wie beim Flugprotokoll: nur ein ausdrueckliches `false` loest etwas
    /// aus, ein aelterer Server (`None`) nicht.
    #[test]
    fn diagnose_nur_ausdruecklich_fehlende() {
        let s = [
            status_mit_diagnose("da", Some(true)),
            status_mit_diagnose("fehlt", Some(false)),
            status_mit_diagnose("alt", None),
        ];
        assert_eq!(super::diagnose_fehlt_auf_dem_server(&s), vec!["fehlt"]);
    }

    fn status_mit_diagnose(id: &str, da: Option<bool>) -> PirepPruefstatus {
        PirepPruefstatus {
            diagnose_log_vorhanden: da,
            ..status(id, None)
        }
    }

    /// Der Abbruch laedt NUR die Diagnose hoch (`nur_diagnose = true`).
    /// Waere das Flugprotokoll dabei, legte der Import eine Sitzung fuer
    /// einen Flug an, den es in phpVMS nicht gibt.
    #[test]
    fn abbruch_laedt_nur_die_diagnose() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        // Am Zeilenanfang suchen: Dieser Test steht FRUEHER in der Datei als
        // die Funktion, und sein eigener Suchtext ist eingerueckt — ohne den
        // Zeilenumbruch fand der Waechter sich selbst (CI 23.09.2026).
        let start = SRC
            .find("\nasync fn flight_cancel(")
            .expect("flight_cancel nicht mehr gefunden — Test anpassen, nicht loeschen")
            + 1;
        let rest_der_datei = &SRC[start..];
        let ende_der_funktion = rest_der_datei[1..]
            .find("\nasync fn ")
            .map(|i| i + 1)
            .unwrap_or(rest_der_datei.len());
        let koerper = &rest_der_datei[..ende_der_funktion];
        let aufruf = koerper
            .find("spawn_flight_log_upload(")
            .expect("flight_cancel laedt die Diagnose nicht mehr hoch");
        let rest = &koerper[aufruf..];
        let ende = rest.find(");").unwrap_or(rest.len());
        // Auf das LETZTE Argument abstellen, nicht auf „irgendwo steht true":
        // ein Variablenname mit „true" darin haette den Waechter sonst
        // gruen gelassen (Cloud-QS 23.09.2026).
        let letztes = rest[..ende]
            .rsplit(',')
            .next()
            .map(str::trim)
            .unwrap_or_default();
        assert_eq!(
            letztes, "true",
            "flight_cancel muss mit nur_diagnose=true aufrufen, sonst wandert \
             das Flugprotokoll eines abgebrochenen Fluges in den Import"
        );
        // Und er muss laufen, BEVOR ein Serverfehler die Funktion verlaesst.
        let fehlerausgang = koerper
            .find("\n    result?;")
            .expect("result?-Ausgang nicht mehr gefunden");
        assert!(
            aufruf < fehlerausgang,
            "ohne Netz kehrt flight_cancel bei result? zurueck — der Upload \
             muss davor stehen"
        );
    }

    /// Das echte Format der Protokollzeile (serde-Tag `type`, snake_case),
    /// auch wenn mehr als das abgesuchte Endstueck davor steht.
    #[test]
    fn eingereicht_erkennt_das_ereignis_am_dateiende() {
        let dir =
            std::env::temp_dir().join(format!("aeroacars-nachreichen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mit = dir.join("mit.jsonl");
        let ohne = dir.join("ohne.jsonl");
        let fuell = "{\"type\":\"position\",\"x\":1}\n".repeat(40_000); // ~1 MB davor
        std::fs::write(
            &mit,
            format!("{fuell}{{\"type\":\"pirep_filed\",\"payload\":{{}}}}\n"),
        )
        .unwrap();
        std::fs::write(&ohne, &fuell).unwrap();
        assert!(protokoll_ist_eingereicht(&mit));
        assert!(!protokoll_ist_eingereicht(&ohne));
        assert!(!protokoll_ist_eingereicht(&dir.join("gibtsnicht.jsonl")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── DAL667 (07.11.2026): Abgabe eines gelandeten Flugs ───────────────
    //
    // Gelandet und geparkt in KSFO, danach im MSFS-Menue eine 777 in Rio
    // geladen, nach App-Neustart „Trotzdem fortsetzen" in Dubai. Der Flug
    // wurde nie abgegeben und spaeter storniert.

    fn gelandet_in_ksfo() -> super::FlightStats {
        let mut st = super::FlightStats::new();
        st.phase = super::FlightPhase::Arrived;
        st.landing_rate_fpm = Some(-180.0);
        st.landing_at = Some(chrono::Utc::now() - chrono::Duration::minutes(75));
        st.landing_lat = Some(37.6155);
        st.landing_lon = Some(-122.3900);
        st.last_known_lat = Some(37.6155);
        st.last_known_lon = Some(-122.3900);
        st
    }

    fn sprung_nach_dubai() -> super::ResumeDiscontinuity {
        super::ResumeDiscontinuity {
            drift_nm: 7030.0,
            altitude_delta_ft: 0.0,
            fuel_delta_kg: 0.0,
            both_grounded: true,
            ziel_vorher_nm: Some(0.5),
            ziel_nachher_nm: Some(7030.0),
            danach_am_boden: true,
            luecke_secs: Some(4200),
            nur_bestaetigt: true,
            am: None,
            nach_ankunft: false,
        }
    }

    #[test]
    fn sprung_vermerken_haengt_an_und_stempelt() {
        use super::{sprung_vermerken, FlightPhase};
        let jetzt = chrono::Utc::now();
        let mut st = gelandet_in_ksfo();
        sprung_vermerken(&mut st, sprung_nach_dubai(), jetzt);
        assert_eq!(st.resume_spruenge.len(), 1);
        let d = st.resume_discontinuity.expect("juengster Sprung fehlt");
        assert_eq!(d.am, Some(jetzt));
        assert!(d.nach_ankunft, "geparkt mit gemessener Landung → nach der Ankunft");

        // Im Reiseflug ist es ein Sprung des Flugs, nicht nach der Ankunft.
        let mut reise = super::FlightStats::new();
        reise.phase = FlightPhase::Cruise;
        sprung_vermerken(&mut reise, sprung_nach_dubai(), jetzt);
        assert!(!reise.resume_discontinuity.unwrap().nach_ankunft);

        // Geparkt, aber die Landung wurde nie gemessen (GAF 9655): kein Freibrief.
        let mut ungemessen = gelandet_in_ksfo();
        ungemessen.landing_rate_fpm = None;
        sprung_vermerken(&mut ungemessen, sprung_nach_dubai(), jetzt);
        assert!(!ungemessen.resume_discontinuity.unwrap().nach_ankunft);
    }

    #[test]
    fn sprung_nach_dem_parken_sperrt_die_abgabe_nicht() {
        use super::{abgabe_sperre, nach_ankunft_notiz, sperrender_sprung, sprung_vermerken, spruenge_fuer_notiz};
        let mut st = gelandet_in_ksfo();
        sprung_vermerken(&mut st, sprung_nach_dubai(), chrono::Utc::now());
        assert_eq!(abgabe_sperre(&st), None, "DAL667: fertiger Flug, keine Sperre");
        assert!(!sperrender_sprung(&st), "kein Sprung-Banner fuer einen Wechsel nach dem Parken");
        assert!(spruenge_fuer_notiz(&st).is_empty(), "kein Vermerk UNTERBROCHENER FLUG");
        let notiz = nach_ankunft_notiz(&st, "Rest");
        assert!(notiz.starts_with("NACH DER ANKUNFT:"), "{notiz}");
        assert!(notiz.ends_with("Rest"), "{notiz}");
        // Ohne Wechsel bleibt die Notiz unberuehrt.
        assert_eq!(nach_ankunft_notiz(&gelandet_in_ksfo(), "Rest"), "Rest");
    }

    #[test]
    fn sprung_vor_der_landung_sperrt_weiter() {
        use super::{abgabe_sperre, sprung_vermerken, AbgabeSperre, FlightPhase};
        let t0 = chrono::Utc::now();

        // MSC1588: Sprung im Reiseflug, danach „Arrived" mit gemessener Landung.
        let mut st = gelandet_in_ksfo();
        st.phase = FlightPhase::Cruise;
        st.landing_at = None;
        st.landing_rate_fpm = None;
        sprung_vermerken(&mut st, sprung_nach_dubai(), t0 - chrono::Duration::minutes(30));
        st.phase = FlightPhase::Arrived;
        st.landing_at = Some(t0);
        st.landing_rate_fpm = Some(-150.0);
        assert_eq!(abgabe_sperre(&st), Some(AbgabeSperre::Sprung));

        // Ein Sprung nach der Landung verdeckt einen davor nicht.
        sprung_vermerken(&mut st, sprung_nach_dubai(), t0 + chrono::Duration::minutes(10));
        assert_eq!(abgabe_sperre(&st), Some(AbgabeSperre::Sprung));

        // Stempel „nach der Ankunft", aber danach noch einmal gelandet: davor.
        let mut neu = gelandet_in_ksfo();
        sprung_vermerken(&mut neu, sprung_nach_dubai(), t0);
        neu.landing_at = Some(t0 + chrono::Duration::minutes(40));
        assert_eq!(abgabe_sperre(&neu), Some(AbgabeSperre::Sprung));

        // Aus einer aelteren Fassung gespeichert (ohne Stempel): sperrt.
        let mut alt = gelandet_in_ksfo();
        alt.resume_discontinuity = Some(sprung_nach_dubai());
        assert_eq!(abgabe_sperre(&alt), Some(AbgabeSperre::Sprung));
    }

    #[test]
    fn auto_file_entscheidet_unabhaengig_von_pause_und_sprung() {
        use super::{auto_file_entscheiden, sprung_vermerken, AbgabeSperre, FlightPhase};
        let jetzt = chrono::Utc::now();

        // DAL667: geparkt, Simulator pausiert, danach ein anderes Flugzeug.
        let mut st = gelandet_in_ksfo();
        st.paused_since = Some(jetzt);
        sprung_vermerken(&mut st, sprung_nach_dubai(), jetzt);
        assert_eq!(auto_file_entscheiden(&mut st, false, jetzt.timestamp()), Some(Ok(())));
        assert!(st.auto_file_gestartet, "Merker gesetzt");
        assert_eq!(
            auto_file_entscheiden(&mut st, false, jetzt.timestamp()),
            None,
            "ein zweiter Takt gibt nicht noch einmal ab"
        );

        // MSC1588 bleibt beim Piloten.
        let mut msc = gelandet_in_ksfo();
        msc.phase = FlightPhase::Cruise;
        sprung_vermerken(&mut msc, sprung_nach_dubai(), jetzt - chrono::Duration::minutes(90));
        msc.phase = FlightPhase::Arrived;
        assert_eq!(
            auto_file_entscheiden(&mut msc, false, jetzt.timestamp()),
            Some(Err((false, Some(AbgabeSperre::Sprung))))
        );
        assert!(!msc.auto_file_gestartet);
    }

    #[test]
    fn abgabe_tor_misst_geparkt_am_aufsetzpunkt() {
        use super::{abgabe_tor_position, FlightPhase};
        let rio = Some((-22.8100, -43.2506));
        let mut st = gelandet_in_ksfo();
        // Nach „Trotzdem fortsetzen" schreibt der Takt Dubai in `last_known_*`.
        st.last_known_lat = Some(25.2528);
        st.last_known_lon = Some(55.3644);
        assert_eq!(abgabe_tor_position(&st, rio), Some((37.6155, -122.3900)));
        assert_eq!(abgabe_tor_position(&st, None), Some((37.6155, -122.3900)));

        // Geparkt ohne Aufsetzpunkt: wie bisher, Live vor gespeichert.
        st.landing_lat = None;
        assert_eq!(abgabe_tor_position(&st, rio), rio);
        assert_eq!(abgabe_tor_position(&st, None), Some((25.2528, 55.3644)));

        // Nicht geparkt: wie bisher die Live-Lage.
        let mut rollen = gelandet_in_ksfo();
        rollen.phase = FlightPhase::TaxiIn;
        assert_eq!(abgabe_tor_position(&rollen, rio), rio);
    }

    #[test]
    fn live_snapshot_nur_wenn_er_zum_flug_gehoert() {
        use super::{live_snapshot_gehoert_zum_flug, sprung_vermerken};
        let st = gelandet_in_ksfo();
        assert!(live_snapshot_gehoert_zum_flug(&st, false));
        assert!(!live_snapshot_gehoert_zum_flug(&st, true), "was_just_resumed");
        let mut pause = gelandet_in_ksfo();
        pause.paused_since = Some(chrono::Utc::now());
        assert!(!live_snapshot_gehoert_zum_flug(&pause, false), "Simulator pausiert");
        let mut gewechselt = gelandet_in_ksfo();
        sprung_vermerken(&mut gewechselt, sprung_nach_dubai(), chrono::Utc::now());
        assert!(!live_snapshot_gehoert_zum_flug(&gewechselt, false), "anderes Flugzeug nach dem Parken");
    }

    /// Nach einem Wechsel nach dem Parken haelt der Streamer den Flug an —
    /// sonst gehen die Werte des neuen Flugzeugs als Spur hinaus (DAL667,
    /// Recorder-Sitzung 3081: sechs Dubai-Positionen der 777).
    #[test]
    fn nach_dem_parken_gewechselt_wird_nicht_weiter_aufgezeichnet() {
        use super::{flug_nach_ankunft_eingefroren, sprung_vermerken, FlightPhase};
        let mut st = gelandet_in_ksfo();
        assert!(!flug_nach_ankunft_eingefroren(&st), "ohne Wechsel laeuft alles normal");
        sprung_vermerken(&mut st, sprung_nach_dubai(), chrono::Utc::now());
        assert!(flug_nach_ankunft_eingefroren(&st));
        // Ein Sprung im Flug haelt die Aufzeichnung nicht an (dort fragt der Pilot).
        let mut reise = gelandet_in_ksfo();
        reise.phase = FlightPhase::Cruise;
        sprung_vermerken(&mut reise, sprung_nach_dubai(), chrono::Utc::now());
        assert!(!flug_nach_ankunft_eingefroren(&reise));

        // Und der Streamer fragt es am Tor des Lebenszeichen-Zweigs.
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let tor = SRC
            .find(concat!("|| flug_nach_ankunft_", "eingefroren(&flight.stats"))
            .expect("Streamer haelt den Flug nach einem Wechsel nicht an");
        let mut ab = tor.saturating_sub(200);
        while !SRC.is_char_boundary(ab) {
            ab -= 1;
        }
        let vorher = &SRC[ab..tor];
        assert!(vorher.contains("was_just_resumed"), "nicht am Lebenszeichen-Tor: {vorher}");
    }

    /// Codex-Pruefung DAL667, Befund 2: „Flug abgeben" aus dem Banner (nach
    /// Neustart, anderes Flugzeug weit weg) vermerkt den Wechsel. Die
    /// Berechnung ist dieselbe wie bei „Trotzdem fortsetzen".
    #[test]
    fn abweichung_als_sprung_erkennt_den_wechsel() {
        use super::{abweichung_als_sprung, sprung_vermerken, abgabe_sperre, nach_ankunft_notiz};
        let mut st = gelandet_in_ksfo();
        st.last_known_alt_ft = Some(13.0);
        st.last_known_gs_kt = Some(0.0);
        st.last_known_at = Some(chrono::Utc::now() - chrono::Duration::minutes(60));
        let jetzt = chrono::Utc::now();
        let mut dubai = sim_core::SimSnapshot::default();
        dubai.lat = 25.2481;
        dubai.lon = 55.3644;
        dubai.altitude_msl_ft = 4.0;
        dubai.on_ground = true;
        dubai.fuel_total_kg = 63191.0;
        let d = abweichung_als_sprung(&st, &dubai, jetzt).expect("Wechsel nach Dubai nicht erkannt");
        assert!(d.nur_bestaetigt);
        assert!(d.drift_nm > 6000.0, "{}", d.drift_nm);
        // Am selben Stand passt die Lage: kein Vermerk.
        let mut gate = dubai.clone();
        gate.lat = 37.6155;
        gate.lon = -122.3900;
        gate.fuel_total_kg = 0.0;
        assert!(abweichung_als_sprung(&st, &gate, jetzt).is_none());
        // Vermerkt sperrt er nicht und steht in der Notiz.
        sprung_vermerken(&mut st, d, jetzt);
        assert_eq!(abgabe_sperre(&st), None);
        assert!(nach_ankunft_notiz(&st, "").starts_with("NACH DER ANKUNFT:"));
    }

    /// Befund 2, Verdrahtung: beide Abgabewege vermerken vor dem Lesen.
    #[test]
    fn beide_abgabewege_vermerken_den_wechsel() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let aufruf = concat!("wechsel_vor_der_abgabe_", "vermerken(&app, ");
        for (kopf, lesen) in [
            (concat!("\nasync fn ", "flight_end("), "validate_for_filing("),
            ("\nasync fn flight_end_manual(", "pirep_bewertete_teile("),
        ] {
            let start = SRC.find(kopf).unwrap_or_else(|| panic!("{kopf} nicht gefunden"));
            let koerper = &SRC[start..];
            let v = koerper.find(aufruf).unwrap_or_else(|| panic!("{kopf}: kein Vermerk"));
            let l = koerper.find(lesen).unwrap_or_else(|| panic!("{kopf}: {lesen} fehlt"));
            assert!(v < l, "{kopf}: Vermerk muss vor {lesen} stehen");
        }
    }

    /// Befund 1: Das Lebenszeichen im angehaltenen Zustand braucht keinen
    /// guten Snapshot — frisch nach einem Neustart gibt es keinen, und ohne
    /// Lebenszeichen raeumt phpVMS den PIREP ab.
    #[test]
    fn lebenszeichen_ohne_guten_snapshot() {
        use super::{build_heartbeat_body, FlightPhase};
        let mut st = gelandet_in_ksfo();
        st.takeoff_at = Some(chrono::Utc::now() - chrono::Duration::hours(7));
        st.landing_at = Some(chrono::Utc::now() - chrono::Duration::hours(1));
        let body = build_heartbeat_body(&sim_core::SimSnapshot::default(), &st, FlightPhase::Arrived);
        assert_eq!(body.flight_time, Some(360), "Flugzeit aus den Flugdaten");

        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let tor = SRC
            .find(concat!("|| flug_nach_ankunft_", "eingefroren(&flight.stats"))
            .expect("Tor nicht gefunden");
        let zweig = &SRC[tor..tor + SRC[tor..].find("continue;").unwrap()];
        assert!(
            !zweig.contains("if let Some(hb) = last_good_snap"),
            "Lebenszeichen haengt wieder an einem guten Snapshot"
        );
    }

