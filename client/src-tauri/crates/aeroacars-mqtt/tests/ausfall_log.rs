//! Die echte Treiberschleife gegen einen toten Broker: Das Log nennt den
//! Ausfall einmal und schreibt nicht je Versuch neue Zeilen (Health-Report
//! 28.11.2026: 4971× dieselbe Warnung in 422 Minuten).
//!
//! Echte Uhr und echter Socket — gut 11 s, damit drei Verbindungsversuche
//! (Takt 5 s) hineinpassen.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
struct Mitschnitt(Arc<Mutex<Vec<u8>>>);

impl Write for Mitschnitt {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Log der Ein-Faden-Laufzeit mitschneiden (thread-lokal, gilt damit auch fuer
/// die Aufgaben der Treiberschleife).
fn mitschneiden() -> (Mitschnitt, tracing::subscriber::DefaultGuard) {
    let log = Mitschnitt::default();
    let ziel = log.clone();
    let wache = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_env_filter("aeroacars_mqtt=debug")
            .with_ansi(false)
            .with_writer(move || ziel.clone())
            .finish(),
    );
    (log, wache)
}

fn verbinden(port: u16) -> aeroacars_mqtt::Handle {
    aeroacars_mqtt::start(aeroacars_mqtt::MqttConfig {
        broker_url: format!("mqtt://127.0.0.1:{port}"),
        username: "u".into(),
        password: "p".into(),
        va_prefix: "gsg".into(),
        pilot_id: "1".into(),
    })
    .unwrap()
}

impl Mitschnitt {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

#[tokio::test]
async fn toter_broker_flutet_das_log_nicht() {
    let (log, _wache) = mitschneiden();
    // Port belegen und wieder freigeben: dort lauscht niemand.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = verbinden(port);

    tokio::time::sleep(Duration::from_millis(11_500)).await;
    drop(handle);

    let text = log.text();
    let zaehle = |s: &str| text.lines().filter(|z| z.contains(s)).count();
    assert_eq!(zaehle("MQTT poll error"), 1, "{text}");
    assert_eq!(zaehle("Abo auf"), 0, "{text}");
    assert_eq!(zaehle("Rueckkanal abonniert"), 0, "{text}");
}

#[tokio::test]
async fn rueckkehr_wird_gemeldet_und_abonniert() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (log, _wache) = mitschneiden();
    let broker = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = broker.local_addr().unwrap().port();
    tokio::spawn(async move {
        // Erster Versuch: sofort zu. Zweiter: CONNACK und offen halten.
        drop(broker.accept().await.unwrap());
        let (mut leitung, _) = broker.accept().await.unwrap();
        let mut puffer = [0u8; 4096];
        assert!(leitung.read(&mut puffer).await.unwrap() > 0);
        leitung.write_all(&[0x20, 0x02, 0x00, 0x00]).await.unwrap();
        while leitung.read(&mut puffer).await.is_ok_and(|n| n > 0) {}
    });
    let handle = verbinden(port);

    tokio::time::sleep(Duration::from_millis(6_500)).await;
    let text = log.text();
    drop(handle);

    let zaehle = |s: &str| text.lines().filter(|z| z.contains(s)).count();
    assert_eq!(zaehle("MQTT poll error"), 1, "{text}");
    assert_eq!(zaehle("MQTT wieder verbunden"), 1, "{text}");
    assert_eq!(zaehle("Rueckkanal abonniert"), 2, "{text}");
}
