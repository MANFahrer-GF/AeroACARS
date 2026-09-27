//! Zeigt live, was der X-Plane-Adapter liest — derselbe Code wie im Client.
//!
//!     cargo run -p sim-xplane --example schalter_live [-- <sekunden>]
//!
//! Entstanden am 27.09.2026 zur Abnahme des Existenz-Fixes: RREF liefert
//! auch fehlende Datarefs (als 0), jedes Flugzeug galt als Challenger 650.
//! Hier sieht man pro Zeile, ob das Profil stimmt und ob Klappen,
//! Transponder, Strobes und Anschnallzeichen beim Schalten mitgehen.

use sim_core::SimKind;
use sim_xplane::XPlaneAdapter;

fn main() {
    let sekunden: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(120);
    let mut a = XPlaneAdapter::new();
    a.start(SimKind::XPlane12);
    let ende = std::time::Instant::now() + std::time::Duration::from_secs(sekunden);
    let mut letzte = String::new();
    while std::time::Instant::now() < ende {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let Some(s) = a.snapshot() else { continue };
        // Profil aus dem abonnierten Klappen-Dataref ablesen.
        let profil = a
            .subscribed_datarefs()
            .into_iter()
            .find(|d| d.name.contains("CL650") || d.name.contains("Rotate/"))
            .map(|d| d.name)
            .unwrap_or("Standard");
        let zeile = format!(
            "Profil={profil} | Klappen={:.2} | Transponder={} | Strobe-Schalter={:?} Strobes={:?} | Beacon={:?} | Gurte={:?} | Autobrake={}",
            s.flaps_position,
            s.xpdr_mode_label.as_deref().unwrap_or("-"),
            s.strobe_state,
            s.light_strobe,
            s.light_beacon,
            s.seatbelts_sign,
            s.autobrake.as_deref().unwrap_or("-"),
        );
        if zeile != letzte {
            println!("{}  {zeile}", chrono::Utc::now().format("%H:%M:%S"));
            letzte = zeile;
        }
    }
    a.stop();
}
