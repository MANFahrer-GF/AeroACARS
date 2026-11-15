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

/// So viele versuchte Aufrufe in Folge muessen beim Registrieren synchron
/// scheitern, bis die Verbindung als tot gilt.
///
/// Feldbefund 13.11.2026 (Thomas, 3.0.2, MSFS beendete sich gerade):
/// direkt nach `SimConnect_Open` scheiterte JEDES der 422
/// `AddToDataDefinition` mit `0x80004005` — 422 ERROR-Zeilen in 7 ms,
/// danach alle Folgeaufrufe ebenso. Eine synchrone Einzelablehnung ist aus
/// dem Feld nicht belegt; der Auslassweg (QS #147) nimmt dafuer ebenfalls
/// `E_FAIL` an, der Code allein entscheidet also nicht. Der Nachbar schon:
/// Ein einzeln abgelehntes Feld laesst den naechsten Aufruf durch, eine
/// tote Pipe nicht. Drei statt zwei, damit zwei benachbarte
/// echte Ablehnungen nie als Abriss gelten — sonst scheiterte jede
/// Neuverbindung an denselben Feldern (Dauerausfall).
pub const ABRISS_NACH_FEHLERN_IN_FOLGE: u32 = 3;

/// Zaehlt synchrone Fehlschlaege in Folge beim Registrieren.
#[derive(Debug, Default)]
pub struct Abrisswaechter {
    in_folge: u32,
}

impl Abrisswaechter {
    /// Ein Aufruf gelang: die Folge reisst ab.
    pub fn erfolg(&mut self) {
        self.in_folge = 0;
    }

    /// Ein Aufruf scheiterte. `true`: die Verbindung gilt als tot.
    pub fn fehler(&mut self) -> bool {
        self.in_folge += 1;
        self.in_folge >= ABRISS_NACH_FEHLERN_IN_FOLGE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abriss_erst_nach_drei_fehlern_in_folge() {
        let mut w = Abrisswaechter::default();
        assert!(!w.fehler());
        assert!(!w.fehler());
        assert!(w.fehler());
    }

    #[test]
    fn erfolg_dazwischen_setzt_die_folge_zurueck() {
        // Einzelne und paarweise Ablehnungen bleiben Auslassungen.
        let mut w = Abrisswaechter::default();
        for _ in 0..100 {
            assert!(!w.fehler());
            assert!(!w.fehler());
            w.erfolg();
        }
    }

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
