//! Begrenzter JPEG-Segment-Parser ohne Bildbibliothek: entfernt alle
//! APP1-Segmente (EXIF mit GPS, XMP) vor dem Senden. Der Rest der Datei bleibt
//! Byte fuer Byte erhalten, das Ergebnis ist deterministisch: gleiche Datei,
//! gleiche Bytes (kein `konflikt` bei Wiederholung des Uploads).

use crate::Fehler;

/// Entfernt alle APP1-Segmente (`FF E1`) im Kopfteil bis zum Beginn der Bilddaten (SOS).
pub fn app1_entfernen(jpeg: &[u8]) -> Result<Vec<u8>, Fehler> {
    let fehl = |was: &str| Fehler::Format(format!("JPEG unbrauchbar: {was}"));
    if jpeg.len() < 4 || jpeg[..3] != [0xFF, 0xD8, 0xFF] {
        return Err(fehl("kein JPEG"));
    }
    let mut aus = Vec::with_capacity(jpeg.len());
    aus.extend_from_slice(&jpeg[..2]); // SOI
    let mut i = 2usize;
    loop {
        if i + 2 > jpeg.len() {
            return Err(fehl("Ende vor den Bilddaten"));
        }
        if jpeg[i] != 0xFF {
            return Err(fehl("Segmentgrenze erwartet"));
        }
        let m = jpeg[i + 1];
        if m == 0xFF {
            // Fuellbyte vor dem Marker: weglassen
            i += 1;
            continue;
        }
        if m == 0xD9 {
            aus.extend_from_slice(&jpeg[i..]);
            return Ok(aus);
        }
        if m == 0x01 || (0xD0..=0xD8).contains(&m) {
            aus.extend_from_slice(&jpeg[i..i + 2]); // Marker ohne Laenge
            i += 2;
            continue;
        }
        if i + 4 > jpeg.len() {
            return Err(fehl("Segment ohne Laenge"));
        }
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        if len < 2 || i + 2 + len > jpeg.len() {
            return Err(fehl("Segmentlaenge ausserhalb der Datei"));
        }
        let ende = i + 2 + len;
        if m != 0xE1 {
            aus.extend_from_slice(&jpeg[i..ende]);
        }
        i = ende;
        if m == 0xDA {
            // Bilddaten bis EOI unveraendert uebernehmen
            aus.extend_from_slice(&jpeg[i..]);
            return Ok(aus);
        }
    }
}

/// Position fuer das EXIF-GPS im JPEG (gleicher Aufbau wie `exifGps.ts` im Paket).
#[derive(Debug, Clone, PartialEq)]
pub struct GpsPunkt {
    pub breite: f64,
    pub laenge: f64,
    pub hoehe_ft: Option<f64>,
    pub kurs: Option<f64>,
    pub zeit_ms: Option<i64>,
}

fn dms(grad: f64) -> [(u32, u32); 3] {
    let a = grad.abs();
    let mut d = a.floor();
    let mut m = ((a - d) * 60.0).floor();
    let mut s = (((a - d) * 60.0 - m) * 60.0 * 10000.0).round();
    if s >= 600_000.0 {
        s -= 600_000.0;
        m += 1.0;
    }
    if m >= 60.0 {
        m -= 60.0;
        d += 1.0;
    }
    [(d as u32, 1), (m as u32, 1), (s as u32, 10000)]
}

/// Das APP1-Segment (FF E1, Laenge, "Exif\0\0", TIFF mit GPS-IFD), Byte fuer Byte wie im Paket.
pub fn gps_segment(p: &GpsPunkt) -> Vec<u8> {
    let rat = |paare: &[(u32, u32)]| paare.iter().flat_map(|(z, n)| [z.to_be_bytes(), n.to_be_bytes()].concat()).collect::<Vec<u8>>();
    let ascii = |s: &str| s.bytes().map(|b| b & 127).chain([0]).collect::<Vec<u8>>();
    // (Tag, Typ, Anzahl, Daten): Typ 1 BYTE, 2 ASCII, 5 RATIONAL
    let mut e: Vec<(u16, u16, u32, Vec<u8>)> = vec![
        (1, 2, 2, ascii(if p.breite >= 0.0 { "N" } else { "S" })),
        (2, 5, 3, rat(&dms(p.breite))),
        (3, 2, 2, ascii(if p.laenge >= 0.0 { "E" } else { "W" })),
        (4, 5, 3, rat(&dms(p.laenge))),
    ];
    if let Some(ft) = p.hoehe_ft {
        let m = ft * 0.3048;
        e.push((5, 1, 1, vec![u8::from(m < 0.0)]));
        e.push((6, 5, 1, rat(&[((m.abs() * 100.0).round() as u32, 100)])));
    }
    let t = p.zeit_ms.and_then(chrono::DateTime::from_timestamp_millis);
    if let Some(t) = t {
        use chrono::Timelike;
        e.push((7, 5, 3, rat(&[(t.hour(), 1), (t.minute(), 1), (t.second(), 1)])));
    }
    if let Some(k) = p.kurs {
        e.push((0x10, 2, 2, ascii("T")));
        e.push((0x11, 5, 1, rat(&[((((k % 360.0) + 360.0) % 360.0 * 100.0).round() as u32, 100)])));
    }
    if let Some(t) = t {
        e.push((0x1d, 2, 11, ascii(&t.format("%Y:%m:%d").to_string())));
    }
    let gps_offset: u32 = 8 + 2 + 12 + 4;
    let tabelle = 2 + e.len() as u32 * 12 + 4;
    let (mut zeilen, mut daten) = (Vec::new(), Vec::<u8>::new());
    for (tag, typ, anzahl, d) in &e {
        zeilen.extend(tag.to_be_bytes());
        zeilen.extend(typ.to_be_bytes());
        zeilen.extend(anzahl.to_be_bytes());
        if d.len() <= 4 {
            zeilen.extend(d);
            zeilen.extend(std::iter::repeat_n(0u8, 4 - d.len()));
        } else {
            zeilen.extend((gps_offset + tabelle + daten.len() as u32).to_be_bytes());
            daten.extend(d);
            if daten.len() % 2 == 1 {
                daten.push(0);
            }
        }
    }
    let mut tiff = vec![0x4D, 0x4D, 0x00, 0x2A, 0, 0, 0, 8];
    tiff.extend([0, 1, 0x88, 0x25, 0, 4, 0, 0, 0, 1]);
    tiff.extend(gps_offset.to_be_bytes());
    tiff.extend([0, 0, 0, 0]);
    tiff.extend((e.len() as u16).to_be_bytes());
    tiff.extend(zeilen);
    tiff.extend([0, 0, 0, 0]);
    tiff.extend(daten);
    let mut aus = vec![0xFF, 0xE1];
    aus.extend(((tiff.len() + 6 + 2) as u16).to_be_bytes());
    aus.extend(b"Exif\0\0");
    aus.extend(tiff);
    aus
}

/// Ersetzt jedes APP1 durch die neue Position direkt hinter SOI; ohne Position bleibt keine stehen.
pub fn position_setzen(jpeg: &[u8], p: Option<&GpsPunkt>) -> Result<Vec<u8>, Fehler> {
    let ohne = app1_entfernen(jpeg)?;
    let Some(p) = p else { return Ok(ohne) };
    let mut aus = ohne[..2].to_vec();
    aus.extend(gps_segment(p));
    aus.extend_from_slice(&ohne[2..]);
    Ok(aus)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn gps_segment_gleicht_dem_paket_byte_fuer_byte() {
        // Erwartung erzeugt mit `exifGpsSegment` aus client/src/components/bilder/logik/exifGps.ts
        let mit = GpsPunkt { breite: 47.2617, laenge: -11.3414, hoehe_ft: Some(1909.2), kurs: Some(260.9), zeit_ms: Some(1_794_079_811_963) };
        assert_eq!(hex(&gps_segment(&mit)), "ffe101044578696600004d4d002a00000008000188250004000000010000001a00000000000a00010002000000024e0000000002000500000003000000980003000200000002570000000004000500000003000000b00005000100000001000000000006000500000001000000c80007000500000003000000d00010000200000002540000000011000500000001000000e8001d00020000000b000000f0000000000000002f000000010000000f0000000100066d50000027100000000b00000001000000140000000100046e60000027100000e3500000006400000013000000010000001e000000010000000b00000001000065ea00000064323032363a31313a30370000");
        let ohne = GpsPunkt { breite: -33.9, laenge: 151.2, hoehe_ft: None, kurs: None, zeit_ms: None };
        assert_eq!(hex(&gps_segment(&ohne)), "ffe100884578696600004d4d002a00000008000188250004000000010000001a0000000000040001000200000002530000000002000500000003000000500003000200000002450000000004000500000003000000680000000000000021000000010000003600000001000000000000271000000097000000010000000c000000010000000000002710");
    }

    #[test]
    fn position_setzen_ersetzt_das_alte_app1() {
        let alt = jpeg(true, true);
        let p = GpsPunkt { breite: 1.0, laenge: 2.0, hoehe_ft: None, kurs: None, zeit_ms: None };
        let neu = position_setzen(&alt, Some(&p)).unwrap();
        assert_eq!(&neu[2..2 + gps_segment(&p).len()], gps_segment(&p).as_slice(), "neues Segment direkt hinter SOI");
        assert_eq!(app1_entfernen(&neu).unwrap(), app1_entfernen(&alt).unwrap(), "sonst Byte fuer Byte gleich");
        assert_eq!(position_setzen(&alt, None).unwrap(), app1_entfernen(&alt).unwrap());
    }

    fn segment(m: u8, inhalt: &[u8]) -> Vec<u8> {
        let mut v = vec![0xFF, m];
        v.extend_from_slice(&((inhalt.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(inhalt);
        v
    }

    /// SOI, JFIF (APP0), [Exif], DQT, SOS mit Daten (enthaelt FF 00 und FF E1-aehnliche Bytes), EOI.
    fn jpeg(mit_exif: bool, mit_xmp: bool) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        v.extend(segment(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0"));
        if mit_exif {
            v.extend(segment(0xE1, b"Exif\0\0MM\0*GPS-Position 50.1 8.6"));
        }
        if mit_xmp {
            v.extend(segment(0xE1, b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta/>"));
        }
        v.extend(segment(0xDB, &[0, 1, 2, 3]));
        v.extend(segment(0xDA, &[1, 1, 0, 0, 0x3F, 0]));
        v.extend_from_slice(&[0x12, 0xFF, 0x00, 0xFF, 0xE1, 0x34, 0xFF, 0xD0, 0x56]); // Bilddaten mit Stuffing und RST
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }

    #[test]
    fn app1_exif_und_xmp_fliegen_raus_der_rest_bleibt_byte_gleich() {
        let sauber = jpeg(false, false);
        for (e, x) in [(true, false), (false, true), (true, true)] {
            let r = app1_entfernen(&jpeg(e, x)).unwrap();
            assert_eq!(r, sauber, "exif={e} xmp={x}");
            assert!(!String::from_utf8_lossy(&r).contains("GPS-Position"));
        }
        assert_eq!(app1_entfernen(&sauber).unwrap(), sauber, "ohne APP1 unveraendert");
    }

    #[test]
    fn deterministisch_und_die_bilddaten_werden_nie_angefasst() {
        let a = app1_entfernen(&jpeg(true, true)).unwrap();
        let b = app1_entfernen(&jpeg(true, true)).unwrap();
        assert_eq!(a, b);
        assert!(a.ends_with(&[0x12, 0xFF, 0x00, 0xFF, 0xE1, 0x34, 0xFF, 0xD0, 0x56, 0xFF, 0xD9]), "ein FF E1 in den Bilddaten bleibt");
    }

    #[test]
    fn kaputtes_wird_abgelehnt_statt_geraten() {
        assert!(matches!(app1_entfernen(b"PNG!"), Err(Fehler::Format(_))));
        let mut k = jpeg(true, false);
        k.truncate(12); // mitten im APP0
        assert!(matches!(app1_entfernen(&k), Err(Fehler::Format(_))));
        let mut l = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF, 1, 2]; // Laenge ragt hinaus
        assert!(matches!(app1_entfernen(&l), Err(Fehler::Format(_))));
        l = vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x01, 0, 0]; // Laenge < 2
        assert!(matches!(app1_entfernen(&l), Err(Fehler::Format(_))));
        assert!(matches!(app1_entfernen(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0, 0]), Err(Fehler::Format(_))), "ohne SOS/EOI");
    }
}
