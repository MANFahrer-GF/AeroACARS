fn bahn_felder(stats: &FlightStats, icao: Option<&str>, skip_grund: Option<String>) -> BahnFelder {
    #[cfg(test)]
    BAHN_FELDER_AUFRUFE.with(|z| z.set(z.get() + 1));
    let rm = stats.runway_match.as_ref();
    // Reihenfolge nach Spec §5.3: Die Flugzeugdatei ist die Verfeinerung,
    // die Typtabelle die Basis. Liegt ein aus der Datei gelesener Wert vor,
    // hat er Vorrang — er beschreibt das tatsaechlich geflogene Add-on und
    // nicht das Realmuster.
    //
    // Vorrang heisst aber NICHT blind: `spurweite_aus_paket` liefert nur
    // dann etwas, wenn die Zuordnung eindeutig und der Wert plausibel war.
    // Alles andere faellt auf die Tabelle zurueck.
    let aus_datei = stats.fahrwerk_spurweite_m;
    // GSG1249-Folgefund BCS3317/A306 (Vereinheitlichung 09/2026): dieselbe
    // Sim-Schreibweisen-Kaskade wie `aircraft_limits_for` — vorher nur ein
    // direkter Tabellenschlag auf den rohen `icao`-Wert, der Baureihen ohne
    // Variante ("A300" statt "A306") verlor, obwohl `muster_kandidaten` sie
    // schon kennt. Siehe `muster_fuer_typtabelle`-Dokumentation.
    let spur_m = aus_datei
        .or_else(|| muster_fuer_typtabelle(icao, |m| landing_scoring::spurweite::spurweite_m(m)));
    let breite_m = rm.map(|m| m.width_ft as f64 * 0.3048).filter(|w| *w > 0.0);
    let versatz_m = stats.bahn_max_querversatz_m;

    // Kleinster Randabstand: halbe Bahnbreite minus das aeussere Rad.
    // Negativ heisst, das Rad war jenseits der befestigten Flaeche.
    //
    // Nur wenn ALLE drei Groessen bekannt sind. Ein geschaetzter Randabstand
    // waere eine Behauptung ueber die Bahnkante, die die Daten nicht decken —
    // und er stuende in der Anzeige neben echten Messwerten, ununterscheidbar.
    // Dieselbe Rechnung wie in `sub_bahndisziplin` — bis auf die letzte
    // Stelle. Die Anzeige zeigt den Wert, an dem die Bewertung haengt;
    // zwei Zahlen fuer dieselbe Landung waeren schlimmer als eine
    // fehlende.
    //
    // Insbesondere dieselbe Aussenkante: bis zum aeusseren Rand des
    // aeussersten REIFENS, nicht bis zur Bein-Mitte.
    let rand_m = match (breite_m, spur_m, versatz_m) {
        (Some(b), Some(sp), Some(v)) => {
            Some(b / 2.0 - (v.abs() + landing_scoring::spurweite::aussenkante_halb_aus_spur(sp)))
        }
        _ => None,
    };

    let belag = landing_scoring::belag::belag_aus_angabe(rm.map(|m| m.surface.as_str()));

    // ── Der Kantenuebertritt, aus der GANZEN Spur ────────────────────────
    //
    // `bahn_kante_laengs_m` entsteht live, beim ersten Uebertritt nach dem
    // Schliessen des Messfensters. Live laesst sich nicht wissen, ob das
    // Flugzeug draussen BLEIBT — und genau darauf kommt es an: Ein
    // Ausbrechen ueber die Kante mit anschliessender Korrektur ist kein
    // Raeumen der Bahn.
    //
    // Hier liegt die ganze Spur vor, also wird die Stelle nachgerechnet:
    // der erste Uebertritt, nach dem kein Punkt mehr innerhalb liegt.
    //
    // Am Korpus gemessen (772 Landungen, bei denen beide Regeln ein
    // Ergebnis liefern): 763 stimmen ueberein, **neun weichen ab** — und
    // zwar erheblich, bei HAGN 35 um 1533 m (2616 gegen 1083). Dort
    // markierte die Live-Regel ein Ausbrechen als Ausfahrt.
    //
    // Dieselbe Regel benutzt das Pruefwerkzeug `tools/korpus/spuren_export.py`.
    // Zwei Stellen, eine Regel — sonst misst der Korpus etwas anderes als
    // der Client tut, und alle daraus abgeleiteten Zahlen sind schief.
    // Gesucht wird erst AB dem Beginn des Ausschwenkens.
    //
    // Ein Ausbrechen, das vor dem Kurswechsel liegt und danach draussen
    // bleibt, waere sonst der Raeumpunkt — und laege damit VOR dem
    // Bewertungsende. Der Live-Pfad hat diesen Riegel seit Runde 18; der
    // Nachrechnung fehlte er, und der Test von damals ging durch den
    // Live-Pfad und sah es nicht.
    let ab_m = stats.bahn_raeum_laengs_m.unwrap_or(f64::NEG_INFINITY);
    let kante_mit_quer = breite_m.and_then(|b| {
        let halbe = b / 2.0;
        let spur = &stats.bahn_spur;
        (1..spur.len()).find_map(|i| {
            let drin_davor = (spur[i - 1].1 as f64).abs() <= halbe;
            let draussen = (spur[i].1 as f64).abs() > halbe;
            let nach_dem_ausschwenken = spur[i].0 as f64 >= ab_m;
            let bleibt_draussen = spur[i..].iter().all(|(_, q)| (*q as f64).abs() > halbe);
            (drin_davor && draussen && nach_dem_ausschwenken && bleibt_draussen)
                .then(|| (spur[i].0 as f64, spur[i].1))
        })
    });
    let kante_aus_spur = kante_mit_quer.map(|(l, _)| l);
    // B5 (Bahnansicht 22.10.2026, #1662 ESSA 19L): Die Seite, auf der die
    // Spur die Bahn ENDGUELTIG verlaesst. Der Kurswechsel-Ausloeser hatte
    // bei 1535 m auf einen Linksschlenker reagiert (Kurs und Querbewegung
    // beide links, also „bestaetigt") — verlassen wurde die Bahn aber bei
    // 2031 m rechts, ueber +23 m hinaus und danach draussen. Der Uebertritt
    // ist die direkte Beobachtung; er hat Vorrang vor der Kurs-Probe.
    let kante_seite: Option<String> =
        kante_mit_quer.map(|(_, q)| if q > 0.0 { "right" } else { "left" }.to_string());
    // B5b (Thomas 22.10.2026, Score-Version 26 — dasselbe, noch nicht
    // veroeffentlichte Release): War der Kurswechsel ein Schlenker zur
    // ANDEREN Seite als der endgueltige Kantenuebertritt, endet der bewertete
    // Teil dort, wo die Spur die Kante endgueltig verlaesst — nicht am
    // Schlenker. Ohne solchen Uebertritt (oder bei gleicher Seite) bleibt es
    // beim Beginn des Ausschwenkens. Nur neue Fluege; Altbestand wird nie neu
    // gerechnet. Das Messfenster (`mess_ende_laengs_m`, Fahrt) ist davon
    // unberuehrt.
    let bewertungs_ende_raeum_m: Option<f64> = match (kante_mit_quer, &kante_seite, &stats.bahn_raeum_seite) {
        (Some((k, _)), Some(ks), Some(live)) if ks != live => Some(k),
        _ => stats.bahn_raeum_laengs_m,
    };

    // ── Punkt und Fahrt gehoeren zusammen ────────────────────────────────
    //
    // Die Fahrt gehoert zu der Stelle, an der sie gemessen wurde. Deshalb
    // werden beide **hier zusammen** bestimmt und nicht getrennt: Ein
    // `.or()` auf den Geschwindigkeitswert allein greift sonst auf einen
    // anderen Punkt zu.
    //
    // Genau das stand hier: `kante_gs.or(stats.bahn_raeum_gs_kt)`. War der
    // Uebertritt nachgerechnet und verschoben, fiel die Fahrt still auf die
    // des KURSWECHSELS zurueck — also auf den Punkt, den `scoring_cutoff_m`
    // meldet, hunderte Meter frueher und deutlich schneller. Der Kommentar
    // daneben sagte das Gegenteil, und der Test sah es nicht, weil er
    // `bahn_raeum_gs_kt` gar nicht setzte.
    let (kante_m, kante_gs) = match kante_aus_spur {
        // Nachgerechnet: die live gemessene Fahrt nur, wenn sie zu dieser
        // Stelle gehoert. Die Spur traegt keine Geschwindigkeit, also gibt
        // es fuer eine verschobene Stelle keine.
        Some(k) => {
            let gs = match (stats.bahn_kante_laengs_m, stats.bahn_kante_gs_kt) {
                (Some(live), Some(gs)) if (k - live).abs() < 25.0 => Some(gs),
                _ => None,
            };
            (Some(k), gs)
        }
        // Keine Nachrechnung moeglich: der live gemessene Uebertritt mit
        // seiner eigenen Fahrt.
        None => match stats.bahn_kante_laengs_m {
            Some(live) => (Some(live), stats.bahn_kante_gs_kt),
            // Gar kein Uebertritt: Es bleibt der Beginn des Ausschwenkens —
            // und dazu gehoert dessen Fahrt.
            None => (stats.bahn_raeum_laengs_m, stats.bahn_raeum_gs_kt),
        },
    };

    // Die Ausfahrten aus der Bodenkarte, die der Anflug ohnehin geladen hat.
    //
    // Ohne Karte bleibt die Liste leer — das ist NICHT dasselbe wie „diese
    // Bahn hat keine Ausfahrten", aber es sieht in der Anzeige so aus.
    // Deshalb zeichnet sie Stummel nur, wenn wirklich welche gefunden
    // wurden, und behauptet nie das Gegenteil.
    // v1.7.8 — die Rollwege der Szenerie haben Vorrang.
    //
    // Die Serverkarte kommt aus OpenStreetMap, also aus einer dritten
    // Welt: 167 namenlose Ausfahrten an 68 Plaetzen, sieben Plaetze ganz
    // ohne. Die Szenerie kennt sie — allein die installierte
    // X-Plane-Szenerie fuehrt 243.945 benannte Rollwegkanten an 6.114
    // Flughaefen.
    //
    // Nur wenn die Szenerie welche hat; sonst bleibt es bei der
    // Serverkarte. Eine leere Szenerie-Karte waere schlechter als eine
    // gute OSM-Karte.
    //
    // ⚠ Beide Karten (Szenerie UND Serverkarte) muessen gegen den
    // Flughafen der GEMATCHTEN BAHN (`rm.airport_ident`) geprueft werden —
    // NICHT gegen `runway_correlation_icao`. Beides beschreibt normalerweise
    // denselben Platz, ist aber NICHT garantiert identisch:
    // `correlate_airport_icao` bevorzugt bei mehrdeutigen Nachbarflughaefen
    // bewusst den GEPLANTEN Platz, waehrend `lookup_runway_with_fallback`
    // (davon unabhaengig) eine OurAirports-Bahn eines ANDEREN, tatsaechlich
    // naeheren Nachbarflughafens zurueckgeben kann. Eine gegen
    // `runway_correlation_icao` gepruefte Karte waere dann auf die Bahn
    // des NACHBARN projiziert worden — plausibel aussehende, aber falsche
    // oder fehlende Ausfahrten. `rm.airport_ident` ist der einzige Wert,
    // der zur tatsaechlich verwendeten GEOMETRIE gehoert (externe
    // Gegenpruefung, Codex, adversarial, 04.09.2026, Runde 7).
    //
    // FAIL CLOSED ohne gematchte Bahn (`rm = None`): das kostet nichts —
    // ohne `rm` berechnet der `match` unten ohnehin keine Ausfahrten —,
    // schliesst aber eine Fail-open-Luecke bei einem aus einer ALTEN
    // Fassung wiederhergestellten Flug, dessen `runway_correlation_icao`
    // per `#[serde(default)]` auf `None` steht, obwohl `runway_match`
    // durchaus vorliegt.
    let landeplatz_der_bahn = rm.map(|m| m.airport_ident.as_str());

    // Woher die Rollwege stammen — die Anzeige nennt die Quelle
    // („aus der Szenerie“ / „aus OpenStreetMap“, Bahnansicht 2.1.8).
    let mut ausfahrten_quelle: Option<String> = None;
    // ⚠ Gemerkt je Bodenkarte und Bahn — siehe `ausfahrten_gemerkt`.
    let ausfahrten: Vec<storage::RunwayExit> = match (rm, bodenquelle(stats, landeplatz_der_bahn)) {
        (Some(m), Some(quelle)) if m.width_ft > 0.0 => {
            let aus_szenerie = matches!(quelle, Bodenquelle::Szenerie(_));
            ausfahrten_quelle = Some(if aus_szenerie { "szenerie" } else { "osm" }.to_string());
            // Doppelnamen an derselben Abzweigung → ein Name (#1936): der,
            // den die Bodenkarte des Servers (OSM) kennt — nur sinnvoll,
            // wenn die Rollwege aus der Szenerie stammen und die Karte zum
            // selben Platz gehoert (dieselbe Pruefung wie `bodenquelle`).
            let osm = stats
                .arr_ground_geojson
                .as_ref()
                .filter(|_| aus_szenerie)
                .filter(|_| {
                    matches!(
                        (stats.arr_ground_geojson_icao.as_deref(), landeplatz_der_bahn),
                        (Some(gi), Some(lp)) if gi.eq_ignore_ascii_case(lp)
                    )
                });
            let bekannt = |name: &str| {
                osm.is_some_and(|k| {
                    serde_json::to_string(name)
                        .is_ok_and(|n| k.contains(&format!("\"r\":{n}")))
                })
            };
            ausfahrten::doppelnamen_zusammenfassen(ausfahrten_gemerkt(stats, quelle, m), bekannt)
            .into_iter()
            .map(|a| storage::RunwayExit {
                name: a.name,
                laengs_m: a.laengs_m,
                seite: a.seite,
                // Der Verlauf gehoert dazu — aus ihm zeichnet die
                // Queransicht den Korridor des genommenen Rollwegs.
                //
                // Er fehlte hier bis zur QS-Runde 8 am 26.08.2026: berechnet,
                // mit zwoelf Tests abgedeckt, und beim Umkopieren in einer
                // Feldliste vergessen. Der Korridor erschien deshalb NUR in
                // der Vorschau, wo ich ihn von Hand gesetzt hatte, und bei
                // keinem einzigen echten Flug.
                verlauf: a
                    .verlauf
                    .into_iter()
                    .map(|v| storage::Verlaufspunkt {
                        laengs_m: v.laengs_m,
                        quer_m: v.quer_m,
                    })
                    .collect(),
            })
            .collect()
        }
        _ => Vec::new(),
    };

    // Den Verlauf nur dort behalten, wo die Anzeige ihn zeichnet.
    //
    // # Warum das noetig ist
    //
    // Die Queransicht zeichnet den Korridor NUR fuer die genommene
    // Ausfahrt — Raeumseite, hoechstens 120 Meter vom Raeumpunkt. Alle
    // anderen Verlaeufe wuerden mitreisen, ohne je gezeichnet zu werden.
    //
    // In Muenchen (26L) sind das 14 Ausfahrten mit je bis zu 57 Punkten:
    // rund 36 Kilobyte je Landung, mehr als die Rollspur selbst. Auf
    // eine Ausfahrt eingekuerzt bleiben davon rund 2,5.
    //
    // Die Reserve von 200 statt 120 Metern ist Absicht: Das Fenster der
    // Anzeige muss VOLLSTAENDIG in dem liegen, was hier durchkommt.
    // Waeren beide Zahlen gleich, wuerde jede kuenftige Lockerung dort
    // stillschweigend ins Leere greifen — der Korridor verschwaende, und
    // niemand wuesste warum.
    // Wert seit v3: bundle/grenzwerte/ausrollen.json -> verlauf_behalten_m (v2.1.24: 200.0)
    // Dieselbe Ableitung wie unten in `clearance_side` — einmal
    // gerechnet, damit beide nicht auseinanderlaufen koennen.
    let raeum_seite: Option<String> = kante_seite.clone().or_else(|| stats.bahn_raeum_seite.clone()).or_else(|| {
        match (stats.bahn_raeum_kurs_diff, stats.bahn_raeum_laengs_m) {
            (Some(d), Some(ab)) => bahn_raeum_seite(d, &stats.bahn_spur, ab),
            _ => None,
        }
    });
    let mut ausfahrten = ausfahrten;
    for a in ausfahrten.iter_mut() {
        let nah = match (stats.bahn_raeum_laengs_m, raeum_seite.as_deref()) {
            (Some(r), Some(seite)) => {
                a.seite == seite && (a.laengs_m - r).abs() <= crate::grenzwerte::grenzwerte().ausrollen.verlauf_behalten_m.wert
            }
            _ => false,
        };
        if !nah {
            a.verlauf.clear();
        }
    }
    // Die genutzte Ausfahrt — EINMAL hier bestimmt und als eigenes Feld
    // gesendet (Bahnansicht 2.1.8, #1936): Kopf, Bild, Lupe, PDF und
    // Webapp zeigen nur diesen Namen, statt dreimal selbst zu suchen.
    //
    // Aussen `None`: keine Bodenkarte — nichts sagen. `Some(None)`: Karte da,
    // aber keine genutzte Ausfahrt — geht als `null` raus und loescht beim
    // Recorder den Namen einer frueheren Revision (Codex 22.10.2026).
    let clearance_exit: Option<Option<String>> = ausfahrten_quelle.as_ref().map(|_| match (kante_m, raeum_seite.as_deref()) {
        (Some(r), Some(seite)) => {
            ausfahrten::genutzte_ausfahrt(&ausfahrten, r, seite, |a| (a.seite.as_str(), a.laengs_m))
                .map(|a| a.name.clone())
        }
        _ => None,
    });

    BahnFelder {
        // Genau der Anteil, den `assess_touchdown` von der Aufsetz-
        // distanz abzieht — deshalb dieselbe Funktion, nicht eine
        // zweite Herleitung.
        spur_nullpunkt_versatz_m: rm.map(|m| displacement_not_in_geometry_ft(m) as f64 * 0.3048),
        // Zwei Punkte, zwei Bedeutungen — siehe `bahn_kante_laengs_m`.
        //
        // `clearance_point_m` ist die Stelle des VERLASSENS (Kante), denn
        // dort setzt die Anzeige die Marke „Bahn geraeumt". Fehlt sie —
        // etwa weil das Flugzeug auf der Bahn zum Stehen kam —, faellt sie
        // auf den Beginn des Ausschwenkens zurueck; das ist dann der
        // letzte belegte Punkt und keine Erfindung.
        // Beide kommen aus derselben Entscheidung weiter oben — kein
        // `.or()` mehr auf einem der beiden allein.
        clearance_point_m: kante_m,
        clearance_speed_kt: kante_gs,
        // Ab hier wird nicht mehr bewertet: der Beginn des Ausschwenkens.
        // Ohne dieses Feld fiel die Bewertungsgrenze mit der Kante
        // zusammen, und das Ausschwenken zaehlte als Fehler des Piloten —
        // bei `0Ab3v9EvNN1LKZ8z` (EDDH 05) 21,95 m auf einer Bahn mit
        // 23 m Halbbreite, gemessen unmittelbar vor dem Abbiegen.
        scoring_cutoff_m: bewertungs_ende_raeum_m,
        mess_ende_laengs_m: stats.bahn_fenster_zu_laengs_m,
        // Die Ausfahrtsseite, notfalls nachgerechnet.
        //
        // Live gelingt sie selten: Im Moment des Kurswechsels ist die
        // Querbewegung erst wenige Meter gross, und der Vertrag verlangt
        // zwei uebereinstimmende Groessen. Mit der ganzen Spur ist sie
        // sichtbar.
        clearance_side: raeum_seite.clone(),
        track_width_m: spur_m,
        // Solange die Spurweite aus der Typtabelle kommt, ist die Quelle
        // fest. Schritt 11 der Bauliste liest sie aus der Flugzeugdatei —
        // dann entscheidet sich hier, welcher Wert gewonnen hat.
        track_width_source: spur_m.map(|_| {
            if aus_datei.is_some() {
                "aircraft_file".to_string()
            } else {
                "type_table".to_string()
            }
        }),
        wingspan_m: muster_fuer_typtabelle(icao, |m| landing_scoring::spurweite::spannweite_m(m)),
        runway_width_m: breite_m,
        min_edge_clearance_m: rand_m,
        max_lateral_offset_m: versatz_m,
        lateral_samples: stats
            .bahn_spur
            .iter()
            .map(|(lg, qr)| storage::LateralSample {
                laengs_m: *lg,
                quer_m: *qr,
            })
            .collect(),
        vorherige_durchgaenge: vorherige_durchgaenge(stats),
        // `Unbekannt` ist NICHT „unbefestigt" — es ist „wir wissen es
        // nicht". Beide fuehren zum selben Verzicht auf die seitliche
        // Bewertung, aber die Anzeige muss sie auseinanderhalten koennen.
        surface_paved: match belag {
            landing_scoring::belag::Belag::Unbekannt => None,
            b => Some(b.seitlich_bewertbar()),
        },
        overrun_m: stats.bahn_overrun_m,
        lateral_skip_reason: skip_grund,
        clearance_exit,
        runway_exits_quelle: if ausfahrten.is_empty() { None } else { ausfahrten_quelle },
        runway_exits: ausfahrten,
    }
}

/// Auf einen Dezimeter runden, für die Leitung.
///
/// Als `f64`, weil ein gerundetes `f32` sich beim Serialisieren wieder
/// aufbläht: `523.2f32` schreibt sich als `523.2000122070312`.
fn dezimeter(m: f32) -> f64 {
    (m as f64 * 10.0).round() / 10.0
}

impl BahnFelder {
    /// Dieselben Werte, fuer die Leitung.
    ///
    /// **Der Client rechnet, der Server zeigt an.** Diese Funktion ist die
    /// einzige Uebersetzung zwischen beiden Welten; wer ein Feld ergaenzt,
    /// ergaenzt es hier und ist an allen Stellen fertig.
    ///
    /// Leere Spuren werden zu `None`, nicht zu `Some(vec![])`: Eine leere
    /// Liste sieht in der Anzeige aus wie eine Messung, die nichts
    /// gefunden hat, und ist von „nicht erfasst" nicht zu unterscheiden.
    /// `final_` sagt, ob das Ausrollen feststeht. Bei `touchdown_complete`
    /// ist es `false` — dort rollt das Flugzeug noch. Siehe
    /// `BahnWire::rollout_final`.
    fn wire(&self, final_: bool) -> aeroacars_mqtt::BahnWire {
        aeroacars_mqtt::BahnWire {
            rollout_final: final_,
            spur_nullpunkt_versatz_m: self.spur_nullpunkt_versatz_m,
            clearance_point_m: self.clearance_point_m,
            scoring_cutoff_m: self.scoring_cutoff_m,
            mess_ende_laengs_m: self.mess_ende_laengs_m,
            clearance_speed_kt: self.clearance_speed_kt,
            clearance_side: self.clearance_side.clone(),
            track_width_m: self.track_width_m,
            track_width_source: self.track_width_source.clone(),
            wingspan_m: self.wingspan_m,
            runway_width_m: self.runway_width_m,
            min_edge_clearance_m: self.min_edge_clearance_m,
            max_lateral_offset_m: self.max_lateral_offset_m,
            lateral_samples: if self.lateral_samples.is_empty() {
                None
            } else {
                Some(
                    self.lateral_samples
                        .iter()
                        .map(|s| aeroacars_mqtt::LateralSampleWire {
                            // Auf einen Dezimeter gerundet — und zwar als
                            // f64, sonst bringt es nichts.
                            //
                            // Ein `f32` mit dem Wert 523,2 serialisiert als
                            // `523.2000122070312`: achtzehn Zeichen fuer
                            // eine Groesse, die auf zehn Zentimeter gemeint
                            // ist und deren Quelle (GPS im Simulator) auf
                            // etwa einen Meter genau misst.
                            //
                            // Gemessen an einer vollen Spur mit 400 Punkten:
                            // 23 KB roh, 13 KB gerundet. Der Broker verwirft
                            // Pakete ueber 64 KB (`max_packet_size` in der
                            // mosquitto-Konfiguration), und derselbe Payload
                            // liegt danach in der Datenbank und im
                            // Flugprotokoll — dreimal derselbe Ballast.
                            laengs_m: dezimeter(s.laengs_m),
                            quer_m: dezimeter(s.quer_m),
                        })
                        .collect(),
                )
            },
            vorherige_durchgaenge: if self.vorherige_durchgaenge.is_empty() {
                None
            } else {
                Some(
                    self.vorherige_durchgaenge
                        .iter()
                        .map(|d| aeroacars_mqtt::DurchgangWire {
                            lateral_samples: d
                                .lateral_samples
                                .iter()
                                .map(|s| aeroacars_mqtt::LateralSampleWire {
                                    laengs_m: dezimeter(s.laengs_m),
                                    quer_m: dezimeter(s.quer_m),
                                })
                                .collect(),
                        })
                        .collect(),
                )
            },
            surface_paved: self.surface_paved,
            overrun_m: self.overrun_m,
            lateral_skip_reason: self.lateral_skip_reason.clone(),
            // Wie bei der Spur: leer heisst weglassen, nicht `[]`. Eine
            // leere Liste sieht in der Anzeige aus wie „diese Bahn hat
            // keine Ausfahrten" — das ist etwas anderes als „wir hatten
            // keine Bodenkarte".
            // Wie die Ausfahrten: abgeleitet aus der Bodenkarte, deshalb
            // ohne Karte weggelassen statt `null` (ein `null` loeschte beim
            // Recorder den gespeicherten Wert, RFC 7396).
            clearance_exit: self.clearance_exit.clone(),
            runway_exits_quelle: self.runway_exits_quelle.clone(),
            runway_exits: if self.runway_exits.is_empty() {
                None
            } else {
                Some(
                    self.runway_exits
                        .iter()
                        .map(|a| aeroacars_mqtt::RunwayExitWire {
                            name: a.name.clone(),
                            laengs_m: a.laengs_m,
                            seite: a.seite.clone(),
                            verlauf: a
                                .verlauf
                                .iter()
                                .map(|v| aeroacars_mqtt::VerlaufspunktWire {
                                    laengs_m: v.laengs_m,
                                    quer_m: v.quer_m,
                                })
                                .collect(),
                        })
                        .collect(),
                )
            },
        }
    }
}

/// Frühere Durchgänge, die in der Ansicht dieser Landung Platz haben.
///
/// Nur solche auf DERSELBEN Bahn: Die Spur ist gegen die Bahn projiziert,
/// auf der damals aufgesetzt wurde. Wer nach dem Durchstarten auf einer
/// anderen Bahn landet, hätte sonst eine Spur im Bild, die dort nie lief.
///
/// Ausgedünnt auf zwanzig Meter: Der Durchgang ist Beiwerk und reist im
/// selben Paket wie die Spur der Landung (Brokergrenze 64 KB).
fn vorherige_durchgaenge(stats: &FlightStats) -> Vec<storage::VorigerDurchgang> {
    let Some(rm) = stats.runway_match.as_ref() else {
        return Vec::new();
    };
    let bahn = format!("{} {}", rm.airport_ident, rm.runway_ident);
    stats
        .bahn_vorige_spuren
        .iter()
        .filter(|(b, spur)| *b == bahn && spur.len() >= 2)
        .map(|(_, spur)| {
            let mut aus: Vec<storage::LateralSample> = Vec::new();
            for (i, &(lg, qr)) in spur.iter().enumerate() {
                let letzter = i + 1 == spur.len();
                if let Some(v) = aus.last() {
                    if (lg - v.laengs_m).abs() < 20.0 && !letzter {
                        continue;
                    }
                }
                aus.push(storage::LateralSample {
                    laengs_m: lg,
                    quer_m: qr,
                });
            }
            storage::VorigerDurchgang {
                lateral_samples: aus,
            }
        })
        .collect()
}

/// Traeger der abgeleiteten Bahndisziplin-Werte — siehe `bahn_felder`.
struct BahnFelder {
    /// Um wie viele Meter die LAENGSMASSE der Spur gegen die
    /// Landeschwelle verschoben sind.
    ///
    /// # Warum es dieses Feld gibt
    ///
    /// Der Payload fuehrt zwei Nullpunkte nebeneinander. Der
    /// Aufsetzpunkt (`td_distance_from_threshold_m`) ist immer ab der
    /// LANDESCHWELLE gemessen; die Spurwerte (`lateral_samples`,
    /// `clearance_point_m`, `runway_exits`, `scoring_cutoff_m`) kommen
    /// roh aus `projiziere_auf_bahn` und liegen damit ab dem
    /// Schwellenpunkt DER NAVDATEN.
    ///
    /// Ob die beiden zusammenfallen, entscheidet die Datenquelle: Steckt
    /// der Versatz schon im Schwellenpunkt selbst (bestaetigt die
    /// Geometrie den gemeldeten Wert innerhalb der Toleranz — siehe
    /// `displacement_not_in_geometry_ft`), sind sie identisch; steckt er
    /// nicht drin (oder ist die Geometrie unklar), liegen sie genau um
    /// den gemeldeten Wert auseinander. `displacement_not_in_geometry_ft`
    /// weiss das pro Bahn — und dieser Wert ist es.
    ///
    /// ⚠ NICHT die versetzte Schwelle selbst. Das ist eine ANDERE Zahl:
    /// `effective_displaced_threshold_ft` ist immer der volle, gemeldete
    /// Versatz aus dem Feld, unabhaengig davon ob er schon im
    /// Schwellenpunkt steckt (die Geometrie fliesst dort NICHT mehr ein —
    /// siehe deren Doku).
    ///
    /// v1.7.18: die alte, pauschale Erkennung ueber die eingebettete
    /// OurAirports-Tabelle lag bei 66 % der Bahnen mit echtem Versatz
    /// falsch (Beleg: FDX2/LEMD 32L, 06.09.2026 — ein Aufsetzen 376 m
    /// hinter der Schwelle wurde als 552 m DAVOR gezeigt).
    /// `geometry_implied_displacement_ft` fragt seither nur noch
    /// Navigraph selbst (eigene Schwelle gegen Gegenschwelle,
    /// stichprobenbelegt 87,5 % Trefferquote) — TJPS 12 (LAN273, 573 m
    /// Versatz) ist genau der Fall, den die alte Tabellen-Erkennung nicht
    /// erreicht hat, weil OurAirports fuer diese Bahn keinen Wert fuehrt;
    /// die neue Selbstprobe erkennt ihn korrekt als schon versetzt.
    spur_nullpunkt_versatz_m: Option<f64>,
    clearance_point_m: Option<f64>,
    scoring_cutoff_m: Option<f64>,

    /// Bis wohin der groesste Querversatz mitgewachsen ist.
    ///
    /// Siehe `bahn_fenster_zu_laengs_m`: liegt regelmaessig VOR
    /// `scoring_cutoff_m`, weil das Messfenster unter sechzig Knoten
    /// schliesst, der Kurs aber erst spaeter abweicht.
    mess_ende_laengs_m: Option<f64>,
    clearance_speed_kt: Option<f64>,
    clearance_side: Option<String>,
    track_width_m: Option<f64>,
    track_width_source: Option<String>,
    wingspan_m: Option<f64>,
    runway_width_m: Option<f64>,
    min_edge_clearance_m: Option<f64>,
    max_lateral_offset_m: Option<f64>,
    lateral_samples: Vec<storage::LateralSample>,
    vorherige_durchgaenge: Vec<storage::VorigerDurchgang>,
    surface_paved: Option<bool>,
    overrun_m: Option<f64>,
    lateral_skip_reason: Option<String>,
    /// Die genutzte Ausfahrt (Name), einmal hier bestimmt — siehe
    /// `bahn_felder`. Kopf, Bild, Lupe, PDF und Webapp zeigen nur sie.
    /// Aussen `None` = keine Bodenkarte, `Some(None)` = Karte, kein Treffer.
    clearance_exit: Option<Option<String>>,
    /// Woher `runway_exits` stammen: `"szenerie"` oder `"osm"`.
    runway_exits_quelle: Option<String>,
    runway_exits: Vec<storage::RunwayExit>,
}

/// v3 (P-4.04): die Bewertungseingabe einer Landung — EINE Stelle fuer den
/// Landungsdatensatz und das Eingabe-Dokument an live.
///
/// v0.10.0 (#runway-utilization-score): LDA-basierten Bahn-Auslastungs-
/// Score aktivieren. Wenn alle benötigten Felder vorhanden → neuer
/// Algorithmus, sonst skipped mit konkretem Reason (KEIN Rollback auf
/// meter-only). v0.15.x: effektiven Landeflughafen (Divert-aware) statt
/// geplantes Ziel.
fn landungs_bewertungseingabe(
    flight: &ActiveFlight,
    stats: &FlightStats,
    aircraft_icao: Option<&str>,
    effective_arr_icao: &str,
) -> landing_scoring::LandingScoringInput {
    let mut scoring_input = scoring_eingang(
        stats,
        aircraft_icao,
        Some(Ausweichziel {
            eingereicht: effective_arr_icao,
            geplant: &flight.arr_airport,
        }),
        flight.plan_strecke_nm,
    );
    fill_v2_rollout_fields(&mut scoring_input, stats, effective_arr_icao);
    scoring_input
}

/// v3 (P-9.44, Kap. 5.2): Der `herkunft`-Teil des Eingabe-Dokuments — woher
/// die Werte in `eingang` kamen. Reine Dokumentation: Die Bewertung liest nur
/// `eingang` (`crate::bewertung::bewerten` nimmt allein den
/// `LandingScoringInput`, der Server gibt dem Modul nur `eingang`). Eine neue
/// OurAirports-Fassung aendert damit nie die Note eines gespeicherten Flugs.
/// Gebaut im selben Augenblick wie die Eingabe (beim Touch-and-Go VOR dem
/// Zuruecksetzen der Landungsfelder, P-4.13).
fn landungs_herkunft(
    flight: &ActiveFlight,
    stats: &FlightStats,
    eingabe: &landing_scoring::LandingScoringInput,
    effective_arr_icao: &str,
) -> serde_json::Value {
    let bahn_art = match (stats.szenerie_uebernahme.as_ref(), stats.runway_source) {
        (Some(b), _) if !b.uebernommen.is_empty() => Some("szenerie"),
        (_, Some(runway::RunwaySource::Navigraph)) => Some("navigraph"),
        (_, Some(runway::RunwaySource::OurAirportsFallback)) => Some("ourairports"),
        _ => None,
    };
    let stand = laufzeit_paket::stand();
    serde_json::json!({
        // Kennung des Aufsetzers; daran traegt `landung_beim_wegsteigen_
        // abschliessen` die Touch-and-Go-Entscheidung nach (Codex 30.10.).
        "aufsetzen": stats.landing_at.or(stats.sampler_touchdown_at),
        "sinkrate": {
            "landing_source": stats.landing_source,
            "landing_confidence": stats.landing_confidence,
            "vs_quelle": stats.landing_vs_source,
            "sim_wert_fpm": stats.landing_sim_referenz_fpm,
            "landing_analysis": stats.landing_analysis,
            "hopser": stats.bounce_count,
            "leichte_hopser": stats.light_bounce_count,
        },
        "bahn": {
            // Dieselbe Gruppe wie im Touchdown-Payload (eine Quelle).
            "zuordnung": bahn_herkunft(stats),
            "runway_nav_geometry": stats.runway_nav_geometry,
            "tch_ist_ft": stats.runway_tch_actual_ft,
            "szenerie_stand": stats.korrelierter_szenerie_stand,
            "korrekturen": stats.szenerie_uebernahme,
            "rollspur": {
                "bahn_raeum_laengs_m": stats.bahn_raeum_laengs_m,
                "bahn_raeum_gs_kt": stats.bahn_raeum_gs_kt,
                "bahn_raeum_seite": stats.bahn_raeum_seite,
                "bahn_max_querversatz_m": stats.bahn_max_querversatz_m,
                "bahn_overrun_m": stats.bahn_overrun_m,
                "bahn_proben": stats.bahn_proben,
                "bahn_fenster_zu": stats.bahn_fenster_zu,
            },
            "quelle": {
                "art": bahn_art,
                // Immer: Die OurAirports-Tabelle dieses Laufs wird auch bei
                // einem Navigraph-Treffer gelesen (Belag, Phantom-Bahnen).
                // Der eingebaute Stand hat keinen Commit (Direktabruf,
                // `runway.rs`): `null`, die Daten belegt dann `paket`
                // (Manifest-Hash ueber die Hashes aller Dateien).
                "ourairports_commit": stand.ourairports_commit,
                "paket": stand.id,
                "navigraph_zyklus": stats.runway_nav_cycle,
                "szenerie": stats.sim_kennung,
            },
        },
        "spurweite": {
            "m": eingabe.fahrwerk_spurweite_m,
            "quelle": match (stats.fahrwerk_spurweite_m, eingabe.fahrwerk_spurweite_m) {
                (Some(_), _) => Some("flugzeugdatei"),
                (None, Some(_)) => Some("typtabelle"),
                _ => None,
            },
        },
        "vref": {
            "gemessen_kt": stats.landing_vref_gemessen.map(|(v, _)| v),
            "gemessen_quelle": stats.landing_vref_gemessen.map(|(_, q)| q),
            "quelle": stats.landing_vref_source,
            "abweichung_kt": stats.landing_vref_deviation_kt,
            "ias_kt": stats.landing_speed_kt,
            "formel_kt": stats.landing_vref_formel_kt,
            "toleranz_kt": stats.landing_vref_toleranz_kt,
            "landeklappen": stats.landing_vref_landeklappen,
            "pmdg_vref_kt": stats.pmdg_vref_at_landing,
        },
        "abdeckung_fehlt": stats.landung_abdeckung_fehlt,
        "divert_hint": stats.divert_hint,
        "ausweichziel": { "eingereicht": effective_arr_icao, "geplant": flight.arr_airport },
        "phasenwechsel": stats.transitions,
        "aufsetzer": stats.touchdown_events,
        "durchstarts": stats.go_around_count,
    })
}

/// v3 (Kap. 5.2): `meta`-Zusatz, `herkunft` und `roh` eines Aufsetzers, im
/// selben Augenblick wie die Eingabe gebaut (Touch-and-Go: VOR dem
/// Zuruecksetzen, P-4.13 — dann wird auch der Anflugpuffer hier kopiert).
/// Reine Dokumentation: `bewerten` nimmt nur den `LandingScoringInput`.
/// `muster` ist das Muster, mit dem `eingabe` gebaut wurde.
fn landungs_begleitung(
    flight: &ActiveFlight,
    stats: &FlightStats,
    eingabe: &landing_scoring::LandingScoringInput,
    muster: Option<&str>,
    effective_arr_icao: &str,
) -> landungs_eingabe::Begleitung {
    let mut meta = stats.landing_sim_meta.clone().unwrap_or_else(|| {
        serde_json::json!({ "sim": null, "sim_fassung": null, "profil": null, "profil_variante": null, "kategorie": null })
    });
    meta["aircraft_icao"] = muster.into();
    landungs_eingabe::Begleitung {
        meta,
        herkunft: landungs_herkunft(flight, stats, eingabe, effective_arr_icao),
        // Das Fenster (17 Kanaele) steht als `fenster` daneben — eigene
        // Spalte `window_gz` beim Server (P-6.01), hier nicht doppelt.
        roh: serde_json::json!({
            // 120: traegt die Note; 400: Anflug-Forensik (`anflugpuffer_json`).
            "anflugpuffer": anflugpuffer_json(stats),
            "rollspur": {
                "bahn": stats.runway_match.as_ref().map(|rm| format!("{} {}", rm.airport_ident, rm.runway_ident)),
                "punkte_laengs_quer_m": stats.bahn_spur,
                "bezug_veraltet": stats.bahn_spur_bezug_veraltet,
            },
            "metar": stats.arr_metar_raw,
        }),
    }
}

fn build_landing_record<F>(
    flight: &ActiveFlight,
    stats: &FlightStats,
    sim_kind_label: Option<&str>,
    aircraft_icao: Option<&str>,
    aircraft_title: Option<&str>,
    // v0.15.x: der EFFEKTIVE (tatsächliche) Landeflughafen. Bei einem Divert
    // != flight.arr_airport (= geplant). Wird NUR für den Runway-Geometrie-
    // Trust-Check genutzt, damit eine Divert-Landung am ECHTEN Flughafen
    // (z.B. MGGT statt geplant MZBE) nicht als `icao_mismatch` verworfen wird
    // → Rollout/Runway werden korrekt gewertet (Live-Report Thomas, GLG2).
    // Das Anzeige-`arr_airport` im Record bleibt das geplante Ziel (+ Divert-
    // Banner). Für Nicht-Diverts ist effective == geplant (kein Verhaltens-
    // unterschied).
    effective_arr_icao: &str,
    // Die Rechnung der Einreichung; `None` = selbst rechnen (Live-Vorschau).
    rechnung: Option<&PirepRechnung>,
    airport_lookup: F,
) -> Option<LandingRecord>
where
    F: Fn(&str) -> Option<(f64, f64)>,
{
    if !landungsdatensatz_moeglich(stats) {
        return None;
    }
    let touchdown_at = stats.landing_at?;
    // v0.7.21 (MS713-QS): SSoT-Methode statt direkten Feld-Zugriff.
    // Damit der LandingRecord (= was im Pilot-Client Tab Landung
    // angezeigt wird) UND der Webapp-Live-Map den IDENTISCHEN Edge-Wert
    // sehen — nicht den Streamer-Tick-Snapshot.
    // ⚠ Kein früher Abbruch mehr, wenn die Landung nur nicht MESSBAR war.
    //
    // Codex-Abnahme 12.09.2026: Beide Zeilen brachen den Bau ab, sobald Rate
    // oder Note fehlten — bei einer gesperrten Landung entstand also gar kein
    // Datensatz. Der Pilot sah dann nichts (und beim Speichern kam die
    // irreführende Diagnose "no touchdown captured"), statt der Erklärung,
    // warum es keine Note gibt. Die Landung hat stattgefunden; sie gehört in
    // die Historie, nur eben ohne Zahlen.
    //
    // Ohne Aufsetzzeitpunkt gibt es weiterhin keinen Datensatz — dann fehlt
    // das Ereignis selbst, nicht nur seine Messung.
    let nicht_bewertbar = stats.landung_abdeckung_fehlt.clone();
    let landing_rate_fpm = match stats.canonical_landing_rate_fpm() {
        Some(v) => Some(v),
        None if nicht_bewertbar.is_some() => None,
        None => return None,
    };
    // Ohne Touchdown-Klasse kein Datensatz (ausser „nicht bewertbar") —
    // die Klasse selbst ist seit Score-Version 19 kein Rueckfall mehr.
    if stats.landing_score.is_none() && nicht_bewertbar.is_none() {
        return None;
    }
    // v0.7.1 P1.3-Fix + Round-2 P2-Fix: score_numeric UND score_label
    // beide aus dem Aggregate-Score, damit sie semantisch zueinander
    // passen. Vorher: "SMOOTH · 77/100" (Label aus Touchdown-Klasse,
    // Zahl aus Aggregate) — fuer Pilot verwirrend.
    // Fallback auf Touchdown-Klassifikation wenn keine Sub-Scores
    // berechnet werden konnten (Edge-Case Schutz).
    // v3 (P-4.31): Bewertung nur im WASM-Modul. Beim Einreichen die EINE
    // Rechnung der Einreichung (Codex-Befund zu P-4.40).
    let bewertet = match rechnung {
        Some(r) => r.ergebnis.clone(),
        None => crate::bewertung::bewerten_fuer(&flight.pirep_id, &landungs_bewertungseingabe(flight, stats, aircraft_icao, effective_arr_icao)),
    };
    let computed_sub_scores = bewertet.as_ref().map(|(a, _)| a.teilnoten.clone()).unwrap_or_default();
    let aggregate_master = bewertet.as_ref().and_then(|(a, _)| a.gesamt);
    // Lernpaket AP2: nur wenn die Gesamtnote tatsaechlich gedeckelt ist,
    // traegt der Datensatz den Grund — die Anzeige erklaert damit die Note.
    let score_deckel =
        aggregate_master.and(bewertet.as_ref().and_then(|(a, _)| a.deckel.clone()));
    // Score-Version 19: kein Rueckfall auf die Touchdown-Klasse — sie kennt
    // keinen Deckel (Vorlage Punktesystem, Punkt 7). Ohne Rate liefert
    // `aggregate_master_score` `None` ("lieber gar keine Note als eine
    // geschenkte") — dann keine Bewertung.
    let score_numeric = aggregate_master.map(|m| m as i32);
    let score_label = aggregate_master.map(aggregate_score_label);
    let grade = score_numeric.map(letter_grade);

    // Compute fuel-efficiency once so the Landing tab doesn't have to
    // redo the formula. Same shape as build_pirep_fields.
    let actual_burn = match (stats.takeoff_fuel_kg, stats.landing_fuel_kg) {
        (Some(toff), Some(land)) if toff > land && toff > 0.0 && land >= 0.0 => Some(toff - land),
        _ => None,
    };
    let (fuel_diff_kg, fuel_pct) = match (stats.planned_burn_kg, actual_burn) {
        (Some(plan), Some(actual)) if plan > 0.0 => {
            let d = actual - plan;
            (Some(d), Some((d / plan) * 100.0))
        }
        _ => (None, None),
    };

    let runway_match = stats.runway_match.as_ref().map(|m| {
        // v0.8.0 — Source + NavRunway-Geometrie aus stats holen.
        // Bei OurAirports-Fallback bleiben die nav_*-Felder None.
        let source_wire = stats
            .runway_source
            .map(runway_source_wire)
            .map(|s| s.to_string());
        let nav_g = stats.runway_nav_geometry.as_ref();
        LandingRunwayMatch {
            airport_ident: m.airport_ident.clone(),
            runway_ident: m.runway_ident.clone(),
            surface: m.surface.clone(),
            length_ft: m.length_ft as f64,
            centerline_distance_m: m.centerline_distance_m,
            centerline_distance_abs_ft: m.centerline_distance_abs_ft,
            side: m.side.clone(),
            touchdown_distance_from_threshold_ft: m.touchdown_distance_from_threshold_ft,
            source: source_wire,
            nav_cycle: stats.runway_nav_cycle.clone(),
            true_course_deg: nav_g.map(|g| g.true_course),
            displaced_threshold_ft: wire_displaced_threshold_ft(Some(m)),
            tch_expected_ft: nav_g.map(|g| g.tch_ft),
            glideslope_angle_deg: nav_g.map(|g| g.glideslope_angle),
        }
    });

    // v0.11.2 (Pilotenwunsch ViolonC): touchdown_profile umfasst jetzt
    // Pre-TD (5 s Snapshot-Buffer, schon in stats.touchdown_profile) PLUS
    // bis zu 3 s Post-TD aus stats.post_touchdown_buffer. Pilot sieht im
    // Touchdown-Chart damit auch die Sinkrate-Kurve direkt nach dem
    // Aufsetzen (Strut-Compression / Rebound). Vorher endete das Chart
    // hart am TD-Punkt, was den fachlichen Wunsch „wie schnell hat sich
    // die V/S nach TD beruhigt?" nicht beantwortete.
    //
    // Post-TD-Samples bekommen positive t_ms (TD-Edge ist t_ms = 0),
    // gleicher Berechnungs-Schema wie Pre-TD (siehe lib.rs:16539).
    // v0.12.8: Post-TD-Fenster fürs V/S-Profil auf 10 s erweitert
    // (Pilot-Wunsch). Filtert den `post_touchdown_buffer`; ist der
    // kürzer, werden eben nur die vorhandenen Sekunden gezeigt.
    let touchdown_profile = merge_touchdown_profile(
        &stats.touchdown_profile,
        &stats.post_touchdown_buffer,
        touchdown_at,
        crate::grenzwerte::grenzwerte().landung.post_td_profile_ms.wert,
    );

    // v0.7.1 Phase 1 (P1.1-D + P1.3-C): erweitert um t_ms, agl_ft,
    // is_scored_gate, is_flare. Werte aus dem ApproachBufferSample +
    // landing-scoring Crate Konstanten.
    //
    // Phase 1 nutzt agl_ft direkt (nicht HAT) — `arr_elevation_ft` ist
    // im FlightStats nicht persistiert sondern nur lokal in
    // compute_approach_stability_v2. Phase 3 kann das praezisieren wenn
    // noetig; fuer das Chart-Rendering ist agl_ft genau genug.
    use landing_scoring::gate::{
        STABILITY_GATE_FLARE_CUTOFF_MS, STABILITY_GATE_MAX_HEIGHT_FT, STABILITY_GATE_MIN_HEIGHT_FT,
    };
    let approach_samples: Vec<ApproachSample> = stats
        .approach_buffer
        .iter()
        .map(|s| {
            let height = s.agl_ft;
            let in_height_band =
                height > STABILITY_GATE_MIN_HEIGHT_FT && height <= STABILITY_GATE_MAX_HEIGHT_FT;
            let (t_ms, in_flare) = match stats.landing_at {
                Some(td_ts) => {
                    let dt_ms = (s.at - td_ts).num_milliseconds() as i32;
                    let ms_before_td = -dt_ms as i64;
                    let in_flare =
                        ms_before_td >= 0 && ms_before_td < STABILITY_GATE_FLARE_CUTOFF_MS;
                    (Some(dt_ms), in_flare)
                }
                None => (None, false),
            };
            ApproachSample {
                vs_fpm: s.vs_fpm,
                bank_deg: s.bank_deg,
                t_ms,
                agl_ft: Some(height),
                is_scored_gate: Some(in_height_band && !in_flare),
                is_flare: Some(in_flare),
                gs_kt: Some(s.gs_kt),
            }
        })
        .collect();
    // v0.7.1 Round-2 P2-Fix: gate_window VOR dem LandingRecord-Init
    // berechnen damit approach_samples noch verfügbar ist (wird unten
    // ins Record gemoved).
    let gate_window = build_storage_gate_window(&approach_samples);

    // v0.7.18 (B-012): Touchdown-Airport-Resolution einmal berechnen
    // und in den Record einsetzen.
    let td_resolution = resolve_touchdown_airport(
        stats.runway_match.as_ref(),
        stats.landing_lat,
        stats.landing_lon,
        &flight.arr_airport,
        &airport_lookup,
    );

    // v0.8.0: Pure-Function-Assessment einmal rechnen, in den Record
    // einsetzen.
    let assessed = assess_touchdown(stats);

    // v1.7.0 Bahndisziplin — an einer Stelle abgeleitet, siehe `bahn_felder`.
    let bahn = bahn_felder(stats, aircraft_icao, bahn_skip_grund(&computed_sub_scores));
    // v3 (P-4.21): Soll-Band aus dem Modul, ueber dieselben Proben und
    // denselben Gleitwinkel, die die Anfluggrafik zeigt.
    let modul_sollband = crate::bewertung::sollband(
        &flight.pirep_id,
        &approach_samples
            .iter()
            .map(|p| landing_scoring::sollband::Probe {
                gs_kt: p.gs_kt.map(f64::from),
                is_flare: p.is_flare,
                t_ms: p.t_ms.map(f64::from),
            })
            .collect::<Vec<_>>(),
        runway_match.as_ref().and_then(|m| m.glideslope_angle_deg),
    );

    Some(LandingRecord {
        // ⚠ Muss mit — ohne ihn zeichnet der Client die Spur falsch,
        // sobald eine versetzte Schwelle im Spiel ist.
        spur_nullpunkt_versatz_m: bahn.spur_nullpunkt_versatz_m,
        clearance_point_m: bahn.clearance_point_m,
        scoring_cutoff_m: bahn.scoring_cutoff_m,
        mess_ende_laengs_m: bahn.mess_ende_laengs_m,
        lateral_skip_reason: bahn.lateral_skip_reason.clone(),
        runway_exits: bahn.runway_exits.clone(),
        clearance_exit: bahn.clearance_exit.clone().flatten(),
        runway_exits_quelle: bahn.runway_exits_quelle.clone(),
        clearance_speed_kt: bahn.clearance_speed_kt,
        clearance_side: bahn.clearance_side,
        track_width_m: bahn.track_width_m,
        track_width_source: bahn.track_width_source,
        wingspan_m: bahn.wingspan_m,
        runway_width_m: bahn.runway_width_m,
        min_edge_clearance_m: bahn.min_edge_clearance_m,
        max_lateral_offset_m: bahn.max_lateral_offset_m,
        lateral_samples: bahn.lateral_samples,
        vorherige_durchgaenge: bahn.vorherige_durchgaenge,
        surface_paved: bahn.surface_paved,
        overrun_m: bahn.overrun_m,
        pirep_id: flight.pirep_id.clone(),
        touchdown_at,
        recorded_at: Utc::now(),
        // Prefer `bid_callsign` over the raw `flight_number` — see the
        // `callsign` doc-comment on `ActiveFlightInfo` for the full story
        // (Personal/Free-Flight bookings with `flight_number: 0`).
        flight_number: resolve_flight_ident(&flight.flight_number, flight.bid_callsign.as_deref()),
        airline_icao: flight.airline_icao.clone(),
        dpt_airport: flight.dpt_airport.clone(),
        arr_airport: flight.arr_airport.clone(),
        // v0.7.18 (B-012): Touchdown-Airport-Resolution-Werte werden
        // ueber die `td_resolution`-Bindung weiter unten in der `..` -
        // Erweiterung gesetzt. Hier nur Hauptfelder; siehe unten.
        touchdown_airport: Some(td_resolution.icao.clone()),
        touchdown_airport_source: Some(td_resolution.source.as_wire_str().to_string()),
        touchdown_distance_to_destination_nm: td_resolution.distance_to_destination_nm,
        touchdown_nearest_distance_nm: td_resolution.nearest_distance_nm,
        aircraft_registration: Some(flight.planned_registration.clone()).filter(|s| !s.is_empty()),
        aircraft_icao: aircraft_icao
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty()),
        aircraft_title: aircraft_title
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty()),
        sim_kind: sim_kind_label.map(|s| s.to_string()),

        score_numeric,
        score_label: score_label.map(|s| s.to_string()),
        grade_letter: grade.map(|g| g.to_string()),
        score_deckel,

        landing_rate_fpm,
        landing_peak_vs_fpm: stats.landing_peak_vs_fpm,
        // Auch die rohen G-Werte stammen aus dem Aufsetzfenster. Ohne sie
        // fällt im Client zusätzlich der `scoreG()`-Rückfall weg, der sonst
        // ein "Hard G"-Flag für eine ungemessene Landung erzeugte.
        landing_g_force: aus_dem_fenster(stats, stats.landing_g_force),
        landing_peak_g_force: aus_dem_fenster(stats, stats.landing_peak_g_force),
        landing_pitch_deg: stats.landing_pitch_deg,
        // Bank at touchdown isn't a top-level FlightStats field; pull
        // the sample closest to t=0 from the touchdown profile.
        landing_bank_deg: aufsetz_bank_deg(stats),
        landing_speed_kt: stats.landing_speed_kt,
        landing_heading_deg: stats.landing_heading_deg,
        landing_weight_kg: stats.landing_weight_kg,
        touchdown_sideslip_deg: stats.touchdown_sideslip_deg,
        bounce_count: stats.bounce_count,

        headwind_kt: stats.landing_headwind_kt,
        crosswind_kt: stats.landing_crosswind_kt,

        approach_vs_stddev_fpm: stats.canonical_vs_stddev_fpm(),
        approach_bank_stddev_deg: stats.canonical_bank_stddev_deg(),
        rollout_distance_m: stats.rollout_distance_m,

        planned_block_fuel_kg: stats.planned_block_fuel_kg,
        planned_burn_kg: stats.planned_burn_kg,
        planned_tow_kg: stats.planned_tow_kg,
        planned_ldw_kg: stats.planned_ldw_kg,
        planned_zfw_kg: stats.planned_zfw_kg,
        actual_trip_burn_kg: actual_burn,
        fuel_efficiency_kg_diff: fuel_diff_kg,
        fuel_efficiency_pct: fuel_pct,
        // Eingefroren in `sprit_auswertung_einmal` — hier nur gelesen.
        sprit: stats.sprit_auswertung.clone(),
        takeoff_weight_kg: stats.takeoff_weight_kg,
        takeoff_fuel_kg: stats.takeoff_fuel_kg,
        landing_fuel_kg: stats.landing_fuel_kg,
        block_fuel_kg: stats.block_fuel_kg,

        runway_match,
        touchdown_profile,
        approach_samples,

        // v0.5.43: Forensik aus dem 50-Hz-Sampler-Buffer (compute_landing_analysis)
        // wenn der Buffer-Dump erfolgreich war. Sonst alle None.
        //
        // v0.20.2: unplausible Edge-Werte werden hier NICHT durchgereicht. Die
        // Kanonik verwirft sie zwar fuer Score, Log und PIREP — aber die
        // Forensik-Kachel im Client und die Webapp lesen `vs_at_edge_fpm`
        // direkt (`scoreBasisVs()` nimmt es sogar als erste Wahl). Bei Flug 804
        // (ELLX) haette die Forensik also weiter "+24 fpm" als Aufsetz-Sinkrate
        // gezeigt, waehrend die Kachel daneben -38 sagt. Zwei Zahlen fuer
        // denselben Touchdown — genau der Riss, den wir ausraeumen. Ist der
        // Wert kein Flugzustand, gibt es ihn nicht.
        // Auch dieser Wert stammt aus dem Aufsetzfenster. Reicht dessen
        // Abdeckung nicht, gibt es ihn nicht — sonst zeigte die Forensik-
        // Kachel eine Zahl, während die Bewertung daneben "nicht bewertbar"
        // sagt. Dieselbe Regel wie bei einem unplausiblen Wert eine Zeile
        // tiefer: Ist er keine Messung, gibt es ihn nicht.
        landung_nicht_bewertbar: landung_nicht_bewertbar_json(stats),
        fenster_unzureichend: stats.landung_abdeckung_fehlt.is_some(),
        sampler_diagnose: stats.sampler_diagnose.clone(),
        vs_at_edge_fpm: ana_f32(&stats.landing_analysis, "vs_at_edge_fpm")
            .filter(|&v| landing_rate_is_plausible(v))
            .filter(|_| stats.landung_abdeckung_fehlt.is_none()),
        // Die Quelle faellt mit dem Wert. Wird `vs_at_edge_fpm` als
        // unplausibel verworfen, kommt die angezeigte Zahl aus der
        // Rueckfallkaskade — dann darf die Kachel kein Messverfahren
        // nennen, das zu dieser Zahl gar nicht gehoert (QS-Befund v1.6.3).
        vs_at_edge_quelle: ana_f32(&stats.landing_analysis, "vs_at_edge_fpm")
            .filter(|&v| landing_rate_is_plausible(v))
            .filter(|_| stats.landung_abdeckung_fehlt.is_none())
            .and_then(|_| {
                stats
                    .landing_analysis
                    .as_ref()
                    .and_then(|a| a.get("vs_at_edge_quelle"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            }),
        vs_simvar_edge_fpm: ana_f32(&stats.landing_analysis, "vs_simvar_edge_fpm"),
        // v1.6.9 — Bestandteile der Sinkrate + Referenz des Simulators.
        vs_gelaende_fpm: ana_f32(&stats.landing_analysis, "vs_gelaende_fpm"),
        vs_eigensinken_fpm: ana_f32(&stats.landing_analysis, "vs_eigensinken_fpm"),
        vs_sim_referenz_fpm: ana_f32(&stats.landing_analysis, "vs_sim_referenz_fpm"),
        vs_smoothed_250ms_fpm: ana_f32(&stats.landing_analysis, "vs_smoothed_250ms_fpm"),
        vs_smoothed_500ms_fpm: ana_f32(&stats.landing_analysis, "vs_smoothed_500ms_fpm"),
        vs_smoothed_1000ms_fpm: ana_f32(&stats.landing_analysis, "vs_smoothed_1000ms_fpm"),
        vs_smoothed_1500ms_fpm: ana_f32(&stats.landing_analysis, "vs_smoothed_1500ms_fpm"),
        peak_g_post_500ms: aus_dem_fenster(
            stats,
            ana_f32(&stats.landing_analysis, "peak_g_post_500ms"),
        ),
        peak_g_post_1000ms: aus_dem_fenster(
            stats,
            ana_f32(&stats.landing_analysis, "peak_g_post_1000ms"),
        ),
        // v0.12.3 (LE4/LE7): gescorter G (EMA, sonst raw_fallback) + Methode
        // — die G-Force-Card headlinet diesen Wert.
        landing_scored_g_force: score_g_for_stats(stats).map(|s| s.scored_g),
        scored_g_method: score_g_for_stats(stats).map(|s| s.method.as_str().to_string()),
        // v0.7.17 (B-009): G-Force-Forensik
        g_at_edge: aus_dem_fenster(stats, ana_f32(&stats.landing_analysis, "g_at_edge")),
        g_smoothed_250ms_post: aus_dem_fenster(
            stats,
            ana_f32(&stats.landing_analysis, "g_smoothed_250ms_post"),
        ),
        g_median_post_500ms: aus_dem_fenster(
            stats,
            ana_f32(&stats.landing_analysis, "g_median_post_500ms"),
        ),
        g_p95_post_500ms: aus_dem_fenster(
            stats,
            ana_f32(&stats.landing_analysis, "g_p95_post_500ms"),
        ),
        max_gear_force_n: aus_dem_fenster(
            stats,
            ana_f32(&stats.landing_analysis, "max_gear_force_n"),
        ),
        peak_vs_pre_flare_fpm: ana_f32(&stats.landing_analysis, "peak_vs_pre_flare_fpm"),
        vs_at_flare_end_fpm: ana_f32(&stats.landing_analysis, "vs_at_flare_end_fpm"),
        flare_reduction_fpm: ana_f32(&stats.landing_analysis, "flare_reduction_fpm"),
        flare_dvs_dt_fpm_per_sec: ana_f32(&stats.landing_analysis, "flare_dvs_dt_fpm_per_sec"),
        flare_quality_score: ana_i32(&stats.landing_analysis, "flare_quality_score"),
        flare_detected: ana_bool(&stats.landing_analysis, "flare_detected"),
        forensic_sample_count: ana_u32(&stats.landing_analysis, "sample_count"),

        // ─── v0.8.3 (#8) — Forensische Bounce-Counts ins LandingRecord ───
        // Damit das UI „Light bounce X ft erkannt (score-frei)" zeigen
        // kann auch bei 5-14 ft Hopsern, die bis Lernpaket AP3 (29.09.2026)
        // score-frei waren; seitdem zaehlen sie ab 5 ft auch in der Note.
        // Reported 2026-05-14 Adrian: Touchdown #167 hatte
        // bounce_max_agl_ft=14.05, aber UI zeigte bounce_count=0 →
        // Pilot dachte "nicht erkannt".
        bounce_max_agl_ft: ana_f32(&stats.landing_analysis, "bounce_max_agl_ft"),
        forensic_bounce_count: ana_u32(&stats.landing_analysis, "forensic_bounce_count")
            .map(|n| n.min(u8::MAX as u32) as u8),
        scored_bounce_count: ana_u32(&stats.landing_analysis, "scored_bounce_count")
            .map(|n| n.min(u8::MAX as u32) as u8),
        scored_light_bounce_count: ana_u32(&stats.landing_analysis, "scored_light_bounce_count")
            .map(|n| n.min(u8::MAX as u32) as u8),

        // ─── v0.7.1 Felder (Spec docs/spec/v0.7.1-landing-ux-fairness.md §5) ──
        // Phase 1 = backbone: Felder durchreichen, kein Pilot-sichtbares
        // Verhalten geaendert. Phase 2/3 konsumieren sie.
        ux_version: 1, // v0.7.1+ marker — UI nutzt sub_scores statt LegacyPirepNotice
        forensics_version: touchdown_v2::FORENSICS_VERSION,
        landing_confidence: stats.landing_confidence.clone(),
        landing_source: stats.landing_source.clone(),
        // F7 (P2.1-A): bestehende Backend-Felder exponieren — keine
        // neue Berechnung. Phase 3 wird sub_stability Voting umstellen.
        approach_vs_jerk_fpm: stats.approach_vs_jerk_fpm,
        approach_ias_stddev_kt: stats.approach_ias_stddev_kt,
        approach_stable_config: stats.approach_stable_config,
        // v0.19.x FIX: FlightStats::approach_excessive_sink is now
        // Option<bool> like every sibling field here — None means "gate
        // had too few samples to measure", not "measured, no excess sink".
        approach_excessive_sink: stats.approach_excessive_sink,
        // v0.7.1 Round-2 P2-Fix: gate_window oben berechnet aus den
        // approach_samples — UI-Tooltip kann jetzt die echten
        // Boundaries zeigen statt nur die Crate-Konstanten.
        gate_window,
        // v0.11.0-dev: 3 weitere Stability-Werte ins Storage durchreichen.
        // Werden vom Stability-v2-Compute schon gefuellt (lib.rs:16699-16708)
        // und im PIREP/MQTT mitgeschickt — Storage-LandingRecord hatte sie
        // bisher nicht, die neue ApproachStabilityCard im Pilot-Client
        // braucht sie aber. Alte landing_history.json bleibt via
        // serde(default) lesbar.
        approach_vs_deviation_fpm: stats.approach_vs_deviation_fpm,
        approach_max_vs_deviation_below_500_fpm: stats.approach_max_vs_deviation_below_500_fpm,
        approach_used_hat: Some(stats.approach_used_hat),
        // P1.5 + Phase 2 F1/F2/F3 + P1.3-Fix: sub_scores wurden oben
        // schon berechnet damit aggregate_master_score → score_numeric
        // gehen kann. Hier nur noch durchreichen, NICHT erneut rechnen.
        sub_scores: computed_sub_scores,
        // v0.7.6 P1-3: Runway-Geometry-Trust ins LandingRecord. Damit
        // der Pilot-Client (LandingPanel.tsx) die TD-Zone und Float-
        // Distance bei untrusted geometry ausblenden kann — gleiche
        // Logik wie Web-Dashboard. Spec docs/spec/v0.7.6-landing-
        // payload-consistency.md §3 P1-3.
        runway_geometry_trusted: {
            let (trusted, _) = runway_geometry_trust_check(
                stats
                    .runway_match
                    .as_ref()
                    .map(|m| m.airport_ident.as_str()),
                effective_arr_icao,
                stats
                    .divert_hint
                    .as_ref()
                    .and_then(|h| h.actual_icao.as_deref()),
                stats
                    .runway_match
                    .as_ref()
                    .map(|m| m.centerline_distance_m as f32),
                stats.landing_float_distance_m,
            );
            Some(trusted)
        },
        runway_geometry_reason: {
            let (_, reason) = runway_geometry_trust_check(
                stats
                    .runway_match
                    .as_ref()
                    .map(|m| m.airport_ident.as_str()),
                effective_arr_icao,
                stats
                    .divert_hint
                    .as_ref()
                    .and_then(|h| h.actual_icao.as_deref()),
                stats
                    .runway_match
                    .as_ref()
                    .map(|m| m.centerline_distance_m as f32),
                stats.landing_float_distance_m,
            );
            reason.map(String::from)
        },
        // v0.7.19 GAF-707 Accident-Detection. Felder kommen aus dem
        // gelatchten FlightStats — gesetzt entweder durch Sim-Event-
        // Pfad (mid-air) oder durch Heuristik am Touchdown-Edge.
        accident: stats.accident_detected,
        accident_kind: stats.accident_kind.clone(),
        accident_confidence: stats.accident_confidence.clone(),
        accident_reasons: stats.accident_reasons.clone(),
        accident_at: stats.accident_at,

        // v0.8.0 VPS-Navdata + Runway-Awareness — Pure-Function-
        // Assessment-Werte. Wenn der runway_match aus Navigraph kam
        // sind ALLE Felder gefüllt; bei OurAirports-Fallback ist
        // TDZ/Aim/td_distance trotzdem gesetzt (length + td_distance
        // sind quell-agnostisch), nur DDS bleibt None weil
        // displaced_threshold_ft nicht in OurAirports vorkommt.
        // TCH bleibt für jetzt None — braucht AGL@threshold-crossing
        // Scan, kommt in Folge-Slice.
        td_distance_from_threshold_m: assessed.td_distance_from_threshold_m,
        td_in_tdz: assessed.tdz.map(|t| t.in_tdz),
        td_third: assessed.tdz.map(|t| t.third),
        td_tdz_length_m: assessed.tdz.map(|t| t.tdz_length_m),
        aim_delta_m: assessed.aim.as_ref().map(|a| a.delta_m),
        aim_class: assessed
            .aim
            .as_ref()
            .map(|a| aim_class_wire(a.class).to_string()),
        aim_point_m: assessed.aim.as_ref().map(|a| a.aim_point_m),
        tch_actual_ft: assessed.tch.as_ref().map(|t| t.actual_ft),
        tch_delta_ft: assessed.tch.as_ref().map(|t| t.delta_ft),
        tch_class: assessed
            .tch
            .as_ref()
            .map(|t| tch_class_wire(t.class).to_string()),
        tch_rad_ft: assessed.tch.as_ref().map(|t| t.rad_ft),
        tch_hoehengruppe: assessed.tch.as_ref().map(|t| t.gruppe.nummer()),
        pre_displaced_threshold: assessed.dds.map(|d| d.in_pre_threshold_zone),
        // v0.10.0 (#runway-utilization-score): markiert dass dieses
        // Record mit dem LDA-basierten Bahn-Auslastungs-Score gebaut
        // wurde. UI (LandingPanel.tsx) gated darauf das Rendering der
        // neuen `extra`-Lines + erweiterten Rationale-/Warning-Keys.
        // v0.16.21: bump 3→4 — MSFS touchdown V/S SimVar-lag corrected.
        // v0.20.x: bump 4→5 — Bahnauslastung-QS (Float-Toleranz +
        // Banding grosszuegiger) + Sinkraten-Ziel-Korridor.
        // v1.6.7: bump 7→8 — Minderverbrauch bis 15 % voll bepunktet;
        // Bahn-Auslastung auf die echte genutzte Strecke umgestellt.
        score_algorithm_version: Some(SCORE_ALGORITHMUS_VERSION),
        bewertung_status: Some(crate::bewertung::status_text(&bewertet).to_string()),
        // v3 (P-4.21): Anzeige-Grenzen und Soll-Band aus dem Modul.
        modul_grenzen: bewertet.as_ref().map(|(a, _)| a.grenzen.clone()),
        modul_sollband,
        // Lernpaket AP4/AP5 (29.09.2026): reine Forensik, nur lokal.
        anflug_gleitpfad: stats.anflug_forensik.gleitpfad.clone(),
        anflug_ruhe: stats.anflug_forensik.ruhe.clone(),
        abfangen: abfangen_aus_analyse(&stats),
        // Score-Version 19: dieselben Quellen wie im Touchdown-Payload, damit
        // Client und Webapp dieselben Werte zeigen.
        landing_groundspeed_kt: stats.landing_groundspeed_kt,
        landing_heading_true_deg: stats.landing_heading_true_deg,
        go_around_count: Some(stats.go_around_count),
        landing_wing_strike_severity_pct: stats.landing_wing_strike_severity_pct,
        landing_touchdown_zone: zone_fuer_leitung(flight, stats),
        landing_float_distance_m: stats.landing_float_distance_m,
        landing_vref_deviation_kt: stats.landing_vref_deviation_kt,
        landing_vref_source: stats.landing_vref_source.map(|s| s.to_string()),
        landing_vref_toleranz_kt: stats.landing_vref_toleranz_kt,
        landing_yaw_rate_deg_per_sec: stats.landing_yaw_rate_deg_per_sec,
        landing_brake_energy_proxy: stats.landing_brake_energy_proxy,
        landing_decel_mps2: stats.landing_decel_mps2,
        arr_metar: stats.arr_metar_raw.clone().filter(|m| !m.is_empty()),
        client_version: Some(aufzeichnende_client_version(stats)),
        landing_lat: stats.landing_lat,
        landing_lon: stats.landing_lon,
        approach_runway_changed_late: Some(stats.approach_runway_changed_late),
        approach_stable_at_da: stats.approach_stable_at_da,
        approach_stall_warning_count: Some(stats.approach_stall_warning_count),
        approach_wind: stats.approach_wind.clone(),
        approach_spaetes_final_ab_ft: stats.approach_spaetes_final.map(|sf| sf.ab_hoehe_ft),
        approach_spaetes_final_kurve_deg: stats.approach_spaetes_final.map(|sf| sf.kurve_deg),
        approach_spaetes_final_querneigung_unter_300_deg: stats
            .approach_spaetes_final
            .map(|sf| sf.querneigung_max_unter_300_deg),
        approach_spaetes_final_rechtzeitig: stats.approach_spaetes_final.map(|sf| sf.rechtzeitig),
        flugzeiten: Some(flugzeiten(stats)),
    })
}

/// Persist a landing record from a flight that just filed its PIREP.
/// Best-effort — failure is logged and ignored so the file path
/// doesn't fail because the history disk write blew up.
///
/// ⚠ Nimmt `flight.stats` SELBST, und nur zum Bauen des Datensatzes
/// (v2.0.18). Bis 2.0.17 hielten die Aufrufer die Sperre über alles:
/// Simulator abfragen, Flughafen-Liste kopieren, Datensatz bauen, die ganze
/// Landungs-Historie (bis 500 Landungen) lesen und neu schreiben, loggen.
/// Der Wächter meldete das bei jedem Flug mit 140 bis 157 ms (DLH 440/441,
/// 14.10.2026). Jetzt: Simulator und Flughafen-Liste vorher, Datensatz unter
/// der Sperre, Schreiben und Log danach.
///
/// ⚠ Aufrufer dürfen `flight.stats` dabei NICHT halten — die Sperre ist nicht
/// wiedereintrittsfähig, der Aufruf bliebe für immer stehen.
/// Ein Eingabe-Dokument: Art, Eingabe, Fenster, Begleitung (meta, Herkunft
/// P-9.44, roh — Kap. 5.2) und — fuer die Landung — das Ergebnis der
/// Einreichung. `None` = noch zu rechnen (Touch-and-Go).
type EingabeTeil = (
    landungs_eingabe::Art,
    landing_scoring::LandingScoringInput,
    Vec<TouchdownWindowSample>,
    landungs_eingabe::Begleitung,
    Option<Option<(landing_scoring::wasm::Ausgabe, crate::bewertung::Status)>>,
);

/// Die Eingabe-Dokumente einer Einreichung (P-7.02, P-4.13): erst die
/// eingefrorenen Touch-and-Go (seq 1 …), dann die letzte Landung. Ohne
/// Fenster kein Dokument — der Server verlangt mindestens eine Probe (nach
/// einem Neustart ist es leer). Die Landung traegt Eingabe UND Ergebnis der
/// Rechnung der Einreichung: Der Server rechnet mit genau dieser Eingabe nach.
fn eingabe_teile(
    flight: &ActiveFlight,
    stats: &FlightStats,
    mit_landung: bool,
    rechnung: PirepRechnung,
) -> Vec<EingabeTeil> {
    let mut teile: Vec<EingabeTeil> = stats
        .aufsetzer_eingefroren
        .iter()
        .map(|(e, f, h)| (landungs_eingabe::Art::TouchAndGo, e.clone(), f.clone(), h.clone(), None))
        .collect();
    if mit_landung {
        if stats.landung_fenster.is_empty() {
            tracing::info!(pirep_id = %flight.pirep_id, "kein Aufsetzfenster im Speicher — kein Eingabe-Dokument der Landung");
        } else {
            // Dasselbe Muster wie `pirep_rechnung_bilden`.
            let muster = muster_fuer_landung(stats, &flight.aircraft_icao);
            let begleitung = landungs_begleitung(flight, stats, &rechnung.eingabe, muster, &rechnung.arr);
            teile.push((
                landungs_eingabe::Art::Landung,
                rechnung.eingabe,
                stats.landung_fenster.clone(),
                begleitung,
                Some(rechnung.ergebnis),
            ));
        }
    }
    teile
}

fn record_landing_for_filed_flight(
    app: &AppHandle,
    flight: &ActiveFlight,
    // v0.15.x: tatsächlicher Landeflughafen (Divert-aware) — siehe
    // build_landing_record. Für Nicht-Diverts == flight.arr_airport.
    effective_arr_icao: &str,
) {
    // ── Vor der Sperre: alles, was nicht aus den Flugdaten kommt ─────────
    let snapshot = current_snapshot(app);
    let sim_kind_label = read_sim_config(app).kind;
    let sim_label = match sim_kind_label {
        SimKind::Off => None,
        SimKind::Msfs2020 | SimKind::Msfs2024 => Some("MSFS"),
        SimKind::XPlane11 | SimKind::XPlane12 => Some("X-PLANE"),
    };
    // v0.7.17 (B-015): Snapshot kann beim Filing None sein (Sim
    // disconnected, X-Plane Web API aus, etc.). Vorher: aircraft_icao
    // fiel dann ebenfalls None → sub_rollout-Score nutzte konservative
    // Light-GA-Schwellen → A320 mit 1096 m Rollout bekam fälschlich
    // 80 PTS (good_stop) statt 100 PTS (excellent_stop). Fallback auf
    // Bid-Wert (`flight.aircraft_icao`) der beim Flugstart aus dem
    // phpVMS-Bid persistiert wurde und über die gesamte Flight-Dauer
    // stabil bleibt. Webapp benutzt denselben Wert → konsistent.
    let aircraft_title = snapshot.as_ref().and_then(|s| s.aircraft_title.as_deref());

    // v0.7.18 (B-012): airport_lookup-Closure aus dem AppState.airports-Cache.
    let airport_lookup_data: std::collections::HashMap<String, (f64, f64)> = {
        let app_state = app.state::<AppState>();
        let guard = app_state.airports.lock().expect("airports lock");
        guard
            .iter()
            .filter_map(|(k, v)| match (v.lat, v.lon) {
                (Some(la), Some(lo)) => Some((k.clone(), (la, lo))),
                _ => None,
            })
            .collect()
    };
    let airport_lookup = |icao: &str| airport_lookup_data.get(&icao.to_uppercase()).copied();

    // ── Unter der Sperre: nur Flugdaten lesen und den Datensatz bauen ────
    let record = {
        let mut gehalten = flight.stats.lock().expect("flight stats");
        let stats: &mut FlightStats = &mut gehalten;
        // v0.16.24: finalize-with-best — re-correlate the runway against the
        // (now-hopefully-warm) navdata cache and write the upgraded fields back
        // BEFORE building the record, so build_landing_record + all downstream
        // consumers (assess_touchdown, the TouchdownPayload, the score inputs)
        // see one consistent set. NON-RACY: this is a synchronous read of the
        // current cache contents — no async re-correlation races the filing.
        // ON-PLAN INVARIANT: when the touchdown match was already Navigraph
        // (non-divert), `finalize_runway_correlation` returns the stamped values
        // verbatim, so this write-back is a byte-identical no-op.
        //
        // v0.16.24 (QS-6): the PIREP score/body callers (`flight_end`,
        // `flight_end_manual`) now run `apply_finalized_runway_correlation` BEFORE
        // building the body, so by the time we get here the upgrade has usually
        // already happened — this second call is then the idempotent no-op
        // described above. Kept so `record_landing_for_filed_flight` stays correct
        // standalone (the LandingRecord is finalized regardless of caller).
        apply_finalized_runway_correlation(flight, stats);

        // Sim → Buchung → Flugzeugtitel, an EINER Stelle (siehe
        // `sim_core::muster_aufloesen`). Diese Kette stand hier und an einer
        // zweiten Stelle wortgleich, waehrend drei weitere Stellen kuerzere
        // Ketten benutzten — daran ist EWG248 gescheitert.
        let aufgeloest = stats.aufgeloestes_muster.clone().or_else(|| {
            sim_core::muster_aufloesen(
                snapshot.as_ref().and_then(|s| s.aircraft_icao.as_deref()),
                &flight.aircraft_icao,
                aircraft_title,
            )
        });
        let aircraft_icao = aufgeloest.as_deref();

        // Codex-Befund zu P-4.40: die Rechnung dieser Einreichung — dieselbe,
        // die phpVMS bekam. Datensatz und Dokument lesen sie.
        let rechnung = einreichungs_rechnung(flight, stats, effective_arr_icao);
        let record = build_landing_record(
            flight,
            stats,
            sim_label,
            aircraft_icao,
            aircraft_title,
            effective_arr_icao,
            Some(&rechnung),
            airport_lookup,
        );
        // v3 (P-7.02): Eingabe und Fenster derselben Landung (dieselbe
        // Rechnung wie der Datensatz), siehe `eingabe_teile`.
        let teile = eingabe_teile(flight, stats, record.is_some(), rechnung);
        (record, teile)
    };
    let (record, teile) = record;
    // Bewertung fuer die Dokumente AUSSERHALB der Sperre der Flugdaten.
    let konto = upload_konto(app);
    if let (Some(konto), Ok(daten)) = (konto, app.path().app_data_dir()) {
        let mut abgelegt = false;
        for (i, (art, eingabe, fenster, begleitung, ergebnis)) in teile.iter().enumerate() {
            let seq = i as u32 + 1;
            // Touch-and-Go: eigene Landung, eigene Rechnung — ausserhalb der
            // Sperre. Die Landung bringt ihr Ergebnis aus der Einreichung mit.
            let ergebnis = match ergebnis {
                Some(e) => e.clone(),
                None => crate::bewertung::bewerten_fuer(&flight.pirep_id, eingabe),
            };
            let doc = landungs_eingabe::dokument(
                &flight.pirep_id,
                seq,
                *art,
                laufzeit_paket::flug_pin(&flight.pirep_id).map(|id| id.to_string()),
                eingabe,
                begleitung,
                fenster,
                &ergebnis,
            );
            match landungs_eingabe::einreihen(&daten, &konto, &flight.pirep_id, seq, &doc) {
                Ok(pfad) => {
                    abgelegt = true;
                    // v3 (P-5.01): Hash und Groesse je `seq` ins Flugprotokoll.
                    record_event(
                        app,
                        &flight.pirep_id,
                        &FlightLogEvent::BewertungEingabe {
                            timestamp: Utc::now(),
                            seq,
                            sha256: doc["eingabe"]["meta"]["input_sha256"].as_str().unwrap_or_default().to_string(),
                            groesse: std::fs::metadata(&pfad).map(|m| m.len()).unwrap_or(0),
                        },
                    );
                }
                Err(e) => tracing::warn!(error = %e, seq, "Eingabe-Dokument nicht ablegbar"),
            }
        }
        if abgelegt {
            upload_warteschlange_anstossen();
        }
    }

    // ── Nach der Sperre: Historie schreiben und loggen ───────────────────
    let Some(record) = record else {
        tracing::debug!(
            pirep_id = %flight.pirep_id,
            "no touchdown captured — skipping landing-history record"
        );
        return;
    };
    let Some(store) = open_landing_store(app) else {
        tracing::warn!("landing store unavailable — landing not recorded");
        return;
    };
    if let Err(e) = store.upsert(record) {
        tracing::warn!(error = ?e, "could not persist landing record");
    } else {
        tracing::info!(pirep_id = %flight.pirep_id, "landing record persisted");
        // Erst nach erfolgreichem Schreiben sichern — sonst laedt der Upload
        // einen Bestand hoch, der die eben geflogene Landung gar nicht enthaelt.
        spawn_landing_backup(app);
    }
}

/// Landungs-Sicherung auf dem Live-Server.
///
/// Gesichert werden NUR die Kennzahlen — die Messkurven bleiben draussen
/// (siehe `aeroacars_mqtt::backup`): Sie sind 95 % der Datenmenge UND liegen
/// als Flug-Aufzeichnung ohnehin schon auf dem Server.
///
/// Kein Ein/Aus-Schalter: Einstellungen leben im Frontend-Speicher und
/// erreichen diesen Pfad nicht, der nach einer Landung im Hintergrund
/// laeuft. Ein halber Schalter, der den Automatiklauf nicht abschaltet,
/// waere schlechter als keiner. Inhaltlich unbedenklich, weil der Server
/// dieselben Landungen ueber die PIREPs ohnehin kennt.
async fn upload_landing_backup(app: AppHandle) -> Result<usize, String> {
    let Some(token) = secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten() else {
        return Err("kein Live-Server-Zugang (nicht provisioniert)".into());
    };
    let Some(store) = open_landing_store(&app) else {
        return Err("Landungsspeicher nicht verfuegbar".into());
    };

    // v0.19.x FIX: `put_landings` REPLACES the server's backup — it is not
    // a merge. `restore_landing_backup` documents "merge before every
    // upload so a second computer's landings aren't overwritten" as an
    // existing safety invariant, but that merge only ever ran on the
    // download/restore path. A client whose local store is missing
    // records for any reason — a failed automatic restore-on-login
    // (App.tsx swallows that error on purpose, since the backup is a
    // nice-to-have, not a login requirement), a fresh install, a deleted
    // landings.json — would silently overwrite the server's accumulated
    // history with its own incomplete set the next time a landing
    // triggers a backup. Merging the server's current state in FIRST
    // makes the upload a strict superset of what the server already had,
    // so replacing the server's state with it can never lose anything.
    // A GET failure (as opposed to "no backup yet") means we cannot prove
    // that, so the whole backup is skipped rather than risking data loss
    // — the next landing retries.
    match aeroacars_mqtt::backup::get_landings(None, &token).await {
        Ok(Some(payload)) => {
            let incoming: Vec<storage::LandingRecord> = payload
                .landings
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect();
            // v0.20.x QS fix: `merge_from` is a plain union with no delete
            // concept — without this, a record the pilot deleted locally
            // (landing_delete only ever touches the local file, never the
            // server) would be silently resurrected right here, since the
            // server's backup still has the old copy. Filter it out first.
            let tombstoned = open_deleted_landings_tombstone(&app)
                .and_then(|t| t.read_all().ok())
                .unwrap_or_default();
            let incoming = storage::filter_tombstoned(incoming, &tombstoned);
            store.merge_from(incoming).map_err(|e| e.to_string())?;
        }
        Ok(None) => {
            // No server backup yet — nothing to merge, safe to proceed
            // (this is the normal first-ever-upload case).
        }
        Err(e) => {
            return Err(format!(
                "Serverstand konnte vor der Sicherung nicht geprüft werden, \
                 Sicherung übersprungen (Datenverlustschutz): {e}"
            ));
        }
    }

    let records = store.list().map_err(|e| e.to_string())?;
    let stripped: Vec<serde_json::Value> = records
        .iter()
        .filter_map(|r| serde_json::to_value(r).ok())
        .map(aeroacars_mqtt::backup::strip_curves)
        .collect();

    let res = aeroacars_mqtt::backup::put_landings(None, &token, &stripped)
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(
        count = res.count,
        bytes = res.bytes,
        "landing backup uploaded"
    );
    Ok(res.count)
}

/// Serverstand holen und in den lokalen Bestand einmischen.
///
/// Bewusst ZUSAMMENFUEHREN statt ersetzen: Wer abwechselnd an zwei Rechnern
/// fliegt, verlöre sonst die Landungen des jeweils anderen. Der Serverstand
/// kann zudem aelter sein als der lokale — er darf nichts ueberschreiben.
async fn restore_landing_backup(app: AppHandle) -> Result<usize, String> {
    let Some(token) = secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten() else {
        return Err("kein Live-Server-Zugang (nicht provisioniert)".into());
    };
    let Some(store) = open_landing_store(&app) else {
        return Err("Landungsspeicher nicht verfuegbar".into());
    };
    let Some(payload) = aeroacars_mqtt::backup::get_landings(None, &token)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(0);
    };
    let incoming: Vec<storage::LandingRecord> = payload
        .landings
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    // v0.20.x QS fix: same reasoning as `upload_landing_backup` — a
    // record deleted locally must stay deleted through an explicit
    // restore too, not just through the automatic backup path.
    let tombstoned = open_deleted_landings_tombstone(&app)
        .and_then(|t| t.read_all().ok())
        .unwrap_or_default();
    let incoming = storage::filter_tombstoned(incoming, &tombstoned);
    let added = store.merge_from(incoming).map_err(|e| e.to_string())?;
    tracing::info!(added, saved_at = %payload.saved_at, "landing backup restored");
    Ok(added)
}

/// Sicherung nach einer neuen Landung — verzoegert und zusammengefasst.
///
/// Die Verzoegerung fasst mehrere Schreibvorgaenge kurz nacheinander zu einem
/// Upload zusammen und haelt den Flugende-Pfad frei: Ein Backup, das den
/// Betrieb aufhaelt, waere schlechter als keines. Fehler werden geschluckt —
/// der naechste Anlass versucht es erneut.
fn spawn_landing_backup(app: &AppHandle) {
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        if let Err(e) = upload_landing_backup(app).await {
            tracing::debug!(error = %e, "landing backup deferred — will retry on next landing");
        }
    });
}
