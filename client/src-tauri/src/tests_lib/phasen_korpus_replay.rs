    use super::*;
    use flate2::read::GzDecoder;
    use std::io::{BufRead, BufReader, Write};

    struct Aufnahme {
        positionen: Vec<(DateTime<Utc>, SimSnapshot)>,
        v1: Vec<(DateTime<Utc>, String)>,
        v2: Vec<(DateTime<Utc>, String)>,
        version: String,
        go_arounds_pirep: Option<u64>,
    }

    fn zeit(v: &serde_json::Value) -> Option<DateTime<Utc>> {
        v.as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
    }

    /// Volle Deserialisierung, wo der Snapshot sie trägt; sonst die Felder,
    /// die die Phasen-Arme lesen (ältere Logs kennen neuere Pflichtfelder
    /// nicht — gleiche Rückfallebene wie `enroute_reconcile_replay_tests`).
    fn snapshot(s: &serde_json::Value) -> SimSnapshot {
        if let Ok(snap) = serde_json::from_value::<SimSnapshot>(s.clone()) {
            return snap;
        }
        let mut snap = SimSnapshot::default();
        let f = |k: &str| s[k].as_f64();
        if let Some(x) = f("lat") { snap.lat = x; }
        if let Some(x) = f("lon") { snap.lon = x; }
        if let Some(x) = f("altitude_msl_ft") { snap.altitude_msl_ft = x; }
        if let Some(x) = f("altitude_agl_ft") { snap.altitude_agl_ft = x; }
        if let Some(x) = f("vertical_speed_fpm") { snap.vertical_speed_fpm = x as f32; }
        if let Some(x) = f("groundspeed_kt") { snap.groundspeed_kt = x as f32; }
        if let Some(x) = f("indicated_airspeed_kt") { snap.indicated_airspeed_kt = x as f32; }
        if let Some(x) = f("fuel_total_kg") { snap.fuel_total_kg = x as f32; }
        if let Some(x) = f("pitch_deg") { snap.pitch_deg = x as f32; }
        if let Some(x) = f("bank_deg") { snap.bank_deg = x as f32; }
        if let Some(x) = f("heading_deg_true") { snap.heading_deg_true = x as f32; }
        if let Some(x) = f("gear_position") { snap.gear_position = x as f32; }
        if let Some(x) = f("flaps_position") { snap.flaps_position = x as f32; }
        if let Some(x) = s["on_ground"].as_bool() { snap.on_ground = x; }
        if let Some(x) = s["paused"].as_bool() { snap.paused = x; }
        if let Some(x) = s["slew_mode"].as_bool() { snap.slew_mode = x; }
        if let Some(x) = s["parking_brake"].as_bool() { snap.parking_brake = x; }
        if let Some(x) = s["engines_running"].as_u64() { snap.engines_running = x as u8; }
        snap.pushback_state = s["pushback_state"].as_u64().map(|x| x as u8);
        snap.velocity_body_z_fps = f("velocity_body_z_fps").map(|x| x as f32);
        snap.total_weight_kg = f("total_weight_kg").map(|x| x as f32);
        snap.zfw_kg = f("zfw_kg").map(|x| x as f32);
        snap.aircraft_icao = s["aircraft_icao"].as_str().map(str::to_owned);
        snap
    }

    fn lesen(pfad: &std::path::Path) -> Option<Aufnahme> {
        let f = std::fs::File::open(pfad).ok()?;
        let mut a = Aufnahme {
            positionen: Vec::new(),
            v1: Vec::new(),
            v2: Vec::new(),
            version: String::new(),
            go_arounds_pirep: None,
        };
        for zeile in BufReader::new(GzDecoder::new(f)).lines() {
            let Ok(zeile) = zeile else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&zeile) else { continue };
            if a.version.is_empty() {
                if let Some(ver) = v["client_version"].as_str() {
                    a.version = ver.to_owned();
                }
            }
            match v["type"].as_str() {
                Some("position") => {
                    let Some(t) = zeit(&v["timestamp"]) else { continue };
                    let s = &v["snapshot"];
                    if let Some(sp) = s["shadow_phase"].as_str() {
                        if a.v2.last().map(|(_, p)| p.as_str()) != Some(sp) {
                            a.v2.push((t, sp.to_owned()));
                        }
                    }
                    a.positionen.push((t, snapshot(s)));
                }
                Some("phase_changed") => {
                    if let (Some(t), Some(to)) = (zeit(&v["timestamp"]), v["to"].as_str()) {
                        a.v1.push((t, to.to_owned()));
                    }
                }
                Some("pirep_filed") => {
                    a.go_arounds_pirep = v["payload"]["go_around_count"].as_u64();
                }
                _ => {}
            }
        }
        (a.positionen.len() >= 100).then_some(a)
    }

    fn fingerabdruck(buf: &std::collections::VecDeque<ApproachBufferSample>) -> serde_json::Value {
        let summe: f64 = buf
            .iter()
            .map(|s| f64::from(s.agl_ft) + f64::from(s.vs_fpm) + f64::from(s.bank_deg))
            .sum();
        serde_json::json!({
            "n": buf.len(),
            "summe": (summe * 10.0).round() / 10.0,
            "von": buf.front().map(|s| s.at.to_rfc3339()),
            "bis": buf.back().map(|s| s.at.to_rfc3339()),
        })
    }

    #[test]
    #[ignore = "Korpus-Replay: nur mit AEROACARS_REPLAY_DIR/AEROACARS_REPLAY_OUT"]
    fn phasen_korpus() {
        let dir = std::env::var("AEROACARS_REPLAY_DIR").expect("AEROACARS_REPLAY_DIR");
        let out = std::env::var("AEROACARS_REPLAY_OUT").expect("AEROACARS_REPLAY_OUT");
        let mit_ref = std::env::var("AEROACARS_REPLAY_REF").as_deref() == Ok("max");
        let mut dateien: Vec<std::path::PathBuf> = Vec::new();
        let mut stapel = vec![std::path::PathBuf::from(&dir)];
        while let Some(d) = stapel.pop() {
            for e in std::fs::read_dir(&d).expect("Korpus-Ordner").flatten() {
                let p = e.path();
                if p.is_dir() {
                    stapel.push(p);
                } else if p.to_string_lossy().ends_with(".jsonl.gz") {
                    dateien.push(p);
                }
            }
        }
        dateien.sort();
        let mut w = std::io::BufWriter::new(std::fs::File::create(&out).expect("Ausgabe"));
        let mut gelesen = 0;
        for pfad in &dateien {
            let Some(a) = lesen(pfad) else { continue };
            gelesen += 1;
            let flight = enroute_reconcile_replay_tests::replay_fixture();
            if mit_ref {
                let max = a.positionen.iter().map(|(_, s)| s.altitude_msl_ft).fold(0.0, f64::max);
                flight.stats.lock().unwrap().planned_cruise_alt_ft = Some((max / 1000.0).floor() * 1000.0);
            }
            let mut bogen: Vec<(String, String)> = Vec::new();
            let mut anflug_puffer: Vec<serde_json::Value> = Vec::new();
            // Stück 3 (20.10.2026): wie lange stand die Phase hoechstens am
            // Stueck auf `Landing`, waehrend das Flugzeug ueber 300 ft flog?
            let mut luft_landing_seit: Option<DateTime<Utc>> = None;
            let mut luft_landing_max_s = 0_i64;
            for (t, snap) in &a.positionen {
                let vorher = flight.stats.lock().unwrap().phase;
                let _ = phasen_takt(&flight, snap, *t);
                let stats = flight.stats.lock().unwrap();
                if stats.phase == FlightPhase::Landing && !snap.on_ground && snap.altitude_agl_ft > 300.0 {
                    let seit = *luft_landing_seit.get_or_insert(*t);
                    luft_landing_max_s = luft_landing_max_s.max((*t - seit).num_seconds());
                } else {
                    luft_landing_seit = None;
                }
                if stats.phase != vorher || bogen.is_empty() {
                    if stats.phase == FlightPhase::Landing && vorher != FlightPhase::Landing {
                        let mut f = fingerabdruck(&stats.approach_buffer);
                        // Dieselbe Berechnung, die in die Landebewertung geht
                        // (ohne Platzhöhe/Gleitpfad — die kennt das Replay nicht).
                        f["stabilitaet"] = serde_json::Value::String(format!(
                            "{:?}",
                            compute_approach_stability_v2(
                                &stats.approach_buffer,
                                None,
                                stats.landing_at,
                                None,
                                None,
                                Default::default(),
                            )
                        ));
                        anflug_puffer.push(f);
                    }
                    bogen.push((t.to_rfc3339(), format!("{:?}", stats.phase)));
                }
            }
            let stats = flight.stats.lock().unwrap();
            let zeile = serde_json::json!({
                "datei": pfad.strip_prefix(&dir).unwrap_or(pfad).to_string_lossy(),
                "version": a.version,
                "positionen": a.positionen.len(),
                "neu": bogen,
                "v1": a.v1,
                "v2": a.v2,
                "go_arounds_neu": stats.go_around_count,
                "go_arounds_pirep": a.go_arounds_pirep,
                "anflug_puffer": anflug_puffer,
                "luft_landing_max_s": luft_landing_max_s,
                "aufsetzer": stats
                    .touchdown_events
                    .iter()
                    .map(|e| format!("{:?}@{}", e.kind, e.timestamp.format("%H:%M:%S")))
                    .collect::<Vec<_>>(),
            });
            writeln!(w, "{zeile}").expect("schreiben");
        }
        eprintln!("phasen_korpus: {gelesen} von {} Logs gespielt → {out}", dateien.len());
    }
