//! Adressen der Gegenstellen aus dem Laufzeit-Paket (`endpunkte.json`,
//! Pflichtenheft P-3.60).
//!
//! Das Paket darf den **Pfad** einer Adresse aendern, der Host jedes Feldes
//! ist aber im Code fest gebunden ([`HOSTS`]). Ein anderer Host braucht ein
//! Release (P-2.15, Z9). Fest je Feld, nicht eine gemeinsame Liste: sonst
//! koennte ein Paket `live` auf einen anderen erlaubten Host setzen und das
//! Pilot-Token oder den Hoppie-Code dorthin schicken (Codex 29.10.).

use std::sync::OnceLock;

/// Feld → einzig erlaubter Host. Nur `https`.
pub const HOSTS: &[(&str, &str)] = &[
    ("live", "live.kant.ovh"),
    ("hoppie", "www.hoppie.nl"),
    ("viffsys", "api.viffsys.com"),
    ("simbrief", "www.simbrief.com"),
    ("metar", "aviationweather.gov"),
    ("vatsim_vdgs", "vats.im"),
];

/// Die Adressen eines Stands. Jede Adresse ist `https://<host>[/pfad]` ohne
/// abschliessenden Schraegstrich, Host aus [`HOSTS`].
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpunkte {
    format: u32,
    #[serde(default)]
    #[allow(dead_code)]
    notiz: Vec<String>,
    /// AeroACARS-Live: Recorder, Navdaten, Provisionierung, Flugprotokolle,
    /// Laufzeit-Paket, Kabinenton.
    pub live: String,
    /// Hoppie ACARS (`connect.html`).
    pub hoppie: String,
    /// VIFFSYS (VDGS/CDM).
    pub viffsys: String,
    /// SimBrief (OFP-Abruf).
    pub simbrief: String,
    /// aviationweather.gov (METAR).
    pub metar: String,
    /// VATSIM-VDGS-Seite.
    pub vatsim_vdgs: String,
    /// Pfad der Ticket-Schnittstelle des GSG-HelpDesk (Hilfe anfordern,
    /// P-9.22). Nur der **Pfad**: Der Host bleibt im Programm fest
    /// (`ALLOWED_PHPVMS_HOST`), denn dorthin geht der phpVMS-Schluessel des
    /// Piloten. Fehlt das Feld (Paket vor `hilfe-formular-1`), gilt
    /// [`HELPDESK_PFAD_STANDARD`].
    #[serde(default)]
    helpdesk_pfad: Option<String>,
}

/// Pfad der HelpDesk-Ticket-Schnittstelle, wenn das Paket keinen nennt.
pub const HELPDESK_PFAD_STANDARD: &str = "/api/helpdesk/tickets";
/// Laengster erlaubter HelpDesk-Pfad.
pub const HELPDESK_PFAD_MAX: usize = 120;

impl Endpunkte {
    /// Pfad der HelpDesk-Ticket-Schnittstelle (geprueft beim Lesen).
    pub fn helpdesk_pfad(&self) -> &str {
        self.helpdesk_pfad.as_deref().unwrap_or(HELPDESK_PFAD_STANDARD)
    }
}

/// Eingebauter Stand = Datei im Paketordner (eine Quelle).
pub const EINGEBAUT: &str = include_str!("../../../../../bundle/endpunkte.json");

static QUELLE: OnceLock<fn() -> &'static Endpunkte> = OnceLock::new();

/// Die geltenden Adressen. Der Client meldet beim Start eine Quelle an, die
/// aus seinem einen Paketstand liest (P-3.02); ohne Anmeldung (Tests,
/// Werkzeuge) gilt der eingebaute Stand.
pub fn endpunkte() -> &'static Endpunkte {
    match QUELLE.get() {
        Some(quelle) => quelle(),
        None => {
            static EIGEN: OnceLock<Endpunkte> = OnceLock::new();
            EIGEN.get_or_init(|| lesen(EINGEBAUT).expect("eingebaute endpunkte.json"))
        }
    }
}

/// Meldet die Quelle an (einmal je Lauf). `false`, wenn schon eine gilt.
pub fn quelle_setzen(quelle: fn() -> &'static Endpunkte) -> bool {
    QUELLE.set(quelle).is_ok()
}

/// Liest `endpunkte.json` (fail-closed): Format 1, jede Adresse `https`,
/// Host aus [`HOSTS`], keine Zugangsdaten, kein Port, kein `?`/`#`, kein
/// abschliessender Schraegstrich.
pub fn lesen(json: &str) -> Result<Endpunkte, String> {
    let e: Endpunkte = serde_json::from_str(json).map_err(|x| format!("endpunkte.json: {x}"))?;
    if e.format != 1 {
        return Err(format!("endpunkte.json: Format {} unbekannt", e.format));
    }
    for (name, url) in [
        ("live", &e.live),
        ("hoppie", &e.hoppie),
        ("viffsys", &e.viffsys),
        ("simbrief", &e.simbrief),
        ("metar", &e.metar),
        ("vatsim_vdgs", &e.vatsim_vdgs),
    ] {
        let host = HOSTS.iter().find(|(f, _)| *f == name).map(|(_, h)| *h).ok_or("Feld ohne Host")?;
        pruefe_url(url, host).map_err(|x| format!("endpunkte.json: {name}: {x}"))?;
    }
    if let Some(pfad) = &e.helpdesk_pfad {
        pruefe_pfad(pfad).map_err(|x| format!("endpunkte.json: helpdesk_pfad: {x}"))?;
    }
    Ok(e)
}

/// Ein Pfad auf dem festen phpVMS-Host: beginnt mit `/api/helpdesk/` (der
/// Schluessel des Piloten geht so an keine andere phpVMS-Schnittstelle), kein
/// `.`-Segment, nur ASCII
/// `[A-Za-z0-9/_.-]` (also kein `?`, `#`, `@`, `%`, Leerzeichen, `\` — eine
/// Abfrage mit Geheimnissen oder ein anderer Host sind so ausgeschlossen),
/// kein `..`, kein `//`, kein abschliessender Schraegstrich, hoechstens
/// [`HELPDESK_PFAD_MAX`] Zeichen.
fn pruefe_pfad(pfad: &str) -> Result<(), String> {
    if pfad.len() > HELPDESK_PFAD_MAX {
        return Err("zu lang".into());
    }
    if !pfad.starts_with("/api/helpdesk/") || pfad.ends_with('/') {
        return Err("muss mit /api/helpdesk/ beginnen und ohne / enden".into());
    }
    if !pfad.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'.' | b'-')) {
        return Err("Zeichen ungueltig".into());
    }
    if pfad.contains("..") || pfad.contains("//") || pfad.split('/').any(|s| s == ".") {
        return Err("Form ungueltig".into());
    }
    Ok(())
}

/// Laengster erlaubter Pfad der allgemeinen Modul-Schnittstelle.
pub const MODUL_API_PFAD_MAX: usize = 120;
/// Hoechstens so viele Eintraege in `tabellen/modul-api.json`.
pub const MODUL_API_PFADE_MAX: usize = 32;

/// Erstes Segment nach `/api/`, das die Modul-Schnittstelle nie erreicht: die
/// Kern-Schnittstelle von phpVMS (`RouteServiceProvider::mapApiRoutes`). Mit
/// dem Schluessel des Piloten liessen sich dort Fluege, Buchungen und PIREPs
/// aendern. Fest im Programm, das Paket kann die Liste nicht verkleinern.
const MODUL_API_KERN: &[&str] = &[
    "acars", "airlines", "airports", "bids", "cron", "fleet", "flights", "news", "pireps", "settings", "status", "subfleet",
    "user", "users", "version",
];
/// Segment, das nirgends im Pfad stehen darf: die kostenpflichtige
/// Drehbuch-Bestellung der Kabinenansagen.
const MODUL_API_DREHBUCH: &str = "drehbuch";

/// Prueft einen Pfad der Modul-Schnittstelle, bei jedem Aufruf und fuer jeden
/// Eintrag der Paketliste: beginnt mit `/api/`, hoechstens
/// [`MODUL_API_PFAD_MAX`] Zeichen, nur `[A-Za-z0-9/_.-]`, kein `..`, `//` oder
/// `.`-Segment, kein abschliessender Schraegstrich, kein phpVMS-Kernpfad und
/// kein Segment `drehbuch`. Gross-/Kleinschreibung zaehlt bei der Sperre nicht.
pub fn modul_api_pfad_pruefen(pfad: &str) -> Result<(), String> {
    if pfad.len() > MODUL_API_PFAD_MAX {
        return Err("zu lang".into());
    }
    let rest = pfad.strip_prefix("/api/").ok_or("muss mit /api/ beginnen")?;
    if rest.is_empty() || pfad.ends_with('/') {
        return Err("Form ungueltig".into());
    }
    if !pfad.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'.' | b'-')) {
        return Err("Zeichen ungueltig".into());
    }
    if pfad.contains("..") || pfad.contains("//") || pfad.split('/').any(|s| s == ".") {
        return Err("Form ungueltig".into());
    }
    let klein = rest.to_ascii_lowercase();
    if klein.split('/').next().is_some_and(|erstes| MODUL_API_KERN.contains(&erstes)) {
        return Err("phpVMS-Kernpfad gesperrt".into());
    }
    if klein.split('/').any(|s| s == MODUL_API_DREHBUCH) {
        return Err("Drehbuch-Pfad gesperrt".into());
    }
    Ok(())
}

/// Die Pfade aus `tabellen/modul-api.json`. Eigene Datei statt Feld in
/// `endpunkte.json`: [`Endpunkte`] weist unbekannte Felder ab, ein aelterer
/// Client wuerde eine Generation mit dem neuen Feld verwerfen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModulApi {
    pfade: Vec<String>,
}

#[derive(serde::Deserialize)]
struct ModulApiDatei {
    format: u32,
    pfade: Vec<String>,
}

impl ModulApi {
    /// Steht der Pfad auf der Paketliste? Trifft nur auf Segmentgrenze:
    /// `/api/a/b` deckt `/api/a/b/c`, aber nicht `/api/a/bc`.
    pub fn erlaubt(&self, pfad: &str) -> bool {
        self.pfade.iter().any(|p| pfad == p || pfad.strip_prefix(p.as_str()).is_some_and(|r| r.starts_with('/')))
    }
}

/// Liest `tabellen/modul-api.json` (fail-closed: ein schlechter Eintrag
/// verwirft das Paket im Selbsttest).
pub fn modul_api_lesen(json: &str) -> Result<ModulApi, String> {
    let d: ModulApiDatei = serde_json::from_str(json).map_err(|x| format!("modul-api.json: {x}"))?;
    if d.format != 1 {
        return Err(format!("modul-api.json: Format {} unbekannt", d.format));
    }
    if d.pfade.len() > MODUL_API_PFADE_MAX {
        return Err("modul-api.json: zu viele Pfade".into());
    }
    for pfad in &d.pfade {
        modul_api_pfad_pruefen(pfad).map_err(|x| format!("modul-api.json: {pfad}: {x}"))?;
    }
    Ok(ModulApi { pfade: d.pfade })
}

fn pruefe_url(url: &str, erlaubt: &str) -> Result<(), String> {
    let rest = url.strip_prefix("https://").ok_or("nur https")?;
    let host = rest.split('/').next().unwrap_or("");
    if host != erlaubt {
        return Err(format!("Host {host:?} statt {erlaubt:?}"));
    }
    if url.ends_with('/') || url.contains(['?', '#', '@', ' ', '\\']) {
        return Err("Form ungueltig".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eingebaute_adressen_wie_2_1_24() {
        let e = lesen(EINGEBAUT).unwrap();
        assert_eq!(e.live, "https://live.kant.ovh");
        assert_eq!(e.hoppie, "https://www.hoppie.nl/acars/system/connect.html");
        assert_eq!(e.viffsys, "https://api.viffsys.com");
        assert_eq!(e.simbrief, "https://www.simbrief.com");
        assert_eq!(e.metar, "https://aviationweather.gov");
        assert_eq!(e.vatsim_vdgs, "https://vats.im/vdgs");
        assert_eq!(e.helpdesk_pfad(), "/api/helpdesk/tickets");
    }

    #[test]
    fn helpdesk_pfad_aus_dem_paket_mit_rueckfall() {
        let gut: serde_json::Value = serde_json::from_str(EINGEBAUT).unwrap();
        // Paket ohne das Feld (vor hilfe-formular-1): heutiger Pfad.
        let mut ohne = gut.clone();
        ohne.as_object_mut().unwrap().remove("helpdesk_pfad");
        assert_eq!(lesen(&ohne.to_string()).unwrap().helpdesk_pfad(), HELPDESK_PFAD_STANDARD);
        // Anderer Pfad aus dem Paket wird genutzt.
        let mut neu = gut.clone();
        neu["helpdesk_pfad"] = "/api/helpdesk/v2/tickets".into();
        assert_eq!(lesen(&neu.to_string()).unwrap().helpdesk_pfad(), "/api/helpdesk/v2/tickets");
        let lang = format!("/api/helpdesk/{}", "a".repeat(HELPDESK_PFAD_MAX - 14));
        assert_eq!(lang.len(), HELPDESK_PFAD_MAX);
        neu["helpdesk_pfad"] = lang.clone().into();
        assert!(lesen(&neu.to_string()).is_ok(), "genau an der Grenze");
        for boese in [
            format!("{lang}b"),
            "/helpdesk/tickets".into(),
            "api/helpdesk/tickets".into(),
            "/api/".into(),
            "/api/helpdesk/".into(),
            // andere phpVMS-Schnittstelle mit dem Schluessel des Piloten
            "/api/user".into(),
            "/api/pireps/1/comments".into(),
            "/api/helpdesk/./tickets".into(),
            "/api/helpdesk/.".into(),
            "/api/helpdesk/tickets/".into(),
            "/api/../admin".into(),
            "/api/helpdesk/..".into(),
            "/api//helpdesk".into(),
            "/api/helpdesk/t?api_key=x".into(),
            "/api/helpdesk/t#x".into(),
            "/api/helpdesk/t%2F..".into(),
            "/api/helpdesk/t@evil.example".into(),
            "/api/helpdesk/t x".into(),
            "/api/helpdesk/t\\x".into(),
            "https://evil.example/api/t".into(),
            "//evil.example/api/t".into(),
            "/api/helpdesk/tick\u{e4}ts".into(),
        ] {
            let mut v = gut.clone();
            v["helpdesk_pfad"] = boese.clone().into();
            assert!(lesen(&v.to_string()).is_err(), "{boese}");
        }
        let mut kein_text = gut;
        kein_text["helpdesk_pfad"] = 5.into();
        assert!(lesen(&kein_text.to_string()).is_err());
    }

    #[test]
    fn modul_api_pfad_grenzen() {
        for gut in ["/api/kabinenansagen/briefing", "/api/kabinenansagen/briefing/pruefen", "/api/x", "/api/flightsx", "/api/a.b/c-d_e"] {
            assert_eq!(modul_api_pfad_pruefen(gut), Ok(()), "{gut}");
        }
        let lang = format!("/api/{}", "a".repeat(MODUL_API_PFAD_MAX - 5));
        assert!(modul_api_pfad_pruefen(&lang).is_ok(), "genau an der Grenze");
        for boese in [
            format!("{lang}b"),
            "".into(),
            "/api/".into(),
            "/api".into(),
            "api/x".into(),
            "/x/api/y".into(),
            "/api/x/".into(),
            "/api//x".into(),
            "/api/x//y".into(),
            "/api/./x".into(),
            "/api/x/.".into(),
            "/api/../admin".into(),
            "/api/x/../user".into(),
            "/api/x?api_key=1".into(),
            "/api/x#y".into(),
            "/api/x%2F..".into(),
            "/api/x y".into(),
            "/api/x\\y".into(),
            "/api/x@evil.example".into(),
            "https://evil.example/api/x".into(),
            "//evil.example/api/x".into(),
            "/api/kabin\u{e4}".into(),
        ] {
            assert!(modul_api_pfad_pruefen(&boese).is_err(), "{boese}");
        }
    }

    #[test]
    fn modul_api_kernpfade_und_drehbuch_gesperrt() {
        for kern in MODUL_API_KERN {
            let pfad = format!("/api/{kern}");
            assert!(modul_api_pfad_pruefen(&pfad).is_err(), "{pfad}");
            let tiefer = format!("/api/{kern}/1/comments");
            assert!(modul_api_pfad_pruefen(&tiefer).is_err(), "{tiefer}");
            let gross = format!("/api/{}", kern.to_uppercase());
            assert!(modul_api_pfad_pruefen(&gross).is_err(), "{gross}");
        }
        for boese in ["/api/kabinenansagen/drehbuch", "/api/kabinenansagen/DrehBuch/bestellen", "/api/drehbuch"] {
            assert!(modul_api_pfad_pruefen(boese).is_err(), "{boese}");
        }
        assert!(modul_api_pfad_pruefen("/api/kabinenansagen/drehbuecher").is_ok(), "nur das ganze Segment");
    }

    #[test]
    fn modul_api_liste_trifft_nur_auf_segmentgrenze() {
        let m = modul_api_lesen(r#"{"format":1,"notiz":["x"],"pfade":["/api/kabinenansagen/briefing"],"kuenftig":true}"#).unwrap();
        assert!(m.erlaubt("/api/kabinenansagen/briefing"));
        assert!(m.erlaubt("/api/kabinenansagen/briefing/pruefen"));
        assert!(!m.erlaubt("/api/kabinenansagen/briefings"));
        assert!(!m.erlaubt("/api/kabinenansagen"));
        assert!(!m.erlaubt("/api/helpdesk/tickets"));
        assert!(!ModulApi::default().erlaubt("/api/kabinenansagen/briefing"), "leere Liste erlaubt nichts");
    }

    #[test]
    fn modul_api_datei_fail_closed() {
        for boese in [
            r#"{"format":2,"pfade":[]}"#.to_string(),
            r#"{"format":1}"#.to_string(),
            r#"{"format":1,"pfade":"/api/x"}"#.to_string(),
            r#"{"format":1,"pfade":["/api/user"]}"#.to_string(),
            r#"{"format":1,"pfade":["/api/x","/api/kabinenansagen/drehbuch"]}"#.to_string(),
            r#"{"format":1,"pfade":["https://evil.example/api/x"]}"#.to_string(),
            format!(r#"{{"format":1,"pfade":[{}]}}"#, vec![r#""/api/x""#; MODUL_API_PFADE_MAX + 1].join(",")),
        ] {
            assert!(modul_api_lesen(&boese).is_err(), "{boese}");
        }
        assert_eq!(modul_api_lesen(r#"{"format":1,"pfade":[]}"#).unwrap(), ModulApi::default());
    }

    #[test]
    fn fremder_host_und_falsche_form_werden_abgewiesen() {
        let gut: serde_json::Value = serde_json::from_str(EINGEBAUT).unwrap();
        for boese in [
            "https://evil.example",
            "http://live.kant.ovh",
            "https://live.kant.ovh.evil.example",
            "https://user:pw@live.kant.ovh",
            "https://live.kant.ovh:8443",
            "https://live.kant.ovh/",
            "https://live.kant.ovh/?x=1",
            // anderer, fuer ein ANDERES Feld erlaubter Host (Codex 29.10.)
            "https://www.hoppie.nl",
            "https://LIVE.KANT.OVH",
            "https://live.kant.ovh./api",
        ] {
            let mut v = gut.clone();
            v["live"] = boese.into();
            assert!(lesen(&v.to_string()).is_err(), "{boese}");
        }
        let mut hoppie_fremd = gut.clone();
        hoppie_fremd["hoppie"] = "https://aviationweather.gov/acars/system/connect.html".into();
        assert!(lesen(&hoppie_fremd.to_string()).is_err());
        let mut nur_pfad = gut.clone();
        nur_pfad["live"] = "https://live.kant.ovh/v2".into();
        assert!(lesen(&nur_pfad.to_string()).is_ok(), "Pfad darf sich aendern");
        let mut fremd = gut;
        fremd["neu"] = "https://live.kant.ovh".into();
        assert!(lesen(&fremd.to_string()).is_err());
    }
}
