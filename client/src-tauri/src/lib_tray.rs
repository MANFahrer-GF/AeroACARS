/// What state the tray icon is currently expressing — drives the
/// icon color, tooltip text, and the "status" header item in the
/// right-click menu. Computed from the active flight's stats every
/// `TRAY_UPDATE_INTERVAL_SECS` so the pilot can glance at the
/// taskbar/menubar and immediately tell what AeroACARS is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayState {
    /// No active flight. Default app icon (template-friendly).
    Idle,
    /// Active flight, heartbeat fresh, no offline backlog.
    Active,
    /// Active flight but heartbeat is stale OR position queue is
    /// piling up. Visible warning so pilot can investigate.
    Warning,
    /// Critical: PIREP cancelled remotely (404 from heartbeat) or
    /// streamer hasn't posted in a long time. Plus a one-shot
    /// system notification when first transitioning into this state.
    Error,
}

const TRAY_UPDATE_INTERVAL_SECS: u64 = 5;
/// Heartbeat older than this → tray flips to Warning.
const TRAY_HEARTBEAT_STALE_SECS: i64 = 60;
/// Position queue with more than this many pending → Warning.
const TRAY_QUEUE_WARN_THRESHOLD: u32 = 5;

/// Snapshot of what the tray should currently show. Held by
/// `AppState` so the periodic updater can diff against the previous
/// state to (a) avoid pointless icon swaps and (b) detect
/// transitions that warrant a system notification.
#[derive(Debug, Clone)]
struct TrayDisplay {
    state: TrayState,
    tooltip: String,
    /// Human-readable header to show as the first (disabled) menu
    /// entry, or `None` to omit the header (idle case).
    menu_header: Option<String>,
}

fn compute_tray_display(app: &AppHandle) -> TrayDisplay {
    let state = app.state::<AppState>();
    let guard = state.active_flight.lock().expect("active_flight lock");
    let Some(flight) = guard.as_ref() else {
        return TrayDisplay {
            state: TrayState::Idle,
            tooltip: "AeroACARS".to_string(),
            menu_header: None,
        };
    };
    let stats = flight.stats.lock().expect("flight stats");

    // Detect critical conditions first.
    let cancelled = flight.cancelled_remotely.load(Ordering::Relaxed);
    let now = Utc::now();
    let heartbeat_age_s = stats
        .last_heartbeat_at
        .map(|t| (now - t).num_seconds())
        .unwrap_or(i64::MAX);
    let queue = stats.queued_position_count;
    // #phase-v2 Cutover: Tray-Tooltip zeigt die v2-Phase.
    let phase_label = phase_human_label(stats.phase);
    let callsign = format_callsign(&flight.airline_icao, &flight.flight_number);
    let route = format!("{} → {}", flight.dpt_airport, flight.arr_airport);

    let s = if cancelled {
        TrayState::Error
    } else if heartbeat_age_s > TRAY_HEARTBEAT_STALE_SECS || queue >= TRAY_QUEUE_WARN_THRESHOLD {
        TrayState::Warning
    } else {
        TrayState::Active
    };

    let tooltip = match s {
        TrayState::Error => format!(
            "AeroACARS — {} {}\nPIREP serverseitig gecancelt",
            callsign, route
        ),
        TrayState::Warning => {
            let mut parts = vec![format!("AeroACARS — {} {}", callsign, route)];
            if heartbeat_age_s > TRAY_HEARTBEAT_STALE_SECS && stats.last_heartbeat_at.is_some() {
                parts.push(format!("Heartbeat stale ({}s)", heartbeat_age_s));
            } else if stats.last_heartbeat_at.is_none() {
                parts.push("Warte auf ersten Heartbeat".to_string());
            }
            if queue >= TRAY_QUEUE_WARN_THRESHOLD {
                parts.push(format!("{} Positionen offline gestaut", queue));
            }
            parts.join("\n")
        }
        TrayState::Active => {
            let mut t = format!("AeroACARS — {} {} · {}", callsign, route, phase_label);
            if let Some(t0) = stats.last_heartbeat_at {
                let age = (now - t0).num_seconds();
                t.push_str(&format!("\nHeartbeat vor {}s", age));
            }
            t
        }
        TrayState::Idle => "AeroACARS".to_string(),
    };

    let menu_header = Some(format!("✈ {} · {}", callsign, phase_label));

    TrayDisplay {
        state: s,
        tooltip,
        menu_header,
    }
}

/// Build the system-tray icon with a Show / Quit context menu and
/// click-to-toggle behaviour. Called once during Tauri setup; the
/// resulting tray lives for the app lifetime — when the window is
/// hidden via the minimize-to-tray feature this icon is the only
/// way back to the app.
///
/// On Windows the icon shows in the system tray (bottom-right);
/// on macOS in the menubar (top-right) — same code, the OS routes
/// it to the platform-native location.
/// Build the tray context menu with the given status-header label.
/// Extracted so the periodic tray updater can rebuild and swap it
/// when the active flight's state changes (Tauri 2's TrayIcon has
/// `set_menu` but no menu getter, so we build fresh each time).
fn build_tray_menu(
    app: &AppHandle,
    status_label: &str,
) -> Result<tauri::menu::Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    let status_item = MenuItem::with_id(app, "status", status_label, false, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let show_item = MenuItem::with_id(app, "show", "Anzeigen / Show", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Beenden / Quit", true, None::<&str>)?;
    Ok(Menu::with_items(
        app,
        &[&status_item, &separator, &show_item, &quit_item],
    )?)
}

fn build_tray_icon(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::Manager;

    let menu = build_tray_menu(app, "AeroACARS — kein aktiver Flug")?;

    // Idle icon = the default app icon, untouched. The periodic
    // tray-updater swaps to a status-badged variant when there's an
    // active flight (see `make_status_icon`).
    let icon = app
        .default_window_icon()
        .ok_or("no default window icon")?
        .clone();

    TrayIconBuilder::with_id("aeroacars-tray")
        .tooltip("AeroACARS")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            "quit" => {
                // Bypass the CloseRequested → hide path by calling
                // app.exit() directly. Same effect as toggling
                // minimize-to-tray off and clicking X, but explicit.
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click on the tray icon toggles the main window's
            // visibility. Right-click opens the context menu (handled
            // by Tauri itself because show_menu_on_left_click=false).
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        let _ = w.show();
                        let _ = w.unminimize();
                        let _ = w.set_focus();
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}

/// Color used for the status badge in the bottom-right corner of the
/// tray icon. RGBA. Idle has no badge — falls through to default
/// app icon.
fn tray_state_color(state: TrayState) -> Option<[u8; 4]> {
    match state {
        TrayState::Idle => None,
        // Calm Apple-system green for "all good".
        TrayState::Active => Some([0x30, 0xD1, 0x58, 0xFF]),
        // Warm amber for "look at me".
        TrayState::Warning => Some([0xFF, 0x9F, 0x0A, 0xFF]),
        // Strong Apple-system red for "PIREP gone".
        TrayState::Error => Some([0xFF, 0x45, 0x3A, 0xFF]),
    }
}

/// Produce a tray-icon variant of `base` with a colored status circle
/// painted in the bottom-right corner. The badge is sized to the
/// lower-right ~35 % of the icon and has a 1-pixel transparent ring
/// around it so it visually pops off the underlying logo.
///
/// Returns `None` for `TrayState::Idle` — caller uses the unmodified
/// base icon in that case.
fn make_status_icon<'a>(
    base: &tauri::image::Image<'a>,
    state: TrayState,
) -> Option<tauri::image::Image<'static>> {
    let color = tray_state_color(state)?;
    let w = base.width();
    let h = base.height();
    let mut rgba: Vec<u8> = base.rgba().to_vec();

    // Badge geometry — bottom-right circle covering ~35 % of the icon.
    let badge_d = (w.min(h) as f32 * 0.55) as i32;
    let r = badge_d / 2;
    let cx = w as i32 - r - 1;
    let cy = h as i32 - r - 1;
    // 1-pixel transparent halo for visual separation.
    let r_outer = r + 1;

    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            let idx = ((y as u32 * w + x as u32) * 4) as usize;
            if d2 <= r * r {
                // Solid badge color.
                rgba[idx] = color[0];
                rgba[idx + 1] = color[1];
                rgba[idx + 2] = color[2];
                rgba[idx + 3] = color[3];
            } else if d2 <= r_outer * r_outer {
                // Halo — clear to transparent.
                rgba[idx + 3] = 0;
            }
        }
    }
    Some(tauri::image::Image::new_owned(rgba, w, h))
}

/// Periodic background task: every `TRAY_UPDATE_INTERVAL_SECS`,
/// recompute the desired tray display from the active flight's
/// stats and push it to the live `TrayIcon`. Diff against the
/// previous state to avoid pointless icon-repaints AND to detect
/// the transition into `Error` (which triggers a one-shot system
/// notification so a tray-only pilot can't miss it).
fn spawn_tray_updater(app: AppHandle) {
    use std::sync::Mutex;
    use tauri::tray::TrayIconId;
    let tray_id = TrayIconId::new("aeroacars-tray");
    let last_state: std::sync::Arc<Mutex<Option<TrayState>>> =
        std::sync::Arc::new(Mutex::new(None));
    tauri::async_runtime::spawn(async move {
        // Cache the colored icon variants once at startup so we
        // don't recompute the pixel buffer on every 5 s tick.
        let base_icon = match app.default_window_icon() {
            Some(i) => i.clone(),
            None => {
                tracing::warn!("no default window icon — tray-updater giving up");
                return;
            }
        };
        let active_icon = make_status_icon(&base_icon, TrayState::Active);
        let warning_icon = make_status_icon(&base_icon, TrayState::Warning);
        let error_icon = make_status_icon(&base_icon, TrayState::Error);

        loop {
            tokio::time::sleep(Duration::from_secs(TRAY_UPDATE_INTERVAL_SECS)).await;

            let display = compute_tray_display(&app);
            let prev = {
                let g = last_state.lock().unwrap();
                *g
            };
            let mut g = last_state.lock().unwrap();
            *g = Some(display.state);
            drop(g);

            // Push tooltip + (rebuilt) menu so the status header
            // reflects the current flight info. Tauri 2's TrayIcon
            // exposes set_menu() but not a getter, so we rebuild the
            // menu structure each tick — cheap (3 native menu items)
            // and avoids storing MenuItem<Wry> lifetimes in AppState.
            if let Some(tray) = app.tray_by_id(&tray_id) {
                let _ = tray.set_tooltip(Some(&display.tooltip));

                let header_label = display
                    .menu_header
                    .clone()
                    .unwrap_or_else(|| "AeroACARS — kein aktiver Flug".to_string());
                if let Ok(new_menu) = build_tray_menu(&app, &header_label) {
                    let _ = tray.set_menu(Some(new_menu));
                }

                // Icon swap only on state change (it's a relatively
                // expensive call on Win, and Mac sometimes flickers).
                if prev != Some(display.state) {
                    let new_icon = match display.state {
                        TrayState::Idle => Some(base_icon.clone()),
                        TrayState::Active => active_icon.clone(),
                        TrayState::Warning => warning_icon.clone(),
                        TrayState::Error => error_icon.clone(),
                    };
                    if let Some(icon) = new_icon {
                        let _ = tray.set_icon(Some(icon));
                    }
                }
            }

            // Fire a one-shot system notification when we just
            // entered Error. Pilots running in tray-only mode would
            // otherwise miss the "PIREP cancelled" event entirely
            // (the activity-log entry is invisible until they
            // re-open the window).
            if prev != Some(TrayState::Error) && display.state == TrayState::Error {
                use tauri_plugin_notification::NotificationExt;
                let _ = app
                    .notification()
                    .builder()
                    .title("AeroACARS")
                    .body(&display.tooltip)
                    .show();
            }
        }
    });
}
