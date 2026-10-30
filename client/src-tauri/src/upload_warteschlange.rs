//! v3 (P-7.21–P-7.24, Kap. 7.4): Warteschlange `pending_uploads/` fuer
//! Bewertungseingabe und Flugprotokoll — nach dem Muster der
//! PIREP-Warteschlange (`pending_pireps/`).
//!
//! - `pending_uploads/eingabe/<konto>/<pirep>_<seq>.json.gz`: Eingabe-Dokumente
//!   (`landungs_eingabe`), `<konto>` = Kurzhash des Pilot-Tokens; Abgelehntes
//!   unter `abgelehnt/`.
//! - `pending_uploads/protokoll/<pirep>.json`: ein Eintrag je Flugprotokoll mit
//!   `owner_identity`; das Protokoll selbst bleibt in `flight_logs/` (die
//!   Loeschautomatik der Huelle laesst Protokolle mit offenem Eintrag stehen).
//! - `<datei>.versuch`: Merker je Eintrag (Zahl der Fehlversuche, naechster
//!   Termin, Grund des letzten Fehlschlags — angezeigt nach P-7.24).
//!
//! Der Arbeiter laeuft alle 60 s (`TAKT`); ein Eintrag wird nach jedem
//! Fehlschlag mit wachsendem Abstand 1, 2, 4, 8, 16, dann 30 min erneut
//! versucht, ohne Obergrenze und ohne je aufzugeben. Offene Eintraege werden
//! nie wegen ihres Alters geloescht. Ein Eintrag wartet, bis sein Besitzer
//! angemeldet ist (P-7.22). Reihenfolge je Flug: Eingabe vor PIREP (bei live)
//! vor Flugprotokoll — ein Protokoll geht erst, wenn der PIREP nicht mehr in
//! `pending_pireps/` liegt und keine Eingabe desselben Flugs mehr offen ist.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub(crate) const ORDNER: &str = "pending_uploads";
/// Takt des Arbeiters (P-7.21).
pub(crate) const TAKT: Duration = Duration::from_secs(60);
/// Groesster Abstand zwischen zwei Versuchen eines Eintrags (P-7.21).
const ABSTAND_MAX_MIN: u64 = 30;

/// Ordner der Eingabe-Dokumente eines Kontos.
pub(crate) fn eingabe_ordner(app_daten: &Path, konto: &str) -> PathBuf {
    app_daten.join(ORDNER).join("eingabe").join(konto)
}

fn protokoll_ordner(app_daten: &Path) -> PathBuf {
    app_daten.join(ORDNER).join("protokoll")
}

fn jetzt_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Abstand nach `versuche` Fehlschlaegen: 1, 2, 4, 8, 16, 30, 30 … Minuten.
pub(crate) fn abstand(versuche: u32) -> Duration {
    let min = 1u64.checked_shl(versuche.saturating_sub(1)).unwrap_or(u64::MAX).min(ABSTAND_MAX_MIN);
    Duration::from_secs(min * 60)
}

/// Merker eines Eintrags (`<datei>.versuch`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Versuch {
    pub versuche: u32,
    /// Fruehester naechster Versuch (Unix-ms).
    pub naechster_ms: i64,
    /// Grund des letzten Fehlschlags.
    pub grund: Option<String>,
}

fn merker(datei: &Path) -> PathBuf {
    let mut name = datei.as_os_str().to_owned();
    name.push(".versuch");
    PathBuf::from(name)
}

pub(crate) fn versuch(datei: &Path) -> Versuch {
    std::fs::read(merker(datei)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Ist der Eintrag jetzt dran? Ohne Merker sofort.
pub(crate) fn faellig(datei: &Path, jetzt: i64) -> bool {
    versuch(datei).naechster_ms <= jetzt
}

/// Haelt einen Fehlschlag fest: naechster Versuch nach [`abstand`].
pub(crate) fn fehlschlag(datei: &Path, jetzt: i64, grund: &str) {
    let mut v = versuch(datei);
    v.versuche = v.versuche.saturating_add(1);
    v.naechster_ms = jetzt + abstand(v.versuche).as_millis() as i64;
    v.grund = Some(grund.chars().take(300).collect());
    if let Ok(b) = serde_json::to_vec(&v) {
        let _ = std::fs::write(merker(datei), b);
    }
}

/// Eintrag erledigt: Datei und Merker weg.
pub(crate) fn erledigt(datei: &Path) {
    let _ = std::fs::remove_file(datei);
    let _ = std::fs::remove_file(merker(datei));
}

/// Merker ohne Eintrag (Eintrag verschoben oder geloescht) aufraeumen.
pub(crate) fn verwaiste_merker_raeumen(ordner: &Path) {
    let Ok(es) = std::fs::read_dir(ordner) else { return };
    for e in es.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) == Some("versuch") && !p.with_extension("").exists() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Eintrag fuer ein Flugprotokoll.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProtokollEintrag {
    pub pirep_id: String,
    /// phpVMS-Pilot-ID des Besitzers (wie `QueuedPirep::owner_identity`).
    pub owner_identity: String,
    /// Eingereiht (Unix-ms).
    pub seit_ms: i64,
}

/// Reiht das Flugprotokoll `pirep_id` ein. Ein schon offener Eintrag bleibt
/// (mit seinem Merker) unveraendert.
pub(crate) fn protokoll_einreihen(app_daten: &Path, pirep_id: &str, owner_identity: &str) -> std::io::Result<PathBuf> {
    let ordner = protokoll_ordner(app_daten);
    std::fs::create_dir_all(&ordner)?;
    // Dieselbe Abbildung wie die Protokolldatei (Codex 30.10.).
    let ziel = ordner.join(format!("{}.json", protokoll_stamm(pirep_id)));
    if ziel.exists() {
        return Ok(ziel);
    }
    let e = ProtokollEintrag { pirep_id: pirep_id.to_string(), owner_identity: owner_identity.to_string(), seit_ms: jetzt_ms() };
    let tmp = ziel.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&e).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, &ziel)?;
    Ok(ziel)
}

/// Alle offenen Protokoll-Eintraege (Pfad, Inhalt).
pub(crate) fn protokolle(app_daten: &Path) -> Vec<(PathBuf, ProtokollEintrag)> {
    let ordner = protokoll_ordner(app_daten);
    verwaiste_merker_raeumen(&ordner);
    let Ok(es) = std::fs::read_dir(&ordner) else { return Vec::new() };
    let mut v: Vec<_> = es
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|p| Some((p.clone(), serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?)))
        .collect();
    v.sort_by_key(|(_, e): &(PathBuf, ProtokollEintrag)| e.seit_ms);
    v
}

/// Dateiname des Flugprotokolls ohne `.jsonl` (gleiche Abbildung wie der
/// Recorder: alles ausser `[A-Za-z0-9_-]` wird `_`).
pub(crate) fn protokoll_stamm(pirep_id: &str) -> String {
    pirep_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

/// Dateinamen (ohne `.jsonl`) der Protokolle mit offenem Eintrag — die
/// Loeschautomatik laesst sie stehen.
pub(crate) fn offene_protokolle(app_daten: &Path) -> HashSet<String> {
    protokolle(app_daten).into_iter().map(|(_, e)| protokoll_stamm(&e.pirep_id)).collect()
}

/// PIREP-ID eines Eingabe-Dokuments (`<pirep>_<seq>.json.gz`).
fn eingabe_pirep(datei: &Path) -> Option<String> {
    let name = datei.file_name()?.to_str()?.strip_suffix(".json.gz")?;
    Some(name.rsplit_once('_')?.0.to_string())
}

/// Offene Eingabe-Dokumente eines Kontos (ohne Abgelehntes).
pub(crate) fn eingaben(app_daten: &Path, konto: &str) -> Vec<PathBuf> {
    let Ok(es) = std::fs::read_dir(eingabe_ordner(app_daten, konto)) else { return Vec::new() };
    let mut v: Vec<PathBuf> = es
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.to_str().is_some_and(|s| s.ends_with(".json.gz")))
        .collect();
    v.sort();
    v
}

/// Warum ein Protokoll (noch) nicht gesendet wird.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Protokoll {
    Senden,
    /// P-7.22: ein anderes Konto ist angemeldet — der Eintrag wartet.
    WartetAufBesitzer,
    /// Reihenfolge: der PIREP liegt noch in `pending_pireps/`.
    WartetAufPirep,
    /// Reihenfolge: eine Eingabe desselben Flugs ist noch offen.
    WartetAufEingabe,
    /// Der Flug laeuft noch (sein Protokoll waechst).
    FlugLaeuft,
}

/// Reine Entscheidung je Protokoll-Eintrag (getestet).
pub(crate) fn protokoll_entscheiden(
    e: &ProtokollEintrag,
    angemeldet: Option<&str>,
    aktiver_flug: Option<&str>,
    pireps_offen: &HashSet<String>,
    eingaben_offen: &HashSet<String>,
) -> Protokoll {
    if angemeldet != Some(e.owner_identity.as_str()) {
        Protokoll::WartetAufBesitzer
    } else if aktiver_flug == Some(e.pirep_id.as_str()) {
        Protokoll::FlugLaeuft
    } else if pireps_offen.contains(&e.pirep_id) {
        Protokoll::WartetAufPirep
    } else if eingaben_offen.contains(&e.pirep_id) {
        Protokoll::WartetAufEingabe
    } else {
        Protokoll::Senden
    }
}

/// PIREP-IDs mit offenem Eingabe-Dokument im Konto.
pub(crate) fn eingaben_offen(app_daten: &Path, konto: Option<&str>) -> HashSet<String> {
    konto.map(|k| eingaben(app_daten, k).iter().filter_map(|p| eingabe_pirep(p)).collect()).unwrap_or_default()
}

/// Ein ausstehender Upload fuer die Anzeige (P-7.24).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Ausstehend {
    pub pirep_id: String,
    /// `eingabe` oder `protokoll`.
    pub art: &'static str,
    /// Eingereiht seit (Unix-ms).
    pub seit_ms: i64,
    pub versuche: u32,
    /// Grund des letzten Fehlschlags.
    pub grund: Option<String>,
}

/// Alle ausstehenden Uploads des angemeldeten Kontos: Eingaben seines
/// Token-Ordners, Protokolle mit seiner Pilot-ID. Fremde bleiben verborgen.
pub(crate) fn ausstehend(app_daten: &Path, konto: Option<&str>, besitzer: Option<&str>) -> Vec<Ausstehend> {
    let mut v = Vec::new();
    if let Some(k) = konto {
        for p in eingaben(app_daten, k) {
            let Some(pirep_id) = eingabe_pirep(&p) else { continue };
            let seit_ms = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as i64);
            let w = versuch(&p);
            v.push(Ausstehend { pirep_id, art: "eingabe", seit_ms, versuche: w.versuche, grund: w.grund });
        }
    }
    for (p, e) in protokolle(app_daten) {
        if besitzer != Some(e.owner_identity.as_str()) {
            continue;
        }
        let w = versuch(&p);
        v.push(Ausstehend { pirep_id: e.pirep_id, art: "protokoll", seit_ms: e.seit_ms, versuche: w.versuche, grund: w.grund });
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordner(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("aeroacars-uw-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// P-7.21: Abstand waechst 1 → 30 min und bleibt dort — nie „aufgeben“.
    #[test]
    fn abstand_waechst_bis_dreissig_minuten() {
        let min: Vec<u64> = (1..=8).map(|n| abstand(n).as_secs() / 60).collect();
        assert_eq!(min, [1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(abstand(10_000).as_secs(), 30 * 60, "auch nach sehr vielen Versuchen");
    }

    /// Wiederholung mit Abstand: nach einem Fehlschlag erst nach 1 min wieder
    /// dran, nach dem zweiten nach 2 min; erledigt raeumt den Merker.
    /// Gegenprobe: ohne Merker sofort faellig.
    #[test]
    fn wiederholung_mit_abstand() {
        let d = ordner("abstand");
        let datei = d.join("P1_1.json.gz");
        std::fs::write(&datei, b"x").unwrap();
        assert!(faellig(&datei, 0), "Gegenprobe: neuer Eintrag sofort");
        fehlschlag(&datei, 1_000, "HTTP 503");
        assert!(!faellig(&datei, 1_000 + 59_000));
        assert!(faellig(&datei, 1_000 + 60_000));
        fehlschlag(&datei, 100_000, "Netz weg");
        assert!(!faellig(&datei, 100_000 + 119_000));
        assert!(faellig(&datei, 100_000 + 120_000));
        let v = versuch(&datei);
        assert_eq!((v.versuche, v.grund.as_deref()), (2, Some("Netz weg")));
        erledigt(&datei);
        assert!(!datei.exists() && !merker(&datei).exists());
    }

    /// „Gibt nie auf“: auch nach hundert Fehlschlaegen bleibt der Eintrag
    /// liegen und wird spaetestens nach 30 min wieder versucht.
    #[test]
    fn gibt_nie_auf() {
        let d = ordner("nie");
        let datei = d.join("P1_1.json.gz");
        std::fs::write(&datei, b"x").unwrap();
        for i in 0..100 {
            fehlschlag(&datei, i, "HTTP 500");
        }
        assert!(datei.exists());
        assert!(faellig(&datei, 99 + 30 * 60_000));
        assert!(!faellig(&datei, 99 + 30 * 60_000 - 1));
    }

    fn eintrag(pirep: &str, besitzer: &str) -> ProtokollEintrag {
        ProtokollEintrag { pirep_id: pirep.into(), owner_identity: besitzer.into(), seit_ms: 0 }
    }

    /// Reihenfolge Eingabe → PIREP → Protokoll und Kontowechsel (P-7.22).
    #[test]
    fn protokoll_reihenfolge_und_besitzer() {
        let e = eintrag("P1", "7");
        let leer = HashSet::new();
        let p1: HashSet<String> = ["P1".to_string()].into();
        assert_eq!(protokoll_entscheiden(&e, Some("7"), None, &leer, &leer), Protokoll::Senden);
        assert_eq!(protokoll_entscheiden(&e, Some("8"), None, &leer, &leer), Protokoll::WartetAufBesitzer);
        assert_eq!(protokoll_entscheiden(&e, None, None, &leer, &leer), Protokoll::WartetAufBesitzer);
        assert_eq!(protokoll_entscheiden(&e, Some("7"), None, &p1, &leer), Protokoll::WartetAufPirep);
        assert_eq!(protokoll_entscheiden(&e, Some("7"), None, &leer, &p1), Protokoll::WartetAufEingabe);
        assert_eq!(protokoll_entscheiden(&e, Some("7"), Some("P1"), &leer, &leer), Protokoll::FlugLaeuft);
        // Gegenprobe: fremde Flüge in den Mengen halten P1 nicht auf.
        let p2: HashSet<String> = ["P2".to_string()].into();
        assert_eq!(protokoll_entscheiden(&e, Some("7"), Some("P2"), &p2, &p2), Protokoll::Senden);
    }

    /// Einreihen ist idempotent (Merker bleibt), Anzeige zeigt nur das
    /// eigene Konto, Eingabe-PIREP aus dem Dateinamen.
    #[test]
    fn einreihen_und_anzeige_je_konto() {
        let d = ordner("anzeige");
        let p = protokoll_einreihen(&d, "AB/C", "7").unwrap();
        assert_eq!(p.file_name().unwrap(), "AB_C.json", "wie die Protokolldatei AB_C.jsonl");
        fehlschlag(&p, 0, "HTTP 502");
        protokoll_einreihen(&d, "AB/C", "7").unwrap();
        assert_eq!(versuch(&p).versuche, 1, "zweites Einreihen setzt den Merker nicht zurueck");
        protokoll_einreihen(&d, "FREMD", "8").unwrap();
        let ein = eingabe_ordner(&d, "k1");
        std::fs::create_dir_all(ein.join("abgelehnt")).unwrap();
        std::fs::write(ein.join("X_Y_2.json.gz"), b"x").unwrap();
        std::fs::write(ein.join("abgelehnt").join("Z_1.json.gz"), b"x").unwrap();
        assert_eq!(eingaben_offen(&d, Some("k1")), HashSet::from(["X_Y".to_string()]));
        let a = ausstehend(&d, Some("k1"), Some("7"));
        let mut kurz: Vec<_> = a.iter().map(|x| (x.pirep_id.as_str(), x.art, x.versuche)).collect();
        kurz.sort();
        assert_eq!(kurz, [("AB/C", "protokoll", 1), ("X_Y", "eingabe", 0)], "der Eintrag behaelt die echte PIREP-ID");
        assert_eq!(a.iter().find(|x| x.art == "protokoll").unwrap().grund.as_deref(), Some("HTTP 502"));
        // Gegenprobe: anderes Konto sieht nur seins.
        let b = ausstehend(&d, Some("k2"), Some("8"));
        assert_eq!(b.iter().map(|x| x.pirep_id.as_str()).collect::<Vec<_>>(), ["FREMD"]);
        // Verwaister Merker wird geraeumt.
        erledigt(&p);
        std::fs::write(merker(&p), b"{}").unwrap();
        let _ = protokolle(&d);
        assert!(!merker(&p).exists());
    }
}
