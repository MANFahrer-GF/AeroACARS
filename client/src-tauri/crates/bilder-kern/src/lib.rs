//! Bilder-Tab (N-13, Faehigkeit `bilder-1`, P-11.30 bis P-11.34).
//!
//! Grundsatz (P-11.33): Dieser Teil liefert nur Plattform-Bausteine und trifft
//! keine fachlichen Entscheidungen (aktiver Flug, Zuordnung, Metadaten,
//! Verkleinern, EXIF, Filter). Das ist Sache des Laufzeit-Pakets. Hier liegen
//! die reinen, ohne Tauri testbaren Teile; die Tauri-Befehle in
//! `src/bilder.rs` sind duenne Adapter darueber.
//!
//! * [`namen`]       strenge Namenspruefung, ein Bild = Teile mit festen Endungen
//! * [`speicher`]    verwalteter Ordner: lesen, schreiben (atomar), loeschen
//! * [`eingang`]     dauerhafter Eingang der Funde (`.eingang/`)
//! * [`jpeg`]        EXIF/XMP (APP1) aus JPEG-Bytes entfernen
//! * [`kontext`]     Stand zu einer Zeit aus Flugprotokoll und Logbuch
//! * [`quellen`]     bestaetigte Quellordner, Uebernahme, „vollstaendig geschrieben“
//! * [`beobachter`]  Ordnerbeobachtung (Abfrage im Takt), Ereignis `bild_quelle_neu`
//! * [`live`]        Multipart-Aufbau fuer `POST /api/bilder` (P-7.25 „Vertrag“)
//! * [`sicher`]      Dateien ohne Linkfolgen oeffnen (Symlink-/Wettlauf-Schutz)
//! * [`verdeckung`]  liegt ein fremdes Fenster ueber dem Simulator? (reine Rechteckpruefung)
//! * [`suche`]       reine Teile des Aufspuerens von Bildquellen
//! * [`png`]         PNG-Kodierung der Fensteraufnahme
//! * [`tastenkuerzel`] Tastenkuerzel-Text lesen
//! * [`plattform`]   Windows-Teile (Known Folder, Registry, Vordergrund,
//!   Fensteraufnahme, Hotkey); auf anderen Systemen Ersatz ohne Wirkung

pub mod beobachter;
pub mod eingang;
pub mod fehler;
pub mod jpeg;
pub mod kontext;
pub mod live;
pub mod namen;
pub mod plattform;
pub mod png;
pub mod quellen;
pub mod sicher;
pub mod speicher;
pub mod suche;
pub mod tastenkuerzel;
pub mod verdeckung;

pub use fehler::Fehler;
