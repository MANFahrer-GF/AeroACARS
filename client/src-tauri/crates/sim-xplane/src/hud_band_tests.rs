//! Tests fuer `hud_band`.
//!
//! Herleitung der Erwartungen: NICHT aus dem Rust-Code. Die Golden-Datagramme
//! in `testdata/hud_band_golden.json` erzeugt `testdata/hud_band_golden.cjs`,
//! indem es das echte `panel.js` mit Schein-DOM und fester Uhr laeuft
//! (zeichne()/zeichneTicker()) und die entstandenen Texte nach ADR-0005 in
//! Laeufe uebersetzt (Klasse `aa2-g/-w/-b` -> g/w/b, Beschriftung -> d,
//! Statuspunkt -> Art `p`; Art je panel.css-Element). Bewusste Abweichungen rechnet das Skript selbst nach:
//! Wind als Text, kein Spinner. Die woertlichen Strings unten sind Abschriften
//! daraus, damit der Test lesbar bleibt.

use super::*;
use serde_json::json;

const GOLDEN: &str = include_str!("../testdata/hud_band_golden.json");

fn t0() -> i64 {
    chrono::DateTime::parse_from_rfc3339("2026-10-04T12:00:00Z")
        .unwrap()
        .timestamp_millis()
}

/// Eingabe aus der JSON-Form der Golden-Datei bauen und das Datagramm
/// (seq 1) erzeugen.
fn datagramm_aus(e: &Value) -> String {
    let eingabe = BandEingabe {
        verbunden: e["verbunden"].as_bool().unwrap(),
        status: e.get("status"),
        debrief: e.get("debrief"),
        aktivitaet: e.get("aktivitaet"),
        jetzt_ms: e["jetzt_ms"].as_i64().unwrap(),
        host: e["host"].as_str().unwrap(),
        port: e["port"].as_u64().unwrap() as u16,
        fehler_text: e["fehler_text"].as_str(),
        fehler_seit_ms: e["fehler_seit_ms"].as_i64(),
    };
    let f = baue_band(&eingabe);
    f.zu_datagramm(1, f.ruhig)
}

fn golden() -> Vec<Value> {
    serde_json::from_str(GOLDEN).unwrap()
}

#[test]
fn alle_golden_faelle_stimmen_mit_panel_js_ueberein() {
    let faelle = golden();
    assert!(faelle.len() >= 300, "Fixture verdaechtig klein");
    let mut abweichungen = Vec::new();
    for f in &faelle {
        let ist = datagramm_aus(&f["eingabe"]);
        let soll = f["erwartet"].as_str().unwrap();
        if ist != soll {
            abweichungen.push(format!(
                "{}\n  erwartet: {soll:?}\n  erhalten: {ist:?}",
                f["name"]
            ));
        }
    }
    assert!(
        abweichungen.is_empty(),
        "{} von {} Faellen weichen ab:\n{}",
        abweichungen.len(),
        faelle.len(),
        abweichungen
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn die_fixture_deckt_alle_zehn_lagen_ab() {
    // Gegenprobe zur Fixture selbst: ein Fixture ohne Lage wuerde diese
    // Lage nie pruefen.
    let mut gesehen = std::collections::HashSet::new();
    for f in golden() {
        let e = &f["eingabe"];
        gesehen.insert(format!(
            "{:?}",
            lage(e["verbunden"].as_bool().unwrap(), e.get("status"))
        ));
    }
    for l in [
        "Getrennt",
        "Bereit",
        "Pausiert",
        "Unterwegs",
        "Anflug",
        "Auswertung",
        "Ergebnis",
        "Rollen",
        "AmStand",
        "Eingereicht",
    ] {
        assert!(gesehen.contains(l), "Lage {l} fehlt in der Fixture");
    }
}

fn kuratiert(name: &str) -> String {
    let f = golden()
        .into_iter()
        .find(|f| f["name"] == name)
        .unwrap_or_else(|| panic!("Fall {name} fehlt"));
    datagramm_aus(&f["eingabe"])
}

/// Je Lage ein Fall, woertlich (Abschrift aus panel.js-Lauf, siehe oben).
#[test]
fn je_lage_ein_fall_woertlich() {
    let soll: &[(&str, &str)] = &[
        ("lage-getrennt", "BAND 1 0 2\n\nbp\tniAeroACARS\tnsKeine Verbindung - 127.0.0.1:47847\tdlseit\tnv12 s\tdlGrund\tnvZeitueberschreitung nach 3 s\n"),
        ("lage-bereit", "BAND 1 1 2\nntvor 3 s\tdmPIREP prefiled - EDDF > KJFK\ndp\tniAeroACARS\tnsBereit - wartet auf Flug\n"),
        ("lage-pausiert", "BAND 1 0 2\n\nap\tniDLH 400\twsSim getrennt - Flug pausiert\tdlseit\tnv5 min\tdlRoute\tnvEDDF > KJFK\tdxIn AeroACARS fortsetzen\n"),
        ("lage-unterwegs-boden", "BAND 1 1 2\n\ngp\tniDLH 400\tdlRoute\tnvEDDF > KJFK\tdlFuel\tgv5120 / 5200\tdlZFW\twv60000 / 70000\tdlWX\tnvEDDF 240/12kt VIS 10km+ BKN025 12/05 Q1018\tdxBoarding\n"),
        ("lage-unterwegs-flug-ete", "BAND 1 1 2\nntvor 3 s\tdmPIREP prefiled - EDDF > KJFK\ngp\tniDLH 400\tdlRoute\tnvEDDF > KJFK\tdlFL\tnv360\tdlETE\tnv2h05m\tdxReiseflug\n"),
        ("lage-anflug", "BAND 1 0 2\n\ngp\tniDLH 400\tdlV/S\tgv-700\tdlIAS\tnv135\tdlAGL\tnv1020\tdlBank\tnv-1.3\tdlWind\tnvHW 12\twvXW 18 R\tdx~RWY 25C\n"),
        ("lage-auswertung", "BAND 1 0 2\n\ngp\tniDLH 400\tnsLandung wird ausgewertet\n"),
        ("lage-ergebnis",
            "BAND 1 0 2\n\ngp\tniDLH 400\tnz87\tweFIRM\tdlRate\tnv-455\tdlG\tnv1.23\tdlBounces\tnv1\tdlBahn\tnv25C\n"),
        ("lage-rollen",
            "BAND 1 0 2\n\ngp\tniDLH 400\tnz87\tweFIRM\tdlRate\tnv-455\tdxRollen zum Stand\n"),
        ("lage-amstand", "BAND 1 0 2\n\ngp\tniDLH 400\tnsAm Stand\tdlBlock an\tnv11:55:12Z\tdlBlockzeit\tnv3 h 10 min\n"),
        ("lage-eingereicht", "BAND 1 0 2\n\ngp\tniDLH 400\tgsPIREP eingereicht\tdlGesendet\tnvOK\n"),
        ("lage-eingereicht-wartet", "BAND 1 0 2\n\nap\tniDLH 400\twsPIREP wartet\tdlOffen\tnv4\n"),
    ];
    for (name, erwartet) in soll {
        assert_eq!(&kuratiert(name), erwartet, "{name}");
    }
}

#[test]
fn ruhig_nur_unterwegs_und_bereit() {
    for (name, ruhig) in [
        ("lage-bereit", 1),
        ("lage-unterwegs-boden", 1),
        ("lage-getrennt", 0),
        ("lage-pausiert", 0),
        ("lage-anflug", 0),
        ("lage-auswertung", 0),
        ("lage-ergebnis", 0),
        ("lage-rollen", 0),
        ("lage-amstand", 0),
        ("lage-eingereicht", 0),
    ] {
        let d = kuratiert(name);
        assert!(d.starts_with(&format!("BAND 1 {ruhig} ")), "{name}: {d:?}");
    }
}

fn flug(jetzt_ms: i64) -> String {
    let status = json!({
        "phase": "cruise", "takeoff_at": "2026-10-04T10:30:00Z",
        "callsign": "DLH 400", "dpt_airport": "EDDF", "arr_airport": "KJFK",
        "live": {"altitude_pressure_ft": 36000, "ete_min": 125}
    });
    let e = BandEingabe {
        verbunden: true,
        status: Some(&status),
        debrief: None,
        aktivitaet: None,
        jetzt_ms,
        host: "127.0.0.1",
        port: 47847,
        fehler_text: None,
        fehler_seit_ms: None,
    };
    let f = baue_band(&e);
    let datenzeile = f.zeilen[1].zu_text();
    // Zeitzelle = Beschriftung und Wert direkt nach der FL-Zelle.
    let i = datenzeile
        .find("\tdlET")
        .or_else(|| datenzeile.find("\tdlFLT"))
        .unwrap();
    datenzeile[i + 1..]
        .split('\t')
        .take(2)
        .collect::<Vec<_>>()
        .join("|")
}

#[test]
fn zeitzelle_rotiert_im_30_s_takt_ete_eta_flt() {
    // Uhrwerte fest: Fenster 12:00:00 / 12:00:30 / 12:01:00 / 12:01:30.
    // panel.js: i = floor(now/30000) % Kandidaten; Kandidaten ETE, ETA, FLT.
    // Bei 12:00:00Z ist der Index 0 (aus dem panel.js-Lauf der Fixture).
    let a = t0();
    assert_eq!(flug(a), "dlETE|nv2h05m");
    assert_eq!(
        flug(a + 29_999),
        "dlETE|nv2h05m",
        "innerhalb des Fensters stabil"
    );
    assert_eq!(flug(a + 30_000), "dlETA|nv14:05z");
    assert_eq!(flug(a + 60_000), "dlFLT|nv1 h 31 min");
    assert_eq!(flug(a + 90_000), "dlETE|nv2h05m", "zurueck zum Anfang");
}

#[test]
fn zeitzelle_ohne_ete_rotiert_nur_ueber_flt() {
    let status = json!({"phase":"cruise","takeoff_at":"2026-10-04T10:30:00Z"});
    let e = BandEingabe {
        verbunden: true,
        status: Some(&status),
        debrief: None,
        aktivitaet: None,
        jetzt_ms: t0(),
        host: "h",
        port: 1,
        fehler_text: None,
        fehler_seit_ms: None,
    };
    let z = baue_band(&e).zeilen[1].zu_text();
    assert!(z.contains("\tdlFLT\tnv1 h 30 min"), "{z}");
    assert!(!z.contains("ETE") && !z.contains("ETA"), "{z}");
}

#[test]
fn fehlende_felder_werden_zu_strichen_oder_fallen_weg() {
    // Ergebnis ohne Landeprotokoll: Wert und Etikett „--" (panel.js), Zellen weg.
    let status = json!({"phase":"landing","landing_score_finalized":true});
    let e = BandEingabe {
        verbunden: true,
        status: Some(&status),
        debrief: None,
        aktivitaet: None,
        jetzt_ms: t0(),
        host: "h",
        port: 1,
        fehler_text: None,
        fehler_seit_ms: None,
    };
    let f = baue_band(&e);
    assert_eq!(f.lage, Lage::Ergebnis);
    assert_eq!(f.zeilen[1].zu_text(), "gp\tniAeroACARS\tnz--\tge--");
    // Route ohne Flughaefen: ----
    let status = json!({"phase":"cruise","takeoff_at":"2026-10-04T11:00:00Z"});
    let e = BandEingabe {
        status: Some(&status),
        ..e
    };
    assert!(baue_band(&e).zeilen[1].zu_text().contains("nv---- > ----"));
}

fn fiese_faelle() -> Vec<Value> {
    let lang = "x".repeat(500);
    let wort = "wort ".repeat(120);
    let akt = |m: &str, d: Option<&str>| {
        let mut v = json!({"timestamp":"2026-10-04T11:59:00Z","level":"warn","message":m});
        if let Some(d) = d {
            v["detail"] = json!(d);
        }
        v
    };
    let st = |p: &str| {
        json!({"phase": p, "landing_score_finalized": true, "takeoff_at": "2026-10-04T10:00:00Z",
            "callsign": "\u{1F600}\u{00E4}\t\n@", "dpt_airport": "\u{00C4}\u{00D6}", "arr_airport": lang,
            "dep_metar": lang, "arr_metar": wort, "predicted_runway": "\u{2192}\u{26A0}",
            "block_on_at": "kaputt", "block_off_at": "2026-10-04T08:00:00Z",
            "live": {"ete_min": 90, "headwind_kt": -9, "crosswind_kt": -30,
                     "vertical_speed_fpm": -9999, "gs_kt": 300}})
    };
    let mut v = Vec::new();
    for p in [
        "boarding",
        "cruise",
        "descent",
        "final",
        "landing",
        "taxi_in",
        "blocks_on",
        "pirep_submitted",
        "\u{1F4A5}unbekannt",
    ] {
        v.push(json!({"verbunden":true,"status":st(p),
            "debrief":{"score_label":"\u{00E4}\u{00F6}","score_numeric":"\u{1F600}","runway_match":{"runway_ident":lang}},
            "aktivitaet":akt(&lang, Some(&lang)),"jetzt_ms":t0(),"host":"h","port":1}));
    }
    v.push(json!({"verbunden":true,"status":null,"aktivitaet":akt(&wort,None),"jetzt_ms":t0(),"host":"h","port":1}));
    v.push(json!({"verbunden":false,"status":null,"jetzt_ms":t0(),"host":"\u{00FC}","port":1,
        "fehler_text":"Fehler \u{1F600} \u{00E4} \t\n lang lang lang lang lang lang lang lang lang","fehler_seit_ms":0}));
    v.push(json!({"verbunden":true,"status":{"phase":"cruise","takeoff_at":"2026-10-04T10:00:00Z"},
        "aktivitaet":akt("\u{2192} \u{2713} \u{00B0} \u{2026} \u{1F600}", Some("Gr\u{00F6}\u{00DF}e")),"jetzt_ms":t0(),"host":"h","port":1}));
    v
}

#[test]
fn invarianten_ueber_alle_lagen_und_fiese_eingaben() {
    let mut alle: Vec<Value> = golden().iter().map(|f| f["eingabe"].clone()).collect();
    alle.extend(fiese_faelle());
    for e in &alle {
        let d = datagramm_aus_roh(e);
        let n = pruefe_datagramm(&d).unwrap_or_else(|m| panic!("{m}: {d:?} aus {e}"));
        assert!((1..=4).contains(&n));
        for zeile in d.lines().skip(1) {
            assert!(zeile.len() <= 512);
            assert!(zeile
                .bytes()
                .all(|b| (0x20..=0x7e).contains(&b) || b == b'\t'));
        }
    }
}

/// Wie `datagramm_aus`, aber tolerant gegen fehlende Felder (fiese Faelle).
fn datagramm_aus_roh(e: &Value) -> String {
    let eingabe = BandEingabe {
        verbunden: e["verbunden"].as_bool().unwrap(),
        status: e.get("status"),
        debrief: e.get("debrief"),
        aktivitaet: e.get("aktivitaet"),
        jetzt_ms: e["jetzt_ms"].as_i64().unwrap(),
        host: e["host"].as_str().unwrap(),
        port: e["port"].as_u64().unwrap() as u16,
        fehler_text: e.get("fehler_text").and_then(Value::as_str),
        fehler_seit_ms: e.get("fehler_seit_ms").and_then(Value::as_i64),
    };
    let f = baue_band(&eingabe);
    f.zu_datagramm(7, f.ruhig)
}

#[test]
fn ticker_wird_gekuerzt_nicht_abgeschnitten_und_ist_ascii() {
    let akt = json!({"timestamp":"2026-10-04T11:59:57Z","level":"error",
        "message":"Verz\u{00F6}gerung \u{2192} ".to_string() + &"wort ".repeat(60)});
    let e = BandEingabe {
        verbunden: true,
        status: None,
        debrief: None,
        aktivitaet: Some(&akt),
        jetzt_ms: t0(),
        host: "h",
        port: 1,
        fehler_text: None,
        fehler_seit_ms: None,
    };
    let z = baue_band(&e).zeilen[0].zu_text();
    // Alter n, Meldung b (error); Meldung: Umlaut ersetzt, <= 100 Zeichen, endet auf "...".
    let (alter, msg) = z.split_once('\t').unwrap();
    assert_eq!(alter, "ntvor 3 s");
    assert!(msg.starts_with("bmVerzoegerung > wort wort"), "{msg}");
    assert!(msg.ends_with("..."), "{msg}");
    assert!(msg.len() - 1 <= TICKER_MAX, "{}", msg.len());
}

#[test]
fn zeile_ueber_512_byte_wird_im_lauf_gekuerzt_und_bleibt_gueltig() {
    let z = Zeile {
        laeufe: vec![
            Lauf::neu(Farbe::Normal, Art::Wert, &"a".repeat(300)),
            Lauf::neu(Farbe::Warnung, Art::Wert, &"b".repeat(300)),
            Lauf::neu(Farbe::Gut, Art::Wert, "nie sichtbar"),
        ],
    };
    let t = z.zu_text();
    assert_eq!(t.len(), 512);
    assert!(t.ends_with("..."));
    let laeufe: Vec<&str> = t.split('\t').collect();
    assert_eq!(laeufe.len(), 2, "dritter Lauf faellt weg");
    assert!(laeufe[1].starts_with("wv"));
}

#[test]
fn statuspunkt_hat_nie_text_und_ascii_gilt_fuer_alle_arten() {
    assert_eq!(Lauf::punkt(Farbe::Gut).text(), "");
    assert_eq!(Lauf::neu(Farbe::Gut, Art::Punkt, "x").text(), "");
    let z = Zeile {
        laeufe: vec![
            Lauf::punkt(Farbe::Akzent),
            Lauf::neu(Farbe::Normal, Art::Kennung, "\u{00C4}\tB\n@"),
        ],
    };
    assert_eq!(z.zu_text(), "ap\tniAeB@");
}

#[test]
fn pruefer_lehnt_verstoesse_ab() {
    // Gegenprobe: der Pruefer der Invarianten muss rot werden koennen.
    assert_eq!(
        pruefe_datagramm("BAND 1 0 2\n\nnp\n"),
        Ok(2),
        "leere Zeile ok"
    );
    assert_eq!(pruefe_datagramm("BAND 1 0 1\nnvabc\tdl\n"), Ok(1));
    assert!(pruefe_datagramm("BAND 1 0 1\nnv\u{00E4}\n").is_err());
    assert!(pruefe_datagramm("BAND 1 0 1\nnvx\u{1}\n").is_err());
    assert!(pruefe_datagramm("BAND 1 2 1\nnvx\n").is_err());
    assert!(pruefe_datagramm("BAND 1 0 5\n\n\n\n\n\n").is_err());
    assert!(pruefe_datagramm("BAND 1 0 2\nnvx\n").is_err());
    assert!(pruefe_datagramm("BAND 1 0 1\nxvfalschefarbe\n").is_err());
    assert!(pruefe_datagramm("BAND 1 0 1\nnqfalscheart\n").is_err());
    assert_eq!(
        pruefe_datagramm("BAND 1 0 1\nnz87\twePIREP\n"),
        Ok(1),
        "Note und Etikett"
    );
    assert!(pruefe_datagramm("BAND 1 0 1\nn\n").is_err(), "Art fehlt");
    assert!(
        pruefe_datagramm("BAND 1 0 1\nnv\t\n").is_err(),
        "leerer Lauf"
    );
    assert!(
        pruefe_datagramm("BAND 1 0 1\ngp@\n").is_err(),
        "Punkt mit Text"
    );
    assert!(pruefe_datagramm(&format!("BAND 1 0 1\nnv{}\n", "a".repeat(511))).is_err());
    assert!(pruefe_datagramm("BAND 2147483648 0 1\nnvx\n").is_err());
    assert!(
        pruefe_datagramm("BAND 1 0 1\nnvx").is_err(),
        "Zeilenende fehlt"
    );
}

#[test]
fn aus_datagramm_blendet_das_band_aus() {
    assert_eq!(aus_datagramm(7), "BAND 7 0 0\n");
    assert_eq!(pruefe_datagramm(&aus_datagramm(7)), Ok(0));
    assert_eq!(aus_datagramm(u32::MAX), "BAND 2147483647 0 0\n");
}

#[test]
fn js_zahlenformat_wie_javascript() {
    // Erwartete Werte aus node: (-1.25).toFixed(1)="-1.3", (2.5).toFixed(0)="3",
    // (1.005).toFixed(2)="1.00", (-0.4).toFixed(0)="-0", (0.125).toFixed(2)="0.13",
    // (999.5).toFixed(0)="1000", (0).toFixed(2)="0.00"
    assert_eq!(to_fixed(-1.25, 1), "-1.3");
    assert_eq!(to_fixed(2.5, 0), "3");
    assert_eq!(to_fixed(1.005, 2), "1.00");
    assert_eq!(to_fixed(-0.4, 0), "-0");
    assert_eq!(to_fixed(0.125, 2), "0.13");
    assert_eq!(to_fixed(999.5, 0), "1000");
    assert_eq!(to_fixed(0.0, 2), "0.00");
    // Math.round: -0.5 -> -0, 2.5 -> 3, -2.5 -> -2
    assert_eq!(rund(-0.5), 0);
    assert_eq!(rund(2.5), 3);
    assert_eq!(rund(-2.5), -2);
}
