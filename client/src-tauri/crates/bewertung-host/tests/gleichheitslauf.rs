//! Gleichheitslauf (Pflichtenheft P-4.43, P-13.01, P-14.03): Das
//! Bewertungsmodul rechnet 2.x-Fluege aus ihrem Flugprotokoll nach und
//! vergleicht mit der dort gespeicherten 2.x-Note. Nur Vergleich — das
//! Ergebnis wird nie als Note gespeichert.
//!
//! 2.x-Fluege haben kein Eingabe-Dokument. Die Eingabe entsteht aus den
//! verdichteten Feldern der Ereignisse `touchdown_complete` (letzter
//! Aufsetzer), `rollout_finalized`, `pirep_filed` und dem Positions-Tick am
//! Aufsetzen (Wind, wahre Fahrt, Fahrwerksart) — Zuordnung wie im Client
//! (`scoring_eingang`, `fill_v2_rollout_fields`). Was das Protokoll nicht
//! traegt, steht je Flug unter `fehlt`; was aus einem Ersatz kommt, unter
//! `nachgespielt`. Achsen, die davon abhaengen, zaehlen nicht als gruen.
//!
//! Aufruf (Auszug aus den Protokollen, siehe Bericht Etappe 3):
//! `GLEICHHEIT_AUSZUG=<auszug.jsonl> GLEICHHEIT_TICKS=<ticks.jsonl>
//! GLEICHHEIT_MANIFEST=<manifest.json des Pakets>
//! GLEICHHEIT_ERGEBNIS=<lauf.json> cargo test -p bewertung-host
//! --test gleichheitslauf -- --ignored --nocapture`.
//! Ergebnis ist ein Dokument im Format des Servers (aeroacars-live,
//! `recorder/src/gleichheitslauf.ts`): `{paket, modul_fassung, gelaufen_at,
//! zeilen}`. Das Paket muss genau die Dateien tragen, mit denen gerechnet
//! wurde (Modul, Grenzwerte, Tabellen) — sonst bricht der Lauf ab.
//! Mit `KORPUS_AUSWAHL=<datei>` (Zeilen `<protokoll> <name>`) und
//! `KORPUS_ZIEL=<ordner>` schreibt er zusaetzlich den Selbsttest-Korpus
//! (P-13.02): je Flug Eingabe und erwartete Ausgabe, ohne Personendaten.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::time::Instant;

use bewertung_host::Host;
use landing_scoring::wasm::{bewerten_json, Eingabe};
use landing_scoring::LandingScoringInput;
use serde_json::{json, Value};

const MODUL: &[u8] = include_bytes!("../../../../../bundle/module/bewertung.wasm");
const VIEL: u64 = 10_000_000_000;

fn f32_(v: &Value) -> Option<f32> {
    v.as_f64().map(|x| x as f32)
}
fn f64_(v: &Value) -> Option<f64> {
    v.as_f64()
}
fn str_(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}
fn bool_(v: &Value) -> Option<bool> {
    v.as_bool()
}

fn grenzwert(bereich: &str, name: &str) -> f32 {
    let pfad = format!("{}/../../../../bundle/grenzwerte/{bereich}.json", env!("CARGO_MANIFEST_DIR"));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(pfad).unwrap()).unwrap();
    v[name]["wert"].as_f64().unwrap() as f32
}

/// Fluggeraet ohne Raeder an Land: Hubschrauber, Wasserflugzeug
/// (`aircraft_category::resolve_category` im Client, Tabelle `flotte.json`).
struct Flotte {
    kategorie: HashMap<String, String>,
    wasser: Vec<String>,
}

impl Flotte {
    fn laden() -> Self {
        let pfad = format!("{}/../../../../bundle/tabellen/flotte.json", env!("CARGO_MANIFEST_DIR"));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(pfad).unwrap()).unwrap();
        let mut kategorie = HashMap::new();
        for (k, liste) in v["kategorien"].as_object().unwrap() {
            for t in liste.as_array().unwrap() {
                kategorie.insert(t.as_str().unwrap().to_string(), k.clone());
            }
        }
        let wasser = v["wasser"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect();
        Self { kategorie, wasser }
    }
    fn kategorie(&self, icao: Option<&str>) -> String {
        icao.map(|i| i.trim().to_ascii_uppercase())
            .and_then(|i| self.kategorie.get(&i).cloned())
            .unwrap_or_else(|| "medium".into())
    }
    fn nicht_konventionell(&self, tick: &Value) -> bool {
        let icao = tick["aircraft_icao"].as_str().map(|i| i.trim().to_ascii_uppercase());
        tick["gear_is_floats"] == true
            || tick["gear_is_skid"] == true
            || tick["water_rudder_present"] == true
            || icao.as_deref().is_some_and(|i| self.kategorie.get(i).is_some_and(|k| k == "heli") || self.wasser.iter().any(|w| w == i))
    }
}

/// Ein Flugprotokoll im Auszug: seine Ereignisse in Reihenfolge.
struct Flug {
    datei: String,
    ereignisse: Vec<Value>,
}

fn letztes<'a>(f: &'a Flug, typ: &str) -> Option<&'a Value> {
    f.ereignisse.iter().rev().find(|e| e["type"] == typ)
}

/// Die Eingabe aus einem 2.x-Protokoll. `fehlt`/`nachgespielt` nennen die
/// Felder, die nicht 1:1 aus dem Protokoll kommen.
struct Nachbau {
    eingabe: Eingabe,
    fehlt: Vec<&'static str>,
    nachgespielt: Vec<&'static str>,
}

/// Grenzwerte aus dem Paket, die der Eingangsbau im Client nutzt.
struct Paket {
    min_bezug_kt: f32,
    vs_boden_fpm: f32,
    g_faktor_xplane: f32,
}

impl Paket {
    fn laden() -> Self {
        Self {
            min_bezug_kt: grenzwert("anflug_kern", "min_bezugsgeschwindigkeit_kt"),
            vs_boden_fpm: grenzwert("aufsetzen", "vs_floor_fpm"),
            g_faktor_xplane: grenzwert("landung", "g_kanal_steigung_msfs") / grenzwert("landung", "g_kanal_steigung_xplane"),
        }
    }
}

fn nachbauen(f: &Flug, tick: Option<&Value>, nicht_konv: bool, p: &Paket) -> Option<Nachbau> {
    let min_bezug_kt = p.min_bezug_kt;
    // Ohne `touchdown_complete` (Upload-Luecke, aeltere Clients) bleibt die
    // Abgabe: dann fehlen alle Felder vom Aufsetzen.
    static LEER: Value = Value::Null;
    let td = letztes(f, "touchdown_complete").map(|e| &e["payload"]).unwrap_or(&LEER);
    let pf = &letztes(f, "pirep_filed")?["payload"];
    let start = letztes(f, "flight_started");
    // Der Ausroll-Abschluss schreibt die Bahnfelder des letzten Aufsetzers
    // fort; er gilt nur, wenn er nach dem letzten Aufsetzer kam.
    let ro = f
        .ereignisse
        .iter()
        .rposition(|e| e["type"] == "touchdown_complete")
        .map_or(&f.ereignisse[..], |i| &f.ereignisse[i..])
        .iter()
        .rev()
        .find(|e| e["type"] == "rollout_finalized")
        .map(|e| &e["payload"]);
    // Anflugwerte zur Abgabe (PIREP), sonst vom Aufsetzen.
    let abgabe = |k: &str| -> &Value { Some(&pf[k]).filter(|v| !v.is_null()).unwrap_or(&td[k]) };
    let bahn = |k: &str| -> &Value { ro.map(|r| &r[k]).filter(|v| !v.is_null()).unwrap_or(&td[k]) };
    let mut fehlt = vec!["plan_strecke_nm", "ias_stddev_unter_500_kt", "stable_config_500"];
    let mut nachgespielt = Vec::new();
    if td.is_null() {
        nachgespielt.push("ohne_touchdown_complete");
    }

    // `hopser_aus_dem_fenster`/`aus_dem_fenster`: ohne Abdeckung keine
    // Hopser und keine G-Spitze im Payload.
    let abdeckung_fehlt = pf["fenster_unzureichend"] == true
        || !td["landung_nicht_bewertbar"].is_null()
        || (!td.is_null() && td["bounce_count"].is_null());
    // Sinkrate wie `canonical_landing_rate_fpm` zur Abgabe: Kante aus der
    // letzten Analyse, sonst die vom Simulator gemessene Spitze. Das
    // Protokoll traegt das Ergebnis nur gerundet (`landing_vs_fpm`); der
    // Kandidat muss darauf passen.
    // Gerundet bzw. bei aelteren Clients abgeschnitten (`as i32`).
    let gerundet = pf["landing_vs_fpm"].as_f64().or(td["vs_fpm"].as_f64());
    let ganz = [pf["landing_vs_fpm"].as_f64(), td["vs_fpm"].as_f64()];
    let passt = |v: f32| ganz.iter().flatten().any(|g| v.round() as f64 == *g || v.trunc() as f64 == *g);
    let kante = letztes(f, "landing_analysis").and_then(|a| f32_(&a["analysis"]["vs_at_edge_fpm"]));
    let spitze = f32_(&pf["landing_peak_vs_fpm"]);
    let plausibel = |v: &f32| v.is_finite() && *v < 0.0 && *v >= p.vs_boden_fpm;
    let kandidaten = if abdeckung_fehlt { vec![spitze] } else { vec![kante, spitze] };
    let vs = kandidaten.into_iter().flatten().find(plausibel);
    let vs_fpm = match (vs, gerundet) {
        (Some(v), Some(_)) if passt(v) => Some(v),
        (_, Some(g)) => {
            nachgespielt.push("vs_nur_gerundet");
            Some(g as f32)
        }
        (_, None) => None,
    };
    // G zur Abgabe (`landing_scored_g_force`, roh) auf der Referenzkette
    // (`g_auf_referenzkette`); sonst der Wert vom Aufsetzen.
    let scored_g = match (f32_(&pf["landing_scored_g_force"]), td["simulator"].as_str()) {
        (Some(g), Some("xplane")) => Some(1.0 + (g - 1.0) * p.g_faktor_xplane),
        (Some(g), _) => Some(g),
        (None, _) => f32_(&td["scored_g_load"]),
    }
    .filter(|_| !abdeckung_fehlt);
    let mut l = LandingScoringInput {
        vs_fpm,
        peak_g_load: f32_(&td["peak_g_load"]),
        scored_g_load: scored_g,
        approach_vs_stddev_fpm: f32_(abgabe("approach_vs_stddev_fpm")),
        approach_bank_stddev_deg: f32_(abgabe("approach_bank_stddev_deg")),
        rollout_distance_m: f32_(bahn("rollout_distance_m")),
        planned_burn_kg: f32_(&pf["planned_burn_kg"]),
        actual_trip_burn_kg: f32_(&pf["actual_trip_burn_kg"]),
        planned_zfw_kg: f32_(&td["planned_zfw_kg"]),
        planned_tow_kg: f32_(&td["planned_tow_kg"]).or_else(|| f32_(&pf["planned_tow_kg"])),
        geflogene_strecke_nm: f32_(&pf["distance_nm"]).filter(|v| *v > 0.0),
        durchstarts: pf["go_around_count"].as_u64().or(td["go_around_count"].as_u64()).unwrap_or(0) as u32,
        aircraft_icao: str_(&td["landing_vref_muster"]),
        fahrwerk_spurweite_m: f64_(bahn("track_width_m")),
        ..Default::default()
    };
    if l.aircraft_icao.is_none() {
        l.aircraft_icao = tick.and_then(|t| str_(&t["aircraft_icao"]));
        nachgespielt.push("aircraft_icao");
    }
    if abdeckung_fehlt {
        // Automaten-Entscheidung „Abdeckung fehlt“ aus dem Protokoll.
        nachgespielt.push("landung_abdeckung_fehlt");
    } else {
        let analyse = letztes(f, "landing_analysis").map(|a| &a["analysis"]).unwrap_or(&LEER);
        l.bounce_count = td["bounce_count"]
            .as_u64()
            .or(td["scored_bounce_count"].as_u64())
            .or(analyse["scored_bounce_count"].as_u64())
            .map(|n| n as u32);
        l.light_bounce_count =
            Some(td["scored_light_bounce_count"].as_u64().or(analyse["scored_light_bounce_count"].as_u64()).unwrap_or(0) as u32);
    }
    l.anflug = landing_scoring::anflug_urteil::AnflugWerte {
        vs_jerk_fpm: f32_(abgabe("approach_vs_jerk_fpm")),
        bank_stddev_deg: f32_(abgabe("approach_bank_stddev_deg")),
        ias_stddev_kt: f32_(abgabe("approach_ias_stddev_kt")),
        excessive_sink: bool_(abgabe("approach_excessive_sink")),
        stable_config: bool_(abgabe("approach_stable_config")),
        gleitpfad_dots: f32_(&pf["anflug_gleitpfad"]["gesamt"]["mittel_abs_dots"]),
        ias_stddev_unter_500_kt: None,
        stable_config_500: None,
        spaetes_final_rechtzeitig: bool_(&td["approach_spaetes_final_rechtzeitig"]),
    };
    // Ausweichflug: eingereichtes Ziel != geplantes (`Ausweichziel`).
    let eingereicht = str_(&pf["arr"]);
    let geplant = start.and_then(|s| str_(&s["arr_airport"]));
    l.diverted = match (&eingereicht, &geplant) {
        (Some(e), Some(g)) => Some(!e.trim().eq_ignore_ascii_case(g.trim())),
        _ => {
            nachgespielt.push("diverted");
            None
        }
    };
    // P-4.14 (b): Nur eine ACHSE aus der Szenerie zaehlt. 2.x uebernahm sie
    // allein bei X-Plane; bei MSFS hiess „uebernommen" nur Breite/Laenge,
    // die Mittellinie blieb Navigraph (CFG2001, FQNP 23).
    l.bahn_geometrie_aus_szenerie =
        Some(bahn("bahn_szenerie_status") == "uebernommen" && td["simulator"].as_str() == Some("xplane"));
    l.abfangen = match &pf["abfangen"] {
        Value::Null => None,
        v => match serde_json::from_value(v.clone()) {
            Ok(a) => Some(a),
            Err(_) => {
                fehlt.push("abfangen");
                None
            }
        },
    };

    // fill_v2_rollout_fields
    l.aim_point_m = f64_(bahn("aim_point_m"));
    l.tdz_end_m = f64_(bahn("td_tdz_length_m"));
    l.bahn_max_querversatz_m = f64_(bahn("max_lateral_offset_m"));
    l.bahn_overrun_m = f64_(bahn("overrun_m"));
    let spur: Vec<(f64, f64)> = bahn("lateral_samples")
        .as_array()
        .map(|a| a.iter().filter_map(|p| Some((p["laengs_m"].as_f64()?, p["quer_m"].as_f64()?))).collect())
        .unwrap_or_default();
    let mess_ende = f64_(bahn("mess_ende_laengs_m"));
    // Proben im Bewertungsfenster: das Protokoll traegt nur die ausgeduennte
    // Spur. Ab 3 Spurpunkten im Fenster sind es sicher >= 3 Proben (die
    // einzige Schwelle, `sub_bahndisziplin`), darunter unbekannt.
    let im_fenster = spur.iter().filter(|(lg, _)| mess_ende.is_none_or(|e| *lg <= e)).count();
    l.bahn_proben = Some(im_fenster);
    if im_fenster < 3 {
        nachgespielt.push("bahn_proben");
    }
    if let Some(bis) = landing_scoring::sub_bahndisziplin::achsen_fenster_bis_m(mess_ende, f64_(bahn("clearance_point_m")), None) {
        if mess_ende.is_none() {
            nachgespielt.push("bahn_achsen_fenster");
        }
        if let Some(b) = landing_scoring::sub_bahndisziplin::achsen_befund(&spur, bis) {
            l.bahn_achsen_abweichung_grad = Some(b.winkel_grad);
            l.bahn_achsen_kreuzt_mitte = Some(b.kreuzt_mitte);
            l.bahn_achsen_groesster_betrag_m = Some(b.groesster_betrag_m);
        }
    }
    l.runway_surface = str_(bahn("runway_surface"));
    l.td_distance_from_threshold_m = f64_(bahn("td_distance_from_threshold_m"));
    l.landing_float_distance_m = f32_(&td["landing_float_distance_m"]);
    l.runway_length_m = f32_(bahn("runway_length_m"));
    l.runway_displaced_threshold_ft = bahn("runway_displaced_threshold_ft").as_f64().map(|v| v as i32);
    l.pre_displaced_threshold = bool_(bahn("pre_displaced_threshold"));
    l.runway_geometry_trusted = bool_(bahn("runway_geometry_trusted"));
    l.runway_match_icao = str_(bahn("runway_match_icao"));
    l.airport_source = l.runway_match_icao.as_ref().map(|_| "runway_match".to_string());
    l.runway_match_ident = str_(bahn("runway_match_ident"));
    l.runway_match_centerline_offset_m = f32_(bahn("runway_match_centerline_offset_m"));
    l.runway_width_m = f32_(bahn("runway_width_m")).filter(|w| *w > 0.0);
    l.landing_heading_true_deg = f32_(&td["heading_true_deg"]);
    l.runway_true_course_deg = f32_(bahn("runway_true_course_deg"));
    l.nicht_konventionell = nicht_konv;
    if nicht_konv {
        l.approach_vs_stddev_fpm = None;
        l.approach_bank_stddev_deg = None;
        l.anflug = Default::default();
    }
    // Wind und wahre Fahrt: Stempel aus dem 50-Hz-Abtaster, im Protokoll nur
    // der naechste 1-Hz-Positions-Tick.
    let brauchbar = |v: Option<f32>| v.filter(|x| x.is_finite() && *x > min_bezug_kt);
    if let Some(t) = tick {
        l.landing_wind_direction_deg = f32_(&t["wind_direction_deg"]);
        l.landing_wind_speed_kt = f32_(&t["wind_speed_kt"]);
    }
    nachgespielt.push("wind_aus_tick");
    let tas = brauchbar(tick.and_then(|t| f32_(&t["true_airspeed_kt"])));
    l.landing_groundspeed_kt = tas
        .or_else(|| match (brauchbar(f32_(&td["gs_kt"])), f32_(&td["headwind_kt"])) {
            (Some(gs), Some(hw)) => brauchbar(Some(gs + hw)),
            (Some(gs), None) => Some(gs),
            (None, _) => None,
        })
        .or_else(|| brauchbar(f32_(&td["ias_kt"])));
    nachgespielt.push("bezugsfahrt_aus_tick");
    Some(Nachbau { eingabe: Eingabe { landung: l }, fehlt, nachgespielt })
}

/// Achsen, deren Eingabe im 2.x-Protokoll fehlt oder ersetzt ist (P-13.01).
fn betroffene_achsen(n: &Nachbau) -> Vec<&'static str> {
    if n.nachgespielt.contains(&"ohne_touchdown_complete") {
        return vec!["*"];
    }
    // plan_strecke_nm und `diverted` (Fuel); Anflug unter 500 ft (Stabilitaet).
    let mut a = vec!["fuel", "stability"];
    if n.nachgespielt.contains(&"vs_nur_gerundet") {
        a.push("landing_rate");
    }
    if n.nachgespielt.contains(&"landung_abdeckung_fehlt") {
        a.extend(["landing_rate", "g_force", "bounces"]);
    }
    if n.nachgespielt.iter().any(|x| x.starts_with("wind") || x.starts_with("bezugsfahrt")) {
        a.push("alignment");
    }
    if n.nachgespielt.iter().any(|x| x.starts_with("bahn_")) {
        a.push("runway_discipline");
    }
    if n.nachgespielt.contains(&"aircraft_icao") {
        a.extend(["rollout", "runway_discipline"]);
    }
    if n.fehlt.contains(&"abfangen") {
        a.push("abfangen");
    }
    a.sort_unstable();
    a.dedup();
    a
}

/// Vergleich einer Teilnote nach P-12.02: Note und Status exakt, Text-
/// Schluessel exakt, Anzeige (gerundete Texte) exakt.
fn teil_vergleich(alt: &Value, neu: &Value) -> Vec<(String, &'static str, Value, Value)> {
    let mut d = Vec::new();
    for (feld, art) in [
        ("score", "note"),
        ("points", "note"),
        ("skipped", "note"),
        ("band", "text"),
        ("label_key", "text"),
        ("rationale_key", "text"),
        ("tip_key", "text"),
        ("reason", "text"),
        ("warning", "text"),
        ("extra", "text"),
        ("gate", "text"),
        ("value_key", "text"),
        ("value", "anzeige"),
        ("value_params", "anzeige"),
        ("messwert", "anzeige"),
    ] {
        let leer = |v: &Value| v.is_null() || v.as_array().is_some_and(|a| a.is_empty()) || v.as_object().is_some_and(|o| o.is_empty());
        let (a, n) = (f32_normiert(&alt[feld]), f32_normiert(&neu[feld]));
        if feld == "messwert" && messwert_angezeigt_gleich(alt["key"].as_str(), &a, &n) {
            continue;
        }
        if !(a == n || (leer(&a) && leer(&n))) {
            d.push((feld.to_string(), art, a, n));
        }
    }
    d
}

/// Anzeigerundung des Messwerts je Achse (P-12.02). Messwerte tragen nur
/// `g_force` (G, 0,01), `alignment` (Anteil der Bahnhaelfte, `sub_alignment`;
/// P-12.02 nennt ihn nicht, 0,01 wie G) und `abfangen` (Sekunden, Anzeige
/// mit einer Nachkommastelle, `abfangen.rs`). Andere Achsen: exakt.
fn messwert_schritt(achse: Option<&str>) -> Option<f64> {
    match achse? {
        "g_force" | "alignment" => Some(0.01),
        "abfangen" => Some(0.1),
        _ => None,
    }
}

fn messwert_angezeigt_gleich(achse: Option<&str>, a: &Value, n: &Value) -> bool {
    match (messwert_schritt(achse), a.as_f64(), n.as_f64()) {
        (Some(s), Some(a), Some(n)) => (a / s).round() == (n / s).round(),
        _ => false,
    }
}

/// Nr. des verglichenen Aufsetzers: 2.x speichert die Note des letzten
/// (`touchdown_complete` je Aufsetzer, Touch-and-Go davor); 3.0 zaehlt ab 1.
fn seq(f: &Flug) -> u64 {
    (f.ereignisse.iter().filter(|e| e["type"] == "touchdown_complete").count() as u64).max(1)
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// Umschlag fuer den Server. Prueft, dass das Paket genau die Dateien traegt,
/// mit denen gerechnet wurde (`GENUTZT`; `bundle(pfad)` liefert sie aus dem
/// Repo), und dass seine `score_version` die Fassung des Moduls ist.
/// Dateien, mit denen dieses Werkzeug rechnet: muessen im Paket stehen.
const GENUTZT: [&str; 5] = [
    "module/bewertung.wasm",
    "grenzwerte/anflug_kern.json",
    "grenzwerte/aufsetzen.json",
    "grenzwerte/landung.json",
    "tabellen/flotte.json",
];

fn umschlag(manifest: &[u8], bundle: impl Fn(&str) -> Option<Vec<u8>>, fassung: u64, gelaufen_at: String, zeilen: Vec<Value>) -> Value {
    let m: Value = serde_json::from_slice(manifest).unwrap();
    let dateien = m["files"].as_array().unwrap();
    // Nur die genutzten Dateien: Texte u. a. aendern die Bewertung nicht.
    for g in GENUTZT {
        let d = dateien.iter().find(|d| d["path"] == g).unwrap_or_else(|| panic!("{g} fehlt im Paket"));
        let b = bundle(g).unwrap_or_else(|| panic!("{g} fehlt im Repo"));
        assert_eq!(sha256_hex(&b), d["sha256"].as_str().unwrap(), "Paket passt nicht: {g}");
    }
    assert_eq!(m["score_version"].as_u64(), Some(fassung), "score_version des Pakets != Fassung des Moduls");
    json!({
        "paket": { "generation": m["generation"].as_u64().unwrap(), "manifest_sha256": sha256_hex(manifest) },
        "modul_fassung": fassung,
        "gelaufen_at": gelaufen_at,
        "zeilen": zeilen,
    })
}

/// 2.x schrieb `f32` ueber `serde_json::Value` (als `f64` verbreitert,
/// 0.1899999976158142), das Modul schreibt die kuerzeste `f32`-Form (0.19).
/// Derselbe Wert — verglichen wird deshalb auf `f32`.
fn f32_normiert(v: &Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => json!(n.as_f64().unwrap() as f32),
        Value::Array(a) => Value::Array(a.iter().map(f32_normiert).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), f32_normiert(x))).collect()),
        x => x.clone(),
    }
}

fn lesen(pfad: &str) -> Vec<Flug> {
    let mut fluege: Vec<Flug> = Vec::new();
    for zeile in BufReader::new(std::fs::File::open(pfad).unwrap()).lines() {
        let Ok(v) = serde_json::from_str::<Value>(&zeile.unwrap()) else { continue };
        if v["type"] == "__datei" {
            fluege.push(Flug { datei: v["datei"].as_str().unwrap().to_string(), ereignisse: Vec::new() });
        } else if let Some(f) = fluege.last_mut() {
            f.ereignisse.push(v);
        }
    }
    fluege
}

/// Je Protokoll der Positions-Tick am letzten Aufsetzer (naechster an
/// `contact_at`) und alle Ticks um alle Aufsetzer.
fn ticks_lesen(pfad: &str) -> HashMap<String, (Option<Value>, Vec<Value>)> {
    let zeit = |s: &str| -> f64 {
        // RFC 3339 mit bis zu 9 Nachkommastellen → Sekunden (nur Abstand).
        let (datum, rest) = s.split_once('T').unwrap();
        let tage: i64 = datum.split('-').map(|x| x.parse::<i64>().unwrap()).fold(0, |a, x| a * 400 + x);
        let rest = rest.trim_end_matches('Z');
        let mut t = rest.split(':');
        let h: f64 = t.next().unwrap().parse().unwrap();
        let m: f64 = t.next().unwrap().parse().unwrap();
        let s: f64 = t.next().unwrap().parse().unwrap();
        tage as f64 * 86_400.0 + h * 3600.0 + m * 60.0 + s
    };
    let mut aus = HashMap::new();
    for zeile in BufReader::new(std::fs::File::open(pfad).unwrap()).lines() {
        let v: Value = serde_json::from_str(&zeile.unwrap()).unwrap();
        let tds = v["td"].as_array().cloned().unwrap_or_default();
        let alle: Vec<Value> = tds.iter().flat_map(|t| t["ticks"].as_array().cloned().unwrap_or_default()).collect();
        let naechster = tds.last().and_then(|t| {
            let c = zeit(t["contact_at"].as_str()?);
            t["ticks"]
                .as_array()?
                .iter()
                .filter(|p| p["timestamp"].is_string())
                .min_by(|a, b| {
                    let da = (zeit(a["timestamp"].as_str().unwrap()) - c).abs();
                    let db = (zeit(b["timestamp"].as_str().unwrap()) - c).abs();
                    da.total_cmp(&db)
                })
                .cloned()
        });
        aus.insert(v["datei"].as_str().unwrap().to_string(), (naechster, alle));
    }
    aus
}

#[test]
#[ignore]
fn gleichheitslauf() {
    let fluege = lesen(&std::env::var("GLEICHHEIT_AUSZUG").unwrap());
    let ticks = ticks_lesen(&std::env::var("GLEICHHEIT_TICKS").unwrap());
    let ziel = std::env::var("GLEICHHEIT_ERGEBNIS").unwrap();
    let manifest = std::fs::read(std::env::var("GLEICHHEIT_MANIFEST").unwrap()).unwrap();
    let mut zeilen: Vec<Value> = Vec::new();
    let mut fassungen = std::collections::BTreeSet::new();
    let mut pireps = std::collections::HashSet::new();
    let auswahl: HashMap<String, String> = std::env::var("KORPUS_AUSWAHL")
        .ok()
        .map(|p| {
            std::fs::read_to_string(p)
                .unwrap()
                .lines()
                .filter_map(|z| z.split_once(' ').map(|(a, b)| (a.to_string(), b.trim().to_string())))
                .collect()
        })
        .unwrap_or_default();
    let korpus_ziel = std::env::var("KORPUS_ZIEL").ok();
    let host = Host::neu(MODUL).unwrap();
    let flotte = Flotte::laden();
    let paket = Paket::laden();
    let mut zaehler: BTreeMap<String, usize> = BTreeMap::new();

    for f in &fluege {
        let td = letztes(f, "touchdown_complete").map(|e| &e["payload"]);
        let pf = letztes(f, "pirep_filed").map(|e| &e["payload"]);
        let erster_tick = letztes(f, "position").map(|e| &e["snapshot"]);
        let (tick, alle_ticks) = ticks.get(&f.datei).cloned().unwrap_or((None, Vec::new()));
        let sim = td.and_then(|t| str_(&t["simulator"])).or_else(|| erster_tick.and_then(|s| str_(&s["simulator"])));
        let profil = erster_tick.and_then(|s| str_(&s["aircraft_profile"]));
        let mut kopf = json!({
            "datei": f.datei,
            "pirep_id": pf.and_then(|p| str_(&p["pirep_id"])),
            "sim": sim,
            "profil": profil,
            "fassung_2x": pf.map(|p| p["score_algorithm_version"].clone()),
            "client_version": pf.and_then(|p| str_(&p["client_version"])),
            "seq": seq(f),
        });
        let ref_teile = pf.and_then(|p| p["sub_scores"].as_array()).filter(|a| !a.is_empty());
        // Der Server verwirft den ganzen Lauf bei verglichener Zeile ohne
        // Kennung oder doppeltem `pirep_id#seq`; solche Fluege zaehlen hier.
        let pirep = pf.and_then(|p| str_(&p["pirep_id"])).filter(|p| !p.is_empty());
        let grund = if pf.is_none() {
            Some("kein_pirep")
        } else if pirep.is_none() {
            Some("keine_pirep_id")
        } else if ref_teile.is_none() {
            Some("keine_2x_teilnoten")
        } else if !pireps.insert((pirep.clone(), seq(f))) {
            Some("doppelt")
        } else {
            None
        };
        if let Some(g) = grund {
            kopf["status"] = json!("nicht_pruefbar");
            kopf["grund"] = json!(g);
            *zaehler.entry(format!("nicht_pruefbar/{g}")).or_default() += 1;
            zeilen.push(kopf);
            continue;
        }
        // Der Client setzt `landing_nicht_konventionell` bei jedem Aufsetzer
        // und nimmt es im Flug nie zurueck — also: irgendein Aufsetzer.
        let nicht_konv = alle_ticks.iter().any(|t| flotte.nicht_konventionell(t));
        let Some(n) = nachbauen(f, tick.as_ref(), nicht_konv, &paket) else { unreachable!() };
        let ein = serde_json::to_string(&n.eingabe).unwrap();
        let t0 = Instant::now();
        let (ausgabe, fuel) = host.bewerten_mit_verbrauch(&ein, VIEL).unwrap();
        let dauer_us = t0.elapsed().as_micros();
        assert_eq!(ausgabe, bewerten_json(&ein), "WASM != nativ: {}", f.datei);
        let neu: Value = serde_json::from_str(&ausgabe).unwrap();
        fassungen.insert(neu["fassung"].as_u64().unwrap());
        let ref_teile = ref_teile.unwrap();
        let betroffen = betroffene_achsen(&n);
        let mut abw = Vec::new();
        let neu_teile = neu["teilnoten"].as_array().unwrap();
        let mut schluessel: Vec<&str> = ref_teile.iter().chain(neu_teile).filter_map(|t| t["key"].as_str()).collect();
        schluessel.dedup();
        let mut gesehen = std::collections::BTreeSet::new();
        for k in schluessel {
            if !gesehen.insert(k) {
                continue;
            }
            let a = ref_teile.iter().find(|t| t["key"] == k);
            let b = neu_teile.iter().find(|t| t["key"] == k);
            match (a, b) {
                (Some(a), Some(b)) => {
                    for (feld, art, x, y) in teil_vergleich(a, b) {
                        abw.push(json!({"achse": k, "feld": feld, "art": art, "alt": x, "neu": y, "eingabe_unvollstaendig": (betroffen.contains(&k) || betroffen == ["*"])}));
                    }
                }
                (a, b) => abw.push(json!({"achse": k, "feld": "vorhanden", "art": "note", "alt": a.is_some(), "neu": b.is_some(), "eingabe_unvollstaendig": (betroffen.contains(&k) || betroffen == ["*"])})),
            }
        }
        let gesamt_alt = pf.unwrap()["landing_score"].clone();
        let gesamt_neu = neu["gesamt"].clone();
        let icao = n.eingabe.landung.aircraft_icao.clone();
        kopf["status"] = json!("verglichen");
        kopf["kategorie"] = json!(flotte.kategorie(icao.as_deref()));
        kopf["muster"] = json!(icao);
        kopf["gesamt_alt"] = gesamt_alt.clone();
        kopf["gesamt_neu"] = gesamt_neu.clone();
        kopf["gesamt_gleich"] = json!(gesamt_alt == gesamt_neu);
        kopf["abweichungen"] = json!(abw);
        kopf["fehlt"] = json!(n.fehlt);
        kopf["nachgespielt"] = json!(n.nachgespielt);
        kopf["betroffene_achsen"] = json!(betroffen);
        kopf["fuel"] = json!(fuel);
        kopf["dauer_us"] = json!(dauer_us);
        *zaehler.entry("verglichen".into()).or_default() += 1;
        zeilen.push(kopf);

        if let (Some(name), Some(ziel)) = (auswahl.get(&f.datei), &korpus_ziel) {
            // Selbsttest-Korpus: nur Eingabe und Ausgabe des Moduls — keine
            // Kennung, kein Name, kein Rufzeichen.
            let fall = json!({ "eingabe": n.eingabe, "ausgabe": neu });
            std::fs::write(format!("{ziel}/{name}.json"), serde_json::to_string_pretty(&fall).unwrap() + "\n").unwrap();
        }
    }
    assert_eq!(fassungen.len(), 1, "Modul meldet mehrere Fassungen: {fassungen:?}");
    let bundle = |pfad: &str| std::fs::read(format!("{}/../../../../bundle/{pfad}", env!("CARGO_MANIFEST_DIR"))).ok();
    let gelaufen_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let lauf = umschlag(&manifest, bundle, *fassungen.first().unwrap(), gelaufen_at, zeilen);
    std::fs::write(ziel, serde_json::to_string(&lauf).unwrap() + "\n").unwrap();
    println!("GLEICHHEITSLAUF paket {} {zaehler:?}", lauf["paket"]);
}

#[test]
fn messwert_wird_nach_anzeigerundung_verglichen() {
    let teil = |k: &str, m: f64| json!({"key": k, "score": 80, "messwert": m});
    // G: 1,231 und 1,234 zeigen beide 1,23.
    assert!(teil_vergleich(&teil("g_force", 1.231), &teil("g_force", 1.234)).is_empty());
    // Gegenprobe: eine Anzeigestelle Unterschied ist eine Abweichung.
    assert_eq!(teil_vergleich(&teil("g_force", 1.234), &teil("g_force", 1.244)).len(), 1);
    assert!(teil_vergleich(&teil("abfangen", 4.21), &teil("abfangen", 4.18)).is_empty());
    assert_eq!(teil_vergleich(&teil("abfangen", 4.21), &teil("abfangen", 4.31)).len(), 1);
    // Achse ohne Rundungsregel: exakt.
    assert_eq!(teil_vergleich(&teil("fuel", 1.231), &teil("fuel", 1.234)).len(), 1);
}

#[test]
fn seq_ist_der_letzte_aufsetzer() {
    let flug = |n: usize| Flug { datei: "x".into(), ereignisse: (0..n).map(|_| json!({"type": "touchdown_complete"})).collect() };
    assert_eq!(seq(&flug(0)), 1);
    assert_eq!(seq(&flug(1)), 1);
    assert_eq!(seq(&flug(2)), 2);
}

fn probe_manifest(modul: &[u8], fassung: u64) -> Vec<u8> {
    let mut files: Vec<Value> = GENUTZT.iter().map(|g| json!({"path": g, "sha256": sha256_hex(&probe_datei(g, modul))})).collect();
    files.push(json!({"path": "texte/de.json", "sha256": sha256_hex(b"nicht im repo")}));
    serde_json::to_vec(&json!({"generation": 27, "score_version": fassung, "files": files})).unwrap()
}

fn probe_datei(pfad: &str, modul: &[u8]) -> Vec<u8> {
    if pfad == "module/bewertung.wasm" { modul.to_vec() } else { pfad.as_bytes().to_vec() }
}

fn probe_bundle(pfad: &str) -> Option<Vec<u8>> {
    GENUTZT.contains(&pfad).then(|| probe_datei(pfad, b"modul"))
}

#[test]
fn umschlag_im_format_des_servers() {
    let m = probe_manifest(b"modul", 27);
    let u = umschlag(&m, probe_bundle, 27, "2026-10-29T22:00:00Z".into(), vec![json!({"datei": "a"})]);
    assert_eq!(u["paket"]["generation"], 27);
    // Server (`bundleIdText`): 12 bis 64 Hex-Zeichen.
    let sha = u["paket"]["manifest_sha256"].as_str().unwrap();
    assert!(sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    assert_eq!(sha, sha256_hex(&m));
    assert_eq!(u["modul_fassung"], 27);
    assert_eq!(u["zeilen"].as_array().unwrap().len(), 1);
}

#[test]
#[should_panic(expected = "Paket passt nicht: module/bewertung.wasm")]
fn umschlag_verweigert_fremdes_modul() {
    let m = probe_manifest(b"modul", 27);
    umschlag(&m, |p| probe_bundle(p).map(|b| if p == "module/bewertung.wasm" { b"anderes modul".to_vec() } else { b }), 27, String::new(), vec![]);
}

#[test]
#[should_panic(expected = "tabellen/flotte.json fehlt im Paket")]
fn umschlag_verweigert_paket_ohne_genutzte_tabelle() {
    let mut m: Value = serde_json::from_slice(&probe_manifest(b"modul", 27)).unwrap();
    m["files"].as_array_mut().unwrap().retain(|d| d["path"] != "tabellen/flotte.json");
    umschlag(&serde_json::to_vec(&m).unwrap(), probe_bundle, 27, String::new(), vec![]);
}

#[test]
#[should_panic(expected = "score_version des Pakets")]
fn umschlag_verweigert_andere_fassung() {
    let m = probe_manifest(b"modul", 26);
    umschlag(&m, probe_bundle, 27, String::new(), vec![]);
}
