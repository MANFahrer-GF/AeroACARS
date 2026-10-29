//! Kabinenmodul fuer den WASM-Host (P-3.58): `alloc(len)` gibt Speicher
//! fuer die Eingabe, `kabine(ptr, len)` fuehrt einen
//! [`Befehl`](cabin_announcements::modul::Befehl) aus und liefert Zeiger
//! (hohe 32 Bit) und Laenge (niedrige 32 Bit) der Antwort. Der Ablauf steht
//! in `cabin-announcements` (`Planer`); hier nur Zustand und Exporte.
//!
//! Anders als das Bewertungsmodul lebt eine Instanz den ganzen Flug: Der
//! Planer liegt im Modulspeicher, die Antwort in einem wiederverwendeten
//! Puffer (sonst wuechse der Speicher mit jedem Takt).

use std::cell::RefCell;
use std::sync::OnceLock;

use cabin_announcements::modul::Befehl;
use cabin_announcements::{Planer, Tabelle};
use serde_json::Value;

thread_local! {
    static PLANER: RefCell<Option<Planer>> = const { RefCell::new(None) };
    static AUSGABE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Die Tabelle dieser Instanz (eine je Flug, kommt mit dem ersten Befehl).
static TABELLE: OnceLock<Tabelle> = OnceLock::new();

fn tabelle_setzen(json: &str) -> Result<(), String> {
    TABELLE.set(cabin_announcements::lesen(json)?).map_err(|_| "Tabelle schon gesetzt".to_string())?;
    cabin_announcements::quelle_setzen(|| TABELLE.get().expect("Tabelle gesetzt"));
    Ok(())
}

fn ausfuehren(b: Befehl) -> Result<Value, String> {
    if b.legt_an() {
        let tabelle = match &b {
            Befehl::Neu { tabelle, .. } | Befehl::Fortsetzen { tabelle, .. } | Befehl::Laden { tabelle, .. } => tabelle.clone(),
            _ => unreachable!(),
        };
        let p = cabin_announcements::modul::erzeugen(b)?;
        tabelle_setzen(&tabelle)?;
        PLANER.with_borrow_mut(|z| *z = Some(p));
        return Ok(Value::Null);
    }
    PLANER.with_borrow_mut(|z| cabin_announcements::modul::anwenden(z.as_mut().ok_or("kein Planer")?, b))
}

/// Speicher fuer die Eingabe anfordern (der Host schreibt hinein).
#[no_mangle]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len as usize);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Einen Befehl ausfuehren. Die Antwort gilt bis zum naechsten Aufruf.
///
/// # Safety
/// `ptr` muss von `alloc(len)` stammen und `len` beschriebene Bytes haben;
/// der Speicher geht in den Besitz des Moduls ueber.
#[no_mangle]
pub unsafe extern "C" fn kabine(ptr: *mut u8, len: u32) -> u64 {
    // SAFETY: siehe oben — Zeiger und Laenge stammen aus `alloc`.
    let eingabe = unsafe { Vec::from_raw_parts(ptr, len as usize, len as usize) };
    let ergebnis = serde_json::from_slice::<Befehl>(&eingabe)
        .map_err(|e| format!("Befehl: {e}"))
        .and_then(ausfuehren);
    AUSGABE.with_borrow_mut(|puffer| {
        puffer.clear();
        serde_json::to_writer(&mut *puffer, &ergebnis).expect("Antwort schreibbar");
        ((puffer.as_ptr() as usize as u64) << 32) | puffer.len() as u64
    })
}
