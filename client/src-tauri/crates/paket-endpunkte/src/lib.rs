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
    Ok(e)
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
