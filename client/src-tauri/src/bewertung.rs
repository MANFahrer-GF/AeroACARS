//! v3 Etappe 3 (P-4.31, P-4.33): Die Landebewertung laeuft nur noch im
//! WASM-Modul — auch der Rueckfall ist dasselbe Modul aus dem eingebauten
//! Paket, nie die native Rechnung. So rechnen Mac, Windows und live gleich.
//!
//! Reihenfolge: Modul des geltenden Paketstands → eingebautes Modul
//! (`vorlaeufig`) → keine Note (`modulfehler`).

use std::sync::{Arc, Mutex, OnceLock};

use bewertung_host::Host;
use landing_scoring::wasm::{Ausgabe, Eingabe};
use landing_scoring::LandingScoringInput;

/// Rechenzeitgrenze je Bewertung: 10 x Median der echten Protokolle (P-4.32).
/// Gemessen 29.10. im Gleichheitslauf (`bewertung-host/tests/
/// gleichheitslauf.rs`) ueber 1 860 Landungen: Median 974 274, Maximum
/// 5 044 000.
pub(crate) const FUEL_GRENZE: u64 = 9_742_740;

/// Woher die Note kam (Vermerk im PIREP, P-4.33).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    /// Modul des geltenden Paketstands.
    Endgueltig,
    /// Paketmodul gescheitert, eingebautes Modul gerechnet.
    Vorlaeufig,
}

/// Nur fuer Tests (P-4.40): erzwingt die Faelle der Tabelle in Kap. 4.5.
/// Gilt je Thread; die Wache stellt beim Verlassen den Normalfall wieder her.
/// Im Produktionscode gibt es das nicht.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Erzwungen {
    /// Paketmodul faellt aus: `vorlaeufig`.
    PaketKaputt,
    /// Beide fallen aus: `modulfehler`.
    BeideKaputt,
}

#[cfg(test)]
thread_local! {
    static ERZWUNGEN: std::cell::Cell<Option<Erzwungen>> = const { std::cell::Cell::new(None) };
    /// Wie oft [`bewerten`] auf diesem Thread lief — fuer die Tests „eine
    /// Bewertung je Landung bzw. je PIREP-Aufbau“.
    static AUFRUFE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Nur fuer Tests: Aufrufe von [`bewerten`] auf diesem Thread seit dem
/// letzten [`aufrufe_zuruecksetzen`].
#[cfg(test)]
pub(crate) fn aufrufe() -> u32 {
    AUFRUFE.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) fn aufrufe_zuruecksetzen() {
    AUFRUFE.with(|c| c.set(0));
}

#[cfg(test)]
pub(crate) struct ErzwungenWache;

#[cfg(test)]
impl Drop for ErzwungenWache {
    fn drop(&mut self) {
        ERZWUNGEN.with(|c| c.set(None));
    }
}

/// Erzwingt `fall`, bis die Wache faellt.
#[cfg(test)]
pub(crate) fn erzwingen(fall: Erzwungen) -> ErzwungenWache {
    ERZWUNGEN.with(|c| c.set(Some(fall)));
    ErzwungenWache
}

#[cfg(test)]
fn erzwungen() -> Option<Erzwungen> {
    ERZWUNGEN.with(std::cell::Cell::get)
}

fn paket_host() -> Option<&'static Host> {
    #[cfg(test)]
    if erzwungen().is_some() {
        return None;
    }
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
    #[cfg(test)]
    if erzwungen() == Some(Erzwungen::BeideKaputt) {
        return None;
    }
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

// Nur fuer Tests: erzwingt den Pinstand (P-3.31) auf diesem Thread.
#[cfg(test)]
thread_local! {
    static PIN_ERZWUNGEN: std::cell::Cell<Option<crate::laufzeit_paket::PinStand>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) struct PinWache;

#[cfg(test)]
impl Drop for PinWache {
    fn drop(&mut self) {
        PIN_ERZWUNGEN.with(|c| c.set(None));
    }
}

/// Nur fuer Tests: Pinstand `p` fuer jeden Flug, bis die Wache faellt.
#[cfg(test)]
pub(crate) fn pin_erzwingen(p: crate::laufzeit_paket::PinStand) -> PinWache {
    PIN_ERZWUNGEN.with(|c| c.set(Some(p)));
    PinWache
}

fn pin_stand(pirep_id: &str) -> crate::laufzeit_paket::PinStand {
    #[cfg(test)]
    if let Some(p) = PIN_ERZWUNGEN.with(std::cell::Cell::get) {
        return p;
    }
    crate::laufzeit_paket::pin_stand(pirep_id)
}

/// Ob die Bewertung dieses Flugs ohne seinen gepinnten Stand rechnet (P-3.31).
pub(crate) fn pin_fehlt(pirep_id: &str) -> bool {
    pin_stand(pirep_id) == crate::laufzeit_paket::PinStand::Fehlt
}

/// Bewertet eine Landung des Flugs `pirep_id` (P-3.31): Gilt sein gepinnter
/// Stand, wie [`bewerten`]. Fehlt er, rechnet die eingebaute Fassung und die
/// Note ist `vorlaeufig` („vorlaeufig, Fassung n“). Wurde er nachgeholt,
/// rechnet sein eigenes Modul (`endgueltig`); scheitert das, wie [`bewerten`]
/// mit `vorlaeufig`.
pub(crate) fn bewerten_fuer(pirep_id: &str, landung: &LandingScoringInput) -> Option<(Ausgabe, Status)> {
    use crate::laufzeit_paket::PinStand;
    let vorlaeufig = |b: Option<(Ausgabe, Status)>| b.map(|(a, _)| (a, Status::Vorlaeufig));
    match pin_stand(pirep_id) {
        PinStand::Gilt => bewerten(landung),
        PinStand::Fehlt => vorlaeufig(bewerten(landung)),
        PinStand::Nachgeholt(modul) => {
            #[cfg(test)]
            AUFRUFE.with(|c| c.set(c.get() + 1));
            let host = nachgeholter_host(modul);
            let json = serde_json::to_string(&Eingabe { landung: landung.clone() }).ok()?;
            match host.as_ref().map(|h| rechnen(h, &json)) {
                Some(Ok(a)) => Some((a, Status::Endgueltig)),
                _ => {
                    tracing::error!("Bewertung mit dem nachgeholten Stand gescheitert — vorlaeufig");
                    vorlaeufig(bewerten(landung))
                }
            }
        }
    }
}

/// Wie viele Hosts nachgeholter Staende geladen bleiben (die zuletzt gebrauchten).
const NACHGEHOLT_MAX: usize = 3;

/// Der Host des nachgeholten Stands eines offenen Flugs (P-3.31), je Modul
/// einmal geladen (auch ein Ladefehler gilt fuer dieses Modul). Schluessel ist
/// die Adresse des Moduls: `pin_nachholen` legt jedes nachgeholte Modul als
/// eigenen, nie freigegebenen Speicher ab (`Box::leak`) — andere Adresse =
/// anderer Stand (Codex 30.10.: vorher EIN Host fuer alle Fluege).
fn nachgeholter_host(modul: &'static [u8]) -> Option<Arc<Host>> {
    type Eintrag = ((usize, usize), Option<Arc<Host>>);
    static H: Mutex<Vec<Eintrag>> = Mutex::new(Vec::new());
    let schluessel = (modul.as_ptr() as usize, modul.len());
    let mut h = H.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(i) = h.iter().position(|(k, _)| *k == schluessel) {
        let e = h.remove(i);
        h.insert(0, e);
        return h[0].1.clone();
    }
    let host = match Host::neu(modul) {
        Ok(h) => Some(Arc::new(h)),
        Err(e) => {
            tracing::error!(error = %e, "Bewertungsmodul des nachgeholten Stands nicht ladbar");
            None
        }
    };
    h.insert(0, (schluessel, host.clone()));
    h.truncate(NACHGEHOLT_MAX);
    host
}

/// Bewertet eine Landung im Modul. `None` = `modulfehler` (beide Module
/// gescheitert) — dann gibt es keine Note.
pub(crate) fn bewerten(landung: &LandingScoringInput) -> Option<(Ausgabe, Status)> {
    #[cfg(test)]
    AUFRUFE.with(|c| c.set(c.get() + 1));
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

/// v3 (P-4.21): Soll-Band des Anflugs aus dem Modul (Export `sollband`) des
/// Flugs `pirep_id` — dieselbe Wahl wie bei der Note ([`bewerten_fuer`]):
/// nachgeholter Stand, sonst Paketmodul, sonst eingebautes. Faktor und
/// Bandbreite fragt es beim selben Modul ab (eine Probe 100 kt beim Winkel
/// und bei 3°), wie die Webapp (`sollbandModul.ts`); scheitert ein Aufruf,
/// rechnet das naechste Modul alles neu — nie gemischt (Codex 30.10.).
/// `None`, wenn kein Modul antwortet.
pub(crate) fn sollband(
    pirep_id: &str,
    proben: &[landing_scoring::sollband::Probe],
    winkel_grad: Option<f64>,
) -> Option<storage::ModulSollband> {
    use landing_scoring::sollband::{Anfrage, Probe, Punkt};
    let mit = |host: &Host| -> Option<storage::ModulSollband> {
        let rufen = |anfrage: Anfrage| -> Option<Vec<Punkt>> {
            let json = serde_json::to_string(&anfrage).ok()?;
            serde_json::from_str(&host.sollband(&json, FUEL_GRENZE).ok()?).ok()
        };
        let bezug = |w: Option<f64>| {
            rufen(Anfrage { proben: vec![Probe { gs_kt: Some(100.0), ..Default::default() }], winkel_grad: w })?
                .into_iter()
                .next()
        };
        let (beim_winkel, bei_3) = (bezug(winkel_grad)?, bezug(Some(3.0))?);
        Some(storage::ModulSollband {
            punkte: rufen(Anfrage { proben: proben.to_vec(), winkel_grad })?,
            faktor: beim_winkel.soll / bei_3.soll,
            toleranz: bei_3.oben - bei_3.soll,
        })
    };
    let nachgeholt = match pin_stand(pirep_id) {
        crate::laufzeit_paket::PinStand::Nachgeholt(modul) => nachgeholter_host(modul),
        _ => None,
    };
    for host in [nachgeholt.as_deref(), paket_host(), eingebauter_host()].into_iter().flatten() {
        match mit(host) {
            Some(b) => return Some(b),
            None => tracing::warn!("Soll-Band im Modul gescheitert, naechstes Modul"),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P-4.21: Soll-Band, Faktor und Bandbreite kommen aus dem Modul und
    /// gleichen der Regel im Crate (dieselbe Quelle wie das Modul).
    #[test]
    fn sollband_kommt_aus_dem_modul() {
        use landing_scoring::sollband as sb;
        let proben: Vec<sb::Probe> = [(Some(140.0), None, Some(-9000.0)), (Some(20.0), None, None), (Some(130.0), Some(true), None), (Some(128.0), None, Some(500.0))]
            .into_iter()
            .map(|(gs_kt, is_flare, t_ms)| sb::Probe { gs_kt, is_flare, t_ms })
            .collect();
        let m = sollband("TEST", &proben, Some(4.0)).expect("Modul antwortet");
        // Bis 1e-9 wie das Crate nativ: `tan` im Modul (wasm, libm) und
        // nativ (Windows-CRT) unterscheiden sich in der letzten Stelle
        // (CI 30.10.: -991.9060244483051 gegen ...052). Gezeigt wird, was
        // das Modul rechnet — auf jeder Plattform dasselbe.
        let nativ = sb::sollband(&proben, Some(4.0));
        assert_eq!(m.punkte.len(), 1, "nur die erste Probe zaehlt");
        assert_eq!(m.punkte.len(), nativ.len());
        for (a, b) in m.punkte.iter().zip(&nativ) {
            assert_eq!(a.index, b.index);
            for (x, y) in [(a.soll, b.soll), (a.oben, b.oben), (a.unten, b.unten)] {
                assert!((x - y).abs() < 1e-9, "{x} gegen {y}");
            }
        }
        assert!((m.faktor - sb::gleitwinkel_faktor(Some(4.0))).abs() < 1e-12, "{}", m.faktor);
        assert!((m.toleranz - sb::SOLLBAND_TOLERANZ_FPM).abs() < 1e-12);
        let drei = sollband("TEST", &proben, None).unwrap();
        assert!((drei.faktor - 1.0).abs() < 1e-12);
    }

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

    /// P-3.31: Fehlt der Stand des Flugs, rechnet die eingebaute Fassung und
    /// die Note heisst `vorlaeufig` (Gegenprobe: gilt er, `endgueltig`). Ist er
    /// nachgeholt, rechnet sein Modul `endgueltig` — hier das eingebaute als
    /// Stellvertreter, das Ergebnis muss dasselbe sein.
    #[test]
    fn ohne_pinstand_vorlaeufig_nachgeholt_endgueltig() {
        use crate::laufzeit_paket::PinStand;
        let e = LandingScoringInput { vs_fpm: Some(-210.0), peak_g_load: Some(1.3), bounce_count: Some(0), ..Default::default() };
        let (normal, s) = bewerten_fuer("P", &e).expect("Modul rechnet");
        assert_eq!(s, Status::Endgueltig, "Gegenprobe: Stand gilt");
        let _w = pin_erzwingen(PinStand::Fehlt);
        let (a, s) = bewerten_fuer("P", &e).expect("eingebaute Fassung rechnet");
        assert_eq!((s, a.gesamt, a.fassung), (Status::Vorlaeufig, normal.gesamt, 27));
        assert!(pin_fehlt("P"));
        let _w = pin_erzwingen(PinStand::Nachgeholt(crate::laufzeit_paket::eingebautes_bewertungsmodul().unwrap()));
        let (a, s) = bewerten_fuer("P", &e).expect("nachgeholtes Modul rechnet");
        assert_eq!((s, a.gesamt), (Status::Endgueltig, normal.gesamt));
        assert!(!pin_fehlt("P"));
    }

    /// P-3.31 (Codex 30.10.): Jeder Flug rechnet mit dem Modul SEINES
    /// nachgeholten Stands, nicht mit dem zuerst nachgeholten. Stellvertreter:
    /// ein ladbares Modul (`endgueltig`) und ein unladbares (Rueckfall,
    /// `vorlaeufig`) — mit einem einzigen Host je Lauf rechnete der zweite
    /// Flug mit dem Modul des ersten.
    #[test]
    fn nachgeholt_rechnet_je_flug_mit_dem_eigenen_modul() {
        use crate::laufzeit_paket::PinStand;
        let e = LandingScoringInput { vs_fpm: Some(-210.0), peak_g_load: Some(1.3), bounce_count: Some(0), ..Default::default() };
        let gut = crate::laufzeit_paket::eingebautes_bewertungsmodul().unwrap();
        let kaputt: &'static [u8] = Box::leak(b"kein wasm".to_vec().into_boxed_slice());
        for (pirep, modul, erwartet) in [("A", gut, Status::Endgueltig), ("B", kaputt, Status::Vorlaeufig), ("C", gut, Status::Endgueltig)] {
            let _w = pin_erzwingen(PinStand::Nachgeholt(modul));
            assert_eq!(bewerten_fuer(pirep, &e).map(|(_, s)| s), Some(erwartet), "Flug {pirep}");
            assert!(sollband(pirep, &[], None).is_some(), "Soll-Band {pirep}");
        }
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
