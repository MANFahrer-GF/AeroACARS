//! Build-Skript der Schalterprüfung.
//!
//! Gleiches Muster wie `client/src-tauri/crates/sim-msfs/build.rs`: bindgen
//! gegen das dort vendorte MSFS-SDK (`ffi/include/Wrapper.h` → `SimConnect.h`)
//! und Linken gegen `ffi/lib/SimConnect.lib`. Die Dateien werden nur GELESEN,
//! nicht kopiert oder verändert — es gibt genau eine SDK-Kopie im Repo.
//!
//! Zusätzlich: `SimConnect.dll` wird verzögert geladen (/DELAYLOAD) und in die
//! Exe eingebettet (siehe `src/sim.rs::dll_bereitstellen`). So reicht dem
//! Piloten EINE Datei; ohne das startete die Exe gar nicht erst, wenn die DLL
//! nicht daneben liegt.
//!
//! Auf Nicht-Windows-Zielen tut das Skript nichts — dort werden nur die
//! plattformunabhängigen Teile geprüft und getestet.

use std::env;
use std::path::PathBuf;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR fehlt"));
    let sdk = manifest_dir.join("../../client/src-tauri/crates/sim-msfs/ffi");
    let lib_dir = sdk.join("lib");
    let out_path = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR fehlt"));

    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rerun-if-changed={}",
        sdk.join("include/SimConnect.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join("SimConnect.lib").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join("SimConnect.dll").display()
    );

    // SimConnect.lib ist die Import-Bibliothek zu SimConnect.dll (sie enthält
    // `__imp_SimConnect_*`). Der Client linkt sie genauso.
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=SimConnect");
    // Verzögert laden: Die DLL wird erst beim ersten SimConnect-Aufruf
    // gebraucht. Bis dahin hat `dll_bereitstellen` die eingebettete Kopie
    // bereits mit vollem Pfad geladen; der Delay-Load-Helfer findet das schon
    // geladene Modul dann über seinen Namen.
    println!("cargo:rustc-link-arg=/DELAYLOAD:SimConnect.dll");
    println!("cargo:rustc-link-lib=dylib=delayimp");
    // Pfad der DLL für `include_bytes!`.
    println!(
        "cargo:rustc-env=SCHALTER_SIMCONNECT_DLL={}",
        lib_dir.join("SimConnect.dll").display()
    );

    generate_bindings(&sdk, &out_path);
}

/// Wie in sim-msfs: am HOST gegated, weil `bindgen` nur unter Windows eine
/// Build-Abhängigkeit ist.
#[cfg(target_os = "windows")]
fn generate_bindings(sdk: &std::path::Path, out_path: &std::path::Path) {
    // ⚠ Positivliste: Was hier fehlt, erzeugt bindgen NICHT, obwohl der
    // Header es deklariert (dieselbe Falle, die im Client zweimal erst in der
    // Windows-CI auffiel).
    let bindings = bindgen::Builder::default()
        .header(sdk.join("include/Wrapper.h").to_string_lossy())
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .clang_args(["-x", "c++"])
        .allowlist_function("SimConnect_Open")
        .allowlist_function("SimConnect_Close")
        .allowlist_function("SimConnect_GetNextDispatch")
        .allowlist_function("SimConnect_GetLastSentPacketID")
        .allowlist_function("SimConnect_AddToDataDefinition")
        // Direktes Lesen der LVars: Block bei Ablehnung neu aufbauen.
        .allowlist_function("SimConnect_ClearDataDefinition")
        .allowlist_function("SimConnect_RequestDataOnSimObject")
        .allowlist_function("SimConnect_MapClientDataNameToID")
        .allowlist_function("SimConnect_CreateClientData")
        .allowlist_function("SimConnect_AddToClientDataDefinition")
        .allowlist_function("SimConnect_RequestClientData")
        // Nur für den MobiFlight-BEFEHLSKANAL ("<Client>.Command"). Damit wird
        // kein Simulatorwert gesetzt; `MF.SimVars.Set.` wird nie gesendet.
        .allowlist_function("SimConnect_SetClientData")
        // Input-Events (B:-Variablen, MSFS 2024) — nur LESEN: Liste, Wert,
        // Änderungs-Abo. `SimConnect_SetInputEvent` bewusst NICHT.
        .allowlist_function("SimConnect_EnumerateInputEvents")
        .allowlist_function("SimConnect_GetInputEvent")
        .allowlist_function("SimConnect_SubscribeInputEvent")
        .allowlist_function("SimConnect_UnsubscribeInputEvent")
        .allowlist_type("SIMCONNECT_RECV")
        .allowlist_type("SIMCONNECT_RECV_ID")
        .allowlist_type("SIMCONNECT_RECV_OPEN")
        .allowlist_type("SIMCONNECT_RECV_EXCEPTION")
        .allowlist_type("SIMCONNECT_RECV_SIMOBJECT_DATA")
        .allowlist_type("SIMCONNECT_RECV_CLIENT_DATA")
        .allowlist_type("SIMCONNECT_DATATYPE")
        .allowlist_type("SIMCONNECT_PERIOD")
        .allowlist_type("SIMCONNECT_CLIENT_DATA_PERIOD")
        .allowlist_var("SIMCONNECT_OBJECT_ID_USER")
        .generate()
        .expect("SimConnect-Bindings konnten nicht erzeugt werden");

    bindings
        .write_to_file(out_path.join("simconnect_bindings.rs"))
        .expect("SimConnect-Bindings konnten nicht geschrieben werden");
}

#[cfg(not(target_os = "windows"))]
fn generate_bindings(_sdk: &std::path::Path, _out_path: &std::path::Path) {
    panic!(
        "schalterpruefung: SimConnect-Bindings lassen sich nur auf einem Windows-Host erzeugen. \
         Den Windows-Build macht die CI (.github/workflows/schalterpruefung.yml)."
    );
}
