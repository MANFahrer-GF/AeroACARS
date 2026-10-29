//! Bewertungsmodul fuer den WASM-Host (P-4.30): `alloc(len)` gibt Speicher
//! fuer die Eingabe, `bewerte(ptr, len)` liefert Zeiger (hohe 32 Bit) und
//! Laenge (niedrige 32 Bit) der Ausgabe. Die Regeln stehen in
//! `landing-scoring` (`wasm::bewerten_json`); hier nur die Exporte.

/// Speicher fuer die Eingabe anfordern (der Host schreibt hinein).
#[no_mangle]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len as usize);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Bewerten.
///
/// # Safety
/// `ptr` muss von `alloc(len)` stammen und `len` beschriebene Bytes haben;
/// der Speicher geht in den Besitz des Moduls ueber.
#[no_mangle]
pub unsafe extern "C" fn bewerte(ptr: *mut u8, len: u32) -> u64 {
    // SAFETY: siehe oben — Zeiger und Laenge stammen aus `alloc`.
    let eingabe = unsafe { Vec::from_raw_parts(ptr, len as usize, len as usize) };
    let aus = landing_scoring::wasm::bewerten_json(std::str::from_utf8(&eingabe).unwrap_or(""));
    let mut bytes = aus.into_bytes().into_boxed_slice();
    let (p, n) = (bytes.as_mut_ptr() as usize as u64, bytes.len() as u64);
    std::mem::forget(bytes);
    (p << 32) | n
}
