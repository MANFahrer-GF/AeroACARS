/// Hardcoded phpVMS host this build is locked to. Pre-1.0 we ship
/// AeroACARS as a German Sky Group internal beta; opening the
/// app to other VAs (config-driven URL) is a Phase-3 task once
/// the core is hardened. The login UI ignores whatever URL the
/// user types and always uses this — the input field stays for
/// continuity but the value is overwritten before validation.
const ALLOWED_PHPVMS_HOST: &str = "german-sky-group.eu";

/// v0.12.1 (Stream B) — phpVMS 7 `UserState::ACTIVE`. Verified against
/// the official phpVMS source (`app/Enums/UserState.php`): the enum is
/// `PENDING=0, ACTIVE=1, REJECTED=2, ON_LEAVE=3, SUSPENDED=4, DELETED=5`.
/// Every state except ACTIVE blocks login.
const USER_STATE_ACTIVE: i32 = 1;

/// v0.12.1 (Stream B LE6): map a pilot profile's `state` to a login
/// block reason, or `None` if the pilot may use AeroACARS (ACTIVE).
///
/// **Fail-closed:** a missing or unexpected `state` value blocks with
/// `pilot_state_unknown` — we do not let an unverifiable account through.
/// The returned `&str` is both the `UiError` code and the i18n key stem
/// (`login.error.<code>`); `LoginPage.tsx` maps it.
fn pilot_state_block_reason(profile: &api_client::Profile) -> Option<&'static str> {
    match profile.state {
        Some(USER_STATE_ACTIVE) => None,
        Some(0) => Some("pilot_pending"),
        Some(2) => Some("pilot_rejected"),
        Some(3) => Some("pilot_on_leave"),
        // 4 SUSPENDED + 5 DELETED — both are a hard, definitive block.
        // The "suspended" message ("account blocked, contact the VA")
        // fits a deleted account too, so no separate i18n string.
        Some(4) | Some(5) => Some("pilot_suspended"),
        // None (field absent) or any unexpected integer → fail-closed.
        _ => Some("pilot_state_unknown"),
    }
}

/// Authenticate against a phpVMS site. On success: stores key in OS keyring,
/// writes URL to site config, and caches the live `Client` in `AppState`.
///
/// Domain is locked to `ALLOWED_PHPVMS_HOST` for this build. The
/// `url` argument is rewritten to `https://{ALLOWED_PHPVMS_HOST}`
/// regardless of what the UI sent. Pre-1.0 we don't want pilots
/// pointing AeroACARS at random phpVMS instances and reporting
/// bugs that aren't ours.
#[tauri::command]
async fn phpvms_login(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    url: String,
    api_key: String,
) -> Result<LoginResult, UiError> {
    // Hard-lock the host. We allow http/https variants and any path
    // suffix from the input but always force the canonical hostname.
    let _ = url; // input field is decorative for now
    let locked_url = format!("https://{ALLOWED_PHPVMS_HOST}");
    let conn = Connection::new(&locked_url, api_key.trim())?;
    let client = Client::new(conn)?;
    // Codex-Folgefund (adversarial, 05.09.2026, siebte Runde): DENSELBEN
    // Lifecycle-Lock nehmen wie `flight_start`/`flight_adopt`/Resume — ohne
    // ihn koennte ein Login (kein Flug aktiv, Pruefung besteht) waehrend
    // seines eigenen `get_profile()`-Roundtrips pausieren, in dieser Zeit
    // startet ein ANDERER Account einen Flug, und das Login ueberschreibt
    // `state.client` DANACH trotzdem — der Flug liefe dann unter falschen
    // Credentials weiter. Mit dem Lock kann `flight_start` waehrend dieses
    // gesamten Logins nicht gleichzeitig laufen.
    let setup_guard = FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)?;
    let profile = client.get_profile().await?;
    // Codex-Folgefund (dieselbe Runde): der Logout-Riegel verhindert nur
    // DEN Weg ueber Logout — dieser Aufruf ueberschreibt `state.client`
    // unbedingt, auch OHNE vorheriges Logout (z. B. ein abgelaufener Key
    // wird hier einfach neu eingegeben). Waere die neue Identitaet eine
    // ANDERE als die des laufenden Fluges, haette derselbe Cross-Account-
    // Ausfall (alle nachfolgenden API-Aufrufe liefen unter der neuen
    // Identitaet, aber fuer den alten Flug) den Logout-Riegel einfach
    // umgangen. Ein Re-Login DESSELBEN Piloten (Token-Refresh/Key-Rotation
    // waehrend der eigene Flug noch laeuft) bleibt ausdruecklich erlaubt —
    // deshalb `profile.pilot_id` (server-verifiziert, ueberlebt eine
    // Key-Rotation), NICHT `client.identity_fingerprint()` (haengt am
    // Rohschluessel, den genau dieser Login gerade AUSTAUSCHEN will).
    //
    // Die `state.active_flight.is_some()`-Bedingung ist entscheidend
    // (weiterer Codex-Folgefund derselben Runde): `active_flight_owner_
    // identity` allein bleibt nach einem beendeten Flug stehen — ohne diese
    // Bedingung haette JEDER Login nach dem allerersten abgeschlossenen
    // Flug fuer immer den naechsten Piloten ausgesperrt, obwohl gar kein
    // Flug mehr aktiv ist.
    if state
        .active_flight
        .lock()
        .expect("active_flight lock")
        .is_some()
    {
        let laufender_flug_eigentuemer = state
            .active_flight_owner_identity
            .lock()
            .expect("active_flight_owner_identity lock")
            .clone();
        if let Some(eigentuemer) = laufender_flug_eigentuemer {
            if eigentuemer != profile.pilot_id.to_string() {
                return Err(UiError::new(
                    "flight_active",
                    "Ein Flug eines anderen Accounts ist noch aktiv — bitte zuerst \
                     beenden oder abbrechen, bevor du dich mit einem anderen Account \
                     anmeldest.",
                ));
            }
            // Codex-Folgefund (adversarial, 05.09.2026, achte Runde): dieser
            // Re-Login ist erlaubt (derselbe Pilot), aber er behebt NICHT
            // alles. `spawn_position_streamer` bekam beim Flugstart einen
            // GEKLONTEN `Client` per Wert uebergeben und fragt seither nie
            // wieder `current_client(&state)` ab — Positions-Updates laufen
            // fuer den Rest DIESES Fluges mit den ALTEN Zugangsdaten weiter,
            // selbst nach diesem erfolgreichen Re-Login. `flight_end`/
            // `flight_cancel` loesen ihren Client dagegen JEDESMAL frisch
            // auf (`current_client(&state)`) — der Flug laesst sich also
            // trotzdem sauber beenden/abbrechen, auch wenn die
            // Positions-Uebertragung bis dahin mit dem alten Schluessel
            // weiterlaeuft. Ehrlich sichtbar machen statt so tun als sei
            // alles wieder normal (siehe feedback-no-unverified-claims).
            log_activity_handle(
                &app,
                ActivityLevel::Warn,
                "Erneut angemeldet — Positions-Updates laufen mit dem alten Zugang weiter"
                    .to_string(),
                Some(
                    "Der laufende Flug wird weiterhin mit den Zugangsdaten von vor diesem \
                     Login an phpVMS gemeldet, bis er endet. Bei anhaltenden Problemen den \
                     Flug beenden oder abbrechen und danach neu starten."
                        .to_string(),
                ),
            );
        }
    }

    // v0.12.1 (Stream B LE6): pilot-status gate. Only ACTIVE GSG pilots
    // may use AeroACARS — a pending / rejected / on-leave / suspended
    // account is rejected with a status-specific message. The MQTT
    // credential cache is wiped so a previously-active pilot who is now
    // blocked does not keep live-tracking from a stale cache (LE7).
    if let Some(reason) = pilot_state_block_reason(&profile) {
        clear_mqtt_credentials_cache();
        return Err(UiError::new(reason, format!("pilot state gate: {reason}")));
    }

    secrets::store_api_key(KEYRING_ACCOUNT, api_key.trim())
        .map_err(|e| UiError::new("keyring", e.to_string()))?;
    write_site_config(
        &app,
        &SiteConfig {
            url: locked_url.clone(),
        },
    )?;

    let base_url = client.connection().base_url().to_string();
    setze_session_atomar(&state, client.clone(), profile.pilot_id);
    cache_pilot(&state, &profile);
    // Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde): der
    // Runde-12-Fix ("Spawn NACH dem Session-Commit") stellte nur sicher,
    // dass der Hintergrund-Task den NEUEN Piloten liest — er garantierte
    // NICHT, dass ein eventuell noch installierter Handle des VORIGEN
    // Piloten schon weg ist, BEVOR der neue Login als "fertig" gilt.
    // Zwischen diesem Commit und dem tatsaechlichen Anlaufen des
    // gespawnten Tasks (der den Fremd-Handle erst in SEINEM eigenen
    // Idempotenz-Check stoppen wuerde) klafft ein Fenster, in dem ein
    // Flugstart/Auto-Start des neuen Piloten den ALTEN Handle noch
    // benutzen wuerde (z. B. der Positions-Streamer). Deshalb jetzt SYNCHRON,
    // noch bevor der Login als abgeschlossen gilt: einen zur neuen Epoche
    // nicht mehr passenden Handle sofort stoppen.
    {
        let handle_gehoert_nicht_mehr_dieser_epoche = *state
            .mqtt_owner_epoch
            .lock()
            .expect("mqtt_owner_epoch lock")
            != Some(aktuelle_epoche(&state));
        if handle_gehoert_nicht_mehr_dieser_epoche {
            stoppe_mqtt_publisher(&state).await;
        }
    }
    // v0.5.11: kick off live-tracking provisioning in the background.
    // Non-blocking — login completes regardless of whether the
    // provision call succeeds. Pure background feature, no UI.
    //
    // Codex-Folgefund (adversarial, 05.09.2026, zwoelfte Runde): dieser
    // Spawn stand bisher VOR `setze_session_atomar` — der Hintergrund-Task
    // konnte also schon lostraben und `authenticated_pilot_id` lesen,
    // BEVOR die neue Session ueberhaupt committed war (Tauris Runtime
    // garantiert keine Ausfuehrungsreihenfolge zwischen einem gespawnten
    // Task und dem restlichen synchronen Code hier). Bei einem
    // Kontowechsel ohne Logout haette der Task dann noch den ALTEN
    // Piloten gesehen und dessen laufenden Publisher faelschlich als
    // "passt schon" durchgehen lassen. Jetzt erst spawnen, NACHDEM die
    // neue Session bereits steht.
    {
        let app_for_mqtt = app.clone();
        tauri::async_runtime::spawn(async move {
            init_mqtt_publisher_via_provisioning(app_for_mqtt).await;
        });
    }
    // Lifecycle-Lock JETZT freigeben, nicht erst am Funktionsende: `state.
    // client` ist bereits committed (ein gleichzeitiger `flight_start` sieht
    // ab jetzt den richtigen Client), und `try_resume_flight` weiter unten
    // versucht selbst denselben Lock zu nehmen — haette `phpvms_login` ihn
    // noch gehalten, waere jeder Resume-Versuch nach einem Login lautlos
    // uebersprungen worden ("resume already in progress").
    drop(setup_guard);

    // Auto-start the simulator adapter using the persisted selection.
    let saved_kind = gespeicherte_sim_wahl_anwenden(&app, &state);

    // Try to resume an in-progress flight (e.g. after a client crash).
    try_resume_flight(&app, &state).await;

    log_activity(
        &state,
        ActivityLevel::Info,
        format!("Logged in as {}", profile.name),
        Some(format!("Sim: {:?}", saved_kind)),
    );
    Ok(LoginResult { profile, base_url })
}

/// Cache pilot ident + name on AppState (für Discord-Webhook-Posts).
/// Wird nach jedem get_profile()-Call aufgerufen — die phpVMS-API
/// liefert beides als Teil von /api/user.
fn cache_pilot(state: &tauri::State<'_, AppState>, profile: &api_client::Profile) {
    let ident = profile.ident.clone().unwrap_or_default();
    if !ident.is_empty() || !profile.name.is_empty() {
        *state.cached_pilot.lock().expect("cached_pilot lock") =
            Some((ident, profile.name.clone()));
    }
    // v0.7.8 v1.5: Profile.callsign cachen fuer SimBrief-direct-Match.
    *state
        .cached_pilot_callsign
        .lock()
        .expect("cached_pilot_callsign lock") =
        profile.callsign.clone().filter(|s| !s.trim().is_empty());

    // v0.16.23: SimBrief-Identifier aus dem phpVMS-Profil auto-sourcen.
    // Wenn der Pilot KEINEN Identifier in den Settings hat (weder Username
    // noch User-ID) UND das Profil ein nicht-leeres `simbrief_username`
    // liefert, uebernehmen wir es als Username. Das macht
    // `flight_refresh_route_only` (Route-Sync) sofort nutzbar ohne dass
    // der Pilot das Feld in Settings nochmal abtippen muss.
    //
    // HARTE Regel: niemals einen explizit gesetzten Identifier
    // ueberschreiben. Manuelle Settings / localStorage gewinnen IMMER.
    // Deshalb nur fuellen wenn `was_set == false`. `set_simbrief_settings`
    // (Pilot tippt in Settings) ueberschreibt diesen Auto-Wert spaeter
    // problemlos wieder.
    maybe_autosource_simbrief_username(
        &mut state
            .simbrief_settings
            .lock()
            .expect("simbrief_settings lock"),
        profile.simbrief_username.as_deref(),
    );
}

/// Schreibt `state.client` und `state.authenticated_pilot_id` als EINEN
/// atomaren Schritt — haelt BEIDE Mutexe gleichzeitig (Reihenfolge: erst
/// `client`, dann Piloten-ID; `aktuelle_session_atomar` liest in
/// DERSELBEN Reihenfolge, sonst Deadlock-Gefahr).
///
/// Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): der "atomare
/// Schnappschuss" aus Runde 8 (`spawn_pirep_queue_worker`) war nur
/// GEGENUEBER Awaits atomar, nicht gegenueber echter Thread-
/// Nebenlaeufigkeit auf Tauris Multi-Thread-Runtime — zwei UNABHAENGIGE
/// Mutexe geben keine gemeinsame Atomaritaets-Garantie, selbst ohne Await
/// dazwischen, wenn Leser und Schreiber auf verschiedenen OS-Threads
/// laufen koennen. `phpvms_login`/`phpvms_load_session` schrieben diese
/// zwei Felder bisher nacheinander mit ZWEI getrennten Lock/Unlock-Paaren
/// — ein Leser haette dazwischen (neuer) Client + (alte) Identitaet oder
/// umgekehrt sehen koennen. Jetzt schreiben UND lesen alle sicherheits-
/// relevanten Stellen ueber diese beiden Helfer, mit fester Sperr-
/// Reihenfolge, sodass kein Leser mehr eine zerrissene Paarung sehen kann.
///
/// Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde): schreibt
/// jetzt zusaetzlich `state.session_epoch` — als DRITTES Feld in
/// DERSELBEN atomaren Operation, feste Reihenfolge Client → Piloten-ID →
/// Epoche (`aktuelle_session_atomar`/`leere_session_atomar` lesen bzw.
/// schreiben in derselben Reihenfolge). Gibt die neue Epoche zurueck,
/// damit Aufrufer (z. B. `phpvms_login` fuer die MQTT-Provisionierung)
/// sie sich fuer spaetere Vergleiche merken koennen.
fn setze_session_atomar(state: &tauri::State<'_, AppState>, client: Client, pilot_id: i64) -> u64 {
    let mut client_guard = state.client.lock().expect("client mutex");
    let mut pilot_guard = state
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock");
    let mut epoch_guard = state.session_epoch.lock().expect("session_epoch lock");
    *client_guard = Some(client);
    *pilot_guard = Some(pilot_id);
    *epoch_guard += 1;
    *epoch_guard
}

/// Gegenstueck zu `setze_session_atomar` fuer den Logout: leert `state.
/// client`, laesst `authenticated_pilot_id` bewusst unangetastet (siehe
/// dessen Doku), erhoeht aber `session_epoch` — DAS ist seit Runde 13 das
/// autoritative Signal fuer "die Sitzung hat sich geaendert", nicht mehr
/// der Vergleich einzelner Felder. Gibt die neue Epoche zurueck.
fn leere_session_atomar(state: &tauri::State<'_, AppState>) -> u64 {
    let mut client_guard = state.client.lock().expect("client mutex");
    let mut epoch_guard = state.session_epoch.lock().expect("session_epoch lock");
    *client_guard = None;
    *epoch_guard += 1;
    *epoch_guard
}

/// Liest `state.client` + `state.authenticated_pilot_id` + `state.
/// session_epoch` als denselben atomaren Schnappschuss, den
/// `setze_session_atomar` schreibt — siehe dort fuer die Begruendung der
/// festen Sperr-Reihenfolge.
fn aktuelle_session_atomar(state: &tauri::State<'_, AppState>) -> Option<(Client, i64, u64)> {
    let client_guard = state.client.lock().expect("client mutex");
    let pilot_guard = state
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock");
    let epoch_guard = state.session_epoch.lock().expect("session_epoch lock");
    let client = client_guard.clone()?;
    let pilot_id = (*pilot_guard)?;
    Some((client, pilot_id, *epoch_guard))
}

/// Liest nur die aktuelle Epoche — fuer Hintergrund-Tasks (MQTT-
/// Provisionierung, Profil-Refresh), die ihre eigene Start-Epoche
/// unmittelbar vor jeder Mutation gegen den JETZT gueltigen Wert
/// gegenpruefen, ohne dafuer Client/Piloten-ID zu brauchen.
fn aktuelle_epoche(state: &tauri::State<'_, AppState>) -> u64 {
    *state.session_epoch.lock().expect("session_epoch lock")
}

/// v0.16.23: Auto-source-Logik fuer den SimBrief-Username (aus
/// `cache_pilot` extrahiert, damit sie unit-testbar ist ohne
/// `tauri::State`).
///
/// Fuellt `settings.username` NUR wenn:
///   1. Aktuell KEIN Identifier gesetzt ist (weder `username` noch
///      `user_id`) — ein explizit gesetzter Identifier wird NIE
///      ueberschrieben, und
///   2. `profile_simbrief_username` vorhanden + nach Trim nicht-leer ist.
///
/// Gibt `true` zurueck wenn ein Wert uebernommen wurde (fuer Tests +
/// optionales Logging). Idempotent: ein zweiter Aufruf mit gleichem
/// Profil ist nach dem ersten Fuellen ein No-Op (weil dann ein Identifier
/// gesetzt ist).
fn maybe_autosource_simbrief_username(
    settings: &mut SimBriefSettings,
    profile_simbrief_username: Option<&str>,
) -> bool {
    let already_set = settings.username.is_some() || settings.user_id.is_some();
    if already_set {
        return false;
    }
    let Some(name) = profile_simbrief_username else {
        return false;
    };
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    settings.username = Some(name.to_string());
    tracing::info!(
        "SimBrief-Username aus phpVMS-Profil auto-gesourcet (kein expliziter Identifier gesetzt)"
    );
    true
}

// ---- v0.5.11: MQTT live-tracking auto-provisioning --------------------
//
// Pure background feature — pilot-invisible, no UI. Provisioning is
// optional: failure is non-fatal, AeroACARS continues to function as
// before. Spec: aeroacars-live/docs/aeroacars-integration-spec-v2.md.

const MQTT_KEYRING_USERNAME: &str = "mqtt-username";
const MQTT_KEYRING_PASSWORD: &str = "mqtt-password";
const MQTT_KEYRING_VA: &str = "mqtt-va";
const MQTT_KEYRING_PILOT_ID: &str = "mqtt-pilot-id";
const MQTT_KEYRING_BROKER: &str = "mqtt-broker-url";
/// phpVMS-PILOTENNUMMER (`profile.pilot_id`) des Kontos, fuer das die
/// MQTT-Daten provisioniert wurden.
///
/// ⚠ Nicht dasselbe wie `MQTT_KEYRING_PILOT_ID`: Der Recorder fuehrt Piloten
/// unter der phpVMS-BENUTZER-ID (`users.id`), der Client kennt als
/// `authenticated_pilot_id` die Pilotennummer (`users.pilot_id`). Bei GSG
/// sind beide ab Benutzer 15 verschieden (Sven M: id 23, Nummer 19). Bis
/// v1.7.44 wurde trotzdem Recorder-ID gegen Pilotennummer verglichen —
/// bei 13 von 17 Piloten verwarf die Sitzungs-Wiederherstellung deshalb bei
/// JEDEM Start die gespeicherten Daten und provisionierte neu, bis die
/// Anmelde-Bremse des Servers griff (Befund 21.09.2026, Sven M: ganzer Flug
/// ohne Live-Tracking). Die Zuordnung wird deshalb hier gespeichert und
/// Nummer gegen Nummer verglichen (`mqtt_cache_gehoert_angemeldetem`).
const MQTT_KEYRING_PHPVMS_PILOT: &str = "mqtt-phpvms-pilot-nr";

/// Gehoeren die gespeicherten MQTT-Daten zum angemeldeten Konto?
/// `gespeicherte_nr` ist `MQTT_KEYRING_PHPVMS_PILOT` — fehlt sie (Cache aus
/// einer Fassung vor diesem Feld), gilt der Cache als NICHT zugeordnet und
/// es wird einmal neu provisioniert, was die Nummer dann nachtraegt.
fn mqtt_cache_gehoert_angemeldetem(gespeicherte_nr: Option<&str>, angemeldet: i64) -> bool {
    gespeicherte_nr.is_some_and(|nr| nr.trim() == angemeldet.to_string())
}

/// Try to start the MQTT live-tracking publisher.
///
/// Two-stage flow:
///   1. If MQTT credentials are already cached in the keyring (from
///      a previous successful provision), use them directly.
///   2. Otherwise, read the phpVMS API key from the keyring (must be
///      present — the user has logged in at least once) and call
///      live.kant.ovh's `/api/provision` endpoint with it. The
///      server returns MQTT credentials which we cache and use.
///
/// All failures are logged via `tracing::warn!` and ignored — this
/// is a non-fatal background feature. If the user's never logged in,
/// or the VPS is down, or the API key is invalid, the function just
/// returns without starting MQTT.
async fn init_mqtt_publisher_via_provisioning(app: AppHandle) {
    use aeroacars_mqtt::{provision::provision, start, MqttConfig};

    let state = app.state::<AppState>();

    // v0.5.14: Idempotency-Guard. Diese Funktion wird aus drei Stellen
    // aufgerufen — Setup-Hook, cmd_login, phpvms_load_session. Beim
    // App-Start mit gespeicherter Session feuern Setup + load_session
    // praktisch zeitgleich → ohne Guard zwei Clients mit gleichem
    // (alten v0.5.13) bzw. fast-gleichem (v0.5.14, ms-Timestamp)
    // client_id, die sich gegenseitig vom Broker kicken. Ergebnis war
    // Reconnect-Cycle alle 1-10 Sekunden.
    //
    // Mit Guard: nur der erste Call macht `start()`, alle weiteren
    // Aufrufe sind no-op. `phpvms_logout` setzt `state.mqtt = None`
    // sauber zurück, sodass der nächste Login wieder startet.
    //
    // Codex-Folgefund (adversarial, 05.09.2026, elfte Runde): "laeuft schon
    // einer" allein reicht nicht — `phpvms_login` erlaubt ausdruecklich
    // einen Re-Login OHNE vorheriges Logout (solange kein Flug aktiv ist).
    // Ohne den Eigentuemer-Vergleich haette der Idempotenz-Guard einen
    // fremden, noch laufenden Publisher als "schon da" durchgehen lassen —
    // Pilot Bs Positionen/Events waeren ueber Pilot As MQTT-Verbindung
    // gelaufen.
    //
    // Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde): nach
    // DREI Runden Einzel-Patches gegen genau diesen Lebenszyklus — darunter
    // zwei echte WIDERSPRUECHE zwischen fruheren Fixes (Runde 11s "MQTT vor
    // fehlschlagfaehigen Schritten stoppen" widersprach Runde 10/12s
    // Annahme, ein API-Key-/Piloten-ID-Wechsel sei als Abbruch-Signal
    // verlaesslich, weil beide Werte bei Logout NICHT oder erst SPAETER
    // veraendert werden) — ist `state.session_epoch` jetzt das EINZIGE
    // Signal, das dieser gesamte Ablauf noch prueft. Sie steigt SYNCHRON
    // bei JEDER Sitzungsaenderung (Login, Logout, Load-Session), lange
    // bevor irgendeine fehlschlagfaehige Nebenoperation (Keyring, Datei)
    // beginnt.
    let epoche_bei_start = aktuelle_epoche(&state);
    {
        let mut guard = state.mqtt.lock().await;
        if guard.is_some() {
            let eigentuemer_epoche = *state
                .mqtt_owner_epoch
                .lock()
                .expect("mqtt_owner_epoch lock");
            if eigentuemer_epoche == Some(epoche_bei_start) {
                tracing::debug!(
                    "live-tracking: publisher already running for this session, skipping re-init"
                );
                return;
            }
            tracing::info!(
                "live-tracking: laufender Publisher gehoert einer anderen Sitzung \
                 — wird gestoppt, bevor neu provisioniert wird"
            );
            if let Some(handle) = guard.take() {
                handle.shutdown();
            }
        }
    }
    // Fuer den Cache-Filter unten (gecachte Credentials koennen von einem
    // ANDEREN Piloten stammen) — unabhaengig von der Epochen-Pruefung oben,
    // die nur ueber Sitzungs-WECHSEL entscheidet, nicht darueber, WELCHER
    // Pilot gerade aktiv ist.
    let aktuelle_pilot_id = *state
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock");

    // Check cache first.
    let cached = (|| -> Option<MqttConfig> {
        let user = secrets::load_api_key(MQTT_KEYRING_USERNAME)
            .ok()
            .flatten()?;
        let pw = secrets::load_api_key(MQTT_KEYRING_PASSWORD)
            .ok()
            .flatten()?;
        let va = secrets::load_api_key(MQTT_KEYRING_VA).ok().flatten()?;
        let pilot_id = secrets::load_api_key(MQTT_KEYRING_PILOT_ID)
            .ok()
            .flatten()?;
        let broker = secrets::load_api_key(MQTT_KEYRING_BROKER).ok().flatten()?;
        Some(MqttConfig {
            broker_url: broker,
            username: user,
            password: pw,
            va_prefix: va,
            pilot_id,
        })
    })();

    // Codex-Folgefund (adversarial, 05.09.2026, elfte Runde): gecachte
    // MQTT-Credentials tragen die `pilot_id` VON DAMALS — stammen sie von
    // einem anderen Piloten als dem GERADE eingeloggten (Re-Login ohne
    // Logout), waere Pilot As alte MQTT-Identitaet fuer Pilot Bs Flug
    // uebernommen worden. Nur verwenden, wenn sie zum aktuellen Account
    // passen oder (Setup-Hook-Fall) noch gar keine Piloten-ID bekannt ist.
    //
    // ⚠ `c.pilot_id` ist die Recorder-ID (phpVMS-Benutzer-ID), die
    // angemeldete ID die Pilotennummer — verglichen wird deshalb die mit
    // gespeicherte Pilotennummer (siehe `MQTT_KEYRING_PHPVMS_PILOT`).
    let cache_pilot_nr = secrets::load_api_key(MQTT_KEYRING_PHPVMS_PILOT)
        .ok()
        .flatten();
    let cached = cached.filter(|_| {
        aktuelle_pilot_id
            .is_none_or(|id| mqtt_cache_gehoert_angemeldetem(cache_pilot_nr.as_deref(), id))
    });

    let cfg = if let Some(c) = cached {
        tracing::info!("live-tracking: using cached MQTT credentials");
        c
    } else {
        // No cache — provision from server.
        let api_key = match secrets::load_api_key(KEYRING_ACCOUNT) {
            Ok(Some(k)) => k,
            Ok(None) => {
                tracing::debug!(
                    "live-tracking: no phpVMS API key in keyring yet — \
                     will retry after login"
                );
                return;
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "live-tracking: keyring read failed — skipping"
                );
                return;
            }
        };

        // Befund 21.09.2026: vorher reichte EIN gescheiterter Aufruf, und
        // das Live-Tracking blieb fuer die ganze Sitzung aus — ohne Sitzung
        // im Recorder gingen danach auch Flug- und Diagnose-Log verloren.
        // Voruebergehende Fehler werden jetzt wiederholt (Abstand siehe
        // `naechster_versuch`), bis es klappt, die Sitzung wechselt oder
        // ein anderer Aufruf den Publisher schon gestartet hat.
        // App-Start und Sitzungs-Wiederherstellung starten die
        // Provisionierung fast gleichzeitig. Nur EINE Schleife darf
        // wiederholen — zwei wuerden die Anmelde-Bremse des Servers (10 pro
        // Stunde je IP) doppelt so schnell fuellen.
        static WIEDERHOLUNG_LAEUFT: AtomicBool = AtomicBool::new(false);
        struct WiederholungFrei;
        impl Drop for WiederholungFrei {
            fn drop(&mut self) {
                WIEDERHOLUNG_LAEUFT.store(false, Ordering::SeqCst);
            }
        }
        let mut wiederholung: Option<WiederholungFrei> = None;
        let mut versuch: u32 = 0;
        let resp = loop {
            match provision(&api_key, None).await {
                Ok(r) => break r,
                Err(e) => {
                    if wiederholung.is_none() {
                        if WIEDERHOLUNG_LAEUFT.swap(true, Ordering::SeqCst) {
                            tracing::info!(
                                error = %e,
                                "live-tracking: provision call failed — eine andere Wiederholung laeuft schon"
                            );
                            return;
                        }
                        wiederholung = Some(WiederholungFrei);
                    }
                    let warten = aeroacars_mqtt::provision::naechster_versuch(&e, versuch);
                    tracing::warn!(
                        error = %e,
                        versuch,
                        naechster_versuch_s = warten.map(|d| d.as_secs()),
                        "live-tracking: provision call failed (non-fatal)"
                    );
                    let Some(warten) = warten else {
                        return;
                    };
                    tokio::time::sleep(warten).await;
                    if aktuelle_epoche(&state) != epoche_bei_start {
                        tracing::info!(
                            "live-tracking: Sitzung gewechselt — Provisionierung dieses Laufs endet"
                        );
                        return;
                    }
                    if state.mqtt.lock().await.is_some() {
                        tracing::info!(
                            "live-tracking: Publisher laeuft inzwischen — Wiederholung endet"
                        );
                        return;
                    }
                    versuch = versuch.saturating_add(1);
                }
            }
        };

        // Cache for next launch. Failures here are non-fatal —
        // we'll just provision again next time. Log (key NAME + error only;
        // never the secret value) so a silently-failing keyring is diagnosable.
        let store = |k: &str, v: &str| {
            if let Err(e) = secrets::store_api_key(k, v) {
                tracing::warn!(key = k, error = %e, "live-tracking: caching MQTT credential failed (non-fatal, will re-provision)");
            }
        };
        // Codex-Abnahme (15.09.2026): `provision(...).await` oben ist ein
        // echter Await — ein Kontowechsel/Logout in dieser Zeit erhoeht die
        // Epoche. Vorher wurden die Credentials trotzdem in den Keyring
        // geschrieben (Epochen-Pruefung erst NACH `start`), ein ueberholter
        // Request konnte so die Zugangsdaten des vorigen Piloten nach dem
        // Logout-Loeschen wieder zurueckschreiben. Jetzt: pruefen+schreiben
        // atomar unter dem Epochen-Lock; ein Logout, der die Epoche erhoeht,
        // wartet auf das Schreiben und loescht danach (`phpvms_logout`:
        // `leere_session_atomar` VOR `clear_mqtt_credentials_cache`).
        let geschrieben =
            mqtt_cache_schreiben_wenn_epoche_gilt(&state.session_epoch, epoche_bei_start, || {
                store(MQTT_KEYRING_USERNAME, &resp.username);
                store(MQTT_KEYRING_PASSWORD, &resp.password);
                store(MQTT_KEYRING_VA, &resp.va_prefix);
                store(MQTT_KEYRING_PILOT_ID, &resp.pilot_id);
                store(MQTT_KEYRING_BROKER, &resp.broker_url);
                // Ohne bekannte Pilotennummer (Setup-Hook vor dem Login)
                // eine alte Nummer entfernen — sie gehoerte evtl. zu einem
                // anderen Konto. Die naechste Provisionierung traegt sie nach.
                match aktuelle_pilot_id {
                    Some(nr) => store(MQTT_KEYRING_PHPVMS_PILOT, &nr.to_string()),
                    None => {
                        let _ = secrets::delete_api_key(MQTT_KEYRING_PHPVMS_PILOT);
                    }
                }
            });
        if !geschrieben {
            tracing::warn!(
                "live-tracking: Sitzung aenderte sich waehrend der Provisionierung — \
                 Antwort verworfen, nichts in den Keyring geschrieben"
            );
            return;
        }
        tracing::info!(
            pilot_id = %resp.pilot_id,
            va = %resp.va_prefix,
            newly_created = resp.newly_created,
            "live-tracking provisioned"
        );
        resp.into()
    };

    // Dasselbe Token, mit dem MQTT gleich laeuft, auch fuer die Inhalte
    // (Skin, Kartenstil, VATGlasses, Discord) — nicht erneut aus dem
    // Speicher lesen: schlug das Schreiben dort fehl, haette `live_inhalt`
    // sonst die ganze Sitzung kein oder ein altes Token.
    let inhalte_token = cfg.password.clone();
    let handle = match start(cfg) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!(error = %e, "live-tracking: MQTT start failed");
            return;
        }
    };

    // Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): hier ist der
    // erste Punkt, an dem sich ueberhaupt etwas pruefen laesst (der Handle
    // existiert jetzt). Hat sich die Sitzung seither geaendert, wird der
    // frisch gebaute Handle verworfen statt installiert. Seit Runde 13:
    // Epoche statt API-Key (siehe Doku bei `session_epoch`).
    if aktuelle_epoche(&state) != epoche_bei_start {
        tracing::warn!(
            "live-tracking: Sitzung aenderte sich waehrend der Provisionierung — \
             frisch gebauter MQTT-Handle wird verworfen, nicht installiert"
        );
        handle.shutdown();
        return;
    }

    // v0.13.0 Slice 6: Take the integrity-flag receiver and forward
    // each event as a Tauri event "integrity-flag" to the React UI.
    // Done once per Handle creation — Handle::take_integrity_rx is
    // protected by Option-Take so subsequent calls return None.
    if let Some(mut integrity_rx) = handle.take_integrity_rx().await {
        let emit_app = app.clone();
        tokio::spawn(async move {
            while let Some(event) = integrity_rx.recv().await {
                tracing::debug!(
                    severity = %event.session_effective_severity,
                    "integrity_flag received from recorder"
                );
                let payload = serde_json::json!({
                    "session_id": event.session_id,
                    "session_effective_severity": event.session_effective_severity,
                    "flag": event.flag,
                });
                let _ = tauri::Emitter::emit(&emit_app, "integrity-flag", payload.clone());
                // v0.16.0 (#LAN-Remote): also fan out to LAN WS subscribers.
                emit_app
                    .state::<AppState>()
                    .remote_events
                    .send(remote::RemoteEvent::new("integrity-flag", payload));
            }
            tracing::debug!("integrity_flag forwarder loop exiting");
        });
    }

    // Pilotenchat: derselbe Weg wie beim Integritaets-Kanal. Der zweite
    // `send` an `remote_events` ist die LAN-Bruecke — dadurch sieht das
    // Tablet dieselben Zurufe zur selben Zeit, ohne eigene Verdrahtung.
    if let Some(mut chat_rx) = handle.take_chat_rx().await {
        let emit_app = app.clone();
        tokio::spawn(async move {
            while let Some(n) = chat_rx.recv().await {
                let payload = serde_json::to_value(&n).unwrap_or(serde_json::Value::Null);
                let _ = tauri::Emitter::emit(&emit_app, "chat-nachricht", payload.clone());
                emit_app
                    .state::<AppState>()
                    .remote_events
                    .send(remote::RemoteEvent::new("chat-nachricht", payload));
            }
            tracing::debug!("Chat-Weiterleiter beendet");
        });
    }

    // Codex-Folgefund (adversarial, 05.09.2026, zwoelfte Runde): die
    // Pruefung aus Runde 11 lag zwar unmittelbar VOR dieser Zeile im
    // Quelltext, aber `state.mqtt.lock().await` ist selbst ein echter
    // Await — zwischen dem Bestehen der Pruefung und dem tatsaechlichen
    // Erwerb des Locks konnte ein paralleler Logout (der denselben Lock
    // ueber `stoppe_mqtt_publisher` nimmt) dazwischenfunken. Die Pruefung
    // muss deshalb INNERHALB der gehaltenen Sperre stattfinden, nicht
    // davor — nur so kann kein Logout mehr zwischen „geprueft" und
    // „installiert" hindurchschluepfen.
    // Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde): die
    // Epoche wird HIER, INNERHALB der gehaltenen Sperre, erneut gelesen —
    // nicht (wie zwischenzeitlich der API-Key) VOR dem Lock-Erwerb. Ein
    // paralleler Logout erhoeht die Epoche synchron als Teil SEINES
    // Session-Umbaus, lange bevor er ueberhaupt versucht, `state.mqtt` zu
    // sperren (`stoppe_mqtt_publisher`) — durch die feste Mutex-Reihenfolge
    // ist entweder der Logout ODER diese Installation zuerst vollstaendig
    // fertig, nie beides gleichzeitig ueberlappend. Ein Wechsel der Epoche
    // ist dadurch garantiert sichtbar, sobald dieser Code den Lock haelt.
    {
        let mut mqtt_guard = state.mqtt.lock().await;
        if aktuelle_epoche(&state) != epoche_bei_start {
            tracing::warn!(
                "live-tracking: Sitzung aenderte sich waehrend der Provisionierung — \
                 frisch gebauter MQTT-Handle wird verworfen, nicht installiert"
            );
            drop(mqtt_guard);
            handle.shutdown();
            return;
        }
        *mqtt_guard = Some(handle);
        // Unter derselben Sperre wie die Installation: ein paralleler
        // Logout (`stoppe_mqtt_publisher` nimmt diese Sperre, danach
        // `clear_mqtt_credentials_cache`) leert das Token damit sicher
        // NACH dem Setzen — es bleibt nie das eines Abgemeldeten stehen.
        live_zugang::token_setzen(Some(inhalte_token));
    }

    // Ab hier gilt das Token (Cache oder frisch provisioniert). Skin,
    // Kartenstil und VATGlasses holen sich ihre Inhalte daraufhin neu —
    // vorher bekamen sie vom Server 401 und blieben bei der Vorgabe
    // (29.09.2026, siehe live_zugang.rs). Das Tablet ueber die LAN-Bruecke
    // bekommt dasselbe Signal.
    let _ = tauri::Emitter::emit(&app, "live-zugang-bereit", ());
    state.remote_events.send(remote::RemoteEvent::new(
        "live-zugang-bereit",
        serde_json::Value::Null,
    ));
    discord_rpc::nach_anmeldung();
    *state
        .mqtt_owner_epoch
        .lock()
        .expect("mqtt_owner_epoch lock") = Some(epoche_bei_start);
    tracing::info!("live-tracking publisher running");
    spawn_flug_logs_nachreichen(&app);
}

/// Wie weit das Nachreichen zurueckschaut (P-7.23: 30 Tage, ohne
/// Stueckgrenze — fehlende Protokolle gehen in die Warteschlange, die sie
/// einzeln sendet).
const NACHREICHEN_MAX_ALTER: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);
/// Hoechstens so viele DIAGNOSE-Logs je Programmstart hochladen.
const NACHREICHEN_MAX_UPLOADS: usize = 5;
/// Hoechstens so viele IDs je Statusabfrage (Grenze des Endpunkts).
const NACHREICHEN_ABFRAGE_MAX: usize = 50;
/// So weit zurueck werden fehlende DIAGNOSE-Logs nachgereicht. Kurz, weil
/// der Client nur das Tagesprotokoll von heute und gestern mitschickt.
const NACHREICHEN_DIAGNOSE_MAX_ALTER: std::time::Duration =
    std::time::Duration::from_secs(36 * 3600);
/// Nur das Ende der Datei nach `pirep_filed` absuchen — Flugprotokolle
/// werden bis zu 15 MB gross, das Ereignis steht am Schluss.
const NACHREICHEN_ENDE_BYTES: u64 = 512 * 1024;

/// Ein lokales Flugprotokoll als Kandidat fuers Nachreichen.
#[derive(Debug, Clone)]
struct LokalesProtokoll {
    pirep_id: String,
    alter: std::time::Duration,
    /// Enthaelt es ein `pirep_filed`? Abgebrochene Fluege nicht nachreichen —
    /// der Import legte sonst eine Sitzung fuer einen Flug an, den es in
    /// phpVMS so nie gab.
    eingereicht: bool,
}

/// Welche lokalen Protokolle kommen fuers Nachfragen beim Server in Frage?
/// Juengste zuerst, ohne den laufenden Flug, hoechstens `max` (Obergrenze
/// des Status-Endpunkts: 50).
fn nachreich_kandidaten(
    protokolle: &[LokalesProtokoll],
    aktiver_pirep: Option<&str>,
    max_alter: std::time::Duration,
    max: usize,
) -> Vec<String> {
    let mut passend: Vec<&LokalesProtokoll> = protokolle
        .iter()
        .filter(|p| p.eingereicht && p.alter <= max_alter)
        .filter(|p| aktiver_pirep != Some(p.pirep_id.as_str()))
        .collect();
    passend.sort_by_key(|p| p.alter);
    passend
        .into_iter()
        .take(max)
        .map(|p| p.pirep_id.clone())
        .collect()
}

/// Was fehlt laut Server? Nur ein ausdrueckliches `false` zaehlt — ein
/// Server ohne das Feld (`None`) bekommt nichts nachgeschickt.
fn fehlt_auf_dem_server(status: &[aeroacars_mqtt::pirep_status::PirepPruefstatus]) -> Vec<String> {
    status
        .iter()
        .filter(|s| s.flug_log_vorhanden == Some(false))
        .map(|s| s.pirep_id.clone())
        .collect()
}

/// Wie beim Flugprotokoll, nur fuer das Diagnose-Log.
fn diagnose_fehlt_auf_dem_server(
    status: &[aeroacars_mqtt::pirep_status::PirepPruefstatus],
) -> Vec<String> {
    status
        .iter()
        .filter(|s| s.diagnose_log_vorhanden == Some(false))
        .map(|s| s.pirep_id.clone())
        .collect()
}

/// Kandidaten fuers Nachreichen des DIAGNOSE-Logs — anders als beim
/// Flugprotokoll auch ABGEBROCHENE Fluege, denn genau deren technische
/// Zeilen fehlen uns (23.09.2026).
///
/// Kurzes Fenster: Mitgeschickt wird das Tagesprotokoll der App (heute und
/// gestern). Fuer einen Flug von vorgestern stuende darin nichts mehr ueber
/// ihn — das waere ein Upload ohne Inhalt.
fn diagnose_kandidaten(
    protokolle: &[LokalesProtokoll],
    aktiver_pirep: Option<&str>,
    max_alter: std::time::Duration,
    max: usize,
) -> Vec<String> {
    let mut passend: Vec<&LokalesProtokoll> = protokolle
        .iter()
        .filter(|p| p.alter <= max_alter)
        .filter(|p| aktiver_pirep != Some(p.pirep_id.as_str()))
        .collect();
    passend.sort_by_key(|p| p.alter);
    passend
        .into_iter()
        .take(max)
        .map(|p| p.pirep_id.clone())
        .collect()
}

/// P-7.23: Reiht jedes eingereichte Protokoll ein, das laut Server fehlt
/// (Besitzer `besitzer`); gibt die Zahl der eingereihten zurueck. Abgebrochene
/// Fluege (nicht in `flug_kandidaten`) bleiben draussen.
fn fehlende_einreihen(
    daten: &std::path::Path,
    status: &[aeroacars_mqtt::pirep_status::PirepPruefstatus],
    flug_kandidaten: &[String],
    besitzer: &str,
) -> usize {
    let mut n = 0;
    for pirep_id in fehlt_auf_dem_server(status).into_iter().filter(|id| flug_kandidaten.contains(id)) {
        match upload_warteschlange::protokoll_einreihen(daten, &pirep_id, besitzer) {
            Ok(_) => n += 1,
            Err(e) => tracing::warn!(pirep_id = %pirep_id, error = %e, "Nachreichen: nicht einreihbar"),
        }
    }
    n
}

/// P-7.23: alle Kandidaten ohne Stueckgrenze, Diagnose zuerst, jede ID
/// einmal, in Abfragen zu hoechstens `NACHREICHEN_ABFRAGE_MAX` IDs.
fn abfrage_bloecke(diagnose: &[String], flug: &[String]) -> Vec<Vec<String>> {
    let mut alle: Vec<String> = Vec::new();
    for id in diagnose.iter().chain(flug) {
        if !alle.contains(id) {
            alle.push(id.clone());
        }
    }
    alle.chunks(NACHREICHEN_ABFRAGE_MAX).map(<[String]>::to_vec).collect()
}

/// Steht `"type":"pirep_filed"` im letzten Stueck der Datei?
fn protokoll_ist_eingereicht(pfad: &std::path::Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(pfad) else {
        return false;
    };
    let laenge = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = laenge.saturating_sub(NACHREICHEN_ENDE_BYTES);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return false;
    }
    let mut ende = Vec::new();
    if f.read_to_end(&mut ende).is_err() {
        return false;
    }
    let nadel = br#""type":"pirep_filed""#;
    ende.windows(nadel.len()).any(|w| w == nadel)
}

/// Reicht Flugprotokolle nach, die der Server nicht hat — einmal je
/// Programmstart, kurz nach der ersten erfolgreichen Verbindung. Seit 3.0
/// (P-7.23) das Sicherheitsnetz hinter der Warteschlange: Fehlendes wird dort
/// eingereiht, nicht hier hochgeladen.
///
/// Befund 21.09.2026 (Sven M): Ein Flug ohne Live-Sitzung verlor sein
/// Flugprotokoll, weil der Upload nur EINMAL beim Einreichen versucht wurde
/// und der Recorder ihn damals ablehnte. Ab Recorder-Stand
/// `fix/verbindung-nachhaltig` nimmt der Server solche Protokolle an; dieser
/// Schritt sorgt dafuer, dass sie auch ankommen — ebenso nach Netz- oder
/// Serverausfaellen beim Einreichen.
fn spawn_flug_logs_nachreichen(app: &AppHandle) {
    static GELAUFEN: AtomicBool = AtomicBool::new(false);
    if GELAUFEN.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Den Start nicht mit Uploads belasten.
        tokio::time::sleep(std::time::Duration::from_secs(90)).await;
        if !flug_logs_nachreichen(&app).await {
            // Voraussetzungen fehlten (noch nicht angemeldet o. ae.) — der
            // naechste erfolgreiche Verbindungsaufbau darf es erneut versuchen.
            GELAUFEN.store(false, Ordering::SeqCst);
        }
    });
}

/// `false`, wenn die Voraussetzungen fehlten und nichts versucht wurde.
async fn flug_logs_nachreichen(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let epoche = aktuelle_epoche(&state);
    let Some(angemeldet) = *state
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock")
    else {
        return false;
    };
    let (Some(username), Some(password)) = (
        secrets::load_api_key(MQTT_KEYRING_USERNAME).ok().flatten(),
        secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten(),
    ) else {
        return false;
    };
    // Nur mit Zugangsdaten DIESES Kontos (siehe `MQTT_KEYRING_PHPVMS_PILOT`).
    let nr = secrets::load_api_key(MQTT_KEYRING_PHPVMS_PILOT)
        .ok()
        .flatten();
    if !mqtt_cache_gehoert_angemeldetem(nr.as_deref(), angemeldet) {
        return false;
    }
    let Ok(dir) = app.path().app_data_dir().map(|d| d.join("flight_logs")) else {
        return false;
    };
    let aktiv = state
        .active_flight
        .lock()
        .expect("active_flight lock")
        .as_ref()
        .map(|f| f.pirep_id.clone());

    let jetzt = std::time::SystemTime::now();
    let mut protokolle = Vec::new();
    // Erst lesen, wenn der Schreib-Faden alles abgelegt hat (`record_event`).
    let _ = tokio::task::spawn_blocking(flugprotokoll_abwarten).await;
    if let Ok(eintraege) = std::fs::read_dir(&dir) {
        for e in eintraege.flatten() {
            let pfad = e.path();
            if pfad.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(pirep_id) = pfad
                .file_stem()
                .and_then(|x| x.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            let Some(alter) = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|m| jetzt.duration_since(m).ok())
            else {
                continue;
            };
            if alter > NACHREICHEN_MAX_ALTER {
                continue;
            }
            let eingereicht = protokoll_ist_eingereicht(&pfad);
            protokolle.push(LokalesProtokoll {
                pirep_id,
                alter,
                eingereicht,
            });
        }
    }
    let flug_kandidaten =
        nachreich_kandidaten(&protokolle, aktiv.as_deref(), NACHREICHEN_MAX_ALTER, usize::MAX);
    let diag_kandidaten = diagnose_kandidaten(
        &protokolle,
        aktiv.as_deref(),
        NACHREICHEN_DIAGNOSE_MAX_ALTER,
        NACHREICHEN_ABFRAGE_MAX,
    );
    let bloecke = abfrage_bloecke(&diag_kandidaten, &flug_kandidaten);
    if bloecke.is_empty() {
        return true;
    }
    let geprueft: usize = bloecke.iter().map(Vec::len).sum();
    let mut status = Vec::new();
    for teil in &bloecke {
        match aeroacars_mqtt::pirep_status::pruefstatus_abrufen(teil, &username, &password, None).await {
            Ok(s) => status.extend(s),
            Err(e) => {
                tracing::warn!(error = %e, "Nachreichen: Status nicht abrufbar — naechster Start");
                return true;
            }
        }
    }
    // Nur Fluege, die auch eingereicht wurden: Seit die Diagnose-Kandidaten
    // in derselben Abfrage stehen, enthaelt die Antwort auch abgebrochene
    // Fluege — deren VOLLES Flugprotokoll darf nicht hochgehen, sonst legt
    // der Import eine Sitzung fuer einen Flug an, den es in phpVMS nie gab
    // (Cloud-QS 23.09.2026). Symmetrisch zum Diagnose-Zweig weiter unten.
    let fehlend = fehlt_auf_dem_server(&status).into_iter().filter(|id| flug_kandidaten.contains(id)).count();
    tracing::info!(
        geprueft,
        fehlend,
        "Nachreichen: Flugprotokolle mit dem Server abgeglichen"
    );
    // P-7.23: in die Warteschlange (Besitzer = angemeldetes Konto); der
    // Arbeiter sendet sie einzeln und wiederholt, bis sie ankommen.
    if let Ok(daten) = app.path().app_data_dir() {
        if fehlende_einreihen(&daten, &status, &flug_kandidaten, &angemeldet.to_string()) > 0 {
            upload_warteschlange_anstossen();
        }
    }

    // Fehlende Diagnose-Logs — auch von abgebrochenen Fluegen. Beim Abbruch
    // laedt der Client sie selbst hoch; das hier faengt die Faelle, in denen
    // das scheiterte (kein Netz, Server weg, App gleich mit beendet).
    //
    // NICHT bei selbst eingeschalteter Fehlersuche: Mit `RUST_LOG` schreibt
    // rumqttc seine Pakete mit, und im CONNECT stehen Name und Passwort —
    // dieselbe Regel wie beim Upload nach dem Einreichen.
    if std::env::var_os("RUST_LOG").is_none() {
        let diagnose_fehlt: Vec<String> = diagnose_fehlt_auf_dem_server(&status)
            .into_iter()
            .filter(|id| diag_kandidaten.contains(id))
            .collect();
        if !diagnose_fehlt.is_empty() {
            tracing::info!(
                fehlend = diagnose_fehlt.len(),
                "Nachreichen: Diagnose-Logs fehlen auf dem Server"
            );
        }
        // So weit zurueck, wie ein Kandidat alt sein darf: Die Zeilen eines
        // 35 Stunden alten Fluges stehen in einer aelteren Tagesdatei. Ohne
        // sie ginge ein Upload raus, der den Flug auf dem Server als
        // erledigt markiert, ohne das Gesuchte zu enthalten (Cloud-QS
        // 23.09.2026).
        let tage = (NACHREICHEN_DIAGNOSE_MAX_ALTER.as_secs() as i64) / 86_400 + 1;
        let dateien = diagnose_logs_der_letzten_tage(tage);
        // Einmal lesen, einmal packen — die fehlenden PIREPs bekommen alle
        // DIESELBEN Tagesdateien. Vorher lief das je PIREP neu, bis zu
        // fuenfmal 8 MB (Cloud-QS 23.09.2026).
        let paket = if dateien.is_empty() {
            None
        } else {
            match aeroacars_mqtt::log_upload::diagnose_paket_bauen(&dateien, true).await {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::warn!(error = %e, "Nachreichen: Diagnose-Paket nicht lesbar");
                    None
                }
            }
        };
        let Some(paket) = paket else {
            return true;
        };
        for pirep_id in diagnose_fehlt.into_iter().take(NACHREICHEN_MAX_UPLOADS) {
            if aktuelle_epoche(&state) != epoche {
                tracing::info!("Nachreichen: Sitzung gewechselt — abgebrochen");
                break;
            }
            let _sperre = UPLOAD_SPERRE.lock().await;
            match aeroacars_mqtt::log_upload::diagnose_paket_senden(
                &paket, &pirep_id, &username, &password, None,
            )
            .await
            {
                Ok(stats) => tracing::info!(
                    pirep_id = %pirep_id,
                    gzip_kb = stats.compressed_size / 1024,
                    "Nachreichen: Diagnose-Log hochgeladen"
                ),
                Err(e) => tracing::warn!(
                    pirep_id = %pirep_id,
                    error = %e,
                    "Nachreichen: Diagnose-Upload fehlgeschlagen — naechster Start versucht es wieder"
                ),
            }
        }
    }
    true
}

#[cfg(test)]
#[path = "tests_lib/nachreichen_tests.rs"]
mod nachreichen_tests;

/// Stoppt einen eventuell laufenden MQTT-Publisher UND vergisst, welcher
/// Epoche er gehoerte — die beiden gehoeren immer zusammen (siehe
/// `mqtt_owner_epoch`s Doku), deshalb ein gemeinsamer Helfer statt drei
/// Kopien (`phpvms_logout`, Status-Gate in `phpvms_load_session`,
/// Auth-Fehler in `phpvms_load_session`).
async fn stoppe_mqtt_publisher(state: &tauri::State<'_, AppState>) {
    if let Some(handle) = state.mqtt.lock().await.take() {
        handle.shutdown();
    }
    *state
        .mqtt_owner_epoch
        .lock()
        .expect("mqtt_owner_epoch lock") = None;
}

/// Forget cached MQTT credentials. Called from logout — next session
/// re-provisions cleanly. The phpVMS API key in `KEYRING_ACCOUNT`
/// already gets cleared by the existing logout flow.
fn clear_mqtt_credentials_cache() {
    live_zugang::token_setzen(None);
    for key in [
        MQTT_KEYRING_USERNAME,
        MQTT_KEYRING_PASSWORD,
        MQTT_KEYRING_VA,
        MQTT_KEYRING_PILOT_ID,
        MQTT_KEYRING_BROKER,
        MQTT_KEYRING_PHPVMS_PILOT,
    ] {
        // Weiterhin nicht fatal (Logout soll nicht daran scheitern), aber
        // nicht mehr stumm: ein stehengebliebener Eintrag waere sonst nicht
        // diagnostizierbar. Nur Schluesselname + Fehler, nie der Wert.
        if let Err(e) = secrets::delete_api_key(key) {
            tracing::warn!(key, error = %e, "live-tracking: Loeschen des MQTT-Credential-Caches fehlgeschlagen");
        }
    }
}

#[cfg(test)]
#[path = "tests_lib/mqtt_cache_zuordnung_tests.rs"]
mod mqtt_cache_zuordnung_tests;

/// Schreibt den MQTT-Credential-Cache (`schreibe`) NUR, wenn die Sitzung
/// seit `epoche_bei_start` unveraendert ist — Pruefung und Schreiben
/// laufen unter DEMSELBEN gehaltenen `session_epoch`-Lock, damit kein
/// Login/Logout (`setze_session_atomar`/`leere_session_atomar` erhoehen
/// die Epoche unter diesem Lock) zwischen "geprueft" und "geschrieben"
/// hindurchschluepfen kann. Gibt `true` zurueck, wenn geschrieben wurde.
/// `schreibe` darf keine anderen AppState-Locks nehmen (Sperr-Reihenfolge).
fn mqtt_cache_schreiben_wenn_epoche_gilt(
    session_epoch: &Mutex<u64>,
    epoche_bei_start: u64,
    schreibe: impl FnOnce(),
) -> bool {
    let epoch_guard = session_epoch.lock().expect("session_epoch lock");
    if *epoch_guard != epoche_bei_start {
        return false;
    }
    schreibe();
    drop(epoch_guard);
    true
}

#[cfg(test)]
#[path = "tests_lib/mqtt_cache_epochen_tests.rs"]
mod mqtt_cache_epochen_tests;

/// Forget the current session. Removes the keyring entry and site config,
/// clears the in-memory client.
#[tauri::command]
async fn phpvms_logout(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), UiError> {
    // Codex-Folgefund (adversarial, 05.09.2026, sechste Runde): der
    // Eigentuemer-Schutz aus den vorigen Runden (pirep_queue, Resume-Ablage)
    // deckt nur, WAS AUF DER PLATTE liegt — nicht die LEBENDEN API-Aufrufe
    // waehrend eines laufenden Fluges (Positions-Updates, PIREP-Filing,
    // Cancel, MQTT). Alle diese lesen `current_client(&state)` frisch bei
    // jedem Aufruf; ein Logout mitten im Flug wuerde JEDEN nachfolgenden
    // Aufruf mit den Credentials des NAECHSTEN eingeloggten Piloten
    // ausfuehren, unabhaengig davon wem der Flug gehoert. Statt jede
    // einzelne Aufrufstelle im gesamten Flug-Lebenszyklus einzeln
    // abzusichern (Positions-Worker, normales/manuelles Filing, Cancel,
    // MQTT-Finalisierung — eine sehr breite Flaeche mit hohem Streu-Risiko),
    // wird die Ursache verriegelt: ein Logout waehrend ein Flug aktiv ist,
    // ist schlicht nicht erlaubt. Der urspruengliche Kommentar unten ("ein
    // anderer Pilot kann sich auf derselben Maschine anmelden") gilt
    // weiterhin — nur eben erst NACHDEM der laufende Flug beendet/
    // abgebrochen wurde, nicht waehrenddessen.
    //
    // Codex-Folgefund (adversarial, 05.09.2026, siebte Runde): denselben
    // Lifecycle-Lock wie `flight_start`/`phpvms_login` nehmen — sonst
    // koennte ein GLEICHZEITIGER `flight_start` (echte Parallelitaet auf
    // einem Mehr-Thread-Runtime, kein Await noetig) den Flug GENAU zwischen
    // dieser Pruefung und dem Leeren von `state.client` installieren.
    let setup_guard = FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)?;
    if state
        .active_flight
        .lock()
        .expect("active_flight lock")
        .is_some()
    {
        return Err(UiError::new(
            "flight_active",
            "Ein Flug ist noch aktiv — bitte zuerst beenden oder abbrechen, \
             bevor du dich abmeldest.",
        ));
    }
    // Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde): jetzt
    // ueber `leere_session_atomar` statt `state.client` isoliert zu leeren
    // — das erhoeht `session_epoch` als FESTEN Teil derselben Operation,
    // synchron und lange bevor irgendeine fehlschlagfaehige Nebenoperation
    // (Keyring, Datei) beginnt. Ein Hintergrund-Task (MQTT-Provisionierung,
    // Profil-Refresh), der seine Start-Epoche mit der aktuellen vergleicht,
    // erkennt diesen Logout garantiert, sobald er selbst wieder zum Zug
    // kommt — unabhaengig davon, ob `delete_api_key` weiter unten
    // gelingt oder fehlschlaegt.
    leere_session_atomar(&state);
    drop(setup_guard);
    // Codex-Folgefund (adversarial, 05.09.2026, elfte Runde): diese
    // In-Memory-Aufraeumung MUSS VOR den beiden fehlschlagfaehigen
    // Keyring-/Datei-Operationen weiter unten passieren, nicht danach.
    // Vorher konnte ein `?` bei `delete_api_key`/`clear_site_config`
    // vorzeitig zurueckkehren, WAEHREND der MQTT-Publisher und die
    // SimBrief-Identitaet des abgemeldeten Piloten noch standen — das
    // Frontend zeigt trotzdem den Login-Screen (siehe `handleLogout`,
    // das nur `flight_active`/`flight_setup_in_progress` blockiert, jeden
    // anderen Fehlercode aber als "ausgeloggt genug" behandelt). Jetzt
    // laeuft diese Aufraeumung IMMER, bevor irgendetwas fehlschlagen kann.
    stoppe_mqtt_publisher(&state).await;
    clear_mqtt_credentials_cache();
    // Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): `simbrief_
    // settings` (auto-gesourct in `cache_pilot` ODER manuell per Settings
    // gesetzt) blieb nach Logout stehen — `maybe_autosource_simbrief_
    // username` fuellt NIE einen bereits gesetzten Wert nach. Ohne diese
    // Leerung haette der naechste Pilot auf derselben Maschine (der KEINEN
    // eigenen Identifier in seinem localStorage hat — App.tsx synct dann
    // gar nichts) stillschweigend den vorigen Piloten weiterbenutzt: dessen
    // aktuellster SimBrief-OFP waere ihm als eigener angeboten worden.
    *state
        .simbrief_settings
        .lock()
        .expect("simbrief_settings lock") = SimBriefSettings::default();
    secrets::delete_api_key(KEYRING_ACCOUNT).map_err(|e| UiError::new("keyring", e.to_string()))?;
    clear_site_config(&app)?;
    tracing::info!("logged out");
    Ok(())
}

/// On app launch: try to restore the previous session from disk + keyring.
/// Returns `None` if no session is stored or stored key is now invalid.
#[tauri::command]
async fn phpvms_load_session(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<LoginResult>, UiError> {
    let Some(_cfg) = read_site_config(&app)? else {
        return Ok(None);
    };
    let Some(api_key) = secrets::load_api_key(KEYRING_ACCOUNT)
        .map_err(|e| UiError::new("keyring", e.to_string()))?
    else {
        return Ok(None);
    };

    // Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): diese
    // Funktion nahm bisher NICHT denselben Lifecycle-Lock wie `phpvms_
    // login`/`phpvms_logout`/`flight_start`/`flight_adopt` — ein
    // gleichzeitiger Login/Flugstart waehrend dieser Restore-Roundtrip
    // (`get_profile`) noch laeuft, haette einen fremden Flug installieren
    // koennen, BEVOR diese Funktion `state.client` unten ueberschreibt.
    let setup_guard = FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)?;

    // Force the locked host even if the persisted config has an old
    // URL from a development build that allowed arbitrary phpVMS
    // instances. See `ALLOWED_PHPVMS_HOST` for the rationale.
    let locked_url = format!("https://{ALLOWED_PHPVMS_HOST}");
    let conn = Connection::new(&locked_url, &api_key)?;
    let client = Client::new(conn)?;
    match client.get_profile().await {
        Ok(profile) => {
            // v0.12.1 (Stream B LE6): pilot-status gate on session
            // restore. A pilot whose GSG account became pending /
            // suspended / on-leave between sessions must not silently
            // get their session (and live-tracking) back. Drop the
            // stored key + MQTT cache and land on the login form — the
            // explicit re-login then shows the status-specific message.
            if let Some(reason) = pilot_state_block_reason(&profile) {
                let _ = secrets::delete_api_key(KEYRING_ACCOUNT);
                let _ = clear_site_config(&app);
                clear_mqtt_credentials_cache();
                // Codex-Folgefund (adversarial, 05.09.2026, vierzehnte
                // Runde): dieser Zweig leerte `state.client` nie explizit
                // und erhoehte `session_epoch` nicht — ein Setup-Hook-
                // Provisionierungs-Task, der noch die ALTE Epoche haelt
                // (von VOR diesem abgelehnten Restore), haette danach
                // trotzdem noch erfolgreich pruefen und installieren
                // koennen. `leere_session_atomar` deckt beides ab.
                leere_session_atomar(&state);
                // Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde):
                // der Setup-Hook feuert seine EIGENE, parallele MQTT-
                // Provisionierung schon vor diesem Restore-Roundtrip — ist
                // sie bereits fertig UND installiert, BEVOR wir hier den
                // Status-Gate-Fehler sehen, bliebe ein laufender Publisher
                // fuer einen Account zurueck, den diese Funktion gerade als
                // nicht aktiv erklaert. Symmetrisch zu `phpvms_logout`
                // stoppen und leeren.
                stoppe_mqtt_publisher(&state).await;
                log_activity_handle(
                    &app,
                    ActivityLevel::Warn,
                    "AeroACARS-Sitzung beendet — GSG-Account nicht aktiv".to_string(),
                    Some(format!(
                        "Status-Gate: {reason}. Bitte neu einloggen oder \
                         die VA-Leitung kontaktieren."
                    )),
                );
                // v0.12.1 (Stream B LE6, QS-P2): return the block reason as
                // a UiError (not a silent `Ok(None)`) so the login form can
                // surface the status-specific message. The pilot still
                // lands on the login form — App.tsx maps this to
                // `loggedOut` + an initial error.
                return Err(UiError::new(
                    reason,
                    format!("pilot state gate (restore): {reason}"),
                ));
            }
            let base_url = client.connection().base_url().to_string();
            setze_session_atomar(&state, client.clone(), profile.pilot_id);
            cache_pilot(&state, &profile);
            // Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde):
            // derselbe synchrone Stopp wie in `phpvms_login` — der
            // Setup-Hook provisioniert spekulativ VOR diesem Restore-
            // Roundtrip (auf Basis eines eventuell veralteten Caches); hat
            // `get_profile` gerade einen ANDEREN Piloten bestaetigt als den
            // gecachten, darf dessen Handle nicht bis zum naechsten
            // Provisionierungs-Tick weiterlaufen.
            {
                let handle_gehoert_nicht_mehr_dieser_epoche = *state
                    .mqtt_owner_epoch
                    .lock()
                    .expect("mqtt_owner_epoch lock")
                    != Some(aktuelle_epoche(&state));
                if handle_gehoert_nicht_mehr_dieser_epoche {
                    stoppe_mqtt_publisher(&state).await;
                }
            }
            // Lifecycle-Lock freigeben BEVOR try_resume_flight ihn selbst
            // nimmt — dasselbe Muster wie in `phpvms_login` (Kommentar
            // dort): haette diese Funktion ihn noch gehalten, waere jeder
            // Resume-Versuch lautlos als "already in progress" uebersprungen
            // worden.
            drop(setup_guard);
            // Auto-start the simulator adapter when we restore an existing session.
            let saved_kind = gespeicherte_sim_wahl_anwenden(&app, &state);
            try_resume_flight(&app, &state).await;
            // Throttle "Session restored" entries: at most once per
            // 60 s. A session-restore is benign noise on rapid Tauri
            // restarts (debug cycles, dev HMR rebuilds) and the
            // activity log was filling up with 4-6 of these in a
            // row from yesterday's testing. Real users restart the
            // app maybe twice a day, so 60 s is plenty.
            use std::sync::atomic::{AtomicI64, Ordering};
            static LAST_SESSION_LOG_S: AtomicI64 = AtomicI64::new(0);
            let now_s = Utc::now().timestamp();
            let prev_s = LAST_SESSION_LOG_S.load(Ordering::Relaxed);
            if now_s - prev_s >= 60 {
                LAST_SESSION_LOG_S.store(now_s, Ordering::Relaxed);
                log_activity(
                    &state,
                    ActivityLevel::Info,
                    format!("Session restored — {}", profile.name),
                    Some(format!("Sim: {:?}", saved_kind)),
                );
            }
            // v0.5.12: belt-and-suspenders — kick off MQTT live-tracking
            // provisioning here too, in case the setup-hook race lost
            // out to a not-yet-ready keyring on this platform. The
            // function is idempotent: if MQTT is already running this
            // is a no-op (well, almost — it may try to overwrite the
            // handle, but the existing one keeps working). If keyring
            // wasn't ready at setup time but is now, this is the catch.
            {
                let app_for_mqtt = app.clone();
                tauri::async_runtime::spawn(async move {
                    init_mqtt_publisher_via_provisioning(app_for_mqtt).await;
                });
            }
            Ok(Some(LoginResult { profile, base_url }))
        }
        // Stored key was rejected — drop it so the next login goes via the form.
        Err(ApiError::Unauthenticated) | Err(ApiError::Forbidden) => {
            // v0.7.17 (B-007): Account-Sperre / inaktiv / revoked API-Key.
            // Vorher: API-Key stumm geloescht, Pilot landete auf Login-Form
            // ohne Erklaerung. Jetzt: klarer Activity-Log-Eintrag damit
            // der Pilot sofort versteht warum er ausgeloggt wurde und was
            // er tun muss (VA-Admin kontaktieren).
            let _ = secrets::delete_api_key(KEYRING_ACCOUNT);
            let _ = clear_site_config(&app);
            clear_mqtt_credentials_cache();
            // Codex-Folgefund (adversarial, 05.09.2026, vierzehnte Runde):
            // dieselbe Luecke wie im Status-Gate-Zweig — ohne Epochen-
            // Bump haette ein Setup-Hook-Provisionierungs-Task mit der
            // ALTEN Epoche trotzdem noch erfolgreich installieren koennen.
            leere_session_atomar(&state);
            // Codex-Folgefund (adversarial, 05.09.2026, elfte Runde):
            // derselbe Wettlauf wie beim Status-Gate-Zweig oben — der
            // Setup-Hook provisioniert parallel, unabhaengig davon ob
            // dieser Restore-Roundtrip am Ende mit einem Auth-Fehler
            // endet. Ohne diesen Stopp bliebe ein zwischenzeitlich
            // installierter Publisher fuer einen Account zurueck, den
            // diese Funktion hier gerade als nicht mehr gueltig erklaert.
            stoppe_mqtt_publisher(&state).await;
            log_activity_handle(
                &app,
                ActivityLevel::Warn,
                "phpVMS hat dich abgemeldet — Account gesperrt, inaktiv oder API-Key revoked",
                Some(
                    "Bitte VA-Admin kontaktieren. Nach Freigabe einfach neu einloggen.".to_string(),
                ),
            );
            Ok(None)
        }
        Err(other) => Err(other.into()),
    }
}

#[cfg(test)]
/// Codex-Folgefund (adversarial, 05.09.2026, sechste Runde): die
/// Eigentuemer-Bindung aus den vorigen Runden schuetzt nur, was AUF DER
/// PLATTE liegt (Resume-Ablage, PIREP-Queue) — nicht die LEBENDEN
/// API-Aufrufe waehrend eines laufenden Fluges. `phpvms_logout`/
/// `phpvms_login` bleiben Tauri-gebundene async-Kommandos, nicht isoliert
/// testbar (wie der Rest dieser Infrastruktur) — Quelltext-Wächter direkt
/// auf die Aufrufreihenfolge.
#[path = "tests_lib/flug_aktiv_verhindert_konto_wechsel_wiring_tests.rs"]
mod flug_aktiv_verhindert_konto_wechsel_wiring_tests;
