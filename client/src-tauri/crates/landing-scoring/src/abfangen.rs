//! Abfangen (Flare) über die Höhe — Messung und Teilnote (05.10.2026).
//!
//! # Warum neu
//!
//! Die alte Flare-Erkennung (`compute_landing_analysis`) sucht die steilste
//! Sinkrate in den festen 2 s vor dem Aufsetzen. Wer früh abfängt und dann
//! schwebt, hat in diesen 2 s längst keine Sinkrate mehr — QAF434 und THY39
//! meldeten „Kein Flare", obwohl sauber abgefangen wurde. Hier wird über die
//! HÖHE gemessen: vom letzten Durchgang durch 50 ft bis zum Aufsetzen.
//!
//! # Warum es Punkte gibt
//!
//! Bis Score-Version 19 zählte der Flare nur indirekt (Sinkrate 3, G 3,
//! Aufsetzpunkt 1). Langes Schweben lohnte sich damit: weich aufgesetzt,
//! kaum Abzug für den langen Weg. Der Live-Korpus (05.10.2026, 409 Landungen
//! mit Positionsspur) zeigt für Linienflugzeuge Median 8 s ab 50 ft, p90
//! ~12 s; ab 12 s setzt über die Hälfte hinter der Aufsetzzone auf, und die
//! Sinkrate wird nach 8 s kaum noch weicher. Bänder und Gewicht 2:
//! Entscheidung Thomas, 05.10.2026.
//!
//! # Höhe
//!
//! Die Höhe ist relativ zur Höhe beim ersten Bodenkontakt. So fällt der
//! Versatz des Bezugspunkts weg (B772: der Sim meldet am Aufsetzen ~16 ft
//! über Grund) — und es ist dieselbe Rechnung wie im Korpus, aus dem die
//! Bänder stammen (Server: `alt_ft` minus `alt_ft` am ersten Bodenpunkt).

use serde::{Deserialize, Serialize};

use crate::sub_rollout::{category_for_icao, Category};
use crate::{Band, SubScoreEntry};

/// Ab dieser Höhe über dem Aufsetzbezug wird gemessen.
pub const ABFANG_HOEHE_FT: f32 = 50.0;
/// Sinkrate, ab der das Flugzeug als „fast waagerecht" gilt — Beginn des
/// Schwebens. Nur Anzeige, keine Note.
pub const SCHWEBEN_AB_VS_FPM: f32 = -100.0;
/// So viel muss die Sinkrate unter den 50-ft-Wert zurückgehen, damit der
/// Flare als begonnen gilt — dieselbe 50-fpm-Schwelle wie die bisherige
/// Erkennung (`MSFS_FLARE_DETECT_FLOOR_FPM`). Nur Anzeige.
pub const BEGINN_REDUKTION_FPM: f32 = 50.0;
/// Wieder steigen im Abfangen (Ballooning): darüber höchstens 50 Punkte.
/// Korpus: > 0 fpm bei 9 %, > 50 fpm bei 3 % der Landungen.
pub const BALLOONING_VS_FPM: f32 = 50.0;
/// Mindestens so viele Messpunkte zwischen 50 ft und Aufsetzen.
const MIN_PROBEN: usize = 3;

/// Ein Messpunkt des Anflugs vor dem Aufsetzen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbfangPunkt {
    /// Millisekunden relativ zum Aufsetzen (negativ = davor).
    pub t_ms: i64,
    /// Höhe über dem Aufsetzbezug in Fuß (siehe Modulkopf).
    pub hoehe_ft: f32,
    pub vs_fpm: f32,
    pub gs_kt: f32,
}

/// Messwerte des Abfangens — reine Messung, die Teilnote steht daneben.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Abfangen {
    /// Sekunden vom letzten Durchgang durch 50 ft bis zum Aufsetzen.
    #[serde(default)]
    pub dauer_ab_50ft_s: Option<f32>,
    /// Sinkrate beim Durchgang durch 50 ft.
    #[serde(default)]
    pub vs_50ft_fpm: Option<f32>,
    /// Höhe, ab der die Sinkrate um mindestens 50 fpm zurückging.
    #[serde(default)]
    pub beginn_hoehe_ft: Option<f32>,
    /// Sinkrate am Aufsetzen — dieselbe Zahl wie `vs_at_edge_fpm`.
    #[serde(default)]
    pub vs_aufsetzen_fpm: Option<f32>,
    /// Aufsetzen minus 50 ft: positiv = Sinkrate abgebaut.
    #[serde(default)]
    pub reduktion_fpm: Option<f32>,
    /// Sekunden fast waagerecht (Sinkrate über −100 fpm) vor dem Aufsetzen.
    #[serde(default)]
    pub schweben_s: Option<f32>,
    /// Strecke dabei über Grund, in Metern.
    #[serde(default)]
    pub schweben_m: Option<f32>,
    /// Größte Vertikalgeschwindigkeit ab 50 ft; > 0 = wieder gestiegen.
    #[serde(default)]
    pub max_vs_fpm: Option<f32>,
    /// Warum es keine Werte gibt: `kein_50ft_durchgang`, `zu_wenig_proben`.
    #[serde(default)]
    pub grund_ohne_werte: Option<String>,
}

fn runden(v: f32, stellen: i32) -> f32 {
    let f = 10f32.powi(stellen);
    (v * f).round() / f
}

/// Misst das Abfangen. `punkte` aufsteigend nach Zeit, nur vor dem
/// Aufsetzen; `vs_aufsetzen_fpm` ist die veröffentlichte Aufsetz-Sinkrate.
pub fn messen(punkte: &[AbfangPunkt], vs_aufsetzen_fpm: Option<f32>) -> Abfangen {
    let ohne = |grund: &str| Abfangen {
        vs_aufsetzen_fpm,
        grund_ohne_werte: Some(grund.to_string()),
        ..Default::default()
    };
    let p: Vec<&AbfangPunkt> = punkte.iter().filter(|p| p.t_ms <= 0).collect();
    // Letzter Abwärts-Durchgang durch 50 ft (ein Durchstarten davor zählt nicht).
    let Some(i) = (1..p.len())
        .rev()
        .find(|&i| p[i - 1].hoehe_ft > ABFANG_HOEHE_FT && p[i].hoehe_ft <= ABFANG_HOEHE_FT)
    else {
        return ohne("kein_50ft_durchgang");
    };
    let (a, b) = (p[i - 1], p[i]);
    let anteil = (a.hoehe_ft - ABFANG_HOEHE_FT) / (a.hoehe_ft - b.hoehe_ft);
    let t50 = a.t_ms as f32 + (b.t_ms - a.t_ms) as f32 * anteil;
    let vs50 = a.vs_fpm + (b.vs_fpm - a.vs_fpm) * anteil;
    let danach = &p[i..];
    if danach.len() < MIN_PROBEN {
        return ohne("zu_wenig_proben");
    }

    let beginn_hoehe_ft = danach
        .iter()
        .find(|q| q.vs_fpm >= vs50 + BEGINN_REDUKTION_FPM)
        .map(|q| runden(q.hoehe_ft, 0));
    let (schweben_s, schweben_m) = match danach.iter().position(|q| q.vs_fpm >= SCHWEBEN_AB_VS_FPM)
    {
        Some(k) => {
            let s = -(danach[k].t_ms as f32) / 1000.0;
            let gs: Vec<f32> = danach[k..]
                .iter()
                .map(|q| q.gs_kt)
                .filter(|g| g.is_finite())
                .collect();
            let m =
                (!gs.is_empty()).then(|| s * gs.iter().sum::<f32>() / gs.len() as f32 * 0.514_444);
            (Some(runden(s, 1)), m.map(|m| runden(m, 0)))
        }
        None => (Some(0.0), Some(0.0)),
    };
    let max_vs = danach
        .iter()
        .map(|q| q.vs_fpm)
        .fold(f32::NEG_INFINITY, f32::max);

    Abfangen {
        dauer_ab_50ft_s: Some(runden(-t50 / 1000.0, 1)),
        vs_50ft_fpm: Some(runden(vs50, 0)),
        beginn_hoehe_ft,
        vs_aufsetzen_fpm,
        reduktion_fpm: vs_aufsetzen_fpm.map(|v| runden(v - vs50, 0)),
        schweben_s,
        schweben_m,
        max_vs_fpm: Some(runden(max_vs, 0)),
        grund_ohne_werte: None,
    }
}

/// Bandgrenzen (obere Grenzen in Sekunden für 100 / 80 / 50 Punkte).
fn grenzen(icao: Option<&str>) -> [f32; 3] {
    match category_for_icao(icao) {
        // GA, Bizjet, unbekannt — Korpus Median 12,5 s, p90 19,5 s.
        Category::Light => [16.0, 20.0, 24.0],
        Category::Medium | Category::Heavy => [10.0, 12.0, 15.0],
    }
}

/// Teilnote „Abfangen". `None` ohne Messung — dann erscheint die Achse gar
/// nicht (Altbestand, fehlende Daten), statt als Strafe.
pub fn sub_abfangen(abfangen: Option<&Abfangen>, icao: Option<&str>) -> Option<SubScoreEntry> {
    const KEY: &str = "abfangen";
    const LABEL: &str = "landing.sub.abfangen";
    let a = abfangen?;
    let dauer = a.dauer_ab_50ft_s.filter(|d| d.is_finite())?;
    let [gut, lang, sehr_lang] = grenzen(icao);
    let (mut punkte, mut band, mut grund) = if dauer <= gut {
        (100u8, Band::Good, "flare_normal")
    } else if dauer <= lang {
        (80, Band::Good, "flare_lang")
    } else if dauer <= sehr_lang {
        (50, Band::Ok, "flare_sehr_lang")
    } else {
        (25, Band::Bad, "flare_zu_lang")
    };
    if a.max_vs_fpm.is_some_and(|v| v > BALLOONING_VS_FPM) && punkte > 50 {
        (punkte, band, grund) = (50, Band::Ok, "ballooning");
    }
    let wert = format!("{dauer:.1} s ab 50 ft");
    Some(SubScoreEntry::scored(KEY, LABEL, punkte, wert, grund, band).mit_messwert(dauer))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(t_ms: i64, hoehe_ft: f32, vs_fpm: f32) -> AbfangPunkt {
        AbfangPunkt {
            t_ms,
            hoehe_ft,
            vs_fpm,
            gs_kt: 133.0,
        }
    }

    /// THY39 (A359, LTFM 35R, 05.10.2026), Server-Spur aus der Live-DB:
    /// 11 s ab 50 ft, 820 m hinter der Schwelle, −46 fpm.
    fn thy39() -> Vec<AbfangPunkt> {
        [
            (-12463, 62.0, -681.0),
            (-11958, 58.0, -671.0),
            (-11403, 51.0, -654.0),
            (-10949, 48.0, -641.0),
            (-10394, 43.0, -624.0),
            (-9890, 39.0, -607.0),
            (-9383, 34.0, -576.0),
            (-8880, 30.0, -534.0),
            (-8375, 27.0, -486.0),
            (-7871, 25.0, -431.0),
            (-7366, 22.0, -383.0),
            (-6813, 20.0, -346.0),
            (-6308, 18.0, -338.0),
            (-5802, 15.0, -348.0),
            (-5299, 12.0, -357.0),
            (-4795, 10.0, -338.0),
            (-4291, 7.0, -294.0),
            (-3787, 6.0, -238.0),
            (-3283, 5.0, -169.0),
            (-2780, 4.0, -100.0),
            (-2277, 3.0, -57.0),
            (-1722, 1.0, -36.0),
            (-1216, 2.0, -38.0),
            (-712, 2.0, -47.0),
            (-206, 1.0, -51.0),
        ]
        .into_iter()
        .map(|(t, h, v)| p(t, h, v))
        .collect()
    }

    /// QAF434 (B772): früh abgefangen, dann ~3 s Schweben mit leichtem Steigen.
    fn qaf434() -> Vec<AbfangPunkt> {
        [
            (-10066, 69.0, -961.0),
            (-9560, 60.0, -953.0),
            (-9056, 49.0, -938.0),
            (-8551, 42.0, -913.0),
            (-8046, 36.0, -881.0),
            (-7539, 28.0, -837.0),
            (-6983, 21.0, -759.0),
            (-6478, 16.0, -654.0),
            (-5973, 11.0, -532.0),
            (-5469, 7.0, -420.0),
            (-4964, 4.0, -310.0),
            (-4459, 2.0, -220.0),
            (-3904, 1.0, -124.0),
            (-3390, 0.0, -51.0),
            (-2883, 0.0, 4.0),
            (-2379, 0.0, 31.0),
            (-1872, 0.0, 35.0),
            (-1366, 0.0, 23.0),
            (-861, 0.0, -6.0),
            (-356, 0.0, -48.0),
        ]
        .into_iter()
        .map(|(t, h, v)| p(t, h, v))
        .collect()
    }

    #[test]
    fn thy39_flare_wird_erkannt_und_ist_lang() {
        let a = messen(&thy39(), Some(-46.0));
        assert_eq!(a.grund_ohne_werte, None);
        assert_eq!(a.dauer_ab_50ft_s, Some(11.3));
        assert_eq!(a.vs_50ft_fpm, Some(-650.0));
        assert_eq!(a.reduktion_fpm, Some(604.0));
        assert_eq!(a.beginn_hoehe_ft, Some(34.0));
        assert_eq!(a.schweben_s, Some(2.8));
        assert_eq!(a.max_vs_fpm, Some(-36.0));
        let s = sub_abfangen(Some(&a), Some("A359")).unwrap();
        assert_eq!(s.points, 80);
        assert_eq!(s.rationale_key.as_deref(), Some("landing.rat.flare_lang"));
    }

    #[test]
    fn qaf434_frueher_flare_mit_schweben() {
        let a = messen(&qaf434(), Some(-86.0));
        assert_eq!(a.dauer_ab_50ft_s, Some(9.1));
        assert_eq!(a.reduktion_fpm, Some(853.0));
        assert_eq!(a.schweben_s, Some(3.4));
        assert_eq!(a.max_vs_fpm, Some(35.0));
        // 9,1 s und nur +35 fpm Steigen: im Rahmen.
        let s = sub_abfangen(Some(&a), Some("B772")).unwrap();
        assert_eq!(s.points, 100);
    }

    #[test]
    fn baender_linie_und_klein() {
        let mit = |d: f32| Abfangen {
            dauer_ab_50ft_s: Some(d),
            max_vs_fpm: Some(-50.0),
            ..Default::default()
        };
        for (d, icao, erwartet) in [
            (10.0, "A320", 100u8),
            (10.1, "A320", 80),
            (12.0, "B738", 80),
            (12.1, "B738", 50),
            (15.0, "A388", 50),
            (15.1, "A388", 25),
            (16.0, "C172", 100),
            (16.1, "C172", 80),
            (20.1, "E55P", 50),
            (24.1, "PC12", 25),
        ] {
            assert_eq!(
                sub_abfangen(Some(&mit(d)), Some(icao)).unwrap().points,
                erwartet,
                "{d} s {icao}"
            );
        }
    }

    #[test]
    fn ballooning_hoechstens_50_aber_nie_besser() {
        let a = Abfangen {
            dauer_ab_50ft_s: Some(8.0),
            max_vs_fpm: Some(51.0),
            ..Default::default()
        };
        let s = sub_abfangen(Some(&a), Some("A320")).unwrap();
        assert_eq!(
            (s.points, s.rationale_key.as_deref()),
            (50, Some("landing.rat.ballooning"))
        );
        // Gegenprobe: 50 fpm genau ist noch kein Ballooning.
        let a = Abfangen {
            max_vs_fpm: Some(50.0),
            ..a
        };
        assert_eq!(sub_abfangen(Some(&a), Some("A320")).unwrap().points, 100);
        // Schon schlechter als 50: bleibt bei 25.
        let a = Abfangen {
            dauer_ab_50ft_s: Some(20.0),
            max_vs_fpm: Some(200.0),
            ..Default::default()
        };
        assert_eq!(sub_abfangen(Some(&a), Some("A320")).unwrap().points, 25);
    }

    #[test]
    fn ohne_messung_keine_achse() {
        assert!(sub_abfangen(None, Some("A320")).is_none());
        let a = messen(
            &[p(-3000, 20.0, -300.0), p(-1000, 5.0, -200.0)],
            Some(-150.0),
        );
        assert_eq!(a.grund_ohne_werte.as_deref(), Some("kein_50ft_durchgang"));
        assert!(sub_abfangen(Some(&a), Some("A320")).is_none());
    }

    #[test]
    fn letzter_durchgang_zaehlt_nach_durchstarten() {
        // Erst runter durch 50, wieder hoch, dann der echte Anflug.
        let mut v = vec![
            p(-60000, 80.0, -600.0),
            p(-58000, 40.0, -600.0),
            p(-50000, 300.0, 1500.0),
        ];
        v.extend(thy39());
        assert_eq!(messen(&v, Some(-46.0)).dauer_ab_50ft_s, Some(11.3));
    }
}
