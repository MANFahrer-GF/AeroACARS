//! Prüfstand: der ECHTE Client-Code (v1.9.11) gegen den neuen Recorder.
//! Läuft nur mit E2E_BASE (z. B. http://127.0.0.1:47831).
use aeroacars_mqtt::{backup, bordbuch, chat, log_upload, messung, navdata, pirep_status, provision};
use serde_json::json;
use std::fmt::Display;

const KEY: &str = "e2e-phpvms-schluessel-0001";
static FEHLER: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Schlimm ist nur, was ein Pilot als „ausgesperrt / gedrosselt / zu gross" erlebt.
fn ausgesperrt(t: &str) -> bool {
    let k = t.to_lowercase();
    ["401", "403", "429", "413", "nicht angemeldet", "pilot-token missing", "too many", "unauthenticated", "busy"]
        .iter()
        .any(|w| k.contains(w))
}
fn pruefe<T, E: Display>(name: &str, r: Result<T, E>) {
    match r {
        Ok(_) => println!("  ✓ {name}"),
        Err(e) => {
            let t = e.to_string();
            if ausgesperrt(&t) {
                println!("  ✗ {name}  →  {t}");
                FEHLER.lock().unwrap().push(format!("{name}: {t}"));
            } else {
                println!("  ✓ {name}  (Server antwortet fachlich: {})", t.chars().take(70).collect::<String>());
            }
        }
    }
}
fn zufall(n: usize) -> String {
    let mut x: u64 = 0x9E3779B97F4A7C15;
    let mut s = String::with_capacity(n);
    while s.len() < n {
        x ^= x << 13; x ^= x >> 7; x ^= x << 17;
        s.push_str(&format!("{x:016x}"));
    }
    s
}
fn protokoll(pirep: &str, fuellung_kb: usize) -> Vec<u8> {
    let start = chrono_ms() - 3 * 3_600_000;
    let mut z = vec![
        json!({"type":"flight_started","timestamp":iso(start),"pirep_id":pirep,"dpt_airport":"LHBP","arr_airport":"EDDS","airline_icao":"MAH","flight_number":"516"}).to_string(),
        json!({"type":"pirep_filed","timestamp":iso(start+6_000_000),"payload":{"ts":start+6_000_000,"pirep_id":pirep,"flight_number":"MAH516","dep":"LHBP","arr":"EDDS","landing_score":80}}).to_string(),
    ];
    if fuellung_kb > 0 { z.push(json!({"type":"telemetry","blob":zufall(fuellung_kb * 1024)}).to_string()); }
    (z.join("\n") + "\n").into_bytes()
}
fn chrono_ms() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64 }
fn iso(ms: i64) -> String {
    // minimal ISO-8601 (UTC) ohne chrono-Abhaengigkeit
    let s = ms / 1000; let (d, r) = (s / 86400, s % 86400);
    let (mut y, mut t) = (1970i64, d);
    loop { let l = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 }; if t >= l { t -= l; y += 1 } else { break } }
    let lm = [31, if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0; while t >= lm[m] { t -= lm[m]; m += 1 }
    format!("{y:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", m + 1, t + 1, r / 3600, (r % 3600) / 60, r % 60)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn echter_client_gegen_neuen_recorder() {
    let Ok(base) = std::env::var("E2E_BASE") else { eprintln!("E2E_BASE fehlt — uebersprungen"); return; };
    let angriff = std::env::var("E2E_ANGRIFF").is_ok();
    let erwarte_neu = std::env::var("E2E_ERWARTE_GESPERRT").is_ok(); // unbekannter Pilot unter Angriff
    let http = reqwest::Client::new();

    if angriff {
        println!("\n>>> ANGRIFF von derselben Adresse (alles 127.0.0.1) <<<");
        let mut z429 = 0;
        for i in 0..420 {
            let r = http.get(format!("{base}/api/navdata/cycle")).header("authorization", format!("Bearer erfunden-{i}-{}", zufall(8))).send().await.unwrap();
            if r.status() == 429 { z429 += 1 }
        }
        println!("  420 Anfragen mit erfundenen Koepfen: {z429} gedrosselt");
        let mut f429 = 0;
        for i in 0..70 {
            let r = http.post(format!("{base}/api/provision")).json(&json!({"api_key": format!("falscher-schluessel-{i}-abcdef")})).send().await.unwrap();
            if r.status() == 429 { f429 += 1 }
        }
        println!("  70 falsche Anmeldungen: {f429} gesperrt");
        let mut g = 0;
        for _ in 0..40 {
            let r = http.put(format!("{base}/api/backup/landings")).header("authorization", "Bearer erfunden-erfunden").header("content-type", "application/json")
                .body(json!({"x": zufall(200 * 1024)}).to_string()).send().await;
            if let Ok(r) = r { if r.status() == 401 { g += 1 } }
        }
        println!("  40 grosse Pakete mit erfundenem Kopf: {g} abgewiesen\n");
    }

    println!("== Anmeldung (echte provision()) ==");
    let ant = provision::provision(KEY, Some(&format!("{base}/api/provision"))).await;
    if erwarte_neu {
        match &ant { Err(e) => println!("  (erwartet, unbekannter Pilot unter Angriff) {e}"), Ok(_) => println!("  Pilot kam durch") }
        return;
    }
    let ant = ant.expect("ANMELDUNG FEHLGESCHLAGEN — ein Pilot koennte sich nicht anmelden");
    let (user, pw) = (ant.username.clone(), ant.password.clone());
    println!("  ✓ angemeldet als {user}");

    let runden = 3;
    for runde in 1..=runden {
        println!("\n== Runde {runde}/{runden}: alle Client-Aufrufe ==");
        let b = Some(base.as_str()); let t = Some(pw.as_str());
        pruefe("navdata::get_cycle", navdata::get_cycle(b, t).await);
        pruefe("navdata::get_airport EDDF", navdata::get_airport("EDDF", b, t).await);
        pruefe("navdata::get_ground_index", navdata::get_ground_index(b, t).await);
        pruefe("navdata::get_airport_ground EDDF", navdata::get_airport_ground("EDDF", b, t, None).await);
        pruefe("navdata::get_aircraft_aliases", navdata::get_aircraft_aliases(b, t).await);
        pruefe("backup::put_landings", backup::put_landings(b, &pw, &[json!({"pirep_id":"E2E1","ts":1,"landing_score":88})]).await);
        pruefe("backup::get_landings", backup::get_landings(b, &pw).await);
        pruefe("chat::verlauf", chat::verlauf(b, t).await);
        pruefe("chat::teilnehmer", chat::teilnehmer(b, t).await);
        pruefe("bordbuch::eintrag_sichern", bordbuch::eintrag_sichern(b, &pw, &json!({"id":"e1","ts":1})).await);
        pruefe("bordbuch::eintraege_holen", bordbuch::eintraege_holen(b, &pw).await);
        pruefe("bordbuch::einstellungen_holen", bordbuch::einstellungen_holen(b, &pw).await);
        pruefe("messung::vermessen", messung::vermessen(b, &pw).await);
        pruefe("messung::lvar_namen (ohne Pfad, aeltere Form)", messung::lvar_namen(b, &pw, "A320", "Fenix", None).await);
        pruefe("messung::lvar_namen (mit SimObject-Pfad, v1.9.12)", messung::lvar_namen(b, &pw, "A320", "Fenix", Some("Community/fenix-a320/SimObjects/Airplanes/Fenix_A320")).await);
        pruefe("messung::senden", messung::senden(b, &pw, &json!({"icao":"A320","titel":"Fenix","werte":{}})).await);

        let dir = std::env::temp_dir();
        let klein = dir.join(format!("e2e-klein-{runde}.jsonl")); std::fs::write(&klein, protokoll(&format!("E2E-K-{runde}"), 0)).unwrap();
        let gross = dir.join(format!("e2e-gross-{runde}.jsonl")); std::fs::write(&gross, protokoll(&format!("E2E-G-{runde}"), 2048)).unwrap();
        let up = format!("{base}/api/flight-logs/upload");
        pruefe("log_upload::upload_flight_log (klein)", log_upload::upload_flight_log(&klein, &format!("E2E-K-{runde}"), &user, &pw, Some(&up)).await);
        pruefe("log_upload::upload_flight_log (GROSS, ueber 64 KB gepackt)", log_upload::upload_flight_log(&gross, &format!("E2E-G-{runde}"), &user, &pw, Some(&up)).await);
        pruefe("log_upload::upload_diagnose_logs", log_upload::upload_diagnose_logs(&[gross.clone()], &format!("E2E-G-{runde}"), &user, &pw, Some(&format!("{base}/api/flight-logs/diagnose"))).await);
        pruefe("log_upload::fremde_route", log_upload::fremde_route(&format!("E2E-K-{runde}"), &user, &pw, Some(&format!("{base}/api/flight-route"))).await);
        pruefe("pirep_status::pruefstatus_abrufen", pirep_status::pruefstatus_abrufen(&[format!("E2E-K-{runde}")], &user, &pw, Some(&format!("{base}/api/flight-logs/pirep-status"))).await);
    }

    println!("\n== Ohne Anmeldung (Oberflaeche der alten Clients: Skin, Karte, Sektoren, Discord) ==");
    for p in ["/api/v2-skin", "/api/basemap", "/api/vatglasses?fl=alle", "/api/public/discord-rpc-config"] {
        let r = http.get(format!("{base}{p}")).send().await.unwrap();
        pruefe(&format!("GET {p}  → {}", r.status()), if ausgesperrt(&r.status().to_string()) { Err(format!("HTTP {}", r.status())) } else { Ok(()) });
    }
    let h = http.head(format!("{base}/api/healthz")).send().await.unwrap();
    pruefe(&format!("HEAD /api/healthz → {}", h.status()), if h.status() == 200 { Ok(()) } else { Err(format!("HTTP {}", h.status())) });

    println!("\n== Flugzeug-Scan (aircraft_scan.rs: nur X-API-Key, ZIP) ==");
    let r = http.get(format!("{base}/api/ascan/vermessen")).header("x-api-key", KEY).send().await.unwrap();
    pruefe(&format!("GET /api/ascan/vermessen mit X-API-Key → {}", r.status()), if r.status() == 200 { Ok(()) } else { Err(format!("HTTP {}", r.status())) });
    let r = http.post(format!("{base}/api/ascan/submissions?source=client&sim=msfs")).header("x-api-key", KEY).header("content-type", "application/zip").body(zufall(300 * 1024).into_bytes()).send().await.unwrap();
    // Kein gueltiges ZIP → 400 "ZIP nicht lesbar" beweist: Zugang und Groesse sind durch.
    pruefe(&format!("POST 300 KB ZIP mit X-API-Key → {}", r.status()), if r.status() == 400 { Ok(()) } else { Err(format!("HTTP {}", r.status())) });

    println!("\n== Dauerlast wie ein aktiver Pilot: 8 parallele Straenge x 40 Aufrufe ==");
    let mut hs = vec![];
    for i in 0..8 {
        let (b, p) = (base.clone(), pw.clone());
        hs.push(tokio::spawn(async move {
            let mut bad = vec![];
            for _ in 0..40 {
                if let Err(e) = navdata::get_cycle(Some(&b), Some(&p)).await { let t = e.to_string(); if ausgesperrt(&t) { bad.push(t) } }
                if let Err(e) = chat::verlauf(Some(&b), Some(&p)).await { let t = e.to_string(); if ausgesperrt(&t) { bad.push(t) } }
            }
            (i, bad)
        }));
    }
    for h in hs { let (i, bad) = h.await.unwrap(); if bad.is_empty() { println!("  ✓ Strang {i}: 80 Aufrufe ohne Sperre") } else { println!("  ✗ Strang {i}: {:?}", &bad[..bad.len().min(2)]); FEHLER.lock().unwrap().push(format!("Dauerlast {i}")); } }

    let f = FEHLER.lock().unwrap();
    assert!(f.is_empty(), "\n\nPILOTEN WAeREN BETROFFEN:\n{}", f.join("\n"));
    println!("\n✓✓ Der echte Client kommt mit dem neuen Server ohne jede Sperre zurecht.");
}
