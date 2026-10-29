    //! Die Spur wird so fein, wie der Abtaster misst — nicht so grob,
    //! wie gesendet wird.
    //!
    //! # Warum es diese Pruefungen zusaetzlich zu den Waechtern gibt
    //!
    //! `tests/spur_verdrahtung.rs` prueft, DASS der Puffer abgeschoepft
    //! wird, in der richtigen Reihenfolge, mit Merker — alles am
    //! Quelltext. Keine davon merkt, wenn das Abschoepfen zwar dasteht,
    //! aber nichts bewirkt: Merker zu weit vorn, Filter zu scharf,
    //! Mindestabstand zu gross. Diese Pruefungen messen das Ergebnis.
    //!
    //! # Der Befund dahinter (Thomas, 25.08.2026)
    //!
    //! „Zum Anfang und Ende kaum welche." Flug #1081 (KDAL/13L): sieben
    //! Punkte auf den ersten 298 Metern nach dem Aufsetzen, 42,6 m
    //! Abstand, groesste Luecke 222 m.
    use super::*;

    const M_JE_GRAD_LON: f64 = 111_320.0 * 0.642_787_6; // bei 50 Grad Nord

    fn testbahn() -> runway::RunwayMatch {
        runway::RunwayMatch {
            airport_ident: "TEST".to_string(),
            runway_ident: "09".to_string(),
            heading_true_deg: 90.0,
            length_ft: 9843.0,
            width_ft: 148.0,
            surface: "ASP".to_string(),
            threshold_lat: 50.0,
            threshold_lon: 8.0,
            end_lat: 50.0,
            end_lon: 8.0 + 3000.0 / M_JE_GRAD_LON,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 0.0,
            side: "left".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        }
    }

    fn probe(td: DateTime<Utc>, t_s: f64, gs_kt: f32) -> TelemetrySample {
        let strecke = 300.0 + gs_kt as f64 * 0.514_444 * t_s;
        TelemetrySample {
            // Ausroll-Vorrichtung; der Sprit spielt hier keine Rolle.
            fuel_kg: 0.0,
            at: td + chrono::Duration::milliseconds((t_s * 1000.0) as i64),
            vs_fpm: 0.0,
            g_force: 1.0,
            g_semibody: None,
            on_ground: true,
            agl_ft: 0.0,
            msl_ft: 300.0,
            heading_true_deg: 90.0,
            groundspeed_kt: gs_kt,
            indicated_airspeed_kt: gs_kt,
            true_airspeed_kt: gs_kt,
            lat: 50.0,
            lon: 8.0 + strecke / M_JE_GRAD_LON,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            gear_normal_force_n: None,
            total_weight_kg: None,
        }
    }

    /// Ein Flug, dessen 50-Hz-Puffer `sekunden` Ausrollen enthaelt.
    fn flug_mit_puffer(sekunden: f64, gs_kt: f32) -> FlightStats {
        let mut stats = FlightStats::default();
        let td = Utc::now();
        stats.landing_at = Some(td);
        stats.runway_match = Some(testbahn());
        let mut t = 0.0;
        while t <= sekunden {
            stats.snapshot_buffer.push_back(probe(td, t, gs_kt));
            t += 0.02; // 50 Hz
        }
        stats
    }

    fn abstaende(stats: &FlightStats) -> Vec<f64> {
        stats
            .bahn_spur
            .windows(2)
            .map(|w| (w[1].0 - w[0].0) as f64)
            .collect()
    }

    #[test]
    fn bei_140_knoten_liegen_die_punkte_rund_zehn_meter_auseinander() {
        // Der eigentliche Fall: Die Aufsetzzone wird mit voller Fahrt
        // durchfahren. Vorher kam sie aus dem Zwei-Hertz-Takt und hatte
        // sechsunddreissig Meter zwischen zwei Punkten.
        let mut stats = flug_mit_puffer(4.0, 140.0);
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);

        let n = stats.bahn_spur.len();
        // 4 s bei 140 kt sind 288 m; bei zehn Metern Mindestabstand also
        // rund achtundzwanzig Punkte.
        assert!(
            n >= 25,
            "nur {n} Punkte auf 288 m — das ist der alte Sendetakt"
        );

        let groesste = abstaende(&stats).into_iter().fold(0.0_f64, f64::max);
        assert!(
            groesste < 20.0,
            "groesste Luecke {groesste} m, erwartet unter 20"
        );
    }

    #[test]
    fn der_mindestabstand_wird_trotzdem_eingehalten() {
        // Gegenprobe zur Feinheit: Der Puffer hat fuenfzig Werte je
        // Sekunde. Kaemen sie alle in die Ablage, waeren es tausende —
        // die Zeichnung waere ein Strich und die Meldung unnoetig gross.
        let mut stats = flug_mit_puffer(4.0, 140.0);
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        let n = stats.bahn_spur.len();
        assert!(n < 60, "{n} Punkte — der Mindestabstand greift nicht");
        let kleinster = abstaende(&stats).into_iter().fold(f64::MAX, f64::min);
        assert!(
            kleinster >= 8.0,
            "zwei Punkte nur {kleinster} m auseinander"
        );
    }

    #[test]
    fn der_erste_durchgang_bleibt_fuer_die_anzeige_erhalten() {
        // EWG9503: aufgesetzt, durchgestartet, gelandet. Die erste Spur
        // gehoert nicht in die Bewertung, soll aber gezeigt werden.
        let mut stats = flug_mit_puffer(4.0, 140.0);
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        let erste = stats.bahn_spur.len();
        assert!(erste > 5, "Gegenprobe: der erste Durchgang hat eine Spur");

        // Derselbe Ruecksetzer, den der Sampler beim Steigen ruft.
        clear_approach_stability_and_rollout(&mut stats);
        assert!(stats.bahn_spur.is_empty(), "die Spur der Wertung ist leer");

        // Gleiche Bahn: der Durchgang reist mit, ausgeduennt, nicht leer.
        let d = vorherige_durchgaenge(&stats);
        assert_eq!(d.len(), 1, "der erste Durchgang fehlt");
        assert!(d[0].lateral_samples.len() >= 2);
        assert!(d[0].lateral_samples.len() <= erste);

        // Und er erreicht die Bahnfelder, aus denen Aufzeichnung und
        // Nutzlast entstehen — nicht nur die Hilfsfunktion.
        let felder = bahn_felder(&stats, Some("A320"), None);
        assert_eq!(felder.vorherige_durchgaenge.len(), 1);
        let wire = felder.wire(true);
        assert_eq!(wire.vorherige_durchgaenge.map(|v| v.len()), Some(1));

        // Andere Bahn nach dem Durchstarten: nichts zeigen — die Spur ist
        // gegen die alte Bahn projiziert.
        stats.runway_match.as_mut().unwrap().runway_ident = "27".to_string();
        assert!(vorherige_durchgaenge(&stats).is_empty());
    }

    #[test]
    fn nach_dem_durchstarten_erntet_die_spur_keinen_startlauf() {
        // EWG9503 (#1431): Aufsetzen, Durchstarten, Ruecksetzer beim
        // Steigen. Der Puffer haelt noch Bodenproben vom Startlauf; das
        // erste `landing_at` steht noch. Ohne Untergrenze wandern sie in
        // die Spur der naechsten Landung.
        let mut stats = flug_mit_puffer(4.0, 140.0);
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        assert!(
            !stats.bahn_spur.is_empty(),
            "Gegenprobe: der Puffer traegt Bodenproben"
        );

        // Derselbe Ruecksetzer, den der Sampler beim Steigen ruft.
        clear_approach_stability_and_rollout(&mut stats);
        assert!(stats.bahn_spur.is_empty());

        // Naechster Takt vor dem zweiten Aufsetzen — derselbe Puffer.
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        assert!(
            stats.bahn_spur.is_empty(),
            "{} Punkte vom Startlauf in der Spur der naechsten Landung",
            stats.bahn_spur.len()
        );

        // Das zweite Aufsetzen erntet wieder: neue Proben, neues
        // `landing_at`, spaeter als der Ruecksetzer.
        let td2 = stats.snapshot_buffer.back().unwrap().at + chrono::Duration::seconds(60);
        stats.landing_at = Some(td2);
        let mut t = 0.0;
        while t <= 2.0 {
            stats.snapshot_buffer.push_back(probe(td2, t, 140.0));
            t += 0.02;
        }
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        assert!(
            stats.bahn_spur.len() >= 10,
            "das zweite Aufsetzen bekommt keine Spur ({} Punkte)",
            stats.bahn_spur.len()
        );
    }

    #[test]
    fn ein_live_punkt_holt_die_letzte_probe_nicht_zurueck() {
        // LPPT 02 (#1403): Der Puffer liefert nichts Neues mehr, der
        // Live-Takt schreibt weiter vorn. Mit `>=` am Merker kam die
        // letzte Probe bei jedem Takt wieder — Saegezahn in der Anzeige.
        let mut stats = flug_mit_puffer(4.0, 140.0);
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        for i in 1..=3 {
            let vorn = 600.0 + 20.0 * i as f64;
            spur_fortschreiben(&mut stats, 140.0, vorn, 0.0, 22.5);
            spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        }
        let rueck: Vec<_> = stats
            .bahn_spur
            .windows(2)
            .filter(|w| w[1].0 < w[0].0)
            .map(|w| (w[0].0, w[1].0))
            .collect();
        assert!(rueck.is_empty(), "Spur laeuft rueckwaerts: {rueck:?}");
    }

    #[test]
    fn zweimal_abschoepfen_verdoppelt_nichts() {
        let mut stats = flug_mit_puffer(4.0, 140.0);
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        let nach_einmal = stats.bahn_spur.len();
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        assert_eq!(
            stats.bahn_spur.len(),
            nach_einmal,
            "der zweite Durchlauf hat Punkte hinzugefuegt"
        );
    }

    #[test]
    fn der_merker_laeuft_bis_ans_ende_des_puffers() {
        let mut stats = flug_mit_puffer(4.0, 140.0);
        assert!(stats.bahn_spur_bis.is_none());
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        let bis = stats.bahn_spur_bis.expect("Merker gesetzt");
        let ms = (bis - stats.landing_at.unwrap()).num_milliseconds();
        assert!(
            ms > 3_000,
            "der Merker steht bei {ms} ms statt am Pufferende"
        );
    }

    #[test]
    fn nachschieben_setzt_die_spur_fort_statt_neu_anzufangen() {
        // Der Betriebsfall: Erst ein Stueck Puffer, Ticks spaeter der
        // Rest. Die Punkte muessen weiterlaufen, nicht zurueckspringen.
        let mut stats = flug_mit_puffer(2.0, 140.0);
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        let erste_runde = stats.bahn_spur.len();
        let letzter = stats.bahn_spur.last().expect("Punkte").0;

        let td = stats.landing_at.expect("Aufsetzzeit");
        let mut t = 2.02;
        while t <= 4.0 {
            stats.snapshot_buffer.push_back(probe(td, t, 140.0));
            t += 0.02;
        }
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);

        assert!(
            stats.bahn_spur.len() > erste_runde,
            "die zweite Haelfte kam nicht dazu"
        );
        let kleinster = abstaende(&stats).into_iter().fold(f64::MAX, f64::min);
        assert!(kleinster > 0.0, "die Punkte laufen rueckwaerts");
        assert!(
            stats.bahn_spur.last().expect("Punkte").0 > letzter,
            "die Spur ist nicht weitergelaufen"
        );
    }

    #[test]
    fn ohne_bahntreffer_passiert_nichts() {
        let mut stats = flug_mit_puffer(4.0, 140.0);
        stats.runway_match = None;
        spur_aus_puffer_abschoepfen(&mut stats, 22.5);
        assert!(stats.bahn_spur.is_empty());
    }
