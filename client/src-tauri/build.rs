fn main() {
    // v0.9.0 (#GlitchTip): Damit `option_env!("AEROACARS_SENTRY_DSN")` in
    // `src/sentry_init.rs` zur Build-Zeit ausgewertet wird, muss Cargo
    // bei Aenderung des env neu kompilieren. Sonst bleibt der eingebrannte
    // Wert aus dem ersten Build cached.
    println!("cargo:rerun-if-env-changed=AEROACARS_SENTRY_DSN");
    // v0.9.0 (#Discord-RPC): App-ID kommt zur LAUFZEIT vom Server-Endpoint
    // (siehe src/discord_rpc.rs refresh_app_id) — daher KEIN env-changed-Trigger
    // fuer AEROACARS_DISCORD_APP_ID. Bewusst.
    produktion_zusammensetzen();
    tauri_build::build()
}

/// v3 (Etappe 0): `lib.rs` wird in Dateien zerlegt. Die Text-Waechter
/// (`src/tests_lib/`, `tests/*.rs`) pruefen Verdrahtung im Quelltext und
/// lesen deshalb `OUT_DIR/produktion.txt`: `lib.rs`, in dem jede
/// `include!("x.rs");`-Zeile durch den Dateiinhalt und jedes ausgelagerte
/// `mod x;` durch `mod x { … }` ersetzt ist — derselbe Text wie vor der
/// Zerlegung. Nur fuer Tests; das Programm selbst liest die Datei nie.
fn produktion_zusammensetzen() {
    // Module, die vorher als `mod x { … }` in lib.rs standen.
    const AUSGELAGERTE_MODULE: &[&str] = &["pirep_queue", "nachtrag_queue"];

    let wurzel = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let lesen = |name: &str| {
        let pfad = wurzel.join("src").join(name);
        println!("cargo:rerun-if-changed={}", pfad.display());
        std::fs::read_to_string(&pfad).unwrap_or_else(|e| panic!("{}: {e}", pfad.display()))
    };
    let lib = lesen("lib.rs");
    let mut aus = String::with_capacity(lib.len() * 2);
    for zeile in lib.split_inclusive('\n') {
        let inhalt = zeile.trim_end_matches('\n');
        if let Some(datei) = inhalt
            .strip_prefix("include!(\"")
            .and_then(|r| r.strip_suffix("\");"))
        {
            aus.push_str(&lesen(datei));
        } else if let Some(name) = inhalt
            .strip_prefix("mod ")
            .and_then(|r| r.strip_suffix(';'))
            .filter(|n| AUSGELAGERTE_MODULE.contains(n))
        {
            aus.push_str(&format!("mod {name} {{\n{}}}\n", lesen(&format!("{name}.rs"))));
        } else {
            aus.push_str(zeile);
        }
    }
    let ziel = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("produktion.txt");
    std::fs::write(ziel, aus).expect("produktion.txt");
}
