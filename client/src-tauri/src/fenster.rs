//! Größe und Lage des Hauptfensters.
//!
//! v1.7.29 (Pilotenwunsch Thomas K., 15.09.2026): Das Cockpit ist höher als
//! die Startgröße 1200×800 — die unteren Felder und Knöpfe lagen unter der
//! Kante, das Fenster musste nach jedem Start von Hand größer gezogen werden.
//!
//! Zwei Teile:
//! - Das Hauptfenster merkt sich Größe, Lage und Maximiert-Zustand
//!   (`tauri-plugin-window-state`) und öffnet beim nächsten Start so.
//!   Bewusst OHNE Sichtbarkeit: wer über das Tray beendet, während das
//!   Fenster versteckt ist, bekäme sonst beim nächsten Start kein Fenster.
//! - Beim Wechsel ins Cockpit darf die Oberfläche das Fenster auf die Höhe
//!   des Inhalts VERGRÖSSERN — nie verkleinern, nie über die Arbeitsfläche
//!   des Bildschirms hinaus, nicht bei maximiertem Fenster. Wer es danach
//!   kleiner zieht, behält das.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use crate::UiError;

const HAUPTFENSTER: &str = "main";

fn gemerkte_eigenschaften() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED
}

pub fn zustand_plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_window_state::Builder::new()
        .with_state_flags(gemerkte_eigenschaften())
        .with_filter(|label| label == HAUPTFENSTER)
        .build()
}

/// Das Plugin schreibt die Datei nur beim geordneten Beenden. Ein Update
/// beendet den Prozess aber hart (siehe Gedächtnis
/// „Update-Relaunch = unclean exit“) — deshalb kurz nach dem letzten
/// Verschieben/Größenändern zusätzlich sichern.
///
/// Codex-QS (15.09.2026) P2: EIN Hintergrund-Faden mit gleitender Frist,
/// nicht ein Faden pro Ereignis (beim Ziehen feuern Hunderte).
const SICHERN_NACH: Duration = Duration::from_millis(1500);

#[derive(Debug)]
struct Sicherung {
    frist: Option<Instant>,
    faden_laeuft: bool,
}

#[derive(Debug, PartialEq)]
enum Schritt {
    Warten(Duration),
    Sichern,
}

impl Sicherung {
    /// Ein Ereignis schiebt die Frist hinaus. `true`: es läuft noch kein
    /// Faden, der Aufrufer muss einen starten (der Zustand gilt ab jetzt
    /// als belegt).
    fn ereignis(&mut self, jetzt: Instant) -> bool {
        self.frist = Some(jetzt + SICHERN_NACH);
        !std::mem::replace(&mut self.faden_laeuft, true)
    }

    /// Was der Faden als Nächstes tut. Beim Sichern bleibt er BELEGT —
    /// ein Ereignis währenddessen setzt nur eine neue Frist.
    fn schritt(&mut self, jetzt: Instant) -> Schritt {
        match self.frist {
            Some(frist) if frist > jetzt => Schritt::Warten(frist - jetzt),
            _ => {
                self.frist = None;
                Schritt::Sichern
            }
        }
    }

    /// Nach dem Sichern: `true` = kam in der Zwischenzeit ein Ereignis,
    /// weitermachen; sonst gibt der Faden den Zustand frei.
    fn nach_dem_sichern(&mut self) -> bool {
        if self.frist.is_some() {
            return true;
        }
        self.faden_laeuft = false;
        false
    }
}

static SICHERUNG: Mutex<Sicherung> = Mutex::new(Sicherung {
    frist: None,
    faden_laeuft: false,
});

fn sicherung() -> std::sync::MutexGuard<'static, Sicherung> {
    SICHERUNG.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn nach_aenderung_sichern<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if window.label() != HAUPTFENSTER {
        return;
    }
    if !matches!(
        event,
        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_)
    ) {
        return;
    }
    if !sicherung().ereignis(Instant::now()) {
        return;
    }

    let app = window.app_handle().clone();
    let gestartet = std::thread::Builder::new()
        .name("fensterlage-sichern".into())
        .spawn(move || loop {
            let schritt = sicherung().schritt(Instant::now());
            match schritt {
                Schritt::Warten(dauer) => std::thread::sleep(dauer),
                Schritt::Sichern => {
                    if let Err(e) = app.save_window_state(gemerkte_eigenschaften()) {
                        tracing::warn!(error = %e, "Fensterlage ließ sich nicht sichern");
                    }
                    if !sicherung().nach_dem_sichern() {
                        break;
                    }
                }
            }
        });
    if let Err(e) = gestartet {
        tracing::warn!(error = %e, "Faden zum Sichern der Fensterlage ließ sich nicht starten");
        sicherung().faden_laeuft = false;
    }
}

/// Neue äußere Höhe und obere Kante in physischen Pixeln, oder `None`,
/// wenn nichts zu tun ist. Rein, damit die Grenzen ohne Fenster testbar sind.
///
/// - `innen_h`/`aussen_h`: aktuelle Höhe des Inhaltsbereichs / des ganzen
///   Fensters samt Titelleiste
/// - `benoetigt_innen_h`: Höhe, die der Inhalt braucht
/// - `fenster_y`: obere Kante des Fensters
/// - `flaeche_y`/`flaeche_h`: Arbeitsfläche des Bildschirms (ohne Taskleiste)
pub(crate) fn neue_hoehe_und_lage(
    innen_h: u32,
    aussen_h: u32,
    benoetigt_innen_h: u32,
    fenster_y: i32,
    flaeche_y: i32,
    flaeche_h: u32,
) -> Option<(u32, i32)> {
    let deko = aussen_h.saturating_sub(innen_h);
    let ziel_aussen = benoetigt_innen_h.saturating_add(deko).min(flaeche_h);
    // Nur echtes Wachstum; ein paar Pixel Rundung sind kein Anlass.
    if ziel_aussen <= aussen_h.saturating_add(4) {
        return None;
    }
    let flaeche_unten = flaeche_y as i64 + flaeche_h as i64;
    let unten = fenster_y as i64 + ziel_aussen as i64;
    let y = if unten > flaeche_unten {
        (flaeche_unten - ziel_aussen as i64).max(flaeche_y as i64) as i32
    } else {
        fenster_y
    };
    Some((ziel_aussen, y))
}

/// Vergrößert das Hauptfenster, bis `benoetigt_hoehe` (CSS-Pixel des
/// Inhaltsbereichs) hineinpasst — höchstens bis zur Arbeitsfläche.
#[tauri::command]
pub async fn fenster_an_inhalt_anpassen(
    window: WebviewWindow,
    benoetigt_hoehe: f64,
) -> Result<bool, UiError> {
    if window.label() != HAUPTFENSTER || !benoetigt_hoehe.is_finite() || benoetigt_hoehe <= 0.0 {
        return Ok(false);
    }
    let fehler = |e: tauri::Error| UiError::new("fenster", e.to_string());
    if window.is_maximized().map_err(fehler)?
        || window.is_fullscreen().map_err(fehler)?
        || window.is_minimized().map_err(fehler)?
        || !window.is_visible().map_err(fehler)?
    {
        return Ok(false);
    }
    let Some(monitor) = window.current_monitor().map_err(fehler)? else {
        return Ok(false);
    };
    let skala = window.scale_factor().map_err(fehler)?;
    let innen = window.inner_size().map_err(fehler)?;
    let aussen = window.outer_size().map_err(fehler)?;
    let lage = window.outer_position().map_err(fehler)?;
    let flaeche = monitor.work_area();
    let benoetigt = (benoetigt_hoehe * skala).ceil().min(u32::MAX as f64) as u32;

    let Some((neue_aussen_h, neue_y)) = neue_hoehe_und_lage(
        innen.height,
        aussen.height,
        benoetigt,
        lage.y,
        flaeche.position.y,
        flaeche.size.height,
    ) else {
        return Ok(false);
    };
    let deko = aussen.height.saturating_sub(innen.height);
    if neue_y != lage.y {
        window
            .set_position(PhysicalPosition::new(lage.x, neue_y))
            .map_err(fehler)?;
    }
    window
        .set_size(PhysicalSize::new(
            innen.width,
            neue_aussen_h.saturating_sub(deko),
        ))
        .map_err(fehler)?;
    tracing::info!(
        von = aussen.height,
        auf = neue_aussen_h,
        "Hauptfenster an den Cockpit-Inhalt angepasst"
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{neue_hoehe_und_lage, Schritt, Sicherung, SICHERN_NACH};
    use std::time::{Duration, Instant};

    fn frei() -> Sicherung {
        Sicherung {
            frist: None,
            faden_laeuft: false,
        }
    }

    #[test]
    fn beim_ziehen_startet_nur_ein_faden() {
        let mut z = frei();
        let t = Instant::now();
        assert!(z.ereignis(t), "erstes Ereignis startet den Faden");
        for i in 1..500 {
            assert!(!z.ereignis(t + Duration::from_millis(i)));
        }
    }

    #[test]
    fn gleitende_frist_sichert_erst_nach_dem_letzten_ereignis() {
        let mut z = frei();
        let t = Instant::now();
        z.ereignis(t);
        z.ereignis(t + Duration::from_millis(1000));
        assert_eq!(
            z.schritt(t + Duration::from_millis(1600)),
            Schritt::Warten(Duration::from_millis(900))
        );
        assert_eq!(
            z.schritt(t + Duration::from_millis(1000) + SICHERN_NACH),
            Schritt::Sichern
        );
    }

    #[test]
    fn ereignis_waehrend_des_sicherns_startet_keinen_zweiten_faden() {
        let mut z = frei();
        let t = Instant::now();
        z.ereignis(t);
        assert_eq!(z.schritt(t + SICHERN_NACH), Schritt::Sichern);
        // Faden speichert gerade — ein neues Ereignis darf nicht starten:
        assert!(!z.ereignis(t + SICHERN_NACH));
        // ... derselbe Faden macht weiter statt sich zu beenden:
        assert!(z.nach_dem_sichern());
        assert!(matches!(z.schritt(t + SICHERN_NACH), Schritt::Warten(_)));
    }

    #[test]
    fn ohne_neues_ereignis_gibt_der_faden_frei() {
        let mut z = frei();
        let t = Instant::now();
        z.ereignis(t);
        assert_eq!(z.schritt(t + SICHERN_NACH), Schritt::Sichern);
        assert!(!z.nach_dem_sichern());
        assert!(
            z.ereignis(t + SICHERN_NACH * 2),
            "danach startet wieder ein Faden"
        );
    }

    // Fenster 800 hoch (innen 770, 30 Titelleiste), oben bei 100,
    // Arbeitsfläche 0..1400.

    #[test]
    fn waechst_auf_den_bedarf_des_inhalts() {
        assert_eq!(
            neue_hoehe_und_lage(770, 800, 1000, 100, 0, 1400),
            Some((1030, 100))
        );
    }

    #[test]
    fn verkleinert_nie_und_ignoriert_rundungspixel() {
        assert_eq!(neue_hoehe_und_lage(770, 800, 600, 100, 0, 1400), None);
        assert_eq!(neue_hoehe_und_lage(770, 800, 772, 100, 0, 1400), None);
    }

    #[test]
    fn hoechstens_bis_zur_arbeitsflaeche() {
        assert_eq!(
            neue_hoehe_und_lage(770, 800, 3000, 100, 0, 1400),
            Some((1400, 0))
        );
    }

    #[test]
    fn rutscht_nach_oben_statt_unter_die_taskleiste() {
        // 1100 außen ab y=500 reichte bis 1600 > 1400 → nach oben auf 300.
        assert_eq!(
            neue_hoehe_und_lage(770, 800, 1070, 500, 0, 1400),
            Some((1100, 300))
        );
    }

    #[test]
    fn beruecksichtigt_eine_versetzte_arbeitsflaeche() {
        // Zweiter Monitor über dem ersten: Fläche -1080..0.
        assert_eq!(
            neue_hoehe_und_lage(770, 800, 1500, -900, -1080, 1040),
            Some((1040, -1080))
        );
    }
}
