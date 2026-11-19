//! Happy Eyeballs (RFC 8305) im MQTT-Aufbau, Patch in `vendor/rumqttc`.
//!
//! Anlass (19.11.2026, Umzug von live.kant.ovh): Mit AAAA-Eintrag und
//! verschlucktem IPv6 kam der Client nie an, jeder Versuch endete nach
//! 5 s (`connection_timeout`), IPv4 wurde nie probiert. Die Versuche sind
//! hier nachgebildet, damit „verschluckt“ (Paket verschwindet, Versuch hängt)
//! und „abgewiesen“ (sofortiger Fehler) ohne Root-Rechte und auf jeder
//! Plattform gleich ablaufen. Die Tokio-Uhr steht (`start_paused`), Zeiten
//! sind damit exakt und hängen nicht von der Last des Rechners ab.
//!
//! Die Verdrahtung in `socket_connect` sichern `socket_connect_nutzt_*`
//! (Quelltext) und `mqtt_aufbau_ueber_rumqttc_eventloop` (echter Durchlauf);
//! dass sie im Netz wirkt, belegt die Nachstellung auf einem Linux-Server
//! (Commit-Text).

use rumqttc::happy_eyeballs::{reihenfolge, verbinden, VERSUCHS_ABSTAND};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

/// rumqttc-Vorgabe für `connection_timeout` (NetworkOptions::new: 5 s).
const RUMQTTC_TIMEOUT: Duration = Duration::from_secs(5);

fn v6() -> SocketAddr {
    "[2001:db8::1]:443".parse().unwrap()
}
fn v4() -> SocketAddr {
    "192.0.2.1:443".parse().unwrap()
}

#[derive(Clone, Copy)]
enum Netz {
    /// Verbindung steht sofort.
    Geht,
    /// Paket wird verschluckt (DROP): der Versuch hängt.
    Verschluckt,
    /// Paket wird abgewiesen (REJECT): sofortiger Fehler.
    Abgewiesen,
}

type Versuch = Pin<Box<dyn Future<Output = io::Result<SocketAddr>> + Send>>;

/// Baut die Versuchsfunktion: merkt sich jeden gestarteten Versuch mit
/// Startzeit und verhält sich je Familie wie das angegebene Netz.
fn netz(
    ipv6: Netz,
    ipv4: Netz,
    beginn: Instant,
    gestartet: Arc<Mutex<Vec<(SocketAddr, Duration)>>>,
) -> impl FnMut(SocketAddr) -> Versuch {
    move |adresse| {
        gestartet.lock().unwrap().push((adresse, beginn.elapsed()));
        let verhalten = if adresse.is_ipv6() { ipv6 } else { ipv4 };
        Box::pin(async move {
            match verhalten {
                Netz::Geht => Ok(adresse),
                Netz::Abgewiesen => Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    format!("abgewiesen: {adresse}"),
                )),
                Netz::Verschluckt => std::future::pending().await,
            }
        })
    }
}

struct Ablauf {
    ergebnis: Result<io::Result<SocketAddr>, tokio::time::error::Elapsed>,
    dauer: Duration,
    /// Gestartete Versuche mit Startzeitpunkt relativ zum Beginn.
    gestartet: Vec<(SocketAddr, Duration)>,
}

impl Ablauf {
    fn adressen(&self) -> Vec<SocketAddr> {
        self.gestartet.iter().map(|(a, _)| *a).collect()
    }
}

/// Wie rumqttc: der ganze Aufbau läuft unter `connection_timeout`.
async fn aufbau(adressen: Vec<SocketAddr>, ipv6: Netz, ipv4: Netz) -> Ablauf {
    let gestartet = Arc::new(Mutex::new(Vec::new()));
    let beginn = Instant::now();
    let ergebnis = tokio::time::timeout(
        RUMQTTC_TIMEOUT,
        verbinden(
            adressen,
            VERSUCHS_ABSTAND,
            netz(ipv6, ipv4, beginn, gestartet.clone()),
        ),
    )
    .await;
    let dauer = beginn.elapsed();
    let gestartet = gestartet.lock().unwrap().clone();
    Ablauf {
        ergebnis,
        dauer,
        gestartet,
    }
}

/// Fall 2 (Pflicht): AAAA vorhanden, IPv6 verschluckt → über IPv4 in < 1 s.
/// Vor dem Patch: nie verbunden, Timeout nach 5 s.
#[tokio::test(start_paused = true)]
async fn ipv6_verschluckt_ipv4_kommt_unter_einer_sekunde_an() {
    let a = aufbau(vec![v6(), v4()], Netz::Verschluckt, Netz::Geht).await;
    let adresse = a
        .ergebnis
        .expect("Aufbau lief in den rumqttc-Timeout")
        .expect("Aufbau gescheitert");
    assert_eq!(adresse, v4(), "IPv4 muss gewinnen");
    assert!(a.dauer < Duration::from_secs(1), "zu langsam: {:?}", a.dauer);
    assert_eq!(
        a.gestartet,
        vec![(v6(), Duration::ZERO), (v4(), VERSUCHS_ABSTAND)],
        "IPv6 sofort, IPv4 nach dem Abstand"
    );
}

/// Fall 1: nur IPv4 (heutiger Stand ohne AAAA) → sofort, ein Versuch.
#[tokio::test(start_paused = true)]
async fn nur_ipv4_verbindet_sofort() {
    let a = aufbau(vec![v4()], Netz::Verschluckt, Netz::Geht).await;
    assert_eq!(a.ergebnis.unwrap().unwrap(), v4());
    assert_eq!(a.dauer, Duration::ZERO, "kein Warten");
    assert_eq!(a.gestartet, vec![(v4(), Duration::ZERO)]);
}

/// Fall 3: IPv6 abgewiesen → IPv4 sofort, ohne den Abstand abzuwarten.
#[tokio::test(start_paused = true)]
async fn ipv6_abgewiesen_ipv4_sofort() {
    let a = aufbau(vec![v6(), v4()], Netz::Abgewiesen, Netz::Geht).await;
    assert_eq!(a.ergebnis.unwrap().unwrap(), v4());
    assert_eq!(a.dauer, Duration::ZERO, "Fehler startet sofort den nächsten");
    assert_eq!(
        a.gestartet,
        vec![(v6(), Duration::ZERO), (v4(), Duration::ZERO)]
    );
}

/// Gegenprobe: funktionierendes IPv6 wird genommen, IPv4 gar nicht erst probiert.
#[tokio::test(start_paused = true)]
async fn funktionierendes_ipv6_wird_genommen() {
    let a = aufbau(vec![v6(), v4()], Netz::Geht, Netz::Geht).await;
    assert_eq!(*a.ergebnis.as_ref().unwrap().as_ref().unwrap(), v6());
    assert_eq!(a.dauer, Duration::ZERO);
    assert_eq!(a.adressen(), vec![v6()], "IPv4 darf nicht starten");
}

/// Gegenprobe: beide verschluckt → Fehler durch den rumqttc-Timeout (nicht
/// später, kein eigener längerer Timer); beide Familien wurden versucht.
#[tokio::test(start_paused = true)]
async fn beide_verschluckt_enden_im_timeout() {
    let a = aufbau(vec![v6(), v4()], Netz::Verschluckt, Netz::Verschluckt).await;
    assert!(a.ergebnis.is_err(), "muss im Timeout enden");
    assert_eq!(a.dauer, RUMQTTC_TIMEOUT);
    assert_eq!(a.adressen(), vec![v6(), v4()]);
}

/// Gegenprobe: beide abgewiesen → sofort der letzte Fehler, kein Hängen.
#[tokio::test(start_paused = true)]
async fn beide_abgewiesen_melden_sofort_fehler() {
    let a = aufbau(vec![v6(), v4()], Netz::Abgewiesen, Netz::Abgewiesen).await;
    let fehler = a
        .ergebnis
        .as_ref()
        .expect("darf nicht bis zum Timeout hängen")
        .as_ref()
        .expect_err("muss scheitern");
    assert_eq!(fehler.kind(), io::ErrorKind::ConnectionRefused);
    assert!(
        fehler.to_string().contains("192.0.2.1"),
        "letzter Fehler: {fehler}"
    );
    assert_eq!(a.dauer, Duration::ZERO);
    assert_eq!(a.adressen(), vec![v6(), v4()]);
}

/// Gegenprobe: keine Adresse → derselbe Fehler wie in rumqttc 0.25.1.
#[tokio::test(start_paused = true)]
async fn keine_adresse_meldet_fehler() {
    let a = aufbau(vec![], Netz::Geht, Netz::Geht).await;
    let fehler = a.ergebnis.unwrap().expect_err("ohne Adresse kein Erfolg");
    assert_eq!(fehler.kind(), io::ErrorKind::InvalidInput);
    assert!(a.gestartet.is_empty());
}

/// Der verlorene Versuch wird abgebrochen (verworfen), sobald einer gewinnt.
#[tokio::test(start_paused = true)]
async fn verlierer_wird_abgebrochen() {
    struct Merker(Arc<AtomicBool>);
    impl Drop for Merker {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let verworfen = Arc::new(AtomicBool::new(false));
    let merker = verworfen.clone();
    let ergebnis = tokio::time::timeout(
        RUMQTTC_TIMEOUT,
        verbinden(vec![v6(), v4()], VERSUCHS_ABSTAND, move |adresse| {
            let merker = Merker(merker.clone());
            async move {
                if adresse.is_ipv6() {
                    let _halten = merker;
                    std::future::pending::<io::Result<SocketAddr>>().await
                } else {
                    Ok(adresse)
                }
            }
        }),
    )
    .await;
    assert_eq!(ergebnis.unwrap().unwrap(), v4());
    assert!(
        verworfen.load(Ordering::SeqCst),
        "IPv6-Versuch muss verworfen sein"
    );
}

/// Mehrere Adressen: Familien wechseln sich ab, die erste Familie des
/// Resolvers (Systemvorrang nach RFC 6724) geht vor.
#[test]
fn reihenfolge_wechselt_familien_ab() {
    let a6: SocketAddr = "[2001:db8::2]:443".parse().unwrap();
    let a4: SocketAddr = "192.0.2.2:443".parse().unwrap();
    assert_eq!(
        reihenfolge(vec![v6(), a6, v4(), a4]),
        vec![v6(), v4(), a6, a4]
    );
    assert_eq!(reihenfolge(vec![v4(), a4, v6()]), vec![v4(), v6(), a4]);
    assert_eq!(reihenfolge(vec![v4()]), vec![v4()]);
}

/// Rumpf von `socket_connect` aus der gepatchten Kopie.
fn socket_connect_rumpf() -> &'static str {
    let quelle = include_str!("../../../vendor/rumqttc/src/eventloop.rs");
    let start = quelle
        .find("pub(crate) async fn socket_connect(")
        .expect("socket_connect fehlt in vendor/rumqttc/src/eventloop.rs");
    let rest = &quelle[start..];
    let ende = rest.find("\n}\n").expect("Ende von socket_connect");
    &rest[..ende]
}

/// Verdrahtung: rumqttc baut den TCP-Strom für Tcp/Tls/Ws/Wss (und v5)
/// über `socket_connect`. Fällt der Patch dort weg, kommt die alte Schleife
/// zurück und Fall 2 wäre im Client wieder kaputt, obwohl die Tests von
/// `verbinden` grün blieben.
#[test]
fn socket_connect_nutzt_happy_eyeballs() {
    let rumpf = socket_connect_rumpf();
    assert!(
        rumpf.contains("happy_eyeballs::verbinden("),
        "socket_connect ruft happy_eyeballs::verbinden nicht auf:\n{rumpf}"
    );
    assert!(
        rumpf.contains("VERSUCHS_ABSTAND"),
        "socket_connect nutzt den RFC-Abstand nicht:\n{rumpf}"
    );
    assert!(
        !rumpf.contains("for addr in"),
        "socket_connect probiert wieder nacheinander:\n{rumpf}"
    );
}

/// Durchlauf durch die echte rumqttc-EventLoop (gepatchtes `socket_connect`
/// mit Namensauflösung, Socket-Optionen, MQTT-CONNECT): ein Mini-Broker auf
/// 127.0.0.1 antwortet mit CONNACK. „localhost“ liefert je nach System ::1
/// und 127.0.0.1; ::1 wird dann sofort abgewiesen.
#[tokio::test]
async fn mqtt_aufbau_ueber_rumqttc_eventloop() {
    use rumqttc::{ConnectReturnCode, Event, Incoming, MqttOptions};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let broker = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = broker.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut strom, _) = broker.accept().await.unwrap();
        let mut puffer = [0u8; 256];
        let _ = strom.read(&mut puffer).await;
        // CONNACK, Sitzung neu, angenommen
        strom.write_all(&[0x20, 0x02, 0x00, 0x00]).await.unwrap();
        let _ = strom.read(&mut puffer).await;
    });

    let mut opts = MqttOptions::new("he-test", "localhost", port);
    opts.set_keep_alive(Duration::from_secs(60));
    let mut el = rumqttc::EventLoop::new(opts, 10);
    let ereignis = tokio::time::timeout(RUMQTTC_TIMEOUT, el.poll())
        .await
        .expect("Timeout")
        .expect("Aufbau gescheitert");
    match ereignis {
        Event::Incoming(Incoming::ConnAck(c)) => assert_eq!(c.code, ConnectReturnCode::Success),
        anderes => panic!("erwartet CONNACK, kam {anderes:?}"),
    }
}

/// Verdrahtung mit echten Sockets über Loopback: verschlossener IPv6-Port
/// (abgewiesen) und offener IPv4-Port → IPv4. Läuft nur, wenn ::1 da ist.
#[tokio::test]
async fn echte_sockets_loopback() {
    let ipv4 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = ipv4.local_addr().unwrap().port();
    let Ok(v6_frei) = std::net::TcpListener::bind("[::1]:0") else {
        eprintln!("kein ::1 auf diesem Rechner, Test übersprungen");
        return;
    };
    // Port belegen und wieder freigeben → dort lauscht niemand.
    let tot: SocketAddr = v6_frei.local_addr().unwrap();
    drop(v6_frei);
    let ziel4: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let strom = tokio::time::timeout(
        RUMQTTC_TIMEOUT,
        verbinden(
            vec![tot, ziel4],
            VERSUCHS_ABSTAND,
            tokio::net::TcpStream::connect::<SocketAddr>,
        ),
    )
    .await
    .expect("Timeout")
    .expect("Aufbau gescheitert");
    assert_eq!(strom.peer_addr().unwrap(), ziel4);
}
