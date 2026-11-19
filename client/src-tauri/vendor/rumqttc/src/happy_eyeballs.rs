//! AeroACARS-Patch (nicht Teil von rumqttc 0.25.1): Happy Eyeballs v2
//! nach RFC 8305 für den TCP-Aufbau zum Broker.
//!
//! rumqttc 0.25.1 probiert die Adressen aus `lookup_host` streng
//! nacheinander. Hat der Name einen AAAA-Eintrag und verschluckt das Netz
//! des Piloten IPv6 (Pakete verschwinden ohne Antwort), hängt der erste
//! Versuch bis zum `connection_timeout` von rumqttc fest und IPv4 kommt
//! nie an die Reihe. Hier startet der nächste Versuch spätestens nach
//! `VERSUCHS_ABSTAND` parallel; die erste Verbindung gewinnt, die übrigen
//! Versuche werden verworfen (ihre Sockets schließen beim Drop).
//!
//! Einzelheiten und Ausbau: `AEROACARS-PATCH.md` im Crate-Ordner.

use futures_util::stream::{FuturesUnordered, StreamExt};
use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::{sleep, Instant};

/// Abstand, nach dem der nächste Versuch parallel startet
/// (RFC 8305 Abschnitt 5 empfiehlt 250 ms).
pub const VERSUCHS_ABSTAND: Duration = Duration::from_millis(250);

/// Reihenfolge der Versuche nach RFC 8305 Abschnitt 4: Die Familie der
/// ersten Adresse vom Resolver geht vor (das Betriebssystem stellt IPv6
/// dort nach RFC 6724 nach vorn, wenn es IPv6 für nutzbar hält), danach
/// wechseln sich die Familien ab.
pub fn reihenfolge(adressen: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let Some(erste) = adressen.first() else {
        return adressen;
    };
    let erste_ist_v6 = erste.is_ipv6();
    let (mut vorn, mut danach): (VecDeque<_>, VecDeque<_>) = adressen
        .into_iter()
        .partition(|a| a.is_ipv6() == erste_ist_v6);
    let mut ergebnis = Vec::with_capacity(vorn.len() + danach.len());
    loop {
        match (vorn.pop_front(), danach.pop_front()) {
            (None, None) => break,
            (a, b) => ergebnis.extend(a.into_iter().chain(b)),
        }
    }
    ergebnis
}

/// Verbindet nach Happy Eyeballs: erster Versuch sofort, jeder weitere
/// nach `abstand` oder sofort, wenn ein laufender Versuch scheitert. Das
/// erste `Ok` gewinnt. Scheitern alle, kommt der letzte Fehler zurück.
/// Eine Gesamtgrenze setzt der Aufrufer (bei rumqttc `connection_timeout`).
pub async fn verbinden<T, F, Fut>(
    adressen: Vec<SocketAddr>,
    abstand: Duration,
    mut versuch: F,
) -> io::Result<T>
where
    F: FnMut(SocketAddr) -> Fut,
    Fut: Future<Output = io::Result<T>>,
{
    let mut warteschlange = reihenfolge(adressen).into_iter();
    let mut laufend = FuturesUnordered::new();
    let mut letzter_fehler = None;

    match warteschlange.next() {
        Some(adresse) => laufend.push(versuch(adresse)),
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "could not resolve to any address",
            ))
        }
    }

    let takt = sleep(abstand);
    tokio::pin!(takt);

    loop {
        let noch_adressen = warteschlange.len() > 0;
        tokio::select! {
            Some(ergebnis) = laufend.next() => match ergebnis {
                Ok(verbindung) => return Ok(verbindung),
                Err(fehler) => {
                    letzter_fehler = Some(fehler);
                    if let Some(adresse) = warteschlange.next() {
                        laufend.push(versuch(adresse));
                        takt.as_mut().reset(Instant::now() + abstand);
                    } else if laufend.is_empty() {
                        break;
                    }
                }
            },
            _ = &mut takt, if noch_adressen => {
                if let Some(adresse) = warteschlange.next() {
                    laufend.push(versuch(adresse));
                }
                takt.as_mut().reset(Instant::now() + abstand);
            }
            else => break,
        }
    }

    Err(letzter_fehler.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "could not resolve to any address",
        )
    }))
}
