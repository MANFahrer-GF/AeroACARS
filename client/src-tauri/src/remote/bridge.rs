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
//! ## What is covered vs excluded
//!
//! Every command in the `generate_handler!` list (lib.rs) is dispatched
//! here EXCEPT a small deny-set:
//!
//! - `xplane_install_plugin` / `xplane_detect_install_path` — write to /
//!   probe the **sim PC's** local X-Plane install; a remote tablet has no
//!   business mutating the host filesystem, and the paths are the host's.
//! - `error_reporting_set_consent` — GDPR consent must be given on the
//!   machine that actually sends the telemetry (the desktop), not proxied.
//!
//! There are NO updater / process / relaunch / Window-taking commands in
//! the handler list, so the "exclude those" rule is satisfied by the list
//! simply not containing any.

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
        "logbook_stats" => from_uierr(crate::logbook_stats(st!()).await),
        "phpvms_get_bids" => from_uierr(crate::phpvms_get_bids(st!()).await),
        "news_fetch" => from_uierr(crate::news_fetch(st!()).await),
        "phpvms_refresh_profile" => from_uierr(crate::phpvms_refresh_profile(st!()).await),
        "flight_list_orphans" => from_uierr(crate::flight_list_orphans(st!()).await),
        "flight_discover_resumable" => {
            from_uierr(crate::flight_discover_resumable(app.clone(), st!()).await)
        }

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
                    crate::flight_start(app.clone(), st!(), a.bid_id, a.acknowledge_aircraft_mismatch)
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
                Ok(a) => {
                    from_uierr(crate::flight_start_manual(app.clone(), st!(), a.bid_id, a.plan).await)
                }
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
                    crate::flight_cancel_orphan(app.clone(), st!(), a.pirep_id, a.bid_id, a.flight_id)
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
                Ok(a) => from_uierr(crate::phpvms_login(app.clone(), st!(), a.url, a.api_key).await),
                Err(e) => Err(e),
            }
        }
        "phpvms_logout" => from_uierr(crate::phpvms_logout(app.clone(), st!()).await),
        // v0.16.2: the paired tablet inherits the sim-PC's logged-in session.
        // The frontend's startup login-check (App.tsx) calls this; bridging it
        // means the tablet skips the API-key login page (the backend already
        // holds the session). Returns Option<LoginResult> (profile, NOT the key).
        "phpvms_load_session" => {
            from_uierr(crate::phpvms_load_session(app.clone(), st!()).await)
        }

        // ========================== SETTINGS =============================
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
        "flight_logs_stats" => from_uierr(crate::flight_logs_stats(app.clone())),
        "flight_logs_delete_all" => from_uierr(crate::flight_logs_delete_all(app.clone())),
        "flight_logs_purge_older_than" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                older_than_days: u32,
            }
            match parse_args::<A>(body) {
                Ok(a) => from_uierr(crate::flight_logs_purge_older_than(app.clone(), a.older_than_days)),
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
                Ok(a) => from_string_err(crate::discord_rpc::discord_rpc_set_sim_lost(a.lost).await),
                Err(e) => Err(e),
            }
        }

        // ============================ REMOTE SELF ========================
        // The remote-server control commands themselves take a real
        // `tauri::State`; they manage the host's own server and are not
        // meaningfully driveable from the tablet that is being served, so
        // they are intentionally NOT bridged.
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
        assert_eq!(a, A { icao: "EDDF".into() });
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
}
