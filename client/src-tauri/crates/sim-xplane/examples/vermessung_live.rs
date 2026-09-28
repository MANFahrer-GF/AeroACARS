//! Prüft den „Flugzeug vermessen"-Spiegel gegen ein laufendes X-Plane:
//!
//!     cargo run -p sim-xplane --example vermessung_live
//!
//! Verbindet per Web-API v2, zeigt, wie viele Werte ankommen, und stellt
//! dann über die REST-Schnittstelle einmal die Strobes um — der Spiegel muss
//! die Änderung sehen.

use std::time::{Duration, Instant};

fn setze(id: i64, wert: f64) {
    let _ = ureq::request(
        "PATCH",
        &format!("http://127.0.0.1:8086/api/v2/datarefs/{id}/value"),
    )
    .send_json(serde_json::json!({ "data": wert }));
}

fn main() {
    let t = Instant::now();
    let s = match sim_xplane::vermessung::Spiegel::starten() {
        Ok(s) => s,
        Err(e) => {
            println!("✗ {e}");
            return;
        }
    };
    println!(
        "verbunden in {:?}: {} Datarefs abonniert · Flugzeug {:?}",
        t.elapsed(),
        s.abonniert,
        s.flugzeug.descrip
    );
    let mut vorher = 0;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(500));
        let n = s.verbunden();
        if n == vorher && n > 0 {
            break;
        }
        vorher = n;
    }
    println!(
        "{} Datarefs liefern Werte ({:?})",
        s.verbunden(),
        t.elapsed()
    );
    let a = s.schnappschuss();
    let strobe = "sim/cockpit2/switches/strobe_lights_on";
    let alt = a.get(strobe).copied().unwrap_or(0.0);
    let id: i64 = ureq::get(&format!(
        "http://127.0.0.1:8086/api/v2/datarefs?filter[name]={strobe}"
    ))
    .call()
    .ok()
    .and_then(|r| r.into_json::<serde_json::Value>().ok())
    .and_then(|v| v["data"][0]["id"].as_i64())
    .unwrap_or(0);
    setze(id, if alt > 0.5 { 0.0 } else { 1.0 });
    std::thread::sleep(Duration::from_millis(1500));
    let b = s.schnappschuss();
    println!("Strobe vorher {alt} → nachher {:?}", b.get(strobe));
    let geaendert = a.iter().filter(|(k, v)| b.get(*k) != Some(v)).count();
    println!("{geaendert} Werte haben sich in 1,5 s geändert");
    setze(id, alt);
}
