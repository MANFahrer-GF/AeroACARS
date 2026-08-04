//! Build script for sim-msfs.
//!
//! Generates Rust FFI bindings against the vendored MSFS 2024
//! SimConnect SDK (`ffi/include/Wrapper.h` → `SimConnect.h`) and
//! tells Cargo to link statically against `ffi/lib/SimConnect.lib`.
//!
//! On non-Windows targets the script is a no-op so the workspace
//! still builds (sim-msfs is gated behind `cfg(target_os = "windows")`
//! everywhere it matters).

use std::env;
use std::path::PathBuf;

fn main() {
    // Skip everything outside Windows — the SimConnect SDK is a
    // Windows-only library. The crate itself stubs out into a no-op
    // type on other platforms (see lib.rs).
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    println!("cargo:rerun-if-changed=ffi/include/Wrapper.h");
    println!("cargo:rerun-if-changed=ffi/include/SimConnect.h");
    println!("cargo:rerun-if-changed=ffi/lib/SimConnect.lib");
    println!("cargo:rerun-if-changed=ffi/lib/SimConnect.dll");

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let out_path = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let lib_dir = manifest_dir.join("ffi/lib");

    // Static link against SimConnect.lib. The matching SimConnect.dll
    // needs to be next to the produced .exe at runtime — Tauri's
    // resource pipeline handles that, but we also copy it into OUT_DIR
    // so a plain `cargo run` works during dev.
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=SimConnect");

    // Copy SimConnect.dll into OUT_DIR (where the linker looks) AND
    // — critically for runtime — into the directory that holds the
    // final binary. Cargo doesn't expose that path directly, so we
    // climb three levels out of OUT_DIR:
    //   target/<profile>/build/<crate>-<hash>/out  →  target/<profile>
    // That's where the aeroacars-app.exe lives during `cargo run`,
    // so the OS loader will find SimConnect.dll next to it.
    for file in &["SimConnect.dll", "SimConnect.lib"] {
        let src = lib_dir.join(file);
        let dst = out_path.join(file);
        if let Err(e) = std::fs::copy(&src, &dst) {
            eprintln!("warning: could not copy {file} to OUT_DIR: {e}");
        }
    }
    if let Some(target_dir) = out_path
        .ancestors()
        .nth(3)
        // ancestors yields self, so skip(0)→OUT_DIR, skip(1)→out's parent, etc.
        // We want target/<profile>; that's three levels up from the
        // ".../build/<crate>-<hash>/out" leaf.
    {
        let dll_dst = target_dir.join("SimConnect.dll");
        match std::fs::copy(lib_dir.join("SimConnect.dll"), &dll_dst) {
            Ok(_) => {
                println!(
                    "cargo:warning=sim-msfs: copied SimConnect.dll to {}",
                    dll_dst.display()
                );
            }
            Err(e) => {
                println!(
                    "cargo:warning=sim-msfs: could not copy SimConnect.dll to {}: {e}",
                    dll_dst.display()
                );
            }
        }
    }

    generate_bindings(&manifest_dir, &out_path);
}

/// Generate the SimConnect bindings.
///
/// Gated on the HOST, not the target: a build script is compiled for the
/// host, and Cargo resolves `[target.'cfg(...)'.build-dependencies]`
/// against the host too — so on macOS/Linux `bindgen` is not a
/// dependency at all and the name doesn't resolve. A runtime
/// `CARGO_CFG_TARGET_OS` check can't help, because the whole file still
/// has to type-check. Without this split `cargo build --workspace` fails
/// with E0433 on any non-Windows machine.
#[cfg(target_os = "windows")]
fn generate_bindings(manifest_dir: &PathBuf, out_path: &PathBuf) {
    // Run bindgen against the wrapper. We allowlist what we actually
    // use so the generated file stays small and compiles fast.
    let bindings = bindgen::Builder::default()
        .header(manifest_dir.join("ffi/include/Wrapper.h").to_string_lossy())
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .clang_args(["-x", "c++"])
        // --- Functions ---
        .allowlist_function("SimConnect_Open")
        .allowlist_function("SimConnect_Close")
        .allowlist_function("SimConnect_AddToDataDefinition")
        .allowlist_function("SimConnect_ClearDataDefinition")
        .allowlist_function("SimConnect_RequestDataOnSimObject")
        .allowlist_function("SimConnect_CallDispatch")
        .allowlist_function("SimConnect_GetNextDispatch")
        // Correlates an async SIMCONNECT_RECV_EXCEPTION back to the
        // specific AddToDataDefinition call that caused it, via its
        // dwSendID — needed to attribute inspector-watch registration
        // failures to the right watch entry (see register_inspector).
        .allowlist_function("SimConnect_GetLastSentPacketID")
        // --- Receiver structs we actually inspect ---
        .allowlist_type("SIMCONNECT_RECV")
        .allowlist_type("SIMCONNECT_RECV_ID")
        .allowlist_type("SIMCONNECT_RECV_OPEN")
        .allowlist_type("SIMCONNECT_RECV_QUIT")
        .allowlist_type("SIMCONNECT_RECV_EXCEPTION")
        .allowlist_type("SIMCONNECT_RECV_SIMOBJECT_DATA")
        // ClientData receiver — `SIMCONNECT_RECV_CLIENT_DATA` is
        // the same shape as SIMOBJECT_DATA but with a different
        // RECV_ID, so SimConnect routes the bytes properly.
        .allowlist_type("SIMCONNECT_RECV_CLIENT_DATA")
        // System state events for PMDG aircraft change detection.
        .allowlist_type("SIMCONNECT_RECV_SYSTEM_STATE")
        .allowlist_type("SIMCONNECT_RECV_EVENT")
        .allowlist_type("SIMCONNECT_EXCEPTION")
        .allowlist_type("SIMCONNECT_DATATYPE")
        .allowlist_type("SIMCONNECT_PERIOD")
        .allowlist_type("SIMCONNECT_CLIENT_DATA_PERIOD")
        .allowlist_type("SIMCONNECT_DATA_REQUEST_FLAG")
        .allowlist_type("SIMCONNECT_CLIENT_DATA_REQUEST_FLAG")
        // ClientData functions — needed for PMDG SDK integration
        // (Phase H.4). PMDG ships a custom ClientData channel
        // ("PMDG_NG3_Data" / "PMDG_777X_Data") with the full
        // cockpit state; subscribing requires these three:
        .allowlist_function("SimConnect_MapClientDataNameToID")
        .allowlist_function("SimConnect_AddToClientDataDefinition")
        .allowlist_function("SimConnect_RequestClientData")
        // System state — used to detect which aircraft is loaded
        // (PMDG variant detection from the .air file path).
        .allowlist_function("SimConnect_RequestSystemState")
        .allowlist_function("SimConnect_SubscribeToSystemEvent")
        // --- Constants we reference ---
        .allowlist_var("SIMCONNECT_OBJECT_ID_USER")
        .allowlist_var("SIMCONNECT_DATA_REQUEST_FLAG_DEFAULT")
        .allowlist_var("SIMCONNECT_DATA_REQUEST_FLAG_CHANGED")
        .allowlist_var("SIMCONNECT_CLIENT_DATA_REQUEST_FLAG_DEFAULT")
        .allowlist_var("SIMCONNECT_CLIENT_DATA_REQUEST_FLAG_CHANGED")
        .generate()
        .expect("Unable to generate SimConnect bindings");

    bindings
        .write_to_file(out_path.join("simconnect_bindings.rs"))
        .expect("Failed to write SimConnect bindings");
}

/// Non-Windows host. Reached only when cross-compiling *to* Windows from
/// e.g. macOS — the SimConnect SDK is Windows-only and `bindgen` isn't
/// even a dependency here, so fail loudly rather than emit nothing and
/// let the crate fail later with a confusing missing-symbol error.
/// Windows builds run on Windows CI runners, so this path is not part of
/// any normal flow.
#[cfg(not(target_os = "windows"))]
fn generate_bindings(_manifest_dir: &PathBuf, _out_path: &PathBuf) {
    panic!(
        "sim-msfs: cannot generate SimConnect bindings on a non-Windows host. \
         Cross-compiling to Windows is not supported — build on Windows."
    );
}
