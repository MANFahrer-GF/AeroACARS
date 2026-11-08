//! Stand zu einer Zeit: aus dem lokalen Flugprotokoll `flight_logs/<pirep>.jsonl`
//! (Quelle `protokoll`), sonst aus den Block-Zeiten des PIREP in phpVMS (Quelle
//! `logbuch`, nur was dort steht, nie geraten), sonst `keine`. Dazu die
//! ausgeduennte Spur eines Flugs. Reine Dateiarbeit ohne Tauri; der naechste
//! Flugplatz und der Abruf bei phpVMS kommen vom Aufrufer.
//!
//! Feldnamen des Stands (gleich im Eingang und in `bild_kontext`): `zeit_utc`,
//! `breite`, `laenge`, `hoehe_ft`, `gs_kt`, `kurs`, `flugzeug_titel`,
//! `flugzeug_icao`, `simulator`, `phase`, `pirep_id`, `rufzeichen`, `abflug`, `ziel`.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Map, Value};

use crate::namen::name_pruefen;
use crate::speicher::ms_seit_1970;
use crate::Fehler;

/// So weit darf ein Protokollstand von der gefragten Zeit entfernt sein.
pub const TOLERANZ_MS: i64 = 120_000;
/// Hoechstens so viele Punkte hat eine ausgeduennte Spur.
pub const MAX_SPUR: usize = 600;

pub type Stand = Map<String, Value>;

/// `2026-11-07T14:58:16.123Z` (oder mit Versatz) -> ms seit 1970.
pub fn zeit_ms(iso: &str) -> Option<i64> {
    let t = iso.trim();
    chrono::DateTime::parse_from_rfc3339(t)
        .ok()
        .map(|d| d.timestamp_millis())
        // phpVMS/Carbon zeigt Zeiten manchmal ohne Versatz („2026-11-07 10:00:00“); das ist UTC.
        .or_else(|| chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").ok().map(|n| n.and_utc().timestamp_millis()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Treffer {
    pub stand: Stand,
    pub abstand_ms: i64,
}

#[derive(Debug, Default)]
pub struct Auswertung {
    pub treffer: Vec<Option<Treffer>>,
    pub spur: Vec<[f64; 2]>,
}

/// Zeitstempel einer Zeile, ohne sie ganz zu zerlegen.
fn zeile_zeit(zeile: &str) -> Option<i64> {
    let k = "\"timestamp\":\"";
    let a = zeile.find(k)? + k.len();
    let e = a + zeile[a..].find('"')?;
    zeit_ms(&zeile[a..e])
}

fn zahl(v: &Value, k: &str) -> Value {
    v.get(k).filter(|x| x.is_number()).cloned().unwrap_or(Value::Null)
}

fn text(v: &Value, k: &str) -> Value {
    v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map_or(Value::Null, |s| Value::String(s.to_string()))
}

#[derive(Default, Clone)]
struct Kopf {
    pirep_id: String,
    rufzeichen: Option<String>,
    abflug: Option<String>,
    ziel: Option<String>,
}

fn kopf_lesen(v: &Value, kopf: &mut Kopf) {
    let airline = v["airline_icao"].as_str().unwrap_or("").trim();
    let nr = v["flight_number"].as_str().unwrap_or("").trim();
    kopf.rufzeichen = (!nr.is_empty()).then(|| format!("{airline}{nr}"));
    kopf.abflug = v["dpt_airport"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    kopf.ziel = v["arr_airport"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
}

/// Wertet ein Flugprotokoll (eine JSON-Zeile je Ereignis) fuer die gefragten
/// Zeiten aus: je Zeit der zeitlich naechste `position`-Stand innerhalb von
/// [`TOLERANZ_MS`], mit der Phase und den Flugdaten, die zu diesem Zeitpunkt galten.
pub fn protokoll_auswerten<R: BufRead>(leser: R, pirep_id: &str, zeiten: &[i64], mit_spur: bool) -> Auswertung {
    let mut aus = Auswertung { treffer: vec![None; zeiten.len()], spur: Vec::new() };
    let mut kopf = Kopf { pirep_id: pirep_id.to_string(), ..Default::default() };
    let mut phase: Option<String> = None;
    for zeile in leser.lines().map_while(Result::ok) {
        if zeile.starts_with(r#"{"type":"position""#) {
            let ts = zeile_zeit(&zeile);
            let gefragt = ts.map(|t| zeiten.iter().any(|z| (t - z).abs() <= TOLERANZ_MS)).unwrap_or(false);
            if !gefragt && !mit_spur {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(&zeile) else { continue };
            let s = &v["snapshot"];
            if mit_spur {
                if let (Some(la), Some(lo)) = (s["lat"].as_f64(), s["lon"].as_f64()) {
                    aus.spur.push([la, lo]);
                }
            }
            let Some(t) = ts else { continue };
            for (i, z) in zeiten.iter().enumerate() {
                let d = (t - z).abs();
                if d > TOLERANZ_MS || aus.treffer[i].as_ref().is_some_and(|b| b.abstand_ms <= d) {
                    continue;
                }
                let mut m = Stand::new();
                m.insert("zeit_utc".into(), v["timestamp"].clone());
                m.insert("breite".into(), zahl(s, "lat"));
                m.insert("laenge".into(), zahl(s, "lon"));
                m.insert("hoehe_ft".into(), zahl(s, "altitude_msl_ft"));
                m.insert("gs_kt".into(), zahl(s, "groundspeed_kt"));
                m.insert("kurs".into(), zahl(s, "heading_deg_true"));
                m.insert("flugzeug_titel".into(), text(s, "aircraft_title"));
                m.insert("flugzeug_icao".into(), text(s, "aircraft_icao"));
                m.insert("simulator".into(), text(s, "simulator"));
                m.insert("phase".into(), phase.clone().map_or(Value::Null, Value::String));
                m.insert("pirep_id".into(), json!(kopf.pirep_id));
                m.insert("rufzeichen".into(), kopf.rufzeichen.clone().map_or(Value::Null, Value::String));
                m.insert("abflug".into(), kopf.abflug.clone().map_or(Value::Null, Value::String));
                m.insert("ziel".into(), kopf.ziel.clone().map_or(Value::Null, Value::String));
                aus.treffer[i] = Some(Treffer { stand: m, abstand_ms: d });
            }
        } else if zeile.starts_with(r#"{"type":"phase_changed""#) {
            if let Ok(v) = serde_json::from_str::<Value>(&zeile) {
                phase = v["to"].as_str().map(str::to_string);
            }
        } else if zeile.starts_with(r#"{"type":"flight_started""#) {
            if let Ok(v) = serde_json::from_str::<Value>(&zeile) {
                kopf_lesen(&v, &mut kopf);
            }
        }
    }
    aus
}

/// Gleichmaessig auf hoechstens `max` Punkte ausduennen; erster und letzter Punkt bleiben.
pub fn ausduennen(spur: Vec<[f64; 2]>, max: usize) -> Vec<[f64; 2]> {
    if max < 2 || spur.len() <= max {
        return spur;
    }
    let n = spur.len();
    (0..max).map(|i| spur[i * (n - 1) / (max - 1)]).collect()
}

fn protokoll_datei(dir: &Path, pirep_id: &str) -> Result<PathBuf, Fehler> {
    name_pruefen("PIREP-id", pirep_id)?;
    Ok(dir.join(format!("{pirep_id}.jsonl")))
}

fn oeffnen(pfad: &Path) -> Option<BufReader<fs::File>> {
    crate::sicher::lesend_ohne_folgen(pfad).ok().map(BufReader::new)
}

/// Erste Zeit eines Protokolls (die erste Zeile mit Zeitstempel).
fn erste_zeit(pfad: &Path) -> Option<i64> {
    oeffnen(pfad)?.lines().map_while(Result::ok).take(5).find_map(|z| zeile_zeit(&z))
}

/// Protokolle, deren Zeitraum (erste Zeile bis Aenderungszeit) eine der Zeiten einschliesst.
fn kandidaten(dir: &Path, zeiten: &[i64]) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = protokolle(dir)
        .into_iter()
        .filter(|(_, _, start, ende)| zeiten.iter().any(|z| *z >= start - TOLERANZ_MS && *z <= ende + TOLERANZ_MS))
        .map(|(id, p, _, _)| (id, p))
        .collect();
    v.sort();
    v
}

/// Alle lesbaren Flugprotokolle: Pirep-Kennung, Pfad, erste Zeit, Aenderungszeit.
fn protokolle(dir: &Path) -> Vec<(String, PathBuf, i64, i64)> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut v = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(id) = p.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
        if !crate::namen::name_gueltig(&id) {
            continue;
        }
        let Some(ende) = e.metadata().and_then(|m| m.modified()).ok().and_then(|t: SystemTime| ms_seit_1970(t)) else { continue };
        let Some(start) = erste_zeit(&p) else { continue };
        v.push((id, p, start, ende));
    }
    v
}

/// Je Zeit der naechste Protokollstand (aus allen in Frage kommenden Flugprotokollen,
/// oder nur aus dem von `pirep_id`). Liegt eine Zeit bis `nachlauf_ms` hinter dem Ende eines
/// Protokolls und kein Flug deckt sie ab, gilt dessen letzter Stand (das Bild entstand am Abstellplatz).
pub fn protokoll_staende(dir: &Path, pirep_id: Option<&str>, zeiten: &[i64], nachlauf_ms: i64) -> Result<Vec<Option<Treffer>>, Fehler> {
    let dateien = match pirep_id {
        Some(id) => vec![(id.to_string(), protokoll_datei(dir, id)?)],
        None => kandidaten(dir, zeiten),
    };
    let mut best: Vec<Option<Treffer>> = vec![None; zeiten.len()];
    for (id, p) in dateien {
        let Some(r) = oeffnen(&p) else { continue };
        let a = protokoll_auswerten(r, &id, zeiten, false);
        for (i, t) in a.treffer.into_iter().enumerate() {
            if let Some(t) = t {
                if best[i].as_ref().is_none_or(|b| t.abstand_ms < b.abstand_ms) {
                    best[i] = Some(t);
                }
            }
        }
    }
    if nachlauf_ms > 0 && best.iter().any(Option::is_none) {
        let alle = protokolle(dir);
        for (i, z) in zeiten.iter().enumerate() {
            if best[i].is_none() {
                best[i] = nachlauf_treffer(&alle, pirep_id, *z, nachlauf_ms);
            }
        }
    }
    Ok(best)
}

/// Der letzte Stand des zuletzt beendeten Protokolls, in dessen Nachlauf `zeit` liegt.
fn nachlauf_treffer(alle: &[(String, PathBuf, i64, i64)], pirep_id: Option<&str>, zeit: i64, nachlauf_ms: i64) -> Option<Treffer> {
    let (id, p, _, ende) = alle
        .iter()
        .filter(|(id, _, _, ende)| pirep_id.is_none_or(|x| x == id) && *ende < zeit && zeit <= ende + nachlauf_ms)
        .max_by_key(|(_, _, _, ende)| *ende)?;
    let mut t = protokoll_auswerten(oeffnen(p)?, id, &[*ende], false).treffer.pop().flatten()?;
    t.abstand_ms = zeit - ende;
    Some(t)
}

/// Ausgeduennte Spur eines Flugs aus seinem Protokoll (`[[breite, laenge], ...]`).
pub fn protokoll_spur(dir: &Path, pirep_id: &str) -> Result<Vec<[f64; 2]>, Fehler> {
    let p = protokoll_datei(dir, pirep_id)?;
    let Some(r) = oeffnen(&p) else { return Err(Fehler::NichtGefunden(format!("Kein Flugprotokoll fuer {pirep_id}"))) };
    Ok(ausduennen(protokoll_auswerten(r, pirep_id, &[], true).spur, MAX_SPUR))
}

/// Was von einem PIREP bei phpVMS bekannt ist.
#[derive(Debug, Clone, Default)]
pub struct PirepKurz {
    pub pirep_id: String,
    pub block_off_ms: Option<i64>,
    pub block_on_ms: Option<i64>,
    /// Flugnummer aus phpVMS (kein Funkrufzeichen).
    pub flugnummer: Option<String>,
    pub abflug: Option<String>,
    pub ziel: Option<String>,
    pub flugzeug_icao: Option<String>,
}

/// Der PIREP, dessen Block-Zeiten `zeit` einschliessen, sonst der zuletzt geparkte, in dessen
/// Nachlauf (`nachlauf_ms` ab Block-on) sie liegt. Fehlt eine der beiden Zeiten, passt er nicht.
pub fn logbuch_stand(pireps: &[PirepKurz], zeit: i64, nachlauf_ms: i64) -> Option<Stand> {
    let bloecke = || pireps.iter().filter_map(|p| Some((p, p.block_off_ms?, p.block_on_ms?)));
    let p = bloecke()
        .find(|(_, a, b)| *a <= zeit && zeit <= *b)
        .or_else(|| bloecke().filter(|(_, _, b)| *b < zeit && zeit <= b + nachlauf_ms).max_by_key(|(_, _, b)| *b))
        .map(|(p, _, _)| p)?;
    let opt = |s: &Option<String>| s.clone().filter(|x| !x.is_empty()).map_or(Value::Null, Value::String);
    let mut m = Stand::new();
    m.insert("pirep_id".into(), json!(p.pirep_id));
    m.insert("flugnummer".into(), opt(&p.flugnummer));
    m.insert("abflug".into(), opt(&p.abflug));
    m.insert("ziel".into(), opt(&p.ziel));
    m.insert("flugzeug_icao".into(), opt(&p.flugzeug_icao));
    Some(m)
}

/// So viele Fluege bietet die Zuordnung hoechstens an.
pub const MAX_FLUEGE: usize = 100;

/// Ein Flug, dem ein Bild von Hand zugeordnet werden kann.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlugEintrag {
    pub pirep_id: String,
    /// Nur aus dem Flugprotokoll (Airline und Flugnummer); das Logbuch kennt kein Rufzeichen.
    pub rufzeichen: Option<String>,
    /// Flugnummer aus phpVMS, nur fuer die Anzeige der Liste.
    pub flugnummer: Option<String>,
    pub abflug: Option<String>,
    pub ziel: Option<String>,
    pub flugzeug_icao: Option<String>,
    pub start_ms: Option<i64>,
    pub ende_ms: Option<i64>,
}

impl FlugEintrag {
    pub fn als_json(&self) -> Value {
        json!({
            "pirep_id": self.pirep_id, "rufzeichen": self.rufzeichen, "flugnummer": self.flugnummer, "abflug": self.abflug, "ziel": self.ziel,
            "flugzeug_icao": self.flugzeug_icao, "start_ms": self.start_ms, "ende_ms": self.ende_ms,
        })
    }

    /// Abstand zur Bildzeit; ein Flug, der die Zeit einschliesst, hat 0. Ohne Flugzeit ganz hinten.
    fn abstand(&self, zeit: i64) -> i64 {
        match (self.start_ms, self.ende_ms) {
            (Some(a), Some(b)) if a <= zeit && zeit <= b => 0,
            (Some(a), Some(b)) => (zeit - a).abs().min((zeit - b).abs()),
            (Some(t), None) | (None, Some(t)) => (zeit - t).abs(),
            (None, None) => i64::MAX,
        }
    }
}

/// Die Fluege aus den lokalen Flugprotokollen (auch ohne Verbindung zu phpVMS).
pub fn protokoll_fluege(dir: &Path) -> Vec<FlugEintrag> {
    protokolle(dir)
        .into_iter()
        .map(|(id, p, start, ende)| {
            let mut kopf = Kopf::default();
            // `flight_started` steht am Anfang; ein Protokoll ohne die Zeile bleibt mit Kennung und Zeit anwaehlbar.
            let zeile = oeffnen(&p).and_then(|r| r.lines().map_while(Result::ok).take(50).find(|z| z.starts_with(r#"{"type":"flight_started""#)));
            if let Some(v) = zeile.and_then(|z| serde_json::from_str::<Value>(&z).ok()) {
                kopf_lesen(&v, &mut kopf);
            }
            FlugEintrag { pirep_id: id, rufzeichen: kopf.rufzeichen, flugnummer: None, abflug: kopf.abflug, ziel: kopf.ziel, flugzeug_icao: None, start_ms: Some(start), ende_ms: Some(ende) }
        })
        .collect()
}

/// Protokoll und Logbuch zu einer Liste: je PIREP ein Eintrag, die Block-Zeiten gehen vor,
/// das Rufzeichen kommt nur aus dem Protokoll. Mit `zeit` steht der zeitlich naechste Flug
/// vorn, sonst der neueste. Zweiter Wert: Die Liste wurde auf [`MAX_FLUEGE`] gekuerzt.
pub fn fluege_ordnen(mut fluege: Vec<FlugEintrag>, logbuch: &[PirepKurz], zeit: Option<i64>) -> (Vec<FlugEintrag>, bool) {
    for p in logbuch {
        let i = fluege.iter().position(|f| f.pirep_id == p.pirep_id).unwrap_or_else(|| {
            fluege.push(FlugEintrag { pirep_id: p.pirep_id.clone(), ..Default::default() });
            fluege.len() - 1
        });
        let f = &mut fluege[i];
        let leer = |s: &Option<String>| s.clone().filter(|x| !x.is_empty());
        f.flugnummer = leer(&p.flugnummer);
        f.abflug = f.abflug.take().or_else(|| leer(&p.abflug));
        f.ziel = f.ziel.take().or_else(|| leer(&p.ziel));
        f.flugzeug_icao = f.flugzeug_icao.take().or_else(|| leer(&p.flugzeug_icao));
        f.start_ms = p.block_off_ms.or(f.start_ms);
        f.ende_ms = p.block_on_ms.or(f.ende_ms);
    }
    match zeit {
        Some(z) => fluege.sort_by_key(|f| f.abstand(z)),
        None => fluege.sort_by_key(|f| std::cmp::Reverse(f.start_ms.or(f.ende_ms))),
    }
    let gekuerzt = fluege.len() > MAX_FLUEGE;
    fluege.truncate(MAX_FLUEGE);
    (fluege, gekuerzt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn fluege_aus_protokoll_und_logbuch_je_pirep_einmal_der_naechste_vorn() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("P1.jsonl"), protokoll()).unwrap();
        let start = zeit_ms("2026-11-07T10:00:00Z").unwrap();
        let aus_protokoll = protokoll_fluege(t.path());
        assert_eq!(aus_protokoll.len(), 1);
        assert_eq!(aus_protokoll[0].rufzeichen.as_deref(), Some("GSG123"));
        assert_eq!(aus_protokoll[0].abflug.as_deref(), Some("EDDF"));
        assert_eq!(aus_protokoll[0].start_ms, Some(start));

        let logbuch = [
            PirepKurz { pirep_id: "P1".into(), block_off_ms: Some(start + 60_000), block_on_ms: Some(start + 3_600_000), flugnummer: Some("123".into()), abflug: Some("EDDF".into()), ziel: Some("EGLL".into()), flugzeug_icao: Some("A20N".into()) },
            PirepKurz { pirep_id: "P2".into(), block_off_ms: Some(start - 86_400_000), block_on_ms: Some(start - 82_800_000), flugnummer: Some("77".into()), ziel: Some("".into()), ..Default::default() },
            PirepKurz { pirep_id: "P3".into(), flugnummer: Some("88".into()), ..Default::default() },
        ];
        // Das Bild liegt im Flug P2: P2 steht vorn, obwohl P1 neuer ist.
        let (v, gekuerzt) = fluege_ordnen(aus_protokoll.clone(), &logbuch, Some(start - 84_000_000));
        assert!(!gekuerzt);
        assert_eq!(v.iter().map(|f| f.pirep_id.as_str()).collect::<Vec<_>>(), ["P2", "P1", "P3"]);
        assert_eq!(v[0].rufzeichen, None, "das Logbuch kennt kein Rufzeichen, die Flugnummer ist keins");
        assert_eq!(v[0].flugnummer.as_deref(), Some("77"));
        assert_eq!(v[0].ziel, None, "leere Felder bleiben leer");
        assert_eq!(v[1].rufzeichen.as_deref(), Some("GSG123"), "das Protokoll kennt das Rufzeichen");
        assert_eq!(v[1].flugzeug_icao.as_deref(), Some("A20N"));
        assert_eq!(v[1].start_ms, Some(start + 60_000), "Block-Zeiten gehen vor");

        // Ohne Bildzeit: der neueste zuerst
        let (v, _) = fluege_ordnen(aus_protokoll, &logbuch, None);
        assert_eq!(v[0].pirep_id, "P1");
        assert_eq!(v[2].pirep_id, "P3");
    }

    #[test]
    fn hoechstens_max_fluege() {
        let viele: Vec<_> = (0..MAX_FLUEGE + 5).map(|i| FlugEintrag { pirep_id: i.to_string(), start_ms: Some(i as i64), ..Default::default() }).collect();
        let (v, gekuerzt) = fluege_ordnen(viele, &[], None);
        assert_eq!(v.len(), MAX_FLUEGE);
        assert!(gekuerzt, "die Oberflaeche erfaehrt, dass Fluege fehlen");
    }

    fn pos(ts: &str, lat: f64, lon: f64, alt: f64) -> String {
        format!(
            r#"{{"type":"position","timestamp":"{ts}","snapshot":{{"timestamp":"{ts}","lat":{lat},"lon":{lon},"altitude_msl_ft":{alt},"groundspeed_kt":250.0,"heading_deg_true":90.0,"aircraft_title":"A320 NX","aircraft_icao":"A20N","simulator":"Msfs2024"}}}}"#
        )
    }

    fn protokoll() -> String {
        [
            r#"{"type":"flight_started","timestamp":"2026-11-07T10:00:00Z","pirep_id":"P1","airline_icao":"GSG","flight_number":"123","dpt_airport":"EDDF","arr_airport":"EGLL"}"#.to_string(),
            r#"{"type":"phase_changed","timestamp":"2026-11-07T10:00:30Z","from":"Boarding","to":"Climb","altitude_msl_ft":100.0,"groundspeed_kt":150.0,"altitude_agl_ft":50.0}"#.to_string(),
            pos("2026-11-07T10:01:00Z", 50.0, 8.0, 1000.0),
            pos("2026-11-07T10:02:00Z", 50.1, 8.1, 2000.0),
            r#"{"type":"phase_changed","timestamp":"2026-11-07T10:02:30Z","from":"Climb","to":"Cruise","altitude_msl_ft":2000.0,"groundspeed_kt":250.0,"altitude_agl_ft":1900.0}"#.to_string(),
            r#"{"type":"activity","timestamp":"2026-11-07T10:02:40Z","level":"info","message":"x","detail":null}"#.to_string(),
            pos("2026-11-07T10:03:00Z", 50.2, 8.2, 3000.0),
            "kaputte zeile".to_string(),
        ]
        .join("\n")
    }

    #[test]
    fn zeiten_mit_und_ohne_versatz() {
        assert_eq!(zeit_ms("2026-11-07T10:00:00Z"), zeit_ms("2026-11-07 10:00:00"));
        assert_eq!(zeit_ms("2026-11-07T11:00:00+01:00"), zeit_ms("2026-11-07T10:00:00Z"));
        assert!(zeit_ms("gestern").is_none());
    }

    #[test]
    fn der_naechste_stand_mit_phase_und_flugdaten() {
        let z = |s| zeit_ms(s).unwrap();
        let a = protokoll_auswerten(Cursor::new(protokoll()), "P1", &[z("2026-11-07T10:01:50Z"), z("2026-11-07T10:03:10Z")], false);
        let t0 = a.treffer[0].as_ref().unwrap();
        assert_eq!(t0.abstand_ms, 10_000, "10:02:00 ist naeher als 10:01:00");
        assert_eq!(t0.stand["breite"], 50.1);
        assert_eq!(t0.stand["hoehe_ft"], 2000.0);
        assert_eq!(t0.stand["phase"], "Climb", "die Phase, die um 10:02:00 galt");
        assert_eq!(t0.stand["rufzeichen"], "GSG123");
        assert_eq!((t0.stand["abflug"].as_str(), t0.stand["ziel"].as_str()), (Some("EDDF"), Some("EGLL")));
        assert_eq!(t0.stand["flugzeug_icao"], "A20N");
        assert_eq!(t0.stand["simulator"], "Msfs2024");
        assert_eq!(t0.stand["pirep_id"], "P1");
        let t1 = a.treffer[1].as_ref().unwrap();
        assert_eq!(t1.stand["phase"], "Cruise");
        assert_eq!(t1.abstand_ms, 10_000);
    }

    #[test]
    fn zu_weit_weg_ist_kein_stand_nichts_wird_geraten() {
        let a = protokoll_auswerten(Cursor::new(protokoll()), "P1", &[zeit_ms("2026-11-07T11:00:00Z").unwrap(), zeit_ms("2026-11-07T09:00:00Z").unwrap()], false);
        assert!(a.treffer.iter().all(Option::is_none));
        // knapp ausserhalb der Toleranz
        let b = protokoll_auswerten(Cursor::new(protokoll()), "P1", &[zeit_ms("2026-11-07T10:05:01Z").unwrap()], false);
        assert!(b.treffer[0].is_none());
        let c = protokoll_auswerten(Cursor::new(protokoll()), "P1", &[zeit_ms("2026-11-07T10:04:59Z").unwrap()], false);
        assert!(c.treffer[0].is_some());
    }

    #[test]
    fn spur_ausgeduennt_erster_und_letzter_punkt_bleiben() {
        let a = protokoll_auswerten(Cursor::new(protokoll()), "P1", &[], true);
        assert_eq!(a.spur, vec![[50.0, 8.0], [50.1, 8.1], [50.2, 8.2]]);
        let viele: Vec<[f64; 2]> = (0..5000).map(|i| [i as f64, 0.0]).collect();
        let d = ausduennen(viele.clone(), MAX_SPUR);
        assert_eq!(d.len(), MAX_SPUR);
        assert_eq!((d[0], d[MAX_SPUR - 1]), (viele[0], viele[4999]));
        assert_eq!(ausduennen(vec![[1.0, 2.0]], MAX_SPUR).len(), 1);
    }

    #[test]
    fn das_richtige_protokoll_wird_gefunden_und_pirep_namen_sind_streng() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("P1.jsonl"), protokoll()).unwrap();
        let anderes = protokoll().replace("2026-11-07", "2026-11-06").replace("P1", "P2");
        fs::write(t.path().join("P2.jsonl"), anderes).unwrap();
        fs::write(t.path().join("notiz.txt"), "x").unwrap();
        let z = zeit_ms("2026-11-07T10:02:05Z").unwrap();
        // Aenderungszeit der Dateien ist „jetzt“ (2026-11-07 oder spaeter): beide Zeitraeume werden geprueft
        let r = protokoll_staende(t.path(), None, &[z], 0).unwrap();
        assert_eq!(r[0].as_ref().unwrap().stand["pirep_id"], "P1");
        let nur = protokoll_staende(t.path(), Some("P2"), &[z], 0).unwrap();
        assert!(nur[0].is_none(), "P2 liegt einen Tag davor");
        assert!(protokoll_staende(t.path(), Some("../x"), &[z], 0).is_err());
        assert_eq!(protokoll_spur(t.path(), "P1").unwrap().len(), 3);
        assert!(matches!(protokoll_spur(t.path(), "P9"), Err(Fehler::NichtGefunden(_))));
    }

    #[test]
    fn logbuch_nur_mit_beiden_block_zeiten_und_nur_was_dasteht() {
        let ms = |s| zeit_ms(s);
        let p = PirepKurz {
            pirep_id: "L1".into(),
            block_off_ms: ms("2026-11-07T10:00:00Z"),
            block_on_ms: ms("2026-11-07T12:00:00Z"),
            flugnummer: Some("9".into()),
            abflug: Some("EDDF".into()),
            ziel: None,
            flugzeug_icao: Some("A20N".into()),
        };
        let z = ms("2026-11-07T11:00:00Z").unwrap();
        let s = logbuch_stand(std::slice::from_ref(&p), z, 0).unwrap();
        assert_eq!((s["pirep_id"].as_str(), s["abflug"].as_str(), s["ziel"].is_null()), (Some("L1"), Some("EDDF"), true));
        assert!(!s.contains_key("breite"), "keine Position geraten");
        assert!(logbuch_stand(std::slice::from_ref(&p), ms("2026-11-07T12:00:01Z").unwrap(), 0).is_none());
        let ohne_ende = PirepKurz { block_on_ms: None, ..p.clone() };
        assert!(logbuch_stand(&[ohne_ende], z, 0).is_none(), "ohne Block-on passt nichts");
    }

    fn protokoll_mit_ende(dir: &Path, name: &str, inhalt: String, ende_ms: i64) {
        let p = dir.join(name);
        fs::write(&p, inhalt).unwrap();
        let zeit = SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(ende_ms as u64);
        fs::File::options().write(true).open(&p).unwrap().set_modified(zeit).unwrap();
    }

    #[test]
    fn nachlauf_nimmt_den_letzten_stand_bis_zur_grenze_und_nicht_darueber() {
        let t = tempfile::tempdir().unwrap();
        let ende = zeit_ms("2026-11-07T10:03:05Z").unwrap();
        protokoll_mit_ende(t.path(), "P1.jsonl", protokoll(), ende);
        const FENSTER: i64 = 30 * 60_000;
        let z = ende + 20 * 60_000;
        let r = protokoll_staende(t.path(), None, &[z, ende + FENSTER, ende + FENSTER + 1], FENSTER).unwrap();
        let treffer = r[0].as_ref().unwrap();
        assert_eq!((treffer.stand["pirep_id"].as_str(), treffer.stand["breite"].as_f64()), (Some("P1"), Some(50.2)), "der letzte Stand");
        assert_eq!(treffer.abstand_ms, 20 * 60_000, "der wahre Abstand");
        assert!(r[1].is_some(), "die Grenze gehoert dazu");
        assert!(r[2].is_none(), "danach nichts mehr");
        assert!(protokoll_staende(t.path(), None, &[z], 0).unwrap()[0].is_none(), "Gegenprobe: ohne Nachlauf nichts");
    }

    #[test]
    fn nachlauf_gilt_nicht_vor_dem_ende_und_fragt_nur_den_gewuenschten_flug() {
        let t = tempfile::tempdir().unwrap();
        let ende = zeit_ms("2026-11-07T10:03:05Z").unwrap();
        protokoll_mit_ende(t.path(), "P1.jsonl", protokoll(), ende);
        const FENSTER: i64 = 30 * 60_000;
        let z = ende + 60 * 60_000;
        let vorher = zeit_ms("2026-11-07T09:50:00Z").unwrap();
        assert!(protokoll_staende(t.path(), None, &[vorher], FENSTER).unwrap()[0].is_none(), "nur die Endseite wird verlaengert");
        assert!(protokoll_staende(t.path(), None, &[z], FENSTER).unwrap()[0].is_none());
        protokoll_mit_ende(t.path(), "P9.jsonl", protokoll().replace("P1", "P9"), ende);
        let nur = protokoll_staende(t.path(), Some("P9"), &[ende + 60_000], FENSTER).unwrap();
        assert_eq!(nur[0].as_ref().unwrap().stand["pirep_id"], "P9");
    }

    #[test]
    fn ein_laufender_zweiter_flug_geht_dem_nachlauf_des_ersten_vor() {
        let t = tempfile::tempdir().unwrap();
        let ende1 = zeit_ms("2026-11-07T10:03:05Z").unwrap();
        protokoll_mit_ende(t.path(), "P1.jsonl", protokoll(), ende1);
        let zweiter = protokoll().replace("T10:0", "T10:1").replace("P1", "P2");
        let ende2 = zeit_ms("2026-11-07T10:13:05Z").unwrap();
        protokoll_mit_ende(t.path(), "P2.jsonl", zweiter, ende2);
        let r = protokoll_staende(t.path(), None, &[zeit_ms("2026-11-07T10:12:05Z").unwrap()], 30 * 60_000).unwrap();
        assert_eq!(r[0].as_ref().unwrap().stand["pirep_id"], "P2");
    }

    #[test]
    fn logbuch_nachlauf_nach_block_on_der_zuletzt_geparkte_und_der_flug_selbst_geht_vor() {
        fn ms(s: &str) -> Option<i64> {
            zeit_ms(s)
        }
        let flug = |id: &str, ab: &str, an: &str| PirepKurz { pirep_id: id.into(), block_off_ms: ms(ab), block_on_ms: ms(an), ..Default::default() };
        let erster = flug("L1", "2026-11-07T10:00:00Z", "2026-11-07T11:00:00Z");
        let zweiter = flug("L2", "2026-11-07T11:10:00Z", "2026-11-07T12:00:00Z");
        const FENSTER: i64 = 30 * 60_000;
        let id = |z: &str, l: &[PirepKurz]| logbuch_stand(l, ms(z).unwrap(), FENSTER).map(|s| s["pirep_id"].as_str().unwrap().to_string());
        let beide = [erster.clone(), zweiter];
        assert_eq!(id("2026-11-07T11:05:00Z", &beide).as_deref(), Some("L1"), "im Nachlauf");
        assert_eq!(id("2026-11-07T11:20:00Z", &beide).as_deref(), Some("L2"), "der laufende Flug geht vor");
        assert_eq!(id("2026-11-07T12:20:00Z", &beide).as_deref(), Some("L2"), "der zuletzt geparkte");
        assert_eq!(id("2026-11-07T11:30:00Z", &[erster.clone()]).as_deref(), Some("L1"), "die Grenze gehoert dazu");
        assert_eq!(id("2026-11-07T11:30:01Z", &[erster]), None, "danach nichts mehr");
    }
}
