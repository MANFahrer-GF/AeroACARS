//! Strenge Namen. Ein Bild besteht im Ordner `<gruppe>/` aus Teilen mit
//! festen Endungen, alle mit demselben `<id>`:
//!
//! | Teil     | Datei                      |
//! |----------|----------------------------|
//! | JPEG     | `<id>.jpg`                 |
//! | Vorschau | `<id>.vorschau.jpg`        |
//! | Sidecar  | `<id>.json`                |
//! | Quelle   | `<id>.quelle.png/.jpg`     |
//!
//! Gruppen- und Bildnamen sind `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`: kein Punkt,
//! kein Trennzeichen, also kann weder `..` noch ein anderes Laufwerk darin
//! stecken, und `<id>.<endung>` laesst sich eindeutig zerlegen.

use crate::Fehler;

pub fn name_gueltig(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 64 || !b[0].is_ascii_alphanumeric() {
        return false;
    }
    b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
}

pub fn name_pruefen(was: &str, s: &str) -> Result<(), Fehler> {
    if name_gueltig(s) {
        Ok(())
    } else {
        Err(Fehler::Name(format!("{was} ungueltig (erlaubt: Buchstaben, Ziffern, _ und -, hoechstens 64 Zeichen)")))
    }
}

/// Teil eines Bildes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Teil {
    Jpeg,
    Vorschau,
    Sidecar,
    /// Eingangskopie der Originaldatei (`png` oder `jpg`), bis das Paket das JPEG geschrieben hat.
    Quelle(QuellFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuellFormat {
    Png,
    Jpg,
}

impl QuellFormat {
    pub fn endung(self) -> &'static str {
        match self {
            QuellFormat::Png => "png",
            QuellFormat::Jpg => "jpg",
        }
    }
}

impl Teil {
    pub fn dateiname(self, id: &str) -> String {
        match self {
            Teil::Jpeg => format!("{id}.jpg"),
            Teil::Vorschau => format!("{id}.vorschau.jpg"),
            Teil::Sidecar => format!("{id}.json"),
            Teil::Quelle(f) => format!("{id}.quelle.{}", f.endung()),
        }
    }
}

/// Zerlegt einen Dateinamen aus dem verwalteten Ordner in (id, Teil).
pub fn teil_aus_dateiname(datei: &str) -> Option<(String, Teil)> {
    let (id, rest) = datei.split_once('.')?;
    if !name_gueltig(id) {
        return None;
    }
    let teil = match rest {
        "jpg" => Teil::Jpeg,
        "vorschau.jpg" => Teil::Vorschau,
        "json" => Teil::Sidecar,
        "quelle.png" => Teil::Quelle(QuellFormat::Png),
        "quelle.jpg" => Teil::Quelle(QuellFormat::Jpg),
        _ => return None,
    };
    Some((id.to_string(), teil))
}

/// Relativer Pfad `<gruppe>/<datei>` (immer mit `/`, auch unter Windows).
pub fn rel_pfad(gruppe: &str, datei: &str) -> String {
    format!("{gruppe}/{datei}")
}

/// Liest einen relativen Pfad der Oberflaeche streng: genau zwei Teile,
/// gueltige Namen, bekannte Endung. Alles andere (`..`, `\`, Laufwerk,
/// absolut, leere Teile) ist ein Fehler, bevor das Dateisystem beruehrt wird.
pub fn rel_pfad_lesen(rel: &str) -> Result<(String, String, Teil), Fehler> {
    let ungueltig = || Fehler::Pfad(format!("Pfad ungueltig: {rel}"));
    let mut teile = rel.split('/');
    let (Some(gruppe), Some(datei), None) = (teile.next(), teile.next(), teile.next()) else {
        return Err(ungueltig());
    };
    if !name_gueltig(gruppe) {
        return Err(ungueltig());
    }
    let (id, teil) = teil_aus_dateiname(datei).ok_or_else(ungueltig)?;
    Ok((gruppe.to_string(), id, teil))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namen_streng() {
        for ok in ["a", "A1", "2026-11-07", "abc_DEF-9", &"a".repeat(64)] {
            assert!(name_gueltig(ok), "{ok}");
        }
        for schlecht in ["", ".", "..", "a.b", "a/b", "a\\b", "-a", "_a", "a b", "ä", &"a".repeat(65), "a\0b", "C:"] {
            assert!(!name_gueltig(schlecht), "{schlecht:?}");
        }
    }

    #[test]
    fn dateiname_und_zerlegen_sind_umkehrbar() {
        for t in [Teil::Jpeg, Teil::Vorschau, Teil::Sidecar, Teil::Quelle(QuellFormat::Png), Teil::Quelle(QuellFormat::Jpg)] {
            assert_eq!(teil_aus_dateiname(&t.dateiname("abc-1")), Some(("abc-1".to_string(), t)));
        }
        for fremd in ["abc.png", "abc.jpeg", "abc.json.bak", ".tmp", "abc", "abc.quelle.gif", "a b.jpg"] {
            assert_eq!(teil_aus_dateiname(fremd), None, "{fremd}");
        }
    }

    #[test]
    fn relativer_pfad_ausbruch_scheitert_vor_dem_dateisystem() {
        assert_eq!(rel_pfad_lesen("g/abc.jpg").unwrap(), ("g".into(), "abc".into(), Teil::Jpeg));
        for schlecht in [
            "../abc.jpg", "g/../abc.jpg", "g/../../x.jpg", "/g/abc.jpg", "g//abc.jpg", "g/abc.jpg/", "g\\abc.jpg",
            "g/sub/abc.jpg", "abc.jpg", "", "g/..", "C:/x.jpg", "g/abc.jpg\0", "g/../g/abc.jpg", "./g/abc.jpg",
        ] {
            assert!(rel_pfad_lesen(schlecht).is_err(), "{schlecht:?}");
        }
    }
}
