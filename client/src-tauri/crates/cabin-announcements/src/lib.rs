//! Kabinenansagen für AeroACARS (Beta, ab v2.0.25).
//!
//! Pure Logik ohne tauri/reqwest, damit alles ohne Simulator testbar ist:
//! - [`slot`]: der Ansage-Katalog (wer spricht, fest oder KI, Länge)
//! - [`flugart`]: Linie / Bizjet / aus (Fracht)
//! - [`zeit`]: Tageszeit und Ortszeit aus dem SimBrief-Plan
//! - [`planer`]: wann welche Ansage fällig wird
//! - [`warteschlange`]: nichts überlappt, Ruhephase, Verfall
//! - [`drehbuch`]: KI-Texte prüfen, bevor sie gesprochen werden
//! - [`ssml`]: Azure-SSML mit dem richtigen Locale
//! - [`cache`]: Dateinamen für den Audio-Cache
//!
//! Grundlage: LEITFADEN_Ansage-Drehbuch.md (Nextcloud,
//! Kabinenansagen_2026-10-17).

pub mod cache;
pub mod drehbuch;
pub mod flugart;
pub mod planer;
pub mod slot;
pub mod ssml;
pub mod warteschlange;
pub mod zeit;

pub use flugart::Flugart;
pub use planer::{Faellig, Flugdaten, Planer, Tick, Verfall};
pub use slot::{Rolle, Slot};
pub use warteschlange::Warteschlange;
