//! Ein Fehlertyp fuer alle Bausteine; `code()` wird zum `UiError`-Code.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fehler {
    /// Name von Gruppe, Bild oder Teil entspricht nicht dem festen Muster.
    Name(String),
    /// Relativer Pfad verlaesst den verwalteten Ordner oder hat die falsche Form.
    Pfad(String),
    NichtGefunden(String),
    /// Kein JPEG/PNG, oder Inhalt passt nicht zur Endung.
    Format(String),
    /// Datei noch nicht vollstaendig geschrieben.
    Unfertig(String),
    /// Pfad liegt nicht in einer bestaetigten, aktiven Quelle.
    Quelle(String),
    /// Aktion in dieser Lage nicht erlaubt (z. B. Verschieben aus einer Quelle ohne Freigabe).
    Verboten(String),
    /// Zu gross.
    Gross(String),
    /// Angabe des Aufrufers unbrauchbar (Sidecar kein Objekt, Tastenkuerzel, Ordner ...).
    Eingabe(String),
    /// Auf diesem System nicht moeglich.
    NichtVerfuegbar(String),
    /// Kein AeroACARS-Flug aktiv: keine Uebernahme, keine Aufnahme.
    KeinFlug(String),
    /// Ein anderes Fenster liegt ueber dem Simulator: keine Bildschirmkopie.
    Verdeckt(String),
    Io(String),
}

impl Fehler {
    pub fn code(&self) -> &'static str {
        match self {
            Fehler::Name(_) => "bild_name",
            Fehler::Pfad(_) => "bild_pfad",
            Fehler::NichtGefunden(_) => "bild_nicht_gefunden",
            Fehler::Format(_) => "bild_format",
            Fehler::Unfertig(_) => "bild_unfertig",
            Fehler::Quelle(_) => "bild_quelle",
            Fehler::Verboten(_) => "bild_verboten",
            Fehler::Gross(_) => "bild_zu_gross",
            Fehler::Eingabe(_) => "bild_eingabe",
            Fehler::NichtVerfuegbar(_) => "bild_nicht_verfuegbar",
            Fehler::KeinFlug(_) => "kein_flug",
            Fehler::Verdeckt(_) => "verdeckt",
            Fehler::Io(_) => "bild_io",
        }
    }

    pub fn meldung(&self) -> &str {
        match self {
            Fehler::Name(s)
            | Fehler::Pfad(s)
            | Fehler::NichtGefunden(s)
            | Fehler::Format(s)
            | Fehler::Unfertig(s)
            | Fehler::Quelle(s)
            | Fehler::Verboten(s)
            | Fehler::Gross(s)
            | Fehler::Eingabe(s)
            | Fehler::NichtVerfuegbar(s)
            | Fehler::KeinFlug(s)
            | Fehler::Verdeckt(s)
            | Fehler::Io(s) => s,
        }
    }
}

impl fmt::Display for Fehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.meldung())
    }
}

impl std::error::Error for Fehler {}

impl From<std::io::Error> for Fehler {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::NotFound {
            Fehler::NichtGefunden(e.to_string())
        } else {
            Fehler::Io(e.to_string())
        }
    }
}
