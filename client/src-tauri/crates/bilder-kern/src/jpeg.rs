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

#[cfg(test)]
mod tests {
    use super::*;

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
