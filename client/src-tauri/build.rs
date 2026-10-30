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
    eingebautes_paket_auflisten();
    kennung_festlegen();
    // v3 (Etappe 5, P-8.02/P-8.10): Mit App-Manifest prueft Tauri JEDEN
    // App-Befehl gegen die Capability des Fensters (ohne Manifest gar keinen,
    // tauri-2.11.0 webview/mod.rs:1817). Die Liste kommt aus befehle.json —
    // eine Quelle fuer Stufen, Manifest und erzeugte Capabilities.
    let attribute = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(befehle_aus_liste()));
    tauri_build::try_build(attribute).expect("tauri_build");
}

/// v3 (P-11.21): die Kennung dieses Baus als `AEROACARS_KENNUNG` fuer den
/// Rust-Teil (Log-Ordner, Datenuebernahme), der sie vor dem `AppHandle`
/// braucht. Gleiche Quelle wie Tauri: `tauri.conf.json`, ueberlagert von
/// `TAURI_CONFIG` (setzt die CLI bei `--config`, z. B. die Dev-Kennung aus
/// `tauri.lokal.conf.json`). So nutzt ein Dev-Build nie den Ordner der Beta.
fn kennung_festlegen() {
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    let pfad = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("tauri.conf.json");
    println!("cargo:rerun-if-changed={}", pfad.display());
    let haupt: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&pfad).expect("tauri.conf.json")).expect("tauri.conf.json ist JSON");
    let ueber: Option<serde_json::Value> = std::env::var("TAURI_CONFIG").ok().and_then(|t| serde_json::from_str(&t).ok());
    let kennung = ueber
        .as_ref()
        .and_then(|u| u["identifier"].as_str())
        .or(haupt["identifier"].as_str())
        .expect("identifier in tauri.conf.json");
    println!("cargo:rustc-env=AEROACARS_KENNUNG={kennung}");
}

/// Alle Befehlsnamen aus `befehle.json` (sortiert, fuer das App-Manifest).
fn befehle_aus_liste() -> &'static [&'static str] {
    let pfad = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("befehle.json");
    println!("cargo:rerun-if-changed={}", pfad.display());
    let roh = std::fs::read_to_string(&pfad).expect("befehle.json");
    let v: serde_json::Value = serde_json::from_str(&roh).expect("befehle.json ist JSON");
    let namen: Vec<&'static str> = v["befehle"]
        .as_object()
        .expect("befehle.json: Objekt befehle")
        .keys()
        .map(|k| &*Box::leak(k.clone().into_boxed_str()))
        .collect();
    Box::leak(namen.into_boxed_slice())
}

/// v3 (Etappe 1b): Der eingebaute Stand des Laufzeit-Pakets ist genau der
/// Inhalt von `bundle/` (Gesamtkonzept: keine zweite Wahrheit). Hier entsteht
/// die einzige Liste `(pfad, bytes)`; `laufzeit_paket.rs` bindet sie ein, und
/// jeder, der eine eingebaute Datei braucht (z. B. `runway.rs`), liest daraus.
fn eingebautes_paket_auflisten() {
    let wurzel = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../bundle")
        .canonicalize()
        .expect("bundle/");
    println!("cargo:rerun-if-changed={}", wurzel.display());
    let mut dateien = Vec::new();
    fn sammeln(wurzel: &std::path::Path, ordner: &std::path::Path, aus: &mut Vec<(String, std::path::PathBuf)>) {
        for e in std::fs::read_dir(ordner).expect("bundle/ lesbar").flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let pfad = e.path();
            if pfad.is_dir() {
                sammeln(wurzel, &pfad, aus);
            } else if pfad.is_file() {
                let rel: Vec<String> = pfad
                    .strip_prefix(wurzel)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                aus.push((rel.join("/"), pfad));
            }
        }
    }
    sammeln(&wurzel, &wurzel, &mut dateien);
    dateien.sort();
    let mut code = String::from("pub(crate) const DATEIEN: &[(&str, &[u8])] = &[\n");
    for (rel, pfad) in &dateien {
        code.push_str(&format!("    ({rel:?}, include_bytes!({:?})),\n", pfad.display().to_string()));
    }
    code.push_str("];\n");
    let ziel = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("eingebautes_paket.rs");
    std::fs::write(ziel, code).expect("eingebautes_paket.rs");
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
