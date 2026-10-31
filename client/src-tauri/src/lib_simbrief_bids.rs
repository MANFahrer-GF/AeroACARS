/// Pull the active client out of state, or fail with `not_logged_in`.
fn current_client(state: &tauri::State<'_, AppState>) -> Result<Client, UiError> {
    let guard = state.client.lock().expect("client mutex");
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| UiError::new("not_logged_in", "no active session"))
}

// v0.7.13: `get_minimize_to_tray` Tauri-Command entfernt — Frontend
// liest localStorage selber + setzt via `set_minimize_to_tray`. Es gab
// nie einen Caller fuer den Read-Path. Audit Q4-2026-05.

/// Flip the minimize-to-tray flag. The React Settings panel calls
/// this whenever the user toggles the checkbox AND on every mount
/// (so the persisted localStorage value flows back into the backend
/// after a restart). Cheap atomic flip — the actual close-handler
/// just reads the flag at the moment a CloseRequested fires.
#[tauri::command]
fn set_minimize_to_tray(app: AppHandle, state: tauri::State<'_, AppState>, enabled: bool) -> Result<(), UiError> {
    // v3 (Etappe 5): die Huelle haelt den Wert, angewandt beim Start.
    huelle::einstellung_setzen(&app, |e| e.minimize_to_tray = enabled);
    let was = state
        .minimize_to_tray_enabled
        .swap(enabled, std::sync::atomic::Ordering::Relaxed);
    if was != enabled {
        tracing::info!(enabled, "minimize_to_tray toggled");
    }
    Ok(())
}

// v0.9.0 (#GlitchTip): Pilot-Consent fuer anonyme Fehler-Telemetrie an
// GlitchTip. Default = aus (Opt-In). Frontend persisted in localStorage
// `aeroacars.errorReporting.enabled` und ruft diesen Command auf jedem
// Mount + jedem Toggle. Gleiche Pattern wie minimize_to_tray.
//
// Bei `false` werden vor dem before_send-Hook ALLE Events verworfen —
// auch wenn anderer Code sentry::capture_message() aufruft.
//
// Spec: docs/spec/v0.9.0-glitchtip-self-hosted.md (LE4) +
//       docs/spec/v0.9.0-telemetry-contract.md Sektion 9 (DSGVO).
#[tauri::command]
fn error_reporting_set_consent(app: AppHandle, enabled: bool) -> Result<(), UiError> {
    sentry_init::set_consent(enabled);
    // v3 (Etappe 5): die Huelle haelt die Einwilligung, angewandt beim Start.
    huelle::einstellung_setzen(&app, |e| e.fehlerberichte = enabled);
    Ok(())
}

// v0.10.0 hotfix: der `error_reporting_send_test_event`-Tauri-Command
// (war für eine v0.9.3 GlitchTip-Pipeline-DevTest-Funktion gedacht) ist
// hier rausgeflogen weil seine Implementation `sentry_init::send_test_event`
// nie ans Repo committed wurde (nur lokal im Working-Tree existiert) —
// CI-Build failed beim aeroacars-app Compile (cannot find function in
// module). Falls wir den Test-Button später wirklich shippen wollen,
// MUSS er als atomarer Commit (Rust-Fn + Tauri-Command + invoke_handler-
// Registrierung + Frontend-Aufruf) reingebracht werden, nicht halbgar.

// v0.7.13 Discord-Webhook-File-Commands entfernt + v0.7.14 Migration:
// die alte Pilot-Local-Datei `<app_data_dir>/discord-webhook.txt` wird beim
// App-Start geloescht (siehe `migrate_remove_discord_webhook_file` weiter
// unten + Call im Setup-Hook). Discord-Posts macht ab v0.7.14 ausschliesslich
// der Recorder auf live.kant.ovh, konfigurierbar via Webapp-Admin-Settings.

/// v0.7.14: Migration — alte Pilot-Local-Webhook-Datei (`<app_data_dir>/
/// discord-webhook.txt`) aus v0.7.13 loeschen. Der Pilot-Client postet ab
/// v0.7.14 nichts mehr in Discord — der Recorder macht das zentral.
fn migrate_remove_discord_webhook_file(app: &AppHandle) {
    let Ok(base) = app.path().app_data_dir() else {
        return;
    };
    let path = base.join("discord-webhook.txt");
    if path.exists() {
        match std::fs::remove_file(&path) {
            Ok(()) => tracing::info!(
                "v0.7.14 migration: discord-webhook.txt geloescht (Discord-Posts macht jetzt der VPS-Recorder)"
            ),
            Err(e) => tracing::warn!(error = %e, "v0.7.14 migration: discord-webhook.txt loeschen failed"),
        }
    }
}

// v0.7.13: `get_simbrief_settings` Tauri-Command entfernt — Frontend
// haelt die Settings selber in React-State + persistiert via
// `set_simbrief_settings`. Kein Read-Path-Caller. Audit Q4-2026-05.

/// v0.7.8: Set SimBrief-Identifier-Settings. Wird vom Frontend
/// SettingsPanel + App.tsx Login-Mount aufgerufen. Empty-Strings werden
/// als None behandelt (Pilot loescht das Feld → kein Identifier).
/// Spec §4.
#[tauri::command]
fn set_simbrief_settings(
    state: tauri::State<'_, AppState>,
    username: Option<String>,
    user_id: Option<String>,
) -> Result<(), UiError> {
    let username = username.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    });
    let user_id = user_id.and_then(|s| {
        // Numerische User-IDs — Frontend filtert schon non-digit raus,
        // aber defensiv hier auch nochmal trimmen.
        let t = s.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    });
    let mut guard = state
        .simbrief_settings
        .lock()
        .expect("simbrief_settings lock");
    let was_set = guard.username.is_some() || guard.user_id.is_some();
    let now_set = username.is_some() || user_id.is_some();
    *guard = SimBriefSettings { username, user_id };
    if was_set != now_set {
        tracing::info!(
            now_set,
            has_username = guard.username.is_some(),
            has_user_id = guard.user_id.is_some(),
            "simbrief_settings updated",
        );
    }
    Ok(())
}

/// v0.7.8: "Pruefen"-Button in Settings — testet ob der konfigurierte
/// SimBrief-Identifier (Username/User-ID) ein valides Profil hat.
/// Frontend ruft das mit den AKTUELLEN Werten aus den Inputs auf (nicht
/// aus dem AppState), damit der Pilot direkt nach Tippen testen kann
/// ohne erst "Save" zu druecken.
/// Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md §4.4.
#[tauri::command]
async fn verify_simbrief_identifier(
    state: tauri::State<'_, AppState>,
    username: Option<String>,
    user_id: Option<String>,
) -> Result<VerifySimBriefResult, UiError> {
    let username = username.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    });
    let user_id = user_id.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    });
    if username.is_none() && user_id.is_none() {
        return Err(UiError::new(
            "no_identifier",
            "Mindestens Username oder User-ID muss gesetzt sein.",
        ));
    }

    // Wir brauchen einen Client mit HTTP-Setup. Der phpVMS-Client
    // taugt nicht — wir wollen direkt zu simbrief.com. Aber: der
    // api_client::Client haengt am phpVMS-Endpoint. Workaround:
    // wir setzen einen Dummy-Connection auf, da fetch_simbrief_direct
    // intern nur self.http nutzt (= reqwest::Client) der unabhaengig
    // vom phpVMS-Endpoint ist.
    let client = current_client(&state)?;

    match client
        .fetch_simbrief_direct(user_id.as_deref(), username.as_deref())
        .await
    {
        Ok(ofp) => Ok(VerifySimBriefResult {
            ok: true,
            origin: Some(ofp.ofp_origin_icao),
            destination: Some(ofp.ofp_destination_icao),
            callsign: Some(ofp.ofp_flight_number),
            error_code: None,
        }),
        Err(SimBriefDirectError::UserNotFound) => Ok(VerifySimBriefResult {
            ok: false,
            origin: None,
            destination: None,
            callsign: None,
            error_code: Some("user_not_found".to_string()),
        }),
        Err(SimBriefDirectError::Unavailable) => Ok(VerifySimBriefResult {
            ok: false,
            origin: None,
            destination: None,
            callsign: None,
            error_code: Some("unavailable".to_string()),
        }),
        Err(SimBriefDirectError::Network) => Ok(VerifySimBriefResult {
            ok: false,
            origin: None,
            destination: None,
            callsign: None,
            error_code: Some("network".to_string()),
        }),
        Err(SimBriefDirectError::ParseFailed) => Ok(VerifySimBriefResult {
            ok: false,
            origin: None,
            destination: None,
            callsign: None,
            error_code: Some("parse_failed".to_string()),
        }),
        Err(SimBriefDirectError::NoIdentifier) => Err(UiError::new(
            "no_identifier",
            "no SimBrief identifier provided",
        )),
    }
}

/// v0.7.8: Result-DTO fuer den "Pruefen"-Button. Bei Erfolg sieht der
/// Pilot Origin/Destination/Callsign des latest OFP — das ist gleichzeitig
/// eine Bestaetigung dass der Username/User-ID korrekt konfiguriert ist
/// und der OFP zum aktuellen Flug-Plan passt.
#[derive(Debug, Clone, Serialize)]
struct VerifySimBriefResult {
    /// true wenn SimBrief das Profil findet + OFP parsable.
    ok: bool,
    /// Origin-ICAO des latest OFP (nur bei ok=true).
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
    /// Destination-ICAO (nur bei ok=true).
    #[serde(skip_serializing_if = "Option::is_none")]
    destination: Option<String>,
    /// Callsign aus dem OFP (nur bei ok=true).
    #[serde(skip_serializing_if = "Option::is_none")]
    callsign: Option<String>,
    /// Bei ok=false: spezifischer Error-Code ("user_not_found" /
    /// "unavailable" / "network" / "parse_failed") fuer i18n-Lookup.
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
}

/// Find the N nearest airports to the active flight's CURRENT
/// position. Powers the manual-divert modal in the cockpit so the
/// pilot can pick from a list of nearby fields (or type a custom
/// ICAO if their actual landing strip isn't in our local DB).
///
/// Returns an empty vec when no flight is active or no airport is
/// within `arrival::NEAREST_SEARCH_RADIUS_NM` of the current position.
#[tauri::command(async)]
fn divert_nearest_airports(
    state: tauri::State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<runway::NearestAirport>, UiError> {
    let limit = limit.unwrap_or(5).clamp(1, 20);
    let (lat, lon) = {
        let guard = state.active_flight.lock().expect("active_flight lock");
        let Some(flight) = guard.as_ref() else {
            return Ok(Vec::new());
        };
        let stats = flight.stats.lock().expect("flight stats");
        // v0.19.3: search from the TOUCHDOWN point, not the current position.
        // This is the list the pilot picks his actual landing field from, and
        // the field he landed on is the one under the wheels at touchdown — not
        // necessarily the one nearest to wherever he has taxied since. At an
        // airport cluster (EDDB/EDDT, KJFK/KLGA) a long taxi can put a
        // *different* field at the top of the list. The doc comments here and
        // in `DivertBanner.tsx` always claimed touchdown coords; the code used
        // `last_lat/last_lon`. The comments were right and the code was wrong.
        //
        // Falls back to the last known position when no touchdown was recorded
        // (aircraft never landed cleanly — the pilot still needs *some* list).
        // The pair is taken from ONE source: mixing a touchdown latitude with a
        // current longitude would name a field in the middle of nowhere.
        // Touchdown point first (that is the field he landed on), then the
        // resume-proof last known position — NOT `last_lat/lon`, which a resume
        // nulls: a rotorcraft or a go-around-then-landing flight resumed after a
        // sim loss would otherwise get an EMPTY list and be unable to name the
        // field it actually landed at (QS round 8).
        match (stats.landing_lat, stats.landing_lon) {
            (Some(la), Some(lo)) => (la, lo),
            _ => match aircraft_position_for_gates(&stats) {
                Some(p) => p,
                None => return Ok(Vec::new()),
            },
        }
    };
    // ICAO-only: whatever the pilot picks here becomes the PIREP's arrival
    // airport in phpVMS, which cannot resolve an FAA local code like "48FA".
    Ok(runway::find_nearest_icao_airports(
        lat,
        lon,
        crate::grenzwerte::grenzwerte().ankunft.nearest_search_radius_nm.wert * 1852.0,
        limit,
    ))
}

/// Re-fetch the pilot profile from phpVMS. Returns the fresh profile
/// (or None if not logged in / fetch failed). Mostly used by the UI to
/// pick up `curr_airport` after a PIREP files — phpVMS updates the
/// pilot's current airport server-side once a flight is accepted, but
/// our cached LoginResult never sees it without an explicit re-fetch.
#[tauri::command]
async fn phpvms_refresh_profile(
    state: tauri::State<'_, AppState>,
) -> Result<Option<Profile>, UiError> {
    // Codex-Folgefund (adversarial, 05.09.2026, zwoelfte Runde): `client`
    // wurde bisher isoliert per `current_client` erfasst, OHNE die dazu
    // gehoerende Piloten-ID — nach dem echten Server-Roundtrip
    // (`get_profile().await`) schrieb `cache_pilot` das Ergebnis
    // ungeprueft zurueck. Meldet sich waehrend dieses Roundtrips ein
    // ANDERER Pilot an (ausdruecklich erlaubt ohne vorheriges Logout),
    // haette die verspaetete Antwort fuer Pilot A dessen `cached_pilot`/
    // `cached_pilot_callsign`/SimBrief-Auto-Source ueber Pilot Bs frisch
    // angemeldete Sitzung geschrieben.
    //
    // Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde): ein
    // Vergleich der Piloten-ID allein reicht nicht — `authenticated_
    // pilot_id` wird bei Logout bewusst NICHT geleert, ein Logout
    // WAEHREND dieses Roundtrips haette den Vergleich also trotzdem
    // bestehen lassen, obwohl `state.client` schon `None` war. Jetzt wird
    // die EPOCHE verglichen, die bei JEDER Sitzungsaenderung (auch
    // Logout) steigt — das autoritative Signal, nicht mehr ein einzelnes
    // Feld.
    let Some((client, _angefragte_pilot_id, epoche_bei_start)) = aktuelle_session_atomar(&state)
    else {
        return Ok(None);
    };
    match client.get_profile().await {
        Ok(p) => {
            if aktuelle_epoche(&state) != epoche_bei_start {
                tracing::warn!(
                    "profile refresh: Sitzung aenderte sich waehrend des Roundtrips — \
                     verspaetetes Profil wird verworfen, nicht zurueckgeschrieben"
                );
                return Ok(None);
            }
            cache_pilot(&state, &p);
            Ok(Some(p))
        }
        Err(e) => {
            tracing::warn!(error = %e, "profile refresh failed");
            Ok(None)
        }
    }
}

/// Single GitHub release record — the subset we render in the
/// in-app "What's new" modal. Source: anonymous GET against
/// <https://api.github.com/repos/MANFahrer-GF/AeroACARS/releases/tags/v{version}>
/// (GitHub allows 60 anonymous req/h per IP, way more than we'd ever
/// hit since we cache the result for the modal's lifetime).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseNotes {
    /// Release name (typically "AeroACARS v0.1.23"). Used as the
    /// modal header.
    pub name: String,
    /// Tag name ("v0.1.23"). Used as a stable identifier so the
    /// frontend can store "last-seen" in localStorage.
    pub tag_name: String,
    /// Markdown body — bilingual format with `## 🇩🇪 Deutsch` and
    /// `## 🇬🇧 English` section markers. Frontend splits and renders
    /// just the matching language.
    pub body: String,
    /// ISO-8601 publish timestamp.
    pub published_at: String,
    /// Direct link to the GitHub release page — used as a fallback
    /// "View on GitHub" button in the modal.
    pub html_url: String,
}

/// Fetch the GitHub release notes for a specific version tag. Used by
/// the in-app "What's new" modal that fires once per version after
/// the auto-updater swaps the binary in.
///
/// Returns `Err(...)` for network failures or non-200 responses;
/// frontend falls back to "open on GitHub" in those cases. We
/// deliberately don't authenticate the request — at one fetch per
/// app-start the 60-req/h anonymous limit is irrelevant, and pulling
/// secrets into a release-notes feature would be silly.
#[tauri::command]
async fn fetch_release_notes(version: String) -> Result<ReleaseNotes, UiError> {
    let tag = if version.starts_with('v') {
        version.clone()
    } else {
        format!("v{version}")
    };
    let url = format!("https://api.github.com/repos/MANFahrer-GF/AeroACARS/releases/tags/{tag}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| UiError::new("network", e.to_string()))?;
    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| UiError::new("network", e.to_string()))?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(UiError::new(
            "not_found",
            format!("no release with tag {tag}"),
        ));
    }
    if !status.is_success() {
        return Err(UiError::new(
            "github_error",
            format!("GitHub returned HTTP {}", status.as_u16()),
        ));
    }
    response
        .json::<ReleaseNotes>()
        .await
        .map_err(|e| UiError::new("parse", e.to_string()))
}

/// Feste Beschriftung des Bids-Fehlers im Aktivitätsprotokoll. Als Konstante,
/// weil `resolve_bids_activity_error` denselben Text wiederfinden muss.
const BIDS_FAILED_MESSAGE: &str = "Bids konnten nicht geladen werden";

/// Wartezeiten vor dem 2. und 3. Versuch. Kurz genug, dass der Pilot nichts
/// merkt (in Summe 1,6 s bei zwei Fehlschlägen), lang genug, damit ein
/// belegter Verbindungspool sich leeren kann.
const BIDS_RETRY_DELAYS_MS: [u64; 2] = [400, 1200];

/// Ab hier wird kein neuer Versuch mehr begonnen.
///
/// QS-Befund 18.08.2026: ohne diese Schranke konnte der Befehl im schlechtesten
/// Fall ~16,6 s haengen — dreimal in den 5-Sekunden-Verbindungs-Timeout plus
/// die Wartezeiten. Fuer den Piloten waere ein 17 s haengender Briefing-Reiter
/// unangenehmer als die rote Karte, die dieser Umbau vermeiden soll.
///
/// Drei Sekunden trennen genau die beiden Faelle: ein schneller Fehlschlag
/// (Verbindung abgewiesen, kurzer Aussetzer) laesst Zeit fuer beide
/// Wiederholungen; ein echter Timeout hat das Zeitbudget schon aufgebraucht,
/// und ein zweiter Versuch wuerde nur dasselbe noch einmal abwarten. Damit
/// bleibt der schlechteste Fall bei ~5 s — also unveraendert gegenueber dem
/// Zustand VOR dieser Aenderung.
const BIDS_RETRY_BUDGET_MS: u128 = 3_000;

/// Lohnt ein zweiter Versuch? Nur bei Fehlern, die von selbst vorbeigehen.
///
/// Ein 401/403/404 oder eine kaputte Antwort wird beim Wiederholen exakt
/// genauso ausfallen — da ist die sofortige Meldung die richtige Antwort, und
/// zwar mit der technischen Zeile, die der Pilot weitergeben kann. Ein
/// Netzfehler oder ein 5xx dagegen ist typischerweise ein Aussetzer von
/// Sekundenbruchteilen.
fn bids_error_is_transient(err: &ApiError) -> bool {
    match err {
        ApiError::Network(_) => true,
        ApiError::Server { status, .. } => *status >= 500,
        _ => false,
    }
}

/// Entwertet einen früheren Bids-Fehler, sobald der Abruf wieder klappt.
///
/// **Das ist der Kern des Feldbefunds vom 18.08.2026.** Ein einziger verlorener
/// Abruf schrieb eine rote Fehlerzeile ins Protokoll — und die blieb dort
/// stehen. Karte und Overlay rendern dieses Protokoll, also starrte den Piloten
/// stundenlang ein Fehler an, der sich vier Sekunden später von selbst erledigt
/// hatte. Der Eintrag wird nicht gelöscht (die Störung war echt), sondern auf
/// `Info` heruntergestuft und als erledigt gekennzeichnet.
fn resolve_bids_activity_error(state: &tauri::State<'_, AppState>) {
    let mut log = state.activity_log.lock().expect("activity_log lock");
    entwerte_bids_fehler(&mut log);
}

/// Der eigentliche Handgriff, getrennt von der Sperre — so ist er ohne
/// Tauri-`State` prüfbar.
fn entwerte_bids_fehler(log: &mut VecDeque<ActivityEntry>) {
    for entry in log.iter_mut() {
        if matches!(entry.level, ActivityLevel::Error) && entry.message == BIDS_FAILED_MESSAGE {
            entry.level = ActivityLevel::Info;
            entry.detail = Some(match entry.detail.take() {
                Some(d) => format!("{d} — inzwischen wieder erreichbar"),
                None => "inzwischen wieder erreichbar".to_string(),
            });
        }
    }
}

/// `GET /api/user/bids` — the pilot's open bids.
///
/// Wiederholt vorübergehende Fehler still, bevor der Pilot etwas davon sieht:
/// die Liste wird im Leerlauf im Sekundentakt abgefragt, ein einzelnes
/// verlorenes Paket ist über einen langen Flug praktisch sicher und sagt
/// nichts aus.
#[tauri::command]
async fn phpvms_get_bids(state: tauri::State<'_, AppState>) -> Result<Vec<Bid>, UiError> {
    // Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde): dieser
    // Retry-Loop kann sich ueber mehrere Sekunden ziehen — ein Kontowechsel
    // WAEHREND eines verzoegerten, am Ende erfolgreichen Versuchs von A
    // haette `resolve_bids_activity_error` einen inzwischen von Pilot B
    // frisch erzeugten Fehler-Eintrag faelschlich als "wieder erreichbar"
    // markiert.
    //
    // Codex-Folgefund (adversarial, 05.09.2026, vierzehnte Runde): `client`
    // und die Epoche wurden hier zunaechst GETRENNT gelesen
    // (`current_client` dann `aktuelle_epoche`) — ein Kontowechsel GENAU
    // dazwischen haette Pilot As Client mit Pilot Bs (bereits aktueller)
    // Epoche gepaart, wodurch der spaetere Vergleich faelschlich "unveraendert"
    // gemeldet haette. Jetzt beides gemeinsam ueber `aktuelle_session_atomar`.
    let Some((client, _pilot_id, epoche_bei_start)) = aktuelle_session_atomar(&state) else {
        return Err(UiError::new("not_logged_in", "no active session"));
    };

    let letzter: ApiError;
    let mut versuch = 0usize;
    let beginn = std::time::Instant::now();
    loop {
        match client.get_bids().await {
            Ok(bids) => {
                if aktuelle_epoche(&state) == epoche_bei_start {
                    resolve_bids_activity_error(&state);
                }
                return Ok(bids);
            }
            Err(e) => {
                if versuch < BIDS_RETRY_DELAYS_MS.len()
                    && beginn.elapsed().as_millis() < BIDS_RETRY_BUDGET_MS
                    && bids_error_is_transient(&e)
                {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        BIDS_RETRY_DELAYS_MS[versuch],
                    ))
                    .await;
                    versuch += 1;
                    continue;
                }
                letzter = e;
                break;
            }
        }
    }

    // Log to the activity feed so the pilot can paste the
    // technical detail without having to dig through console
    // logs. Particularly useful for the BadResponse case where
    // a single malformed bid breaks the whole list — we show
    // exactly which field/path tripped the decoder.
    let detail = if versuch > 0 {
        format!("{letzter} (nach {} Versuchen)", versuch + 1)
    } else {
        format!("{letzter}")
    };
    log_activity(
        &state,
        ActivityLevel::Error,
        BIDS_FAILED_MESSAGE,
        Some(detail),
    );
    Err(letzter.into())
}

/// v0.12.12-dev: `GET /api/news` — VA-News-Posts. Reuse den vorhandenen
/// phpVMS-Client (gleicher API-Key, gleiches Auth-Handling). Per-page
/// hardcoded auf 20 — der News-Tab zeigt nur Page 1 ohne Pagination.
#[tauri::command]
async fn news_fetch(state: tauri::State<'_, AppState>) -> Result<Vec<NewsItem>, UiError> {
    let client = current_client(&state)?;
    client.get_news(20).await.map_err(Into::into)
}

/// SimBrief OFP-Preview (v0.3.0) — fetcht das letzte SimBrief XML
/// für eine OFP-ID und liefert die wesentlichen Plan-Werte. Wird vom
/// v0.3.2: Refresh den SimBrief-OFP für den GERADE LAUFENDEN Flug.
/// Real-Pilot-Workflow: Pilot regeneriert auf simbrief.com einen neuen
/// OFP nachdem AeroACARS schon den alten beim Flight-Start gefangen
/// hat — z.B. weil sich Pax-Bestand, Cargo, oder Reserve-Strategie
/// geändert haben. Vorher musste der Pilot „Discard flight" und neu
/// starten; jetzt zieht der Cockpit-Refresh-Button den frischen OFP
/// und überschreibt die `planned_*`-Felder im aktiven Flug.
///
/// Wir hängen am Bid (statt nur am gespeicherten ofp_id), weil der
/// SimBrief-Sync auf phpVMS-Seite die Relation aktualisiert wenn
/// der Pilot regeneriert — der Bid trägt dann die neue OFP-ID.
/// v0.7.9: Read aktuelles Callsign-Mismatch-Warning. Frontend pollt das
/// nach jedem `flight_refresh_simbrief` um zu prueefen ob das Warning-
/// Banner gezeigt werden soll.
#[tauri::command(async)]
fn ofp_callsign_warning_get(state: tauri::State<AppState>) -> Option<OfpCallsignWarning> {
    state
        .ofp_callsign_warning
        .lock()
        .expect("ofp_callsign_warning lock")
        .clone()
}

// v0.7.13: `ofp_callsign_warning_dismiss` Tauri-Command entfernt — die
// X-Button-Dismiss-Logik im Frontend setzt das Banner ueber lokalen
// React-State auf null statt einen Backend-Roundtrip zu machen. Audit
// Q4-2026-05.

/// v0.7.10: Pre-Flight-Preview eines SimBrief-OFP fuer einen bestimmten Bid.
/// Holt OFP direkt von simbrief.com (= SimBrief-direct, gleicher Pfad wie
/// `flight_refresh_simbrief` aber ohne `active_flight`-Anker — wir nutzen
/// den Bid selbst fuer DEP/ARR/Callsign-Match.
///
/// Damit kann der Pilot VOR `IFR Start` schon die frischen OFP-Werte
/// sehen statt nur den phpVMS-Snapshot. User-Wunsch:
/// "Ich möchte die Daten auch schon vor dem Klick auf Start sehen".
///
/// Match-Verifikation wie v0.7.9: DEP+ARR hard, Callsign soft (nur Warning).
#[tauri::command]
async fn bid_simbrief_preview(
    bid_id: i64,
    state: tauri::State<'_, AppState>,
) -> Result<BidSimBriefPreview, UiError> {
    use api_client::SimBriefDirectError;

    let client = current_client(&state)?;
    let simbrief_settings = state
        .simbrief_settings
        .lock()
        .expect("simbrief_settings lock")
        .clone();

    if !has_simbrief_identifier(&simbrief_settings) {
        return Err(UiError::new(
            "no_simbrief_settings",
            "Bitte erst Settings → SimBrief mit Username oder User-ID konfigurieren.",
        ));
    }

    // Hole Bid-Liste fuer DEP/ARR/Callsign-Match-Anker. Wir koennten den
    // Bid auch aus dem Frontend uebergeben aber das ist zerbrechlicher
    // (Frontend-State-Drift) — kanonisch ist phpVMS.
    let bids = client.get_bids().await.map_err(|e| {
        UiError::new(
            "bids_fetch_failed",
            format!("Bid-Liste konnte nicht geholt werden: {e}"),
        )
    })?;
    let bid = bids
        .iter()
        .find(|b| b.id == bid_id)
        .ok_or_else(|| UiError::new("bid_not_found", "Bid in der phpVMS-Liste nicht gefunden"))?;

    let active_airline_icao = bid
        .flight
        .airline
        .as_ref()
        .map(|a| a.icao.clone())
        .unwrap_or_default();
    let active_flight_number = bid.flight.flight_number.clone();
    let active_bid_callsign = bid.flight.callsign.clone();
    let pilot_callsign = state
        .cached_pilot_callsign
        .lock()
        .expect("cached_pilot_callsign lock")
        .clone();
    let active_dpt = bid.flight.dpt_airport_id.clone();
    let active_arr = bid.flight.arr_airport_id.clone();

    // Direct-Fetch.
    let ofp = match client
        .fetch_simbrief_direct(
            simbrief_settings.user_id.as_deref(),
            simbrief_settings.username.as_deref(),
        )
        .await
    {
        Ok(ofp) => ofp,
        Err(SimBriefDirectError::NoIdentifier) => {
            return Err(UiError::new(
                "no_simbrief_settings",
                "kein SimBrief-Identifier",
            ));
        }
        Err(SimBriefDirectError::UserNotFound) => {
            return Err(UiError::new(
                "simbrief_user_not_found",
                "SimBrief-User nicht gefunden",
            ));
        }
        Err(SimBriefDirectError::Unavailable | SimBriefDirectError::Network) => {
            return Err(UiError::new(
                "simbrief_direct_failed",
                "SimBrief.com nicht erreichbar",
            ));
        }
        Err(SimBriefDirectError::ParseFailed) => {
            return Err(UiError::new(
                "simbrief_direct_failed",
                "SimBrief-OFP konnte nicht geparst werden",
            ));
        }
    };

    // Match-Verify v0.7.9: DEP+ARR hard, Callsign soft.
    let dpt_ok = ofp
        .ofp_origin_icao
        .trim()
        .eq_ignore_ascii_case(active_dpt.trim());
    let arr_ok = ofp
        .ofp_destination_icao
        .trim()
        .eq_ignore_ascii_case(active_arr.trim());

    if !dpt_ok || !arr_ok {
        let candidates = build_candidate_callsigns(
            &active_airline_icao,
            &active_flight_number,
            active_bid_callsign.as_deref(),
            pilot_callsign.as_deref(),
        );
        let active_callsigns_display = if candidates.is_empty() {
            format!("{}{}", active_airline_icao, active_flight_number)
        } else {
            candidates.join(" / ")
        };
        let details = serde_json::json!({
            "active_callsigns": active_callsigns_display,
            "active_dpt": active_dpt,
            "active_arr": active_arr,
            "sb_callsign": ofp.ofp_flight_number,
            "sb_origin": ofp.ofp_origin_icao,
            "sb_dest": ofp.ofp_destination_icao,
        });
        return Err(UiError::new(
            "ofp_does_not_match_active_flight",
            details.to_string(),
        ));
    }

    let callsign_ok = ofp_matches_active_flight(
        &ofp.ofp_origin_icao,
        &ofp.ofp_destination_icao,
        &ofp.ofp_flight_number,
        &active_dpt,
        &active_arr,
        &active_airline_icao,
        &active_flight_number,
        active_bid_callsign.as_deref(),
        pilot_callsign.as_deref(),
    );

    let callsign_warning = if callsign_ok {
        None
    } else {
        let candidates = build_candidate_callsigns(
            &active_airline_icao,
            &active_flight_number,
            active_bid_callsign.as_deref(),
            pilot_callsign.as_deref(),
        );
        let active_callsigns_display = if candidates.is_empty() {
            format!("{}{}", active_airline_icao, active_flight_number)
        } else {
            candidates.join(" / ")
        };
        Some(CallsignWarningDetails {
            sb_callsign: ofp.ofp_flight_number.clone(),
            active_callsigns: active_callsigns_display,
        })
    };

    hoppie::vorschau_merken(flight_atc_callsign(ofp_atc_callsign(&ofp.ofp_flight_number).as_deref(), &active_airline_icao, &active_flight_number), &active_dpt, &active_arr, ofp.aircraft_icao.as_deref().unwrap_or_default());
    Ok(BidSimBriefPreview {
        request_id: ofp.request_id.clone(),
        planned_block_fuel_kg: ofp.planned_block_fuel_kg,
        planned_burn_kg: ofp.planned_burn_kg,
        planned_reserve_kg: ofp.planned_reserve_kg,
        planned_zfw_kg: ofp.planned_zfw_kg,
        planned_tow_kg: ofp.planned_tow_kg,
        planned_ldw_kg: ofp.planned_ldw_kg,
        alternate: ofp.alternate.clone(),
        ofp_flight_number: ofp.ofp_flight_number.clone(),
        ofp_origin_icao: ofp.ofp_origin_icao.clone(),
        ofp_destination_icao: ofp.ofp_destination_icao.clone(),
        pax_count: ofp.pax_count,
        cargo_kg: ofp.cargo_kg,
        freight_kg: ofp.freight_kg,
        aircraft_icao: ofp.aircraft_icao.clone(),
        max_passengers: ofp.max_passengers,
        sched_out_epoch: ofp.sched_out_epoch,
        sched_in_epoch: ofp.sched_in_epoch,
        planned_taxi_kg: ofp.planned_taxi_kg,
        planned_contingency_kg: ofp.planned_contingency_kg,
        planned_alternate_burn_kg: ofp.planned_alternate_burn_kg,
        planned_extra_kg: ofp.planned_extra_kg,
        planned_oew_kg: ofp.planned_oew_kg,
        planned_payload_kg: ofp.planned_payload_kg,
        planned_pax_weight_kg: ofp.planned_pax_weight_kg,
        planned_baggage_kg: ofp.planned_baggage_kg,
        // Briefing-2a: OPERATIONELLER MTOW-Grenzwert fuer diesen Flug
        // (z.B. bahnlaengen-/temperaturlimitiert) — enger als der
        // strukturelle Grenzwert aus phpVMS' `aircraft.mtow`. Pilot-
        // Feedback 2026-08-03: der strukturelle Wert zeigte einen viel
        // groesseren Puffer als real vorhanden.
        planned_max_tow_kg: ofp.max_tow_kg,
        callsign_warning,
    })
}

/// v0.7.10: Pre-Flight-SimBrief-Preview-Response. Schlankes DTO statt
/// `SimBriefRefreshResult` — wir haben keinen `previous_ofp_id` zum Vergleich
/// (= Pre-Flight, kein State).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidSimBriefPreview {
    pub request_id: String,
    pub planned_block_fuel_kg: f32,
    pub planned_burn_kg: f32,
    pub planned_reserve_kg: f32,
    pub planned_zfw_kg: f32,
    pub planned_tow_kg: f32,
    pub planned_ldw_kg: f32,
    pub alternate: Option<String>,
    pub ofp_flight_number: String,
    pub ofp_origin_icao: String,
    pub ofp_destination_icao: String,
    /// v0.7.12: Pax + Cargo aus dem SimBrief-OFP. Damit die Bid-Card
    /// auch ohne phpVMS-Bid-Pointer-Cache die Pax/Cargo-Chips anzeigt
    /// (siehe BidsList.tsx `paxCount`/`cargoKg`).
    pub pax_count: i32,
    /// Gepaeck + Fracht zusammen (SimBrief `<weights><cargo>`).
    pub cargo_kg: f32,
    /// Reine Fracht ohne Pax-Gepaeck — das zeigt die Bid-Card als "Cargo".
    pub freight_kg: f32,
    /// Briefing-2a: Aircraft-ICAO-Typ aus dem SimBrief-OFP — Fallback wenn
    /// der Bid (noch) keinen phpVMS-Bid-Pointer-Subfleet hat.
    pub aircraft_icao: Option<String>,
    /// Sitzplatz-Total der geplanten Config — Grundlage fuer "PAX von N".
    pub max_passengers: Option<i32>,
    /// Geplante Blockzeit ab/an (Unix-Epoch-Sekunden, UTC) — Fallback-STD/STA
    /// wenn phpVMS' `flight.dpt_time`/`arr_time` fehlen.
    pub sched_out_epoch: Option<i64>,
    pub sched_in_epoch: Option<i64>,
    /// Restliche Fuel-Kategorien — zusammen mit Trip + Reserve ergeben sie
    /// exakt Block (Pilot-Feedback 2026-08-02: Trip+Reserve allein liess
    /// sich nicht zu Block nachrechnen).
    pub planned_taxi_kg: f32,
    pub planned_contingency_kg: f32,
    pub planned_alternate_burn_kg: f32,
    pub planned_extra_kg: f32,
    /// Operating Empty Weight + Payload — zusammen ergeben sie ZFW.
    pub planned_oew_kg: f32,
    pub planned_payload_kg: f32,
    /// Pax-Gesamtgewicht + Gepaeck-Gesamtgewicht — zusammen mit der reinen
    /// Fracht (`freight_kg`) ergeben sie `planned_payload_kg`.
    pub planned_pax_weight_kg: f32,
    pub planned_baggage_kg: f32,
    /// Operationeller (flug-spezifischer) MTOW-Grenzwert aus dem OFP —
    /// enger als der strukturelle Grenzwert aus phpVMS' `aircraft.mtow`.
    pub planned_max_tow_kg: f32,
    /// Wenn DEP+ARR matchen aber Callsign abweicht (v0.7.9 Soft-Warning).
    pub callsign_warning: Option<CallsignWarningDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallsignWarningDetails {
    pub sb_callsign: String,
    pub active_callsigns: String,
}

fn has_simbrief_identifier(s: &SimBriefSettings) -> bool {
    s.user_id.as_deref().is_some_and(|v| !v.trim().is_empty())
        || s.username.as_deref().is_some_and(|v| !v.trim().is_empty())
}

/// Fallback: wenn der Bid keinen SimBrief-Link mehr hat, error.
#[tauri::command]
async fn flight_refresh_simbrief(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<SimBriefRefreshResult, UiError> {
    // Snapshot bid_id, current_phase, previous_ofp_id, plus Match-
    // Verifikation-Felder (dpt/arr/airline/flight_number) unter dem Lock;
    // release before any await.
    //
    // v0.7.8: Snapshot erweitert um dpt/arr/airline/flight_number fuer
    // die Flight-Match-Verifikation im Direct-Pfad.
    let (
        bid_id,
        current_phase,
        previous_ofp_id,
        active_dpt,
        active_arr,
        active_airline_icao,
        active_flight_number,
        active_bid_callsign,
        active_pilot_callsign,
    ) = {
        let guard = state.active_flight.lock().expect("active_flight lock");
        let flight = guard
            .as_ref()
            .ok_or_else(|| UiError::new("no_active_flight", "no flight is active"))?;
        let stats = flight.stats.lock().expect("flight stats lock");
        (
            flight.bid_id,
            stats.phase,
            stats.simbrief_ofp_id.clone(),
            flight.dpt_airport.clone(),
            flight.arr_airport.clone(),
            flight.airline_icao.clone(),
            flight.flight_number.clone(),
            flight.bid_callsign.clone(),
            flight.pilot_callsign.clone(),
        )
    };

    // v0.7.7 Phase-Gate: OFP-Refresh ist nur bis vor Takeoff erlaubt
    // (Preflight | Boarding | Pushback | TaxiOut). Spec §6.2.
    if !matches!(
        current_phase,
        FlightPhase::Preflight
            | FlightPhase::Boarding
            | FlightPhase::Pushback
            | FlightPhase::TaxiOut
    ) {
        return Err(UiError::new(
            "phase_locked",
            "OFP-Refresh ist nur bis vor Takeoff moeglich (Preflight bis TaxiOut)",
        ));
    }

    // v0.7.8 Pfad-Auswahl: SimBrief-direct vs Pointer-Pfad
    // Spec §5: Wenn SimBrief-Identifier (userid oder username) konfiguriert,
    // versuche Direct zuerst. Bei Mismatch HARD-Block. Bei
    // Netzwerk/Unavailable-Fehler SOFT-Fallback auf Pointer, aber
    // Direct-Error fuer compose_failure merken.
    let simbrief_settings = state
        .simbrief_settings
        .lock()
        .expect("simbrief_settings lock")
        .clone();
    let has_direct = simbrief_settings.user_id.is_some() || simbrief_settings.username.is_some();
    let client = current_client(&state)?;

    let (sb_id, ofp) = if has_direct {
        match try_simbrief_direct_with_match(
            &client,
            &simbrief_settings,
            &active_dpt,
            &active_arr,
            &active_airline_icao,
            &active_flight_number,
            active_bid_callsign.as_deref(),
            active_pilot_callsign.as_deref(),
        )
        .await
        {
            DirectOutcome::Match { ofp } => {
                // v0.7.8: Im Direct-Pfad ist `request_id` aus dem XML die
                // canonical OFP-ID (NICHT die phpVMS-bid.simbrief.id).
                // Erster Pfad-Wechsel kann changed=true triggern auch bei
                // inhaltsgleichem Plan — danach stabil. Spec §3.
                // v0.7.9: clean Match → eventuelles altes Callsign-Warning
                // loeschen.
                *state
                    .ofp_callsign_warning
                    .lock()
                    .expect("ofp_callsign_warning lock") = None;
                (ofp.request_id.clone(), ofp)
            }
            DirectOutcome::MatchWithCallsignWarning {
                ofp,
                simbrief_callsign,
            } => {
                // v0.7.9: DEP+ARR matchen, nur Callsign abweicht. OFP laden,
                // aber Warning ans Frontend signalisieren via AppState-Flag.
                // Spec docs/spec/v0.7.9-ofp-callsign-soft-warn.md.
                let candidates = build_candidate_callsigns(
                    &active_airline_icao,
                    &active_flight_number,
                    active_bid_callsign.as_deref(),
                    active_pilot_callsign.as_deref(),
                );
                let active_callsigns_display = if candidates.is_empty() {
                    format!("{}{}", active_airline_icao, active_flight_number)
                } else {
                    candidates.join(" / ")
                };
                tracing::info!(
                    sb_callsign = %simbrief_callsign,
                    active_callsigns = %active_callsigns_display,
                    "SimBrief OFP geladen mit Callsign-Mismatch — Warning angezeigt"
                );
                *state
                    .ofp_callsign_warning
                    .lock()
                    .expect("ofp_callsign_warning lock") = Some(OfpCallsignWarning {
                    sb_callsign: simbrief_callsign,
                    active_callsigns: active_callsigns_display,
                    issued_at: Utc::now(),
                });
                (ofp.request_id.clone(), ofp)
            }
            DirectOutcome::Mismatch {
                simbrief_origin,
                simbrief_dest,
                simbrief_callsign,
            } => {
                // v0.7.8 HARD-Block bei Mismatch (Spec v1.1 P1-2).
                // v1.5.1: strukturierte Details als JSON im message-Feld
                // damit Frontend einen reichen i18n-Notice rendern kann.
                // v1.5.2 (Thomas-QS P2): active_callsigns als komplette
                // Kandidatenliste joined mit " / " — Pilot sieht ALLE
                // valid-Formen (z.B. "CFG1504 / CFG4TK / 4TK") statt
                // nur airline+flight_number. Wenn er ein Pilot-Callsign
                // konfiguriert hat, weiss er sofort dass das auch
                // akzeptiert worden waere.
                let candidates = build_candidate_callsigns(
                    &active_airline_icao,
                    &active_flight_number,
                    active_bid_callsign.as_deref(),
                    active_pilot_callsign.as_deref(),
                );
                let active_callsigns_display = if candidates.is_empty() {
                    // Sehr defensiver Fallback — sollte praktisch nicht
                    // passieren weil airline+flight_number immer da sind.
                    if active_airline_icao.is_empty() {
                        active_flight_number.clone()
                    } else {
                        format!("{}{}", active_airline_icao, active_flight_number)
                    }
                } else {
                    candidates.join(" / ")
                };
                let details = serde_json::json!({
                    "active_callsigns": active_callsigns_display,
                    "active_dpt": active_dpt,
                    "active_arr": active_arr,
                    "sb_callsign": simbrief_callsign,
                    "sb_origin": simbrief_origin,
                    "sb_dest": simbrief_dest,
                });
                return Err(UiError::new(
                    "ofp_does_not_match_active_flight",
                    details.to_string(),
                ));
            }
            DirectOutcome::Error(direct_err) => {
                // v0.7.8 SOFT-Fallback: SimBrief offline / Network. Versuche
                // Pointer-Pfad, aber wenn der auch tot ist → compose_failure
                // priorisiert den Direct-Fehler (Spec §5.1).
                tracing::warn!(
                    ?direct_err,
                    "SimBrief-direct failed, attempting pointer fallback",
                );
                match fetch_via_pointer(&client, bid_id).await {
                    Ok((sb_id, ofp)) => (sb_id, ofp),
                    Err(pointer_err) => {
                        return Err(compose_simbrief_failure(direct_err, pointer_err));
                    }
                }
            }
        }
    } else {
        // v0.7.7-Pfad: kein SimBrief-Identifier → nur Pointer-Pfad
        fetch_via_pointer(&client, bid_id).await?
    };

    // v0.7.7 changed-Flag: identische OFP-ID → kein Pilot-Frust-Trigger.
    let changed = previous_ofp_id.as_deref() != Some(sb_id.as_str());

    // Mutate the active flight's planned_* fields. We re-acquire the
    // lock here (post-await) and verify the flight is still the same
    // bid — protects against the user discarding mid-fetch.
    {
        let guard = state.active_flight.lock().expect("active_flight lock");
        let flight = guard.as_ref().ok_or_else(|| {
            UiError::new(
                "no_active_flight",
                "flight was discarded during OFP refresh",
            )
        })?;
        if flight.bid_id != bid_id {
            return Err(UiError::new(
                "flight_changed",
                "active flight changed during OFP refresh — try again",
            ));
        }
        let mut stats = flight.stats.lock().expect("flight stats lock");
        if stats.abflug_icao.is_none() {
            stats.abflug_icao = Some(flight.dpt_airport.trim().to_ascii_uppercase());
        }
        stats.ofp_planzeiten = ofp_planzeiten(&ofp);
        kabinen_eckdaten_aus_ofp(&mut stats, &ofp);
        stats.planned_block_fuel_kg = Some(ofp.planned_block_fuel_kg).filter(|&v| v > 0.0);
        stats.planned_burn_kg = Some(ofp.planned_burn_kg).filter(|&v| v > 0.0);
        stats.planned_reserve_kg = Some(ofp.planned_reserve_kg).filter(|&v| v > 0.0);
        // v1.7.35: Sprit-Leiter mitnehmen.
        //
        // Nur Taxi filtert auf `> 0` — dort heisst eine Null „nicht
        // geplant". Contingency, Alternate und Extra filtern auf `>= 0`,
        // weil die Leiter ALLE Bloecke braucht: Ein Flug ohne Extra hat
        // dort echte 0 kg, und die gehoeren in die Grafik, sonst fehlte ein
        // Block ohne Erklaerung. (Der Kommentar sagte bis v1.7.36 pauschal
        // „0 = None" und beschrieb damit nur den Taxi-Fall.)
        stats.planned_taxi_kg = Some(ofp.planned_taxi_kg).filter(|&v| v > 0.0);
        stats.planned_contingency_kg = Some(ofp.planned_contingency_kg).filter(|&v| v >= 0.0);
        stats.planned_alternate_burn_kg = Some(ofp.planned_alternate_burn_kg).filter(|&v| v >= 0.0);
        stats.planned_extra_kg = Some(ofp.planned_extra_kg).filter(|&v| v >= 0.0);
        stats.planned_zfw_kg = Some(ofp.planned_zfw_kg).filter(|&v| v > 0.0);
        stats.planned_tow_kg = Some(ofp.planned_tow_kg).filter(|&v| v > 0.0);
        stats.planned_ldw_kg = Some(ofp.planned_ldw_kg).filter(|&v| v > 0.0);
        stats.planned_route = ofp.route.clone();
        stats.planned_atc_callsign = ofp_atc_callsign(&ofp.ofp_flight_number);
        // v0.15.6: Bugfix — der Refresh-Pfad hat `planned_waypoints` NIE
        // gesetzt (nur `flight_start` tat das). Folge: wer den SimBrief-Plan
        // erst nach dem Bid generierte/aktualisierte (OFP kam per Refresh,
        // nicht beim Start), hatte nie Wegpunkte/Route auf der In-App-Map.
        // Nur überschreiben, wenn der neue OFP welche hat — sonst behalten
        // wir die schon vorhandenen (Refresh ohne navlog = selten).
        if !ofp.waypoints.is_empty() {
            stats.planned_waypoints = ofp.waypoints.clone();
            // v1.7.36: Der Anker (Plan-Reststrecke ab TOD) stammte aus den ALTEN
            // Wegpunkten. Anker und Projektion muessen aus derselben Route kommen
            // (QS-Befund V4, 18.09.2026).
            sprit_plan_neu_lesen(&mut stats);
        }
        stats.planned_alternate = ofp.alternate.clone();
        stats.planned_max_zfw_kg = Some(ofp.max_zfw_kg).filter(|&v| v > 0.0);
        stats.planned_max_tow_kg = Some(ofp.max_tow_kg).filter(|&v| v > 0.0);
        stats.planned_max_ldw_kg = Some(ofp.max_ldw_kg).filter(|&v| v > 0.0);
        stats.flight_plan_source = Some("simbrief");
        // v0.7.7: OFP-ID + generated_at fuer naechsten changed-Vergleich.
        stats.simbrief_ofp_id = Some(sb_id.clone());
        stats.simbrief_ofp_generated_at = if ofp.ofp_generated_at.is_empty() {
            None
        } else {
            Some(ofp.ofp_generated_at.clone())
        };
        drop(stats);
        save_active_flight(&app, flight);
    }
    // v0.7.7 Audit-Trail: alte→neue OFP-ID + changed/unchanged-Marker.
    log_activity(
        &state,
        ActivityLevel::Info,
        if changed {
            "OFP refreshed"
        } else {
            "OFP unchanged"
        }
        .to_string(),
        Some(format!(
            "vorher {}, jetzt {} ({}). Block {:.0} kg, TOW {:.0} kg, LDW {:.0} kg",
            previous_ofp_id.as_deref().unwrap_or("—"),
            sb_id,
            if changed { "neu" } else { "identisch" },
            ofp.planned_block_fuel_kg,
            ofp.planned_tow_kg,
            ofp.planned_ldw_kg
        )),
    );
    Ok(SimBriefRefreshResult {
        ofp: SimBriefOfpDto {
            planned_block_fuel_kg: ofp.planned_block_fuel_kg,
            planned_burn_kg: ofp.planned_burn_kg,
            planned_reserve_kg: ofp.planned_reserve_kg,
            planned_zfw_kg: ofp.planned_zfw_kg,
            planned_tow_kg: ofp.planned_tow_kg,
            planned_ldw_kg: ofp.planned_ldw_kg,
            route: ofp.route,
            alternate: ofp.alternate,
            ofp_flight_number: ofp.ofp_flight_number,
            ofp_origin_icao: ofp.ofp_origin_icao,
            ofp_destination_icao: ofp.ofp_destination_icao,
            ofp_generated_at: ofp.ofp_generated_at,
        },
        previous_ofp_id,
        current_ofp_id: sb_id,
        changed,
    })
}

/// v0.16.23: DTO fuer `flight_refresh_route_only`. Bewusst schmal —
/// die UI braucht nur die Bestaetigung dass die Route uebernommen wurde
/// (Wegpunkt-Anzahl + ggf. neuer Alternate) plus das `route_posted`-Flag
/// das sagt ob das phpVMS-Upload geklappt hat (Warning, kein Hard-Error).
#[derive(Debug, Clone, Serialize)]
struct RouteRefreshResult {
    /// Anzahl Navlog-Wegpunkte die jetzt auf der Map liegen.
    waypoint_count: usize,
    /// Neuer Alternate-ICAO falls der OFP einen lieferte.
    #[serde(skip_serializing_if = "Option::is_none")]
    alternate: Option<String>,
    /// Route-String (ICAO-codiert) falls vorhanden — nur fuer Anzeige.
    #[serde(skip_serializing_if = "Option::is_none")]
    route: Option<String>,
    /// true wenn das `POST /api/pireps/{id}/route`-Upload erfolgreich war.
    /// false = Route lokal aktualisiert (Map zeigt sie), aber das Posten
    /// an phpVMS/VPS schlug fehl — die Korrektur erreicht den Server beim
    /// naechsten erfolgreichen Sync nicht automatisch. Kein Hard-Error.
    route_posted: bool,
}

/// ATC-Rufzeichen eines OFP fuer Hoppie/PDC: Leerzeichen raus, gross geschrieben,
/// dann nur eine gueltige ICAO-Kennung (Flugplan Feld 7: 2-7 Zeichen A-Z/0-9, erstes
/// Zeichen ein Buchstabe). `ofp_flight_number` faellt ohne `<atc><callsign>` auf die
/// reine Flugnummer ("371") zurueck — die ist kein Rufzeichen und ergibt None, dann
/// bleibt Hoppie bei Airline + Flugnummer. Gleiche Regel wie PaxStudio
/// `UtilityService::normalizeAtcCallsign`.
fn ofp_atc_callsign(raw: &str) -> Option<String> {
    let cs = raw
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let mut chars = cs.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_uppercase());
    let rest_ok = chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    (first_ok && rest_ok && (2..=7).contains(&cs.len())).then_some(cs)
}

/// Rufzeichen, unter dem der aktive Flug bei Hoppie/PDC auftritt: das des OFP,
/// sonst Airline + Flugnummer (so wie bisher immer).
pub(crate) fn flight_atc_callsign(
    planned: Option<&str>,
    airline_icao: &str,
    flight_number: &str,
) -> String {
    planned
        .filter(|cs| !cs.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{airline_icao}{flight_number}"))
}

/// v0.16.23: Schreibt AUSSCHLIESSLICH die Routen-Felder eines frischen
/// OFP in die FlightStats — `planned_route`, `planned_waypoints` und
/// (falls der OFP einen liefert) `planned_alternate`, dazu das ATC-Rufzeichen
/// des OFP (`planned_atc_callsign`, kein Score-Feld — nur Hoppie/PDC). Bewusst KEINE
/// Beruehrung irgendeines Score-Feldes: kein `*_kg`, kein
/// `flight_plan_source`, kein `simbrief_ofp_id`, kein
/// `simbrief_ofp_generated_at`.
///
/// **Reviewer-Hinweis:** Das ist die einzige Stelle die im Route-Only-
/// Pfad FlightStats mutiert. Wer hier ein Feld ergaenzt, MUSS pruefen ob
/// es ein Score-Feld ist — der Isolations-Test
/// `route_only_mutation_touches_only_route_fields` faengt einen Fehltritt,
/// aber der Default beim Erweitern muss "nur Route" bleiben. Score-Felder
/// gehoeren in `flight_refresh_simbrief`, NICHT hier.
///
/// `planned_waypoints` wird nur ueberschrieben wenn der neue OFP welche
/// hat (gleiche Vorsicht wie im Refresh-Pfad ~v0.15.6) — ein OFP ohne
/// Navlog soll die schon vorhandenen Wegpunkte nicht loeschen.
fn apply_route_only_to_stats(stats: &mut FlightStats, ofp: &api_client::SimBriefOfp) {
    stats.planned_route = ofp.route.clone();
    stats.planned_atc_callsign = ofp_atc_callsign(&ofp.ofp_flight_number);
    if !ofp.waypoints.is_empty() {
        stats.planned_waypoints = ofp.waypoints.clone();
        // v1.7.36: Der Anker (Plan-Reststrecke ab TOD) stammte aus den ALTEN
        // Wegpunkten. Anker und Projektion muessen aus derselben Route kommen
        // (QS-Befund V4, 18.09.2026).
        sprit_plan_neu_lesen(stats);
    }
    // Alternate nur uebernehmen wenn der OFP einen nicht-leeren liefert —
    // sonst den schon vorhandenen behalten (kein Loeschen durch Sync).
    if let Some(alt) = ofp.alternate.as_ref().filter(|a| !a.trim().is_empty()) {
        stats.planned_alternate = Some(alt.clone());
    }
    // „Zeiten gegen Flugplan" nur NACHTRAGEN, nie ersetzen: Ein Flug, der
    // vor 2.0.15 begann und nach dem Update fortgesetzt wird, hat keinen
    // Plan (AIB 431, 13.10.2026). Reine Anzeige, kein Score-Feld.
    // Bewusst als ganzer Satz, nicht feldweise: Feldweise koennten Zeiten
    // aus zwei verschiedenen OFPs gemischt werden (Codex PR #139, P2 —
    // abgewogen und so belassen).
    if stats.ofp_planzeiten == landing_scoring::flugzeiten::FlugZeiten::default() {
        let mut neu = ofp_planzeiten(ofp);
        // Erledigt-Marker fuer `ofp_nachladen_noetig`: Fehlt dem
        // nachgeladenen OFP die Erstellzeit, gilt die vom Flugstart
        // (Codex PR #139, P2) — sonst luede jeder Neustart erneut.
        if neu.ofp_erstellt.is_none() {
            neu.ofp_erstellt = stats
                .simbrief_ofp_generated_at
                .as_deref()
                .and_then(|t| t.trim().parse::<i64>().ok())
                .filter(|&t| t > 0);
        }
        stats.ofp_planzeiten = neu;
    }
}

/// Muss der wiederhergestellte Flug den OFP einmal nachladen?
///
/// - Sprit-Planwerte je Wegpunkt fehlen (Flug vor v1.7.40 begonnen).
/// - „Zeiten gegen Flugplan" fehlen bei einem SimBrief-Flug (Flug vor
///   2.0.15 begonnen, mitten im Flug aktualisiert — AIB 431, 13.10.2026).
fn ofp_nachladen_noetig(st: &FlightStats) -> bool {
    let sprit_plan_fehlt = st.planned_waypoints.len() >= 2
        && st
            .planned_waypoints
            .iter()
            .all(|f| f.sprit_an_bord_kg.is_none());
    // ⚠ Woran ein OFP-Flug nach dem Fortsetzen erkennbar ist: an der
    // gespeicherten Erstellzeit des OFP. `flight_plan_source` wird nicht
    // gespeichert, `simbrief_ofp_id` fehlt beim Direct-Start ohne
    // Bid-Kennung (Codex PR #139, zweimal P1).
    //
    // Erledigt-Marker: Nach dem Nachladen traegt `ofp_planzeiten` die
    // Erstellzeit (`ofp_erstellt`) — auch wenn der OFP keine einzige
    // Planzeit hat. Sonst luede jeder Neustart erneut (Codex P2).
    let planzeiten_fehlen = st.simbrief_ofp_generated_at.is_some()
        && st.ofp_planzeiten == landing_scoring::flugzeiten::FlugZeiten::default();
    sprit_plan_fehlt || planzeiten_fehlen
}

/// Umleitung (Beta): normalisiert die Eingabe des Piloten. Leer heisst
/// „zuruecknehmen“; das geplante Ziel ebenso (keine Umleitung).
fn umleitung_ziel_normalisieren(eingabe: Option<&str>, geplant: &str) -> Result<Option<String>, UiError> {
    let icao = eingabe.unwrap_or("").trim().to_ascii_uppercase();
    if icao.is_empty() || icao == geplant.trim().to_ascii_uppercase() {
        return Ok(None);
    }
    if icao.len() != 4 || !icao.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(UiError::new("invalid_icao", "ICAO-Code mit 4 Zeichen erwartet"));
    }
    Ok(Some(icao))
}

/// Umleitung (Beta): Der Pilot meldet im Flug ein neues Ziel. Daraus kommt
/// die Umleitungs-Ansage; am PIREP aendert das nichts.
#[tauri::command]
async fn flight_umleitung_setzen(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    icao: Option<String>,
) -> Result<Option<String>, UiError> {
    let flight = state
        .active_flight
        .lock()
        .expect("active_flight lock")
        .as_ref()
        .map(Arc::clone)
        .ok_or_else(|| UiError::new("no_active_flight", "no active flight"))?;
    let ziel = umleitung_ziel_normalisieren(icao.as_deref(), &flight.arr_airport)?;
    flight.stats.lock().expect("flight stats lock").umleitung_ziel = ziel.clone();
    save_active_flight(&app, &flight);
    tracing::info!(ziel = ?ziel, "umleitung vom piloten gemeldet");
    Ok(ziel)
}

/// v0.16.23: **Route-Only-Refresh** — holt den AKTUELLEN SimBrief-OFP und
/// uebernimmt NUR die Strecke (planned_route + planned_waypoints +
/// Alternate) in den laufenden Flug, postet sie an phpVMS und beruehrt
/// dabei KEIN einziges Score-Feld.
///
/// Motivation (v0.16.23): Korrigiert der Pilot seine Route in SimBrief
/// (z.B. ATC-Reroute), lebt der korrigierte Navlog nur im NEUESTEN
/// SimBrief-OFP — phpVMS/VPS halten den alten. Die Map malt aber aus
/// `planned_waypoints`, also blieb die falsche Strecke stehen.
/// `flight_refresh_simbrief` koennte den Navlog zwar neu ziehen, ist aber
/// (a) nach dem Takeoff phasen-gesperrt (Fuel/Loadsheet-Score-Schutz) und
/// (b) postet die Route NIE an phpVMS (das macht nur `flight_start`).
///
/// Unterschiede zu `flight_refresh_simbrief` (ABSICHTLICH):
///   - **KEIN Phasen-Gate** — in JEDER Phase erlaubt (die Strecke ist
///     kein Score-Feld; sie zu korrigieren ist immer sicher).
///   - **Nur der Direct-Pfad** (`try_simbrief_direct_with_match`) mit
///     vollem DEP/ARR-Match-HARD-Block. KEIN Pointer-Fallback — der koennte
///     nur die STALE Route liefern. Ohne Identifier/Direct-Pfad →
///     typed Error `no_simbrief_identifier`.
///   - Schreibt via `apply_route_only_to_stats` AUSSCHLIESSLICH Routen-
///     Felder, dann `post_route`. Post-Fehler = Warning (Route lokal schon
///     aktualisiert), kein Hard-Error.
#[tauri::command]
async fn flight_refresh_route_only(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RouteRefreshResult, UiError> {
    // Snapshot der Match-Verifikations-Felder + pirep_id unter dem Lock;
    // vor jedem await freigeben. ANDERS als flight_refresh_simbrief
    // snapshotten wir NICHT `current_phase` (kein Phasen-Gate) und auch
    // nicht `previous_ofp_id` (kein changed-Flag, kein OFP-ID-Update).
    let (
        bid_id,
        pirep_id,
        active_dpt,
        active_arr,
        active_airline_icao,
        active_flight_number,
        active_bid_callsign,
        active_pilot_callsign,
    ) = {
        let guard = state.active_flight.lock().expect("active_flight lock");
        let flight = guard
            .as_ref()
            .ok_or_else(|| UiError::new("no_active_flight", "no flight is active"))?;
        (
            flight.bid_id,
            flight.pirep_id.clone(),
            flight.dpt_airport.clone(),
            flight.arr_airport.clone(),
            flight.airline_icao.clone(),
            flight.flight_number.clone(),
            flight.bid_callsign.clone(),
            flight.pilot_callsign.clone(),
        )
    };

    // Direct-Pfad ZWINGEND: ohne SimBrief-Identifier koennen wir nur die
    // (potenziell stale) Pointer-Route holen — genau das wollen wir NICHT.
    // Daher hier ein typed Error statt eines Fallbacks.
    let simbrief_settings = state
        .simbrief_settings
        .lock()
        .expect("simbrief_settings lock")
        .clone();
    if simbrief_settings.user_id.is_none() && simbrief_settings.username.is_none() {
        return Err(UiError::new(
            "no_simbrief_identifier",
            "no SimBrief identifier configured — set your SimBrief username in Settings to sync route changes",
        ));
    }

    let client = current_client(&state)?;

    // Latest OFP via Direct-Pfad + DEP/ARR-Match. Mismatch = HARD-Block
    // (ein OFP fuer einen anderen Flug darf nie eine falsche Route malen).
    let ofp = match try_simbrief_direct_with_match(
        &client,
        &simbrief_settings,
        &active_dpt,
        &active_arr,
        &active_airline_icao,
        &active_flight_number,
        active_bid_callsign.as_deref(),
        active_pilot_callsign.as_deref(),
    )
    .await
    {
        DirectOutcome::Match { ofp } | DirectOutcome::MatchWithCallsignWarning { ofp, .. } => ofp,
        DirectOutcome::Mismatch {
            simbrief_origin,
            simbrief_dest,
            simbrief_callsign,
        } => {
            // Gleicher HARD-Block wie flight_refresh_simbrief — DEP/ARR
            // passen nicht → klar ein anderer Flug. Strukturierte
            // JSON-Details, damit der bestehende refreshErrorFormatter
            // den reichen Mismatch-Notice rendern kann.
            let candidates = build_candidate_callsigns(
                &active_airline_icao,
                &active_flight_number,
                active_bid_callsign.as_deref(),
                active_pilot_callsign.as_deref(),
            );
            let active_callsigns_display = if candidates.is_empty() {
                if active_airline_icao.is_empty() {
                    active_flight_number.clone()
                } else {
                    format!("{}{}", active_airline_icao, active_flight_number)
                }
            } else {
                candidates.join(" / ")
            };
            let details = serde_json::json!({
                "active_callsigns": active_callsigns_display,
                "active_dpt": active_dpt,
                "active_arr": active_arr,
                "sb_callsign": simbrief_callsign,
                "sb_origin": simbrief_origin,
                "sb_dest": simbrief_dest,
            });
            return Err(UiError::new(
                "ofp_does_not_match_active_flight",
                details.to_string(),
            ));
        }
        DirectOutcome::Error(direct_err) => {
            // KEIN Pointer-Fallback (der koennte nur die stale Route
            // liefern). Direkter Fehler mit dem actionable Hinweis.
            return Err(match direct_err {
                SimBriefDirectError::UserNotFound => UiError::new(
                    "simbrief_user_not_found",
                    "SimBrief-Username/User-ID nicht gefunden. Pruefe Settings → SimBrief Integration.",
                ),
                SimBriefDirectError::Unavailable => UiError::new(
                    "simbrief_unavailable",
                    "SimBrief gerade nicht erreichbar — versuche es in ein paar Minuten erneut.",
                ),
                SimBriefDirectError::Network | SimBriefDirectError::ParseFailed => UiError::new(
                    "simbrief_direct_failed",
                    format!("SimBrief-direct schlug fehl ({direct_err:?})."),
                ),
                SimBriefDirectError::NoIdentifier => UiError::new(
                    "no_simbrief_identifier",
                    "no SimBrief identifier configured — set your SimBrief username in Settings to sync route changes",
                ),
            });
        }
    };

    // Mutation: NUR Routen-Felder. Same-bid-Re-Check nach dem await
    // (Pilot koennte den Flug mittendrin verworfen/gewechselt haben).
    {
        let guard = state.active_flight.lock().expect("active_flight lock");
        let flight = guard.as_ref().ok_or_else(|| {
            UiError::new(
                "no_active_flight",
                "flight was discarded during route refresh",
            )
        })?;
        if flight.bid_id != bid_id {
            return Err(UiError::new(
                "flight_changed",
                "active flight changed during route refresh — try again",
            ));
        }
        let mut stats = flight.stats.lock().expect("flight stats lock");
        apply_route_only_to_stats(&mut stats, &ofp);
        drop(stats);
        save_active_flight(&app, flight);
    }

    // Route an phpVMS posten (cf. flight_start). Best-effort: ein
    // Fehler ist ein WARNING — die Route ist lokal schon aktualisiert.
    let waypoint_count = ofp.waypoints.len();
    let route_posted = if ofp.waypoints.is_empty() {
        // Kein Navlog im OFP → nichts zu posten. Wir behandeln das als
        // "nicht gepostet" (es gibt keine neue Route fuer den Server),
        // aber das ist kein Fehler.
        false
    } else {
        let route = build_route_waypoints(&ofp.waypoints);
        match client.post_route(&pirep_id, &route).await {
            Ok(()) => {
                tracing::info!(
                    pirep_id = %pirep_id,
                    waypoint_count = route.len(),
                    "route-only sync: planned route uploaded to phpVMS"
                );
                true
            }
            Err(e) => {
                tracing::warn!(
                    pirep_id = %pirep_id,
                    error = %e,
                    "route-only sync: route already updated locally but phpVMS upload failed"
                );
                false
            }
        }
    };

    log_activity(
        &state,
        ActivityLevel::Info,
        "Route synchronisiert".to_string(),
        Some(format!(
            "{} Wegpunkte aus dem aktuellen SimBrief-OFP uebernommen ({}).",
            waypoint_count,
            if route_posted {
                "an phpVMS gepostet"
            } else if waypoint_count == 0 {
                "kein Navlog im OFP"
            } else {
                "phpVMS-Upload fehlgeschlagen — lokal aktualisiert"
            }
        )),
    );

    Ok(RouteRefreshResult {
        waypoint_count,
        alternate: ofp.alternate.filter(|a| !a.trim().is_empty()),
        route: ofp.route.filter(|r| !r.trim().is_empty()),
        route_posted,
    })
}

/// v0.7.8: Outcome des SimBrief-direct-Fetch + Match-Verifikation.
/// Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md §5 + §6.
/// v0.7.9 (Pilot-Feedback EWG4PY-Fall): Callsign-Match auf SOFT-WARN
/// abgemildert. Hard-Block nur noch wenn DEP/ARR nicht matchen — sonst
/// laden wir den OFP, zeigen aber einen Warning-Notice wenn Callsign
/// abweicht. Real-Use-Case: VA-Operator-Code (EWL) und SimBrief-Callsign
/// (EWG4PY) sind unterschiedlich.
enum DirectOutcome {
    /// SimBrief lieferte OFP, dpt+arr+callsign matchen alle.
    Match { ofp: SimBriefOfp },
    /// v0.7.9: dpt+arr matchen, aber Callsign abweicht. OFP wird trotzdem
    /// geladen — Warning-Notice mit den Werten im UI rendern.
    MatchWithCallsignWarning {
        ofp: SimBriefOfp,
        simbrief_callsign: String,
    },
    /// dpt oder arr passen NICHT — Pilot hat zwischendurch einen OFP fuer
    /// einen ANDEREN Flughafen regeneriert. HARD-Block bleibt, weil
    /// Route-Mismatch = ein klar anderer Flug.
    Mismatch {
        simbrief_origin: String,
        simbrief_dest: String,
        simbrief_callsign: String,
    },
    /// Direct-Fetch fehlgeschlagen (Network/Unavailable/UserNotFound/
    /// ParseFailed). SOFT-Fallback auf Pointer-Pfad versuchen,
    /// Direct-Error fuer compose_failure merken.
    Error(SimBriefDirectError),
}

/// v0.7.8: Versucht SimBrief-direct Fetch + Match-Verifikation.
/// Spec §5.1.
async fn try_simbrief_direct_with_match(
    client: &Client,
    settings: &SimBriefSettings,
    active_dpt: &str,
    active_arr: &str,
    active_airline_icao: &str,
    active_flight_number: &str,
    active_bid_callsign: Option<&str>,
    active_pilot_callsign: Option<&str>,
) -> DirectOutcome {
    match client
        .fetch_simbrief_direct(settings.user_id.as_deref(), settings.username.as_deref())
        .await
    {
        Ok(ofp) => {
            // v0.7.9: 2-stufige Match-Logik:
            //   1. DEP/ARR matchen → OFP wird geladen
            //   2. Callsign-Check als ZUSAETZLICHES Signal, aber kein
            //      Hard-Block mehr.
            let dpt_ok = ofp
                .ofp_origin_icao
                .trim()
                .eq_ignore_ascii_case(active_dpt.trim());
            let arr_ok = ofp
                .ofp_destination_icao
                .trim()
                .eq_ignore_ascii_case(active_arr.trim());

            if !dpt_ok || !arr_ok {
                // Route-Mismatch = klar ein anderer Flug. Hard-Block.
                return DirectOutcome::Mismatch {
                    simbrief_origin: ofp.ofp_origin_icao.clone(),
                    simbrief_dest: ofp.ofp_destination_icao.clone(),
                    simbrief_callsign: ofp.ofp_flight_number.clone(),
                };
            }

            // DEP+ARR matchen — pruefe Callsign nur fuer Warning.
            let callsign_ok = ofp_matches_active_flight(
                &ofp.ofp_origin_icao,
                &ofp.ofp_destination_icao,
                &ofp.ofp_flight_number,
                active_dpt,
                active_arr,
                active_airline_icao,
                active_flight_number,
                active_bid_callsign,
                active_pilot_callsign,
            );

            if callsign_ok {
                DirectOutcome::Match { ofp }
            } else {
                let simbrief_callsign = ofp.ofp_flight_number.clone();
                DirectOutcome::MatchWithCallsignWarning {
                    ofp,
                    simbrief_callsign,
                }
            }
        }
        Err(e) => DirectOutcome::Error(e),
    }
}

/// v0.7.7/v0.7.8 Pointer-Pfad: Fetch via `client.get_bids()` + Bid.simbrief.id.
/// Faellt bei `bid_not_found` (W5: Bid weg nach Prefile) mit klarem
/// Error. Returns (sb_id, ofp) auf Erfolg.
/// Spec docs/spec/ofp-refresh-during-boarding.md §5 + §5.1.
async fn fetch_via_pointer(client: &Client, bid_id: i64) -> Result<(String, SimBriefOfp), UiError> {
    let bids = client.get_bids().await.map_err(|e| {
        UiError::new(
            "bids_fetch_failed",
            format!("could not refresh bid list: {e}"),
        )
    })?;
    let bid = bids.into_iter().find(|b| b.id == bid_id).ok_or_else(|| {
        UiError::new(
            "bid_not_found",
            "current bid is no longer in your bid list — cannot refresh OFP",
        )
    })?;
    let sb_id = bid
        .flight
        .simbrief
        .as_ref()
        .map(|s| s.id.clone())
        .ok_or_else(|| {
            UiError::new(
                "no_simbrief_link",
                "bid has no SimBrief OFP linked — generate one on simbrief.com first",
            )
        })?;
    let ofp = client.fetch_simbrief_ofp(&sb_id).await.map_err(|e| {
        UiError::new(
            "ofp_fetch_failed",
            format!("SimBrief OFP fetch failed: {e}"),
        )
    })?;
    let ofp = ofp.ok_or_else(|| {
        UiError::new(
            "ofp_unusable",
            "SimBrief returned no usable OFP — check your simbrief.com OFP",
        )
    })?;
    Ok((sb_id, ofp))
}

/// v0.7.8: Composite-Failure wenn BEIDE Pfade tot — Direct-Error
/// priorisieren damit Pilot den actionable Hinweis kriegt
/// ("Username falsch" beats "Bid weg"). Spec §5.1.
fn compose_simbrief_failure(direct: SimBriefDirectError, pointer: UiError) -> UiError {
    match direct {
        SimBriefDirectError::UserNotFound => UiError::new(
            "simbrief_user_not_found",
            "SimBrief-Username/User-ID nicht gefunden. Pruefe Settings → SimBrief Integration.",
        ),
        SimBriefDirectError::Unavailable => UiError::new(
            "simbrief_unavailable_and_bid_gone",
            "SimBrief gerade nicht erreichbar; Pointer-Fallback auch nicht moeglich. Versuche es in ein paar Minuten erneut.",
        ),
        SimBriefDirectError::Network | SimBriefDirectError::ParseFailed => UiError::new(
            "simbrief_direct_failed",
            format!(
                "SimBrief-direct schlug fehl ({:?}). Pointer-Pfad: {}",
                direct, pointer.message,
            ),
        ),
        SimBriefDirectError::NoIdentifier => {
            // Sollte nicht passieren — Pfad-Auswahl im Caller faengt das ab.
            pointer
        }
    }
}

/// v0.7.7 DTO fuer `flight_refresh_simbrief`. Wrappt `SimBriefOfpDto`
/// (= das was die UI zum Anzeigen braucht) und ergaenzt um die OFP-ID-
/// Diff-Information die das Frontend fuer den unveraendert-Toast braucht.
/// `SimBriefOfpDto` wird parallel auch von `fetch_simbrief_preview`
/// genutzt — daher der Split. Spec §6.1.
#[derive(Debug, Clone, Serialize)]
struct SimBriefRefreshResult {
    ofp: SimBriefOfpDto,
    /// SimBrief-OFP-ID die vor diesem Refresh aktiv war. None bei
    /// pre-v0.7.7-Resume-Files (Persistenz-Foundation kam erst in v0.7.7).
    previous_ofp_id: Option<String>,
    /// SimBrief-OFP-ID die jetzt aktiv ist (= aus dem phpVMS-Bid gerade
    /// geholt). Bei `changed=false` ist das identisch zu `previous_ofp_id`.
    current_ofp_id: String,
    /// `current_ofp_id != previous_ofp_id`. Frontend nutzt das fuer die
    /// "OFP unveraendert"-Info-Notice (sichtbar machen wenn PAX Studio
    /// noch nicht synct hat).
    changed: bool,
}

/// Frontend in der ausgeklappten Bid-Card aufgerufen, damit der Pilot
/// die Plan-Werte (Block-Fuel, Trip-Burn, TOW, LDW etc.) **vor**
/// dem Flight-Start sieht.
///
/// Im Gegensatz zum Auto-Fetch beim flight_start() (der die Werte
/// in die FlightStats schreibt), liefert dieser Command sie nur
/// transient an die UI zurück. Kein State-Mutation.
#[tauri::command]
async fn fetch_simbrief_preview(
    state: tauri::State<'_, AppState>,
    ofp_id: String,
) -> Result<Option<SimBriefOfpDto>, UiError> {
    let client = current_client(&state)?;
    match client.fetch_simbrief_ofp(&ofp_id).await {
        Ok(Some(ofp)) => Ok(Some(SimBriefOfpDto {
            planned_block_fuel_kg: ofp.planned_block_fuel_kg,
            planned_burn_kg: ofp.planned_burn_kg,
            planned_reserve_kg: ofp.planned_reserve_kg,
            planned_zfw_kg: ofp.planned_zfw_kg,
            planned_tow_kg: ofp.planned_tow_kg,
            planned_ldw_kg: ofp.planned_ldw_kg,
            route: ofp.route,
            alternate: ofp.alternate,
            ofp_flight_number: ofp.ofp_flight_number,
            ofp_origin_icao: ofp.ofp_origin_icao,
            ofp_destination_icao: ofp.ofp_destination_icao,
            ofp_generated_at: ofp.ofp_generated_at,
        })),
        Ok(None) => Ok(None), // OFP nicht abrufbar (Netz-Fehler / 404 / ...)
        Err(e) => Err(e.into()),
    }
}

/// DTO für den SimBrief-Preview-Tauri-Command. Bewusst flach gehalten,
/// die `waypoints` aus `SimBriefOfp` ignoriert die UI hier (zu sperrig
/// für die kompakte Bid-Card-Vorschau).
#[derive(Debug, Clone, Serialize)]
struct SimBriefOfpDto {
    planned_block_fuel_kg: f32,
    planned_burn_kg: f32,
    planned_reserve_kg: f32,
    planned_zfw_kg: f32,
    planned_tow_kg: f32,
    planned_ldw_kg: f32,
    route: Option<String>,
    alternate: Option<String>,
    // v0.3.0: OFP-Identität für Mismatch-Detection im Frontend.
    ofp_flight_number: String,
    ofp_origin_icao: String,
    ofp_destination_icao: String,
    ofp_generated_at: String,
}

/// Aircraft-Details für die Bid-Card (v0.3.0). Liefert die Registrierung
/// + ICAO + Name eines Aircraft, sodass die Bid-Card "RYR-B738-WL · EI-ENI"
/// anzeigen kann statt nur den Subfleet-Namen.
#[tauri::command]
async fn phpvms_get_aircraft(
    state: tauri::State<'_, AppState>,
    aircraft_id: i64,
) -> Result<AircraftInfoDto, UiError> {
    let client = current_client(&state)?;
    let details = client.get_aircraft(aircraft_id).await?;
    Ok(AircraftInfoDto {
        id: details.id,
        registration: details.registration,
        icao: details.icao,
        name: details.name,
        // Briefing-2a-Spec §5.3: WEIGHTS-Rubrik braucht MTOW fuer die
        // Einordnung ("TOW · n kg unter MTOW"). phpVMS liefert die Masse
        // immer als {kg, lbs} — wir geben nur kg weiter, das Frontend
        // braucht keine lbs-Variante.
        mtow_kg: details.mtow.and_then(|m| m.kg),
    })
}

/// Schmale Aircraft-Info für die Frontend-Anzeige. Nur die Felder die
/// wir wirklich rendern — Status/Airport-ID lassen wir weg.
#[derive(Debug, Clone, Serialize)]
struct AircraftInfoDto {
    id: i64,
    registration: Option<String>,
    icao: Option<String>,
    name: Option<String>,
    mtow_kg: Option<f64>,
}

// ---- Active-flight persistence (for resume after crash/restart) ----
