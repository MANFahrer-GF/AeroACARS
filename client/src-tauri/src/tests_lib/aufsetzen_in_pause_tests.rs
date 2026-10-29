    use super::*;

    /// Codex-Review PR #158: Der Abtaster erkennt den Neulade-Sprung selbst,
    /// bevor der Streamer das Pausen-Flag setzt (bis ~3 s später).
    #[test]
    fn abtaster_erkennt_neuladen_selbst() {
        // KLM759 17.10.: FL372 in der Luft → 233 ft am Boden.
        assert!(sampler_sieht_neuladen(FlightPhase::Cruise, Some((false, 37_226.0)), true, 233.0));
        // Echte Landung: 20 ms vorher knapp über der Bahn → kein Neuladen.
        assert!(!sampler_sieht_neuladen(FlightPhase::Final, Some((false, 312.0)), true, 310.0));
        // Kein Vorwert (erster Messwert, nach der Pause) → nichts annehmen.
        assert!(!sampler_sieht_neuladen(FlightPhase::Cruise, None, true, 233.0));
        // Am Boden geblieben (Rollen) → kein Neuladen.
        assert!(!sampler_sieht_neuladen(FlightPhase::Cruise, Some((true, 233.0)), true, 233.0));
    }

    /// Der pausierte Flug wie bei DLA 8260: Phase bleibt HOLDING, war in der
    /// Luft, und vor dem Neuladen steht schon ein Kandidat samt Messpuffer.
    fn pausierter_flug(t0: DateTime<Utc>) -> FlightStats {
        let mut st = FlightStats::new();
        st.phase = FlightPhase::Holding;
        st.was_airborne = true;
        st.pending_td_at = Some(t0);
        st.pending_td_premium_vs = Some(-180.0);
        st.pending_td_premium_g = Some(1.3);
        st.pending_td_premium_gesehen = true;
        st.snapshot_buffer.push_back(TelemetrySample {
            at: t0,
            on_ground: false,
            ..Default::default()
        });
        st
    }

    /// Pausiert nach Sim-Neuladen, dann am Boden mit Aufsetzflanke (Flanke
    /// und Plugin-Meldung): es entsteht kein Kandidat und damit kein
    /// Touchdown, und der Kandidat von vor dem Neuladen ist verworfen.
    #[test]
    fn nach_sim_neuladen_entsteht_kein_touchdown() {
        let t0 = Utc::now();
        let mut st = pausierter_flug(t0);
        // Die Phase allein haette den Aufsetzer durchgelassen — der Fehler.
        assert!(is_touchdown_capturable_phase(st.phase, st.was_airborne));

        assert!(
            aufsetz_erkennung_ruht_nach_reload(&mut st, true),
            "nach Sim-Neuladen muss die Aufsetzerkennung ruhen"
        );
        assert_eq!(st.pending_td_at, None, "Kandidat von vor dem Neuladen verworfen");
        assert!(st.pending_td_premium_vs.is_none() && st.pending_td_premium_g.is_none());
        assert!(!st.pending_td_premium_gesehen);
        assert!(st.snapshot_buffer.is_empty(), "Messpuffer verworfen");
        // Der Sampler springt danach zum naechsten Messwert — keine Flanke,
        // keine Plugin-Meldung wird aufgenommen, nichts wird bestaetigt.
        assert!(st.sampler_touchdown_at.is_none());
        // Auch mehrere Platzrunden lang: jeder Messwert ruht.
        for i in 1..=12 {
            assert!(aufsetz_erkennung_ruht_nach_reload(&mut st, true));
            assert!(st.pending_td_at.is_none(), "Runde {i}");
        }
    }

    /// Gegenprobe: Nach dem Fortsetzen (`was_just_resumed` false) ruht
    /// nichts mehr, der Zustand bleibt unberuehrt und eine echte Landung
    /// wird wieder als Kandidat aufgenommen.
    #[test]
    fn nach_dem_fortsetzen_wird_die_echte_landung_erkannt() {
        let t0 = Utc::now();
        let mut st = pausierter_flug(t0);
        st.phase = FlightPhase::Final;
        assert!(!aufsetz_erkennung_ruht_nach_reload(&mut st, false));
        assert_eq!(st.pending_td_at, Some(t0), "ohne Pause nichts verworfen");
        assert_eq!(st.snapshot_buffer.len(), 1);

        let mut st = FlightStats::new();
        st.phase = FlightPhase::Final;
        st.was_airborne = true;
        assert!(aufsetz_kandidat_zulaessig(&st, false));
        // Plugin-Pfad nimmt den Kandidaten auf.
        let t_td = t0 + chrono::Duration::seconds(30);
        let zulaessig = aufsetz_kandidat_zulaessig(&st, false);
        assert_eq!(
            premium_td_uebernehmen(&mut st, -240.0, 1.25, zulaessig, t_td),
            PremiumTd::NeuerKandidat
        );
        assert_eq!(st.pending_td_at, Some(t_td));
    }

    /// Sim-Pause bzw. ACARS-Pause: kein NEUER Kandidat — ein laufender bleibt
    /// stehen (Esc kurz nach dem echten Aufsetzen darf die Landung nicht
    /// kosten).
    #[test]
    fn in_der_pause_kein_neuer_kandidat() {
        let t0 = Utc::now();
        let mut st = FlightStats::new();
        st.phase = FlightPhase::Holding;
        st.was_airborne = true;
        assert!(aufsetz_kandidat_zulaessig(&st, false));
        assert!(!aufsetz_kandidat_zulaessig(&st, true), "Sim-Pause");
        st.paused_since = Some(t0);
        assert!(!aufsetz_kandidat_zulaessig(&st, false), "ACARS-Pause");
        let zulaessig = aufsetz_kandidat_zulaessig(&st, false);
        assert_eq!(
            premium_td_uebernehmen(&mut st, -200.0, 1.2, zulaessig, t0),
            PremiumTd::Verworfen
        );
        assert!(st.pending_td_at.is_none());

        let mut st = pausierter_flug(t0);
        st.paused_since = Some(t0);
        assert!(!aufsetz_kandidat_zulaessig(&st, false));
        assert_eq!(st.pending_td_at, Some(t0), "laufender Kandidat bleibt");
    }

    /// Die Schleife des Samplers ist nicht aufrufbar (Hintergrund-Faden,
    /// AppHandle). Dieser Waechter haelt fest, dass sie die beiden Sperren
    /// wirklich benutzt: das Ruhen nach dem Neuladen VOR der Flanken-Pruefung,
    /// mit `was_just_resumed`, samt Vergessen von `prev_in_air` — und dass
    /// Plugin- und Flankenpfad nicht mehr nur die Phase fragen.
    #[test]
    fn sampler_benutzt_die_pausen_sperren() {
        const SRC: &str = include_str!("../lib.rs");
        let ohne_kommentare: String = SRC
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // Am Zeilenanfang suchen und den Text zusammensetzen — sonst finden
        // dieser und der Waechter `beide_episoden_enden_raeumen_die_landung_ab`
        // den Suchtext hier im Testmodul, das VOR der Funktion steht.
        let start = ohne_kommentare
            .find(concat!("\nfn ", "spawn_touchdown_sampler("))
            .expect("spawn_touchdown_sampler nicht gefunden — Test anpassen, nicht löschen");
        let rest = &ohne_kommentare[start..];
        let ende = rest[1..].find("\nfn ").map(|i| i + 1).unwrap_or(rest.len());
        let koerper = &rest[..ende];

        let ruhen = koerper
            .find("aufsetz_erkennung_ruht_nach_reload(")
            .expect("Sampler ruft aufsetz_erkennung_ruht_nach_reload nicht");
        let flanke = koerper
            .find("TD-edge candidate detected")
            .expect("Flanken-Block nicht gefunden");
        assert!(ruhen < flanke, "Ruhen muss vor der Flanken-Pruefung stehen");
        let aufruf = &koerper[ruhen..ruhen + 800];
        assert!(aufruf.contains("was_just_resumed"), "Ruhen haengt an was_just_resumed");
        assert!(aufruf.contains("prev_in_air = None"), "prev_in_air wird vergessen");
        assert!(aufruf.contains("continue"), "der Messwert wird uebersprungen");
        assert_eq!(
            koerper.matches("aufsetz_kandidat_zulaessig(").count(),
            2,
            "Plugin- und Flankenpfad fragen beide die Pausen-Sperre"
        );
        assert!(
            !koerper.contains("is_touchdown_capturable_phase("),
            "nur die Phase zu fragen laesst Aufsetzer in der Pause durch"
        );
    }
