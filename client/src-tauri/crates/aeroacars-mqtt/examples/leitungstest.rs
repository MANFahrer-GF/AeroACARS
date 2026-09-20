//! Gegenprobe zum Verbindungsabriss im Minutentakt (20.09.2026).
//!
//! Baut EINE Verbindung mit genau den Optionen des Clients auf (wss,
//! keep_alive 60 s, clean_session) und schreibt jedes Ereignis mit
//! Zeitstempel. Läuft, bis die Zeit um ist.
//!
//! Aufruf:
//!   URL=wss://host/mqtt U=name P=passwort SEKUNDEN=200 \
//!     cargo run -p aeroacars-mqtt --example leitungstest
use rumqttc::{AsyncClient, LastWill, MqttOptions, QoS, TlsConfiguration, Transport};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() {
    let url = std::env::var("URL").expect("URL");
    let u = std::env::var("U").expect("U");
    let p = std::env::var("P").expect("P");
    let sekunden: u64 = std::env::var("SEKUNDEN")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let takt: u64 = std::env::var("TAKT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    // Genau wie der Client: bei ws/wss ist broker_addr die VOLLE URL.
    let parsed = url::Url::parse(&url).expect("URL");
    let port = parsed.port_or_known_default().unwrap_or(443);
    let id = format!("leitungstest-{}", chrono::Utc::now().timestamp_millis());

    let mut opts = MqttOptions::new(&id, url.clone(), port);
    opts.set_credentials(&u, &p);
    opts.set_keep_alive(Duration::from_secs(60));
    opts.set_clean_session(true);
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    opts.set_transport(Transport::Wss(TlsConfiguration::Rustls(Arc::new(tls))));
    // Wie der echte Client: letzter Wille auf dem Status-Thema.
    if std::env::var("WIE_CLIENT").is_ok() {
        opts.set_last_will(LastWill::new(
            "aeroacars/test/status",
            "offline",
            QoS::AtLeastOnce,
            true,
        ));
    }

    let (client, mut eventloop) = AsyncClient::new(opts, 64);
    if std::env::var("WIE_CLIENT").is_ok() {
        // Vier Abonnements wie der Client (Integritaet, Chat, je zweifach
        // durch die Nachzieh-Schleife).
        for t in ["aeroacars/test/integrity_flag", "aeroacars/test/chat_in"] {
            let _ = client.try_subscribe(t, QoS::AtLeastOnce);
        }
    }
    let start = Instant::now();
    let seit = move || format!("{:6.1}s", start.elapsed().as_secs_f64());

    // Wie der Client: alle paar Sekunden eine Position, QoS 0.
    if takt > 0 {
        let c = client.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(takt)).await;
                let _ = c
                    .publish(
                        "aeroacars/test/leitung",
                        QoS::AtMostOnce,
                        true,
                        b"x".to_vec(),
                    )
                    .await;
            }
        });
    }

    let ende = tokio::time::sleep(Duration::from_secs(sekunden));
    tokio::pin!(ende);
    loop {
        tokio::select! {
            _ = &mut ende => { println!("{} Test vorbei", seit()); return; }
            ev = eventloop.poll() => match ev {
                Ok(rumqttc::Event::Incoming(i)) => println!("{} eingehend {i:?}", seit()),
                Ok(rumqttc::Event::Outgoing(o)) => println!("{} ausgehend {o:?}", seit()),
                Err(e) => {
                    println!("{} FEHLER {e:?}", seit());
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}
