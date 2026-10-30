// ---- Simulator selection + status ----

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
struct SimConfig {
    /// Der Simulator, auf den der Adapter gerade eingestellt ist. Im
    /// Automatik-Modus stellt ihn der Waechter um (`sim_automatik_starten`).
    #[serde(default)]
    kind: SimKind,
    /// „Automatisch“ gewaehlt (v1.9.14). Alte Dateien haben das Feld nicht
    /// und bleiben damit bei der Handwahl — Entscheidung Thomas 30.09.2026:
    /// Bestandspiloten nichts umstellen, nur Neuinstallationen bekommen die
    /// Automatik (siehe `sim_config_bei_neuinstallation`).
    #[serde(default)]
    automatisch: bool,
}

/// Haelt Handwahl, Waechter und Anwenden beim Start auseinander: Lesen,
/// Schreiben und `apply_sim_kind` laufen darunter als EIN Schritt. Sonst
/// koennte der Waechter zwischen „Pilot waehlt X-Plane von Hand“ und dem
/// Anwenden seine eigene Wahl dazwischenschieben — Datei und laufender
/// Adapter zeigten dann auf verschiedene Simulatoren.
static SIM_WAHL_SPERRE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Neuinstallation → „Automatisch“ als Voreinstellung schreiben.
///
/// Neu ist, wer NICHTS von einem frueheren Lauf hat: keine Simulator-
/// Auswahl, keine Seite, keinen Geheimnisspeicher und kein Aktivitaetslog.
///
/// ⚠ `site.json` allein reicht nicht (QS-Befund 1, 30.09.2026): Sie wird
/// bei jedem Abmelden geloescht, und `sim.json` gibt es nur, wenn der Pilot
/// den Simulator je von Hand gewaehlt hat — ein abgemeldeter Bestandspilot
/// mit Voreinstellung sah damit aus wie eine Neuinstallation.
/// `secrets.json` und `activity_log.json` ueberstehen das Abmelden.
///
/// Laeuft als ERSTES im Setup, bevor irgendetwas davon geschrieben wird.
fn sim_config_bei_neuinstallation(app: &AppHandle) {
    let (Ok(sim), Ok(site)) = (sim_config_path(app), site_config_path(app)) else {
        return;
    };
    let gibt_es = |p: Option<PathBuf>| p.map(|p| p.exists());
    let spuren = [
        Some(sim.exists()),
        Some(site.exists()),
        gibt_es(
            app.path()
                .app_data_dir()
                .ok()
                .map(|d| d.join("secrets.json")),
        ),
        gibt_es(
            app.path()
                .app_config_dir()
                .ok()
                .map(|d| d.join(ACTIVITY_LOG_FILE)),
        ),
    ];
    if !sim_auto::ist_neuinstallation(&spuren) {
        return;
    }
    let _sperre = SIM_WAHL_SPERRE.lock().unwrap_or_else(|e| e.into_inner());
    // Auf dem Mac gibt es kein MSFS — dort mit X-Plane 12 beginnen, sonst
    // stuende bis zum ersten X-Plane-Start „MSFS 2024“ da (QS 30.09.2026).
    let kind = if cfg!(target_os = "macos") {
        SimKind::XPlane12
    } else {
        SimKind::default()
    };
    let cfg = SimConfig {
        kind,
        automatisch: true,
    };
    if let Err(e) = write_sim_config(app, &cfg) {
        tracing::warn!(error = %e.message, "Neuinstallation: Simulator-Automatik nicht gespeichert");
    }
}

/// Die gespeicherte Auswahl anwenden (Login, Sitzung wiederhergestellt).
///
/// Fehlt `sim.json` noch (Bestandspilot, der nie von Hand gewaehlt hat),
/// wird die bisherige Voreinstellung jetzt als HANDWAHL festgeschrieben —
/// damit gilt er auch nach einem spaeteren Abmelden nie als Neuinstallation.
fn gespeicherte_sim_wahl_anwenden(app: &AppHandle, state: &tauri::State<'_, AppState>) -> SimKind {
    let _sperre = SIM_WAHL_SPERRE.lock().unwrap_or_else(|e| e.into_inner());
    let cfg = read_sim_config(app);
    if sim_config_path(app).is_ok_and(|p| !p.exists()) {
        if let Err(e) = write_sim_config(app, &cfg) {
            tracing::warn!(error = %e.message, "Simulator-Auswahl nicht festgeschrieben");
        }
    }
    let kind = cfg.kind;
    apply_sim_kind(state, kind);
    kind
}

/// Zwischenspeicher der Simulator-Auswahl — im RAM, nicht auf der Platte.
///
/// # Warum es ihn gibt (Befund 12.09.2026, CFG 2090)
///
/// `read_sim_config` las bei JEDEM Aufruf die Konfigurationsdatei: Existenz
/// prüfen, lesen, JSON parsen. Aufgerufen wird sie unter anderem von
/// `current_snapshot` und `current_premium_touchdown` — und die stehen beide
/// im 20-ms-Takt des Aufsetz-Samplers, der zweite sogar unter gehaltenem
/// `FlightStats`-Lock. Das sind bis zu hundert Dateizugriffe je Sekunde,
/// genau während der Simulator die Maschine auslastet.
///
/// Gemessen an der Landung, die diese Untersuchung ausgelöst hat: Der
/// Sampler schaffte statt fünfzig nur noch etwa zwei erfolgreiche
/// Erfassungen je Sekunde; im Aufsetzfenster blieben zehn Proben statt
/// rund zweihundertfünfzig übrig, und die Bewertung erhielt eine
/// Sinkrate aus einem Frame, der eine ganze Sekunde nach dem Aufsetzen lag.
///
/// Die Auswahl ändert sich nur, wenn der Pilot sie in den Einstellungen
/// umstellt — dort wird der Zwischenspeicher mitgeschrieben
/// (`write_sim_config`). Eine Änderung der Datei von aussen, am laufenden
/// Programm vorbei, wirkt erst beim nächsten Start; das ist der Preis und
/// er ist vertretbar.
static SIM_CONFIG_CACHE: std::sync::RwLock<Option<SimConfig>> = std::sync::RwLock::new(None);

fn sim_config_path(app: &AppHandle) -> Result<PathBuf, UiError> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join(SIM_CONFIG_FILE))
        .map_err(|e| UiError::new("config_path", e.to_string()))
}

fn read_sim_config(app: &AppHandle) -> SimConfig {
    if let Some(cfg) = SIM_CONFIG_CACHE.read().ok().and_then(|g| *g) {
        return cfg;
    }
    // Beim Füllen die Schreibsperre halten und NOCHMAL prüfen: Zwischen der
    // Lese- und der Schreibsperre kann ein `write_sim_config` die Auswahl
    // bereits gesetzt haben (Codex-Befund 12.09.2026). Ohne die zweite
    // Prüfung überschriebe der hier gelesene alte Dateiinhalt sie wieder.
    let mut g = SIM_CONFIG_CACHE.write().unwrap_or_else(|e| e.into_inner());
    if let Some(cfg) = *g {
        return cfg;
    }
    let cfg = read_sim_config_von_platte(app);
    *g = Some(cfg);
    cfg
}

/// Die Auswahl wirklich von der Platte lesen — nur beim ersten Mal und nach
/// einer Änderung. Siehe `SIM_CONFIG_CACHE`.
fn read_sim_config_von_platte(app: &AppHandle) -> SimConfig {
    let Ok(path) = sim_config_path(app) else {
        return SimConfig::default();
    };
    if !path.exists() {
        return SimConfig::default();
    }
    match std::fs::read(&path).map(|b| serde_json::from_slice::<SimConfig>(&b)) {
        Ok(Ok(cfg)) => cfg,
        _ => SimConfig::default(),
    }
}

fn write_sim_config(app: &AppHandle, cfg: &SimConfig) -> Result<(), UiError> {
    // Der Zwischenspeicher wird ERST NACH erfolgreichem Schreiben gesetzt,
    // und die Sperre wird über den ganzen Vorgang gehalten.
    //
    // Codex-Befund 12.09.2026 zum ersten Entwurf: Dort stand er davor, mit
    // der Begründung "der Pilot hat die Auswahl ja getroffen". Der Aufrufer
    // bricht bei einem Schreibfehler aber ab (`write_sim_config(...)?`), und
    // `apply_sim_kind` läuft dann gar nicht — der Zwischenspeicher zeigte auf
    // den neuen Simulator, während der alte Adapter weiterlief. Ab da fragte
    // `current_snapshot` den falschen Adapter.
    //
    // Die gehaltene Schreibsperre schliesst zugleich den Wettlauf beim ersten
    // Lesen: Ohne sie konnte ein gleichzeitig laufendes `read_sim_config`
    // seinen alten Dateiinhalt über die gerade geschriebene Auswahl legen.
    let mut cache = SIM_CONFIG_CACHE.write().unwrap_or_else(|e| e.into_inner());
    let path = sim_config_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| UiError::new("config_write", e.to_string()))?;
    }
    let json = serde_json::to_vec_pretty(cfg)
        .map_err(|e| UiError::new("config_serialize", e.to_string()))?;
    std::fs::write(&path, json).map_err(|e| UiError::new("config_write", e.to_string()))?;
    *cache = Some(*cfg);
    Ok(())
}

/// Verbunden? Und welche Fassung meldet der Simulator selbst?
fn sim_verbindung_und_fassung(
    state: &tauri::State<'_, AppState>,
    kind: SimKind,
) -> (bool, Option<SimKind>) {
    if kind.is_xplane() {
        let xp = state.xplane.lock().expect("xplane lock");
        let verbunden = matches!(xp.state(), sim_xplane::ConnectionState::Connected);
        let p = xp.premium_status();
        // Nur eine gerade aktive Plugin-Verbindung zaehlt; ein alter Stand
        // stammt womoeglich vom vorigen Simulator.
        let gemeldet = p
            .active
            .then_some(p.xplane_version)
            .flatten()
            .and_then(sim_auto::fassung_aus_xplane_version);
        return (verbunden, gemeldet);
    }
    #[cfg(target_os = "windows")]
    if kind.is_msfs() {
        let msfs = state.msfs.lock().expect("msfs lock");
        let verbunden = matches!(msfs.state(), sim_msfs::ConnectionState::Connected);
        let gemeldet = msfs
            .szenerie_schnappschuss("")
            .kennung
            .as_deref()
            .and_then(sim_auto::fassung_aus_msfs_kennung);
        return (verbunden, gemeldet);
    }
    (false, None)
}

/// Waechter fuer „Automatisch“ (v1.9.14). Schaut alle 5 s nach und stellt
/// um — NIE waehrend eines Flugs. Die Entscheidung steht in `sim_auto`.
fn sim_automatik_starten(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut takt = tokio::time::interval(std::time::Duration::from_secs(5));
        takt.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            takt.tick().await;
            let cfg = read_sim_config(&app);
            if !cfg.automatisch {
                continue;
            }
            // Die Prozessliste nur lesen, wenn der eingestellte Simulator
            // nicht verbunden ist — und ausserhalb der Sperre (sie kostet
            // einige Millisekunden).
            let verbunden = {
                let state = app.state::<AppState>();
                sim_verbindung_und_fassung(&state, cfg.kind).0
            };
            let laufend = if verbunden {
                None
            } else {
                tokio::task::spawn_blocking(sim_core::process_probe::laufende_simulatoren)
                    .await
                    .ok()
                    .flatten()
            };
            let state = app.state::<AppState>();
            sim_automatik_schritt(&app, &state, laufend);
        }
    });
}

/// Ein Schritt des Waechters, unter der Sperre: erneut lesen, pruefen,
/// umstellen.
fn sim_automatik_schritt(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    laufend: Option<sim_core::process_probe::LaufendeSimulatoren>,
) {
    let _sperre = SIM_WAHL_SPERRE.lock().unwrap_or_else(|e| e.into_inner());
    let cfg = read_sim_config(app);
    if !cfg.automatisch {
        return;
    }
    // Waehrend eines Flugs nie umstellen — auch nicht, solange einer gerade
    // entsteht (Flugstart, Uebernahme, Wiederaufnahme, Auto-Start; dann ist
    // `active_flight` noch leer, QS-Befund 2, 30.09.2026).
    if state
        .flight_setup_in_progress
        .load(std::sync::atomic::Ordering::SeqCst)
        || state
            .active_flight
            .lock()
            .expect("active_flight lock")
            .is_some()
    {
        return;
    }
    let (verbunden, gemeldet) = sim_verbindung_und_fassung(state, cfg.kind);
    let Some(neu) = sim_auto::naechste_wahl(cfg.kind, verbunden, laufend, gemeldet) else {
        return;
    };
    let neu_cfg = SimConfig {
        kind: neu,
        automatisch: true,
    };
    if let Err(e) = write_sim_config(app, &neu_cfg) {
        tracing::warn!(error = %e.message, "Simulator-Automatik: Auswahl nicht gespeichert");
        return;
    }
    apply_sim_kind(state, neu);
    tracing::info!(von = ?cfg.kind, nach = ?neu, "Simulator automatisch umgestellt");
    log_activity(
        state,
        ActivityLevel::Info,
        format!("Simulator automatisch erkannt: {}", sim_anzeigename(neu)),
        None,
    );
    let _ = tauri::Emitter::emit(app, "sim-auswahl-geaendert", kind_str(neu));
}

fn sim_anzeigename(kind: SimKind) -> &'static str {
    match kind {
        SimKind::Off => "aus",
        SimKind::Msfs2020 => "MSFS 2020",
        SimKind::Msfs2024 => "MSFS 2024",
        SimKind::XPlane11 => "X-Plane 11",
        SimKind::XPlane12 => "X-Plane 12",
    }
}

/// Apply the selected kind to whichever adapter handles it. Always
/// stops the inactive adapter so we never have both listening
/// simultaneously (= duplicate snapshots in `current_snapshot`).
fn apply_sim_kind(state: &tauri::State<'_, AppState>, kind: SimKind) {
    // Stop both adapters first; we'll start exactly one (or none) below.
    // v1.9.24: MSFS nur unter der Sperre ANSTOSSEN, das (begrenzte) Warten
    // auf den Worker läuft ohne Sperre — sonst hängen sim_status & Co. bis
    // zu 3 s mit. Sicher, weil alle Aufrufer SIM_WAHL_SPERRE halten: bis
    // nach_abschied startet niemand einen neuen Worker.
    #[cfg(target_os = "windows")]
    let msfs_abschied = state.msfs.lock().expect("msfs lock").stopp_anstossen();
    {
        let mut xp = state.xplane.lock().expect("xplane lock");
        xp.stop();
    }
    #[cfg(target_os = "windows")]
    {
        let sauber = msfs_abschied
            .map(sim_msfs::WorkerAbschied::abwarten)
            .unwrap_or(true);
        state.msfs.lock().expect("msfs lock").nach_abschied(sauber);
    }

    if kind.is_msfs() {
        #[cfg(target_os = "windows")]
        {
            let mut msfs = state.msfs.lock().expect("msfs lock");
            msfs.start(kind);
        }
    } else if kind.is_xplane() {
        let mut xp = state.xplane.lock().expect("xplane lock");
        xp.start(kind);
    }
    // SimKind::Off → both stay stopped.
}

#[derive(Serialize, Default)]
pub struct SimStatus {
    /// "disconnected" | "connecting" | "connected"
    state: String,
    /// User-selected sim ("off" | "msfs2020" | "msfs2024" | "xplane11" | "xplane12").
    kind: String,
    /// ⚠ Ohne Diagnosefelder — siehe `snapshot_ohne_diagnose`.
    #[serde(serialize_with = "snapshot_ohne_diagnose")]
    snapshot: Option<SimSnapshot>,
    last_error: Option<String>,
    /// Whether the selected kind is actually implemented in this build.
    available: bool,
}

/// Der Sim-Status ohne die reinen Diagnosewerte.
///
/// Die Bildrate (2.0.14) gehoert ins Flugprotokoll und ins Diagnoselog,
/// nicht in die Oberflaeche und nicht ueber die LAN-Bruecke an die
/// Fernbedienung (Thomas, 13.10.2026; Codex-QS P2). Hier, an der
/// Serialisierung, gilt das fuer jeden, der einen `SimStatus` baut.
fn snapshot_ohne_diagnose<S: serde::Serializer>(
    snapshot: &Option<SimSnapshot>,
    ser: S,
) -> Result<S::Ok, S::Error> {
    let mut ohne = snapshot.clone();
    if let Some(s) = ohne.as_mut() {
        s.sim_bildrate_hz = None;
    }
    serde::Serialize::serialize(&ohne, ser)
}

fn kind_str(kind: SimKind) -> &'static str {
    match kind {
        SimKind::Off => "off",
        SimKind::Msfs2020 => "msfs2020",
        SimKind::Msfs2024 => "msfs2024",
        SimKind::XPlane11 => "xplane11",
        SimKind::XPlane12 => "xplane12",
    }
}

/// `GET` the persisted sim selection.
#[tauri::command(async)]
fn sim_get_kind(app: AppHandle) -> String {
    kind_str(read_sim_config(&app).kind).to_string()
}

#[derive(Serialize)]
pub struct SimAuswahl {
    automatisch: bool,
    /// Der Simulator, auf den gerade eingestellt ist (bei „Automatisch“ der
    /// erkannte).
    kind: &'static str,
}

/// Auswahl fuer die Einstellungen: „Automatisch“ oder Handwahl, dazu der
/// aktuell eingestellte Simulator.
#[tauri::command(async)]
fn sim_get_auswahl(app: AppHandle) -> SimAuswahl {
    let cfg = read_sim_config(&app);
    SimAuswahl {
        automatisch: cfg.automatisch,
        kind: kind_str(cfg.kind),
    }
}

/// Persist a new sim selection AND apply it to the running adapter.
/// Accepts: "auto" | "off" | "msfs2020" | "msfs2024" | "xplane11" | "xplane12".
/// "auto" (v1.9.14) laesst den Adapter stehen; der Waechter stellt um.
#[tauri::command(async)]
fn sim_set_kind(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    kind: String,
) -> Result<(), UiError> {
    if kind == "auto" {
        // Den Adapter so lassen, wie er ist — der Waechter stellt innerhalb
        // weniger Sekunden um, wenn ein anderer Simulator laeuft.
        let _sperre = SIM_WAHL_SPERRE.lock().unwrap_or_else(|e| e.into_inner());
        let mut cfg = read_sim_config(&app);
        cfg.automatisch = true;
        write_sim_config(&app, &cfg)?;
        tracing::info!(kind = ?cfg.kind, "sim selection: automatic");
        return Ok(());
    }
    let parsed = match kind.as_str() {
        "off" => SimKind::Off,
        "msfs2020" => SimKind::Msfs2020,
        "msfs2024" => SimKind::Msfs2024,
        "xplane11" => SimKind::XPlane11,
        "xplane12" => SimKind::XPlane12,
        _ => {
            return Err(UiError::new(
                "invalid_sim_kind",
                format!("unknown kind: {kind}"),
            ))
        }
    };
    let _sperre = SIM_WAHL_SPERRE.lock().unwrap_or_else(|e| e.into_inner());
    write_sim_config(
        &app,
        &SimConfig {
            kind: parsed,
            automatisch: false,
        },
    )?;
    apply_sim_kind(&state, parsed);
    tracing::info!(?parsed, "sim kind selected");
    Ok(())
}

#[tauri::command(async)]
fn sim_status(app: AppHandle, state: tauri::State<'_, AppState>) -> SimStatus {
    let kind = read_sim_config(&app).kind;

    // X-Plane path is available on every platform (UDP, no platform-
    // specific deps).
    if kind.is_xplane() {
        let adapter = state.xplane.lock().expect("xplane lock");
        let s = match adapter.state() {
            sim_xplane::ConnectionState::Disconnected => "disconnected",
            sim_xplane::ConnectionState::Connecting => "connecting",
            sim_xplane::ConnectionState::Connected => "connected",
        };
        return SimStatus {
            state: s.into(),
            kind: kind_str(kind).into(),
            snapshot: adapter.snapshot(),
            last_error: adapter.last_error(),
            available: true,
        };
    }

    // MSFS path is Windows-only.
    #[cfg(target_os = "windows")]
    {
        let adapter = state.msfs.lock().expect("msfs lock");
        if kind.is_msfs() {
            let s = match adapter.state() {
                sim_msfs::ConnectionState::Disconnected => "disconnected",
                sim_msfs::ConnectionState::Connecting => "connecting",
                sim_msfs::ConnectionState::Connected => "connected",
            };
            return SimStatus {
                state: s.into(),
                kind: kind_str(kind).into(),
                snapshot: adapter.snapshot(),
                last_error: adapter.last_error(),
                available: true,
            };
        }
        // SimKind::Off
        return SimStatus {
            state: "disconnected".into(),
            kind: kind_str(kind).into(),
            snapshot: None,
            last_error: None,
            available: kind == SimKind::Off,
        };
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = state;
        let last_error = if kind.is_msfs() {
            Some("MSFS adapter is Windows-only".into())
        } else {
            None
        };
        SimStatus {
            state: "disconnected".into(),
            kind: kind_str(kind).into(),
            snapshot: None,
            last_error,
            available: kind == SimKind::Off,
        }
    }
}

/// Pilot-initiated re-sync of the active sim adapter's cached
/// snapshot. Clears the lat/lon/etc. cache so `sim_status` returns
/// `state: "connecting"` + `snapshot: None` until the next genuine
/// frame arrives. Lets the briefing tab recover from edge cases the
/// automatic 5 s stale-timeout doesn't cover (sim paused while
/// changing flight, SimConnect trickles stale data, etc.) without
/// any "skip the gate" override semantics.
#[tauri::command(async)]
fn sim_force_resync(app: AppHandle, state: tauri::State<'_, AppState>) {
    let kind = read_sim_config(&app).kind;
    if kind.is_xplane() {
        state.xplane.lock().expect("xplane lock").clear_snapshot();
        return;
    }
    #[cfg(target_os = "windows")]
    {
        if kind.is_msfs() {
            state.msfs.lock().expect("msfs lock").clear_snapshot();
        }
    }
}

// ---- PMDG SDK status (Phase H.4 / v0.2.0) ----
//
// The Settings tab needs to know:
//   * Is a PMDG aircraft loaded? (variant detected)
//   * Are we subscribed?
//   * Are we actually receiving data? (= SDK enabled in pilot's
//     options ini)
// to show the "SDK enabled?" hint when needed and to display the
// premium-telemetry status badge in the cockpit tab.

/// Minimal serializable PMDG status for the UI. Mirrors the
/// adapter's `PmdgStatus` but with the variant flattened to a
/// string so JSON parsing is trivial on the frontend side.
#[derive(serde::Serialize)]
struct PmdgStatusDto {
    /// `"ng3"`, `"x777"`, or `null` when no PMDG aircraft is loaded.
    variant: Option<&'static str>,
    /// True once SimConnect ClientData subscription has been
    /// successfully registered.
    subscribed: bool,
    /// True once at least one ClientData packet has actually arrived.
    /// `subscribed && !ever_received` (after a few seconds) is the
    /// signal that SDK isn't enabled in the pilot's options ini.
    ever_received: bool,
    /// Seconds since the last ClientData packet. `null` when never.
    stale_secs: Option<u64>,
    /// True when (variant detected, subscribed, but no data flowing
    /// for >5 s) — the heuristic for "SDK probably not enabled".
    /// UI shows the enable-hint modal when this is true.
    looks_like_sdk_disabled: bool,
}

#[tauri::command(async)]
fn pmdg_status(state: tauri::State<'_, AppState>) -> PmdgStatusDto {
    #[cfg(target_os = "windows")]
    {
        let adapter = state.msfs.lock().expect("msfs lock");
        let s = adapter.pmdg_status();
        return PmdgStatusDto {
            variant: s.variant.map(|v| match v {
                sim_msfs::pmdg::PmdgVariant::Ng3 => "ng3",
                sim_msfs::pmdg::PmdgVariant::X777 => "x777",
            }),
            subscribed: s.subscribed,
            ever_received: s.ever_received,
            stale_secs: if s.stale_secs == u64::MAX {
                None
            } else {
                Some(s.stale_secs)
            },
            looks_like_sdk_disabled: s.looks_like_sdk_disabled(),
        };
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = state;
        PmdgStatusDto {
            variant: None,
            subscribed: false,
            ever_received: false,
            stale_secs: None,
            looks_like_sdk_disabled: false,
        }
    }
}

// ---- Live SimVar / LVar inspector (Settings → Debug) ----
//
// These commands forward to the MsfsAdapter's inspector watchlist
// (Phase B). Non-Windows targets just return errors / empty lists
// since the adapter is Windows-only anyway.

// Fields are only read inside `inspector_add`'s `#[cfg(target_os = "windows")]`
// body — on non-Windows targets that body is compiled out, so a Linux/macOS
// `cargo check` sees them as "never read". Deserialize still needs them on
// every platform (Tauri parses the command's JSON args before the cfg-gate
// runs), so they can't be removed — just silence the false positive.
#[derive(serde::Deserialize)]
#[allow(dead_code)]
pub(crate) struct InspectorAddArgs {
    name: String,
    unit: String,
    /// "number" | "bool" | "string" — matches sim_msfs::WatchKind.
    kind: String,
}

#[tauri::command(async)]
fn inspector_add(
    _state: tauri::State<'_, AppState>,
    args: InspectorAddArgs,
) -> Result<u32, UiError> {
    #[cfg(target_os = "windows")]
    {
        let kind = match args.kind.as_str() {
            "number" => sim_msfs::WatchKind::Number,
            "bool" => sim_msfs::WatchKind::Bool,
            "string" => sim_msfs::WatchKind::String,
            _ => {
                return Err(UiError::new(
                    "invalid_watch_kind",
                    format!("unknown kind: {}", args.kind),
                ))
            }
        };
        let trimmed_name = args.name.trim().to_string();
        let trimmed_unit = args.unit.trim().to_string();
        if trimmed_name.is_empty() {
            return Err(UiError::new(
                "empty_name",
                "SimVar / LVar name cannot be empty",
            ));
        }
        let adapter = _state.msfs.lock().expect("msfs lock");
        let id = adapter.add_watch(trimmed_name, trimmed_unit, kind);
        Ok(id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = args;
        Err(UiError::new("unsupported", "inspector is Windows-only"))
    }
}

#[tauri::command(async)]
fn inspector_remove(_state: tauri::State<'_, AppState>, id: u32) -> Result<(), UiError> {
    #[cfg(target_os = "windows")]
    {
        let adapter = _state.msfs.lock().expect("msfs lock");
        adapter.remove_watch(id);
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = id;
        Err(UiError::new("unsupported", "inspector is Windows-only"))
    }
}

#[tauri::command(async)]
fn inspector_list(_state: tauri::State<'_, AppState>) -> Vec<serde_json::Value> {
    #[cfg(target_os = "windows")]
    {
        let adapter = _state.msfs.lock().expect("msfs lock");
        adapter
            .watches()
            .into_iter()
            .map(|w| serde_json::to_value(w).unwrap_or(serde_json::Value::Null))
            .collect()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

// ---- Telemetrie-Monitor (v1.8) ----
//
// Der Tab „Telemetrie" (und sein eigenes Fenster) bekommt den Kanalkatalog
// beim Oeffnen und danach 20× je Sekunde einen Frame als Ereignis
// `telemetrie-frame`. Der Strom laeuft nur, solange die Oberflaeche sich
// alle paar Sekunden meldet (`telemetrie_halten`); der Verlauf der letzten
// fuenf Minuten wird dagegen immer mitgeschrieben, damit ein mitten im Flug
// geoeffneter Monitor sofort gefuellte Kurven zeigt. Siehe `telemetrie.rs`.

#[derive(Serialize)]
pub(crate) struct TelemetrieStartDto {
    katalog: telemetrie::KatalogDto,
    verlauf: Vec<telemetrie::Frame>,
}

#[tauri::command]
fn telemetrie_start(app: AppHandle, window: tauri::Window) -> TelemetrieStartDto {
    telemetrie_start_fuer(&app, window.label())
}

/// Gemeinsamer Kern fuer Fenster und LAN-Bruecke (`wer` = Fensterkennung
/// oder `telemetrie::LAN_ZUSCHAUER`).
pub(crate) fn telemetrie_start_fuer(app: &AppHandle, wer: &str) -> TelemetrieStartDto {
    let state = app.state::<AppState>();
    state.telemetrie.halten(wer, std::time::Instant::now());
    TelemetrieStartDto {
        katalog: telemetrie::katalog(),
        verlauf: state.telemetrie.verlauf(),
    }
}

#[tauri::command]
fn telemetrie_halten(app: AppHandle, window: tauri::Window) {
    telemetrie_halten_fuer(&app, window.label());
}

pub(crate) fn telemetrie_halten_fuer(app: &AppHandle, wer: &str) {
    app.state::<AppState>()
        .telemetrie
        .halten(wer, std::time::Instant::now());
}

/// Dieses Fenster schaut nicht mehr zu. Ein anderes (Tab oder eigenes
/// Fenster) haelt den Strom mit seinem eigenen Eintrag weiter offen.
#[tauri::command]
fn telemetrie_stop(app: AppHandle, window: tauri::Window) {
    telemetrie_stop_fuer(&app, window.label());
}

pub(crate) fn telemetrie_stop_fuer(app: &AppHandle, wer: &str) {
    app.state::<AppState>().telemetrie.beenden(wer);
}

/// Fragt ueber den Speichern-Dialog nach dem Ziel und schreibt die CSV.
///
/// Der Pfad kommt bewusst NICHT aus der Oberflaeche: ein Befehl, der jeden
/// uebergebenen `.csv`-Pfad beschreibt, liesse eingeschleusten Code beliebige
/// Dateien ueberschreiben (Codex-Befund 3, 25.09.2026). `Ok(false)` =
/// Dialog abgebrochen.
#[tauri::command]
async fn telemetrie_csv_speichern(
    app: AppHandle,
    inhalt: String,
    dateiname: String,
) -> Result<bool, UiError> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("CSV", &["csv"])
        .set_file_name(dateiname)
        .save_file(move |p| {
            let _ = tx.send(p);
        });
    let Some(ziel) = rx.await.ok().flatten() else {
        return Ok(false);
    };
    let mut pfad = ziel
        .into_path()
        .map_err(|e| UiError::new("bad_path", format!("Speicherort ungültig: {e}")))?;
    if pfad
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        != Some("csv".into())
    {
        pfad.set_extension("csv");
    }
    tokio::fs::write(&pfad, inhalt).await.map_err(|e| {
        UiError::new(
            "write_failed",
            format!("CSV ließ sich nicht speichern: {e}"),
        )
    })?;
    Ok(true)
}

/// Oeffnet den Monitor in einem eigenen Fenster (zweiter Bildschirm).
/// Zweimal aufgerufen wird das vorhandene nach vorn geholt.
#[tauri::command]
async fn telemetrie_fenster_oeffnen(app: AppHandle) -> Result<(), UiError> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    if let Some(vorhanden) = app.get_webview_window("telemetrie") {
        let _ = vorhanden.unminimize();
        let _ = vorhanden.show();
        let _ = vorhanden.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(
        &app,
        "telemetrie",
        // v3 (Etappe 5, P-8.20): aus derselben Generation wie das Hauptfenster.
        WebviewUrl::CustomProtocol(ui_lader::adresse("index.html?fenster=telemetrie")),
    )
    .title("AeroACARS — Telemetrie")
    .inner_size(1400.0, 900.0)
    .min_inner_size(800.0, 560.0)
    .resizable(true)
    .build()
    .map_err(|e| {
        UiError::new(
            "window_failed",
            format!("Fenster ließ sich nicht öffnen: {e}"),
        )
    })?;
    Ok(())
}

/// Fenster, die den Telemetrie-Strom bekommen.
const TELEMETRIE_FENSTER: [&str; 2] = ["main", "telemetrie"];

/// Zusatzwerte beim aktiven Adapter an- oder abmelden.
fn telemetrie_zusatz_setzen(app: &AppHandle, aktiv: bool) {
    let state = app.state::<AppState>();
    let kind = read_sim_config(app).kind;
    let xp = if aktiv && kind.is_xplane() {
        telemetrie::xplane_zusatzfelder()
    } else {
        Vec::new()
    };
    // Mit Semikolon: ohne waere das `if let` auf dem Mac der Schlussausdruck
    // der Funktion, und die Sperre lebte laenger als `state` (E0597).
    if let Ok(a) = state.xplane.lock() {
        a.zusatz_setzen(xp);
    };
    #[cfg(target_os = "windows")]
    {
        let ms = if aktiv && kind.is_msfs() {
            telemetrie::msfs_zusatzfelder()
        } else {
            Vec::new()
        };
        if let Ok(a) = state.msfs.lock() {
            a.zusatz_setzen(ms);
        };
    }
}

fn telemetrie_zusatz_werte(app: &AppHandle) -> HashMap<String, f64> {
    let state = app.state::<AppState>();
    let kind = read_sim_config(app).kind;
    if kind.is_xplane() {
        let roh = state
            .xplane
            .lock()
            .map(|a| a.zusatz_werte())
            .unwrap_or_default();
        return telemetrie::zusatz_umrechnen(roh, true);
    }
    #[cfg(target_os = "windows")]
    {
        if kind.is_msfs() {
            let roh = state
                .msfs
                .lock()
                .map(|a| a.zusatz_werte())
                .unwrap_or_default();
            return telemetrie::zusatz_umrechnen(roh, false);
        }
    }
    HashMap::new()
}

/// Sprit an Bord beim Flugbeginn (`initial_fob_kg`) des laufenden Fluges.
fn telemetrie_sprit_basis(state: &AppState) -> Option<f32> {
    let flug = state.active_flight.lock().ok()?.clone()?;
    let stats = flug.stats.lock().ok()?;
    stats.initial_fob_kg
}

/// Takt des Monitors, laeuft fuer die ganze Lebensdauer der App.
///
/// Je Tick (50 ms): Messpunkt holen, in den Verlauf aufnehmen (10 Hz) und —
/// nur wenn jemand zuschaut — als Ereignis senden. Wechselt die Sicht
/// (Monitor auf/zu, Simulator gewechselt), werden die Zusatzwerte beim
/// Adapter an- bzw. abgemeldet. Ohne Messpunkt passiert nichts.
fn spawn_telemetrie_takt(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut takt = tokio::time::interval(telemetrie::STROM_TAKT);
        takt.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // (aktiv, Simulator) beim letzten Abgleich mit den Adaptern.
        let mut zuletzt: Option<(bool, SimKind)> = None;
        let mut flugzeug: Option<String> = None;
        let mut takt_nr: u32 = 0;
        loop {
            takt.tick().await;
            takt_nr = takt_nr.wrapping_add(1);
            let jetzt = std::time::Instant::now();
            let state = app.state::<AppState>();
            let aktiv = state.telemetrie.aktiv(jetzt);
            let kind = read_sim_config(&app).kind;
            if zuletzt != Some((aktiv, kind)) {
                // Anderer Simulator: der Verlauf gehoert nicht mehr dazu, auch
                // wenn der Flugzeugtitel gleich heisst (Codex-Befund 5).
                if zuletzt.is_some_and(|(_, alt)| alt != kind) {
                    state.telemetrie.verlauf_leeren();
                    flugzeug = None;
                }
                telemetrie_zusatz_setzen(&app, aktiv);
                zuletzt = Some((aktiv, kind));
            }
            let Some(snap) = current_snapshot(&app) else {
                continue;
            };
            // Anderes Flugzeug = der alte Verlauf gehoert nicht mehr dazu.
            if snap.aircraft_title.is_some() && snap.aircraft_title != flugzeug {
                if flugzeug.is_some() {
                    state.telemetrie.verlauf_leeren();
                }
                flugzeug = snap.aircraft_title.clone();
            }
            let mut zusatz = if aktiv {
                telemetrie_zusatz_werte(&app)
            } else {
                HashMap::new()
            };
            // Sprit verbraucht: FOB beim Flugbeginn aus den Flugdaten — der
            // Snapshot des Adapters traegt dort immer 0 (Pruefbericht
            // 26.09.2026), gerechnet wird nur in der Kopie des Positionstakts.
            if let Some(basis) = telemetrie_sprit_basis(&state) {
                zusatz.insert(telemetrie::SPRIT_BASIS.to_string(), basis as f64);
            }
            state.telemetrie.g_spanne_eintragen(&snap, &mut zusatz);
            let frame = telemetrie::frame(&snap, &zusatz);
            state.telemetrie.aufnehmen(&frame, jetzt);
            if aktiv {
                // Nur an die eigenen Fenster — nicht an ein fremdes wie das
                // VATSIM-CDM-Fenster (vats.im), das ebenfalls eine WebView ist.
                for ziel in TELEMETRIE_FENSTER {
                    if state.telemetrie.aktiv_fuer(ziel, jetzt)
                        && app.get_webview_window(ziel).is_some()
                    {
                        let _ = tauri::Emitter::emit_to(&app, ziel, "telemetrie-frame", &frame);
                    }
                }
                // v1.8.1: Tablets ueber die LAN-Bruecke, eigener Kanal und
                // halbe Rate (10 Frames/s).
                if takt_nr % telemetrie::LAN_JEDER_NTE == 0
                    && state
                        .telemetrie
                        .aktiv_mit_praefix(telemetrie::LAN_ZUSCHAUER, jetzt)
                {
                    if let Ok(wert) = serde_json::to_value(&frame) {
                        state
                            .remote_events
                            .send_telemetrie(remote::RemoteEvent::new("telemetrie-frame", wert));
                    }
                }
            }
        }
    });
}

// v0.7.16 introduced an opt-in `set_fenix_beta_enabled` / `get_fenix_beta_enabled`
// Tauri-Command-Paar fuer das Fenix-A32x-Profil. v0.7.17 (F-001) hat das wieder
// entfernt: die Fenix-Erkennung ist jetzt automatisch (siehe AircraftProfile::
// is_fenix()), die LVAR-Overrides werden bedingungslos fuer Fenix-Aircraft
// angewendet, ohne dass der Pilot in den Settings einen Haken setzen muss.

/// X-Plane DataRef inspector: returns the static catalog with the
/// most recent received value per entry. Unlike the MSFS Inspector
/// (where the user adds names manually) the X-Plane catalog is
/// fixed at compile time — every DataRef we subscribe is shown.
/// Pilots use this to verify the UDP feed is alive and the Sim is
/// responding to RREF subscriptions.
#[tauri::command(async)]
fn xplane_inspector_list(state: tauri::State<'_, AppState>) -> Vec<serde_json::Value> {
    let adapter = state.xplane.lock().expect("xplane lock");
    adapter
        .subscribed_datarefs()
        .into_iter()
        .map(|s| serde_json::to_value(s).unwrap_or(serde_json::Value::Null))
        .collect()
}

/// Best-effort detection of the X-Plane install path. Returns the
/// absolute path as a string when found, or `null` when nothing
/// plausible exists — UI then falls back to a folder picker.
///
/// MUST be async + `spawn_blocking`: the Windows path runs
/// `reg.exe query` (sub-process spawn ~200-800ms cold-start) plus
/// up to 4 filesystem `is_dir` probes. Doing that on a sync
/// `#[tauri::command]` blocks the Tauri IPC bandwidth and freezes
/// the Settings panel during its first paint (v0.5.0 release-day
/// regression — pilot reported jerky scrolling + language-switcher
/// hang while the registry probe was running).
#[tauri::command]
async fn xplane_detect_install_path() -> Option<String> {
    tauri::async_runtime::spawn_blocking(|| {
        xplane_plugin_install::detect_install_path().map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .unwrap_or(None)
}

/// Prueft einen im Dialog gewaehlten Ordner und liefert den X-Plane-
/// Hauptordner (auch wenn ein Unterordner gewaehlt wurde), sonst `None`.
/// Fuer Piloten, bei denen die Auto-Erkennung nichts findet.
#[tauri::command]
async fn xplane_check_install_path(path: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        xplane_plugin_install::normalize_install_path(std::path::Path::new(&path))
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .unwrap_or(None)
}

/// Download the matching plugin zip from this AeroACARS version's
/// GitHub release and extract it into `<install_dir>/Resources/
/// plugins/AeroACARS/`. Idempotent — overwrites in place. Returns
/// the install summary (path, bytes, files) on success, or an
/// error message string on failure.
#[tauri::command]
async fn xplane_install_plugin(
    install_dir: String,
) -> Result<xplane_plugin_install::PluginInstallResult, String> {
    let path = std::path::PathBuf::from(install_dir);
    xplane_plugin_install::install_plugin(&path).await
}

// v0.7.13: `xplane_uninstall_plugin` Tauri-Command entfernt — UI hat
// nie einen "Plugin deinstallieren"-Button gehabt. Manuell loescht der
// Pilot via Settings → X-Plane → Plugin-Ordner anzeigen → Loeschen.
// Helper `xplane_plugin_install::uninstall_plugin` ebenfalls entfernt.
// Audit Q4-2026-05.

/// Status of the optional AeroACARS X-Plane Plugin (v0.5.0+
/// "Premium Mode"). The Cockpit tab uses this to show a green
/// "X-PLANE PREMIUM" badge when frame-perfect telemetry is flowing.
///
/// Return shape (JSON):
///   {
///     "active":      bool,    // packets in last 3 s
///     "ever_seen":   bool,    // any packet this session
///     "packet_count": u64,    // total since adapter start
///     "last_error":  string?  // bind-failure reason, if any
///     "protokoll":   0|1|2,   // AP7: 2 = Plugin liefert alle Werte
///     "plugin_version": string?, "xplane_version": u32?,
///     "veraltet":    bool,    // Plugin kann nur Protokoll 1
///     "p2_nicht_erreichbar": bool, // Plugin >= 1.0, aber HALLO ohne Antwort
///     "namen_da": u32, "namen_fehlen": u32
///   }
///
/// Inert when the active sim isn't X-Plane (returns all-false).
#[tauri::command(async)]
fn xplane_premium_status(state: tauri::State<'_, AppState>) -> serde_json::Value {
    let adapter = state.xplane.lock().expect("xplane lock");
    let s = adapter.premium_status();
    let err = adapter.premium_last_error();
    serde_json::json!({
        "active": s.active,
        "ever_seen": s.ever_seen,
        "packet_count": s.packet_count,
        "last_error": err,
        "protokoll": s.protokoll,
        "plugin_version": s.plugin_version,
        "xplane_version": s.xplane_version,
        "veraltet": s.veraltet,
        "p2_nicht_erreichbar": s.p2_nicht_erreichbar,
        "namen_da": s.namen_da,
        "namen_fehlen": s.namen_fehlen,
    })
}

// v0.7.13: `detect_running_sim` Tauri-Command entfernt — geplanter
// "Detect Sim"-Button kam nie ins Settings-UI. Frontend zeigt die
// Sim-Kind-Picker manuell. Audit Q4-2026-05.
