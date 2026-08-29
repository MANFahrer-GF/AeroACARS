//! The `POST /api/cmd/{name}` dispatch table.
//!
//! ## How it works
//!
//! Each Tauri command is a plain `async`/sync fn taking some subset of
//! `(app: AppHandle, state: tauri::State<'_, AppState>, ...named args)`.
//! `app.state::<AppState>()` yields a `tauri::State` exactly as the IPC
//! layer would, so we can call any command *directly* from here — the
//! same trick the auto-start watcher uses (`flight_start(app, app
//! .state::<AppState>(), bid_id, None)`, lib.rs).
//!
//! The HTTP body is a JSON object of the command's **named** args (the
//! same names the Tauri `invoke` front-end sends). We deserialize it into
//! a small per-command `#[derive(Deserialize)]` struct, call the fn, and
//! normalize the return to `Result<serde_json::Value, UiError>`:
//!
//! - a command returning `T` → `Ok(json(T))`,
//! - a command returning `Result<T, UiError>` → propagated,
//! - a command returning `Result<T, String>` (a few legacy ones) → the
//!   `String` is wrapped in a `UiError` so the wire shape is uniform,
//! - `()` → `Ok(null)`.
//!
//! The router maps `Ok` → HTTP 200, `Err(UiError)` → HTTP 422
//! `{code,message}`, and an unknown command name → HTTP 404.
//!
//! ## What is covered
//!
//! **Everything.** v1.5.6 (#lan-bruecke-1zu1) — Vorgabe Thomas: "die
//! LAN-Bruecke soll alles 1 zu 1 machen". Jeder Befehl aus der
//! `generate_handler!`-Liste (lib.rs) wird hier dispatcht; der Guard-Test
//! am Dateiende haelt das durch und hat KEINE Ausnahmeliste mehr.
//!
//! Frueher gab es ein Deny-Set (X-Plane-Plugin-Installation,
//! Fehlerbericht-Einwilligung, die Server-Schalter selbst) plus eine
//! "noch nicht triagiert"-Liste, in der u. a. die OSM-Bodendaten lagen —
//! weshalb die Live-Karte auf dem Tablet monatelang ohne Rollwege
//! dastand. Beide Listen sind leer. Die Befehle laufen ohnehin alle auf
//! dem HOST; das Tablet ist die Fernbedienung, nicht der Ausfuehrende.
//!
//! Zwei Ehrlichkeits-Fussnoten stehen an ihren Arms: `remote_server_stop`
//! /`_set_port` kappen die eigene Leitung (gewollt, wenn der Pilot den
//! Schalter umlegt), und der Aircraft-Scan kann seinen OPTIONALEN
//! "Ordner waehlen"-Dialog im Browser nicht oeffnen — die Automatik und
//! ein getippter Pfad funktionieren.
//!
//! Es gibt KEINE updater/process/relaunch/Window-Befehle in der
//! Handler-Liste; die Regel "solche nicht bruecken" ist also erfuellt,
//! weil es sie schlicht nicht gibt.

use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::remote::RemoteContext;
use crate::{AppState, UiError};

/// Result of looking up + running a command by name.
pub enum Dispatch {
    /// Command ran; here is its normalized JSON result (Ok) or UiError.
    Handled(Result<Value, UiError>),
    /// No command with that name is bridged.
    Unknown,
}

/// Deserialize `body` into a command-arg struct `T`. A malformed body
/// (missing/extra/typed-wrong fields) becomes a `UiError` so the caller
/// sees a clean 422 instead of a 500.
fn parse_args<T: for<'de> Deserialize<'de>>(body: &Value) -> Result<T, UiError> {
    serde_json::from_value(body.clone()).map_err(|e| {
        UiError::new(
            "bad_request",
            format!("ungültige Argumente für den Befehl: {e}"),
        )
    })
}

/// Serialize a command's success value to JSON. Infallible in practice
/// (all return types are `Serialize`); a failure degrades to `null`.
fn ok_json<T: serde::Serialize>(v: T) -> Result<Value, UiError> {
    Ok(serde_json::to_value(v).unwrap_or(Value::Null))
}

/// Wrap a legacy `Result<T, String>` command's error string in a UiError.
fn from_string_err<T: serde::Serialize>(r: Result<T, String>) -> Result<Value, UiError> {
    match r {
        Ok(v) => ok_json(v),
        Err(msg) => Err(UiError::new("command_error", msg)),
    }
}

/// Map a `Result<T, UiError>` command result to the normalized form.
fn from_uierr<T: serde::Serialize>(r: Result<T, UiError>) -> Result<Value, UiError> {
    r.and_then(ok_json)
}

/// Run the named command with the given JSON args object.
///
/// `body` must be a JSON object (the router guarantees this). `app` is a
/// fresh clone per request; `state` is taken via `app.state::<AppState>()`
/// inside each arm exactly as Tauri's IPC does.
pub async fn dispatch(ctx: &RemoteContext, name: &str, body: &Value) -> Dispatch {
    let app: AppHandle = ctx.app.clone();

    // --- Macro for the common shapes -------------------------------------
    //
    // Spelled out per-shape because the commands vary across four axes:
    // takes-app, takes-state, async/sync, and result-kind. Trying to make
    // ONE arm cover all of that is less readable than a handful of focused
    // match arms; the macro just removes the parse+await+normalize
    // boilerplate that every arm would otherwise repeat.

    macro_rules! st {
        () => {
            app.state::<AppState>()
        };
    }

    let result: Result<Value, UiError> = match name {
        // ============================ READS ==============================
        "app_info" => ok_json(crate::app_info()),
        // Reine Auskunft ueber die Platte des Hosts. Ueber die Bruecke
        // erlaubt, weil das Tablet dieselbe Frage stellen darf wie der
        // PC — es liest nichts, was der Pilot nicht ohnehin sieht.
        "navdata_zwischenspeicher_bestand" => {
            ok_json(crate::navdata_zwischenspeicher_bestand(app.clone()))
        }
        "sim_status" => ok_json(crate::sim_status(app.clone(), st!())),
        "sim_get_kind" => ok_json(crate::sim_get_kind(app.clone())),
        "pmdg_status" => ok_json(crate::pmdg_status(st!())),
        "flight_status" => ok_json(crate::flight_status(app.clone(), st!())),
        "flight_get_track" => ok_json(crate::flight_get_track(st!())),
        "flight_get_route_fixes" => ok_json(crate::flight_get_route_fixes(st!())),
        "activity_log_get" => ok_json(crate::activity_log_get(st!())),
        "landing_get_current" => ok_json(crate::landing_get_current(app.clone(), st!())),
        "landing_list" => ok_json(crate::landing_list(app.clone())),
        "auto_start_skip_status" => ok_json(crate::auto_start_skip_status(st!())),
        "auto_start_get_enabled" => ok_json(crate::auto_start_get_enabled(st!())),
        "ofp_callsign_warning_get" => ok_json(crate::ofp_callsign_warning_get(st!())),
        "inspector_list" => ok_json(crate::inspector_list(st!())),
        "xplane_inspector_list" => ok_json(crate::xplane_inspector_list(st!())),
        "xplane_premium_status" => ok_json(crate::xplane_premium_status(st!())),

        "va_live_flights" => from_uierr(crate::va_live_flights(st!()).await),
        // Pilotenchat: eins zu eins. Auf dem Tablet ist Tippen ohnehin
        // unbedenklich (eigene Tastatur, eigener Fokus) — deshalb hier keine
        // abgespeckte Fassung, sondern dieselben Befehle wie am PC.
        "chat_senden" => {
            // rename_all ist hier NICHT optional: die Oberflaeche schickt
            // `anPilotId`. Ohne die Umwandlung liest `an_pilot_id` still
            // `None` — und jede Direktnachricht vom Tablet ginge an ALLE,
            // waehrend das Fenster weiter "Direkt an ..." anzeigt.
            // Derselbe Fehler ist in dieser Datei schon einmal passiert
            // (divertTo, siehe weiter unten) — deshalb steht er ueber jedem
            // Mehrwort-Argument.
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                text: String,
                #[serde(default)]
                an_pilot_id: Option<String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::chat_senden(st!(), a.text, a.an_pilot_id).await),
                Err(e) => Err(e),
            }
        }
        "chat_verlauf" => from_uierr(crate::chat_verlauf().await),
        "chat_teilnehmer" => from_uierr(crate::chat_teilnehmer().await),
        "logbook_stats" => from_uierr(crate::logbook_stats(st!()).await),
        "phpvms_get_bids" => from_uierr(crate::phpvms_get_bids(st!()).await),
        "news_fetch" => from_uierr(crate::news_fetch(st!()).await),
        "phpvms_refresh_profile" => from_uierr(crate::phpvms_refresh_profile(st!()).await),
        "flight_list_orphans" => from_uierr(crate::flight_list_orphans(st!()).await),
        "flight_discover_resumable" => {
            from_uierr(crate::flight_discover_resumable(app.clone(), st!()).await)
        }
        // Lesende Frage ohne Netz: Liegt ein gespeicherter Flug? Die
        // Fernbedienung zeigt denselben Pflicht-Update-Riegel und braucht
        // dieselbe Antwort — sonst sperrte sie im Wettrennen nach einem
        // Neustart, waehrend der Pilot fliegt.
        "flight_wiederaufnahme_steht_aus" => Ok(serde_json::json!(
            crate::flight_wiederaufnahme_steht_aus(app.clone(), st!())
        )),

        "metar_get" => {
            #[derive(Deserialize)]
            struct A {
                icao: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::metar_get(a.icao).await),
                Err(e) => Err(e),
            }
        }
        "airport_get" => {
            #[derive(Deserialize)]
            struct A {
                icao: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::airport_get(st!(), a.icao).await),
                Err(e) => Err(e),
            }
        }
        // v1.5.6 (#lan-bruecke-1zu1): OSM-Bodendaten. OHNE die beiden
        // zeichnet die Live-Karte auf dem Tablet keine Rollwege, Staende
        // und Haltepunkte — der Feldbefund, der diese Runde ausgeloest hat.
        "airport_ground_get" => {
            #[derive(Deserialize)]
            struct A {
                icao: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::airport_ground_get(app.clone(), a.icao).await),
                Err(e) => Err(e),
            }
        }
        "airport_ground_index" => from_uierr(crate::airport_ground_index().await),
        "phpvms_get_aircraft" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                aircraft_id: i64,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::phpvms_get_aircraft(st!(), a.aircraft_id).await),
                Err(e) => Err(e),
            }
        }
        "fleet_list_at_airport" => {
            #[derive(Deserialize)]
            struct A {
                icao: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::fleet_list_at_airport(st!(), a.icao).await),
                Err(e) => Err(e),
            }
        }
        "logbook_pireps" => {
            #[derive(Deserialize)]
            struct A {
                limit: u32,
                offset: u32,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::logbook_pireps(st!(), a.limit, a.offset).await),
                Err(e) => Err(e),
            }
        }
        "logbook_pirep" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::logbook_pirep(st!(), a.id).await),
                Err(e) => Err(e),
            }
        }
        "divert_nearest_airports" => {
            #[derive(Deserialize)]
            struct A {
                #[serde(default)]
                limit: Option<usize>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::divert_nearest_airports(st!(), a.limit)),
                Err(e) => Err(e),
            }
        }
        "fetch_release_notes" => {
            #[derive(Deserialize)]
            struct A {
                version: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::fetch_release_notes(a.version).await),
                Err(e) => Err(e),
            }
        }
        "fetch_simbrief_preview" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                ofp_id: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::fetch_simbrief_preview(st!(), a.ofp_id).await),
                Err(e) => Err(e),
            }
        }
        "bid_simbrief_preview" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                bid_id: i64,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::bid_simbrief_preview(a.bid_id, st!()).await),
                Err(e) => Err(e),
            }
        }

        // ===================== FLIGHT CONTROL ============================
        "flight_start" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                bid_id: i64,
                #[serde(default)]
                acknowledge_aircraft_mismatch: Option<bool>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::flight_start(
                        app.clone(),
                        st!(),
                        a.bid_id,
                        a.acknowledge_aircraft_mismatch,
                    )
                    .await,
                ),
                Err(e) => Err(e),
            }
        }
        "flight_start_manual" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                bid_id: i64,
                plan: crate::ManualFlightPlan,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::flight_start_manual(app.clone(), st!(), a.bid_id, a.plan).await,
                ),
                Err(e) => Err(e),
            }
        }
        "flight_end" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                #[serde(default)]
                divert_to: Option<String>,
                #[serde(default)]
                divert_reason: Option<String>,
                #[serde(default)]
                accident_decision: Option<String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::flight_end(
                        app.clone(),
                        st!(),
                        a.divert_to,
                        a.divert_reason,
                        a.accident_decision,
                    )
                    .await,
                ),
                Err(e) => Err(e),
            }
        }
        "flight_end_manual" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            #[allow(clippy::struct_excessive_bools)]
            struct A {
                #[serde(default)]
                notes_override: Option<String>,
                #[serde(default)]
                divert_to: Option<String>,
                #[serde(default)]
                reason: Option<String>,
                #[serde(default)]
                flight_time_minutes: Option<i32>,
                #[serde(default)]
                block_fuel_kg: Option<f32>,
                #[serde(default)]
                remaining_fuel_kg: Option<f32>,
                #[serde(default)]
                distance_nm: Option<f64>,
                #[serde(default)]
                cruise_level_ft: Option<i32>,
                #[serde(default)]
                landing_rate_fpm: Option<f32>,
                #[serde(default)]
                block_off_at_iso: Option<String>,
                #[serde(default)]
                block_on_at_iso: Option<String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::flight_end_manual(
                        app.clone(),
                        st!(),
                        a.notes_override,
                        a.divert_to,
                        a.reason,
                        a.flight_time_minutes,
                        a.block_fuel_kg,
                        a.remaining_fuel_kg,
                        a.distance_nm,
                        a.cruise_level_ft,
                        a.landing_rate_fpm,
                        a.block_off_at_iso,
                        a.block_on_at_iso,
                    )
                    .await,
                ),
                Err(e) => Err(e),
            }
        }
        "flight_cancel" => {
            #[derive(Deserialize)]
            struct A {
                #[serde(default)]
                force: Option<bool>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::flight_cancel(app.clone(), st!(), a.force).await),
                Err(e) => Err(e),
            }
        }
        "flight_forget" => from_uierr(crate::flight_forget(app.clone(), st!()).await),
        "flight_resume_after_disconnect" => {
            from_uierr(crate::flight_resume_after_disconnect(app.clone(), st!()).await)
        }
        "flight_resume_check_position" => {
            from_uierr(crate::flight_resume_check_position(app.clone(), st!()).await)
        }
        "flight_resume_confirm" => {
            from_uierr(crate::flight_resume_confirm(app.clone(), st!()).await)
        }
        "flight_refresh_simbrief" => {
            from_uierr(crate::flight_refresh_simbrief(app.clone(), st!()).await)
        }
        // v1.5.6 (#lan-bruecke-1zu1): Route nachladen ohne den ganzen OFP.
        "flight_refresh_route_only" => {
            from_uierr(crate::flight_refresh_route_only(app.clone(), st!()).await)
        }
        "flight_adopt" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                pirep_id: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::flight_adopt(app.clone(), st!(), a.pirep_id).await),
                Err(e) => Err(e),
            }
        }
        "flight_cancel_orphan" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                pirep_id: String,
                #[serde(default)]
                bid_id: Option<i64>,
                #[serde(default)]
                flight_id: Option<String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::flight_cancel_orphan(
                        app.clone(),
                        st!(),
                        a.pirep_id,
                        a.bid_id,
                        a.flight_id,
                    )
                    .await,
                ),
                Err(e) => Err(e),
            }
        }
        "flight_forget_remote" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                pirep_id: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => {
                    from_uierr(crate::flight_forget_remote(app.clone(), st!(), a.pirep_id).await)
                }
                Err(e) => Err(e),
            }
        }

        // ============================ LOGIN ==============================
        "phpvms_login" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                url: String,
                api_key: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => {
                    from_uierr(crate::phpvms_login(app.clone(), st!(), a.url, a.api_key).await)
                }
                Err(e) => Err(e),
            }
        }
        "phpvms_logout" => from_uierr(crate::phpvms_logout(app.clone(), st!()).await),
        // v0.16.2: the paired tablet inherits the sim-PC's logged-in session.
        // The frontend's startup login-check (App.tsx) calls this; bridging it
        // means the tablet skips the API-key login page (the backend already
        // holds the session). Returns Option<LoginResult> (profile, NOT the key).
        "phpvms_load_session" => from_uierr(crate::phpvms_load_session(app.clone(), st!()).await),

        // ========================== SETTINGS =============================
        // v1.5.6 (#lan-bruecke-1zu1): gespiegelter UI-Zustand — ohne die
        // drei sieht das Tablet leere SimBrief-Felder und alle Nachrichten
        // als ungelesen (siehe crate::ui_state).
        "ui_state_get_all" => ok_json(crate::ui_state::ui_state_get_all(app.clone())),
        "ui_state_set" => {
            #[derive(Deserialize)]
            struct A {
                key: String,
                #[serde(default)]
                value: Option<String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => {
                    crate::ui_state::ui_state_set(app.clone(), a.key, a.value);
                    Ok(Value::Null)
                }
                Err(e) => Err(e),
            }
        }
        "ui_state_seed" => {
            #[derive(Deserialize)]
            struct A {
                values: std::collections::HashMap<String, String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => ok_json(crate::ui_state::ui_state_seed(app.clone(), a.values)),
                Err(e) => Err(e),
            }
        }
        // v1.5.6 (#lan-bruecke-1zu1): Einwilligung zur Fehlerberichterstattung.
        "error_reporting_set_consent" => {
            #[derive(Deserialize)]
            struct A {
                enabled: bool,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::error_reporting_set_consent(a.enabled)),
                Err(e) => Err(e),
            }
        }
        // v1.5.6: X-Plane-Plugin-Installation. Beides laeuft auf dem HOST
        // (dessen X-Plane-Ordner) — das Tablet ist nur die Fernbedienung.
        "xplane_detect_install_path" => ok_json(crate::xplane_detect_install_path().await),
        "xplane_install_plugin" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                install_dir: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_string_err(crate::xplane_install_plugin(a.install_dir).await),
                Err(e) => Err(e),
            }
        }
        // v1.5.6: Aircraft-Scan. Der Scan selbst laeuft auf dem HOST; nur der
        // OPTIONALE "Ordner manuell waehlen"-Knopf braucht einen nativen
        // Datei-Dialog, den ein Browser nicht hat — die Automatik (MSFS-
        // Community-Ordner / X-Plane-Aircraft aus den Sim-Configs) greift
        // ohne ihn, der Pilot kann den Pfad zur Not tippen.
        "ascan_list_aircraft" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                #[serde(default)]
                manual_dir: Option<String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_string_err(
                    crate::aircraft_scan::ascan_list_aircraft(
                        app.state::<crate::aircraft_scan::AircraftScanState>(),
                        a.manual_dir,
                    )
                    .await,
                ),
                Err(e) => Err(e),
            }
        }
        "ascan_collect" => {
            #[derive(Deserialize)]
            struct A {
                index: usize,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_string_err(
                    crate::aircraft_scan::ascan_collect(
                        app.state::<crate::aircraft_scan::AircraftScanState>(),
                        a.index,
                    )
                    .await,
                ),
                Err(e) => Err(e),
            }
        }
        "ascan_submit" => {
            #[derive(Deserialize)]
            struct A {
                index: usize,
                #[serde(default)]
                endpoint: Option<String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_string_err(
                    crate::aircraft_scan::ascan_submit(
                        app.state::<crate::aircraft_scan::AircraftScanState>(),
                        a.index,
                        a.endpoint,
                    )
                    .await,
                ),
                Err(e) => Err(e),
            }
        }
        "set_minimize_to_tray" => {
            #[derive(Deserialize)]
            struct A {
                enabled: bool,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::set_minimize_to_tray(st!(), a.enabled)),
                Err(e) => Err(e),
            }
        }
        "set_auto_file_enabled" => {
            #[derive(Deserialize)]
            struct A {
                enabled: bool,
            }
            match parse_args::<A>(body) {
                Ok(a) => {
                    crate::set_auto_file_enabled(a.enabled, st!());
                    Ok(Value::Null)
                }
                Err(e) => Err(e),
            }
        }
        "auto_start_set_enabled" => {
            #[derive(Deserialize)]
            struct A {
                enabled: bool,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::auto_start_set_enabled(a.enabled, app.clone(), st!())),
                Err(e) => Err(e),
            }
        }
        "sim_set_kind" => {
            #[derive(Deserialize)]
            struct A {
                kind: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::sim_set_kind(app.clone(), st!(), a.kind)),
                Err(e) => Err(e),
            }
        }
        "sim_force_resync" => {
            crate::sim_force_resync(app.clone(), st!());
            Ok(Value::Null)
        }
        "set_simbrief_settings" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                #[serde(default)]
                username: Option<String>,
                #[serde(default)]
                user_id: Option<String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::set_simbrief_settings(st!(), a.username, a.user_id)),
                Err(e) => Err(e),
            }
        }
        "verify_simbrief_identifier" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                #[serde(default)]
                username: Option<String>,
                #[serde(default)]
                user_id: Option<String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::verify_simbrief_identifier(st!(), a.username, a.user_id).await,
                ),
                Err(e) => Err(e),
            }
        }

        // ===================== ACTIVITY / LANDING / LOGS =================
        "activity_log_clear" => {
            crate::activity_log_clear(st!());
            Ok(Value::Null)
        }
        "landing_delete" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                pirep_id: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::landing_delete(app.clone(), a.pirep_id)),
                Err(e) => Err(e),
            }
        }
        // v1.5.6 (#lan-bruecke-1zu1): Landungs-Sicherung. Laeuft auf dem
        // HOST (schreibt/liest dessen Datei) — das Tablet loest sie nur aus.
        "landing_backup_now" => from_uierr(crate::landing_backup_now(app.clone()).await),
        "landing_backup_restore" => from_uierr(crate::landing_backup_restore(app.clone()).await),
        "flight_logs_stats" => from_uierr(crate::flight_logs_stats(app.clone())),
        "flight_logs_delete_all" => from_uierr(crate::flight_logs_delete_all(app.clone())),
        "flight_logs_purge_older_than" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                older_than_days: u32,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::flight_logs_purge_older_than(
                    app.clone(),
                    a.older_than_days,
                )),
                Err(e) => Err(e),
            }
        }

        // ========================= INSPECTORS ============================
        "inspector_add" => {
            #[derive(Deserialize)]
            struct A {
                args: crate::InspectorAddArgs,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::inspector_add(st!(), a.args)),
                Err(e) => Err(e),
            }
        }
        "inspector_remove" => {
            #[derive(Deserialize)]
            struct A {
                id: u32,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::inspector_remove(st!(), a.id)),
                Err(e) => Err(e),
            }
        }

        // ========================== DISCORD RPC ==========================
        "discord_rpc_get_settings" => {
            from_string_err(crate::discord_rpc::discord_rpc_get_settings().await)
        }
        "discord_rpc_get_status" => {
            from_string_err(crate::discord_rpc::discord_rpc_get_status().await)
        }
        "discord_rpc_send_test" => {
            from_string_err(crate::discord_rpc::discord_rpc_send_test().await)
        }
        "discord_rpc_clear_flight" => {
            from_string_err(crate::discord_rpc::discord_rpc_clear_flight().await)
        }
        "discord_rpc_set_settings" => {
            #[derive(Deserialize)]
            struct A {
                settings: discord_presence::DiscordPresenceSettings,
            }
            match parse_args::<A>(body) {
                Ok(a) => {
                    from_string_err(crate::discord_rpc::discord_rpc_set_settings(a.settings).await)
                }
                Err(e) => Err(e),
            }
        }
        "discord_rpc_push_state" => {
            #[derive(Deserialize)]
            struct A {
                args: crate::discord_rpc::PushStateArgs,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_string_err(crate::discord_rpc::discord_rpc_push_state(a.args).await),
                Err(e) => Err(e),
            }
        }
        "discord_rpc_set_sim_lost" => {
            #[derive(Deserialize)]
            struct A {
                lost: bool,
            }
            match parse_args::<A>(body) {
                Ok(a) => {
                    from_string_err(crate::discord_rpc::discord_rpc_set_sim_lost(a.lost).await)
                }
                Err(e) => Err(e),
            }
        }

        // ========================== HOPPIE ACARS (PDC/CPDLC) ==============
        // Missing here entirely until now — the whole feature (settings,
        // connect, and therefore the PDC/CPDLC tab, which stays hidden
        // while `enabled` reads as unset) was undriveable from a LAN
        // tablet: every command below fell through to `Dispatch::Unknown`
        // (404), and the frontend's own `.then(setSettings)` calls have no
        // `.catch()` (deliberately — see useCpdlcMessages.ts and friends,
        // which rely on the native/Tauri path never rejecting for a
        // command that exists), so the rejection was swallowed and the
        // settings section just sat on its heading forever.
        "hoppie_get_settings" => ok_json(crate::hoppie::hoppie_get_settings(app.clone())),
        "hoppie_set_settings" => {
            #[derive(Deserialize)]
            struct A {
                settings: crate::hoppie::HoppieSettings,
            }
            match parse_args::<A>(body) {
                Ok(a) => ok_json(crate::hoppie::hoppie_set_settings(app.clone(), a.settings)),
                Err(e) => Err(e),
            }
        }
        "hoppie_set_logon_code" => {
            #[derive(Deserialize)]
            struct A {
                code: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::hoppie::hoppie_set_logon_code(a.code)),
                Err(e) => Err(e),
            }
        }
        "hoppie_has_logon_code" => from_uierr(crate::hoppie::hoppie_has_logon_code()),
        "hoppie_clear_logon_code" => from_uierr(crate::hoppie::hoppie_clear_logon_code()),
        "hoppie_connect" => from_uierr(crate::hoppie::hoppie_connect(app.clone(), st!()).await),
        "hoppie_disconnect" => {
            from_uierr(crate::hoppie::hoppie_disconnect(app.clone(), st!()).await)
        }
        "hoppie_status" => from_uierr(crate::hoppie::hoppie_status(st!()).await),
        "hoppie_get_flight_context" => {
            ok_json(crate::hoppie::hoppie_get_flight_context(app.clone()))
        }
        "hoppie_ping_station" => {
            #[derive(Deserialize)]
            struct A {
                station: String,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::hoppie::hoppie_ping_station(st!(), a.station).await),
                Err(e) => Err(e),
            }
        }
        "hoppie_send_logon_request" => {
            #[derive(Deserialize)]
            struct A {
                #[serde(default)]
                station: Option<String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::hoppie::hoppie_send_logon_request(app.clone(), st!(), a.station).await,
                ),
                Err(e) => Err(e),
            }
        }
        "hoppie_send_logoff" => {
            from_uierr(crate::hoppie::hoppie_send_logoff(app.clone(), st!()).await)
        }
        "hoppie_send_telex" => {
            #[derive(Deserialize)]
            struct A {
                text: String,
                #[serde(default)]
                recipient: Option<String>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::hoppie::hoppie_send_telex(app.clone(), st!(), a.text, a.recipient).await,
                ),
                Err(e) => Err(e),
            }
        }
        "hoppie_send_free_text" => {
            #[derive(Deserialize)]
            struct A {
                text: String,
                #[serde(default)]
                mrn: Option<u32>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::hoppie::hoppie_send_free_text(app.clone(), st!(), a.text, a.mrn).await,
                ),
                Err(e) => Err(e),
            }
        }
        "hoppie_send_cpdlc_element" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                element_id: String,
                values: Vec<String>,
                #[serde(default)]
                mrn: Option<u32>,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::hoppie::hoppie_send_cpdlc_element(
                        app.clone(),
                        st!(),
                        a.element_id,
                        a.values,
                        a.mrn,
                    )
                    .await,
                ),
                Err(e) => Err(e),
            }
        }
        "hoppie_send_pdc_request" => {
            #[derive(Deserialize)]
            struct A {
                request: crate::hoppie::PdcRequestArgs,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::hoppie::hoppie_send_pdc_request(app.clone(), st!(), a.request).await,
                ),
                Err(e) => Err(e),
            }
        }
        "hoppie_get_thread" => from_uierr(crate::hoppie::hoppie_get_thread(st!()).await),
        "hoppie_list_elements" => ok_json(crate::hoppie::hoppie_list_elements()),

        // ============================ REMOTE SELF ========================
        // v1.5.6 (#lan-bruecke-1zu1, Thomas: "die LAN-Bruecke soll alles
        // 1 zu 1 machen"): auch die Server-Schalter selbst. Vorher gesperrt
        // mit der Begruendung "vom Tablet nicht sinnvoll steuerbar" — das
        // ist eine Bequemlichkeits-, keine Sicherheitsfrage: hinter die
        // Bruecke kommt ohnehin nur ein per PIN gepaartes Geraet, und der
        // Pilot sieht am Tablet exakt dieselben Schalter wie am PC.
        //
        // EHRLICH DAZU: `remote_server_stop` und `remote_server_set_port`
        // kappen die eigene Leitung — der Server, der die Antwort senden
        // soll, ist danach weg bzw. auf einem anderen Port. Das Tablet
        // sieht dann einen Verbindungsfehler statt einer Antwort; die
        // Aktion selbst ist trotzdem korrekt ausgefuehrt. Wer den Schalter
        // am Tablet umlegt, will genau das. Zum Zurueckholen fuehrt der
        // Weg dann ueber den PC — wie beim Ausschalten jeder Fernbedienung.
        "remote_server_start" => from_uierr(crate::remote::remote_server_start(app.clone()).await),
        "remote_server_stop" => {
            from_uierr(crate::remote::remote_server_stop(app.clone(), st!()).await)
        }
        "remote_server_status" => {
            from_uierr(crate::remote::remote_server_status(app.clone(), st!()).await)
        }
        "remote_server_set_port" => {
            #[derive(Deserialize)]
            struct A {
                port: u16,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(
                    crate::remote::remote_server_set_port(a.port, app.clone(), st!()).await,
                ),
                Err(e) => Err(e),
            }
        }
        "remote_server_revoke_pairing" => {
            from_uierr(crate::remote::remote_server_revoke_pairing(app.clone(), st!()).await)
        }
        // Panel-Server (In-Sim-HUD) — gleiche Begruendung: der Pilot darf
        // sein eigenes HUD auch vom Tablet aus schalten.
        "panel_server_get_enabled" => ok_json(crate::panel_server_get_enabled(app.clone())),
        "panel_server_set_enabled" => {
            #[derive(Deserialize)]
            struct A {
                enabled: bool,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::panel_server_set_enabled(app.clone(), a.enabled)),
                Err(e) => Err(e),
            }
        }
        _ => return Dispatch::Unknown,
    };

    Dispatch::Handled(result)
}

#[cfg(test)]
mod tests {
    //! Dispatch-shape tests. We cannot run a full command here (they need
    //! a live `AppState` + Tauri runtime), so these cover the two pure
    //! seams every arm shares: arg parsing and result normalization, plus
    //! the unknown-command path. A read (`metar_get`-style `{icao}`) and a
    //! control (`flight_start`-style `{bid_id, acknowledgeAircraftMismatch}`)
    //! arg struct are exercised to prove the named-arg contract + the
    //! camelCase rename survive.
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_read_command_args() {
        #[derive(Deserialize, PartialEq, Debug)]
        struct A {
            icao: String,
        }
        let body = json!({ "icao": "EDDF" });
        let a: A = parse_args(&body).unwrap();
        assert_eq!(
            a,
            A {
                icao: "EDDF".into()
            }
        );
    }

    #[test]
    fn parses_control_command_args_with_camelcase_rename() {
        // Mirrors the real `flight_start` arg struct: `rename_all =
        // "camelCase"`, NOT a one-off `rename`. Tauri v2 camelCases every
        // command arg key, so the front-end sends `{bidId,
        // acknowledgeAircraftMismatch}` — this MUST parse from camelCase.
        #[derive(Deserialize, PartialEq, Debug)]
        #[serde(rename_all = "camelCase")]
        struct A {
            bid_id: i64,
            #[serde(default)]
            acknowledge_aircraft_mismatch: Option<bool>,
        }
        // The Tauri front-end sends camelCase arg names for BOTH fields.
        let body = json!({ "bidId": 42, "acknowledgeAircraftMismatch": true });
        let a: A = parse_args(&body).unwrap();
        assert_eq!(
            a,
            A {
                bid_id: 42,
                acknowledge_aircraft_mismatch: Some(true)
            }
        );
        // Optional arg may be omitted.
        let body2 = json!({ "bidId": 7 });
        let a2: A = parse_args(&body2).unwrap();
        assert_eq!(a2.bid_id, 7);
        assert_eq!(a2.acknowledge_aircraft_mismatch, None);
    }

    #[test]
    fn flight_end_parses_camelcase_args() {
        // Mirrors the real `flight_end` arg struct. The divert banner sends
        // `{divertTo, divertReason}` (camelCase). Before the
        // `rename_all = "camelCase"` fix this silently dropped `divertTo`
        // and filed a NORMAL arrival instead of a divert.
        #[derive(Deserialize, PartialEq, Debug)]
        #[serde(rename_all = "camelCase")]
        struct A {
            #[serde(default)]
            divert_to: Option<String>,
            #[serde(default)]
            divert_reason: Option<String>,
            #[serde(default)]
            accident_decision: Option<String>,
        }
        let body = json!({ "divertTo": "EDDM", "divertReason": "weather" });
        let a: A = parse_args(&body).unwrap();
        assert_eq!(a.divert_to, Some("EDDM".to_string()));
        assert_eq!(a.divert_reason, Some("weather".to_string()));
        assert_eq!(a.accident_decision, None);
        // The accident-override path (ActiveFlightPanel) must likewise send
        // camelCase `accidentDecision`; snake_case `accident_decision` was
        // silently dropped, filing as an accident despite the pilot override.
        let override_body = json!({ "accidentDecision": "as_hard_landing" });
        let o: A = parse_args(&override_body).unwrap();
        assert_eq!(o.accident_decision, Some("as_hard_landing".to_string()));
        assert_eq!(o.divert_to, None);
        // A plain arrival (no divert args) still parses cleanly.
        let empty: A = parse_args(&json!({})).unwrap();
        assert_eq!(empty.divert_to, None);
    }

    #[test]
    fn bad_args_become_uierror_not_panic() {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct A {
            bid_id: i64,
        }
        // Wrong type for bid_id.
        let body = json!({ "bid_id": "not-a-number" });
        match parse_args::<A>(&body) {
            Ok(_) => panic!("expected a parse error for a non-numeric bid_id"),
            Err(err) => assert_eq!(err.code, "bad_request"),
        }
    }

    #[test]
    fn string_err_is_wrapped_in_uierror() {
        let r: Result<(), String> = Err("boom".into());
        let out = from_string_err(r).unwrap_err();
        assert_eq!(out.code, "command_error");
        assert_eq!(out.message, "boom");
    }

    #[test]
    fn unit_return_serializes_to_null() {
        let r: Result<(), UiError> = Ok(());
        assert_eq!(from_uierr(r).unwrap(), Value::Null);
    }

    /// Guards the module doc's own claim: every command in lib.rs's
    /// `generate_handler!` list has a dispatch arm here, except the
    /// documented deny-set. Written after finding the whole Hoppie
    /// ACARS / PDC-CPDLC feature (17 commands) had silently fallen
    /// through to `Dispatch::Unknown` since it was added — nobody
    /// noticed because the frontend awaited those calls without a
    /// `.catch()`, so a LAN session just sat on a loading state forever
    /// instead of erroring. A source-text check catches the NEXT
    /// command added to `generate_handler!` and forgotten here, without
    /// needing a live AppState to call `dispatch` for real.
    #[test]
    fn every_generated_command_is_either_bridged_or_explicitly_denied() {
        // Deliberately not bridged — see the module doc's "What is
        // covered vs excluded" section for why each of these is exempt.
        // v1.5.6 (#lan-bruecke-1zu1): LEER. Thomas' Vorgabe ist "die
        // LAN-Bruecke soll alles 1 zu 1 machen" — es gibt keinen Befehl
        // mehr, den die Bruecke absichtlich verschweigt. Die Liste bleibt
        // als Struktur stehen: wer je wieder einen Befehl bewusst
        // aussperrt, traegt ihn hier MIT Begruendung ein, statt ihn im
        // Dispatch stillschweigend fehlen zu lassen.
        const DENY_SET: &[&str] = &[
            // VATSIM-CDM (v1.6.0): oeffnet ein NATIVES Fenster auf dem
            // Sim-PC. Vom Tablet aus ergibt das keinen Sinn — das Fenster
            // ginge auf dem PC auf, nicht auf dem Tablet. Der Browser-Pfad
            // der Oberflaeche faellt deshalb selbst auf openExternal
            // zurueck (VatsimCdmView/CpdlcPanel pruefen isTauri).
            "vdgs_fenster_oeffnen",
            "vdgs_fenster_offen",
        ];

        // v1.5.6: LEER — die frueher hier geparkten Luecken (Taxi-Karte,
        // Route-Refresh, Landungs-Sicherung, Aircraft-Scan, X-Plane-Plugin,
        // Fehlerbericht-Einwilligung) sind alle gebrueckt. Bleibt leer:
        // dieser Test ist ab jetzt die harte Kante gegen die naechste
        // vergessene Bruecke.
        const NOT_YET_TRIAGED: &[&str] = &[];

        let lib_src = include_str!("../lib.rs");
        let start = lib_src
            .find("tauri::generate_handler![")
            .expect("generate_handler! list must exist in lib.rs")
            + "tauri::generate_handler![".len();
        let rest = &lib_src[start..];
        let end = rest
            .find("\n        ])")
            .expect("generate_handler! list must close with `])`");
        let list = &rest[..end];

        let bridge_src = include_str!("bridge.rs");

        let missing: Vec<&str> = list
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("//"))
            .map(|l| l.trim_end_matches(','))
            .map(|l| l.rsplit("::").next().unwrap_or(l))
            .filter(|name| !DENY_SET.contains(name))
            .filter(|name| !NOT_YET_TRIAGED.contains(name))
            .filter(|name| !bridge_src.contains(&format!("\"{name}\" =>")))
            .collect();

        assert!(
            missing.is_empty(),
            "these commands are registered in generate_handler! but have no dispatch arm \
             in remote/bridge.rs (silently 404s over the LAN bridge — add an arm or, if it \
             genuinely shouldn't be remote-driveable, add it to DENY_SET with a reason): {missing:?}"
        );
    }
}
