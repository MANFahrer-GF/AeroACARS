//! Hält macOS davon ab, AeroACARS während eines Flugs auszubremsen.
//!
//! # Der Anlass (AZU 8710, 11.10.2026)
//!
//! X-Plane 12 auf einem M3 Max, ein Bildschirm, AeroACARS also die ganze
//! Zeit hinter dem Simulator. X-Plane lief flüssig, unser Client aber stand
//! in der Flare wiederholt 0,4–0,8 s still — Aufsetz-Sampler und
//! Positionsstrom gleichzeitig, ohne dass im Code etwas so lange blockiert.
//! `LSAppNapIsDisabled` in der Info.plist (seit AFR 421) reichte nicht.
//!
//! macOS gibt Programmen im Hintergrund weniger Rechenzeit und bündelt ihre
//! Zeitgeber. Eine Aktivität mit `UserInitiated | LatencyCritical` sagt dem
//! System ausdrücklich: Hier läuft zeitkritische Arbeit, die der Nutzer
//! angestoßen hat. Sie gilt, solange der Wächter lebt.

/// Solange dieser Wert lebt, läuft die Aktivität. Außerhalb von macOS leer.
pub struct FlugAktivitaet {
    #[cfg(target_os = "macos")]
    token: Token,
}

#[cfg(target_os = "macos")]
struct Token(
    objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
);

// SAFETY: Das Token ist ein undurchsichtiges Objekt, das nur an
// `NSProcessInfo::endActivity` zurückgegeben wird. `NSProcessInfo` ist laut
// Apple threadsicher; die Aktivität darf auf jedem Thread beendet werden.
// Nötig, weil der Wächter in einer tokio-Aufgabe über `.await` hinweg lebt.
#[cfg(target_os = "macos")]
unsafe impl Send for Token {}

impl FlugAktivitaet {
    pub fn beginnen() -> Self {
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
            let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
                NSActivityOptions::UserInitiated | NSActivityOptions::LatencyCritical,
                &NSString::from_str("AeroACARS zeichnet einen Flug auf"),
            );
            tracing::info!("macOS-Aktivität für die Flugaufzeichnung begonnen");
            Self {
                token: Token(token),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {}
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for FlugAktivitaet {
    fn drop(&mut self) {
        // SAFETY: Das Token stammt aus `beginActivityWithOptions_reason`.
        unsafe {
            objc2_foundation::NSProcessInfo::processInfo().endActivity(&self.token.0);
        }
        tracing::info!("macOS-Aktivität für die Flugaufzeichnung beendet");
    }
}

/// Den aufrufenden Faden als dringlich anmelden — für die Aufsetzmessung.
///
/// macOS: Dienstgüte `USER_INTERACTIVE`, dieselbe Stufe wie die Bedienung
/// selbst; das System plant den Faden auch dann zuerst ein, wenn AeroACARS
/// hinter dem Simulator liegt. Andere Systeme: unverändert (der eigene Faden
/// allein löst schon die Abhängigkeit vom gemeinsamen Pool).
pub fn messfaden_vorrang() {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: Setzt nur die Dienstgüte des aufrufenden Fadens.
        let rc = unsafe {
            libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
        };
        if rc != 0 {
            tracing::warn!(rc, "Dienstgüte für den Messfaden nicht gesetzt");
        }
    }
}
