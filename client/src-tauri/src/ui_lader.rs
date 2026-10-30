//! v3 (Etappe 5): Lader der Oberflaeche ueber das eigene Protokoll `aeroui://`
//! (Pflichtenheft P-8.02, P-8.03, P-8.20).
//!
//! Quelle ist genau EINE je Programmlauf: `ui/` der aktiven Generation
//! (`laufzeit_paket::ui_paket`) oder, wenn dieser Lauf keine hat, das
//! eingebaute Frontend (`asset_resolver`, wie `remote/router.rs`). Fehlt eine
//! Datei im Paket oder passt ihr Hash nicht, kommt die Fehlerseite „Paket
//! defekt, eingebauter Stand aktiv“ — und ab dann gilt fuer diesen Lauf das
//! eingebaute Frontend (nie gemischt innerhalb einer Seite, P-3.02).
//!
//! Jede Antwort traegt die CSP aus `tauri.conf.json` als Header (ohne
//! `unsafe-eval`, nur die feste Hostliste). Ein eigenes Protokoll gilt in
//! Tauri als lokal (`webview/mod.rs` `is_local_url`): Die Rechte kommen also
//! aus der Capability des Fensters (`capabilities/ui.json`).
//!
//! Selbsttest (P-8.03-Messung, nur CI): Mit `AEROACARS_UI_SELBSTTEST=<datei>`
//! oeffnet das Programm nur das Fenster `ui` mit einer festen Testseite aus
//! `src/ui_lader_selbsttest/`, ueber denselben Paket-Weg wie eine echte
//! Generation; das Ergebnis schickt die Seite per GET an
//! `aeroui://localhost/__selbsttest/ergebnis?d=…`, der Lader schreibt es in die
//! Datei und beendet das Programm.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use runtime_bundle::{Eingebaut, Paket};
use sha2::{Digest, Sha256};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime, UriSchemeContext, UriSchemeResponder};

pub(crate) const SCHEMA: &str = "aeroui";

/// Adresse einer Seite der Oberflaeche. Auf Windows schreibt wry das selbst
/// auf `http://aeroui.localhost/…` um (wry webview2 `apply_uri_work_around`).
pub(crate) fn adresse(pfad: &str) -> tauri::Url {
    format!("{SCHEMA}://localhost/{}", pfad.trim_start_matches('/')).parse().expect("feste Adresse")
}

/// Einmal defekt = fuer den Rest des Laufs eingebaut.
static DEFEKT: AtomicBool = AtomicBool::new(false);

/// Selbsttest: Ergebnisdatei und Testpaket.
static SELBSTTEST: OnceLock<(PathBuf, Paket)> = OnceLock::new();

/// Eine ausgelieferte Datei.
pub(crate) struct Datei {
    pub bytes: Vec<u8>,
    pub mime: String,
    /// CSP fuer die Antwort (bei HTML aus dem eingebauten Frontend die von
    /// Tauri bearbeitete Fassung mit Hashes, sonst die aus tauri.conf.json).
    pub csp: Option<String>,
}

/// Der Paketpfad zu einem Anfragepfad: ohne fuehrenden `/`, Ordner = `index.html`.
fn relativ(pfad: &str) -> String {
    let p = pfad.split(['?', '#']).next().unwrap_or("").trim_start_matches('/');
    if p.is_empty() || p.ends_with('/') {
        format!("{p}index.html")
    } else {
        p.to_string()
    }
}

/// MIME-Typ nach Endung — gleich fuer Paket und eingebautes Frontend.
pub(crate) fn mime(pfad: &str) -> Option<&'static str> {
    let endung = pfad.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match endung.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        "pbf" => "application/x-protobuf",
        _ => return None,
    })
}

fn csp_aus_konfiguration<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    app.config().app.security.csp.as_ref().map(|c| c.to_string())
}

/// Das Paket, aus dem die Oberflaeche kommt — oder `None` = eingebaut.
fn paket() -> Option<&'static Paket> {
    if DEFEKT.load(Ordering::SeqCst) {
        return None;
    }
    SELBSTTEST.get().map(|(_, p)| p).or_else(crate::laufzeit_paket::ui_paket)
}

/// Eine Datei der Oberflaeche dieses Laufs (P-8.20: dieselbe Quelle fuer
/// Hauptfenster, Telemetrie, LAN-Webapp und HUD). `Err` = fehlt/defekt; war
/// die Quelle das Paket, gilt ab jetzt das eingebaute Frontend.
pub(crate) fn datei<R: Runtime>(app: &AppHandle<R>, pfad: &str) -> Result<Datei, String> {
    let rel = relativ(pfad);
    if let Some(p) = paket() {
        let im_paket = format!("ui/{rel}");
        return match p.datei(&im_paket) {
            Ok(bytes) => Ok(Datei {
                bytes,
                mime: mime(&rel).unwrap_or("application/octet-stream").to_string(),
                csp: csp_aus_konfiguration(app),
            }),
            Err(e) => {
                if macht_defekt(p, &im_paket) {
                    DEFEKT.store(true, Ordering::SeqCst);
                    tracing::error!(datei = %im_paket, error = %e, "Oberflaeche: Paket defekt, ab jetzt eingebauter Stand");
                }
                Err(format!("{im_paket}: {e}"))
            }
        };
    }
    let asset = app.asset_resolver().get(rel.clone()).ok_or_else(|| format!("{rel}: nicht eingebaut"))?;
    // Tauris Aufloeser liefert fuer Unbekanntes index.html (SPA). Eine Datei
    // mit fremder Endung, die als HTML zurueckkommt, fehlt also.
    let erwartet = mime(&rel);
    if asset.mime_type.starts_with("text/html") && erwartet.is_some_and(|m| !m.starts_with("text/html")) {
        return Err(format!("{rel}: nicht eingebaut"));
    }
    Ok(Datei {
        mime: erwartet.map(str::to_string).unwrap_or(asset.mime_type),
        csp: asset.csp_header.or_else(|| csp_aus_konfiguration(app)),
        bytes: asset.bytes,
    })
}

/// Defekt ist das Paket, wenn eine Datei aus seinem Manifest nicht lesbar ist
/// (Hash, Platte) oder wenn Inhalt der Oberflaeche fehlt, auf den eine Seite
/// verweist (HTML, Skript, Stil, WASM). Nicht: Anfragen, die der Browser von
/// sich aus stellt (`favicon.ico` bei WebView2, Messung P-8.03 29.10.2026).
fn macht_defekt(p: &Paket, im_paket: &str) -> bool {
    p.manifest.datei(im_paket).is_some()
        || matches!(im_paket.rsplit_once('.').map(|(_, e)| e), Some("html" | "js" | "mjs" | "css" | "wasm"))
}

/// Fehlerseite (P-8.01): ohne Skript, damit sie unter der CSP immer zeichnet.
pub(crate) fn fehlerseite(grund: &str) -> String {
    let grund = grund.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!doctype html><html lang=\"de\"><head><meta charset=\"utf-8\"><title>AeroACARS</title></head>\
<body style=\"font-family:system-ui,sans-serif;background:#0b1220;color:#e6edf6;padding:2rem\">\
<h1>Paket defekt, eingebauter Stand aktiv</h1>\
<p>Eine Datei der Oberfl&auml;che fehlt oder ist besch&auml;digt. Ab jetzt gilt bis zum n&auml;chsten Start der eingebaute Stand.</p>\
<p lang=\"en\">Interface package damaged — the built-in version is now active.</p>\
<p><a href=\"/\" style=\"color:#7cc4ff\">Neu laden / Reload</a></p>\
<pre style=\"opacity:.6\">{grund}</pre></body></html>"
    )
}

fn antwort(status: StatusCode, mime: &str, csp: Option<String>, bytes: Vec<u8>) -> Response<Vec<u8>> {
    let mut b = Response::builder().status(status).header(header::CONTENT_TYPE, mime);
    if let Some(csp) = csp {
        b = b.header("Content-Security-Policy", csp);
    }
    b.body(bytes).expect("Antwort")
}

/// Handler fuer `aeroui://` (in `lib.rs` am Builder angemeldet).
pub(crate) fn anfrage<R: Runtime>(ctx: UriSchemeContext<'_, R>, req: Request<Vec<u8>>, antworter: UriSchemeResponder) {
    let app = ctx.app_handle().clone();
    std::thread::spawn(move || antworter.respond(beantworten(&app, &req)));
}

fn beantworten<R: Runtime>(app: &AppHandle<R>, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let pfad = req.uri().path().to_string();
    if pfad == "/__selbsttest/ergebnis" {
        if let Some((ziel, _)) = SELBSTTEST.get() {
            let daten = tauri::Url::parse(&format!("x://h/?{}", req.uri().query().unwrap_or("")))
                .ok()
                .and_then(|u| u.query_pairs().find(|(k, _)| k == "d").map(|(_, v)| v.into_owned()))
                .unwrap_or_default();
            let ok = std::fs::write(ziel, daten).is_ok();
            tracing::info!(ziel = %ziel.display(), ok, "UI-Selbsttest: Ergebnis geschrieben");
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                app.exit(if ok { 0 } else { 2 });
            });
            return antwort(StatusCode::OK, "text/plain", None, b"ok".to_vec());
        }
    }
    match datei(app, &pfad) {
        Ok(d) => antwort(StatusCode::OK, &d.mime, d.csp, d.bytes),
        Err(grund) => antwort(
            StatusCode::NOT_FOUND,
            "text/html; charset=utf-8",
            csp_aus_konfiguration(app),
            fehlerseite(&grund).into_bytes(),
        ),
    }
}

/// Ein Paket nur aus den genannten Dateien (Selbsttest), ueber denselben
/// Pruefweg wie eine echte Generation (Manifest + Hash je Datei).
fn paket_aus(dateien: &[(&str, &[u8])]) -> Paket {
    let files: Vec<_> = dateien
        .iter()
        .map(|(p, b)| {
            serde_json::json!({
                "path": p, "sha256": runtime_bundle::hex(&Sha256::digest(b)),
                "size": b.len(), "kind": "ui",
            })
        })
        .collect();
    let manifest = serde_json::json!({
        "bundle_schema": 1, "generation": 0,
        "created_at": "2026-01-01T00:00:00Z", "expires_at": "2999-01-01T00:00:00Z",
        "client_min": "0.0.0", "client_max": "999.0.0", "requires": [],
        "score_version": landing_scoring::wasm::FASSUNG, "input_schema": 0, "files": files,
    });
    let inhalt: HashMap<String, Vec<u8>> = dateien.iter().map(|(p, b)| (p.to_string(), b.to_vec())).collect();
    Eingebaut { manifest: serde_json::to_vec(&manifest).expect("Manifest"), dateien: inhalt }
        .paket()
        .expect("Selbsttest-Paket")
}

/// Kleinstes gueltiges WASM-Modul (Magic + Version).
const MINI_WASM: &[u8] = b"\0asm\x01\0\0\0";

/// Startet den Selbsttest, wenn `AEROACARS_UI_SELBSTTEST` gesetzt ist: das
/// Hauptfenster wird geschlossen, nur `ui` mit der Testseite geoeffnet.
/// `true` = Selbsttest laeuft, der Rest von `setup` entfaellt.
pub(crate) fn selbsttest_starten<R: Runtime>(app: &tauri::App<R>) -> bool {
    let Some(ziel) = std::env::var_os("AEROACARS_UI_SELBSTTEST") else {
        return false;
    };
    let paket = paket_aus(&[
        ("ui/index.html", include_bytes!("ui_lader_selbsttest/index.html")),
        ("ui/selbsttest.js", include_bytes!("ui_lader_selbsttest/selbsttest.js")),
        ("ui/befehle.json", include_bytes!("../befehle.json")),
        ("ui/schrift.woff2", include_bytes!("../../src/assets/fonts/B612Mono-Regular.woff2")),
        ("ui/mini.wasm", MINI_WASM),
    ]);
    let _ = SELBSTTEST.set((PathBuf::from(ziel), paket));
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.destroy();
    }
    let gebaut = tauri::WebviewWindowBuilder::new(app, "ui", tauri::WebviewUrl::CustomProtocol(adresse("index.html")))
        .title("AeroACARS UI-Selbsttest")
        .inner_size(800.0, 600.0)
        .build();
    if let Err(e) = gebaut {
        tracing::error!(error = %e, "UI-Selbsttest: Fenster nicht anlegbar");
    }
    // Waechter: Kommt binnen 90 s kein Ergebnis, mit Fehler enden.
    let app = app.handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(90));
        if let Some((ziel, _)) = SELBSTTEST.get() {
            let _ = std::fs::write(ziel, br#"{"fehler":"kein Ergebnis binnen 90 s"}"#);
        }
        app.exit(3);
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pfade_und_mime() {
        assert_eq!(relativ("/"), "index.html");
        assert_eq!(relativ(""), "index.html");
        assert_eq!(relativ("/assets/a.js?x=1"), "assets/a.js");
        assert_eq!(relativ("/flugkarte/"), "flugkarte/index.html");
        assert_eq!(mime("a/b.wasm"), Some("application/wasm"));
        assert_eq!(mime("x.woff2"), Some("font/woff2"));
        assert_eq!(mime("index.html"), Some("text/html; charset=utf-8"));
        assert_eq!(mime("ohne_endung"), None);
    }

    /// Das Testpaket liefert nur, was im Manifest steht (derselbe Weg wie
    /// eine echte Generation; Hash-Pruefung je Lesen in `runtime-bundle`).
    #[test]
    fn selbsttest_paket_liefert_nur_manifest_dateien() {
        let p = paket_aus(&[("ui/index.html", b"<p>x</p>")]);
        assert_eq!(p.datei("ui/index.html").unwrap(), b"<p>x</p>");
        assert!(p.datei("ui/fehlt.js").is_err());
    }

    /// Fehlt ein Skript, ist das Paket defekt; fragt WebView2 von sich aus
    /// nach `favicon.ico`, nicht (Windows-Messung 29.10.2026).
    #[test]
    fn nur_fehlender_inhalt_macht_defekt() {
        let p = paket_aus(&[("ui/index.html", b"<p>x</p>")]);
        assert!(macht_defekt(&p, "ui/assets/index-abc.js"));
        assert!(macht_defekt(&p, "ui/index.html"));
        assert!(!macht_defekt(&p, "ui/favicon.ico"));
        assert!(!macht_defekt(&p, "ui/robots.txt"));
    }

    /// P-13.20: Hauptfenster, Telemetriefenster, LAN-Webapp und HUD lesen
    /// ueber denselben Lader aus derselben Generation. Gilt ein Paket mit
    /// Oberflaeche, liefert es jedem Ziel seine Dateien — kein Ziel greift
    /// am Lader vorbei auf das eingebaute Frontend (die Testanwendung hat
    /// keins: ein Umweg waere hier ein Fehler).
    #[test]
    #[cfg(not(windows))]
    fn alle_ziele_zeigen_dieselbe_generation() {
        let app = tauri::test::mock_app();
        let app = app.handle();
        let _ = SELBSTTEST.set((
            PathBuf::new(),
            paket_aus(&[
                ("ui/index.html", b"<p>G-Test</p>"),
                ("ui/hud/panel.js", b"/* G-Test */"),
                ("ui/hud/panel.css", b"/* G-Test css */"),
            ]),
        ));
        let haupt = adresse("index.html");
        let telemetrie = adresse("index.html?fenster=telemetrie");
        for url in [&haupt, &telemetrie] {
            let pfad = format!("{}?{}", url.path(), url.query().unwrap_or(""));
            assert_eq!(datei(app, &pfad).unwrap().bytes, b"<p>G-Test</p>", "{url}");
        }
        // LAN: Wurzel und SPA-Deep-Link.
        for pfad in ["/", "/logbook"] {
            assert_eq!(crate::remote::router::spa_datei(app, pfad).unwrap().bytes, b"<p>G-Test</p>", "LAN {pfad}");
        }
        assert_eq!(
            crate::panel_server::hud_dateien(app).unwrap(),
            ("/* G-Test */".to_string(), "/* G-Test css */".to_string())
        );
    }

    #[test]
    fn fehlerseite_ohne_skript_und_maskiert() {
        let s = fehlerseite("ui/<x>.js");
        assert!(s.contains("Paket defekt, eingebauter Stand aktiv"));
        assert!(!s.contains("<script"));
        assert!(s.contains("ui/&lt;x&gt;.js"));
    }
}
