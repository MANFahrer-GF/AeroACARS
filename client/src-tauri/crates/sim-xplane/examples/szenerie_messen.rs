//! Misst den Szenerie-Leser gegen die hier installierte X-Plane-Szenerie.
//!
//!     cargo run --release -p sim-xplane --example szenerie_messen
//!
//! Gibt die Bauzeit des Verzeichnisses, die Abrufzeit je Flughafen und
//! die Gegenprobe gegen den vollen Durchlauf aus. Das Werkzeug gehoert
//! ins Repo und nicht nach /tmp — es hat die Zahlen erzeugt, die in
//! szenerie.rs als Begruendung stehen.

use sim_xplane::szenerie::{flughafen, installationen, SzenerieIndex};
fn main() {
    let Some(wurzel) = installationen().into_iter().next() else {
        println!("  keine Installation");
        return;
    };
    let t = std::time::Instant::now();
    let idx = SzenerieIndex::bauen(&wurzel);
    println!(
        "  Verzeichnis gebaut: {:?}   {} Flughaefen",
        t.elapsed(),
        idx.anzahl()
    );
    println!("  gueltig: {}", idx.gueltig());
    for icao in ["EDDH", "FACT", "KJFK", "EGPR", "EDDV", "EDHE"] {
        let t = std::time::Instant::now();
        let f = idx.flughafen(icao);
        let d = t.elapsed();
        // Gegenprobe: derselbe Platz ueber den vollen Durchlauf.
        let v = flughafen(icao);
        let gleich = match (&f, &v) {
            (Some(a), Some(b)) => a.bahnen == b.bahnen && a.rollwege == b.rollwege,
            (None, None) => true,
            _ => false,
        };
        println!(
            "  {icao}  {:>10?}   {} Bahnen, {} Rollwege   gleich wie voller Durchlauf: {}",
            d,
            f.as_ref().map(|x| x.bahnen.len()).unwrap_or(0),
            f.as_ref().map(|x| x.rollwege.len()).unwrap_or(0),
            if gleich { "JA" } else { "NEIN" }
        );
    }
}
