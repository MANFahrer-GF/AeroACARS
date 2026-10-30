//! `paket-pruefen <manifest.json> --client-version V --faehigkeiten a,b`
//!
//! P-2.30 (Matrix-Test in der CI): Wie behandelt ein Client dieser Fassung
//! mit diesen Faehigkeiten das neue Paket? Gibt `uebernommen` oder
//! `sauber uebergangen: <Grund>` aus (beides Rueckgabewert 0). Rueckgabewert
//! 1 nur, wenn das Manifest unlesbar ist — dann taete es kein Client.

use runtime_bundle::{vertraeglich, Fehler};

fn main() {
    match lauf() {
        Ok(text) => println!("{text}"),
        Err(e) => {
            eprintln!("paket-pruefen: {e}");
            std::process::exit(1);
        }
    }
}

fn lauf() -> Result<String, String> {
    let mut args = std::env::args().skip(1);
    let pfad = args.next().ok_or("Manifest fehlt")?;
    let (mut version, mut faehigkeiten) = (None, Vec::new());
    while let Some(schalter) = args.next() {
        let wert = args.next().ok_or_else(|| format!("{schalter}: Wert fehlt"))?;
        match schalter.as_str() {
            "--client-version" => version = Some(wert),
            "--faehigkeiten" => faehigkeiten = wert.split(',').filter(|s| !s.is_empty()).map(Into::into).collect(),
            _ => return Err(format!("unbekannt: {schalter}")),
        }
    }
    let version = version.ok_or("--client-version fehlt")?;
    let manifest = std::fs::read(&pfad).map_err(|e| format!("{pfad}: {e}"))?;
    match vertraeglich(&manifest, &version, &faehigkeiten) {
        Ok(()) => Ok(format!("Client {version}: uebernommen")),
        Err(Fehler::Unpassend(grund)) => Ok(format!("Client {version}: sauber uebergangen: {grund}")),
        Err(e) => Err(e.to_string()),
    }
}
