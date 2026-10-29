//! Spaetes Final / Kurvenanflug (Score-Version 23, 13.10.2026).
//!
//! # Warum
//!
//! Beim Circling oder einer Platzrunde kommt das Flugzeug erst tief auf die
//! verlaengerte Bahnachse. Das Gate ab 1000 ft wertete die Kurven davor als
//! unruhige Querneigung und die Lage neben dem Gleitpfad als Abweichung —
//! obwohl genau diese Kurven das Verfahren sind.
//!
//! Welches Verfahren geflogen wurde, verraet der Simulator nicht. Erkannt wird
//! deshalb nur die Geometrie: Das Flugzeug liegt erst unter 1000 ft ueber dem
//! Platz dauerhaft auf der Bahnachse und war direkt davor mindestens 20 s am
//! Stueck daneben.
//!
//! # Was ein erkanntes spaetes Final aendert
//!
//! * Querneigungs- und V/S-Streuung sowie der Gleitpfad zaehlen erst ab dem
//!   Ausrichten.
//! * Dafuer gilt die Grenze der Flight Safety Foundation (ALAR 7.1) fuer
//!   Circling: Flugel waagerecht auf dem Final spaetestens 300 ft ueber dem
//!   Platz. Waagerecht heisst hier hoechstens 10° Querneigung — bei 99 % der
//!   geraden Anfluege im GSG-Bestand bleibt sie unter 300 ft darunter.
//!
//! # Grenzen aus dem Bestand
//!
//! Gemessen an 1437 Client-Protokollen (13.10.2026): Mit ±20° Spur und
//! 300 m + 5 % der Entfernung Querablage sind alle geraden Anfluege ueber
//! 1000 ft ausgerichtet; rund 80 Anfluege werden erkannt (GA-Platzrunden,
//! Versatzanfluege wie Tivat, Madeira, Bastia, einzelne Jets). Die Bahnrichtung MUSS aus
//! den Navdaten kommen: aus dem Ausrollen geschaetzt lag sie 7–8° daneben
//! (das Abbiegen lief mit) und machte gerade Anfluege zu spaeten.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ApproachBufferSample;

// Spur hoechstens so weit neben der Bahnrichtung.
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> spur_toleranz_deg (v2.1.24: 20.0)
// Querablage zur verlaengerten Achse: fester Teil …
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> quer_grund_m (v2.1.24: 300.0)
// … plus dieser Anteil der Entfernung zur Schwelle (Trichter).
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> quer_anteil (v2.1.24: 0.05)
// Kuerzere Unterbrechungen zaehlen nicht (Boe, eine verrauschte Probe).
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> luecke_s (v2.1.24: 3.0)
// Laengere Luecke zwischen zwei Proben: Fuer die Zeit dazwischen gibt es
// keine Beobachtung — sie darf nicht als „am Stueck daneben" zaehlen
// (Codex-QS 13.10.2026). Unter 1500 ft tastet der Puffer etwa sekuendlich ab.
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> max_probenabstand_s (v2.1.24: 5.0)
// Die Spur ergibt sich aus der Strecke der letzten Sekunden …
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> spur_zeitraum_s (v2.1.24: 4.0)
// … aber erst ab dieser Strecke (sonst ist sie Rauschen).
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> spur_min_weg_m (v2.1.24: 30.0)
// Erst wer UNTER dieser Hoehe ausgerichtet ist, hat ein spaetes Final.
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> spaet_unter_ft (v2.1.24: 1000.0)
// Fehlausrichtung am Stueck direkt vor dem Final, damit es zaehlt. Ein
// kurzer Schlenker auf geradem Final bleibt darunter. Die Kurve davor ist
// bewusst KEIN zweites Kriterium: ein Schlenker hin und zurueck summiert
// sich schnell auf 90° (QS 13.10.2026) — sie steht nur als Angabe im
// Ergebnis. Im Bestand fielen dadurch 3 von 80 Anfluegen heraus.
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> fehl_min_s (v2.1.24: 20.0)
// Die Kurve vor dem Final wird unter dieser Hoehe aufsummiert.
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> kurve_unter_ft (v2.1.24: 2000.0)
// Spaetestens hier auf dem Final (FSF ALAR 7.1, Circling).
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> final_spaetestens_ft (v2.1.24: 300.0)
// Fluegel waagerecht = hoechstens so viel Querneigung unter 300 ft.
// Wert seit v3: bundle/grenzwerte/spaetes_final.json -> fluegel_waagerecht_deg (v2.1.24: 10.0)

const ERDRADIUS_M: f64 = 6_371_000.0;

/// Ein erkanntes spaetes Final.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpaetesFinal {
    /// Ab hier dauerhaft auf der Bahnachse.
    pub ab: DateTime<Utc>,
    /// Hoehe ueber dem Bezug (Platz oder Grund) beim Ausrichten.
    pub ab_hoehe_ft: f32,
    /// Summe der Spuraenderungen unter 2000 ft vor dem Final.
    pub kurve_deg: f32,
    /// Groesste Querneigung unter 300 ft (ohne die letzten 3 s vor dem
    /// Aufsetzen — das ist das Ausschweben).
    pub querneigung_max_unter_300_deg: f32,
    /// Final spaetestens bei 300 ft UND darunter Fluegel waagerecht.
    pub rechtzeitig: bool,
}

/// Die gelandete Bahn: Schwelle und wahre Richtung aus den Navdaten.
#[derive(Debug, Clone, Copy)]
pub struct Bahnachse {
    pub schwelle_lat: f64,
    pub schwelle_lon: f64,
    pub kurs_wahr_deg: f64,
}

struct Probe {
    at: DateTime<Utc>,
    hoehe_ft: f32,
    bank_deg: f32,
    spur_deg: Option<f64>,
    ausgerichtet: bool,
}

fn winkel_diff(a: f64, b: f64) -> f64 {
    (a - b + 540.0).rem_euclid(360.0) - 180.0
}

fn sekunden(a: DateTime<Utc>, b: DateTime<Utc>) -> f64 {
    (b - a).num_milliseconds() as f64 / 1000.0
}

/// Ortsebene um die Schwelle: (Ost, Nord) in Metern.
fn eben(bahn: &Bahnachse, lat: f64, lon: f64) -> (f64, f64) {
    let x = (lon - bahn.schwelle_lon).to_radians()
        * ERDRADIUS_M
        * bahn.schwelle_lat.to_radians().cos();
    let y = (lat - bahn.schwelle_lat).to_radians() * ERDRADIUS_M;
    (x, y)
}

/// Erkennt ein spaetes Final. `None`, wenn das Flugzeug schon ueber 1000 ft
/// ausgerichtet war, nie ausgerichtet war, Positionen fehlen oder nur ein
/// kurzer Ausreisser vorlag (gerader Anflug — Verhalten wie bisher).
///
/// * `proben`: Anflugproben, aelteste zuerst (der Forensik-Puffer, ~5 min).
/// * `hoehe`: Hoehe ueber dem Bezug je Probe — dieselbe wie im Gate.
/// * `td`: Aufsetzzeitpunkt; nur Proben davor zaehlen.
pub fn erkennen(
    proben: &[&ApproachBufferSample],
    hoehe: impl Fn(&ApproachBufferSample) -> f32,
    td: DateTime<Utc>,
    bahn: &Bahnachse,
    flare_cutoff_ms: i64,
) -> Option<SpaetesFinal> {
    let kurs = bahn.kurs_wahr_deg.rem_euclid(360.0);
    let (ux, uy) = (kurs.to_radians().sin(), kurs.to_radians().cos());

    let mit_pos: Vec<(&ApproachBufferSample, f64, f64)> = proben
        .iter()
        .filter(|s| s.at <= td)
        .filter_map(|s| Some((*s, s.lat?, s.lon?)))
        .collect();
    if mit_pos.len() < 5 {
        return None;
    }
    let ebene: Vec<(f64, f64)> = mit_pos
        .iter()
        .map(|(_, lat, lon)| eben(bahn, *lat, *lon))
        .collect();

    let mut reihe: Vec<Probe> = Vec::with_capacity(mit_pos.len());
    for i in 0..mit_pos.len() {
        let (s, _, _) = mit_pos[i];
        let mut j = i;
        while j > 0 && sekunden(mit_pos[j].0.at, s.at) < crate::grenzwerte::grenzwerte().spaetes_final.spur_zeitraum_s.wert {
            j -= 1;
        }
        let (x, y) = ebene[i];
        let (xa, ya) = ebene[j];
        let weg = (x - xa).hypot(y - ya);
        let spur_deg =
            (weg > crate::grenzwerte::grenzwerte().spaetes_final.spur_min_weg_m.wert).then(|| (x - xa).atan2(y - ya).to_degrees().rem_euclid(360.0));
        let laengs = x * ux + y * uy;
        let quer = x * uy - y * ux;
        let ausgerichtet = match spur_deg {
            // Ohne Spur (Stillstand, Luecke) zaehlt die Probe nicht dagegen.
            None => true,
            Some(sp) => {
                winkel_diff(sp, kurs).abs() <= crate::grenzwerte::grenzwerte().spaetes_final.spur_toleranz_deg.wert
                    && quer.abs() <= crate::grenzwerte::grenzwerte().spaetes_final.quer_grund_m.wert + crate::grenzwerte::grenzwerte().spaetes_final.quer_anteil.wert * laengs.abs()
            }
        };
        reihe.push(Probe {
            at: s.at,
            hoehe_ft: hoehe(s),
            bank_deg: s.bank_deg,
            spur_deg,
            ausgerichtet,
        });
    }

    // Rueckwaerts vom Aufsetzen: der letzte zusammenhaengende ausgerichtete
    // Abschnitt (Unterbrechungen bis LUECKE_S zaehlen nicht).
    let mut idx: Option<usize> = None;
    let mut fehl_seit: Option<DateTime<Utc>> = None;
    let mut abgebrochen = false;
    for k in (0..reihe.len()).rev() {
        if reihe[k].ausgerichtet {
            fehl_seit = None;
            idx = Some(k);
        } else {
            let seit = *fehl_seit.get_or_insert(reihe[k].at);
            if sekunden(reihe[k].at, seit) > crate::grenzwerte::grenzwerte().spaetes_final.luecke_s.wert {
                abgebrochen = true;
                break;
            }
        }
    }
    let idx = idx?;
    // Ausgerichtet bis zum Pufferanfang: das Final begann vor dem Puffer.
    if !abgebrochen {
        return None;
    }
    let ab = &reihe[idx];
    if !(ab.hoehe_ft > 0.0 && ab.hoehe_ft < crate::grenzwerte::grenzwerte().spaetes_final.spaet_unter_ft.wert) {
        return None;
    }

    // Fehlausrichtung am Stueck direkt vor dem Final. Eine Luecke im Puffer
    // beendet das Stueck — dort wurde nichts beobachtet.
    let mut fehl_s = 0.0;
    let mut ok_seit: Option<DateTime<Utc>> = None;
    for k in (0..idx).rev() {
        if sekunden(reihe[k].at, reihe[k + 1].at) > crate::grenzwerte::grenzwerte().spaetes_final.max_probenabstand_s.wert {
            break;
        }
        if reihe[k].ausgerichtet {
            let seit = *ok_seit.get_or_insert(reihe[k].at);
            if sekunden(reihe[k].at, seit) > crate::grenzwerte::grenzwerte().spaetes_final.luecke_s.wert {
                break;
            }
        } else {
            ok_seit = None;
            fehl_s = sekunden(reihe[k].at, ab.at);
        }
    }

    // Kurve vor dem Final unter 2000 ft.
    let mut kurve = 0.0;
    let mut vorige: Option<f64> = None;
    for p in &reihe[..idx] {
        if p.hoehe_ft > crate::grenzwerte::grenzwerte().spaetes_final.kurve_unter_ft.wert {
            vorige = None;
            continue;
        }
        if let Some(sp) = p.spur_deg {
            if let Some(v) = vorige {
                kurve += winkel_diff(sp, v).abs();
            }
            vorige = Some(sp);
        }
    }

    if fehl_s < crate::grenzwerte::grenzwerte().spaetes_final.fehl_min_s.wert {
        return None;
    }

    let querneigung_max = reihe
        .iter()
        .filter(|p| p.hoehe_ft > 0.0 && p.hoehe_ft <= crate::grenzwerte::grenzwerte().spaetes_final.final_spaetestens_ft.wert)
        .filter(|p| (td - p.at).num_milliseconds() >= flare_cutoff_ms)
        .map(|p| p.bank_deg.abs())
        .fold(0.0_f32, f32::max);

    Some(SpaetesFinal {
        ab: ab.at,
        ab_hoehe_ft: ab.hoehe_ft,
        kurve_deg: kurve as f32,
        querneigung_max_unter_300_deg: querneigung_max,
        rechtzeitig: ab.hoehe_ft >= crate::grenzwerte::grenzwerte().spaetes_final.final_spaetestens_ft.wert
            && querneigung_max <= crate::grenzwerte::grenzwerte().spaetes_final.fluegel_waagerecht_deg.wert,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    const M_JE_GRAD_LAT: f64 = 111_194.9;

    fn bahn() -> Bahnachse {
        // Bahn 26 (wahr 260°), Schwelle bei 47,26 N / 11,35 E.
        Bahnachse {
            schwelle_lat: 47.26,
            schwelle_lon: 11.35,
            kurs_wahr_deg: 260.0,
        }
    }

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 12, 0, 0).unwrap()
    }

    /// Punkt `laengs` Meter entlang der Bahnrichtung (negativ = davor) und
    /// `quer` Meter rechts der Achse.
    fn punkt(b: &Bahnachse, laengs: f64, quer: f64) -> (f64, f64) {
        let k = b.kurs_wahr_deg.to_radians();
        let (ux, uy) = (k.sin(), k.cos());
        let x = laengs * ux + quer * uy;
        let y = laengs * uy - quer * ux;
        let lat = b.schwelle_lat + y / M_JE_GRAD_LAT;
        let lon = b.schwelle_lon + x / (M_JE_GRAD_LAT * b.schwelle_lat.to_radians().cos());
        (lat, lon)
    }

    fn probe(at: DateTime<Utc>, lat: f64, lon: f64, hoehe: f32, bank: f32) -> ApproachBufferSample {
        ApproachBufferSample {
            at,
            agl_ft: hoehe,
            msl_ft: hoehe,
            gs_kt: 100.0,
            ias_kt: 100.0,
            vs_fpm: -500.0,
            bank_deg: bank,
            heading_true_deg: 0.0,
            gear_position: 1.0,
            flaps_position: 1.0,
            selected_runway: None,
            stall_warning: false,
            lat: Some(lat),
            lon: Some(lon),
            pitch_deg: None,
            n1_mittel_pct: None,
            headwind_kt: None,
            crosswind_kt: None,
        }
    }

    /// Gerader Anflug: 1 Probe/s, 50 m/s, von 1500 ft auf 0.
    fn gerade(b: &Bahnachse) -> Vec<ApproachBufferSample> {
        let n = 180;
        (0..=n)
            .map(|i| {
                let rest = (n - i) as f64;
                let (lat, lon) = punkt(b, -rest * 50.0, 0.0);
                probe(t0() + Duration::seconds(i as i64), lat, lon, (rest / n as f64 * 1500.0) as f32, 0.5)
            })
            .collect()
    }

    /// Platzrunde: Queranflug quer zur Bahn auf 800 ft, dann eine
    /// 90°-Kurve, Final ab `final_ft` mit `bank_final` Querneigung.
    fn platzrunde(b: &Bahnachse, final_ft: f32, bank_final: f32) -> Vec<ApproachBufferSample> {
        let mut v = Vec::new();
        let mut t = 0;
        // 60 s Queranflug: 1500 m vor der Schwelle, von 3000 m rechts nach 300 m rechts.
        for i in 0..60 {
            let quer = 3000.0 - i as f64 * 45.0;
            let (lat, lon) = punkt(b, -1500.0 - final_ft as f64 * 3.0, quer);
            v.push(probe(t0() + Duration::seconds(t), lat, lon, 800.0, 20.0));
            t += 1;
        }
        // Final: geradeaus, von final_ft auf 0 in 40 s.
        let start = -1500.0 - final_ft as f64 * 3.0;
        for i in 0..=40 {
            let laengs = start + (-start) * i as f64 / 40.0;
            let (lat, lon) = punkt(b, laengs, 0.0);
            let h = final_ft * (40 - i) as f32 / 40.0;
            v.push(probe(t0() + Duration::seconds(t), lat, lon, h, bank_final));
            t += 1;
        }
        v
    }

    fn lauf(v: &[ApproachBufferSample]) -> Option<SpaetesFinal> {
        let refs: Vec<&ApproachBufferSample> = v.iter().collect();
        let td = v.last().unwrap().at;
        erkennen(&refs, |s| s.agl_ft, td, &bahn(), 3000)
    }

    #[test]
    fn gerader_anflug_ist_kein_spaetes_final() {
        assert_eq!(lauf(&gerade(&bahn())), None);
    }

    #[test]
    fn platzrunde_mit_final_ab_600_ft_ist_rechtzeitig() {
        let sf = lauf(&platzrunde(&bahn(), 600.0, 2.0)).expect("erkannt");
        assert!(sf.ab_hoehe_ft < 1000.0 && sf.ab_hoehe_ft > 500.0, "{sf:?}");
        assert!(sf.rechtzeitig);
    }

    #[test]
    fn final_unter_300_ft_ist_nicht_rechtzeitig() {
        let sf = lauf(&platzrunde(&bahn(), 250.0, 2.0)).expect("erkannt");
        assert!(sf.ab_hoehe_ft < 300.0, "{sf:?}");
        assert!(!sf.rechtzeitig);
    }

    #[test]
    fn haengende_fluegel_unter_300_ft_sind_nicht_rechtzeitig() {
        let sf = lauf(&platzrunde(&bahn(), 600.0, 12.0)).expect("erkannt");
        assert!(sf.querneigung_max_unter_300_deg > crate::grenzwerte::grenzwerte().spaetes_final.fluegel_waagerecht_deg.wert);
        assert!(!sf.rechtzeitig);
    }

    /// Das Schlupfloch: ein gerader Anflug mit einem kurzen Schlenker auf
    /// 600 ft darf die Querneigung davor nicht freistellen.
    #[test]
    fn kurzer_schlenker_auf_geradem_final_zaehlt_nicht() {
        let b = bahn();
        let mut v = gerade(&b);
        let n = v.len();
        // 6 s bei ~600 ft 400 m nach rechts versetzt.
        for s in v.iter_mut().skip(n - 75).take(6) {
            let (lat, lon) = punkt(&b, -2000.0, 400.0);
            s.lat = Some(lat);
            s.lon = Some(lon);
        }
        assert_eq!(lauf(&v), None);
    }

    /// Codex-QS: Zwei fehlausgerichtete Proben mit 30 s Luecke dazwischen
    /// sind keine 20 s Fehlausrichtung am Stueck.
    #[test]
    fn luecke_im_puffer_ist_keine_fehlausrichtung() {
        let b = bahn();
        let mut v = gerade(&b);
        let n = v.len();
        // Auf ~600 ft: 4 s daneben, dann 30 s ohne Position, dann wieder 4 s
        // daneben — beobachtet sind nur 8 s.
        for (i, s) in v.iter_mut().enumerate().skip(n - 110).take(38) {
            let rel = i - (n - 110);
            if (4..34).contains(&rel) {
                s.lat = None;
            } else {
                // Parallel zur Achse, 900 m daneben — Spur passt, Ablage nicht.
                let (lat, lon) = punkt(&b, -5500.0 + rel as f64 * 50.0, 900.0);
                s.lat = Some(lat);
                s.lon = Some(lon);
            }
        }
        assert_eq!(lauf(&v), None);
    }

    #[test]
    fn ohne_positionen_keine_erkennung() {
        let mut v = platzrunde(&bahn(), 600.0, 2.0);
        for s in &mut v {
            s.lat = None;
        }
        assert_eq!(lauf(&v), None);
    }
}
