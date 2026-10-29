    //! Ausfahrt ueber der Messschwelle (`AusfahrtKandidat`), Score-Version 22.
    use super::*;

    /// Bahn entlang des Aequators nach Osten, 4.000 m, 60 m breit.
    /// Ein Grad Laenge sind dort 111.320 m; rechts der Achse ist Sueden.
    const GRAD_M: f64 = 111_320.0;

    fn aequator_bahn(aufsetzkurs: f32) -> FlightStats {
        let mut stats = FlightStats::default();
        stats.runway_match = Some(runway::RunwayMatch {
            airport_ident: "TEST".to_string(),
            runway_ident: "09".to_string(),
            heading_true_deg: 90.0,
            length_ft: (4000.0 * 3.280_839_895) as f32,
            width_ft: 197.0,
            surface: "ASP".to_string(),
            threshold_lat: 0.0,
            threshold_lon: 0.0,
            end_lat: 0.0,
            end_lon: 4000.0 / GRAD_M,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 1300.0,
            side: "left".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        });
        stats.landing_heading_true_deg = Some(aufsetzkurs);
        stats
    }

    /// `quer` positiv = rechts der Achse (Konvention von `projiziere_auf_bahn`).
    fn tick(stats: &mut FlightStats, laengs: f64, quer: f64, gs: f32, kurs: f32) {
        let mut snap = SimSnapshot::default();
        snap.lat = -quer / GRAD_M;
        snap.lon = laengs / GRAD_M;
        snap.groundspeed_kt = gs;
        snap.heading_deg_true = kurs;
        snap.on_ground = true;
        bahndisziplin_tick(stats, &snap);
    }

    /// Geradeaus auf der Achse ausrollen, 140 → `bis_kt`, von 400 m an.
    fn ausrollen(stats: &mut FlightStats, bis_m: f64, bis_kt: f32) {
        let mut laengs = 400.0;
        while laengs < bis_m {
            let gs = 140.0 - (140.0 - bis_kt) * ((laengs - 400.0) / (bis_m - 400.0)) as f32;
            tick(stats, laengs, 1.0, gs, 90.0);
            laengs += 20.0;
        }
    }

    // Echte Spur: TUA651 (Hamadoun, B77W, VVTS 25R, 09.10.2026), Flugprotokoll
    // ab dem Aufsetzen, eine Probe je Position (lat, lon, Fahrt, Kurs).
    // Er drehte bei 64 kt nach links in die Schnellabrollbahn, ueberquerte
    // die Kante noch ueber sechzig Knoten — gewertet als 42,6 m Versatz,
    // „Rad neben der Bahn", Bahndisziplin 20 Punkte.
    const TUA651: &[(f64, f64, f32, f32)] = &[
            (10.82359918, 106.65973807, 148.8, 247.05),
            (10.82347454, 106.65941068, 147.9, 247.01),
            (10.82241908, 106.65670819, 140.3, 247.89),
            (10.82236121, 106.65656398, 139.7, 248.83),
            (10.82230708, 106.65642586, 139.1, 249.89),
            (10.82226641, 106.65631959, 138.6, 250.70),
            (10.82221006, 106.65616756, 137.9, 251.21),
            (10.82216032, 106.65602868, 137.1, 250.28),
            (10.82212274, 106.65592455, 136.5, 249.09),
            (10.82207102, 106.65578625, 135.8, 247.32),
            (10.82202188, 106.65565907, 135.1, 246.02),
            (10.82197169, 106.65553341, 134.3, 245.41),
            (10.82191452, 106.65539537, 133.4, 245.48),
            (10.82187128, 106.65529437, 132.8, 245.96),
            (10.82180210, 106.65513359, 131.7, 246.80),
            (10.82175792, 106.65502928, 131.1, 246.98),
            (10.82170822, 106.65491089, 130.4, 247.01),
            (10.82165921, 106.65479356, 129.7, 246.99),
            (10.82159888, 106.65464841, 128.7, 247.17),
            (10.82155852, 106.65455015, 128.1, 247.52),
            (10.82151013, 106.65442984, 127.2, 248.17),
            (10.82146376, 106.65431082, 126.5, 248.90),
            (10.82141667, 106.65418552, 125.7, 249.61),
            (10.82137205, 106.65406343, 124.9, 249.85),
            (10.82132953, 106.65394542, 124.2, 249.85),
            (10.82129664, 106.65385388, 123.6, 249.83),
            (10.82124410, 106.65370779, 122.6, 249.80),
            (10.82120902, 106.65361057, 122.0, 249.77),
            (10.82116277, 106.65348286, 121.2, 249.73),
            (10.82075527, 106.65236130, 113.4, 249.58),
            (10.82071714, 106.65225750, 112.7, 249.46),
            (10.82067871, 106.65215326, 111.9, 249.41),
            (10.82063340, 106.65203076, 111.1, 249.37),
            (10.82060338, 106.65194976, 110.5, 249.35),
            (10.82056303, 106.65184103, 109.6, 249.32),
            (10.82052841, 106.65174790, 109.0, 249.29),
            (10.82048279, 106.65162538, 108.1, 249.26),
            (10.82044816, 106.65153252, 107.3, 249.23),
            (10.82043835, 106.65150624, 107.1, 249.22),
            (10.82037816, 106.65134517, 106.0, 249.18),
            (10.82032854, 106.65121252, 105.1, 249.15),
            (10.82029937, 106.65113462, 104.5, 249.13),
            (10.82027588, 106.65107198, 103.9, 249.12),
            (10.82023211, 106.65095539, 102.5, 249.09),
            (10.82019719, 106.65086248, 102.2, 249.07),
            (10.82015319, 106.65074555, 101.4, 249.04),
            (10.82011001, 106.65063095, 100.5, 249.01),
            (10.82007910, 106.65054905, 99.9, 248.99),
            (10.82004108, 106.65044846, 99.1, 248.96),
            (10.82001159, 106.65037052, 98.4, 248.94),
            (10.81997443, 106.65027233, 97.7, 248.92),
            (10.81993724, 106.65017415, 96.8, 248.90),
            (10.81990493, 106.65008900, 96.1, 248.88),
            (10.81987527, 106.65001095, 95.4, 248.86),
            (10.81983312, 106.64990015, 94.5, 248.83),
            (10.81980749, 106.64983285, 93.8, 248.81),
            (10.81976789, 106.64972893, 93.0, 248.79),
            (10.81974131, 106.64965925, 92.3, 248.77),
            (10.81969901, 106.64954848, 91.4, 248.76),
            (10.81966683, 106.64946418, 90.7, 248.78),
            (10.81964113, 106.64939689, 90.1, 248.77),
            (10.81959795, 106.64928390, 89.0, 248.75),
            (10.81956829, 106.64920631, 88.4, 248.73),
            (10.81954046, 106.64913361, 87.7, 248.71),
            (10.81950678, 106.64904572, 86.9, 248.69),
            (10.81947335, 106.64895862, 86.1, 248.67),
            (10.81907798, 106.64794404, 74.1, 247.75),
            (10.81904368, 106.64785939, 72.7, 247.61),
            (10.81901887, 106.64779882, 71.7, 247.34),
            (10.81899611, 106.64774387, 70.7, 247.06),
            (10.81896709, 106.64767484, 69.4, 246.69),
            (10.81894456, 106.64762225, 68.4, 246.35),
            (10.81891070, 106.64754530, 67.0, 245.63),
            (10.81888231, 106.64748277, 66.1, 244.99),
            (10.81885768, 106.64743009, 65.5, 244.38),
            (10.81883277, 106.64737829, 64.8, 243.75),
            (10.81880311, 106.64731845, 64.1, 243.01),
            (10.81876713, 106.64724835, 63.3, 242.14),
            (10.81874252, 106.64720188, 62.9, 241.56),
            (10.81871056, 106.64714320, 62.9, 240.82),
            (10.81867734, 106.64708402, 63.1, 240.07),
            (10.81865007, 106.64703669, 63.3, 239.47),
            (10.81861678, 106.64698042, 63.6, 238.75),
            (10.81858251, 106.64692410, 64.0, 238.03),
            (10.81802941, 106.64618386, 65.5, 227.78),
            (10.81751029, 106.64567990, 54.9, 220.06),
            (10.81746873, 106.64564500, 52.6, 219.51),
            (10.81742882, 106.64561213, 50.3, 218.97),
            (10.81739792, 106.64558713, 48.4, 218.56),
            (10.81735361, 106.64555190, 45.7, 217.97),
            (10.81731681, 106.64552329, 43.3, 217.43),
            (10.81728937, 106.64550268, 41.6, 216.68),
            (10.81725009, 106.64547468, 38.9, 215.01),
            (10.81721857, 106.64545391, 36.7, 213.18),
            (10.81718820, 106.64543566, 34.6, 210.99),
            (10.81716432, 106.64542257, 33.3, 209.09),
            (10.81713377, 106.64540742, 32.7, 206.67),
            (10.81710270, 106.64539322, 32.3, 204.72),
            (10.81706468, 106.64537689, 32.1, 203.23),
            (10.81702934, 106.64536204, 31.7, 202.70),
            (10.81700624, 106.64535222, 31.5, 202.75),
            (10.81697068, 106.64533665, 31.2, 203.29),
            (10.81694209, 106.64532356, 31.1, 204.02),
            (10.81691414, 106.64531044, 31.0, 204.61),
            (10.81688095, 106.64529422, 30.8, 205.47),
            (10.81685325, 106.64528008, 30.7, 206.35),
            (10.81682018, 106.64526247, 30.6, 207.42),
            (10.81680045, 106.64525164, 30.6, 207.99),
            (10.81676838, 106.64523371, 30.5, 208.63),
            (10.81673873, 106.64521671, 30.5, 209.20),
            (10.81670821, 106.64519885, 30.5, 209.73),
            (10.81667632, 106.64517981, 30.5, 210.28),
            (10.81665764, 106.64516839, 30.4, 210.69),
            (10.81662562, 106.64514814, 30.4, 211.68),
            (10.81659942, 106.64513079, 30.3, 212.71),
            (10.81656948, 106.64510989, 30.3, 214.12),
            (10.81654992, 106.64509551, 30.3, 215.18),
            (10.81652094, 106.64507302, 30.2, 216.92),
            (10.81649506, 106.64505180, 30.2, 218.42),
            (10.81647165, 106.64503169, 30.2, 219.69),
            (10.81644463, 106.64500738, 30.2, 221.12),
            (10.81642227, 106.64498635, 30.2, 222.27),
            (10.81640050, 106.64496517, 30.2, 223.30),
            (10.81637843, 106.64494290, 30.2, 224.35),
            (10.81635585, 106.64491927, 30.2, 225.44),
            (10.81632854, 106.64488943, 30.2, 226.79),
            (10.81630453, 106.64486198, 30.2, 228.01),
            (10.81629098, 106.64484593, 30.2, 228.72),
            (10.81626682, 106.64481634, 30.2, 229.99),
            (10.81624272, 106.64478548, 30.2, 231.30),
            (10.81622777, 106.64476555, 30.2, 232.14),
            (10.81620609, 106.64473529, 30.1, 233.55),
            (10.81618899, 106.64471003, 30.1, 234.89),
            (10.81616933, 106.64467919, 30.1, 236.61),
            (10.81615232, 106.64465113, 30.1, 237.96),
            (10.81613719, 106.64462481, 30.0, 239.18),
            (10.81611985, 106.64459290, 30.0, 240.65),
            (10.81610878, 106.64457146, 30.0, 241.63),
            (10.81609277, 106.64453911, 30.0, 242.93),
            (10.81607913, 106.64451090, 30.0, 243.58),
            (10.81606598, 106.64448368, 30.0, 243.78),
            (10.81605292, 106.64445665, 30.0, 243.78),
            (10.81603991, 106.64442969, 30.0, 243.78),
            (10.81602408, 106.64439695, 30.0, 243.77),
            (10.81601078, 106.64436951, 30.0, 243.76),
            (10.81599761, 106.64434237, 30.0, 243.76),
            (10.81598059, 106.64430698, 30.0, 243.92),
            (10.81569909, 106.64361188, 13.9, 248.86),
    ];

    #[test]
    fn tua651_die_kurve_in_die_abrollbahn_ist_kein_abkommen() {
        let mut stats = FlightStats::default();
        // VVTS 25R, Navdaten (Zyklus 8); Breite aus der Szenerie (46 m).
        stats.runway_match = Some(runway::RunwayMatch {
            airport_ident: "VVTS".to_string(),
            runway_ident: "25R".to_string(),
            heading_true_deg: 248.975,
            length_ft: 10007.0,
            width_ft: 151.0,
            surface: "CONC".to_string(),
            threshold_lat: 10.82485556,
            threshold_lon: 106.663175,
            end_lat: 10.81501944,
            end_lon: 106.637125,
            centerline_distance_m: 4.2,
            centerline_distance_abs_ft: 13.8,
            touchdown_distance_from_threshold_ft: 1419.0,
            side: "right".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        });
        stats.landing_heading_true_deg = Some(247.015_78);
        stats.landing_groundspeed_kt = Some(148.8);
        let rm = stats.runway_match.clone().unwrap();
        let halbe = 151.0 * 0.3048 / 2.0;

        // Gegenprobe: Die Spur muss wirklich ueber sechzig Knoten ueber die
        // Kante gehen — sonst prueft dieser Test die alte Regel.
        let mut kante_ueber_60 = false;
        for &(lat, lon, gs, kurs) in TUA651 {
            let (_, quer) = runway::projiziere_auf_bahn(
                rm.threshold_lat,
                rm.threshold_lon,
                rm.end_lat,
                rm.end_lon,
                lat,
                lon,
            );
            kante_ueber_60 |= gs > 60.0 && quer.abs() > halbe;
            let mut snap = SimSnapshot::default();
            snap.lat = lat;
            snap.lon = lon;
            snap.groundspeed_kt = gs;
            snap.heading_deg_true = kurs;
            snap.on_ground = true;
            bahndisziplin_tick(&mut stats, &snap);
        }
        assert!(kante_ueber_60, "die Spur quert die Kante nicht ueber 60 kt");
        eprintln!(
            "TUA651 nachgerechnet: raeum {:?} m bei {:?} kt, Kurs {:?}, Versatz {:?} m, \
             Proben {}, Kante {:?} m bei {:?} kt, Seite {:?}",
            stats.bahn_raeum_laengs_m,
            stats.bahn_raeum_gs_kt,
            stats.bahn_raeum_kurs_diff,
            stats.bahn_max_querversatz_m,
            stats.bahn_proben,
            stats.bahn_kante_laengs_m,
            stats.bahn_kante_gs_kt,
            stats.bahn_raeum_seite,
        );

        let raeum = stats.bahn_raeum_laengs_m.expect("Raeumpunkt");
        let raeum_gs = stats.bahn_raeum_gs_kt.expect("Raeumfahrt");
        let max = stats.bahn_max_querversatz_m.expect("Versatz");
        assert!(raeum_gs > 60.0, "Raeumpunkt bei {raeum_gs:.0} kt — die alte Stelle");
        assert!((1800.0..2000.0).contains(&raeum), "Raeumpunkt bei {raeum:.0} m");
        assert_eq!(stats.bahn_fenster_zu_laengs_m, Some(raeum));
        assert!(max.abs() < halbe, "Versatz {max:.1} m liegt noch neben der Bahn");
        let kante = stats.bahn_kante_laengs_m.expect("Kantenuebertritt");
        assert!(kante > raeum && kante < 2000.0, "Kante bei {kante:.0} m");
        assert_eq!(stats.bahn_raeum_seite.as_deref(), Some("left"));
        assert!(stats.bahn_ausfahrt_kandidat.is_none());
    }

    #[test]
    fn schnelle_ausfahrt_endet_am_beginn_der_kurve() {
        let mut stats = aequator_bahn(90.0);
        ausrollen(&mut stats, 1800.0, 72.0);
        // Linkskurve ab 1.800 m bei 70 kt, bis 120 m neben die Achse.
        let mut quer = 1.0;
        let mut laengs = 1800.0;
        let mut kurs = 90.0_f32;
        let mut gs = 70.0_f32;
        while quer > -120.0 {
            kurs = (kurs - 3.0).max(60.0);
            let schritt = 10.0;
            laengs += schritt * (kurs as f64).to_radians().sin();
            quer -= schritt * (90.0 - kurs as f64).to_radians().sin();
            gs = (gs - 0.4).max(40.0);
            tick(&mut stats, laengs, quer, gs, kurs);
        }
        let raeum = stats.bahn_raeum_laengs_m.expect("Raeumpunkt");
        assert!(raeum < 1850.0, "Raeumpunkt bei {raeum:.0} m statt am Kurvenbeginn");
        assert!(stats.bahn_raeum_gs_kt.unwrap() > 60.0);
        let max = stats.bahn_max_querversatz_m.unwrap();
        assert!(max.abs() < 5.0, "Versatz {max:.1} m — die Kurve wurde mitgewertet");
        let kante = stats.bahn_kante_laengs_m.expect("Kante");
        assert!(kante > raeum, "Kante {kante:.0} m vor dem Raeumpunkt {raeum:.0} m");
        assert!(stats.bahn_kante_gs_kt.unwrap() > 60.0, "Kantenfahrt nicht vorgemerkt");
    }

    #[test]
    fn ausbrechen_mit_rueckkehr_bleibt_gewertet() {
        let mut stats = aequator_bahn(90.0);
        ausrollen(&mut stats, 1500.0, 80.0);
        // Bei 78 kt 20 Grad nach links, ueber die Kante, dann zurueck.
        for (laengs, quer, kurs) in [
            (1520.0, -5.0, 75.0),
            (1540.0, -15.0, 70.0),
            (1560.0, -28.0, 70.0),
            (1580.0, -36.0, 75.0),
            (1600.0, -38.0, 90.0),
            (1620.0, -30.0, 110.0),
            (1640.0, -15.0, 100.0),
            (1660.0, -5.0, 90.0),
        ] {
            tick(&mut stats, laengs, quer, 76.0, kurs);
        }
        assert!(stats.bahn_ausfahrt_kandidat.is_none(), "Kandidat nach Rueckkehr nicht verworfen");
        let max = stats.bahn_max_querversatz_m.unwrap();
        assert!(max < -35.0, "Versatz {max:.1} m — das Ausbrechen ging verloren");
        assert!(stats.bahn_raeum_laengs_m.is_none());
    }

    #[test]
    fn nach_dem_ende_der_spur_wird_nichts_mehr_bestaetigt() {
        let mut stats = aequator_bahn(90.0);
        ausrollen(&mut stats, 1800.0, 70.0);
        // Kurve bei 68 kt, ueber die Kante, dann Stillstand in der Abrollbahn.
        for (l, q, gs, k) in [(1820.0, 0.0, 68.0, 84.0), (1840.0, -8.0, 65.0, 70.0), (1860.0, -40.0, 50.0, 60.0), (1870.0, -60.0, 3.0, 60.0)] {
            tick(&mut stats, l, q, gs, k);
        }
        assert!(!stats.bahn_spur_laeuft, "Spur laeuft noch");
        let vorher = stats.bahn_max_querversatz_m;
        // Spaeter weiter bis weit neben die Bahn.
        tick(&mut stats, 1880.0, -130.0, 15.0, 60.0);
        assert_eq!(stats.bahn_max_querversatz_m, vorher, "nach dem Nachtrag umbewertet");
    }

    #[test]
    fn ein_sprung_ueber_die_kante_setzt_trotzdem_den_uebertritt() {
        let mut stats = aequator_bahn(90.0);
        ausrollen(&mut stats, 1800.0, 72.0);
        tick(&mut stats, 1810.0, 0.0, 70.0, 84.0);
        tick(&mut stats, 1820.0, -10.0, 68.0, 70.0);
        // Haenger: naechste Position schon 150 m neben der Achse.
        tick(&mut stats, 1900.0, -150.0, 62.0, 60.0);
        let raeum = stats.bahn_raeum_laengs_m.expect("bestaetigt");
        let kante = stats.bahn_kante_laengs_m.expect("Kante fehlt nach dem Sprung");
        assert!(kante > 1820.0 && kante < 1900.0, "Kante bei {kante:.0} m");
        assert!(kante >= raeum);
        assert_eq!(stats.bahn_kante_gs_kt, Some(62.0));
    }

    #[test]
    fn ein_offener_kandidat_ueberlebt_den_neustart() {
        let mut stats = aequator_bahn(90.0);
        ausrollen(&mut stats, 1800.0, 72.0);
        tick(&mut stats, 1810.0, 0.0, 70.0, 84.0);
        tick(&mut stats, 1820.0, -10.0, 68.0, 75.0);
        let k = stats.bahn_ausfahrt_kandidat.clone().expect("Kandidat");
        let json = serde_json::to_string(&PersistedFlightStats::snapshot_from(&stats)).unwrap();
        let mut n = FlightStats::default();
        serde_json::from_str::<PersistedFlightStats>(&json).unwrap().apply_to(&mut n);
        let w = n.bahn_ausfahrt_kandidat.expect("Kandidat nach dem Neustart weg");
        assert_eq!((w.laengs_m, w.kurs_diff, w.proben), (k.laengs_m, k.kurs_diff, k.proben));
        assert_eq!(w.max_querversatz_m, k.max_querversatz_m);
        assert_eq!(w.kante_laengs_m, k.kante_laengs_m);
    }

    #[test]
    fn ueber_90_kt_ist_keine_ausfahrt() {
        let mut stats = aequator_bahn(90.0);
        ausrollen(&mut stats, 1000.0, 100.0);
        let mut quer = 1.0;
        let mut laengs = 1000.0;
        while quer > -130.0 {
            laengs += 18.0;
            quer -= 7.0;
            tick(&mut stats, laengs, quer, 98.0, 70.0);
        }
        let max = stats.bahn_max_querversatz_m.unwrap();
        assert!(max < -100.0, "Versatz {max:.1} m — Abkommen bei 98 kt wurde entschuldigt");
        assert!(stats.bahn_raeum_laengs_m.is_none());
    }

    #[test]
    fn ausrichten_nach_schraegem_aufsetzen_verlegt_keine_ausfahrt() {
        // Schraeg aufgesetzt (Kurs 78), danach auf der Achse (Kurs 90):
        // dauerhaft zwoelf Grad neben dem Aufsetzkurs, aber auf der
        // Bahnrichtung. Bei 1.500 m ein Ausbrechen ueber die Kante (Kurs
        // 87, auf der Bahnrichtung) und zurueck, dann bei 70 kt eine
        // schnelle Ausfahrt nach rechts.
        //
        // Ohne die Bahnrichtungs-Bedingung haette das Ausrichten den
        // Kandidaten schon bei 90 kt angelegt, nie mehr verworfen, und die
        // Bestaetigung an der Ausfahrt haette das Ausbrechen geloescht.
        let mut stats = aequator_bahn(78.0);
        let mut laengs = 400.0;
        while laengs < 1500.0 {
            let gs = 140.0 - 60.0 * ((laengs - 400.0) / 1100.0) as f32;
            tick(&mut stats, laengs, 2.0, gs, 90.0);
            laengs += 20.0;
        }
        for (l, q) in [(1520.0, -10.0), (1540.0, -25.0), (1560.0, -34.0), (1580.0, -25.0), (1600.0, -10.0)] {
            tick(&mut stats, l, q, 78.0, 87.0);
        }
        assert!(stats.bahn_ausfahrt_kandidat.is_none(), "Ausrichten gilt als Kurve");
        let mut quer = -2.0;
        let mut laengs = 1700.0;
        let mut kurs = 90.0_f32;
        while quer < 120.0 {
            kurs = (kurs + 3.0).min(120.0);
            laengs += 10.0 * (kurs as f64).to_radians().sin();
            quer += 10.0 * (kurs as f64 - 90.0).to_radians().sin();
            tick(&mut stats, laengs, quer, 70.0, kurs);
        }
        let raeum = stats.bahn_raeum_laengs_m.expect("Raeumpunkt");
        assert!(raeum > 1700.0, "Raeumpunkt bei {raeum:.0} m — an die Stelle des Ausrichtens verlegt");
        let max = stats.bahn_max_querversatz_m.unwrap();
        assert!(max < -30.0, "Versatz {max:.1} m — das Ausbrechen ging verloren");
    }
