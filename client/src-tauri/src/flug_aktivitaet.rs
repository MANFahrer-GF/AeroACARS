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
/// hinter dem Simulator liegt. Gilt für Mess- und Auswertefaden.
///
/// Windows: siehe [`messfaden_vorrang_windows`] — nur für den Messfaden.
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

/// Windows: den MESSFADEN mit höchster Priorität einplanen (v2.0.19).
///
/// Anlass: Auf Rechnern am Limit kam der Messfaden im Ausrollen 40 bis 66 ms
/// zu spät dran, ohne dass AeroACARS etwas festhielt (Joel MDPC→MUVR mit
/// 2.0.18, Hamadoun CYUL→CYHZ und VHHH→FAOR, 14.10.2026, 3 bis 28 fps). Der
/// Mac hatte den Vorrang seit 2.0.10, Windows nicht.
///
/// Die Felddaten zeigen Vergabe, nicht Zeitgeber: Der Takt liegt im Mittel
/// bei 20,4 bis 21,6 ms, die Ausreisser unregelmaessig bei 26 bis 66 ms —
/// nicht auf dem 15,6-ms-Raster (31/47/62). Ein feinerer Zeitgeber
/// (`timeBeginPeriod`) haette deshalb nichts geaendert.
///
/// Warum `TIME_CRITICAL` und nicht `HIGHEST`: Ziel ist, auch gegen einen
/// voll ausgelasteten Simulator pünktlich dranzukommen, dessen eigene Fäden
/// selbst erhöht laufen können. Vertretbar ist das nur, weil der Faden
/// - je Takt rund 1 ms arbeitet und dazwischen 20 ms schläft (der Schlaf
///   ist die Bremse; MSFS verliert höchstens ~5 % eines Kerns),
/// - nie aktiv wartet: Er blockiert nur an der Adapter-Sperre
///   (`current_snapshot`, Sperre + Kopie) und an `diagnose`, das die
///   Auswertung nur kurz hält; `flight.stats` nimmt er nie.
///
/// ⚠ Deshalb NUR für `aufsetz_messen`, nie für den Auswertefaden: der
/// rechnet nach dem Aufsetzen auch Schweres.
/// Andere Systeme: nichts (macOS siehe [`messfaden_vorrang`]).
pub fn messfaden_vorrang_windows() {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
        };
        // SAFETY: Ändert nur die Priorität des aufrufenden Fadens;
        // `GetCurrentThread` liefert einen Pseudo-Handle, der nicht
        // geschlossen werden muss.
        let ok = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) };
        if ok == 0 {
            tracing::warn!("Vorrang für den Messfaden nicht gesetzt (Windows)");
        } else {
            tracing::info!("Messfaden mit Vorrang eingeplant (Windows, TIME_CRITICAL)");
        }
    }
}
