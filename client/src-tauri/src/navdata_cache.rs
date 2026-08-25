//! Navdaten auf der Platte — damit ein Flug ohne Netz nicht auf die
//! schlechtere Quelle zurückfällt.
//!
//! # Was vorher passierte
//!
//! Der Client zieht beim Flugstart die Bahndaten von Start-, Ziel- und
//! Ausweichflugplatz vom Server. Bis hierher lagen sie ausschliesslich im
//! Arbeitsspeicher, und zwar genau so lange wie der Flug: Beim Abgeben des
//! Flugberichts waren sie weg.
//!
//! War der Server im Moment des Flugstarts nicht erreichbar — Netz weg,
//! Wartung, ein Timeout im falschen Augenblick — fiel die Landebewertung
//! still auf OurAirports zurück. Für denselben Flughafen, den der Pilot
//! vorgestern schon angeflogen hatte.
//!
//! Seit die Serverdaten aus Navigraphs vollständiger Datenbank kommen, ist
//! dieser Rückfall teurer geworden: Belag, Schwellenversatz und die
//! korrigierte Bahnachse gibt es nur dort.
//!
//! # Was hier passiert
//!
//! Jeder erfolgreiche Abruf landet als kleine Datei unter
//! `<app_config_dir>/navdata/<ICAO>.json`. Beim nächsten Mal wird sie
//! gelesen, wenn der Server nicht antwortet. Der Pilot baut sich damit
//! über seine Flüge hinweg von selbst den Bestand auf, den er braucht —
//! ohne Voll-Download, ohne Pflege.
//!
//! Grössenordnung: rund 3 kB je Flugplatz. Die 429 Ziele, die GSG je
//! angeflogen hat, wären zusammen etwa 1,3 MB.
//!
//! # Zwei Regeln, die nicht verhandelbar sind
//!
//! **1. Alter Bestand geht nie als frischer durch.** Wer aus dem
//! Zwischenspeicher liest, bekommt das Datum mitgeliefert, und im
//! Aktivitätsprotokoll steht, welcher Zyklus es war und wie alt. Eine
//! Bewertung, die auf drei Zyklen alten Daten steht, darf nicht so
//! aussehen wie eine auf heutigen — sonst sucht man den Fehler später
//! überall ausser dort.
//!
//! **2. Der Zwischenspeicher ist die ZWEITE Wahl, nie die erste.** Solange
//! der Server antwortet, gilt der Server. Sonst hätten wir einen Bestand,
//! der nie wieder aktuell wird, weil er sich selbst bestätigt.
//!
//! Ein Eintrag, der älter ist als `HOECHSTALTER_TAGE`, wird nicht mehr
//! genommen: Nach einem halben Jahr sind genug Zyklen vergangen, dass
//! Bahnen verlängert, umbenannt oder gesperrt worden sein können. Dann ist
//! OurAirports — so ungeliebt es ist — die ehrlichere Auskunft, weil
//! niemand es für aktuell hält.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aeroacars_mqtt::navdata::NavAirport;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// Ab wann ein gespeicherter Flugplatz nicht mehr verwendet wird.
///
/// Ein AIRAC-Zyklus lebt 28 Tage. Ein halbes Jahr sind rund sechs
/// Zyklen — lang genug, dass der Pilot auch ein Ziel wiederfindet, das
/// er nur zweimal im Jahr anfliegt, und kurz genug, dass sich in der
/// Zwischenzeit keine ganze Flughafengeneration geändert hat.
const HOECHSTALTER_TAGE: u64 = 183;

const SEKUNDEN_JE_TAG: u64 = 86_400;

/// Ein Flugplatz, wie er auf der Platte liegt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GespeicherterFlugplatz {
    /// Der Flugplatz selbst — dieselbe Form, die vom Server kommt.
    pub airport: NavAirport,
    /// AIRAC-Zyklus, unter dem er geholt wurde.
    pub zyklus: String,
    /// Unix-Sekunden des Abrufs.
    pub geholt_am: u64,
}

impl GespeicherterFlugplatz {
    /// Alter in ganzen Tagen, gegen eine vorgegebene Gegenwart. `None`,
    /// wenn der Eintrag in der Zukunft liegt — verstellte Systemuhr, und
    /// Unsinn wird nicht verwendet.
    ///
    /// ⚠ Diese Fassung verlangt die Gegenwart als Argument, damit das
    /// Altern prüfbar ist. Wer sie im Betrieb ruft, will fast immer
    /// `alter_tage_jetzt()`: Ein versehentliches `alter_tage(0)` rechnet
    /// gegen den 1. Januar 1970 und meldet dann für JEDEN Eintrag „0 Tage
    /// alt" — die Meldung, die genau das verschweigt, wofür sie da ist.
    pub fn alter_tage(&self, jetzt: u64) -> Option<u64> {
        jetzt
            .checked_sub(self.geholt_am)
            .map(|d| d / SEKUNDEN_JE_TAG)
    }

    /// Alter in ganzen Tagen gegen die Systemuhr.
    pub fn alter_tage_jetzt(&self) -> u64 {
        self.alter_tage(jetzt_unix()).unwrap_or(0)
    }
}

fn jetzt_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ordner(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|p| p.join("navdata"))
}

/// Der Dateiname zu einem ICAO, unterhalb eines gegebenen Ordners.
///
/// Der ICAO wird auf A–Z und 0–9 verengt, bevor er in einen Pfad geht.
/// Nicht aus Ordnungsliebe: Ein Kürzel wie `../../x` käme sonst aus dem
/// Ordner heraus. Die Kürzel stammen zwar aus unserem eigenen Flugplan,
/// aber ein Pfad, dessen Sicherheit von der Herkunft der Eingabe abhängt,
/// ist nur so lange sicher, bis jemand die Herkunft ändert.
///
/// Diese Fassung nimmt den Ordner als Argument, damit die Dateiarbeit
/// ohne laufende Tauri-App prüfbar ist — die Fassung mit `AppHandle`
/// daneben reicht nur den Ordner durch.
fn datei_in(dir: &std::path::Path, icao: &str) -> Option<PathBuf> {
    let sauber: String = icao
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if sauber.len() < 3 || sauber.len() > 4 {
        return None;
    }
    Some(dir.join(format!("{sauber}.json")))
}

fn datei(app: &AppHandle, icao: &str) -> Option<PathBuf> {
    ordner(app).and_then(|d| datei_in(&d, icao))
}

/// Einen erfolgreich geholten Flugplatz ablegen.
///
/// Fehler sind nie fatal — schlägt das Schreiben fehl, fliegt der Pilot
/// mit den Serverdaten weiter, die er ja gerade bekommen hat. Nur beim
/// NÄCHSTEN Mal ohne Netz fehlt der Rückfall.
pub fn ablegen(app: &AppHandle, icao: &str, airport: &NavAirport, zyklus: &str) {
    let Some(dir) = ordner(app) else { return };
    ablegen_in(&dir, icao, airport, zyklus);
}

/// Wie `ablegen`, aber in einen gegebenen Ordner — prüfbar ohne Tauri.
pub fn ablegen_in(
    dir: &std::path::Path,
    icao: &str,
    airport: &NavAirport,
    zyklus: &str,
) {
    let Some(pfad) = datei_in(dir, icao) else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let eintrag = GespeicherterFlugplatz {
        airport: airport.clone(),
        zyklus: zyklus.to_string(),
        geholt_am: jetzt_unix(),
    };
    let Ok(json) = serde_json::to_string(&eintrag) else {
        return;
    };
    // Erst daneben schreiben, dann umbenennen. Ein abgebrochener
    // Schreibvorgang würde sonst eine halbe Datei hinterlassen, die beim
    // nächsten Start als „vorhanden" gilt und nicht lesbar ist.
    let tmp = pfad.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_err() {
        return;
    }
    if std::fs::rename(&tmp, &pfad).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Grund, warum ein gespeicherter Eintrag nicht verwendet wurde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeinTreffer {
    /// Nichts abgelegt.
    Nichts,
    /// Vorhanden, aber älter als `HOECHSTALTER_TAGE`.
    ZuAlt { tage: u64 },
    /// Datei da, aber nicht lesbar (halb geschrieben, Format geändert).
    Unlesbar,
}

/// Einen abgelegten Flugplatz holen — nur, wenn er jung genug ist.
pub fn holen(app: &AppHandle, icao: &str) -> Result<GespeicherterFlugplatz, KeinTreffer> {
    holen_mit_zeit(app, icao, jetzt_unix())
}

/// Wie `holen`, aber mit vorgegebener Gegenwart — damit das Altern
/// prüfbar ist, ohne die Systemuhr zu stellen.
pub fn holen_mit_zeit(
    app: &AppHandle,
    icao: &str,
    jetzt: u64,
) -> Result<GespeicherterFlugplatz, KeinTreffer> {
    let Some(dir) = ordner(app) else {
        return Err(KeinTreffer::Nichts);
    };
    holen_in(&dir, icao, jetzt)
}

/// Wie `holen_mit_zeit`, aber aus einem gegebenen Ordner — prüfbar ohne
/// Tauri.
pub fn holen_in(
    dir: &std::path::Path,
    icao: &str,
    jetzt: u64,
) -> Result<GespeicherterFlugplatz, KeinTreffer> {
    let Some(pfad) = datei_in(dir, icao) else {
        return Err(KeinTreffer::Nichts);
    };
    let Ok(roh) = std::fs::read_to_string(&pfad) else {
        return Err(KeinTreffer::Nichts);
    };
    let eintrag: GespeicherterFlugplatz =
        serde_json::from_str(&roh).map_err(|_| KeinTreffer::Unlesbar)?;

    match eintrag.alter_tage(jetzt) {
        Some(tage) if tage <= HOECHSTALTER_TAGE => Ok(eintrag),
        Some(tage) => Err(KeinTreffer::ZuAlt { tage }),
        // Eintrag aus der Zukunft: Systemuhr verstellt. Nicht verwenden.
        None => Err(KeinTreffer::Unlesbar),
    }
}

/// Wie viele Flugplätze liegen abgelegt, und wie viel Platz belegen sie?
/// Für die Anzeige in den Einstellungen.
pub fn bestand(app: &AppHandle) -> (usize, u64) {
    let Some(dir) = ordner(app) else {
        return (0, 0);
    };
    bestand_in(&dir)
}

/// Wie `bestand`, aber für einen gegebenen Ordner — prüfbar ohne Tauri.
pub fn bestand_in(dir: &std::path::Path) -> (usize, u64) {
    let Ok(eintraege) = std::fs::read_dir(&dir) else {
        return (0, 0);
    };
    let mut n = 0usize;
    let mut bytes = 0u64;
    for e in eintraege.flatten() {
        if e.path().extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        n += 1;
        if let Ok(m) = e.metadata() {
            bytes += m.len();
        }
    }
    (n, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eintrag(geholt_am: u64) -> GespeicherterFlugplatz {
        GespeicherterFlugplatz {
            airport: NavAirport {
                cycle: "2608".into(),
                valid_to: "2026-09-17".into(),
                icao: "EDDF".into(),
                name: "Frankfurt".into(),
                latitude: 50.033,
                longitude: 8.570,
                elevation_ft: Some(364),
                runways: vec![],
            },
            zyklus: "2608".into(),
            geholt_am,
        }
    }

    /// Ein eigener Ordner je Prüfung. Kein `tempfile`-Zusatzpaket:
    /// Der Name traegt Prozess-Nummer und Zaehler, das genuegt.
    fn probeordner(name: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "aeroacars-navcache-{}-{}-{}",
            name,
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("Probeordner");
        d
    }

    #[test]
    fn abgelegt_und_wieder_gelesen() {
        let d = probeordner("rundlauf");
        ablegen_in(&d, "EDDF", &eintrag(0).airport, "2608");
        let zurueck = holen_in(&d, "EDDF", jetzt_unix()).expect("wieder da");
        assert_eq!(zurueck.airport.icao, "EDDF");
        assert_eq!(zurueck.zyklus, "2608");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn kleinschreibung_und_leerzeichen_finden_dieselbe_datei() {
        // Der Aufrufer normalisiert nicht immer — und zwei Dateien fuer
        // denselben Platz waeren zwei Staende, von denen einer altert.
        let d = probeordner("normalisierung");
        ablegen_in(&d, "eddf", &eintrag(0).airport, "2608");
        assert!(holen_in(&d, "EDDF", jetzt_unix()).is_ok());
        assert!(holen_in(&d, " eddf ", jetzt_unix()).is_ok());
        assert_eq!(bestand_in(&d).0, 1, "es liegen mehrere Dateien fuer einen Platz");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ein_boeses_kuerzel_kommt_nicht_aus_dem_ordner() {
        // `../../x` wuerde sonst ausserhalb schreiben. Die Kuerzel stammen
        // heute aus unserem eigenen Flugplan — ein Pfad, dessen Sicherheit
        // an der Herkunft der Eingabe haengt, ist nur so lange sicher, bis
        // jemand die Herkunft aendert.
        let d = probeordner("pfad");
        for boese in ["../../x", "..", "/etc/passwd", "a/../../b", ""] {
            ablegen_in(&d, boese, &eintrag(0).airport, "2608");
        }
        assert_eq!(bestand_in(&d).0, 0, "ein boeses Kuerzel hat eine Datei angelegt");
        // Und es entstand auch nichts NEBEN dem Ordner.
        let daneben = d.parent().expect("Elternordner").join("x.json");
        assert!(!daneben.exists(), "es wurde ausserhalb des Ordners geschrieben");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn eine_halbe_datei_wird_nicht_als_bestand_ausgegeben() {
        // Abgebrochener Schreibvorgang, geaendertes Format, halb kopiert:
        // Das Ergebnis darf kein Absturz und kein Unsinn sein.
        let d = probeordner("kaputt");
        std::fs::write(d.join("EDDF.json"), "{\"airport\": {\"icao\"").unwrap();
        assert_eq!(holen_in(&d, "EDDF", jetzt_unix()).err(), Some(KeinTreffer::Unlesbar));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ein_leerer_ordner_meldet_keinen_treffer_statt_zu_stuerzen() {
        let d = probeordner("leer");
        assert_eq!(holen_in(&d, "EDDF", jetzt_unix()).err(), Some(KeinTreffer::Nichts));
        assert_eq!(bestand_in(&d), (0, 0));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ein_zu_alter_eintrag_wird_gemeldet_und_nicht_geliefert() {
        let d = probeordner("alt");
        ablegen_in(&d, "EDDF", &eintrag(0).airport, "2608");
        let jetzt = jetzt_unix() + (HOECHSTALTER_TAGE + 5) * SEKUNDEN_JE_TAG;
        match holen_in(&d, "EDDF", jetzt) {
            Err(KeinTreffer::ZuAlt { tage }) => {
                assert!(tage > HOECHSTALTER_TAGE, "{tage} Tage gelten noch als jung");
            }
            anderes => panic!("erwartet ZuAlt, bekommen {anderes:?}"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ein_zweites_ablegen_ersetzt_statt_zu_haeufen() {
        let d = probeordner("ersetzen");
        ablegen_in(&d, "EDDF", &eintrag(0).airport, "2607");
        ablegen_in(&d, "EDDF", &eintrag(0).airport, "2608");
        assert_eq!(bestand_in(&d).0, 1);
        assert_eq!(holen_in(&d, "EDDF", jetzt_unix()).unwrap().zyklus, "2608");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn keine_zwischendatei_bleibt_liegen() {
        // Geschrieben wird erst daneben, dann umbenannt. Bliebe die
        // `.tmp` liegen, zaehlte sie beim naechsten Start mit.
        let d = probeordner("tmp");
        ablegen_in(&d, "EDDF", &eintrag(0).airport, "2608");
        let reste: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .filter(|e| e.path().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(reste.is_empty(), "{} Zwischendatei(en) liegengeblieben", reste.len());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn drei_stellige_kuerzel_gehen_auch() {
        // FAA-Plaetze wie `K7S` sind dreistellig — sie kommen ueber
        // SkyAdventures im Flugplan vor.
        let d = probeordner("dreistellig");
        ablegen_in(&d, "K7S", &eintrag(0).airport, "2608");
        assert!(holen_in(&d, "K7S", jetzt_unix()).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn alter_wird_in_ganzen_tagen_gerechnet() {
        let e = eintrag(1_000_000);
        assert_eq!(e.alter_tage(1_000_000), Some(0));
        assert_eq!(e.alter_tage(1_000_000 + SEKUNDEN_JE_TAG - 1), Some(0));
        assert_eq!(e.alter_tage(1_000_000 + SEKUNDEN_JE_TAG), Some(1));
        assert_eq!(e.alter_tage(1_000_000 + 10 * SEKUNDEN_JE_TAG), Some(10));
    }

    #[test]
    fn alter_tage_jetzt_rechnet_gegen_die_systemuhr() {
        // Die Falle, in die ich beim Verdrahten selbst gelaufen bin:
        // `alter_tage(0)` rechnet gegen 1970 und meldet immer 0 — also
        // genau die Meldung, die das Alter verschweigen soll.
        let frisch = eintrag(jetzt_unix());
        assert_eq!(frisch.alter_tage_jetzt(), 0);
        let alt = eintrag(jetzt_unix().saturating_sub(30 * SEKUNDEN_JE_TAG));
        assert_eq!(alt.alter_tage_jetzt(), 30, "das Alter wird nicht gemeldet");
        assert_eq!(
            alt.alter_tage(0),
            None,
            "gegen 1970 gerechnet ist kein Alter"
        );
    }

    #[test]
    fn ein_eintrag_aus_der_zukunft_gilt_nicht() {
        // Verstellte Systemuhr. `None` statt einer Zahl, damit der
        // Aufrufer ihn verwirft, statt „0 Tage alt" zu lesen.
        let e = eintrag(2_000_000);
        assert_eq!(e.alter_tage(1_000_000), None);
    }

    #[test]
    fn die_hoechstalter_grenze_liegt_bei_einem_halben_jahr() {
        // Sechs AIRAC-Zyklen. Bewusst kein runder Monatswert: Die Grenze
        // hat mit Zyklen zu tun, nicht mit dem Kalender.
        assert_eq!(HOECHSTALTER_TAGE, 183);
        let e = eintrag(0);
        assert_eq!(e.alter_tage(HOECHSTALTER_TAGE * SEKUNDEN_JE_TAG), Some(183));
        assert_eq!(
            e.alter_tage((HOECHSTALTER_TAGE + 1) * SEKUNDEN_JE_TAG),
            Some(184)
        );
    }
}
