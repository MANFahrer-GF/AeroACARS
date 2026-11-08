//! Innenbereich des Simulatorfensters: Rechenlogik der Aufnahme ohne Windows-Aufrufe.
//!
//! Die Aufnahme soll nur den Innenbereich (Client-Bereich) zeigen, ohne Titelleiste,
//! Rahmen und den unsichtbaren Rand, den Windows um ein Fenster legt. Die
//! Windows-Teile liefern die Rechtecke in Bildschirmkoordinaten (Fensterrahmen
//! und Innenbereich); ob und wie geschnitten wird, entscheidet diese reine
//! Funktion. Ein Schnitt, der nicht plausibel aussieht (anderes Mass, Rechtecke
//! passen nicht ineinander, unglaubliche Raender), wird nie ausgefuehrt: das Bild
//! bleibt dann ungeschnitten und der Grund steht im Protokoll.

use std::fmt;

use crate::verdeckung::Rechteck;

/// So viele Pixel darf das Bildmass vom Rahmen abweichen (Rundung in WGC/DPI).
const MASS_TOLERANZ: i32 = 2;
/// Hoechster Rand links, rechts und unten: Rahmen plus unsichtbarer Rand, auch bei 300 % Skalierung.
const MAX_SEITE: i32 = 48;
/// Hoechster Rand oben (Titelleiste, bei 300 % rund 100 px).
const MAX_OBEN: i32 = 160;
/// Links und rechts sind bei einem Fenster gleich breit; mehr Unterschied gilt als unplausibel.
const MAX_SEITEN_UNTERSCHIED: i32 = 8;
/// Kleinster Innenbereich: darunter ist es kein Simulatorbild mehr, sondern ein falsches Rechteck.
/// Bewusst unter der Aussen-Mindestgroesse beim Aufspueren (200 px), damit kleine Fenster geschnitten werden.
const MIN_INNEN: i32 = 64;

/// Raender in Pixeln, die weggeschnitten werden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rand {
    pub links: u32,
    pub oben: u32,
    pub rechts: u32,
    pub unten: u32,
}

/// Ausgang der Pruefung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schnitt {
    /// Kein Rand: Vollbild oder randlos, es faellt kein Pixel weg.
    Ohne,
    /// Plausibler Schnitt; der Innenbereich liegt bei `rand` im Bild.
    Innen(Rand),
    /// Nicht plausibel: das Bild bleibt ungeschnitten. Der Grund ist fuers Protokoll.
    Unplausibel(String),
}

impl fmt::Display for Schnitt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Schnitt::Ohne => write!(f, "kein Schnitt (Vollbild oder randlos)"),
            Schnitt::Innen(r) => write!(f, "Raender links/oben/rechts/unten = {}/{}/{}/{}", r.links, r.oben, r.rechts, r.unten),
            Schnitt::Unplausibel(g) => write!(f, "unplausibel, ungeschnitten ({g})"),
        }
    }
}

fn breite(r: &Rechteck) -> i32 {
    r.rechts - r.links
}

fn hoehe(r: &Rechteck) -> i32 {
    r.unten - r.oben
}

/// Bestimmt den Schnitt. `bild` ist das Mass der Aufnahme, `rahmen_kandidaten` die
/// Rechtecke, die das Bild darstellen koennte (in der Reihenfolge der Vermutung;
/// bei WGC der sichtbare Rahmen, dann das Fensterrechteck), `innen` der
/// Innenbereich in Bildschirmkoordinaten. Der erste Kandidat mit genau dem Bildmass
/// gilt; sonst der eine Kandidat innerhalb der Toleranz. Passt keiner oder sind es
/// mehrere verschiedene, ist der Schnitt unplausibel.
pub fn innenbereich(bild: (u32, u32), rahmen_kandidaten: &[Rechteck], innen: &Rechteck) -> Schnitt {
    let (bb, bh) = (bild.0 as i64, bild.1 as i64);
    if bb <= 0 || bh <= 0 || bb > i32::MAX as i64 || bh > i32::MAX as i64 {
        return Schnitt::Unplausibel("Bild ohne Mass".into());
    }
    let (bb, bh) = (bb as i32, bh as i32);
    let passt = |r: &Rechteck, toleranz: i32| !r.leer() && (breite(r) - bb).abs() <= toleranz && (hoehe(r) - bh).abs() <= toleranz;
    let rahmen = match rahmen_kandidaten.iter().find(|r| passt(r, 0)) {
        Some(r) => r,
        None => {
            let mut toleriert = rahmen_kandidaten.iter().filter(|r| passt(r, MASS_TOLERANZ));
            let Some(erster) = toleriert.next() else {
                return Schnitt::Unplausibel(format!("Bild {bb}x{bh} passt zu keinem Fensterrahmen"));
            };
            if toleriert.any(|r| r != erster) {
                return Schnitt::Unplausibel(format!("Bild {bb}x{bh} passt zu mehreren Fensterrahmen"));
            }
            erster
        }
    };
    if innen.leer() {
        return Schnitt::Unplausibel("Innenbereich leer".into());
    }
    if innen.links < rahmen.links || innen.oben < rahmen.oben || innen.rechts > rahmen.rechts || innen.unten > rahmen.unten {
        return Schnitt::Unplausibel("Innenbereich liegt nicht im Fensterrahmen".into());
    }
    let (l, o, r, u) = (innen.links - rahmen.links, innen.oben - rahmen.oben, rahmen.rechts - innen.rechts, rahmen.unten - innen.unten);
    if l == 0 && o == 0 && r == 0 && u == 0 {
        return Schnitt::Ohne;
    }
    if l > MAX_SEITE || r > MAX_SEITE || u > MAX_SEITE || o > MAX_OBEN {
        return Schnitt::Unplausibel(format!("Raender {l}/{o}/{r}/{u} zu gross"));
    }
    if (l - r).abs() > MAX_SEITEN_UNTERSCHIED {
        return Schnitt::Unplausibel(format!("Seitenraender {l} und {r} sehr ungleich"));
    }
    // Das Bild kann um die Toleranz von seinem Rahmen abweichen: der Schnitt muss hineinpassen.
    let (iw, ih) = (breite(innen), hoehe(innen));
    if iw < MIN_INNEN || ih < MIN_INNEN || l + iw > bb || o + ih > bh {
        return Schnitt::Unplausibel(format!("Innenbereich {iw}x{ih} passt nicht ins Bild {bb}x{bh}"));
    }
    // Rechts und unten aus dem Bild: das Ergebnis hat so immer genau das Mass des Innenbereichs.
    Schnitt::Innen(Rand { links: l as u32, oben: o as u32, rechts: (bb - l - iw) as u32, unten: (bh - o - ih) as u32 })
}

/// Mass des Bildes nach dem Schnitt.
pub fn masse_nach_schnitt(bild: (u32, u32), schnitt: &Schnitt) -> (u32, u32) {
    match schnitt {
        Schnitt::Innen(r) => (bild.0.saturating_sub(r.links + r.rechts), bild.1.saturating_sub(r.oben + r.unten)),
        _ => bild,
    }
}

/// Schneidet ein BGRA-Bild (`bild` = Breite, Hoehe) auf den Innenbereich. Bei
/// `Ohne`, `Unplausibel` oder einem Mass, das nicht zu den Daten passt, kommt das Bild unveraendert zurueck.
pub fn zuschneiden(bgra: Vec<u8>, bild: (u32, u32), schnitt: &Schnitt) -> (Vec<u8>, (u32, u32)) {
    let Schnitt::Innen(r) = schnitt else { return (bgra, bild) };
    let (b, h) = (bild.0 as usize, bild.1 as usize);
    let (nb, nh) = masse_nach_schnitt(bild, schnitt);
    let (nb, nh) = (nb as usize, nh as usize);
    if bgra.len() != b * h * 4 || nb == 0 || nh == 0 || r.links as usize + nb > b || r.oben as usize + nh > h {
        return (bgra, bild);
    }
    let mut aus = Vec::with_capacity(nb * nh * 4);
    for y in 0..nh {
        let von = ((r.oben as usize + y) * b + r.links as usize) * 4;
        aus.extend_from_slice(&bgra[von..von + nb * 4]);
    }
    (aus, (nb as u32, nh as u32))
}

/// Was der GDI-Rueckfall aufnimmt: Quelle in Bildschirmkoordinaten (`BitBlt`), Bildmass
/// und ob `PrintWindow` nur den Innenbereich liefern soll (`PW_CLIENTONLY`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GdiQuelle {
    pub links: i32,
    pub oben: i32,
    pub breite: u32,
    pub hoehe: u32,
    pub nur_innen: bool,
}

/// Quelle des GDI-Rueckfalls. `rahmen` ist das Fensterrechteck mit dem Mass `voll`.
pub fn gdi_quelle(rahmen: &Rechteck, voll: (u32, u32), schnitt: &Schnitt) -> GdiQuelle {
    let (rand_links, rand_oben, nur_innen) = match schnitt {
        Schnitt::Innen(r) => (r.links as i32, r.oben as i32, true),
        _ => (0, 0, false),
    };
    let (breite, hoehe) = masse_nach_schnitt(voll, schnitt);
    GdiQuelle { links: rahmen.links + rand_links, oben: rahmen.oben + rand_oben, breite, hoehe, nur_innen }
}

/// Fensterrechteck und Innenbereich zu einem Zeitpunkt.
pub type Lage = (Option<Rechteck>, Option<Rechteck>);

/// Gilt das Bild noch, nachdem sich das Fenster seit dem Ausmessen bewegt haben kann?
/// Das Mass muss immer gleich sein. Die Position zaehlt nur bei der Bildschirmkopie:
/// `PrintWindow` zeichnet das Fenster selbst, egal wo es gerade liegt.
pub fn lage_passt(davor: &Lage, danach: &Lage, bildschirmkopie: bool) -> bool {
    if bildschirmkopie {
        return davor == danach;
    }
    let mass = |r: &Option<Rechteck>| r.as_ref().map(|r| (breite(r), hoehe(r)));
    mass(&davor.0) == mass(&danach.0) && mass(&davor.1) == mass(&danach.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rc(l: i32, o: i32, r: i32, u: i32) -> Rechteck {
        Rechteck::neu(l, o, r, u)
    }

    #[test]
    fn grenzwerte_gelten_genau_bis_zur_schwelle() {
        let f = rc(0, 0, 1000, 800);
        let schnitt = |l: i32, o: i32, r: i32, u: i32| innenbereich((1000, 800), &[f], &rc(l, o, 1000 - r, 800 - u));
        let unplausibel = |s: Schnitt| matches!(s, Schnitt::Unplausibel(_));
        assert_eq!(schnitt(48, 31, 48, 48), Schnitt::Innen(Rand { links: 48, oben: 31, rechts: 48, unten: 48 }));
        // Jede Seite einzeln einen Pixel drueber; der Unterschied links/rechts bleibt dabei erlaubt.
        assert!(unplausibel(schnitt(49, 31, 41, 8)));
        assert!(unplausibel(schnitt(41, 31, 49, 8)));
        assert!(unplausibel(schnitt(8, 31, 8, 49)));
        assert_eq!(schnitt(8, 160, 8, 8), Schnitt::Innen(Rand { links: 8, oben: 160, rechts: 8, unten: 8 }));
        assert!(unplausibel(schnitt(8, 161, 8, 8)));
        assert!(matches!(schnitt(10, 31, 2, 2), Schnitt::Innen(_)), "8 px Unterschied sind erlaubt");
        assert!(unplausibel(schnitt(11, 31, 2, 2)));
    }

    #[test]
    fn negative_koordinaten_auf_dem_zweiten_monitor() {
        // Maximiert links vom Hauptmonitor: das Fensterrechteck steht 8 px ueber den Bildschirm hinaus.
        let f = rc(-1928, -8, 8, 1088);
        let innen = rc(-1920, 23, 0, 1080);
        let s = innenbereich((1936, 1096), &[f], &innen);
        assert_eq!(s, Schnitt::Innen(Rand { links: 8, oben: 31, rechts: 8, unten: 8 }));
        assert_eq!(masse_nach_schnitt((1936, 1096), &s), (1920, 1057));
        let q = gdi_quelle(&f, (1936, 1096), &s);
        assert_eq!((q.links, q.oben), (innen.links, innen.oben));
    }

    #[test]
    fn gdi_nimmt_den_innenbereich_oder_das_ganze_fenster() {
        let (f, i) = fenster(1280, 720, 31, 1);
        let s = innenbereich((1280, 720), &[f], &i);
        assert_eq!(gdi_quelle(&f, (1280, 720), &s), GdiQuelle { links: i.links, oben: i.oben, breite: 1278, hoehe: 688, nur_innen: true });
        let ganz = GdiQuelle { links: f.links, oben: f.oben, breite: 1280, hoehe: 720, nur_innen: false };
        assert_eq!(gdi_quelle(&f, (1280, 720), &Schnitt::Unplausibel("x".into())), ganz);
        assert_eq!(gdi_quelle(&f, (1280, 720), &Schnitt::Ohne), ganz);
    }

    #[test]
    fn verschieben_stoert_nur_die_bildschirmkopie() {
        let davor: Lage = (Some(rc(100, 50, 1380, 770)), Some(rc(101, 81, 1379, 769)));
        let verschoben: Lage = (Some(rc(140, 50, 1420, 770)), Some(rc(141, 81, 1419, 769)));
        assert!(lage_passt(&davor, &verschoben, false), "PrintWindow ist die Position egal");
        assert!(!lage_passt(&davor, &verschoben, true), "BitBlt haette die falsche Stelle kopiert");
        assert!(lage_passt(&davor, &davor, true));
        let groesser: Lage = (Some(rc(100, 50, 1400, 770)), Some(rc(101, 81, 1399, 769)));
        assert!(!lage_passt(&davor, &groesser, false));
        assert!(!lage_passt(&davor, &groesser, true));
        // Nur der Innenbereich aendert sich, etwa wenn die Menueleiste wegfaellt.
        let innen_anders: Lage = (davor.0, Some(rc(101, 51, 1379, 769)));
        assert!(!lage_passt(&davor, &innen_anders, false));
        assert!(!lage_passt(&davor, &(davor.0, None), false));
    }

    /// Fenster mit sichtbarem Rahmen `fenster` und Innenbereich: Titelleiste `titel`, Seiten/unten `rand` (Pixel).
    fn fenster(b: i32, h: i32, titel: i32, rand: i32) -> (Rechteck, Rechteck) {
        (rc(100, 50, 100 + b, 50 + h), rc(100 + rand, 50 + titel, 100 + b - rand, 50 + h - rand))
    }

    #[test]
    fn vollbild_und_randlos_schneiden_null_pixel() {
        let bildschirm = rc(0, 0, 2560, 1440);
        assert_eq!(innenbereich((2560, 1440), &[bildschirm], &bildschirm), Schnitt::Ohne);
        // Randlos im Fenster: Innenbereich = Rahmen.
        let (f, _) = fenster(1920, 1080, 0, 0);
        assert_eq!(innenbereich((1920, 1080), &[f], &f), Schnitt::Ohne);
    }

    #[test]
    fn fenster_bei_100_125_150_prozent() {
        // Titelleiste 31 px bei 100 %, Rahmen 1 px sichtbar; skaliert.
        for (titel, rand) in [(31, 1), (39, 1), (47, 2)] {
            let (f, i) = fenster(1280, 720, titel, rand);
            let s = innenbereich((1280, 720), &[f], &i);
            let erwartet = Rand { links: rand as u32, oben: titel as u32, rechts: rand as u32, unten: rand as u32 };
            assert_eq!(s, Schnitt::Innen(erwartet), "titel {titel} rand {rand}");
            assert_eq!(masse_nach_schnitt((1280, 720), &s), ((1280 - 2 * rand) as u32, (720 - titel - rand) as u32));
        }
    }

    #[test]
    fn unsichtbarer_rand_wird_ueber_den_zweiten_kandidaten_gefunden() {
        // GetWindowRect enthaelt 7 px unsichtbaren Rand je Seite/unten, der sichtbare Rahmen nicht.
        let fenster_rect = rc(93, 50, 1393, 757); // 1300 x 707
        let sichtbar = rc(100, 50, 1386, 750); // 1286 x 700
        let innen = rc(101, 81, 1385, 749);
        // WGC liefert das Mass des sichtbaren Rahmens: der erste Kandidat passt.
        let s = innenbereich((1286, 700), &[sichtbar, fenster_rect], &innen);
        assert_eq!(s, Schnitt::Innen(Rand { links: 1, oben: 31, rechts: 1, unten: 1 }));
        // Wuerde das Bild das Fensterrechteck darstellen, gelten dessen Raender.
        let s = innenbereich((1300, 707), &[sichtbar, fenster_rect], &innen);
        assert_eq!(s, Schnitt::Innen(Rand { links: 8, oben: 31, rechts: 8, unten: 8 }));
    }

    #[test]
    fn unplausibel_laesst_das_bild_ungeschnitten() {
        let (f, i) = fenster(1280, 720, 31, 1);
        let unplausibel = |s: Schnitt| matches!(s, Schnitt::Unplausibel(_));
        // Bild hat ein anderes Mass als jeder Rahmen (z. B. DPI-Verschiebung).
        assert!(unplausibel(innenbereich((1600, 900), &[f], &i)));
        // Bild ohne Mass, Rahmen ohne Flaeche.
        assert!(unplausibel(innenbereich((0, 720), &[f], &i)));
        assert!(unplausibel(innenbereich((1280, 720), &[], &i)));
        // Innenbereich leer, ausserhalb, ragt hinaus.
        assert!(unplausibel(innenbereich((1280, 720), &[f], &rc(500, 500, 500, 900))));
        assert!(unplausibel(innenbereich((1280, 720), &[f], &rc(3000, 3000, 4000, 4000))));
        assert!(unplausibel(innenbereich((1280, 720), &[f], &rc(f.links - 5, i.oben, i.rechts, i.unten))));
        // Raender zu gross (Titelleiste 300 px, Seiten 100 px).
        let (f2, i2) = fenster(1280, 720, 300, 1);
        assert!(unplausibel(innenbereich((1280, 720), &[f2], &i2)));
        let (f3, i3) = fenster(1280, 720, 31, 100);
        assert!(unplausibel(innenbereich((1280, 720), &[f3], &i3)));
        // Links und rechts sehr verschieden.
        assert!(unplausibel(innenbereich((1280, 720), &[f], &rc(f.links + 1, i.oben, f.rechts - 30, i.unten))));
        // Innenbereich winzig (59 px hoch).
        assert!(unplausibel(innenbereich((300, 220), &[rc(0, 0, 300, 220)], &rc(1, 160, 299, 219))));
        // Unplausibel schneidet nicht.
        let s = innenbereich((1600, 900), &[f], &i);
        let (bgra, mass) = zuschneiden(vec![7; 1600 * 900 * 4], (1600, 900), &s);
        assert_eq!((mass, bgra.len()), ((1600, 900), 1600 * 900 * 4));
    }

    #[test]
    fn mass_toleranz_gilt_aber_der_schnitt_muss_ins_bild_passen() {
        let (f, i) = fenster(1280, 720, 31, 1);
        // Bild 2 px kleiner als der Rahmen: noch Kandidat.
        assert!(matches!(innenbereich((1278, 718), &[f], &i), Schnitt::Unplausibel(_)), "Innenbereich ragt aus dem kleineren Bild");
        assert!(matches!(innenbereich((1279, 720), &[f], &i), Schnitt::Innen(_)));
        // Drei Pixel Abweichung ist zu viel.
        assert!(matches!(innenbereich((1277, 720), &[f], &i), Schnitt::Unplausibel(_)));
    }

    #[test]
    fn exakter_treffer_schlaegt_frueheren_tolerierten_kandidaten() {
        let sichtbar = rc(100, 100, 1100, 800); // 1000 x 700
        let fenster_rect = rc(99, 99, 1101, 801); // 1002 x 702
        let innen = rc(101, 131, 1099, 799);
        let s = innenbereich((1002, 702), &[sichtbar, fenster_rect], &innen);
        assert_eq!(s, Schnitt::Innen(Rand { links: 2, oben: 32, rechts: 2, unten: 2 }));
        let s = innenbereich((1000, 700), &[sichtbar, fenster_rect], &innen);
        assert_eq!(s, Schnitt::Innen(Rand { links: 1, oben: 31, rechts: 1, unten: 1 }));
    }

    #[test]
    fn mehrere_tolerierte_rahmen_ohne_exakten_treffer_sind_unplausibel() {
        let sichtbar = rc(100, 100, 1100, 800);
        let fenster_rect = rc(99, 99, 1101, 801);
        let innen = rc(101, 131, 1099, 799);
        assert!(matches!(innenbereich((1001, 701), &[sichtbar, fenster_rect], &innen), Schnitt::Unplausibel(_)));
        // Dasselbe Rechteck zweimal ist nicht mehrdeutig.
        assert!(matches!(innenbereich((1001, 701), &[sichtbar, sichtbar], &innen), Schnitt::Innen(_)));
    }

    #[test]
    fn tolerierte_abweichung_ergibt_genau_das_innenmass() {
        let (f, i) = fenster(1280, 720, 31, 1); // innen 1278 x 688
        for bild in [(1281, 720), (1279, 720), (1280, 721), (1279, 719), (1282, 722)] {
            let s = innenbereich(bild, &[f], &i);
            assert!(matches!(s, Schnitt::Innen(_)), "Bild {bild:?}: {s}");
            assert_eq!(masse_nach_schnitt(bild, &s), (1278, 688), "Bild {bild:?}");
        }
    }

    #[test]
    fn kleines_fenster_wird_trotzdem_geschnitten() {
        // Die Fenstersuche nimmt Fenster ab 200 x 200 aussen an.
        let s = innenbereich((400, 220), &[rc(0, 0, 400, 220)], &rc(1, 31, 399, 219));
        assert_eq!(s, Schnitt::Innen(Rand { links: 1, oben: 31, rechts: 1, unten: 1 }));
        // Genau an der Untergrenze noch, einen Pixel darunter nicht mehr.
        assert!(matches!(innenbereich((400, 220), &[rc(0, 0, 400, 220)], &rc(1, 155, 399, 219)), Schnitt::Innen(_)));
        assert!(matches!(innenbereich((400, 220), &[rc(0, 0, 400, 220)], &rc(1, 156, 399, 219)), Schnitt::Unplausibel(_)));
    }

    #[test]
    fn zuschneiden_nimmt_genau_den_innenbereich() {
        // 6 x 4 Bild, Pixel tragen ihre Spalte+10*Zeile; Rand l=1, o=2, r=2, u=1 -> 3 x 1.
        let (b, h) = (6usize, 4usize);
        let mut bgra = Vec::new();
        for y in 0..h {
            for x in 0..b {
                bgra.extend_from_slice(&[(x + 10 * y) as u8, 0, 0, 255]);
            }
        }
        let s = Schnitt::Innen(Rand { links: 1, oben: 2, rechts: 2, unten: 1 });
        let (aus, mass) = zuschneiden(bgra.clone(), (6, 4), &s);
        assert_eq!(mass, (3, 1));
        assert_eq!(aus.chunks(4).map(|p| p[0]).collect::<Vec<_>>(), vec![21, 22, 23]);
        // Ohne und falsche Datenlaenge: unveraendert.
        assert_eq!(zuschneiden(bgra.clone(), (6, 4), &Schnitt::Ohne).1, (6, 4));
        assert_eq!(zuschneiden(bgra[..8].to_vec(), (6, 4), &s).1, (6, 4));
    }

    #[test]
    fn protokolltext_nennt_raender_oder_grund() {
        assert_eq!(Schnitt::Innen(Rand { links: 1, oben: 31, rechts: 1, unten: 1 }).to_string(), "Raender links/oben/rechts/unten = 1/31/1/1");
        assert!(Schnitt::Unplausibel("x".into()).to_string().contains("unplausibel, ungeschnitten"));
        assert!(Schnitt::Ohne.to_string().contains("kein Schnitt"));
    }
}
