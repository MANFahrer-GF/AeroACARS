    use super::*;

    /// Die Quelle von `try_resume_flight` — für die Wächter, die belegen,
    /// dass die alte Dauer-Grenze wirklich weg ist und nicht nur umbenannt.
    const LIB_QUELLE: &str = include_str!("../lib.rs");

    fn beispiel_flug() -> PersistedFlight {
        PersistedFlight {
            pirep_id: "p8Q8YeaXqvKgyO6Z".to_string(),
            plan_strecke_nm: None,
            bid_id: 1,
            flight_id: String::new(),
            bid_callsign: None,
            pilot_callsign: None,
            started_at: Utc::now(),
            zuletzt_geschrieben: None,
            airline_icao: "THY".to_string(),
            airline_logo_url: None,
            planned_registration: "TC-LKC".to_string(),
            flight_number: "77".to_string(),
            dpt_airport: "LTFM".to_string(),
            arr_airport: "KMIA".to_string(),
            fares: vec![],
            stats: PersistedFlightStats::default(),
            owner_identity: Some("fingerprint-thy77-pilot".to_string()),
        }
    }

    fn ohne_leerraum(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    /// Alle Nutzlaststellen schreiben dieselbe Algorithmus-Fassung.
    ///
    /// ⚠ Eine hart geschriebene Zahl an einer von drei Stellen faellt
    /// nicht auf: Der Flug wird gespeichert, die Zahl ist plausibel, und
    /// erst eine Auswertung ueber den Bestand stolpert darueber — Monate
    /// spaeter. Die Nadel wird mit `format!` gebaut, sonst faende der
    /// Waechter sich selbst.
    #[test]
    fn die_algorithmusversion_steht_an_allen_stellen() {
        let quelle = ohne_leerraum(LIB_QUELLE);
        let gute = ohne_leerraum(&format!(
            "score_algorithm_version{}Some(SCORE_ALGORITHMUS_VERSION)",
            ':'
        ));
        assert_eq!(
            quelle.matches(&gute).count(),
            3,
            "nicht alle drei Nutzlaststellen benutzen die Konstante"
        );
        // Und keine davon schreibt eine eigene Zahl — egal welche.
        //
        // ⚠ Vorher stand hier die feste 9. Ein Waechter, der nur EINEN
        // falschen Wert kennt, faengt den naechsten nicht: Beim Sprung
        // auf 10 waere eine hart geschriebene 9 aufgefallen, eine hart
        // geschriebene 10 nicht.
        let praefix = ohne_leerraum(&format!("score_algorithm_version{}Some(", ':'));
        for (i, _) in quelle.match_indices(&praefix) {
            let rest = &quelle[i + praefix.len()..];
            let naechstes = rest.chars().next().unwrap_or(' ');
            assert!(
                !naechstes.is_ascii_digit(),
                "eine Nutzlaststelle schreibt die Fassung als feste Zahl ({naechstes}…) \
                 statt ueber SCORE_ALGORITHMUS_VERSION"
            );
        }
    }

    /// Die Bahngeometrie loest IMMER gegen das eingereichte Ziel auf.
    ///
    /// ⚠ Bei einem Divert ist das geplante Ziel der falsche Flughafen:
    /// Die Bahn, auf der gelandet wurde, liegt woanders. Ein Aufruf mit
    /// `flight.arr_airport` faellt nicht auf — er liefert plausible
    /// Zahlen von der falschen Bahn. Drei der vier Aufrufe waren immer
    /// richtig; der vierte (die Schattenbewertung in `build_pirep_notes`)
    /// wurde beim Herausloesen der Funktion uebersehen und erst von der
    /// zweiten externen Runde gefunden.
    ///
    /// Die Nadel wird mit `format!` gebaut, sonst faende der Waechter
    /// sich selbst.
    #[test]
    fn die_bahngeometrie_loest_nie_gegen_das_geplante_ziel_auf() {
        let quelle = ohne_leerraum(LIB_QUELLE);
        let falsch = ohne_leerraum(&format!(
            "fill_v2_rollout_fields(&mut crate_input, stats, &flight.arr_{})",
            "airport"
        ));
        assert!(
            !quelle.contains(&falsch),
            "die Schattenbewertung loest die Bahngeometrie wieder gegen \
             das GEPLANTE Ziel auf — bei einem Divert ist das die falsche \
             Bahn"
        );
    }

    /// Jedes Feld aus `BahnFelder` muss BEIDE Abnehmer erreichen.
    ///
    /// ⚠ `BahnFelder` sagt von sich, es sei die eine Uebersetzung —
    /// „wer ein Feld ergaenzt, ergaenzt es hier und ist an allen Stellen
    /// fertig". Das stimmt nur, wenn beide Kopierstellen mitziehen:
    ///
    ///   * `BahnFelder::wire()`      → die MQTT-Nutzlast (Webapp)
    ///   * `build_landing_record()`  → die lokale Aufzeichnung (Client)
    ///
    /// Genau hier ist `spur_nullpunkt_versatz_m` in v1.7.12 verloren
    /// gegangen: Es stand in der Nutzlast, aber nicht im Record. Der
    /// Client las `null` und zeichnete die Rollspur falsch — lautlos,
    /// denn die Marken sehen plausibel aus, sie liegen nur falsch. Die
    /// Frontend-Tests fanden es nicht, weil sie den Versatz direkt in
    /// die Projektion setzen und den Weg dorthin ueberspringen.
    ///
    /// Die Nadeln werden mit `format!` gebaut, sonst faende der
    /// Waechter sich selbst.
    #[test]
    fn jedes_bahnfeld_erreicht_nutzlast_und_aufzeichnung() {
        let quelle = LIB_QUELLE;
        // Die Felder der Struktur einlesen — nicht abschreiben.
        let start = quelle
            .find(&format!("struct BahnFelder {}", '{'))
            .expect("struct BahnFelder nicht gefunden");
        let struktur = &quelle[start..];
        let ende = struktur
            .find("\n}")
            .expect("Ende der Struktur nicht gefunden");
        let felder: Vec<&str> = struktur[..ende]
            .lines()
            .filter_map(|z| {
                let z = z.trim();
                if z.starts_with("//") || z.starts_with('/') {
                    return None;
                }
                let (name, _) = z.split_once(':')?;
                let name = name.trim();
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                    .then_some(name)
            })
            .filter(|n| !n.is_empty())
            .collect();

        // Riegel gegen einen Waechter, der nichts prueft.
        assert!(
            felder.len() > 8,
            "nur {} Felder erkannt — das Auslesen der Struktur ist kaputt",
            felder.len()
        );
        assert!(felder.contains(&"clearance_point_m"));

        // ⚠ Die RUEMPFE lesen, nicht ein festes Muster suchen: Zwei
        // Felder gehen ueber eine Bedingung in die Nutzlast
        // (`if self.x.is_empty() { None } else { … }`), und ein starres
        // `x: self.x` haette sie faelschlich als fehlend gemeldet.
        // Klammern zaehlen statt auf Einrueckung hoffen: Ein
        // Struktur-Literal enthaelt verschachtelte Bloecke, und eine
        // Suche nach der naechsten schliessenden Zeile schneidet mitten
        // hinein — der Waechter meldete daraufhin fuenf Felder als
        // fehlend, die sehr wohl da waren.
        fn rumpf<'a>(quelle: &'a str, ab: &str) -> &'a str {
            let start = quelle
                .find(ab)
                .unwrap_or_else(|| panic!("{ab} nicht gefunden"));
            let rest = &quelle[start..];
            let auf = rest.find('{').expect("keine oeffnende Klammer");
            let mut tiefe = 0i32;
            for (i, c) in rest[auf..].char_indices() {
                match c {
                    '{' => tiefe += 1,
                    '}' => {
                        tiefe -= 1;
                        if tiefe == 0 {
                            return &rest[..auf + i + 1];
                        }
                    }
                    _ => {}
                }
            }
            rest
        }
        // ⚠ Die Anker mit `format!` bauen, sonst findet der Waechter
        // SICH SELBST statt der Stelle, die er bewachen soll — der erste
        // Anlauf tat genau das und meldete daraufhin jedes Feld als
        // fehlend.
        let nutzlast = ohne_leerraum(rumpf(
            quelle,
            &format!("fn wire(&self, final_{} bool)", ':'),
        ));
        let aufzeichnung = ohne_leerraum(rumpf(quelle, &format!("Some(LandingRecord {}", '{')));

        let mut fehlend: Vec<String> = Vec::new();
        for f in &felder {
            if !nutzlast.contains(&ohne_leerraum(&format!("self.{f}"))) {
                fehlend.push(format!("{f} fehlt in BahnFelder::wire() (MQTT-Nutzlast)"));
            }
            if !aufzeichnung.contains(&ohne_leerraum(&format!("bahn.{f}"))) {
                fehlend.push(format!(
                    "{f} fehlt in build_landing_record() (Aufzeichnung)"
                ));
            }
        }
        assert!(
            fehlend.is_empty(),
            "Felder erreichen nicht beide Abnehmer:\n  {}",
            fehlend.join("\n  ")
        );
    }

    /// JEDER Zweig, mit exakt erwartetem Wert UND Quellenangabe.
    ///
    /// ⚠ Eine Tabelle statt Stichproben. Beim ersten Anlauf fehlte der
    /// Zweig „Schaetzer vorhanden, kein letztes Tief" — eine vertauschte
    /// Quellenangabe blieb dadurch unbemerkt (Gegenprobe, 31.08.2026).
    /// Wer einen Zweig aendert, muss hier eine Zeile aendern; wer eine
    /// Angabe vertauscht, wird an genau dieser Zeile rot.
    #[test]
    fn jeder_zweig_liefert_wert_und_passende_quelle() {
        // Bausteine, damit die Tabelle lesbar bleibt.
        let raster = |v: f32| Sinkratenquellen {
            gerastet: Some(v),
            ..Default::default()
        };
        let msfs_est = |v: f32| Sinkratenquellen {
            schaetzer_msfs: Some(v),
            ..Default::default()
        };
        let puffer = |v: f32| Sinkratenquellen {
            puffer_min: Some(v),
            ..Default::default()
        };
        let abtaster = |v: f32| Sinkratenquellen {
            abtaster: Some(v),
            ..Default::default()
        };
        let tief = |v: f32| Sinkratenquellen {
            tief_agl_min: Some(v),
            ..Default::default()
        };
        let xp = |v: f32, fenster: i64| Sinkratenquellen {
            schaetzer_xp: Some((v, fenster)),
            ..Default::default()
        };
        let letztes = |v: f32| Sinkratenquellen {
            letztes_tief: Some(v),
            ..Default::default()
        };
        let dazu = |a: Sinkratenquellen, b: Sinkratenquellen| Sinkratenquellen {
            gerastet: a.gerastet.or(b.gerastet),
            schaetzer_xp: a.schaetzer_xp.or(b.schaetzer_xp),
            schaetzer_msfs: a.schaetzer_msfs.or(b.schaetzer_msfs),
            abtaster: a.abtaster.or(b.abtaster),
            puffer_min: a.puffer_min.or(b.puffer_min),
            tief_agl_min: a.tief_agl_min.or(b.tief_agl_min),
            letztes_tief: a.letztes_tief.or(b.letztes_tief),
        };
        let leer = Sinkratenquellen::default();

        // (Simulator, Quellen, erwarteter Wert, erwartete Angabe)
        let tabelle: Vec<(&str, Sinkratenquellen, f32, &str)> = vec![
            // ── MSFS ──────────────────────────────────────────────────
            ("msfs", raster(-150.0), -150.0, "msfs_simvar_latched"),
            // Der geraster Wert schlaegt alles andere.
            (
                "msfs",
                dazu(raster(-150.0), dazu(msfs_est(-900.0), puffer(-800.0))),
                -150.0,
                "msfs_simvar_latched",
            ),
            ("msfs", msfs_est(-200.0), -200.0, "agl_estimate_msfs"),
            // Vorrang innerhalb der Kette: Schaetzer schlaegt Puffer.
            (
                "msfs",
                dazu(msfs_est(-200.0), puffer(-300.0)),
                -200.0,
                "agl_estimate_msfs",
            ),
            ("msfs", puffer(-300.0), -300.0, "buffer_min"),
            ("msfs", leer, 0.0, "fallback_zero"),
            // ⚠ Ausgeschlossen: Abtaster und tief_agl_min. Beide
            // vorhanden — MSFS darf trotzdem nicht zugreifen.
            (
                "msfs",
                dazu(abtaster(-3000.0), tief(-2500.0)),
                0.0,
                "fallback_zero",
            ),
            // ⚠ Und der X-Plane-Schaetzer ist ihm fremd.
            ("msfs", xp(-900.0, 100), 0.0, "fallback_zero"),
            // ── X-Plane ───────────────────────────────────────────────
            // Beide da → der TIEFERE, und die Angabe nennt beide.
            (
                "xplane",
                dazu(xp(-900.0, 100), letztes(-400.0)),
                -900.0,
                "agl_estimate_xp_or_last_low",
            ),
            (
                "xplane",
                dazu(xp(-200.0, 100), letztes(-700.0)),
                -700.0,
                "agl_estimate_xp_or_last_low",
            ),
            ("xplane", xp(-900.0, 100), -900.0, "agl_estimate_xp"),
            ("xplane", letztes(-400.0), -400.0, "last_low_agl_vs"),
            // ⚠ Fenster >= 3000 ms: der Schaetzer faellt weg.
            ("xplane", xp(-900.0, 3000), 0.0, "fallback_zero"),
            (
                "xplane",
                dazu(xp(-900.0, 9000), letztes(-400.0)),
                -400.0,
                "last_low_agl_vs",
            ),
            ("xplane", abtaster(-3000.0), -3000.0, "sampler_gear_force"),
            // Vorrang innerhalb der Rueckfallkette:
            // Abtaster > Puffer > tiefes AGL.
            (
                "xplane",
                dazu(abtaster(-3000.0), dazu(puffer(-300.0), tief(-2500.0))),
                -3000.0,
                "sampler_gear_force",
            ),
            (
                "xplane",
                dazu(puffer(-300.0), tief(-2500.0)),
                -300.0,
                "buffer_min",
            ),
            // ⚠ Die Rueckfallkette greift auch, wenn der Schaetzer nur am
            // Fenster scheitert — dann darf der Abtaster ran.
            (
                "xplane",
                dazu(xp(-900.0, 9000), dazu(abtaster(-3000.0), puffer(-300.0))),
                -3000.0,
                "sampler_gear_force",
            ),
            ("xplane", puffer(-300.0), -300.0, "buffer_min"),
            ("xplane", tief(-2500.0), -2500.0, "low_agl_vs_min"),
            ("xplane", leer, 0.0, "fallback_zero"),
            // ⚠ Und der MSFS-Schaetzer ist ihm fremd.
            ("xplane", msfs_est(-200.0), 0.0, "fallback_zero"),
            // ── unbekannter Simulator ─────────────────────────────────
            ("other", raster(-150.0), -150.0, "other_latched"),
            ("other", msfs_est(-200.0), -200.0, "other_agl_estimate"),
            ("other", abtaster(-3000.0), -3000.0, "other_sampler"),
            ("other", puffer(-300.0), -300.0, "other_buffer_min"),
            // Vorrang der ganzen Kette:
            // Raster > MSFS-Schaetzer > Abtaster > Puffer.
            (
                "other",
                dazu(
                    raster(-150.0),
                    dazu(msfs_est(-200.0), dazu(abtaster(-3000.0), puffer(-300.0))),
                ),
                -150.0,
                "other_latched",
            ),
            (
                "other",
                dazu(msfs_est(-200.0), dazu(abtaster(-3000.0), puffer(-300.0))),
                -200.0,
                "other_agl_estimate",
            ),
            (
                "other",
                dazu(abtaster(-3000.0), puffer(-300.0)),
                -3000.0,
                "other_sampler",
            ),
            ("other", leer, 0.0, "other_fallback"),
        ];

        for (sim, q, wert, quelle) in tabelle {
            let (ist_msfs, ist_xplane) = (sim == "msfs", sim == "xplane");
            let ergebnis = touchdown_vs_bestimmen(&q, ist_msfs, ist_xplane);
            assert_eq!(
                ergebnis,
                (wert, quelle),
                "{sim} mit {q:?} sollte ({wert}, {quelle}) geben"
            );
        }
    }

    /// Und keine Angabe darf einen Wert behaupten, den sie nicht liefert.
    ///
    /// ⚠ Die Umkehrung von SRR318: Dort meldete
    /// `agl_estimate_xp_or_last_low` eine 0. Dieser Zweig rechnet
    /// `est.min(last)` — er kann gar keine 0 liefern, solange eine der
    /// beiden Quellen negativ ist. Genau das wird hier festgehalten.
    #[test]
    fn eine_null_kommt_nur_aus_dem_rueckfall() {
        for (ist_msfs, ist_xplane) in [(true, false), (false, true), (false, false)] {
            // Ueber alle Kombinationen vorhandener Quellen.
            // ⚠ EINSCHLIESSLICH der Vollmaske. Sieben Quellen ergeben
            // 128 Masken; `0u8..0b111_1111` endet bei Maske 126 und
            // deckt damit 127 davon ab — ausgerechnet die eine, in der
            // ALLE Quellen etwas haben, fehlte (QS-Befund, 31.08.2026).
            for maske in 0u8..=0b111_1111 {
                let bit = |n: u8| maske & (1 << n) != 0;
                let q = Sinkratenquellen {
                    gerastet: bit(0).then_some(-100.0),
                    schaetzer_xp: bit(1).then_some((-900.0, 100)),
                    schaetzer_msfs: bit(2).then_some(-200.0),
                    abtaster: bit(3).then_some(-3000.0),
                    puffer_min: bit(4).then_some(-300.0),
                    tief_agl_min: bit(5).then_some(-2500.0),
                    letztes_tief: bit(6).then_some(-400.0),
                };
                let (wert, quelle) = touchdown_vs_bestimmen(&q, ist_msfs, ist_xplane);
                if quelle.ends_with("fallback_zero") || quelle == "other_fallback" {
                    assert_eq!(wert, 0.0, "Rueckfall {quelle} mit Wert {wert}");
                } else {
                    assert!(
                        wert < 0.0,
                        "Quelle {quelle} nennt einen Messzweig, liefert aber {wert} — \
                         genau der Widerspruch von SRR318"
                    );
                }
            }
        }
    }

    /// Das Szenerie-Verzeichnis wird beim START gebaut, nicht beim Landen.
    ///
    /// ⚠ Auf macOS haengt daran ein Berechtigungsdialog: Der Bau liest
    /// die X-Plane-Installation, und liegt die an einem geschuetzten Ort
    /// (Schreibtisch, Dokumente, iCloud, externes Laufwerk), fragt das
    /// System beim ERSTEN Zugriff. Bis v1.7.12 war das die
    /// Landungs-Korrelation — der Dialog kam im Aufsetzmoment
    /// (Pilotenmeldung 31.08.2026).
    ///
    /// Die Nadeln werden mit `format!` gebaut, sonst faende der Waechter
    /// sich selbst.
    #[test]
    fn das_szenerie_verzeichnis_wird_beim_start_gebaut() {
        let quelle = ohne_leerraum(LIB_QUELLE);
        let ruf = ohne_leerraum(&format!(
            "szenerie_bahn{}{}szenerie_vorbereiten()",
            "::", ""
        ));
        assert!(
            quelle.contains(&ruf),
            "der Vorabbau fehlt — der Berechtigungsdialog kaeme wieder beim Landen"
        );

        // Und er muss im Start-Haken stehen, nicht irgendwo — sonst
        // waere die Frage nur verschoben, nicht vorgezogen. In ZEILEN
        // gemessen; der erklaerende Kommentar dazwischen ist lang, und
        // ein Zeichenabstand wuerde daran scheitern.
        let zeilen: Vec<&str> = LIB_QUELLE.lines().collect();
        let start = zeilen
            .iter()
            .position(|z| z.contains(&format!(".setup(|{}|", "app")))
            .expect(".setup nicht gefunden");
        let stelle = zeilen
            .iter()
            .position(|z| z.contains(&format!("szenerie_vorbereiten{}", "()")))
            .expect("Aufruf nicht gefunden");
        assert!(
            stelle > start && stelle - start <= 20,
            "der Vorabbau steht {} Zeilen nach `.setup(` — er gehoert direkt hinein",
            stelle.saturating_sub(start)
        );
    }

    /// Die Sprit-Achse darf nicht wieder an den Divert-VERDACHT geraten.
    ///
    /// ⚠ Massgeblich ist das eingereichte Ziel gegen das geplante
    /// (`Ausweichziel`), nicht `divert_hint`. Der Unterschied geht in
    /// beide Richtungen schief — die Begruendung steht bei
    /// `die_spritachse_folgt_dem_eingereichten_ziel_nicht_dem_verdacht`.
    ///
    /// Die Nadel wird mit `format!` gebaut, sonst faende der Waechter
    /// sich selbst.
    #[test]
    fn die_spritachse_liest_nirgends_den_divert_verdacht() {
        let quelle = ohne_leerraum(LIB_QUELLE);
        let nadel = ohne_leerraum(&format!("diverted{}Some(stats.divert_hint", ':'));
        assert!(
            !quelle.contains(&nadel),
            "die Sprit-Achse haengt wieder am Verdacht statt am \
             eingereichten Ziel"
        );
    }

    /// Jedes Szenerie-Korrekturfeld muss an BEIDEN Nutzlaststellen stehen.
    ///
    /// ⚠ Die Nutzlast wird an zwei Stellen gebaut (normaler Abschluss und
    /// der Weg über den Wiederaufnahme-Zweig). Ein neues Feld nur an
    /// einer der beiden zu setzen übersetzt sauber, ist grün in allen
    /// Tests — und fehlt dann bei jedem Flug, der über den anderen Weg
    /// geht. Genau so ist am 29.08.2026 ein Feld an vier Stellen still
    /// verloren gegangen.
    ///
    /// Die Nadeln werden mit `format!` gebaut, sonst fände der Wächter
    /// sich selbst (siehe die Lehre oben).
    #[test]
    fn jedes_szenerie_korrekturfeld_steht_an_beiden_nutzlaststellen() {
        let quelle = ohne_leerraum(LIB_QUELLE);
        for feld in [
            "bahn_kurs_korrektur_grad",
            "bahn_breiten_korrektur_m",
            "bahn_schwellen_korrektur_m",
        ] {
            let nadel = ohne_leerraum(&format!("{feld}{}stats", ':'));
            let treffer = quelle.matches(&nadel).count();
            assert_eq!(
                treffer, 2,
                "{feld} wird an {treffer} Stellen in die Nutzlast \
                 geschrieben, erwartet werden 2 — ein Flug über den \
                 anderen Weg verlöre das Feld"
            );
        }
    }

    /// ⚠ Lehre aus zwei Fehlversuchen an EINEM Tag (29.08.2026):
    ///
    /// Ein Wächter, der die Quelle nach einem Text durchsucht, findet
    /// **sich selbst**, wenn dieser Text als ein Stück im Quelltext steht —
    /// `include_str!("../lib.rs")` enthält ja auch den Wächter. Ein solcher
    /// Wächter ist immer grün und beweist nichts. Beide Male ist es erst in
    /// der Gegenprobe aufgefallen: Ich habe die bewachte Zeile entfernt, und
    /// der Test blieb grün.
    ///
    /// Deshalb wird jede Nadel hier mit `format!` aus Teilen gebaut. Wer
    /// einen Wächter ergänzt, macht es genauso — und prüft ihn, indem er
    /// entfernt, was er bewacht.
    #[allow(dead_code)]
    fn nadel_hinweis() {}

    #[test]
    fn ein_langer_flug_wird_aufgenommen_wenn_er_frisch_geschrieben_wurde() {
        // THY77: 12h39m geflogen. Der Client war 4 Minuten weg (Update +
        // Neustart). Genau dieser Fall wurde bis v1.7.8 verworfen, weil
        // nach der FLUGDAUER entschieden wurde.
        assert!(stillstand_erlaubt_wiederaufnahme(
            chrono::Duration::minutes(4)
        ));
        // Auch nach 20 Stunden Flug — die Dauer spielt keine Rolle mehr,
        // nur die Zeit seit der letzten Aktualisierung.
        assert!(stillstand_erlaubt_wiederaufnahme(chrono::Duration::hours(
            23
        )));
    }

    #[test]
    fn der_stillstand_gilt_auch_wenn_der_server_antwortet() {
        // ⚠ QS-Befund an der eigenen Reparatur: Zuerst griff diese Prüfung
        // NUR im Netzwerk-Fehlerzweig. Auf GSG steht ein Geisterflug ewig
        // auf IN_PROGRESS (der Aufräum-Cron läuft nicht) — der Client hätte
        // ihn also wortlos weitergeführt. Die Prüfung muss VOR der
        // Server-Abfrage stehen, damit sie in jedem Fall gilt.
        let q = ohne_leerraum(LIB_QUELLE);
        let start = q
            .find("asyncfntry_resume_flight")
            .expect("try_resume_flight nicht gefunden");
        let bis_server = q[start..]
            .find("client.get_pirep")
            .expect("Server-Abfrage nicht gefunden");
        assert!(
            q[start..start + bis_server].contains("stillstand_erlaubt_wiederaufnahme(stillstand)"),
            "der Stillstand wird erst NACH der Server-Auskunft geprüft — \
             dann führt ein Geisterflug den Client wortlos weiter"
        );
    }

    #[test]
    fn ein_tagealter_stillstand_wird_nicht_automatisch_aufgenommen() {
        assert!(!stillstand_erlaubt_wiederaufnahme(chrono::Duration::hours(
            25
        )));
        assert!(!stillstand_erlaubt_wiederaufnahme(chrono::Duration::days(
            3
        )));
    }

    #[test]
    fn die_flugdauer_entscheidet_nicht_mehr_ueber_die_wiederaufnahme() {
        // ⚠ Gegen die Rückkehr des Fehlers: Ein Vergleich von `started_at`
        // gegen eine Stundenzahl ist genau das, was THY77 gekostet hat.
        let q = ohne_leerraum(LIB_QUELLE);
        // ⚠ Die Nadel wird zur LAUFZEIT zusammengesetzt. Stünde sie als ein
        // Stück im Quelltext, fände der Wächter sich selbst und wäre immer
        // rot — er hätte nie etwas bewiesen. (Beim ersten Lauf genau passiert.)
        let nadel = format!(
            "{}{}{}",
            "Utc::now()-persisted.", "started_at>chrono::", "Duration::hours"
        );
        assert!(
            !q.contains(&nadel),
            "die Wiederaufnahme entscheidet wieder nach der FLUGDAUER — \
             genau der Fehler, der THY77 gekostet hat"
        );
    }

    #[test]
    fn vor_der_server_auskunft_wird_nichts_geloescht() {
        // ⚠ Der zweite Teil des Schadens: Die alte Grenze hat die Datei
        // nicht nur ignoriert, sondern GELÖSCHT. Damit war der Flug auch
        // durch einen zweiten Neustart nicht mehr zu retten.
        let q = ohne_leerraum(LIB_QUELLE);
        let start = q
            .find("asyncfntry_resume_flight")
            .expect("try_resume_flight nicht gefunden");
        let bis_server = q[start..]
            .find("client.get_pirep")
            .expect("Server-Abfrage nicht gefunden");
        assert!(
            !q[start..start + bis_server].contains("clear_persisted_flight"),
            "es wird gelöscht, BEVOR der Server gefragt wurde"
        );
    }

    /// Codex-Folgefund (adversarial, 05.09.2026, fuenfte Runde): der PIREP
    /// per ID abzufragen sagt nichts darueber, WEM er gehoert — `phpvms_
    /// logout` erlaubt ausdruecklich, dass sich waehrend eines laufenden
    /// Fluges ein ANDERER Pilot auf derselben Maschine anmeldet. Die
    /// Eigentuemer-Pruefung muss deshalb VOR `client.get_pirep` laufen,
    /// nicht danach — sonst haette ein 404 fuer den falschen Account Pilot
    /// As Ablage geloescht, ein Erfolg haette ihren Flug unter Pilot Bs
    /// Session wiederaufgenommen.
    #[test]
    fn die_eigentuemer_pruefung_laeuft_vor_der_server_abfrage_per_id() {
        let q = ohne_leerraum(LIB_QUELLE);
        let start = q
            .find("asyncfntry_resume_flight")
            .expect("try_resume_flight nicht gefunden");
        let rest = &q[start..];
        let pruefung_pos = rest.find("pirep_queue_eintrag_gehoert_aktuellem_piloten(");
        let server_pos = rest.find("client.get_pirep(&persisted.pirep_id)");
        match (pruefung_pos, server_pos) {
            (Some(p), Some(s)) => assert!(
                p < s,
                "die Eigentuemer-Pruefung muss vor der direkten PIREP-ID-Abfrage \
                 laufen — sonst kann ein anderer eingeloggter Pilot Pilot As \
                 Ablage loeschen oder ihren Flug uebernehmen"
            ),
            _ => panic!(
                "Eigentuemer-Pruefung oder PIREP-ID-Abfrage in try_resume_flight \
                 nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }

    #[test]
    fn im_laufenden_flug_gibt_es_ein_lebenszeichen() {
        // ⚠ Ohne das ist der Stillstand wertlos: Die Datei wird sonst nur
        // bei Ereignissen geschrieben, und im Reiseflug passiert stundenlang
        // keines. Ihr Zeitstempel waere dann die Abflugzeit — und damit
        // entschiede wieder die FLUGDAUER, nur unter neuem Namen.
        let q = ohne_leerraum(LIB_QUELLE);
        // ⚠ Nadel zur LAUFZEIT zusammensetzen — siehe `nadel_hinweis()`.
        //
        // Bewacht wird das BESTEHENDE periodische Schreiben im
        // Positionsstrom (alle STATS_PERSIST_EVERY_TICKS Ticks, ~25–150 s).
        // Es ist die bessere Quelle als ein Netz-Ereignis: Es laeuft auch
        // ohne Verbindung, und der Client lebt ja trotzdem.
        //
        // In der QS hatte ich hier ein ZWEITES Lebenszeichen im
        // phpVMS-Worker eingebaut, bevor mir dieses auffiel. Doppelt
        // gemoppelt und schlechter (nur bei erfolgreichem POST) — wieder
        // ausgebaut.
        let nadel = format!(
            "{}{}",
            "ifshould_persist&&!flight.stop.load(Ordering::Relaxed){",
            "save_active_flight(&app,&flight);"
        );
        assert!(
            q.contains(&nadel),
            "der Positionsstrom schreibt den gespeicherten Flug nicht mehr \
             periodisch — dann sagt sein Zeitstempel nichts ueber die \
             Lebendigkeit, und es entscheidet wieder die Flugdauer"
        );
    }

    #[test]
    fn eine_zu_lange_stille_datei_blockiert_die_suche_nicht() {
        // ⚠ Sackgasse aus der eigenen QS: Die Wiederaufnahme lehnt bei
        // Stillstand ab und loescht bewusst nicht — trete die Suche dann
        // trotzdem fuer diese Datei zurueck, wird der verwaiste Flug NIE
        // mehr angeboten.
        let q = ohne_leerraum(LIB_QUELLE);
        let start = q
            .find("asyncfnflight_discover_resumable")
            .expect("flight_discover_resumable nicht gefunden");
        let ende = q[start..]
            .find("client.get_user_pireps_in_progress")
            .expect("Server-Weg nicht gefunden");
        assert!(
            q[start..start + ende].contains("stillstand_erlaubt_wiederaufnahme(stillstand)"),
            "die Suche tritt fuer JEDE vorhandene Datei zurueck — auch fuer eine, \
             die gar nicht mehr aufgenommen wird"
        );
    }

    #[test]
    fn eine_alte_datei_ohne_zeitstempel_faellt_auf_den_start_zurueck() {
        // Dateien aus Fassungen vor v1.7.9 kennen `zuletzt_geschrieben`
        // nicht. Dann bleibt nur `started_at` — das alte Verhalten, aber
        // ohne Loeschen. Ein 14-Stunden-Flug aus einer alten Fassung wird
        // damit weiterhin aufgenommen (14 h < 24 h), ein drei Tage alter
        // nicht.
        let mut p = beispiel_flug();
        p.zuletzt_geschrieben = None;
        p.started_at = Utc::now() - chrono::Duration::hours(14);
        assert!(stillstand_erlaubt_wiederaufnahme(stillstand_von(&p)));

        p.started_at = Utc::now() - chrono::Duration::days(3);
        assert!(!stillstand_erlaubt_wiederaufnahme(stillstand_von(&p)));
    }

    #[test]
    fn der_zeitstempel_schlaegt_den_start() {
        // Der eigentliche Gewinn: Ein LANGER Flug, der gerade eben
        // geschrieben wurde, gilt als lebendig — genau THY77.
        // ⚠ Der Abstand muss GRÖSSER als die Grenze sein, sonst beweist der
        // Test nichts: Mit 13 Stunden bliebe er auch dann grün, wenn der
        // Zeitstempel ignoriert würde (13 < 24). In der Gegenprobe genau so
        // aufgefallen — die Wache blieb grün, während ich den Rückfall
        // entfernt hatte.
        let mut p = beispiel_flug();
        p.started_at = Utc::now() - chrono::Duration::hours(30);
        p.zuletzt_geschrieben = Some(Utc::now() - chrono::Duration::minutes(4));
        assert!(
            stillstand_erlaubt_wiederaufnahme(stillstand_von(&p)),
            "der Zeitstempel wird nicht benutzt — es entscheidet die Flugdauer"
        );
    }

    #[test]
    fn die_stillstandsrechnung_steht_nur_an_einer_stelle() {
        // ⚠ Sie wird an drei Stellen gebraucht (Resume, Suche,
        // Riegel-Abfrage). Steht sie mehrfach, laufen die Fassungen
        // auseinander — eine faellt bei alten Dateien zurueck, die naechste
        // vergisst es.
        let q = ohne_leerraum(LIB_QUELLE);
        let nadel = format!("{}{}", "zuletzt_geschrieben.", "unwrap_or(");
        assert_eq!(
            q.matches(&nadel).count(),
            1,
            "die Stillstandsrechnung steht mehrfach im Quelltext"
        );
    }

    #[test]
    fn der_zeitstempel_wird_beim_schreiben_gesetzt() {
        // Wird er nur beim Aufrufer gesetzt, vergisst ihn die nächste
        // Schreibstelle — und die Datei sieht alt aus, obwohl sie frisch ist.
        let q = ohne_leerraum(LIB_QUELLE);
        let start = q
            .find("fnwrite_persisted_flight")
            .expect("write_persisted_flight nicht gefunden");
        let ende = start + 600;
        assert!(
            q[start..ende.min(q.len())].contains("flight.zuletzt_geschrieben=Some(Utc::now())"),
            "der Schreiber stempelt den Zeitpunkt nicht selbst"
        );
    }
