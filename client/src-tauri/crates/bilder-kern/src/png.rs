//! PNG aus der Fensteraufnahme (BGRA wie es GDI liefert). Kein JPEG und kein
//! EXIF hier: Das Paket macht aus der Eingangskopie sein JPEG (P-11.33).

use crate::Fehler;

/// `bgra` hat `breite * hoehe * 4` Bytes; die Alpha-Werte werden auf deckend gesetzt
/// (`PrintWindow` laesst sie oft auf 0).
pub fn aus_bgra(breite: u32, hoehe: u32, bgra: &[u8]) -> Result<Vec<u8>, Fehler> {
    let soll = (breite as usize).checked_mul(hoehe as usize).and_then(|n| n.checked_mul(4));
    if breite == 0 || hoehe == 0 || soll != Some(bgra.len()) {
        return Err(Fehler::Eingabe("Bildpuffer passt nicht zu den Massen".into()));
    }
    let mut rgb = Vec::with_capacity(bgra.len() / 4 * 3);
    for p in bgra.chunks_exact(4) {
        rgb.extend_from_slice(&[p[2], p[1], p[0]]);
    }
    let mut aus = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut aus, breite, hoehe);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header().map_err(|e| Fehler::Io(e.to_string()))?;
        w.write_image_data(&rgb).map_err(|e| Fehler::Io(e.to_string()))?;
    }
    Ok(aus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_farbkanaele_und_vollstaendig() {
        // 2x1: blau (B=255), rot (R=255), Alpha beliebig
        let bgra = [255, 0, 0, 0, 0, 0, 255, 7];
        let bytes = aus_bgra(2, 1, &bgra).unwrap();
        let (kopf, ende) = (&bytes[..16], &bytes[bytes.len() - 64.min(bytes.len())..]);
        crate::quellen::inhalt_vollstaendig(crate::quellen::BildFormat::Png, kopf, ende).unwrap();
        assert_eq!(crate::quellen::abmessungen(&bytes), Some((2, 1)));
        let mut dec = png::Decoder::new(&bytes[..]).read_info().unwrap();
        let mut buf = vec![0; dec.output_buffer_size()];
        let info = dec.next_frame(&mut buf).unwrap();
        assert_eq!(&buf[..info.buffer_size()], &[0, 0, 255, 255, 0, 0]);
    }

    #[test]
    fn falsche_groesse_scheitert() {
        assert!(aus_bgra(2, 2, &[0; 8]).is_err());
        assert!(aus_bgra(0, 1, &[]).is_err());
    }
}
