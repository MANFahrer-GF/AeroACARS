//! v3 Etappe 3 (P-4.31, P-4.33): Die Landebewertung laeuft nur noch im
//! WASM-Modul — auch der Rueckfall ist dasselbe Modul aus dem eingebauten
//! Paket, nie die native Rechnung. So rechnen Mac, Windows und live gleich.
//!
//! Reihenfolge: Modul des geltenden Paketstands → eingebautes Modul
//! (`vorlaeufig`) → keine Note (`modulfehler`).

use std::sync::OnceLock;

use bewertung_host::Host;
use landing_scoring::wasm::{Ausgabe, Eingabe};
use landing_scoring::LandingScoringInput;

/// Rechenzeitgrenze je Bewertung. VORLAEUFIG: P-4.32 verlangt 10 x Median
/// der echten Protokolle (Messung mit dem Gleichheitslauf, Etappe 3). Gemessen
/// 29.10. an synthetischen Eingaben: Median 0,42 Mio., Maximum 0,86 Mio.
pub(crate) const FUEL_GRENZE: u64 = 50_000_000;

/// Woher die Note kam (Vermerk im PIREP, P-4.33).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    /// Modul des geltenden Paketstands.
    Endgueltig,
    /// Paketmodul gescheitert, eingebautes Modul gerechnet.
    Vorlaeufig,
}

fn paket_host() -> Option<&'static Host> {
    static H: OnceLock<Option<Host>> = OnceLock::new();
    H.get_or_init(|| match Host::neu(crate::laufzeit_paket::stand().bewertung) {
        Ok(h) => Some(h),
        Err(e) => {
            tracing::error!(error = %e, "Bewertungsmodul des Pakets nicht ladbar");
            None
        }
    })
    .as_ref()
}

fn eingebauter_host() -> Option<&'static Host> {
    static H: OnceLock<Option<Host>> = OnceLock::new();
    H.get_or_init(|| match crate::laufzeit_paket::eingebautes_bewertungsmodul().map(Host::neu) {
        Some(Ok(h)) => Some(h),
        Some(Err(e)) => {
            tracing::error!(error = %e, "eingebautes Bewertungsmodul nicht ladbar");
            None
        }
        None => None,
    })
    .as_ref()
}

fn rechnen(host: &Host, json: &str) -> Result<Ausgabe, String> {
    let aus = host.bewerten(json, FUEL_GRENZE).map_err(|e| e.to_string())?;
    serde_json::from_str::<Ausgabe>(&aus).map_err(|_| format!("Ausgabe ungueltig: {}", aus.chars().take(200).collect::<String>()))
}

/// Text fuer `bewertung_status` (P-4.33): `endgueltig`, `vorlaeufig` oder
/// `modulfehler` (keine Note, weil beide Module scheiterten).
pub(crate) fn status_text(b: &Option<(Ausgabe, Status)>) -> &'static str {
    match b {
        Some((_, Status::Endgueltig)) => "endgueltig",
        Some((_, Status::Vorlaeufig)) => "vorlaeufig",
        None => "modulfehler",
    }
}

/// Bewertet eine Landung im Modul. `None` = `modulfehler` (beide Module
/// gescheitert) — dann gibt es keine Note.
pub(crate) fn bewerten(landung: &LandingScoringInput) -> Option<(Ausgabe, Status)> {
    let json = serde_json::to_string(&Eingabe { landung: landung.clone() }).ok()?;
    match paket_host().map(|h| rechnen(h, &json)) {
        Some(Ok(a)) => return Some((a, Status::Endgueltig)),
        Some(Err(e)) => tracing::error!(error = %e, "Bewertung im Paketmodul gescheitert, eingebautes Modul"),
        None => {}
    }
    match eingebauter_host().map(|h| rechnen(h, &json)) {
        Some(Ok(a)) => Some((a, Status::Vorlaeufig)),
        Some(Err(e)) => {
            tracing::error!(error = %e, "Bewertung auch im eingebauten Modul gescheitert: modulfehler");
            None
        }
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Das Modul rechnet wie das Crate (eingebauter Stand) — Note, Deckel,
    /// Teilnoten.
    #[test]
    fn modul_rechnet_wie_das_crate() {
        let e = LandingScoringInput { vs_fpm: Some(-210.0), peak_g_load: Some(1.3), bounce_count: Some(0), ..Default::default() };
        let (a, status) = bewerten(&e).expect("Modul rechnet");
        assert_eq!(status, Status::Endgueltig);
        let subs = landing_scoring::compute_sub_scores(&e);
        assert_eq!(a.teilnoten, subs);
        assert_eq!(a.gesamt, landing_scoring::aggregate_master_score(&subs));
        assert_eq!(a.fassung, 27);
        assert_eq!(status_text(&Some((a, status))), "endgueltig");
        assert_eq!(status_text(&None), "modulfehler");
    }

    /// P-4.50: Die Fassung, die der Client schreibt, ist die des eingebauten
    /// Moduls.
    #[test]
    fn fassung_gleich_modul() {
        let (a, _) = bewerten(&LandingScoringInput::default()).expect("Modul rechnet");
        assert_eq!(u32::from(crate::SCORE_ALGORITHMUS_VERSION), a.fassung);
        assert_eq!(a.fassung, landing_scoring::wasm::FASSUNG);
    }
}
