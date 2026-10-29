    use super::*;

    fn bericht(uebernommen: &[&str]) -> szenerie_bahn::UebernahmeBericht {
        szenerie_bahn::UebernahmeBericht {
            uebernommen: uebernommen.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn auskunft(bahnen: usize) -> sim_core::szenerie::SzenerieFlughafen {
        sim_core::szenerie::SzenerieFlughafen {
            staende: Vec::new(),
            icao: "DAAG".to_string(),
            bahnen: (0..bahnen)
                .map(|_| sim_core::szenerie::SzenerieBahn {
                    bezeichner: "23".to_string(),
                    kurs_grad: 232.7,
                    breite_m: 60.0,
                    laenge_m: 3500.0,
                    versetzte_schwelle_m: 0.0,
                    schwelle: (36.69, 3.21),
                    gegenende: (36.71, 3.24),
                    belag_code: 1,
                    kurs_bestaetigt_grad: None,
                    kurs_quelle: sim_core::szenerie::KursQuelle::Unbestaetigt,
                })
                .collect(),
            rollwege: Vec::new(),
            quelle: "msfs".to_string(),
        }
    }

    #[test]
    fn uebernommen_schlaegt_alles() {
        let mut s = FlightStats::new();
        s.szenerie_uebernahme = Some(bericht(&["23"]));
        s.szenerie_diagnose = Some("geliefert".to_string());
        assert_eq!(szenerie_status(&s), "uebernommen");
    }

    #[test]
    fn eine_auskunft_ohne_passende_bahn_heisst_kein_treffer() {
        // ⚠ Das ist etwas ganz anderes als "keine Auskunft": Der Weg
        // funktioniert, aber die Lagen passen nicht zusammen. Frueher
        // stand fuer beides "navdaten".
        let mut s = FlightStats::new();
        s.szenerie_auskunft = arc_some(auskunft(2));
        s.szenerie_uebernahme = Some(bericht(&[]));
        s.szenerie_diagnose = Some("geliefert".to_string());
        assert_eq!(
            szenerie_status(&s),
            "kein_treffer(bahnen=2, ohne_treffer=0, verworfen=0)"
        );
    }

    /// ⚠ QS-Befund 2: Geerntet wird GENAU EINER, ohne Rueckfall.
    ///
    /// Die erste Fassung erntete mit `find_map` ueber die Zielliste. Das
    /// nimmt das geplante Ziel, sobald dessen Antwort zuerst da ist —
    /// und legt sie am Flug ab. Die Bahnuebernahme weist sie am
    /// ICAO-Vergleich ab, die ROLLWEGE nicht: Die gehen ohne
    /// Platzpruefung in die Ausfahrten. Dann liegen die Ausfahrten
    /// Frankfurts an einer Landung in Malaga.
    #[test]
    fn geerntet_wird_das_ausweichziel_ohne_rueckfall() {
        assert_eq!(
            szenerie_ernteziel("LEZL", Some("LEMG"), None),
            Some("LEMG".to_string()),
            "bei einem Ausweichflug wurde auf das geplante Ziel zurueckgefallen"
        );
    }

    /// Ohne Ausweichziel das geplante — und Unsinn ergibt nichts.
    #[test]
    fn das_ernteziel_ist_sonst_das_geplante() {
        assert_eq!(
            szenerie_ernteziel("LEZL", None, None),
            Some("LEZL".to_string())
        );
        assert_eq!(
            szenerie_ernteziel(" lezl ", Some("  "), None),
            Some("LEZL".to_string()),
            "ein leeres Ausweichziel darf nicht das geplante verdecken"
        );
        assert_eq!(szenerie_ernteziel("XX", None, None), None);
    }

    /// Der eigene Quelltext — fuer die Waechter der Windows-Verdrahtung.
    ///
    /// ⚠ Die Ernte steht hinter `cfg(target_os = "windows")` und wird
    /// auf dem Mac nicht uebersetzt. Eine Gegenprobe, die dort ansetzt,
    /// bleibt gruen und sagt nichts. Diese Waechter lesen deshalb den
    /// Text.
    const QUELLE: &str = crate::quelle_gesamt::PRODUKTION;

    fn ohne_leerraum(s: &str) -> String {
        s.split_whitespace().collect()
    }

    /// Wie `ohne_leerraum`, aber OHNE Zeilenkommentare.
    ///
    /// ⚠ Wer eine ABWESENHEIT prueft, muss diese Sicht benutzen: Ein
    /// Kommentar der Form „hier stand frueher X" enthaelt X woertlich,
    /// und dann findet der Waechter sich selbst.
    fn nur_code(s: &str) -> String {
        ohne_leerraum(
            &s.lines()
                .filter(|z| !z.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// ⚠ QS-Befund 4, erste Runde v1.7.15: Der Produktionsblock haette
    /// entfernt werden koennen, ohne dass ein Test rot wird.
    ///
    /// Der Tick laesst sich nicht im Test fahren. Also wird die
    /// AUFRUFSTELLE bewacht — und zwar auf beides: dass sie
    /// `bahn_am_aufsetzpunkt_nachholen` ruft, und dass Pruefung und
    /// Aufruf NICHT auf zwei Sperren verteilt sind.
    #[test]
    fn der_tick_holt_die_bahnaufloesung_nach() {
        let a = nur_code(QUELLE);
        // ⚠ OHNE Simulator-Parameter. Seit der zweiten QS-Runde nimmt das
        // Nachholen keinen entgegen — es rechnet mit dem Simulator des
        // Aufsetz-Moments (`stats.aufsetz_simulator`). Stuende hier noch
        // die alte Form, waere der Waechter gruen zu machen, indem man
        // den Fehler wieder einbaut.
        let aufruf = format!("{}(&mutst,&flight)", "bahn_am_aufsetzpunkt_nachholen");
        assert!(
            a.contains(&aufruf),
            "der Tick holt die Bahnaufloesung nicht nach — dann erreicht \
             eine spaet eingetroffene Szenerie den Flug nie"
        );
        // ⚠ EIN Lock: Der Aufruf steht INNERHALB des `if let Ok(mut st)`,
        // nicht in einem zweiten Block dahinter. Zwischen zwei Sperren
        // kann der Sampler den Aufsetzpunkt loeschen.
        let stelle = a.find(&aufruf).expect("Aufruf fehlt");
        let davor = &a[..stelle];
        let lock = davor
            .rfind("ifletOk(mutst)=flight.stats.lock()")
            .expect("der Aufruf steht nicht unter einem stats-Lock");
        // ⚠ STRUKTUR statt Abstand: Der Aufruf liegt innerhalb des Blocks,
        // den die Sperre oeffnet — also mehr `{` als `}` dazwischen. Eine
        // Zeichenzahl war ein Mittel, keine Zusicherung (externe QS,
        // 02.09.2026, N11).
        let dazwischen = &a[lock..stelle];
        let auf = dazwischen.matches('{').count();
        let zu = dazwischen.matches('}').count();
        assert!(
            auf > zu,
            "der Aufruf liegt NICHT im Block der Sperre — dazwischen kann der \
             Sampler den Aufsetzpunkt loeschen"
        );
    }

    /// ⚠ QS-Befund 3, achte Runde: Die Simulator-Kennung wird ERSETZT.
    ///
    /// Sie wurde nur gesetzt, solange sie `None` war. Wechselte eine
    /// laufende Aufzeichnung von MSFS 2020 auf 2024 oder verband sich
    /// neu, trug der Flug weiter die alte Kennung — und ausgerechnet die
    /// Auswertung der Facility-Unterschiede haette eine falsche
    /// Diagnose.
    #[test]
    fn die_simulator_kennung_wird_ersetzt() {
        // ⚠ Die Nadeln ZUSAMMENBAUEN, nicht hinschreiben.
        //
        // Der Waechter liest die Datei, in der er selbst steht. Eine
        // wortwoertliche Nadel findet sich in seiner eigenen Behauptung
        // wieder — die Verbots-Pruefung schlaegt dann immer an, und die
        // Bestaetigungs-Pruefung ist immer gruen. Beides sagt nichts.
        let a = ohne_leerraum(QUELLE);
        // ⚠ Dieser Waechter schrieb bis zur elften Runde die FALSCHE
        // Bedingung fest: Er verlangte `if kennung.is_some()`. Damit
        // ueberlebte die Kennung der alten Verbindung jeden Wechsel, bei
        // dem noch keine neue gemeldet war — und der Waechter hielt
        // genau das fest. Zum zweiten Mal ein Waechter, der ein MITTEL
        // festschreibt statt einer Zusicherung.
        for verboten in [
            format!(
                "if{}.is_none(){{{}={};}}",
                "stats.sim_kennung", "stats.sim_kennung", "kennung"
            ),
            format!(
                "if{}.is_some(){{{}={};}}",
                "kennung", "stats.sim_kennung", "kennung"
            ),
        ] {
            assert!(
                !a.contains(&verboten),
                "die Kennung wird bedingt gesetzt — dann ueberlebt die alte \
                 Verbindung den Wechsel"
            );
        }
        assert!(
            a.contains(&format!("{}=schnapp.kennung;", "stats.sim_kennung")),
            "die Kennung kommt nicht unbedingt aus dem Schnappschuss"
        );
    }

    /// ⚠ Und Auskunft und Stand kommen aus EINEM Zugriff.
    #[test]
    fn auskunft_und_stand_werden_zusammen_geholt() {
        // ⚠ Wieder zusammengebaut — siehe `die_simulator_kennung_wird_ersetzt`.
        let a = ohne_leerraum(QUELLE);
        let zusammen = format!("msfs.{}_schnappschuss(z)", "szenerie");
        assert!(
            a.contains(&zusammen),
            "der Abholer nimmt keinen Schnappschuss — dann koennen \
             Generation, Auskunft und Diagnose aus verschiedenen \
             Kontexten stammen"
        );
        // ⚠ Und die getrennten Zugaenge duerfen nicht zurueckkommen.
        for verboten in [
            format!("msfs.{}_mit_stand(", "szenerie"),
            format!("msfs.{}_generation(", "szenerie"),
            format!("msfs.{}_diagnose_fuer(", "szenerie"),
            format!("msfs.{}_fuer(", "szenerie"),
        ] {
            assert!(
                !a.contains(&verboten),
                "ein getrennter Zugang ist zurueck — damit laesst sich der \
                 Riss neu verdrahten"
            );
        }
    }

    /// ⚠ QS-Befund 2, achte Runde: Ein Generationswechsel entwertet die
    /// Flugkopie — auch ohne Ersatzlieferung.
    #[test]
    fn ein_generationswechsel_entwertet_die_flugkopie() {
        assert!(!flugkopie_entwerten(3, 3), "gleiche Generation");
        assert!(
            flugkopie_entwerten(3, 4),
            "nach einem Verbindungswechsel bleibt die alte Kopie gueltig"
        );
        // ⚠ Auch rueckwaerts. Die Generation ist eine Kennung, kein Mass
        // — ein `>`-Vergleich waere eine Annahme ueber die Richtung.
        assert!(flugkopie_entwerten(4, 3));
    }

    /// ⚠ QS-Befund 2, siebte Runde: Die neueste Lieferung gewinnt auch
    /// AM FLUG, nicht nur im Buch.
    ///
    /// Die alte Regel war „nur wenn noch nichts mit diesem Platz da
    /// ist". Damit behielt der Flug die Auskunft aus Versuch 1, obwohl
    /// Versuch 2 im Buch laengst gewonnen hatte — und beim Aufsetzen
    /// wird genau diese Kopie fuer Bahngeometrie und Rollwege benutzt.
    #[test]
    fn eine_neuere_lieferung_ersetzt_die_flugkopie() {
        // Nichts da: nehmen.
        assert!(auskunft_ersetzen(None, "LEZL", 1));
        // Neuerer Stand desselben Platzes: ersetzen.
        assert!(
            auskunft_ersetzen(Some(("LEZL", 1)), "LEZL", 2),
            "die neuere Lieferung erreicht den Flug nicht"
        );
        // Gleicher Stand: nichts tun.
        assert!(!auskunft_ersetzen(Some(("LEZL", 2)), "LEZL", 2));
        // Aelterer Stand: nichts tun.
        assert!(
            !auskunft_ersetzen(Some(("LEZL", 5)), "LEZL", 3),
            "eine aeltere Lieferung hat die neuere ueberschrieben"
        );
    }

    /// Ein anderer Platz ersetzt immer — auch mit kleinerem Stand.
    ///
    /// ⚠ Beim Ausweichflug wechselt das Ernteziel. Die Kopie des alten
    /// Ziels muss weichen, unabhaengig von der Kennung.
    #[test]
    fn ein_anderer_platz_ersetzt_die_flugkopie_immer() {
        assert!(auskunft_ersetzen(Some(("LEZL", 9)), "LEMG", 1));
        // Und Gross/Kleinschreibung ist kein Unterschied.
        assert!(!auskunft_ersetzen(Some(("LEZL", 9)), "lezl", 9));
    }

    /// ⚠ Und der Verbindungswechsel: Buch geleert, frisch gefragt.
    ///
    /// Die Kennungen laufen ueber `zuruecksetzen` hinweg weiter — genau
    /// deshalb traegt die frische Lieferung einen hoeheren Stand und
    /// erreicht den Flug. Bliebe die alte Kopie stehen, waere die
    /// Begruendung fuer `verbindung_zuruecksetzen` hinfaellig.
    #[test]
    fn nach_einem_verbindungswechsel_kommt_die_frische_lieferung_an() {
        // Vor dem Wechsel: Stand 3. Danach vergibt das Buch weiter —
        // die naechste Lieferung traegt einen groesseren Stand.
        assert!(auskunft_ersetzen(Some(("EDDF", 3)), "EDDF", 4));
    }

    /// ⚠ QS-Befund 1 der dritten Runde (P0): Das Fenster oeffnet in den
    /// Landephasen — und je Phase genau einmal.
    #[test]
    fn das_versuchsfenster_oeffnet_in_jeder_landephase() {
        for phase in [
            FlightPhase::Descent,
            FlightPhase::Approach,
            FlightPhase::Final,
        ] {
            assert!(
                braucht_neues_versuchsfenster(phase, None),
                "{phase:?} oeffnet kein Fenster — dann ist der Vorrat am \
                 Gate verbraucht und im Anflug wird nie gefragt"
            );
            assert!(
                !braucht_neues_versuchsfenster(phase, Some(phase)),
                "{phase:?} oeffnet in derselben Phase ein zweites Fenster — \
                 dann fuellt sich der Vorrat staendig auf und die \
                 Obergrenze ist wirkungslos"
            );
        }
    }

    /// ⚠ **QS-Befund 1 der VIERTEN Runde (P0): die reale Folge.**
    ///
    /// Ein Fenster nur beim ersten `Descent` reicht nicht — zehn
    /// Versuche sind zehn Minuten, und ein langer Sinkflug, ein Holding
    /// oder ein Durchstarten dauert laenger. Der vorige Test prueft jede
    /// Phase ISOLIERT und sah genau das nicht.
    #[test]
    fn nach_einem_langen_sinkflug_oeffnet_der_anflug_neu() {
        // Descent hat sein Fenster bekommen …
        assert!(!braucht_neues_versuchsfenster(
            FlightPhase::Descent,
            Some(FlightPhase::Descent)
        ));
        // … und danach kommt Approach: neues Fenster.
        assert!(
            braucht_neues_versuchsfenster(FlightPhase::Approach, Some(FlightPhase::Descent)),
            "nach einem langen Sinkflug bekommt der Anflug nichts mehr"
        );
        assert!(
            braucht_neues_versuchsfenster(FlightPhase::Final, Some(FlightPhase::Approach)),
            "der Endanflug bekommt nichts mehr"
        );
    }

    /// ⚠ **QS-Befund 1, fuenfte Runde: `Approach → Holding → Approach`.**
    ///
    /// Wer die zuletzt GEOEFFNETE Landephase festhaelt, verliert diese
    /// Folge: Beim Verlassen nach `Holding` bliebe `Some(Approach)`
    /// stehen, und beim Wiedereintritt verglichen wir Approach mit
    /// Approach — kein Fenster. Ein laengeres Holding verbraucht die
    /// zehn Versuche, und der naechste Vorrat kaeme erst in `Final`,
    /// also unter 700 ft.
    ///
    /// Deshalb ist `vorher` die Phase des VORIGEN Durchlaufs, irgendeine.
    #[test]
    fn nach_einem_holding_oeffnet_der_anflug_wieder() {
        assert!(
            braucht_neues_versuchsfenster(FlightPhase::Approach, Some(FlightPhase::Holding)),
            "nach einem Holding bekommt der Anflug kein Fenster mehr"
        );
        // Und im Holding selbst wird keins geoeffnet.
        assert!(!braucht_neues_versuchsfenster(
            FlightPhase::Holding,
            Some(FlightPhase::Approach)
        ));
    }

    /// ⚠ Die Fortschreibung gibt IMMER die aktuelle Phase zurueck.
    ///
    /// Schriebe sie nur bei faelligem Fenster fort, waere das Feld ein
    /// Merker fuer geoeffnete Fenster — und `Approach → Holding →
    /// Approach` ginge wieder verloren.
    #[test]
    fn die_vorige_phase_wird_immer_fortgeschrieben() {
        for (phase, vorher) in [
            (FlightPhase::Holding, Some(FlightPhase::Approach)),
            (FlightPhase::Cruise, None),
            (FlightPhase::Approach, Some(FlightPhase::Approach)),
        ] {
            let (_, neu) = versuchsfenster_fortschreiben(phase, vorher);
            assert_eq!(
                neu,
                Some(phase),
                "{phase:?} nach {vorher:?} wurde nicht fortgeschrieben"
            );
        }
    }

    /// Und die ganze Folge ueber die Fortschreibung gefahren.
    #[test]
    fn die_holding_folge_ueber_die_fortschreibung() {
        let folge = [
            (FlightPhase::Approach, true),
            (FlightPhase::Holding, false),
            (FlightPhase::Approach, true),
        ];
        let mut vorher = None;
        for (phase, erwartet) in folge {
            let (faellig, neu) = versuchsfenster_fortschreiben(phase, vorher);
            assert_eq!(faellig, erwartet, "{phase:?} nach {vorher:?}");
            vorher = neu;
        }
    }

    /// Die ganze Folge am Stueck — so wie die Ernte sie sieht.
    ///
    /// ⚠ Die vorige Phase wird bei JEDEM Durchlauf fortgeschrieben, auch
    /// wenn kein Fenster faellig war. Sonst waere sie ein Merker fuer
    /// geoeffnete Fenster und der Befund waere zurueck.
    #[test]
    fn ein_holding_mitten_im_anflug_kostet_kein_fenster() {
        let folge = [
            (FlightPhase::Descent, true),
            (FlightPhase::Approach, true),
            (FlightPhase::Approach, false), // laeuft weiter
            (FlightPhase::Holding, false),  // raus aus dem Anflug
            (FlightPhase::Holding, false),
            (FlightPhase::Approach, true), // ⚠ wieder rein: neues Fenster
            (FlightPhase::Final, true),
        ];
        let mut vorher: Option<FlightPhase> = None;
        for (phase, erwartet) in folge {
            let faellig = braucht_neues_versuchsfenster(phase, vorher);
            assert_eq!(
                faellig, erwartet,
                "{phase:?} nach {vorher:?}: Fenster {faellig}, erwartet {erwartet}"
            );
            vorher = Some(phase);
        }
    }

    /// ⚠ Und der Doppelanflug, den der Bestand belegt.
    ///
    /// Der PTO705-Test faehrt `Final → Climb → Descent → Approach`. Mit
    /// einem einzigen Fenster je Flug haette der ZWEITE Anflug keinen
    /// Versuch mehr.
    #[test]
    fn ein_durchstarten_bekommt_wieder_ein_fenster() {
        let folge = [
            (FlightPhase::Descent, true),
            (FlightPhase::Approach, true),
            (FlightPhase::Final, true),
            (FlightPhase::Climb, false),  // Durchstarten
            (FlightPhase::Descent, true), // zweiter Anflug
            (FlightPhase::Approach, true),
        ];
        // ⚠ Die vorige Phase wird IMMER fortgeschrieben, nicht nur bei
        // einem faelligen Fenster.
        let mut vorher: Option<FlightPhase> = None;
        for (phase, erwartet) in folge {
            let faellig = braucht_neues_versuchsfenster(phase, vorher);
            assert_eq!(
                faellig, erwartet,
                "{phase:?} nach {vorher:?}: Fenster {faellig}, erwartet {erwartet}"
            );
            vorher = Some(phase);
        }
    }

    /// Und vorher nicht.
    ///
    /// ⚠ Am Boden oder im Reiseflug ist die Szenerie des Ziels nicht
    /// geladen. Ein Fenster dort waere genau der verbrauchte Vorrat,
    /// den der Befund beschreibt.
    #[test]
    fn vor_dem_sinkflug_gibt_es_kein_fenster() {
        for phase in [
            FlightPhase::Preflight,
            FlightPhase::Boarding,
            FlightPhase::Pushback,
            FlightPhase::TaxiOut,
            FlightPhase::TakeoffRoll,
            FlightPhase::Takeoff,
            FlightPhase::Climb,
            FlightPhase::Cruise,
        ] {
            assert!(
                !braucht_neues_versuchsfenster(phase, None),
                "{phase:?} oeffnet ein Fenster"
            );
        }
    }

    /// Ohne Ausweichziel ist es einfach der geplante Platz.
    #[test]
    fn ohne_ausweich_nur_das_geplante_ziel() {
        assert_eq!(szenerie_ziele("LEZL", None, None), vec!["LEZL".to_string()]);
    }

    /// ⚠ QS-Befund 2 der zweiten Runde: Der Platz, ueber dem die Raeder
    /// wirklich aufgesetzt haben, steht GANZ vorn.
    ///
    /// Er kommt spaet — aber er kommt auch dann, wenn die
    /// Anflugerkennung den Ausweichflug verpasst hat. Genau dann ist er
    /// die einzige Quelle, die den tatsaechlichen Platz kennt.
    #[test]
    fn der_tatsaechliche_landeplatz_steht_ganz_vorn() {
        assert_eq!(
            szenerie_ziele("LEZL", Some("LEMG"), Some("LEBL")),
            vec!["LEBL".to_string(), "LEMG".to_string(), "LEZL".to_string()]
        );
        assert_eq!(
            szenerie_ernteziel("LEZL", Some("LEMG"), Some("LEBL")).as_deref(),
            Some("LEBL")
        );
    }

    /// Und OHNE erkannten Anflug-Divert trotzdem der tatsaechliche.
    ///
    /// ⚠ Das ist der Fall, den der Befund beschreibt: Die
    /// Anflugerkennung hat nichts gemerkt, `divert_prefetch_icao` ist
    /// leer — und trotzdem ist beim Aufsetzen bekannt, wo gelandet
    /// wurde. Bliebe das Ernteziel hier das geplante, laege die
    /// nachgeforderte Antwort im Buch und erreichte den Flug nie.
    #[test]
    fn ein_erst_beim_aufsetzen_erkannter_divert_wird_geerntet() {
        assert_eq!(
            szenerie_ernteziel("LEZL", None, Some("LEBL")).as_deref(),
            Some("LEBL"),
            "der erst beim Aufsetzen bekannte Platz wurde nicht geerntet"
        );
    }

    /// Mit Ausweichziel stehen BEIDE drin — und das Ausweichziel vorn.
    ///
    /// ⚠ Beide, weil ein Ausweichentschluss spaet faellt und die
    /// Antwort Vorlauf braucht; das Ausweichziel vorn, weil dort
    /// gelandet wird.
    #[test]
    fn mit_ausweich_steht_das_ausweichziel_vorn() {
        assert_eq!(
            szenerie_ziele("LEZL", Some("LEMG"), None),
            vec!["LEMG".to_string(), "LEZL".to_string()]
        );
    }

    /// Gross/klein und Leerzeichen fallen weg, Doppelte auch.
    #[test]
    fn die_ziele_werden_geglaettet() {
        assert_eq!(
            szenerie_ziele(" lezl ", Some("LEZL"), Some("lezl")),
            vec!["LEZL".to_string()],
            "derselbe Platz mehrfach gefragt"
        );
        assert!(szenerie_ziele("XX", Some(""), None).is_empty());
    }

    /// Eine Auskunft ohne Vergleich darf keine Nullmessung erfinden.
    ///
    /// ⚠ Aus der QS (31.08.2026): Liegt eine nichtleere Auskunft vor,
    /// aber kein Uebernahmebericht — fehlende Navdaten, oder die
    /// Auskunft gehoert nach einem Divert zum geplanten statt zum
    /// tatsaechlichen Platz —, dann hat NIE ein Vergleich
    /// stattgefunden. Ein `ohne_treffer=0, verworfen=0` waere erfunden
    /// und wuerde genau die Ursache verdecken, die die Meldung zeigen
    /// soll.
    #[test]
    fn eine_auskunft_ohne_vergleich_erfindet_keine_nullmessung() {
        let mut s = FlightStats::new();
        s.szenerie_auskunft = arc_some(auskunft(40));
        // kein Uebernahmebericht
        assert_eq!(
            szenerie_status(&s),
            "auskunft_ohne_vergleich(platz=DAAG, bahnen=40, diagnose=?)"
        );
    }

    /// Der Fall vom 01.09.2026, als Zeile.
    ///
    /// ⚠ Ohne den Platz in der Meldung kostete die Frage "welcher
    /// Flughafen war das eigentlich?" eine halbe Untersuchung: Es liess
    /// sich nur ueber die Bahnzahl erschliessen (LEZL hat eine, EDDF
    /// vier). Mit Platz und Diagnose beantwortet die Zeile sich selbst.
    #[test]
    fn die_meldung_nennt_den_platz_der_auskunft() {
        let mut s = FlightStats::new();
        let mut a = auskunft(4);
        a.icao = "EDDF".to_string();
        s.szenerie_auskunft = arc_some(a);
        s.szenerie_diagnose = Some("geliefert(EDDF, bahnen=4, rollwege=61)".to_string());
        // kein Uebernahmebericht — in Sevilla gelandet, Frankfurt im Fach
        assert_eq!(
            szenerie_status(&s),
            "auskunft_ohne_vergleich(platz=EDDF, bahnen=4, \
             diagnose=geliefert(EDDF, bahnen=4, rollwege=61))"
        );
    }

    /// Die Meldung muss sagen, WORAN es lag.
    ///
    /// ⚠ Bis v1.7.12 hiessen beide Ursachen nur "kein_treffer" — drei
    /// gescheiterte MSFS-Fluege am 30./31.08.2026 (EDDF 25L, YBWW 12,
    /// YBCG 14) meldeten alle dasselbe, und man konnte nicht sagen, ob
    /// die Kennungen oder die Koordinaten nicht stimmten. Das sind
    /// verschiedene Fehler mit verschiedenen Fixes.
    #[test]
    fn kein_treffer_nennt_die_ursache() {
        // Bezeichner passte nirgends: die Kennungen stimmen nicht.
        let mut s = FlightStats::new();
        s.szenerie_auskunft = arc_some(auskunft(40));
        let mut b = bericht(&[]);
        b.ohne_treffer = vec!["25L".into(), "07R".into()];
        s.szenerie_uebernahme = Some(b);
        assert_eq!(
            szenerie_status(&s),
            "kein_treffer(bahnen=40, ohne_treffer=2, verworfen=0)"
        );

        // Bezeichner passte, Lage nicht: die Koordinaten stimmen nicht.
        let mut s = FlightStats::new();
        s.szenerie_auskunft = arc_some(auskunft(40));
        let mut b = bericht(&[]);
        b.verworfen = vec!["25L".into()];
        s.szenerie_uebernahme = Some(b);
        assert_eq!(
            szenerie_status(&s),
            "kein_treffer(bahnen=40, ohne_treffer=0, verworfen=1)"
        );
    }

    #[test]
    fn die_drei_stummen_faelle_bleiben_unterscheidbar() {
        // Genau diese drei fielen bis v1.7.9 alle unter "navdaten" —
        // deshalb war am 29.08.2026 nicht zu sagen, woran die
        // MSFS-Haelfte scheiterte.
        for fall in ["nicht_angefordert", "abgelehnt", "keine_antwort"] {
            let mut s = FlightStats::new();
            s.szenerie_diagnose = Some(fall.to_string());
            assert_eq!(
                szenerie_status(&s),
                fall,
                "der Fall {fall} wird nicht mehr eigenstaendig gemeldet"
            );
        }
    }

    /// ⚠ Der Befund vom 01.09.2026 (YSBK): Die Szenerie kam NACH dem
    /// Aufsetzen — nachgeholt wird je LIEFERUNG, nicht je Flug.
    #[test]
    fn nachgeholt_wird_bei_einer_neueren_lieferung() {
        // Aufsetzpunkt da, Auskunft da, noch nie zugeordnet (Stand 0).
        assert!(
            bahnaufloesung_nachholen(true, 7, 0),
            "die spaet eingetroffene Szenerie erreicht die Bahnaufloesung nie"
        );
        // ⚠ Und auch dann, wenn schon EINMAL zugeordnet wurde — solange
        // die Lieferung neuer ist. Der erste Entwurf sperrte hier auf
        // „es gibt schon einen Bericht" und liess Auskunft B mit Bericht
        // A nebeneinanderliegen.
        assert!(
            bahnaufloesung_nachholen(true, 9, 7),
            "eine NEUERE Lieferung wird nicht mehr zugeordnet"
        );
    }

    /// Und die Faelle, in denen NICHT nachgeholt wird.
    #[test]
    fn ohne_aufsetzpunkt_auskunft_oder_neueren_stand_wird_nicht_nachgeholt() {
        // Keine Auskunft: Ein zweiter Lauf aendert nichts.
        assert!(!bahnaufloesung_nachholen(false, 7, 0));
        // Derselbe Stand: schon zugeordnet.
        assert!(
            !bahnaufloesung_nachholen(true, 7, 7),
            "es wird erneut zugeordnet, obwohl die Lieferung dieselbe ist — \
             das laeuft dann bei JEDEM Durchlauf"
        );
        // Aelterer Stand: darf nicht zurueckfallen.
        assert!(!bahnaufloesung_nachholen(true, 5, 7));
    }

    /// ⚠ Der Dreier muss VOLLSTAENDIG sein.
    ///
    /// Ein Teil-Dreier hiesse, mit einem geratenen Wert zu rechnen.
    #[test]
    fn ein_unvollstaendiger_aufsetzpunkt_zaehlt_nicht() {
        let mut s = FlightStats::new();
        assert_eq!(gespeicherter_aufsetzpunkt(&s), None);
        s.landing_lat = Some(51.0);
        s.landing_lon = Some(12.0);
        assert_eq!(
            gespeicherter_aufsetzpunkt(&s),
            None,
            "ohne Kurs gilt der Aufsetzpunkt als vollstaendig — dann wird \
             mit einem geratenen Kurs zugeordnet"
        );
        s.landing_heading_true_deg = Some(265.7);
        assert_eq!(gespeicherter_aufsetzpunkt(&s), Some((51.0, 12.0, 265.7)));
    }

    /// ⚠ QS-Befund 1, zwoelfte Runde: Wechselt das Ernteziel und hat
    /// der neue Platz NOCH KEINE Lieferung, muss die alte Kopie weg.
    ///
    /// Sonst gehen ihre Rollwege ohne ICAO-Pruefung in die Ausfahrten —
    /// und ueber den Cache entsteht genau der Rueckfall auf das geplante
    /// Ziel, den `szenerie_ernteziel` verbietet.
    ///
    /// ⚠ Der Test `ein_anderer_platz_ersetzt_die_flugkopie_immer` hielt
    /// die entscheidende Achse konstant: Er prueft nur den Fall, dass
    /// der neue Platz BEREITS geliefert hat.
    #[test]
    fn ein_divert_ohne_lieferung_wirft_die_alte_kopie_weg() {
        let mut buch = sim_core::szenerie::Auftragsbuch::neu();
        buch.wunsch("LEZL");
        let (_, id) = buch.naechsten_stellen(0).expect("Auftrag");
        buch.geliefert_zu_kennung(
            id,
            sim_core::szenerie::SzenerieFlughafen {
                staende: Vec::new(),
                icao: "LEZL".to_string(),
                bahnen: Vec::new(),
                rollwege: vec![sim_core::szenerie::SzenerieRollweg {
                    name: "A".to_string(),
                    punkte: Vec::new(),
                }],
                quelle: "msfs".to_string(),
            },
        )
        .expect("Lieferung");

        let mut s = FlightStats::new();
        schnappschuss_uebernehmen(&mut s, Some("LEZL"), "LEZL", buch.schnappschuss("LEZL"));
        assert_eq!(
            s.szenerie_auskunft.as_ref().map(|a| a.icao.as_str()),
            Some("LEZL"),
            "die Kopie ist gar nicht erst angekommen"
        );

        // ⚠ Ausweichflug: Ziel ist jetzt LEMG — und LEMG hat NICHTS
        // geliefert. Die Generation ist unveraendert.
        buch.wunsch("LEMG");
        schnappschuss_uebernehmen(&mut s, Some("LEMG"), "LEMG", buch.schnappschuss("LEMG"));

        assert!(
            s.szenerie_auskunft.is_none(),
            "die Szenerie von {:?} liegt weiter am Flug — ihre Rollwege \
             landen in den Ausfahrten von LEMG",
            s.szenerie_auskunft.as_ref().map(|a| a.icao.clone())
        );
        assert_eq!(s.szenerie_auskunft_stand, 0);
    }

    /// Und ohne Ernteziel bleibt erst recht nichts liegen.
    #[test]
    fn ohne_ernteziel_bleibt_keine_kopie_liegen() {
        let mut s = FlightStats::new();
        s.szenerie_auskunft = arc_some(auskunft(2));
        s.szenerie_auskunft_stand = 7;
        let buch = sim_core::szenerie::Auftragsbuch::neu();
        schnappschuss_uebernehmen(&mut s, None, "LEZL", buch.schnappschuss("LEZL"));
        assert!(s.szenerie_auskunft.is_none());
    }

    /// ⚠ QS-Befund 2, zwoelfte Runde: Der Generations-Waechter fand nur
    /// SICH SELBST.
    ///
    /// Er suchte `flugkopie_entwerten(stats.szenerie_auskunft_generation,
    /// buch_generation)` — eine Zeichenkette, die es nach dem Umbau nur
    /// noch in seinen eigenen Ausdruecken gab. Ein Rueckbau der
    /// Entwertung waere nicht aufgefallen.
    ///
    /// Jetzt ein Verhaltenstest an der reinen Funktion.
    #[test]
    fn ein_generationswechsel_ohne_lieferung_leert_die_kopie() {
        let mut s = FlightStats::new();
        s.szenerie_auskunft = arc_some(auskunft(2));
        s.szenerie_auskunft_stand = 7;
        // Die Zweitquelle gehoert MIT auf den Pruefstand.
        //
        // Sie haelt die Auskunft des geplanten Ziels ueber einen
        // ZIELwechsel hinweg — aber ein VERBINDUNGSwechsel entwertet
        // auch sie: Ein Sim-Neustart kann fuer denselben Platz andere
        // Rollwege mitbringen, und die ICAO-Pruefung faengt das nicht.
        // Ohne diese beiden Zeilen prueft die Zusicherung unten ein
        // Feld, das nie gesetzt wurde — sie war trivial gruen, und wer
        // die Entwertung ausbaute, bekam 1413 gruene Tests (externe
        // Nachpruefung, 21.09.2026).
        s.ziel_szenerie_auskunft = arc_some(auskunft(2));
        s.ziel_szenerie_auskunft_stand = 7;
        // ⚠ Die Flugkopie stammt aus Generation 0 — die des Buches ist
        // nach dem Wechsel groesser. Mein erster Entwurf setzte hier 1
        // und das Buch stand ebenfalls auf 1: Der Test prueste dann
        // gar keinen Wechsel und scheiterte zu Recht.
        s.szenerie_auskunft_generation = 0;

        // Neue Verbindung: naechste Generation, KEINE Lieferung.
        let mut buch = sim_core::szenerie::Auftragsbuch::neu();
        buch.verbindung_zuruecksetzen();
        let schnapp = buch.schnappschuss("DAAG");
        assert!(schnapp.generation > 0);
        assert!(schnapp.auskunft.is_none());

        schnappschuss_uebernehmen(&mut s, Some("DAAG"), "DAAG", schnapp.clone());

        assert!(
            s.szenerie_auskunft.is_none(),
            "die Kopie der alten Verbindung ueberlebt"
        );
        assert_eq!(s.szenerie_auskunft_stand, 0);
        assert!(
            s.ziel_szenerie_auskunft.is_none(),
            "die Zweitquelle ueberlebt den Verbindungswechsel — damit \
             koennte die Szenerie der ALTEN Verbindung doch noch in die \
             Ausfahrten geraten",
        );
        assert_eq!(s.ziel_szenerie_auskunft_stand, 0);
        assert_eq!(
            s.szenerie_auskunft_generation, schnapp.generation,
            "die Generation wird nicht nachgezogen — dann entwertet jeder \
             Durchlauf erneut"
        );
    }

    /// ⚠ QS-Befund 1, elfte Runde: Die Kennung wird durch `None`
    /// ersetzt.
    ///
    /// Beim Verbindungswechsel leert der Adapter Buch UND Kennung. Kaeme
    /// am Flug nur ein `Some` an, behauptete er weiter eine
    /// Simulatorfassung, die gar nicht mehr verbunden ist.
    #[test]
    fn ein_verbindungswechsel_loescht_die_kennung_am_flug() {
        let mut s = FlightStats::new();
        s.sim_kennung = Some("KittyHawk 11.0".to_string());

        let mut buch = sim_core::szenerie::Auftragsbuch::neu();
        // ⚠ Die Kennung MUSS erst im Buch stehen, sonst prueft der Test
        // nichts: Ein frisches Buch hat ohnehin keine, und der Schnitt
        // koennte ersatzlos entfallen, ohne dass es auffaellt. Genau so
        // stand er bis zur zwoelften Runde da — die Gegenprobe
        // „Kennung ueberlebt den Verbindungsschnitt" blieb gruen.
        buch.kennung_setzen(Some("KittyHawk 11.0".to_string()));
        assert_eq!(
            buch.schnappschuss("LEZL").kennung.as_deref(),
            Some("KittyHawk 11.0"),
            "die Kennung kommt gar nicht erst im Schnappschuss an"
        );

        buch.verbindung_zuruecksetzen();
        assert_eq!(
            buch.schnappschuss("LEZL").kennung,
            None,
            "der Verbindungsschnitt laesst die Kennung im Buch stehen"
        );

        schnappschuss_uebernehmen(&mut s, Some("LEZL"), "LEZL", buch.schnappschuss("LEZL"));
        assert_eq!(
            s.sim_kennung, None,
            "die Kennung der alten Verbindung ueberlebt den Wechsel"
        );
    }

    /// ⚠ QS-Befund 2, elfte Runde: die ECHTE Kette, nicht zwei Haelften.
    ///
    /// Buch → Schnappschuss → Flugzustand → Meldung.
    ///
    /// Der vorige Test baute die Diagnose aus dem Buch (LKTB, 243
    /// Rollwege) und setzte danach eine SEPARAT erzeugte Auskunft
    /// daneben (DAAG, 0 Rollwege). Er prueste damit Buchformatierung und
    /// Durchreichung getrennt; eine verlorene, veraltete oder falsch
    /// zugeordnete Diagnose waere gruen geblieben.
    #[test]
    fn eine_lieferung_ohne_bahnen_ist_ein_eigener_fall() {
        let mut buch = sim_core::szenerie::Auftragsbuch::neu();
        buch.wunsch("LKTB");
        let (_, id) = buch.naechsten_stellen(0).expect("Auftrag");
        buch.geliefert_zu_kennung(
            id,
            sim_core::szenerie::SzenerieFlughafen {
                staende: Vec::new(),
                icao: "LKTB".to_string(),
                bahnen: Vec::new(),
                rollwege: (0..243)
                    .map(|_| sim_core::szenerie::SzenerieRollweg {
                        name: "A".to_string(),
                        punkte: Vec::new(),
                    })
                    .collect(),
                quelle: "msfs".to_string(),
            },
        )
        .expect("Lieferung");

        let mut s = FlightStats::new();
        schnappschuss_uebernehmen(&mut s, Some("LKTB"), "LKTB", buch.schnappschuss("LKTB"));

        let gemeldet = szenerie_status(&s);
        assert!(
            gemeldet.starts_with("ohne_bahnen("),
            "die Meldung lautet: {gemeldet}"
        );
        assert!(gemeldet.contains("LKTB"), "der Platz fehlt: {gemeldet}");
        assert!(
            gemeldet.contains("243"),
            "die Rollwegzahl fehlt: {gemeldet}"
        );
        // ⚠ Und die Auskunft ist WIRKLICH am Flug angekommen — nicht nur
        // die Zeichenkette.
        assert_eq!(
            s.szenerie_auskunft.as_ref().map(|a| a.icao.as_str()),
            Some("LKTB")
        );
    }

    #[test]
    fn bei_x_plane_wird_die_msfs_diagnose_nicht_gestempelt() {
        // ⚠ Der Abholer haengt an `cfg(target_os = "windows")` — darunter
        // faellt auch ein X-Plane-Pilot auf einem Windows-Rechner. Ohne
        // den Riegel bekaeme dessen Flug die Diagnose des untaetigen
        // MSFS-Adapters, und X-Plane-Fluege zaehlten in der Auswertung
        // als MSFS-Fehlschlaege.
        let q = ohne_leerraum_lib();
        let start = q
            .find("fnszenerie_auskunft_uebernehmen")
            .expect("Abholer nicht gefunden");
        // ⚠ Nach dem ATTRIBUT suchen, nicht nach dem blossen Text: Der
        // Erklaerkommentar darueber nennt `cfg(target_os = "windows")`
        // ebenfalls, und ohne Leerraum trifft die Suche dann IHN. Das
        // Fenster endete vor dem Riegel, und der Waechter war rot,
        // obwohl der Riegel da war. Beim ersten Lauf genau passiert.
        let bis_cfg = q[start..]
            .find("#[cfg(target_os=\"windows\")]")
            .expect("cfg-Attribut nicht gefunden");
        let nadel = format!(
            "{}{}",
            "quelle_fuer(simulator)!=szenerie_bahn::", "Quelle::MsfsFacility"
        );
        assert!(
            q[start..start + bis_cfg].contains(&nadel),
            "der Abholer prueft den Simulator nicht, BEVOR er stempelt"
        );
    }

    fn ohne_leerraum_lib() -> String {
        crate::quelle_gesamt::PRODUKTION
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect()
    }

    #[test]
    fn ohne_msfs_adapter_bleibt_es_verstaendlich() {
        // X-Plane liest die Datei direkt; dort gibt es keine Diagnose
        // vom Simulator. Trotzdem darf kein leeres Feld herauskommen.
        let s = FlightStats::new();
        assert_eq!(szenerie_status(&s), "keine_szenerie");

        let mut s = FlightStats::new();
        s.szenerie_uebernahme = Some(bericht(&[]));
        assert_eq!(szenerie_status(&s), "kein_treffer");
    }

    #[test]
    fn der_status_steht_an_beiden_payload_stellen() {
        // ⚠ Es gibt zwei ABLEITUNGEN des Flug-Payloads: die gemeinsame
        // Bahn-Herkunft (die beide Touchdown-Ereignisse tragen) und den
        // PIREP. Steht das Feld nur an einer, fehlt genau die Haelfte der
        // Faelle — und das faellt erst auf, wenn man sie sucht.
        //
        // ⚠ Die Nadel endet vor dem `&`: `bahn_herkunft` nimmt `stats`,
        // die PIREP-Stelle `&stats`. Ein Waechter, der auf `&` besteht,
        // haette nach dem Zusammenlegen genau eine Stelle gefunden und
        // waere rot geworden, ohne dass etwas fehlt.
        let q: String = crate::quelle_gesamt::PRODUKTION
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let nadel = format!("{}{}", "bahn_szenerie_status:Some(", "szenerie_status(");
        assert_eq!(
            q.matches(&nadel).count(),
            2,
            "der Szenerie-Status steht nicht an beiden Payload-Stellen"
        );
    }
