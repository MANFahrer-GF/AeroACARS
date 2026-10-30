/// Shared application state — wraps the currently-authenticated client (if any)
/// and (on Windows) the MSFS adapter.
/// Cap on retained activity-log entries. The frontend displays the most
/// recent N — older lines fall off so the buffer doesn't grow forever
/// during a long flight (~5 h cruise = ~600 phase-tagged position posts
/// in the log; 1000 leaves comfortable headroom for transitions, errors
/// and bid-list updates on top).
const ACTIVITY_LOG_CAPACITY: usize = 1000;

/// Severity of an activity-log entry. Drives the frontend's icon/colour.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ActivityLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActivityEntry {
    timestamp: DateTime<Utc>,
    level: ActivityLevel,
    /// Stable English label for the event (e.g. "PIREP prefiled").
    /// Frontend may translate via i18n if a key matches; otherwise
    /// renders as-is.
    message: String,
    /// Optional context — flight phase, PIREP id, error code, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Default)]
struct AppState {
    client: Mutex<Option<Client>>,
    #[cfg(target_os = "windows")]
    msfs: Mutex<MsfsAdapter>,
    /// X-Plane adapter — cross-platform (UDP). Co-exists with the
    /// MSFS adapter; only one is `started` at a time, dictated by
    /// the persisted `SimKind`.
    xplane: Mutex<sim_xplane::XPlaneAdapter>,
    active_flight: Mutex<Option<Arc<ActiveFlight>>>,
    /// Codex-Folgefund (adversarial, 05.09.2026, siebte Runde): server-
    /// verifizierte GSG-Piloten-ID (`Profile.pilot_id`) des Accounts, der
    /// den AKTUELL aktiven Flug gestartet/uebernommen hat — als String
    /// (`pilot_id.to_string()`), EINMAL gesetzt (in `flight_start`/
    /// `flight_adopt`/erfolgreichem Resume), NICHT bei jedem Speichern/
    /// Einreihen neu abgeleitet. Grund: `phpvms_logout` leert `state.client`,
    /// aber ausdruecklich NICHT `state.active_flight` ("ein anderer Pilot
    /// kann sich auf derselben Maschine anmelden") — ohne dieses Feld haette
    /// jede spaetere Speicherung/Einreihung waehrend eines noch laufenden
    /// Fluges die Identitaet des GERADE eingeloggten Piloten getragen, nicht
    /// die des Piloten, dem der Flug tatsaechlich gehoert.
    /// `PersistedFlight::owner_identity` und
    /// `pirep_queue::QueuedPirep::owner_identity` werden BEIDE aus diesem
    /// Feld befuellt.
    ///
    /// URSPRUENGLICH aus `Client::identity_fingerprint()` (ein Hash aus
    /// Basis-URL + API-Key) — von Codex zu Recht verworfen: eine legitime
    /// API-Key-Rotation DESSELBEN Piloten aendert den Hash, obwohl der
    /// Account derselbe bleibt. `Profile.pilot_id` ist server-verifiziert
    /// und ueberlebt eine Rotation.
    ///
    /// WICHTIG (Codex-Folgefund, dieselbe Runde): dieses Feld allein beweist
    /// NICHT, dass ein Flug aktiv ist — es wird beim Beenden/Abbrechen eines
    /// Fluges NICHT geleert. JEDER Konsument muss `state.active_flight.
    /// is_some()` (oder das serverseitige Reklamier-Ergebnis bei Resume/
    /// Queue) als GLEICHZEITIGE Bedingung pruefen, niemals dieses Feld
    /// isoliert als "ein Flug ist aktiv" lesen.
    active_flight_owner_identity: Mutex<Option<String>>,
    /// Codex-Folgefund (adversarial, 05.09.2026, siebte Runde): die
    /// server-verifizierte Piloten-ID (`Profile.pilot_id`) des GERADE
    /// authentifizierten Accounts — gesetzt bei jedem erfolgreichen Login
    /// (`phpvms_login`, `phpvms_load_session`), damit `flight_start`/
    /// `flight_adopt`/der manuelle Plan-Pfad und `try_resume_flight` die
    /// aktuelle Identitaet lesen koennen OHNE jeweils selbst einen weiteren
    /// `get_profile()`-Server-Roundtrip zu brauchen. NICHT beim Logout
    /// geleert.
    ///
    /// ⚠ Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde): die
    /// urspruengliche Behauptung hier ("harmlos: nur relevant waehrend
    /// state.client gleichzeitig Some ist") stimmte NICHT mehr, sobald
    /// Runde 12s `phpvms_refresh_profile` dieses Feld ALLEIN als Beweis
    /// benutzte, dass eine verspaetete Server-Antwort noch zur aktiven
    /// Sitzung gehoert — nach einem Logout blieb es stehen, waehrend
    /// `state.client` schon `None` war, und der Check bestand trotzdem.
    /// **Neue Regel: fuer "gehoert diese verspaetete Antwort noch zur
    /// aktiven Sitzung"-Pruefungen NIE dieses Feld allein verwenden —
    /// IMMER `session_epoch` vergleichen (siehe dort), die bei JEDER
    /// Sitzungsaenderung inklusive Logout steigt.**
    authenticated_pilot_id: Mutex<Option<i64>>,
    /// Monoton steigender Zaehler: JEDE Sitzungsaenderung (Login, Logout,
    /// Load-Session — unabhaengig davon ob derselbe oder ein anderer
    /// Pilot betroffen ist) erhoeht ihn EINMAL, synchron, als fester
    /// Bestandteil derselben atomaren Schreiboperation wie `client`/
    /// `authenticated_pilot_id` (siehe `setze_session_atomar`/
    /// `leere_session_atomar`).
    ///
    /// Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde): nach
    /// DREI Runden (10-12) mit Einzel-Patches gegen denselben MQTT-
    /// Lebenszyklus-Bereich, darunter zwei echte WIDERSPRUECHE zwischen
    /// fruheren Fixes (Runde 11s "MQTT vor fehlschlagfaehigen Schritten
    /// stoppen" widersprach Runde 10/12s Annahme, ein API-Key-Wechsel sei
    /// als Abbruch-Signal verlaesslich), war ein Vergleich einzelner
    /// Felder (API-Key, Piloten-ID) gegen sich selbst nicht mehr
    /// tragfaehig. `session_epoch` ist das EINZIGE Signal, das
    /// Hintergrund-Tasks (MQTT-Provisionierung, Profil-Refresh, o.ae.)
    /// noch pruefen muessen: haben sie ihre eigene Start-Epoche notiert
    /// und weicht die AKTUELLE Epoche unmittelbar vor jeder Mutation
    /// davon ab, ist die Sitzung nicht mehr dieselbe — unabhaengig davon,
    /// WAS sich geaendert hat oder ob eine fehlschlagfaehige
    /// Nebenoperation (Keyring, Datei) schon durchgelaufen ist.
    session_epoch: Mutex<u64>,
    /// Nachgereicht (externe Gegenpruefung Codex, adversarial, waehrend
    /// der v1.7.17-QS-Runde entdeckt): ein reines Lifecycle-Schloss fuer
    /// die EINZIGE Aktiv-Flug-Ablage (`active_flight_path` — ein fester
    /// Pfad, kein Datei-pro-PIREP). Ohne dieses Schloss ist jedes
    /// `if !flight.stop.load(...) { save_active_flight(...) }` ein
    /// Time-of-Check-to-Time-of-Use-Rennen: zwischen dem Lesen von `stop`
    /// und dem tatsaechlichen Schreiben kann `flight_end`/`flight_cancel`/
    /// `flight_forget` dazwischenfunken, die Ablage loeschen — und der
    /// zuvor schon "erlaubte" Schreibzugriff wuerde sie wiederbeleben,
    /// oder schlimmer: die Ablage eines INZWISCHEN gestarteten neuen
    /// Fluges ueberschreiben (derselbe feste Pfad). `save_active_flight`
    /// und `teardown_active_flight_persistence` sind die EINZIGEN beiden
    /// Stellen, die dieses Schloss nehmen — Pruefen und Schreiben (bzw.
    /// Setzen-und-Loeschen) passiert in JEWEILS EINEM kritischen
    /// Abschnitt, nie ueber zwei getrennte Sperren hinweg.
    persistence_lock: Mutex<()>,
    /// Serialisiert Lese-Aendere-Schreibe-Zyklen auf die
    /// `pending_bid_cleanup.json`-Datei (`enqueue_pending_bid_cleanup` und
    /// `drain_pending_bid_cleanup`). Codex-Folgefund (adversarial,
    /// 05.09.2026, zehnte Runde): beide griffen bisher UNSYNCHRONISIERT
    /// auf dieselbe Datei zu — enqueued ein `flight_end`/`flight_end_
    /// manual` waehrend der Background-Worker gerade zwischen seinem
    /// `read_all()` und seinem `replace(&survivors)` steckt, ueberschreibt
    /// der Worker die frisch hinzugekommene Zeile wieder, ohne sie je
    /// verarbeitet zu haben — der Bid bliebe unbemerkt fuer immer
    /// reserviert.
    pending_bid_cleanup_lock: tokio::sync::Mutex<()>,
    /// Ring buffer of pilot-visible activity events. Surfaced via the
    /// `activity_log_get` Tauri command; the dashboard renders them in
    /// the new "ACARS-Log" tab — same idea as the smartcars activity
    /// feed (login → bids loaded → prefiled → boarding → …).
    activity_log: Mutex<VecDeque<ActivityEntry>>,
    /// Atomic guard for `flight_start` / `flight_adopt`. Both functions await
    /// network calls *between* checking that no flight is active and writing
    /// the new ActiveFlight into state. Without this guard, two concurrent
    /// invokes (StrictMode double-mount, double-click, resume-banner re-render)
    /// would both pass the initial check and the second would silently
    /// overwrite the first — losing the streamer reference and leaving
    /// phpVMS with two adopt attempts. Acquire with `compare_exchange`,
    /// release in *every* exit path (success and error).
    flight_setup_in_progress: AtomicBool,
    /// In-process airport-coords cache. Keyed by ICAO uppercase. Populated on
    /// first lookup so we don't re-fetch on every snapshot tick.
    airports: Mutex<HashMap<String, Airport>>,
    /// v0.8.0 — VPS-managed aircraft-type-aliases. Refreshed on each
    /// flight_start (so a freshly-added alias takes effect at the next
    /// Flight without app-restart) AND lazily warmed by the auto-start
    /// watcher when empty (v0.15.16 — vorher war der Cache vor dem ersten
    /// Flug leer, sodass der Auto-Start-Typcheck aliased Flugzeuge wie
    /// E55P↔"PHENOM 300" nie matchte).
    /// Merged additively with the hardcoded `aircraft_aliases` table —
    /// VPS-Liste wins on conflict (= VA-Admin kann fehlerhaft hardcoded
    /// Aliases übersteuern). Empty when VPS unreachable or pre-fetch
    /// not yet completed.
    /// HashMap<bid-ICAO uppercase, Vec<alias substring uppercase>>.
    aircraft_aliases_vps: Mutex<HashMap<String, Vec<String>>>,
    /// Auto-start watcher state. When true, a background task polls
    /// `current_snapshot()` and the user's bids; if the aircraft is
    /// parked at one of the bid's departure airports AND the loaded
    /// aircraft matches the bid's planned aircraft, it auto-fires
    /// `flight_start` for that bid. Persisted via the same
    /// `aeroacars.autoStart` localStorage key the React side reads.
    auto_start_enabled: AtomicBool,
    /// v0.12.6: Auto-File-Setting, vom Frontend (localStorage) bei Mount
    /// + Toggle via `set_auto_file_enabled` in den Backend-State
    /// gespiegelt. Der Position-Streamer filet den PIREP beim Latch auf
    /// `Arrived` selbst, wenn dieses Flag an ist — fenster-unabhängig.
    /// Vorher lief Auto-File nur als Frontend-`useEffect`, der bei
    /// Hintergrund-Fenster / nicht-aktivem Cockpit-Tab nicht feuerte.
    /// Default `true` (in `.setup()` gesetzt) — passend zum Frontend.
    auto_file_enabled: AtomicBool,
    /// Marker so the watcher doesn't re-trigger after a successful
    /// auto-start while the resulting flight is still active. Set to
    /// the Bid ID we last fired for; cleared when the flight ends.
    /// Without this, a user that cancels the auto-started flight
    /// while still parked at the gate would get an instant re-trigger.
    auto_start_last_bid_id: Mutex<Option<i64>>,
    /// v0.3.0: Letzter Auto-Start-Skip-Grund. Wenn die UI fragt, wir
    /// den ausgeben können statt der Pilot grübelt warum nichts
    /// passiert. Format: "engines_on" / "moving" / "airborne" /
    /// "no_matching_bid" / "no_bids" — mit Timestamp damit die UI
    /// erkennen kann ob's gerade aktuell ist oder uralt.
    auto_start_skip_reason: Mutex<Option<(DateTime<Utc>, String)>>,
    /// v0.15.13: letzter fehlgeschlagener Auto-Start-Versuch
    /// `(Zeitpunkt, bid_id, error_code)`. Verhindert den Retry-Sturm: nach
    /// einem `flight_start`-Fehler feuert der Watcher denselben Bid erst nach
    /// `AUTO_START_FAIL_COOLDOWN_SECS` wieder (vorher: 3-s-Dauer-Retry, der
    /// GlitchTip mit `missing_aircraft` / `flight_already_active` flutete —
    /// #10, 189×). Drosselt zugleich das Activity-/GlitchTip-Log, da der
    /// Fehler-Pfad nur noch ~1×/Cooldown durchlaufen wird. Selbst-korrigierend:
    /// nach dem Cooldown versucht der Watcher erneut (Pilot kann inzwischen das
    /// Flugzeug zuweisen / Sim fertig laden).
    auto_start_fail: Mutex<Option<(DateTime<Utc>, i64, String)>>,
    /// Bid, dessen `flight_start` der AUTO-START gerade ausfuehrt. Eigener
    /// Marker statt `flight_setup_in_progress`: jenes ist ein globaler
    /// Lebenszyklus-Riegel, den auch manueller Start, Uebernahme und
    /// Wiederherstellung setzen — daraus liess sich nicht ablesen, WELCHER
    /// Start laeuft (Codex-Abnahme 22.09.2026). Gesetzt beim Claim, geloescht,
    /// sobald `flight_start_mit` zurueckkehrt.
    auto_start_laeuft_bid: Mutex<Option<i64>>,
    /// When `true`, intercept the main window's CloseRequested event
    /// and `hide()` the window instead of letting it close. The user
    /// gets to it again via the system-tray icon (Win) / menubar
    /// item (Mac). When `false`, close behaves normally and the app
    /// quits. Toggle lives in Settings → Verhalten; persisted via
    /// the React side's `aeroacars.minimizeToTray` localStorage key
    /// and synced into here on every mount + change.
    minimize_to_tray_enabled: AtomicBool,
    /// v0.4.0: Cached pilot identity (e.g. `("GSG0001", "Thomas K")`)
    /// für Discord-Webhook-Posts. Wird beim Profile-Refresh gefüllt
    /// (`get_profile()` returns it), bleibt für die Lebenszeit der
    /// AeroACARS-Session. Wenn kein Profile geladen → `None`, der
    /// Discord-Embed fällt auf "AeroACARS Pilot" zurück.
    cached_pilot: Mutex<Option<(String, String)>>,
    /// v0.7.8 v1.5: gecachter Pilot-personalisierter Callsign-Suffix
    /// (`Profile.callsign`, z.B. "4TK"). Wird beim Login + jedem
    /// `phpvms_refresh_profile` aktualisiert. Von `flight_start` gelesen
    /// und in `ActiveFlight.pilot_callsign` durchgereicht fuer die
    /// SimBrief-direct-Match-Verifikation (Spec §6.1).
    cached_pilot_callsign: Mutex<Option<String>>,
    /// v0.5.11: MQTT live-tracking publisher handle. Connects to
    /// the aeroacars-live VPS (live.kant.ovh) via auto-provisioning
    /// the first time AeroACARS sees a logged-in pilot's API key.
    /// Pure background feature — pilot-invisible, no UI. Failure is
    /// non-fatal: AeroACARS works exactly as before if MQTT is
    /// unreachable. tokio::sync::Mutex (not std) because Handle is
    /// only ever accessed from async contexts (hook points run
    /// inside the streamer's async tasks).
    mqtt: tokio::sync::Mutex<Option<aeroacars_mqtt::Handle>>,
    /// `session_epoch`-Wert, zu dem der aktuell in `mqtt` installierte
    /// Handle provisioniert wurde (`None` = kein Handle).
    ///
    /// Codex-Folgefund (adversarial, 05.09.2026, elfte Runde): `mqtt`
    /// allein beantwortet nur „laeuft ueberhaupt einer", nicht „gehoert er
    /// der GERADE aktiven Sitzung".
    ///
    /// Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde): ein
    /// Vergleich gegen `authenticated_pilot_id` (wie urspruenglich hier)
    /// oder gegen den phpVMS-API-Key (wie zwischenzeitlich in der
    /// Provisionierung) ist KEIN verlaessliches Signal fuer "die Sitzung
    /// hat sich geaendert" — beide Werte werden bei Logout NICHT
    /// (`authenticated_pilot_id`, bewusst, siehe dortige Doku) oder erst
    /// SPAETER, nach fehlschlagfaehigen Schritten (API-Key, siehe Runde
    /// 11 vs. 10/12-Widerspruch) veraendert. Codex fand daraus zwei echte
    /// Widersprueche zwischen den Runden 10-12: ein Task, der noch den
    /// alten Wert sieht, haelt sich faelschlich fuer weiterhin gueltig.
    /// `session_epoch` loest das: sie steigt SYNCHRON und ALS ERSTES bei
    /// JEDER Sitzungsaenderung (Login, Logout, Load-Session) — ein
    /// Hintergrund-Task, der seine Start-Epoche mit der aktuellen
    /// vergleicht, erkennt JEDE Sitzungsaenderung sofort, unabhaengig
    /// davon, WAS sich geaendert hat oder ob eine fehlschlagfaehige
    /// Nebenoperation schon durchgelaufen ist.
    mqtt_owner_epoch: Mutex<Option<u64>>,
    /// v0.7.8: SimBrief-Identifier (Username + User-ID) fuer den
    /// SimBrief-direct OFP-Refresh-Pfad. Wenn mindestens eines gesetzt
    /// ist, kann `flight_refresh_simbrief` den neuen OFP direkt von
    /// simbrief.com holen ohne Bid-Pointer (= W5-Workaround).
    /// User-ID > Username Prioritaet wenn beide da.
    /// Persistenz: Frontend localStorage, Backend-State wird per
    /// `set_simbrief_settings` befuellt (mount + change) und beim
    /// App-Start/Login via App.tsx zurueck-gesynct.
    /// Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md §4.
    simbrief_settings: Mutex<SimBriefSettings>,
    /// v0.7.9: gesetzt vom letzten `flight_refresh_simbrief` wenn
    /// `DirectOutcome::MatchWithCallsignWarning` zurueckkam. Frontend
    /// rendert daraus einen gelben Warning-Notice "OFP geladen, aber
    /// Callsign weicht ab". Wird beim naechsten erfolgreichen Refresh
    /// (Match ohne Warning) oder durch expliziten Dismiss-Command
    /// geloescht.
    ofp_callsign_warning: Mutex<Option<OfpCallsignWarning>>,
    /// v0.16.0 (#LAN-Remote): handle to the running LAN remote-control
    /// server, or `None` while stopped. Holds the graceful-shutdown
    /// trigger + the bound port. Same `tokio::sync::Mutex<Option<…>>`
    /// pattern as `mqtt` above — only ever touched from async command
    /// contexts. Started/stopped via the `remote_server_*` commands;
    /// since v0.16.16 the toggle persists, so the setup hook auto-starts
    /// the server at boot when the pilot left it switched ON.
    remote_server: tokio::sync::Mutex<Option<remote::RemoteServerHandle>>,
    /// v0.16.0 (#LAN-Remote): always-live event tap. The three Tauri push
    /// emit sites (`integrity-flag`, `pirep_auto_filed`,
    /// `pirep_cancelled_remotely`) ALSO fan their payload out here; the WS
    /// handler subscribes so a paired tablet sees the same events. The bus
    /// exists even when the server is stopped (cheap no-op sends), so the
    /// emit sites never branch on server-running state. A `Default` newtype
    /// (broadcast::Sender has no `Default`) keeps `#[derive(Default)]` on
    /// AppState intact.
    remote_events: remote::RemoteEventBus,
    /// v1.3.0 (#Hoppie-PDC-CPDLC): handle to the running Hoppie ACARS
    /// poller, or `None` while stopped/not-connected. Same
    /// `tokio::sync::Mutex<Option<…>>` + start/stop-via-Drop pattern as
    /// `remote_server` above. Opt-in: only ever `Some` because the pilot
    /// enabled the feature AND called `hoppie_connect`.
    hoppie: tokio::sync::Mutex<Option<hoppie::HoppieHandle>>,
    /// v0.20 (Process-Integrity): whether the PREVIOUS AeroACARS run exited
    /// cleanly, per `run_sentinel_present()` checked BEFORE `write_run_sentinel`
    /// overwrites the sentinel for the current run. `None` until the setup
    /// hook determines it (should never observably stay `None` in practice —
    /// set once, early, at startup). Read by `try_resume_flight()` so the
    /// resulting `FlightResumed.previous_exit_clean` reflects the run that
    /// just ended, not the one currently starting.
    previous_run_exit_clean: Mutex<Option<bool>>,
    /// v1.8: Telemetrie-Monitor — Verlauf und ob gerade jemand zuschaut.
    telemetrie: telemetrie::Monitor,
}

/// v0.7.9: Warning-State wenn SimBrief-OFP DEP+ARR matched aber Callsign
/// abweicht. Statt Hard-Block (v0.7.8) wird der OFP geladen und der Pilot
/// sieht einen gelben Hinweis-Notice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfpCallsignWarning {
    /// SimBrief-Callsign aus dem geladenen OFP, z.B. "EWG4PY".
    pub sb_callsign: String,
    /// Joined-Liste der akzeptierten Active-Callsigns, z.B. "EWL9725 / 9725".
    pub active_callsigns: String,
    /// Zeitstempel wann das Warning gesetzt wurde.
    pub issued_at: DateTime<Utc>,
}

/// v0.7.8: SimBrief-Identifier-Settings. Beide Felder optional, aber
/// mindestens eines muss gesetzt sein damit SimBrief-direct funktioniert.
/// Wenn beide gefuellt → User-ID gewinnt (robuster, unveraenderlich).
/// Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md §4.1.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SimBriefSettings {
    /// SimBrief-Profile-Name (z.B. "thomaskant"). Sichtbar in
    /// simbrief.com/dashboard/?username=X.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// SimBrief-User-ID (numerisch als String, z.B. "612345").
    /// Aus SimBrief Account Settings. Robuster als Username weil
    /// unveraenderlich.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

/// RAII guard for `AppState::flight_setup_in_progress`. Acquire it at the
/// start of `flight_start` / `flight_adopt`; the in-progress flag is cleared
/// automatically when the guard goes out of scope (any return path), unless
/// `disarm()` was called to keep the slot reserved (e.g. because the new
/// ActiveFlight has been written into state and the `active_flight` mutex
/// now serves as the conflict guard).
struct FlightSetupGuard<'a> {
    flag: &'a AtomicBool,
    armed: bool,
}

impl<'a> FlightSetupGuard<'a> {
    fn try_acquire(flag: &'a AtomicBool) -> Result<Self, UiError> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| {
                UiError::new(
                    "flight_setup_in_progress",
                    "another flight start or adopt is already in progress — please wait",
                )
            })?;
        Ok(Self { flag, armed: true })
    }

    /// Tell the guard to NOT release the in-progress flag on drop. Use this
    /// once we've successfully written the new ActiveFlight into state — at
    /// that point the `active_flight` mutex is the source of truth.
    fn disarm(mut self) {
        self.armed = false;
        // Manually release the in-progress flag right now; from here the
        // `active_flight` mutex blocks duplicate setups.
        self.flag.store(false, Ordering::SeqCst);
    }
}

impl Drop for FlightSetupGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.flag.store(false, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
#[path = "tests_lib/flight_setup_guard_tests.rs"]
mod flight_setup_guard_tests;

/// In-memory record of an in-progress flight. Held inside an `Arc` so the
/// background streaming task can hold a reference without going through the
/// AppState mutex.
struct ActiveFlight {
    pirep_id: String,
    bid_id: i64,
    /// v0.7.7: phpVMS-Flight-Identifier aus `Bid.flight_id`. Sibling von
    /// `bid_id`. Wird beim `flight_start` aus dem Bid extrahiert BEVOR
    /// `prefile_pirep` feuert — sonst ist der Schluessel verloren wenn
    /// phpVMS-7 den Bid nach Prefile entfernt (W5). v0.7.8 nutzt den
    /// `flight_id` als zentralen Pointer fuer den OFP-Refresh-Pfad
    /// (entweder PAX-Studio-Endpoint /api/paxstudio/flights/{flight_id}/
    /// simbrief ODER SimBrief-direct mit Flight-Match-Verifikation).
    /// Empty string fuer pre-v0.7.7-Resume-Files — v0.7.8 muss diesen
    /// Fall als "legacy active flight, direct refresh unavailable"
    /// behandeln. Spec docs/spec/ofp-refresh-during-boarding.md §6.1.
    flight_id: String,
    /// v0.7.8 v1.5: `Bid.flight.callsign` falls phpVMS das fuellt
    /// (z.B. "CFG4TK"). Match-Kandidat 3 in
    /// `ofp_matches_active_flight`. None wenn phpVMS-VA das Feld nicht
    /// nutzt. Spec §6.1.2.
    bid_callsign: Option<String>,
    /// v0.7.8 v1.5: `Profile.callsign` des Piloten (= persoenliches
    /// Suffix wie "4TK"). Match-Kandidaten 4 ("CFG4TK") + 5 ("4TK") in
    /// `ofp_matches_active_flight`. None wenn pre-2024-phpVMS-Profile
    /// ohne callsign-Feld. Spec §6.1.2.
    pilot_callsign: Option<String>,
    started_at: DateTime<Utc>,
    /// ICAO of the operating airline (e.g. "DLH"). Combined with
    /// `flight_number` to produce the full callsign on the dashboard
    /// ("DLH155" instead of just "155"). Empty string when unknown.
    airline_icao: String,
    /// v0.4.0: phpVMS-hosted Airline-Logo-URL (von `bid.flight.airline.logo`,
    /// z.B. `https://german-sky-group.eu/storage/uploads/airlines/4/.../logo.png`).
    /// Discord-Webhook-Embeds nutzen das als großes Bild unten —
    /// Pilot sieht in seinem Channel das Logo der gerade geflogenen
    /// Airline ohne dass wir es im Repo selbst hosten müssen.
    /// `None` wenn die VA das Logo-Feld nicht gepflegt hat.
    airline_logo_url: Option<String>,
    /// Registration phpVMS assigned to this flight (e.g. "D-AIUV").
    /// Looked up via `get_aircraft(bid.flight.aircraft_id)` at start
    /// time. Compared against the live `ATC ID` SimVar in the activity
    /// log so the pilot sees immediately if they loaded the wrong tail
    /// number in MSFS. Empty string when unknown (fresh-PIREP / disk-
    /// resume edge cases where we couldn't match a bid).
    planned_registration: String,
    /// Aircraft ICAO der Bid (z.B. "B738"). Wird im PIREP-Custom-Field
    /// "Aircraft Type" rausgegeben damit der VA-Admin auf der phpVMS-
    /// Detail-Seite ohne extra Lookup weiß was geflogen wurde.
    /// v0.3.0: vorher nur `planned_registration`, neuer `aircraft_icao`
    /// kommt aus dem gleichen `client.get_aircraft(id)`-Call.
    aircraft_icao: String,
    /// Aircraft-Name (z.B. "Boeing 737-800"). Komplettiert die ICAO-
    /// Anzeige. Empty wenn nicht ermittelbar.
    aircraft_name: String,
    flight_number: String,
    dpt_airport: String,
    arr_airport: String,
    /// v1.5.3 (#ifly-audit, ETE-Deckel): geplante Flugzeit aus phpVMS
    /// (`bid.flight.flight_time`, Minuten). None bei Adopt/Resume (nicht
    /// persistiert) oder wenn phpVMS keine pflegt — dann zeigt die
    /// ETE-Zelle wie bisher ab dem ersten plausiblen Wert.
    planned_flight_time_min: Option<i32>,
    /// v1.7.32: geplante Streckenlaenge aus phpVMS (`bid.flight.distance`,
    /// NM). Grundlage der Mehrweg-Gutschrift auf der OFP-Achse — siehe
    /// `landing_scoring::sub_fuel`s Regelblock. `None` bei Adopt/Resume
    /// (kein Bid-Kontext) oder wenn phpVMS keine pflegt; dann gibt es
    /// keine Gutschrift und die Achse rechnet wie zuvor.
    plan_strecke_nm: Option<f64>,
    /// Final loads (per fare-class id) captured at flight start so we can
    /// include them in the filed PIREP — even if the bid is gone by then.
    fares: Vec<(i64, i32)>,
    /// Mutable running stats updated by the streamer task.
    stats: stats_sperre::GemesseneSperre<FlightStats>,
    stop: AtomicBool,
    /// True until the resume banner is dismissed (confirmed or cancelled).
    /// Drives the resume modal on the dashboard.
    was_just_resumed: AtomicBool,
    /// Compare-and-swap guard for `spawn_position_streamer`. Several UI paths
    /// (resume confirm, StrictMode double-mount, retry on transient error)
    /// could end up calling spawn more than once for the same ActiveFlight;
    /// this flag ensures only ONE streamer task is ever live per flight.
    streamer_spawned: AtomicBool,
    /// Set once we've detected that phpVMS soft-deleted the PIREP under us
    /// (any live POST endpoint returning 404). Idempotency guard so the
    /// "PIREP cancelled remotely" activity entry / UI event fire exactly
    /// once even if positions and the heartbeat both get 404s in the
    /// same cycle.
    cancelled_remotely: AtomicBool,
    /// v0.6.0: Memory-Outbox für phpVMS-Position-POSTs. Streamer-Tick
    /// pusht jede Position rein (= ein schneller Mutex-Push, blockt nie).
    /// `spawn_phpvms_position_worker` zieht periodisch raus und batched.
    /// Bei Quit wird der Inhalt ins file-backed `position_queue.json`
    /// persistiert damit ein Restart die ungesendeten Punkte nachholt.
    /// **Ersetzt das alte Critical-Window + Drain-Pattern komplett.**
    position_outbox: Mutex<std::collections::VecDeque<api_client::PositionEntry>>,
    /// Beim Stopp offene Positionen noch an phpVMS senden — nur beim
    /// normalen Flugende (`flight_end`, `flight_end_manual`). Bei Abbruch,
    /// Verwerfen oder serverseitig geloeschtem PIREP bleibt es aus.
    positionen_nachsenden: AtomicBool,
    /// Generation von Positions-Worker und Streamer. Jeder Start merkt sich
    /// die aktuelle; `restore_flight_for_retry` zaehlt sie hoch. Eine
    /// ueberholte Aufgabe endet, ohne die Outbox zu leeren oder Punkte zu
    /// verwerfen — der neue Worker besitzt sie (Codex-Abnahme 06c72335:
    /// ohne das liefen nach schnellem Fortsetzen zwei Worker und zwei
    /// Streamer auf demselben Flug).
    generation: std::sync::atomic::AtomicU64,
    /// Haelt Starten (`startsperre_nehmen`) und Fortsetzen
    /// (`restore_flight_for_retry`) auseinander: Beides sieht so entweder den
    /// Stand vor oder nach dem anderen, nie eine Mischung (Codex-Abnahme
    /// cd149ec1 — sonst konnte ein spaeter zweiter Starter eine Aufgabe der
    /// neuen Generation doppelt starten).
    start_sperre: Mutex<()>,
    /// v0.6.0: Guard fuer `spawn_phpvms_position_worker` (analog zu
    /// streamer_spawned). Wird im worker-loop selbst gesetzt damit
    /// nicht mehrere parallele Worker laufen.
    phpvms_worker_spawned: AtomicBool,
    /// v0.18.x: Guard fuer `spawn_touchdown_sampler` (analog zu
    /// streamer_spawned/phpvms_worker_spawned). Fehlte bisher — `flight_
    /// resume_confirm` ist ueber die LAN-Fernbedienung (remote/bridge.rs)
    /// erreichbar, und der Frontend-Doppelklick-Schutz (`confirmingRef` in
    /// ResumeFlightBanner.tsx) lebt nur im Hauptfenster, NICHT im separaten
    /// React-Baum der Remote-Weboberflaeche auf einem Tablet/2. PC. Ein
    /// Pilot mit aktivierter Fernbedienung (Thomas K., 05.07.2026) haette
    /// so zwei parallele Touchdown-Sampler fuer denselben Flug bekommen
    /// koennen — jeder mit eigenem lokalem `prev_in_air`-Zustand, beide
    /// schreiben unkoordiniert in dieselben `stats`-Felder (peak_vs_fpm,
    /// sampler_touchdown_at, …). Ohne diesen Guard waere das der einzige
    /// der drei Resume-Spawns ohne Schutz geblieben.
    touchdown_sampler_spawned: AtomicBool,
    /// v0.6.2 — Connection-Health-State zum phpVMS-Server. Vom phpVMS-
    /// Worker gesetzt nach jedem POST:
    ///   - 0 = Live (letzter POST war Erfolg)
    ///   - 1 = Failing (letzter POST hat geschlagen / timeout)
    /// Frontend nutzt das zusammen mit `queued_position_count` für eine
    /// klare 3-Status-Indikator-Anzeige (Live / Sync / Offline) — vor
    /// v0.6.2 zeigte der Indikator „queued offline" für jeden Backlog,
    /// auch ohne echten Connection-Loss → Pilot-Verwirrung.
    connection_state: std::sync::atomic::AtomicU8,

    /// v0.8.0 VPS-Navdata — geladene Airport-Daten (dep/arr/alt) für
    /// die Runway-Awareness-Features. Beim Flugstart parallel vom VPS
    /// gezogen (`fetch_navdata_for_flight`). Cache-Lifetime = Lifetime
    /// des aktiven Flugs (= bis PIREP filed). HashMap-Key ist das ICAO
    /// in UPPERCASE. Empty wenn VPS nicht erreichbar — die
    /// LandingRecord-Felder bleiben dann None und der Pilot-Client
    /// fällt automatisch auf OurAirports zurück
    /// (`runway::lookup_runway_with_fallback`).
    ///
    /// Aktiver Cycle steht zusätzlich in `navdata_cycle` — wird ins
    /// Activity-Log + TouchdownPayload weitergegeben.
    navdata: Mutex<NavdataCache>,
}

/// Pro-Flug-Cache der NavAirport-Daten. Default = leer = OurAirports-
/// Fallback aktiv für alle ICAOs.
#[derive(Debug, Default)]
struct NavdataCache {
    /// ICAO (uppercase) → NavAirport.
    airports: std::collections::HashMap<String, aeroacars_mqtt::navdata::NavAirport>,
    /// AIRAC-Cycle ("2604"). Wird beim ersten erfolgreichen Fetch
    /// gesetzt — alle weiteren Fetches im selben Flug schreiben denselben
    /// Wert (Recorder garantiert dass GET /airport/X dieselbe Cycle-ID
    /// liefert wie der zuletzt aktivierte Cycle).
    cycle: Option<String>,
}

impl NavdataCache {
    fn get(&self, icao: &str) -> Option<&aeroacars_mqtt::navdata::NavAirport> {
        self.airports.get(&icao.trim().to_uppercase())
    }

    /// The airport's reference point from the active AIRAC cycle.
    ///
    /// v0.19.3: this is the FIRST source we ask for "where is this airport"
    /// (see `airport_reference_pos`) — Navigraph, refreshed every 28 days,
    /// and the only one of our three sources that is actually maintained for
    /// flying. We were carrying it in this cache all along and not using it.
    fn reference_pos(&self, icao: &str) -> Option<(f64, f64)> {
        let a = self.get(icao)?;
        // v0.19.3: Navigraph sits at the TOP of the cascade and short-circuits
        // everything below it — so one bad row (or a serde default) would put the
        // destination at Null Island for the whole flight, and at the ~6,400
        // fields with no runway geometry that means a false divert AND a refused
        // filing for every pilot who flies there. The phpVMS arm has always
        // filtered this; the better source deserves the same scepticism.
        if !a.latitude.is_finite() || !a.longitude.is_finite() {
            return None;
        }
        if a.latitude == 0.0 && a.longitude == 0.0 {
            return None;
        }
        Some((a.latitude, a.longitude))
    }
}
