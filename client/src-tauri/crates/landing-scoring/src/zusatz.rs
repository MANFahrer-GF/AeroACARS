//! V9 (Installer-Analyse 30.10.2026, Fähigkeit `zusatz-1`): das schemaoffene
//! Gefäß `zusatz` — ein JSON-OBJEKT, das Modul-Ausgaben unverändert in
//! Landungsdatensatz, MQTT-Touchdown und Flugprotokoll trägt, ohne dass der
//! Kern die Schlüssel kennen muss. Das Gefäß wird nie ausgeführt, nur
//! gespeichert, übertragen und angezeigt.
//!
//! Größengrenze: [`ZUSATZ_MAX_BYTES`], serialisiert gemessen. Begründung
//! (P-2.14): Der Broker verwirft Nachrichten über 64 KiB
//! (`MAX_PAKET_BYTES` in `aeroacars-mqtt`, mosquitto `max_packet_size`).
//! Ein Touchdown ist im Mittel 3,9 KB, im schlimmsten Fall 5,0 KB gross
//! (1022 Landungen, Messung 23.08.2026), die volle Bahnspur kommt mit 13 KB
//! und die Ausfahrten mit 0,6 KB dazu. 5 + 13 + 0,6 + 16 = rund 35 KB: das
//! Gefäss darf ein Viertel des Pakets belegen und lässt dem Rest mehr als
//! die Hälfte Luft. Mehr wäre ohne P-2.14-Neubewertung nicht vertretbar.
//! Überlauf = Feld weglassen (Aufrufer warnt im Log), nie Abbruch.

use serde_json::{Map, Value};

/// Obergrenze des serialisierten `zusatz`-Objekts in Bytes.
pub const ZUSATZ_MAX_BYTES: usize = 16 * 1024;

/// Prüft ein `zusatz`-Objekt gegen die Grenze.
///
/// - `None` oder leeres Objekt: `Ok(None)` (nichts zu tragen).
/// - Innerhalb der Grenze: `Ok(Some(objekt))`, unverändert.
/// - Darüber: `Err(groesse_bytes)`; der Aufrufer lässt das Feld weg und
///   schreibt eine Warnung ins Log.
pub fn begrenzen(zusatz: Option<Map<String, Value>>) -> Result<Option<Map<String, Value>>, usize> {
    let Some(map) = zusatz else { return Ok(None) };
    if map.is_empty() {
        return Ok(None);
    }
    let groesse = serde_json::to_vec(&map).map(|v| v.len()).unwrap_or(usize::MAX);
    if groesse > ZUSATZ_MAX_BYTES {
        Err(groesse)
    } else {
        Ok(Some(map))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn objekt(v: Value) -> Option<Map<String, Value>> {
        v.as_object().cloned()
    }

    #[test]
    fn kein_oder_leeres_objekt_traegt_nichts() {
        assert_eq!(begrenzen(None), Ok(None));
        assert_eq!(begrenzen(objekt(serde_json::json!({}))), Ok(None));
    }

    #[test]
    fn kleines_objekt_bleibt_unveraendert() {
        let z = objekt(serde_json::json!({"tch_extra": {"ft": 52.5, "ok": true}, "liste": [1, 2, 3]}));
        assert_eq!(begrenzen(z.clone()), Ok(z));
    }

    #[test]
    fn grenze_ist_scharf() {
        // `{"k":"<n mal a>"}` hat 8 Byte Rahmen ({"k":""} = 8 Zeichen).
        let genau = objekt(serde_json::json!({"k": "a".repeat(ZUSATZ_MAX_BYTES - 8)}));
        assert!(matches!(begrenzen(genau), Ok(Some(_))), "genau an der Grenze gilt noch");
        let drueber = objekt(serde_json::json!({"k": "a".repeat(ZUSATZ_MAX_BYTES - 7)}));
        assert_eq!(begrenzen(drueber), Err(ZUSATZ_MAX_BYTES + 1));
    }
}
