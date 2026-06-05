//! v0.9.0 (#GlitchTip) — Sentry-Init fuer den Tauri-Client.
//!
//! Spec: docs/spec/v0.9.0-glitchtip-self-hosted.md
//!     + docs/spec/v0.9.0-telemetry-contract.md Sektion 9 (Datenschutz-Gates)
//!
//! Init-Strategie:
//!   - DSN aus build-time env `AEROACARS_SENTRY_DSN` (siehe build.rs).
//!     Wenn leer → kein Init, alle Calls degradieren zu No-Ops.
//!   - Opt-In: Pilot muss explizit zustimmen (Settings-UI + First-Run-
//!     Banner). Bis dann: kein Event geht raus. Default = aus.
//!   - release tag: `aeroacars-client@${CARGO_PKG_VERSION}`
//!
//! Anonymisierung (Allowlist + Redaction):
//!   - `before_send`-Hook strippt alle nicht-allowlisted Tags
//!   - User-PII (email, username, ip_address) → None
//!   - Request-Daten (headers, body, query) komplett weg
//!   - Stack-Frame-Vars weg
//!
//! Privacy-Toggle:
//!   - `set_consent(true|false)` setzt globale Atomic, vor jedem
//!     before_send-Call gepruef. So kann der Pilot live ein/ausschalten,
//!     ohne dass wir Sentry restarten muessen — auch wenn Sentry
//!     re-init wird der DSN beim Re-init reagiert.

use sentry::protocol::{Event, Value};
use sentry::Client;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Pilot-Consent-Gate. Default `false`. Wird von der UI via Tauri-Command
/// `set_error_reporting_consent` gesetzt. Bei `false` wird jedes Event
/// im before_send verworfen.
static CONSENT: AtomicBool = AtomicBool::new(false);

/// Eigene Referenz auf den Sentry-Client. Damit kontrollieren wir den
/// Lifecycle selbst — nicht via `ClientInitGuard`-im-OnceLock (das hielte
/// den Client auch nach `bind_client(None)` weiter am Leben → Transport
/// wuerde pending events doch noch drainen, DS7 nicht erfuellt — QS-Fund
/// F13 in v0.9.1).
///
/// On opt-out: `client.close(Some(ZERO))` signalisiert dem Transport,
/// die Pending-Queue NICHT zu drainen, sondern aufzugeben. Danach
/// `bind_client(None)` + `.take()` der Mutex-Inhalts → Arc-Refcount
/// faellt auf 0 → Transport-Worker-Thread terminiert.
///
/// On opt-in: neuer `Client::from(options)`, `bind_client(Some(...))`,
/// in den Mutex schreiben. Damit ist Re-Opt-In im selben App-Run sauber
/// (F12) UND beim Opt-Out wird der Transport echt zerlegt (F13).
static SENTRY_CLIENT: OnceLock<Mutex<Option<Arc<Client>>>> = OnceLock::new();

fn client_slot() -> &'static Mutex<Option<Arc<Client>>> {
    SENTRY_CLIENT.get_or_init(|| Mutex::new(None))
}

/// Tag-Allowlist (Telemetry-Contract Sektion 1, Client-Subset).
fn allowed_tag_keys() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut s = HashSet::new();
        s.insert("app.component");
        s.insert("app.version");
        s.insert("os");
        s.insert("os.version");
        s.insert("simulator");
        s.insert("aircraft");
        s.insert("airport");
        s.insert("runway");
        s.insert("pirep.id");
        s.insert("callsign");
        // v0.9.0 QS-Hotfix: "route" entfernt — Privacy-Hint sagt "Route NICHT
        // gesendet", also auch nicht erlauben. War ohnehin von keinem Code-Pfad
        // gesetzt (Spec-future placeholder).
        s.insert("phase");
        s.insert("pilot.hash");
        s.insert("forensics.version");
        s.insert("error.code");
        s.insert("error.status_code");
        s.insert("error.kind");
        s.insert("feature.flag");
        s.insert("distance.bucket");
        s
    })
}

/// Baut die ClientOptions — wird sowohl im initial-init() als auch beim
/// Re-Opt-In (rebind_client) genutzt damit Sentry-Re-Init im selben App-Lauf
/// moeglich ist (siehe set_consent). Returns None wenn keine DSN konfiguriert.
fn build_options() -> Option<sentry::ClientOptions> {
    let dsn = option_env!("AEROACARS_SENTRY_DSN").unwrap_or("").trim();
    if dsn.is_empty() {
        return None;
    }
    let release = format!("aeroacars-client@{}", env!("CARGO_PKG_VERSION"));
    let environment = if cfg!(debug_assertions) {
        "development"
    } else {
        "production"
    };
    Some(sentry::ClientOptions {
        dsn: dsn.parse().ok(),
        release: Some(release.into()),
        environment: Some(environment.into()),
        send_default_pii: false,
        traces_sample_rate: 0.0,
        attach_stacktrace: true,
        before_send: Some(std::sync::Arc::new(|event| {
            // Consent-Gate
            if !CONSENT.load(Ordering::Relaxed) {
                return None;
            }
            Some(redact_event(event))
        })),
        ..Default::default()
    })
}

/// Initialisiert Sentry. No-Op wenn DSN nicht in env zur Build-Zeit gesetzt war.
///
/// MUSS frueh in `lib.rs::run()` aufgerufen werden, vor dem Tauri-Builder,
/// damit auch Bootstrap-Panics gefangen werden — selbst BEVOR der Pilot in
/// den Settings Consent gibt. Das Consent-Atomic ist initial false → before_send
/// verwirft alle events trotzdem; sobald der Pilot Consent gibt, drift der
/// Atomic auf true, der Client laeuft seit Boot, alles ist sofort scharf.
pub fn init() {
    create_and_bind();
}

/// Baut einen frischen Client + bindet ihn an den Hub + setzt Initial-Scope-Tags +
/// speichert die Arc in unserem SENTRY_CLIENT-Slot. Wird sowohl beim Boot (init)
/// als auch beim Re-Opt-In nach vorherigem Opt-Out gerufen (set_consent(true)).
///
/// Returns true wenn ein Client erfolgreich erstellt wurde, false wenn keine
/// DSN konfiguriert ist (= leere AEROACARS_SENTRY_DSN-env zur Build-Zeit) ODER
/// ein Client bereits gebunden ist (Idempotenz).
fn create_and_bind() -> bool {
    let mut slot = client_slot().lock().expect("sentry client_slot poisoned");
    if slot.is_some() {
        // schon gebunden — Boot-Init UND fruehzeitiger Consent-Toggle koennten beide
        // hier landen; zweiter Caller wird zur No-Op damit kein zweiter Transport-
        // Worker entsteht.
        return true;
    }
    let Some(options) = build_options() else {
        tracing::info!("[sentry] no DSN configured (AEROACARS_SENTRY_DSN at build-time); skipping init");
        return false;
    };
    let release_tag = options.release.as_ref().map(|c| c.to_string()).unwrap_or_default();

    // QS-Hotfix F17 (Runde 6): apply_defaults() MUSS vor Client::from() laufen.
    // sentry::init() macht das normalerweise hinter den Kulissen, aber wir
    // bauen den Client manuell (= F13-Lifecycle ohne ClientInitGuard).
    // apply_defaults setzt:
    //   - Default-Transport (reqwest-basiert) — ohne den hat der Client
    //     KEINEN Transport und Events gehen nicht raus
    //   - Default-Integrations (Panic, Backtrace, Contexts, …)
    //   - Env/Proxy-Defaults
    // Ohne diesen Call laeuft alles ins Leere — der Client ist "bound"
    // aber stumm. Vorherige F13-Runde hatte das verpasst.
    let options = sentry::apply_defaults(options);

    let client = Arc::new(Client::from(options));
    // Sanity-Check: Transport vorhanden? client.is_enabled() returnt true
    // nur wenn DSN gesetzt UND Transport gebaut wurde. False heisst No-Op-
    // Modus — irgendwas mit der Build-Konfig stimmt nicht (z.B. DSN-Parse
    // fehlgeschlagen, oder apply_defaults konnte keinen Transport bauen weil
    // sentry-feature `reqwest` nicht aktiv ist).
    let enabled = client.is_enabled();
    sentry::Hub::current().bind_client(Some(Arc::clone(&client)));
    *slot = Some(client);
    drop(slot);

    // Initial-Scope: Komponenten-Tags. MUSS nach bind_client kommen weil
    // configure_scope den aktuell gebundenen Client+Hub addressiert.
    sentry::configure_scope(|scope| {
        scope.set_tag("app.component", "client");
        scope.set_tag("app.version", env!("CARGO_PKG_VERSION"));
        scope.set_tag("os", std::env::consts::OS);
    });
    tracing::info!(
        "[sentry] client bound for release {} (enabled={})",
        release_tag, enabled
    );
    if !enabled {
        tracing::warn!(
            "[sentry] client.is_enabled() == false — DSN or transport problem; \
             events will not be sent. Check AEROACARS_SENTRY_DSN at build time."
        );
    }
    enabled
}

/// Setzt den Pilot-Consent. Symmetrisch beide Richtungen — Re-Opt-In im
/// selben App-Run funktioniert ohne Neustart.
///
/// **Opt-Out (`enabled=false`):** 3-stufiger Hard-Stop
///   1. `CONSENT.store(false)` — Atomic-Gate, jeder kuenftige before_send-
///      Aufruf returnt None
///   2. `client.close(Some(ZERO))` — signalisiert dem Transport-Worker dass
///      er die Pending-Queue NICHT mehr drainen darf. close(ZERO) wartet
///      nicht auf Flush — die Queue wird verworfen, in-flight HTTP-Request
///      laeuft noch zu Ende (Netz-Limit) aber kein neuer wird mehr gestartet.
///   3. `Hub::bind_client(None)` + `slot.take()` — sowohl Hub als auch unser
///      eigener Arc-Slot lassen den Client los; Arc-Refcount faellt auf 0;
///      Client wird gedroppt; Transport-Worker-Thread terminiert sauber.
///   Damit ist DS7 hart erfuellt: "ab Klick geht nichts mehr Neues raus,
///   pending Buffer wird verworfen statt gesendet".
///
/// **Opt-In (`enabled=true`):** create_and_bind() — wenn noch kein Client
/// gebunden ist (= nach vorherigem Opt-Out), wird ein frischer Client gebaut
/// und gebunden. Idempotent: wenn schon einer da ist (= Boot-Init oder
/// noch nie opt-out), no-op.
///
/// QS-Hotfix Verlauf v0.9.x dieser Funktion:
///   - v0.9.0 initial: rief flush() bei Opt-Out — schickte pending Events
///     AKTIV raus, exakt das Gegenteil von Opt-Out.
///   - v0.9.0 Runde 1 (F3): flush()-Call entfernt.
///   - v0.9.1 Runde 2 (F9): zusaetzlich Hub::bind_client(None).
///   - v0.9.1 Runde 3 (F12): symmetrischer Re-Init bei Opt-In.
///   - v0.9.1 Runde 4 (F13): SENTRY_GUARD (= ClientInitGuard im OnceLock)
///     hielt eine Arc-Referenz am Leben, sodass `bind_client(None)` allein
///     den Client NICHT gedroppt hat (Transport drainte pending events
///     trotzdem). Komplette Lifecycle-Umstellung: eigene Mutex<Option<Arc<Client>>>
///     + `client.close(ZERO)` + `slot.take()` damit Refcount wirklich auf 0
///     faellt. Jetzt ist DS7 belastbar.
pub fn set_consent(enabled: bool) {
    CONSENT.store(enabled, Ordering::Relaxed);
    tracing::info!("[sentry] consent set to {}", enabled);
    if enabled {
        // No-op wenn Client schon gebunden (Boot-Init), Re-Init wenn nicht.
        create_and_bind();
    } else {
        // 3-stufiger Hard-Stop. Reihenfolge wichtig:
        //  1) Client aus unserem Slot rausziehen (= unsere Arc-Referenz weg)
        //  2) Transport via close(ZERO) signalisieren → Pending-Queue wird
        //     verworfen, in-flight HTTP laeuft noch zu Ende
        //  3) Hub::bind_client(None) → Hub haelt auch keine Referenz mehr
        // Nach diesen 3 Schritten ist Arc-Refcount = 0 → Drop → Transport-
        // Worker-Thread terminiert.
        let to_close = {
            let mut slot = client_slot().lock().expect("sentry client_slot poisoned");
            slot.take()
        };
        if let Some(client) = to_close {
            client.close(Some(std::time::Duration::from_millis(0)));
        }
        sentry::Hub::current().bind_client(None);
    }
}

/// Anonymisiert ein Event: Tags-Allowlist + User-PII + Request-Daten + Frame-Vars.
pub fn redact_event(mut event: Event<'static>) -> Event<'static> {
    // 1. Tag-Allowlist
    let allowed = allowed_tag_keys();
    event.tags.retain(|k, _| allowed.contains(k.as_str()));

    // 2. User-PII (id nur wenn Hash-Format)
    if let Some(user) = event.user.as_mut() {
        let id_ok = user
            .id
            .as_ref()
            .map(|id| id.chars().all(|c| c.is_ascii_hexdigit()) && id.len() <= 16)
            .unwrap_or(false);
        if !id_ok {
            user.id = None;
        }
        user.email = None;
        user.username = None;
        user.ip_address = None;
        // Sonstige beliebige Felder weg
        user.other.clear();
    }

    // 3. Request-Daten: alles weg ausser method + url-ohne-query
    if let Some(req) = event.request.as_mut() {
        req.cookies = None;
        req.data = None;
        req.headers.clear();
        req.env.clear();
        req.query_string = None;
        if let Some(url) = req.url.as_ref() {
            // query-string raus
            let url_str = url.as_str();
            if let Some(idx) = url_str.find('?') {
                if let Ok(clean) = url_str[..idx].parse() {
                    req.url = Some(clean);
                }
            }
        }
    }

    // 4. Breadcrumbs: sensible data-keys redacten
    for crumb in event.breadcrumbs.iter_mut() {
        for (key, value) in crumb.data.iter_mut() {
            let lower = key.to_lowercase();
            if lower.contains("token")
                || lower.contains("password")
                || lower.contains("authorization")
                || lower.contains("cookie")
                || lower == "body"
                || lower == "request_body"
            {
                *value = Value::String("[REDACTED]".into());
            }
        }
    }

    // 5. Stack-Frame-Vars weg
    for ex in event.exception.iter_mut() {
        if let Some(st) = ex.stacktrace.as_mut() {
            for frame in st.frames.iter_mut() {
                frame.vars.clear();
            }
        }
    }
    for thread in event.threads.iter_mut() {
        if let Some(st) = thread.stacktrace.as_mut() {
            for frame in st.frames.iter_mut() {
                frame.vars.clear();
            }
        }
    }

    event
}

/// Convenience-Wrapper. No-Op wenn Sentry nicht init oder Consent aus.
#[allow(dead_code)]
pub fn capture_message(message: &str, level: sentry::Level) {
    sentry::capture_message(message, level);
}

/// Convenience-Wrapper. No-Op wenn Sentry nicht init oder Consent aus.
#[allow(dead_code)]
pub fn capture_error<E: std::error::Error + ?Sized>(err: &E) {
    sentry::capture_error(err);
}

/// v0.15.10: Schickt ein ACARS-Aktivitätsprotokoll-Event (Warn/Error) an
/// GlitchTip — so sehen wir PROBLEME im Feld (gescheiterte POSTs, OFP-/PIREP-
/// Fehler, Verbindungsabbrüche …), nicht nur Abstürze. No-Op ohne DSN/Consent.
/// Die `message` ist der STABILE Titel (sauberes Grouping in GlitchTip), das
/// `detail` (variable Werte) landet als Extra-Kontext. Tag `source=acars`
/// trennt diese Events von Rust-Panics.
#[allow(dead_code)]
pub fn capture_activity(message: &str, detail: Option<&str>, level: sentry::Level) {
    sentry::with_scope(
        |scope| {
            scope.set_tag("source", "acars");
            if let Some(d) = detail {
                scope.set_extra("detail", d.to_owned().into());
            }
        },
        || {
            sentry::capture_message(message, level);
        },
    );
}
