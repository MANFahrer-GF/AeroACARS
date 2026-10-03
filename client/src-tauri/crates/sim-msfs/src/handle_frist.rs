//! Frist fuer einen frisch geoeffneten SimConnect-Handle ohne Lebenszeichen.
//! Plattformunabhaengig, damit die Grenzen ohne Simulator testbar sind.

use std::time::Duration;

/// Wie lange ein frisch geoeffneter Handle ohne Lebenszeichen warten darf.
///
/// Feldbefund 03.10.2026 (Thomas, „Automatisch“ blieb gelb): Der Adapter
/// lief seit dem Morgen, MSFS wurde neu gestartet, der Client verband nie
/// wieder. Im Log: 10:54:27 `SimConnect_Open succeeded`, danach weder
/// `RECV_OPEN` noch ein Snapshot — ein Handle, der nie ein Lebenszeichen
/// gab. Der Stale-Waechter in `run_dispatch` greift aber erst NACH dem ersten Snapshot,
/// und ein toter Pipe-Handle meldet `E_FAIL`, was wir als „Warteschlange
/// leer“ lesen. So blieb er ewig offen.
///
/// Zwei Stufen, weil ein ladender MSFS wirklich lange braucht (im selben
/// Log bis 52 s von Open bis zum ersten Snapshot):
/// * **Kein Handshake** (`RECV_OPEN` fehlt): nach 120 s ohne jede Nachricht
///   neu oeffnen. Ein lebendes MSFS beantwortet den Handshake.
/// * **Handshake da, aber keine Daten** (Hauptmenue, Flug laedt): der Handle
///   lebt; nur ein Sicherheitsnetz nach 10 min ohne jede Nachricht, falls
///   MSFS dazwischen abgestuerzt ist. Jede Nachricht setzt die Uhr zurueck.
pub const FRIST_OHNE_HANDSHAKE: Duration = Duration::from_secs(120);
pub const FRIST_OHNE_DATEN_NACH_HANDSHAKE: Duration = Duration::from_secs(600);

/// Hat ein Handle, der noch nie Daten lieferte, seine Frist ueberschritten?
/// `seit_lebenszeichen`: Zeit seit Open bzw. seit der letzten Nachricht
/// irgendeiner Art, die der Handle lieferte.
pub fn erste_daten_ueberfaellig(handshake_gesehen: bool, seit_lebenszeichen: Duration) -> bool {
    let frist = if handshake_gesehen {
        FRIST_OHNE_DATEN_NACH_HANDSHAKE
    } else {
        FRIST_OHNE_HANDSHAKE
    };
    seit_lebenszeichen > frist
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn ohne_handshake_erst_nach_120_s() {
        assert!(!erste_daten_ueberfaellig(false, S(0)));
        // Ladender MSFS: im Feldlog bis 52 s von Open bis zum ersten Snapshot.
        assert!(!erste_daten_ueberfaellig(false, S(52)));
        assert!(!erste_daten_ueberfaellig(false, S(120)));
        assert!(erste_daten_ueberfaellig(false, S(121)));
    }

    #[test]
    fn mit_handshake_lebt_der_handle_bis_zum_sicherheitsnetz() {
        // Hauptmenue: Handshake da, keine Daten — kein Neuaufbau nach 121 s.
        assert!(!erste_daten_ueberfaellig(true, S(121)));
        assert!(!erste_daten_ueberfaellig(true, S(600)));
        assert!(erste_daten_ueberfaellig(true, S(601)));
    }

    #[test]
    fn handshake_verlaengert_die_frist_nie_nach_unten() {
        for s in 0..1000 {
            if erste_daten_ueberfaellig(true, S(s)) {
                assert!(erste_daten_ueberfaellig(false, S(s)));
            }
        }
    }
}
