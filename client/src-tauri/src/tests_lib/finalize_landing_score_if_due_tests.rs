    use super::*;

    fn t(secs: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(secs, 0).expect("valid ts")
    }

    #[test]
    fn finalizes_after_window_with_no_mention_of_flight_phase_gsg1249() {
        // Das ist der eigentliche Regressionsschutz: landing_at, dann ein
        // "Tick" 9s spaeter (> BOUNCE_WINDOW_SECS) — bei GSG1249 waere die
        // Flugphase zu diesem Zeitpunkt schon TaxiIn gewesen. Die Funktion
        // bekommt gar keine Phase als Parameter, kann sie also strukturell
        // nicht mehr voraussetzen.
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-13.0); // GSG1249-Groessenordnung
                                                 // Das Aufsetzfenster lag vor und war ausreichend — hier geht es um
                                                 // die Flugphase, nicht um die Datenlage (Ergänzung 12.09.2026, seit
                                                 // eine Landung ohne Fenster keine Note mehr bekommt).
        stats.touchdown_window_score_data_ready_at = Some(t(1));
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(
            stats.landing_score.is_some(),
            "Score muss finalisiert sein, auch wenn die Flugphase laengst gewechselt hat"
        );
    }

    #[test]
    fn does_not_finalize_before_window_closes() {
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-13.0);
        finalize_landing_score_if_due(&mut stats, "TEST123", t(5), 0.0, true);
        assert!(
            stats.landing_score.is_none(),
            "vor Ablauf von BOUNCE_WINDOW_SECS darf noch nicht finalisiert werden"
        );
    }

    #[test]
    fn does_not_re_finalize_once_already_set() {
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-13.0);
        stats.landing_score = Some(LandingScore::Hard);
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert_eq!(
            stats.landing_score,
            Some(LandingScore::Hard),
            "ein bereits gesetzter Score darf nicht ueberschrieben werden"
        );
    }

    #[test]
    fn stays_none_without_landing_at() {
        let mut stats = FlightStats::default();
        stats.landing_peak_vs_fpm = Some(-13.0);
        finalize_landing_score_if_due(&mut stats, "TEST123", t(100), 0.0, true);
        assert!(stats.landing_score.is_none());
    }

    #[test]
    fn b005_guard_survives_the_extraction_no_plausible_vs_stays_none() {
        // B-005: fehlt jede plausible Kanonik-Quelle, bleibt landing_score
        // None statt einen erfundenen 0.0-Score zu setzen — derselbe Guard
        // wie vor der Extraktion, jetzt bloss ausserhalb des Match-Arms.
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        // Bewusst KEIN landing_peak_vs_fpm/landing_analysis/landing_rate_fpm.
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(stats.landing_score.is_none());
    }

    /// AFR 421 (06.10.2026): „nicht bewertbar" gilt als abgeschlossen, damit
    /// die Anzeigen nicht ewig „Landung wird ausgewertet" zeigen. Ohne
    /// Abdeckungsbefund bleibt die Auswertung offen.
    #[test]
    fn nicht_bewertbar_gilt_als_abgeschlossen() {
        // Fenster ausgewertet und dicht, nur keine Sinkrate: kein Befund.
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.touchdown_window_score_data_ready_at = Some(t(1));
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(stats.landing_score.is_none());
        assert!(stats.landung_abdeckung_fehlt.is_none());
        assert!(!stats.landing_score_finalized, "ohne Befund bleibt sie offen");

        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landung_abdeckung_fehlt = Some(touchdown_v2::FehlendeAbdeckung {
            groesste_luecke_ms: 300,
            proben: 5,
        });
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(stats.landing_score.is_none(), "keine Note");
        assert!(stats.landing_score_finalized, "aber abgeschlossen");
    }

    #[test]
    fn bug_b_active_sampler_rejection_blocks_fsm_fallback_score() {
        // Bug B (Codex): der Sampler hat den Kandidaten AKTIV per FalseEdge
        // abgelehnt (z.B. InsufficientVoteScore). `landing_at`/`landing_peak_vs_fpm`
        // sind trotzdem plausibel gesetzt (die FSM haette also OHNE diesen
        // Guard einen Score erfunden, obwohl der Sampler den Touchdown mit
        // Evidenz verworfen hat) — landing_score MUSS None bleiben.
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-250.0); // plausibler FSM-Fallback-Wert
        stats.sampler_touchdown_rejected_reason =
            Some(touchdown_v2::FalseEdgeReason::InsufficientVoteScore);
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(
            stats.landing_score.is_none(),
            "eine aktive Sampler-Ablehnung darf vom FSM-Fallback nicht ueberstimmt werden"
        );
    }

    #[test]
    fn bug_b_insufficient_telemetry_still_allows_fsm_fallback_score() {
        // Gegenprobe: `InsufficientTelemetry` ist KEINE aktive Ablehnung
        // ("Sampler weiss es nicht"), sondern legitimiert genau den
        // FSM-Fallback, den dieser Guard sonst blockiert. Dieses Feld
        // wird fuer diesen reason nie gesetzt (siehe Setzstelle) — dieser
        // Test haelt genau das fest, damit niemand die Unterscheidung
        // spaeter versehentlich aufhebt.
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-250.0);
        stats.sampler_touchdown_rejected_reason = None; // s.o.: InsufficientTelemetry setzt nichts
                                                        // Das Fenster lag vor und war ausreichend: Hier geht es um den
                                                        // Bug-B-Guard (aktive Ablehnung des Samplers), nicht um die Frage,
                                                        // ob überhaupt gemessen wurde.
        stats.touchdown_window_score_data_ready_at = Some(t(1));
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(
            stats.landing_score.is_some(),
            "ohne aktive Ablehnung muss der FSM-Fallback weiter scoren duerfen"
        );

        // Ergänzung 12.09.2026: OHNE ausgewertetes Fenster ist derselbe Weg
        // dicht — "der Sampler weiss es nicht" heisst jetzt "wir wissen es
        // nicht", nicht "dann schätzen wir". Das war der Weg, über den
        // CFG 2090 zu 97 Punkten kam.
        let mut ohne_fenster = FlightStats::default();
        ohne_fenster.landing_at = Some(t(0));
        ohne_fenster.landing_peak_vs_fpm = Some(-250.0);
        finalize_landing_score_if_due(&mut ohne_fenster, "TEST123", t(9), 0.0, true);
        assert!(ohne_fenster.landing_score.is_none());
        assert!(ohne_fenster.landung_abdeckung_fehlt.is_some());
    }

    // ── GAF-707-Race, strukturell geloest (Vereinheitlichung 09/2026) ──────
    // Der Sampler-Dump setzt `landing_score` nicht mehr selbst — diese
    // Funktion ist die EINZIGE Quelle. Drei von Codex in zwei frueheren
    // Runden gefundene Luecken werden hier je einzeln nachgestellt.

    #[test]
    fn live_agl_check_blocks_finalisation_before_the_fsm_pending_latch_exists() {
        // Luecke 1 (Codex-Runde 2): `touch_and_go_pending_since` ist ein
        // NACHLAUFENDES Signal, existiert also im allerersten Tick eines
        // Schwellenuebertritts noch nicht. Der Live-Check auf agl_ft/
        // on_ground direkt aus dem SimSnapshot muss diesen Tick trotzdem
        // schon abfangen.
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-13.0);
        stats.touch_and_go_pending_since = None; // absichtlich: FSM hat es noch nicht gesetzt
        let agl_over_threshold = TOUCH_AND_GO_AGL_THRESHOLD_FT + 1.0;
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), agl_over_threshold, false);
        assert!(
            stats.landing_score.is_none(),
            "ueber der T&G-Schwelle in der Luft darf nicht finalisiert werden, \
             auch wenn touch_and_go_pending_since noch nicht gesetzt ist"
        );
    }

    #[test]
    fn on_ground_below_threshold_still_finalizes_normally() {
        // Gegenprobe: der Live-Check darf normale Landungen nicht bremsen.
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-13.0);
        stats.touchdown_window_score_data_ready_at = Some(t(1));
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(stats.landing_score.is_some());
    }

    #[test]
    fn waits_while_a_touch_and_go_dwell_confirmation_is_in_progress() {
        // Zweite Sicherung fuer die Dwell-Dauer selbst (nachdem der Live-
        // Check schon durchgelaufen waere, z.B. AGL zwischenzeitlich wieder
        // knapp unter der Schwelle, aber die FSM prueft noch).
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-13.0);
        stats.touch_and_go_pending_since = Some(t(9));
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(
            stats.landing_score.is_none(),
            "waehrend einer laufenden T&G-Dwell-Bestaetigung darf nicht gescort werden"
        );
    }

    #[test]
    fn sampler_only_bush_flight_without_landing_at_still_scores_via_sampler_touchdown_at() {
        // Luecke 2 (Codex-Runde 2): Busch-Landungen, bei denen die FSM nie
        // FlightPhase::Landing erreicht, haben KEIN `landing_at`. Vor diesem
        // Umbau bekamen sie ihren Score trotzdem sofort vom Sampler-Dump —
        // der doppelte Anker (`landing_at.or(sampler_touchdown_at)`) muss das
        // wiederherstellen.
        let mut stats = FlightStats::default();
        stats.landing_at = None; // FSM nie in Landing angekommen
        stats.sampler_touchdown_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-13.0);
        stats.touchdown_window_score_data_ready_at = Some(t(1)); // Dump bereits fertig
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(
            stats.landing_score.is_some(),
            "ein reiner Sampler-Touchdown ohne landing_at darf nicht dauerhaft ohne Score bleiben"
        );
    }

    #[test]
    fn waits_for_score_data_ready_before_scoring_with_refined_data() {
        // Luecke 3 (Codex-Runde 2): `touchdown_window_dumped_at` wird vom
        // Sampler frueh gesetzt (Doppel-Dump-Schutz), lange bevor die
        // verfeinerten Rohdaten geschrieben sind. Diese Funktion darf nur
        // auf `touchdown_window_score_data_ready_at` warten, nicht auf
        // `touchdown_window_dumped_at`.
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-13.0); // grobe FSM-Schaetzung vorhanden
        stats.sampler_touchdown_at = Some(t(0));
        stats.touchdown_window_dumped_at = Some(t(0)); // frueh gesetzt (Doppel-Dump-Schutz)
        stats.touchdown_window_score_data_ready_at = None; // Rohdaten NOCH NICHT fertig
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(
            stats.landing_score.is_none(),
            "vor touchdown_window_score_data_ready_at darf nicht mit der groeberen \
             Schaetzung gescort werden, auch wenn touchdown_window_dumped_at schon gesetzt ist"
        );
    }

    #[test]
    fn scores_immediately_once_score_data_is_ready() {
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-13.0);
        stats.sampler_touchdown_at = Some(t(0));
        stats.touchdown_window_dumped_at = Some(t(0));
        stats.touchdown_window_score_data_ready_at = Some(t(1));
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(
            stats.landing_score.is_some(),
            "sobald die Rohdaten bereit sind, darf ganz normal finalisiert werden"
        );
        assert!(stats.landing_score_finalized);
        assert!(!stats.landing_score_announced);
    }

    #[test]
    fn falls_back_after_sampler_dump_wait_timeout_instead_of_waiting_forever() {
        // Der Dump ist nie fertig geworden. Das Warten endet trotzdem nach
        // SAMPLER_DUMP_WAIT_TIMEOUT_MS — sonst hinge der Flug für immer.
        //
        // GEÄNDERT am 12.09.2026 (Codex-Abnahme): Früher entstand hier eine
        // Note aus der gröberen FSM-Schätzung. Genau darüber bekam CFG 2090
        // seine 97 Punkte für einen Aufsetzmoment, den niemand aufgezeichnet
        // hat. Ohne Fenster ist nichts gemessen — der Vorgang wird
        // abgeschlossen, aber als "nicht bewertbar", nicht mit einer Note.
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-13.0);
        stats.sampler_touchdown_at = Some(t(0));
        stats.touchdown_window_score_data_ready_at = None;
        let timeout_secs = SAMPLER_DUMP_WAIT_TIMEOUT_MS / 1000;
        finalize_landing_score_if_due(&mut stats, "TEST123", t(timeout_secs), 0.0, true);
        assert!(
            stats.landung_abdeckung_fehlt.is_some(),
            "ohne Aufsetzfenster muss der Vorgang als nicht messbar enden"
        );
        assert!(
            stats.landing_score.is_none(),
            "und ohne Messung darf keine Note entstehen"
        );
        assert_eq!(
            stats.canonical_landing_rate_fpm(),
            None,
            "auch die Ersatzrate ist damit gesperrt"
        );
    }

    #[test]
    fn nach_dem_timeout_bleibt_die_vom_simulator_gemessene_rate_gueltig() {
        // Die Gegenprobe zum Test darüber: Hat MSFS die Aufsetzrate selbst
        // gemeldet, hängt sie nicht an unserem Fenster. 29 von 1170
        // Landungen des Bestands haben gar keinen Dump und trotzdem einen
        // belastbaren Wert — die dürfen ihre Note behalten.
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-168.0);
        stats.landing_rate_fpm = Some(-168.0);
        stats.landing_source = Some("msfs_simvar_latched".to_string());
        stats.sampler_touchdown_at = Some(t(0));
        stats.touchdown_window_score_data_ready_at = None;
        let timeout_secs = SAMPLER_DUMP_WAIT_TIMEOUT_MS / 1000;
        finalize_landing_score_if_due(&mut stats, "TEST123", t(timeout_secs), 0.0, true);
        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-168.0));
        assert!(stats.landing_score.is_some(), "diese Landung ist bewertbar");
    }

    #[test]
    fn no_sampler_touchdown_at_all_scores_immediately_via_fsm_fallback() {
        // Reiner FSM-Pfad (Sampler hat nie einen Kandidaten validiert) —
        // kein Dump zum Warten, sofort entscheiden.
        //
        // Der Pfad bleibt, aber sein Ergebnis hängt an der Quelle: Eine vom
        // Simulator selbst gemeldete Rate trägt eine Note, eine eigene
        // Schätzung ohne Aufsetzfenster nicht (12.09.2026).
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t(0));
        stats.landing_peak_vs_fpm = Some(-13.0);
        stats.landing_source = Some("msfs_simvar_latched".to_string());
        stats.sampler_touchdown_at = None;
        finalize_landing_score_if_due(&mut stats, "TEST123", t(9), 0.0, true);
        assert!(stats.landing_score.is_some());
    }

    /// Codex-QS-Fund (Vereinheitlichung 09/2026, Runde 4): der Multi-TD-
    /// Climb-out-Reset im Sampler-Task raeumt `sampler_touchdown_at` unter
    /// einem DRITTEN, spaeteren Lock aus — nach der Stelle, an der
    /// `touchdown_window_score_data_ready_at` gesetzt wird. Bei einem
    /// FRUEHEN Dump (`climbed_out_early`) waeren beide Felder kurzzeitig
    /// gleichzeitig gesetzt gewesen, ein drittes, enges Zeitfenster fuer
    /// denselben Wettlauf. Der Live-AGL-Check in `finalize_landing_score_
    /// if_due` faengt das zwar ab, aber sauberer: bei einem fruehen Dump
    /// wird "bereit" erst gar nicht signalisiert. Quelltext-Wächter statt
    /// Ablauf-Test, weil die Logik tief im 50Hz-Sampler-Task sitzt (analog
    /// zu `beide_episoden_enden_raeumen_die_landung_ab`).
    #[test]
    fn early_climbout_dump_never_signals_score_data_ready() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let start = SRC
            .find("s.bounce_count = scored_bounce;")
            .unwrap_or_else(|| {
                panic!("Setzstelle nicht mehr gefunden — Test anpassen, nicht loeschen")
            });
        const FENSTER: usize = 3_500;
        let ende = (start + FENSTER).min(SRC.len());
        let ausschnitt = &SRC[start..ende];
        assert!(
            ausschnitt.contains("if !climbed_out_early {")
                && ausschnitt.contains("s.touchdown_window_score_data_ready_at = Some(now);"),
            "das Bereitschaftssignal darf bei einem fruehen (climbed_out_early) Dump \
             nicht mehr gesetzt werden — sonst oeffnet sich wieder das Zeitfenster \
             zwischen dem Setzen und dem spaeteren Multi-TD-Reset"
        );
    }
