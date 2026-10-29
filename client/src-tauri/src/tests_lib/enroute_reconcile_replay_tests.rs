    //! End-to-End-Replay der ECHTEN Flug-Logs aus dem Korpus-Audit 2026-07
    //! durch die komplette v1-FSM (`step_flight_at`, historische Uhr) plus
    //! Schatten-Engine — exakt in der Kopplung des Streamer-Ticks (v1
    //! zuerst, dann v2 mit der Post-Step-Phase als Sync-Quelle, Ergebnis
    //! nach `stats.shadow_phase` gestempelt, wo der Versöhner des
    //! NÄCHSTEN Ticks es liest).
    //!
    //! Fixtures (Dateinamen ohne Callsigns, Konvention wie phase_*):
    //!   * `phase_stuck_climb_be24.jsonl.gz` — der Feld-Report-Flug
    //!     (GSG22-Klasse): v1 blieb historisch von 15:00 bis zum Not-
    //!     Arrived-Fallback auf `Climb` (die `lost_from_peak`-Schwellen
    //!     sind bei 883 ft Peak-AGL mathematisch unerreichbar). Erwartung
    //!     NEU: der Versöhner zieht v1 noch IN DER LUFT aus `Climb`, und
    //!     der Flug endet über die normale Terminal-Kette.
    //!   * `phase_stuck_climb_c172.jsonl.gz` — Ultra-Low-Bush-Hop (Peak
    //!     357 ft AGL, Median 135 ft): v1 bleibt by-design in `Takeoff`
    //!     (500-ft-Climb-Gate nie erreicht), v2 spiegelt Takeoff 1:1 —
    //!     der Versöhner hat hier NICHTS zu tun und darf keine En-Route-
    //!     Phasen erfinden (Negativ-/Regressions-Beleg).

    use super::*;
    use flate2::read::GzDecoder;
    use std::io::{BufRead, BufReader};

    fn fixture_path(name: &str) -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("tests");
        p.push("fixtures");
        p.push(name);
        p
    }

    struct ReplaySample {
        t: DateTime<Utc>,
        snap: SimSnapshot,
    }

    /// Positions-Events laden. Bewusst KEINE volle SimSnapshot-
    /// Deserialisierung (ältere Logs kennen neuere Pflichtfelder nicht) —
    /// Default + gezielte Overrides der Felder, die die durchlaufenen
    /// FSM-Arme konsultieren.
    fn load_positions(name: &str) -> Vec<ReplaySample> {
        let f = std::fs::File::open(fixture_path(name)).expect("fixture file");
        let r = BufReader::new(GzDecoder::new(f));
        let mut out = Vec::new();
        for line in r.lines() {
            let Ok(line) = line else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if v["type"].as_str() != Some("position") {
                continue;
            }
            let Some(t) = v["timestamp"]
                .as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc))
            else {
                continue;
            };
            let s = &v["snapshot"];
            let mut snap = SimSnapshot::default();
            macro_rules! f64_field {
                ($field:ident) => {
                    if let Some(x) = s[stringify!($field)].as_f64() {
                        snap.$field = x;
                    }
                };
            }
            macro_rules! f32_field {
                ($field:ident) => {
                    if let Some(x) = s[stringify!($field)].as_f64() {
                        snap.$field = x as f32;
                    }
                };
            }
            f64_field!(lat);
            f64_field!(lon);
            f64_field!(altitude_msl_ft);
            f64_field!(altitude_agl_ft);
            f32_field!(vertical_speed_fpm);
            f32_field!(groundspeed_kt);
            f32_field!(fuel_total_kg);
            f32_field!(pitch_deg);
            f32_field!(bank_deg);
            if let Some(x) = s["on_ground"].as_bool() {
                snap.on_ground = x;
            }
            if let Some(x) = s["paused"].as_bool() {
                snap.paused = x;
            }
            if let Some(x) = s["slew_mode"].as_bool() {
                snap.slew_mode = x;
            }
            if let Some(x) = s["parking_brake"].as_bool() {
                snap.parking_brake = x;
            }
            if let Some(x) = s["engines_running"].as_u64() {
                snap.engines_running = x as u8;
            }
            snap.pushback_state = s["pushback_state"].as_u64().map(|x| x as u8);
            snap.velocity_body_z_fps = s["velocity_body_z_fps"].as_f64().map(|x| x as f32);
            snap.total_weight_kg = s["total_weight_kg"].as_f64().map(|x| x as f32);
            snap.zfw_kg = s["zfw_kg"].as_f64().map(|x| x as f32);
            snap.aircraft_icao = s["aircraft_icao"].as_str().map(str::to_owned);
            out.push(ReplaySample { t, snap });
        }
        out
    }

    pub(super) fn replay_fixture() -> ActiveFlight {
        ActiveFlight {
            pirep_id: "replay".into(),
            bid_id: 0,
            flight_id: String::new(),
            bid_callsign: None,
            pilot_callsign: None,
            started_at: Utc::now(),
            airline_icao: String::new(),
            planned_registration: String::new(),
            aircraft_icao: String::new(),
            aircraft_name: String::new(),
            flight_number: "22".into(),
            dpt_airport: "EDLN".into(),
            // Absichtlich nicht im (leeren) Navdata-Cache auflösbar —
            // `near_planned` kurzschließt dann zu true (gleiche Isolation
            // wie in arrived_fallback_dwell_tests; Distanz-Geometrie ist
            // dort separat abgedeckt).
            arr_airport: "ZZZZ".into(),
            planned_flight_time_min: None,
            plan_strecke_nm: None,
            airline_logo_url: None,
            fares: Vec::new(),
            stats: stats_sperre::GemesseneSperre::new(FlightStats::new()),
            stop: AtomicBool::new(false),
            was_just_resumed: AtomicBool::new(false),
            streamer_spawned: AtomicBool::new(false),
            cancelled_remotely: AtomicBool::new(false),
            position_outbox: Mutex::new(std::collections::VecDeque::new()),
            positionen_nachsenden: AtomicBool::new(false),
            generation: std::sync::atomic::AtomicU64::new(0),
            start_sperre: Mutex::new(()),
            phpvms_worker_spawned: AtomicBool::new(false),
            touchdown_sampler_spawned: AtomicBool::new(false),
            connection_state: std::sync::atomic::AtomicU8::new(CONN_STATE_LIVE),
            navdata: Mutex::new(NavdataCache::default()),
        }
    }

    /// v1.6 Stand-Gate (RYR-1142-Befund LPPT): TaxiIn→BlocksOn nur noch in
    /// Standnähe, sobald OSM-Parkpositionen vorliegen — plus Rückweg aus
    /// BlocksOn, wenn wieder gerollt wird. Koordinaten sind die echten aus
    /// dem Beweisflug (H4-Halt 423 m vom nächsten Stand, Parkposition 17 m
    /// vom OSM-Punkt für Stand 203).
    mod stand_gate {
        use super::*;

        fn lppt_stands() -> Vec<crate::stands::ParkingStand> {
            vec![
                crate::stands::ParkingStand {
                    name: Some("203".into()),
                    lat: 38.764528,
                    lon: -9.136807,
                    linie: None,
                    flaeche: false,
                },
                crate::stands::ParkingStand {
                    name: Some("204".into()),
                    lat: 38.764391,
                    lon: -9.137202,
                    linie: None,
                    flaeche: false,
                },
            ]
        }

        /// Stillstand mit Parkbremse — einmal am Rollhalt, einmal am Stand.
        fn stopped_at(lat: f64, lon: f64) -> SimSnapshot {
            SimSnapshot {
                lat,
                lon,
                on_ground: true,
                parking_brake: true,
                groundspeed_kt: 0.0,
                // Engines laufen beim Taxi — hält zugleich den universalen
                // Arrived-Fallback aus diesen Tests heraus.
                engines_running: 2,
                ..SimSnapshot::default()
            }
        }

        fn taxi_in_flight(stands: Option<Vec<crate::stands::ParkingStand>>) -> ActiveFlight {
            let flight = replay_fixture();
            {
                let mut stats = flight.stats.lock().unwrap();
                stats.phase = FlightPhase::TaxiIn;
                stats.arr_stands_icao = stands.as_ref().map(|_| "ZZZZ".to_string());
                stats.arr_stands = stands;
            }
            flight
        }

        /// OCN 712 (Joel, 24.09.2026): Nach der Landung stuerzte MSFS ab, nach
        /// dem Neuladen in EDDN standen 9544 kg im Tank. Blocksprit und
        /// Endsprit des PIREP kamen aus diesem Wert — „7 kg Verbrauch" fuer
        /// 834 NM. Durch die ECHTE Tickfunktion geschickt, nicht nachgebaut.
        #[test]
        fn neubeladung_nach_der_landung_aendert_den_sprit_des_fluges_nicht() {
            let flight = taxi_in_flight(None);
            {
                let mut st = flight.stats.lock().unwrap();
                st.takeoff_at = Some(Utc::now() - chrono::Duration::hours(2));
                st.block_fuel_kg = Some(8_951.0);
                st.landing_fuel_kg = Some(2_966.0);
                st.last_fuel_kg = Some(2_940.0);
            }
            let mut snap = stopped_at(49.4952, 11.0779);
            snap.fuel_total_kg = 9_544.0;
            step_flight_at(&flight, &snap, Utc::now());
            let st = flight.stats.lock().unwrap();
            assert_eq!(
                st.block_fuel_kg,
                Some(8_951.0),
                "Blocksprit nach dem Abflug eingefroren"
            );
            assert_eq!(
                st.last_fuel_kg,
                Some(2_940.0),
                "Endsprit bleibt beim letzten echten Wert"
            );
        }

        /// Nach einer Luecke nach der Landung kommt ZUERST ein Muellwert, dann
        /// der echte (APU lief weiter, 400 kg weniger). Der verworfene Muell
        /// darf die Uhr nicht zuruecksetzen — sonst bekaeme der echte Wert
        /// die Luecke nicht mehr gutgeschrieben und bliebe fuer immer
        /// verworfen (Cloud-QS 24.09.2026, vierte Runde).
        #[test]
        fn echter_wert_nach_muell_bekommt_die_luecke_gutgeschrieben() {
            let flight = taxi_in_flight(None);
            let t0 = Utc::now();
            {
                let mut st = flight.stats.lock().unwrap();
                st.takeoff_at = Some(t0 - chrono::Duration::seconds(7_860));
                st.block_fuel_kg = Some(8_951.0);
                st.landing_at = Some(t0);
                st.landing_fuel_kg = Some(2_966.0);
                st.last_fuel_kg = Some(2_940.0);
            }
            let mut muell = stopped_at(49.4952, 11.0779);
            muell.fuel_total_kg = 8.0;
            step_flight_at(&flight, &muell, t0 + chrono::Duration::seconds(1_200));
            assert_eq!(flight.stats.lock().unwrap().last_fuel_kg, Some(2_940.0));
            // 20 Minuten nach der Landung, 250 kg weniger — auch nach einem
            // App-Neustart dazwischen, weil ab der gespeicherten Landezeit
            // gerechnet wird.
            let mut echt = stopped_at(49.4952, 11.0779);
            echt.fuel_total_kg = 2_716.0;
            step_flight_at(&flight, &echt, t0 + chrono::Duration::seconds(1_203));
            assert_eq!(
                flight.stats.lock().unwrap().last_fuel_kg,
                Some(2_716.0),
                "echter Abfall nach der Luecke verworfen"
            );
        }

        /// Nach der Landung liefert das Menue 8 kg bei abgestellten Triebwerken.
        /// Die Marke „Triebwerke aus" darf diesen Wert nicht einrasten — sonst
        /// meldete der PIREP „Rollen nach der Landung: 2958 kg" (Codex-QS
        /// 24.09.2026). Zwei Takte ueber die Zehn-Sekunden-Frist.
        #[test]
        fn triebwerke_aus_rastet_keinen_menuewert_ein() {
            let flight = taxi_in_flight(None);
            let t0 = Utc::now();
            {
                let mut st = flight.stats.lock().unwrap();
                st.takeoff_at = Some(t0 - chrono::Duration::seconds(7_860));
                // Ohne Startsprit steigt `sprit_boden_marken` frueh aus — die
                // erste Fassung dieses Tests konnte deshalb nie rot werden.
                st.takeoff_fuel_kg = Some(8_700.0);
                st.block_fuel_kg = Some(8_951.0);
                st.landing_at = Some(t0);
                st.landing_fuel_kg = Some(2_966.0);
                st.last_fuel_kg = Some(2_940.0);
            }
            let mut menue = stopped_at(49.4952, 11.0779);
            menue.fuel_total_kg = 8.0;
            menue.engines_running = 0;
            step_flight_at(&flight, &menue, t0 + chrono::Duration::seconds(600));
            step_flight_at(&flight, &menue, t0 + chrono::Duration::seconds(615));
            let st = flight.stats.lock().unwrap();
            // Die Marke rastet ein — aber mit dem letzten echten Wert.
            assert_eq!(st.engine_off_fuel_kg, Some(2_940.0), "Abstell-Sprit falsch");
        }

        /// Kennt nur der Aufsetz-Sampler die Landung (Busch, Hubschrauber),
        /// gilt SEINE Zeit als Basis — sonst gaebe es gar keine Grenze.
        #[test]
        fn aufsetzzeit_des_samplers_zaehlt_als_zeitbasis() {
            // Eine Cessna, deren Landung nur der Sampler kennt: 90 kg Block,
            // 60 kg Landesprit, eine Stunde Flug — also rund 10 kg/h am Boden.
            // Mit der Jet-Rate (2000 kg/h) waeren nach einer Minute 38 kg
            // Verlust erlaubt; der Test waehlt 35 kg, damit NUR die richtige
            // Rate ihn verwirft (Codex-QS 24.09.2026).
            let flight = taxi_in_flight(None);
            let t0 = Utc::now();
            {
                let mut st = flight.stats.lock().unwrap();
                st.takeoff_at = Some(t0 - chrono::Duration::seconds(3_600));
                st.block_fuel_kg = Some(90.0);
                st.landing_at = None;
                st.sampler_touchdown_at = Some(t0);
                st.landing_fuel_kg = Some(60.0);
                st.last_fuel_kg = Some(59.0);
            }
            let mut snap = stopped_at(49.4952, 11.0779);
            snap.fuel_total_kg = 25.0;
            step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(60));
            assert_eq!(flight.stats.lock().unwrap().last_fuel_kg, Some(59.0));
        }

        /// Gegenprobe: VOR dem Abflug ist Nachtanken ganz normal — der
        /// Blocksprit folgt dem hoechsten Stand wie bisher.
        #[test]
        fn nachtanken_vor_dem_abflug_zaehlt_weiter() {
            let flight = taxi_in_flight(None);
            {
                let mut st = flight.stats.lock().unwrap();
                st.phase = FlightPhase::Boarding;
                st.takeoff_at = None;
                st.block_fuel_kg = Some(5_000.0);
                st.landing_fuel_kg = None;
                st.last_fuel_kg = Some(5_000.0);
            }
            let mut snap = stopped_at(49.4952, 11.0779);
            snap.fuel_total_kg = 9_000.0;
            step_flight_at(&flight, &snap, Utc::now());
            let st = flight.stats.lock().unwrap();
            assert_eq!(st.block_fuel_kg, Some(9_000.0));
            assert_eq!(st.last_fuel_kg, Some(9_000.0));
        }

        #[test]
        fn taxi_hold_far_from_stands_stays_taxi_in() {
            let flight = taxi_in_flight(Some(lppt_stands()));
            let snap = stopped_at(38.7830, -9.1327); // H4
            let change = step_flight_at(&flight, &snap, Utc::now());
            assert_eq!(change, None, "Rollhalt fern jedes Stands ist kein Block-On");
            let stats = flight.stats.lock().unwrap();
            assert_eq!(stats.phase, FlightPhase::TaxiIn);
            assert!(!stats.blocks_on_reached);
            assert!(stats.block_on_at.is_none());
        }

        /// Rollweg B ost-west auf 50,0° N; die Lead-in-Linie des Stands
        /// „R1" beginnt 11 m nördlich davon und endet nach 55 m.
        const AUF_ROLLWEG_B: (f64, f64) = (50.0, 8.0051);
        const STAND_R1_ENDE: (f64, f64) = (50.0005, 8.005);

        fn rollweg_halt_flight() -> ActiveFlight {
            let flight = taxi_in_flight(Some(vec![crate::stands::ParkingStand {
                name: Some("R1".into()),
                lat: STAND_R1_ENDE.0,
                lon: STAND_R1_ENDE.1,
                linie: Some(vec![(50.0001, 8.005), STAND_R1_ENDE]),
                flaeche: false,
            }]));
            {
                let mut stats = flight.stats.lock().unwrap();
                stats.arr_rollwege = Some(vec![vec![(50.0, 8.0), (50.0, 8.01)]]);
                stats.arr_rollwege_icao = Some("ZZZZ".into());
            }
            flight
        }

        /// LJLJ (BoYJe50, 11.10.2026): Bremse nach 82 s gelöst, 15 s später
        /// am selben Ort abgestellt — die Block-On-Zeit bleibt der Haltbeginn.
        #[test]
        fn bremse_loesen_am_selben_ort_behaelt_den_haltbeginn() {
            let flight = rollweg_halt_flight();
            let t0 = Utc::now();
            let halt = stopped_at(AUF_ROLLWEG_B.0, AUF_ROLLWEG_B.1);
            step_flight_at(&flight, &halt, t0);
            let mut geloest = halt.clone();
            geloest.parking_brake = false;
            step_flight_at(&flight, &geloest, t0 + chrono::Duration::seconds(82));
            let mut aus = halt.clone();
            aus.engines_running = 0;
            let change = step_flight_at(&flight, &aus, t0 + chrono::Duration::seconds(97));
            assert_eq!(change, Some(FlightPhase::BlocksOn));
            assert_eq!(flight.stats.lock().unwrap().block_on_at, Some(t0));
        }

        /// Ohne Standdaten gilt das alte Kriterium, auch wenn die Bodenkarte
        /// Rollwege kennt (nur Halte mit Standdaten sind vermessen).
        #[test]
        fn ohne_standdaten_wartet_der_rollweg_halt_nicht() {
            let flight = taxi_in_flight(None);
            {
                let mut stats = flight.stats.lock().unwrap();
                stats.arr_rollwege = Some(vec![vec![(50.0, 8.0), (50.0, 8.01)]]);
                stats.arr_rollwege_icao = Some("ZZZZ".into());
            }
            let snap = stopped_at(AUF_ROLLWEG_B.0, AUF_ROLLWEG_B.1);
            assert_eq!(step_flight_at(&flight, &snap, Utc::now()), Some(FlightPhase::BlocksOn));
        }

        /// Codex-QS: versetzt Slew/GSX den Flieger bei gesetzter Bremse an
        /// den Stand, gilt die Zeit des Rollweg-Halts nicht als Block-On.
        #[test]
        fn versetzt_an_den_stand_zaehlt_der_rollweg_halt_nicht() {
            let flight = rollweg_halt_flight();
            let t0 = Utc::now();
            step_flight_at(&flight, &stopped_at(AUF_ROLLWEG_B.0, AUF_ROLLWEG_B.1), t0);
            let t1 = t0 + chrono::Duration::seconds(40);
            let change = step_flight_at(&flight, &stopped_at(STAND_R1_ENDE.0, STAND_R1_ENDE.1), t1);
            assert_eq!(change, Some(FlightPhase::BlocksOn));
            assert_eq!(flight.stats.lock().unwrap().block_on_at, Some(t1));
        }

        /// Codex-QS: ein Haltbeginn von anderswo (erneut abgehoben) darf
        /// über den Arrived-Fallback nicht zur Block-On-Zeit werden.
        #[test]
        fn fremder_haltbeginn_wird_nicht_zur_block_on_zeit() {
            let flight = rollweg_halt_flight();
            let t0 = Utc::now();
            let alt = t0 - chrono::Duration::hours(1);
            flight.stats.lock().unwrap().blocks_on_kandidat = Some((alt, 51.0, 9.0));
            let snap = stopped_at(STAND_R1_ENDE.0, STAND_R1_ENDE.1);
            assert_eq!(step_flight_at(&flight, &snap, t0), Some(FlightPhase::BlocksOn));
            let stats = flight.stats.lock().unwrap();
            assert_eq!(stats.block_on_at, Some(t0));
            assert!(stats.blocks_on_kandidat.is_none());
        }

        /// Thorben, TCX 24 EDDN (26.10.2026): Halt mit Bremse auf dem
        /// Rollweg, 48 m vom Ende einer kurzen Lead-in-Linie — bisher sofort
        /// BlocksOn, 36 s später zurück nach TaxiIn.
        #[test]
        fn rollweg_halt_im_stand_radius_ist_noch_kein_block_on() {
            let flight = rollweg_halt_flight();
            let t0 = Utc::now();
            let snap = stopped_at(AUF_ROLLWEG_B.0, AUF_ROLLWEG_B.1);
            assert_eq!(step_flight_at(&flight, &snap, t0), None);
            assert_eq!(step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(84)), None);
            let stats = flight.stats.lock().unwrap();
            assert_eq!(stats.phase, FlightPhase::TaxiIn);
            assert!(stats.block_on_at.is_none());
        }

        /// Hält die Bremse die volle Frist, zählt der Halt — mit der Zeit,
        /// zu der er begann, nicht der Zeit der Bestätigung.
        #[test]
        fn langer_rollweg_halt_wird_block_on_ab_haltbeginn() {
            let flight = rollweg_halt_flight();
            let t0 = Utc::now();
            let snap = stopped_at(AUF_ROLLWEG_B.0, AUF_ROLLWEG_B.1);
            step_flight_at(&flight, &snap, t0);
            let change = step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(120));
            assert_eq!(change, Some(FlightPhase::BlocksOn));
            let stats = flight.stats.lock().unwrap();
            assert_eq!(stats.block_on_at, Some(t0));
            assert!(stats.blocks_on_kandidat.is_none());
        }

        /// Wer auf dem Rollweg die Triebwerke abstellt, ist angekommen.
        #[test]
        fn triebwerke_aus_auf_dem_rollweg_ist_sofort_block_on() {
            let flight = rollweg_halt_flight();
            let t0 = Utc::now();
            let mut snap = stopped_at(AUF_ROLLWEG_B.0, AUF_ROLLWEG_B.1);
            step_flight_at(&flight, &snap, t0);
            snap.engines_running = 0;
            let change = step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(30));
            assert_eq!(change, Some(FlightPhase::BlocksOn));
            assert_eq!(flight.stats.lock().unwrap().block_on_at, Some(t0));
        }

        /// Losrollen verwirft den Haltbeginn; der nächste Halt zählt neu.
        #[test]
        fn weiterrollen_setzt_die_rollweg_frist_zurueck() {
            let flight = rollweg_halt_flight();
            let t0 = Utc::now();
            let halt = stopped_at(AUF_ROLLWEG_B.0, AUF_ROLLWEG_B.1);
            let mut rollt = stopped_at(50.0, 8.0060); // 64 m weiter
            rollt.parking_brake = false;
            rollt.groundspeed_kt = 10.0;
            step_flight_at(&flight, &halt, t0);
            step_flight_at(&flight, &rollt, t0 + chrono::Duration::seconds(30));
            step_flight_at(&flight, &halt, t0 + chrono::Duration::seconds(60));
            assert_eq!(step_flight_at(&flight, &halt, t0 + chrono::Duration::seconds(150)), None);
            let change = step_flight_at(&flight, &halt, t0 + chrono::Duration::seconds(180));
            assert_eq!(change, Some(FlightPhase::BlocksOn));
            assert_eq!(
                flight.stats.lock().unwrap().block_on_at,
                Some(t0 + chrono::Duration::seconds(60))
            );
        }

        /// Gegenprobe: am Ende der Lead-in-Linie, fern der Rollweg-Mittellinie,
        /// gilt der Halt wie bisher sofort.
        #[test]
        fn halt_am_stand_neben_dem_rollweg_ist_sofort_block_on() {
            let flight = rollweg_halt_flight();
            let t0 = Utc::now();
            let snap = stopped_at(STAND_R1_ENDE.0, STAND_R1_ENDE.1);
            assert_eq!(step_flight_at(&flight, &snap, t0), Some(FlightPhase::BlocksOn));
            let stats = flight.stats.lock().unwrap();
            assert_eq!(stats.block_on_at, Some(t0));
            assert_eq!(stats.arr_gate.as_deref(), Some("R1"));
        }

        #[test]
        fn stop_at_stand_becomes_blocks_on_with_gate_name() {
            let flight = taxi_in_flight(Some(lppt_stands()));
            let snap = stopped_at(38.7646843, -9.1368409); // Stand 203, 17 m
            let change = step_flight_at(&flight, &snap, Utc::now());
            assert_eq!(change, Some(FlightPhase::BlocksOn));
            let stats = flight.stats.lock().unwrap();
            assert!(stats.blocks_on_reached);
            assert!(stats.block_on_at.is_some());
            assert_eq!(stats.arr_gate.as_deref(), Some("203"));
            assert_eq!(stats.arr_gate_icao.as_deref(), Some("ZZZZ"));
        }

        #[test]
        fn without_stand_data_the_legacy_criteria_still_apply() {
            let flight = taxi_in_flight(None);
            let snap = stopped_at(38.7830, -9.1327); // H4 — aber ohne Daten
            let change = step_flight_at(&flight, &snap, Utc::now());
            assert_eq!(
                change,
                Some(FlightPhase::BlocksOn),
                "ohne Bodendaten bleibt das alte Kriterium aktiv (kein Blockieren)"
            );
        }

        #[test]
        fn stand_data_for_another_airport_falls_back_to_legacy() {
            let flight = taxi_in_flight(Some(lppt_stands()));
            {
                // Divert-Szenario: die Liste gilt für den GEPLANTEN Flughafen,
                // gelandet wird woanders (at_icao "ZZZZ" ≠ "LPPT").
                let mut stats = flight.stats.lock().unwrap();
                stats.arr_stands_icao = Some("LPPT".into());
            }
            let snap = stopped_at(38.7830, -9.1327);
            let change = step_flight_at(&flight, &snap, Utc::now());
            assert_eq!(change, Some(FlightPhase::BlocksOn));
        }

        /// QS-Befund 21.08.2026, der schwerste: der Replay-Verdacht wurde NUR
        /// in der Luft ausgewertet. Stand er einmal auf wahr, blieb er nach dem
        /// Aufsetzen fuer immer stehen — und der Riegel fror die Phasen-Engine
        /// fuer den Rest des Fluges ein. Kein Uebergang mehr nach TaxiIn, keine
        /// Blockzeit, der Flugbericht unabschliessbar.
        #[test]
        fn replay_verdacht_wird_am_boden_geloescht_und_blockiert_nicht() {
            let flight = taxi_in_flight(Some(lppt_stands()));
            {
                let mut stats = flight.stats.lock().unwrap();
                stats.phase = FlightPhase::BlocksOn;
                stats.blocks_on_reached = true;
                stats.block_on_at = Some(Utc::now());
                stats.arr_gate = Some("203".into());
                stats.arr_gate_icao = Some("ZZZZ".into());
                // Der Verdacht, wie ihn ein Fehlalarm im Endanflug hinterlaesst.
                stats.replay_verdacht = true;
                stats
                    .replay_proben
                    .push_back(replay_erkennung::ReplayProbe {
                        t_s: 0.0,
                        lat: 38.78,
                        lon: -9.13,
                        msl_ft: 300.0,
                        groundspeed_kt: 0.0,
                        ias_kt: 0.0,
                        vs_fps: 0.0,
                        on_ground: false,
                    });
            }
            let snap = SimSnapshot {
                lat: 38.7830,
                lon: -9.1327,
                on_ground: true,
                parking_brake: false,
                groundspeed_kt: 12.0,
                engines_running: 2,
                ..SimSnapshot::default()
            };

            let change = step_flight_at(&flight, &snap, Utc::now());

            // Der Uebergang muss stattfinden — vorher blockierte ihn der
            // stehengebliebene Verdacht.
            assert_eq!(
                change,
                Some(FlightPhase::TaxiIn),
                "der Flug bleibt haengen: der Replay-Verdacht friert die Phasen-Engine ein"
            );
            let stats = flight.stats.lock().unwrap();
            assert!(!stats.replay_verdacht, "Verdacht am Boden nicht geloescht");
            assert!(
                stats.replay_proben.is_empty(),
                "Puffer am Boden nicht geleert"
            );
        }

        #[test]
        fn rolling_again_returns_from_blocks_on_to_taxi_in() {
            let flight = taxi_in_flight(Some(lppt_stands()));
            {
                let mut stats = flight.stats.lock().unwrap();
                stats.phase = FlightPhase::BlocksOn;
                stats.blocks_on_reached = true;
                stats.block_on_at = Some(Utc::now());
                stats.arr_gate = Some("203".into());
                stats.arr_gate_icao = Some("ZZZZ".into());
            }
            let snap = SimSnapshot {
                lat: 38.7830,
                lon: -9.1327,
                on_ground: true,
                parking_brake: false,
                groundspeed_kt: 12.0,
                engines_running: 2,
                ..SimSnapshot::default()
            };
            let change = step_flight_at(&flight, &snap, Utc::now());
            assert_eq!(change, Some(FlightPhase::TaxiIn));
            let stats = flight.stats.lock().unwrap();
            assert!(!stats.blocks_on_reached);
            assert!(stats.block_on_at.is_none());
            assert!(stats.arr_gate.is_none());
            assert!(stats.arr_gate_icao.is_none());
        }

        #[test]
        fn lowercase_bid_icao_still_arms_the_stand_gate() {
            // Review-Fund: arr_stands_icao ist upper-getrimmt, das Bid darf
            // trotzdem lowercase liefern — das Gate muss greifen.
            let mut flight = taxi_in_flight(Some(lppt_stands()));
            flight.arr_airport = "zzzz".into();
            let snap = stopped_at(38.7830, -9.1327); // H4
            let change = step_flight_at(&flight, &snap, Utc::now());
            assert_eq!(change, None, "Gate darf durch Gross/Klein nicht ausfallen");
            let stats = flight.stats.lock().unwrap();
            assert_eq!(stats.phase, FlightPhase::TaxiIn);
        }

        #[test]
        fn gs_spike_with_brake_set_keeps_blocks_on() {
            // Review-Fund: GS-Artefakt (Slew/GSX-Ruck) bei GESETZTER
            // Parkbremse darf die Block-On-Zeit nicht verwerfen.
            let flight = taxi_in_flight(None);
            {
                let mut stats = flight.stats.lock().unwrap();
                stats.phase = FlightPhase::BlocksOn;
                stats.blocks_on_reached = true;
                stats.block_on_at = Some(Utc::now());
            }
            let snap = SimSnapshot {
                lat: 38.7646843,
                lon: -9.1368409,
                on_ground: true,
                parking_brake: true,
                groundspeed_kt: 12.0,
                engines_running: 2,
                ..SimSnapshot::default()
            };
            let change = step_flight_at(&flight, &snap, Utc::now());
            assert_eq!(change, None);
            let stats = flight.stats.lock().unwrap();
            assert_eq!(stats.phase, FlightPhase::BlocksOn);
            assert!(stats.block_on_at.is_some());
        }

        #[test]
        fn creeping_forward_below_threshold_stays_blocks_on() {
            // 5-kt-Schwelle: Bremse lösen + Zentimeter rollen (Towbar-
            // Wackler, Brake-Check) wirft den Block-On nicht weg.
            let flight = taxi_in_flight(None);
            {
                let mut stats = flight.stats.lock().unwrap();
                stats.phase = FlightPhase::BlocksOn;
                stats.blocks_on_reached = true;
                stats.block_on_at = Some(Utc::now());
            }
            let snap = SimSnapshot {
                lat: 38.7646843,
                lon: -9.1368409,
                on_ground: true,
                parking_brake: false,
                groundspeed_kt: 2.0,
                engines_running: 2,
                ..SimSnapshot::default()
            };
            let change = step_flight_at(&flight, &snap, Utc::now());
            assert_eq!(change, None);
            let stats = flight.stats.lock().unwrap();
            assert_eq!(stats.phase, FlightPhase::BlocksOn);
            assert!(stats.blocks_on_reached);
        }
    }

    /// Replayt ein Fixture durch v1+v2 in Streamer-Kopplung und liefert
    /// den v1-Phasen-Bogen als (Zeitpunkt, Phase)-Transitions-Liste.
    fn run_v1_arc(name: &str) -> Vec<(DateTime<Utc>, FlightPhase)> {
        let samples = load_positions(name);
        assert!(samples.len() > 100, "fixture must carry real telemetry");
        let flight = replay_fixture();
        let mut arc: Vec<(DateTime<Utc>, FlightPhase)> = Vec::new();
        for ReplaySample { t, snap } in samples {
            if let Some(p) = step_flight_at(&flight, &snap, t) {
                arc.push((t, p));
            }
            if !snap.paused && !snap.slew_mode {
                let mut stats = flight.stats.lock().unwrap();
                let old_phase_post_step = stats.phase;
                let (shadow, _seg) = stats.shadow_engine.step(
                    t,
                    snap.altitude_msl_ft,
                    snap.altitude_agl_ft,
                    snap.vertical_speed_fpm,
                    snap.on_ground,
                    old_phase_post_step,
                    None,
                );
                stats.shadow_phase = Some(shadow);
                stats.shadow_segment = Some(_seg);
            }
        }
        arc
    }

    #[test]
    fn be24_field_report_flight_leaves_climb_in_the_air_and_lands_normally() {
        // Historisch (echtes Log): Climb ab 15:00:39, dann NICHTS mehr bis
        // der Not-Arrived-Fallback um 15:15:16 zuschlug — Touchdown war
        // 15:11:13. Mit dem Versöhner muss v1 den Climb VOR dem Touchdown
        // verlassen und der Flug über die reguläre Kette enden.
        let arc = run_v1_arc("phase_stuck_climb_be24.jsonl.gz");
        let touchdown = DateTime::parse_from_rfc3339("2026-07-11T15:11:13.795049900Z")
            .unwrap()
            .with_timezone(&Utc);

        // Abflug-Kette unangetastet (der Versöhner greift erst im
        // Luft-Band): Boarding→TaxiOut→TakeoffRoll→Takeoff→Climb wie im
        // echten Log.
        let phases: Vec<FlightPhase> = arc.iter().map(|(_, p)| *p).collect();
        assert_eq!(
            &phases[..4],
            &[
                FlightPhase::TaxiOut,
                FlightPhase::TakeoffRoll,
                FlightPhase::Takeoff,
                FlightPhase::Climb,
            ],
            "departure chain must replay unchanged; full arc: {phases:?}"
        );

        // Kern-Assertion: v1 verlässt Climb IN DER LUFT (vor Touchdown) —
        // exakt das, was im echten Flug nie passierte.
        let left_climb_at = arc
            .iter()
            .skip_while(|(_, p)| *p != FlightPhase::Climb)
            .skip(1)
            .map(|(t, _)| *t)
            .next()
            .expect("v1 must transition out of Climb somewhere");
        assert!(
            left_climb_at < touchdown,
            "reconciler must pull v1 out of Climb before touchdown \
             (left at {left_climb_at}, touchdown {touchdown}); arc: {phases:?}"
        );

        // Terminal-Kette erreicht (Arrived — egal ob über BlocksOn oder
        // den Fallback; entscheidend ist, dass der Flug regulär endet).
        assert_eq!(
            phases.last(),
            Some(&FlightPhase::Arrived),
            "flight must terminate in Arrived; arc: {phases:?}"
        );
        // ... und die Landung wurde als Phase erlebt, nicht übersprungen.
        assert!(
            phases.contains(&FlightPhase::Landing),
            "the landing must appear as a real phase in the arc: {phases:?}"
        );
    }

    #[test]
    fn c172_ultra_low_bush_hop_stays_untouched_by_the_reconciler() {
        // Peak 357 ft AGL — das 500-ft-Climb-Gate wird nie erreicht, v1
        // bleibt by-design in Takeoff, v2 spiegelt Takeoff 1:1 (kein
        // En-Route-Band, keine Divergenz). Der Versöhner darf hier keine
        // Luft-Phasen erfinden — der Bogen muss dem historischen Verlauf
        // entsprechen (Bush-Hop-Verhalten, Landing-Erfassung läuft seit
        // v0.15.23 ohnehin phasenunabhängig über den Sampler).
        let arc = run_v1_arc("phase_stuck_climb_c172.jsonl.gz");
        let phases: Vec<FlightPhase> = arc.iter().map(|(_, p)| *p).collect();
        for (_, p) in &arc {
            assert!(
                matches!(
                    p,
                    FlightPhase::TaxiOut
                        | FlightPhase::TakeoffRoll
                        | FlightPhase::Takeoff
                        | FlightPhase::Arrived
                ),
                "no invented en-route phase on a sub-gate bush hop; arc: {phases:?}"
            );
        }
        assert_eq!(
            phases.last(),
            Some(&FlightPhase::Arrived),
            "arc: {phases:?}"
        );
    }
