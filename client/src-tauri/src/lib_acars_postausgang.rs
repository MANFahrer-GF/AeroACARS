/// Datei des ACARS-Postausgangs im App-Datenordner.
const ACARS_POSTAUSGANG: &str = "acars_queue.json";

/// Höchstens so viele Zeilen je Anfrage.
const ACARS_PAKET_MAX: usize = 50;

/// Obergrenze aller offenen Zeilen (alle PIREPs). Darüber fallen die ältesten
/// weg — mit Warnung. Zum Vergleich: ein Langstreckenflug erzeugt einige
/// hundert Zeilen.
const ACARS_POSTAUSGANG_MAX: usize = 20_000;

/// Eingang des ACARS-Postausgangs — gesetzt von `spawn_acars_postausgang`.
static ACARS_EINGANG: OnceLock<tokio::sync::mpsc::UnboundedSender<AcarsAuftrag>> = OnceLock::new();

enum AcarsAuftrag {
    Zeilen(String, Vec<api_client::LogEntry>),
    /// Alles bisher Eingereihte aufnehmen und sichern, dann melden, ob das
    /// gelungen ist (App-Ende). Kein Erfolg wird gemeldet, der keiner ist.
    Sichern(std::sync::mpsc::SyncSender<bool>),
}

/// Beim Beenden: warten, bis alle eingereihten ACARS-Zeilen in der Datei
/// stehen (höchstens 2 s). Codex-Abnahme 12.10.2026.
fn acars_abwarten_beim_beenden() {
    let Some(tx) = ACARS_EINGANG.get() else {
        return;
    };
    let (fertig_tx, fertig_rx) = std::sync::mpsc::sync_channel(1);
    if tx.send(AcarsAuftrag::Sichern(fertig_tx)).is_err() {
        tracing::error!("ACARS-Postausgang läuft nicht mehr — beim Beenden nicht gesichert");
        return;
    }
    match fertig_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(true) => {}
        Ok(false) => tracing::error!("ACARS-Postausgang beim Beenden nicht gesichert"),
        Err(_) => tracing::error!("ACARS-Postausgang: Sichern beim Beenden dauerte zu lange"),
    }
}

/// ACARS-Zeilen für das phpVMS-Fluglogbuch einreihen. Kehrt sofort zurück.
///
/// Jede Zeile bekommt hier ihre feste Kennung (UUID). phpVMS legt Zeilen mit
/// `id` per `updateOrInsert` an — Neuversuche erzeugen deshalb keine
/// Doppelten, auch wenn eine frühere Anfrage doch angekommen war.
fn acars_einreihen(pirep_id: &str, mut zeilen: Vec<api_client::LogEntry>) {
    for z in &mut zeilen {
        if z.id.is_none() {
            z.id = Some(uuid::Uuid::new_v4().to_string());
        }
        // NaN/∞ würde als null gesendet und von phpVMS (`numeric`) abgelehnt.
        z.lat = z.lat.filter(|v| v.is_finite());
        z.lon = z.lon.filter(|v| v.is_finite());
    }
    match ACARS_EINGANG.get() {
        Some(tx) => {
            if tx.send(AcarsAuftrag::Zeilen(pirep_id.to_string(), zeilen)).is_err() {
                tracing::error!(pirep_id, "ACARS-Postausgang läuft nicht mehr — Zeilen nicht eingereiht");
            }
        }
        None => tracing::warn!(
            pirep_id,
            "ACARS-Postausgang nicht gestartet — Zeilen nicht eingereiht"
        ),
    }
}

/// Den ACARS-Postausgang starten — einmal je Programmlauf.
///
/// # Warum ein einziger Besitzer (AeroACARS 2.0.10)
///
/// Bis 2.0.9 ging eine ACARS-Zeile beim ersten Fehler verloren, und das
/// Senden hielt den Streamer an. Jetzt besitzt genau eine Aufgabe alle offenen
/// Zeilen aller PIREPs und die Datei `acars_queue.json`: kein konkurrierendes
/// Schreiben, keine zwei Zusteller für denselben PIREP, und was beim Beenden
/// oder Absturz offen war, geht beim nächsten Start raus. phpVMS nimmt Zeilen
/// auch nach dem Einreichen an (nur abgebrochene PIREPs nicht) und sortiert
/// sie über `created_at` ein — das Einreichen wartet deshalb nicht.
///
/// Grenze: Eine Zeile, die in den Millisekunden zwischen Einreihen und
/// Sichern durch einen Absturz verloren geht, fehlt im phpVMS-Logbuch; im
/// Flugprotokoll (JSONL) und im Aktivitätsprotokoll steht sie trotzdem.
fn spawn_acars_postausgang(app: &AppHandle) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    if ACARS_EINGANG.set(tx).is_err() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(acars_postausgang_besitzer(app, rx));
}

type AcarsOffen =
    std::collections::BTreeMap<String, std::collections::VecDeque<api_client::LogEntry>>;

/// Ergebnis des Ladens der Ablage.
enum AcarsLaden {
    Ok(AcarsOffen),
    /// Datei gerade nicht lesbar (z. B. kurz gesperrt) — NICHT überschreiben,
    /// später erneut laden.
    Spaeter,
}

/// Datei lesen. Nur bei kaputtem Inhalt (JSON) wird sie beiseitegelegt
/// (`acars_queue.defekt-<zeit>.json`); ein Lesefehler (z. B. ein Virenscanner
/// sperrt sie kurz) lässt sie unangetastet.
fn acars_postausgang_laden(app: &AppHandle) -> AcarsLaden {
    let mut offen = AcarsOffen::new();
    let Ok(dir) = app.path().app_data_dir() else {
        return AcarsLaden::Spaeter;
    };
    let Ok(ablage) = storage::PositionQueue::open_named(&dir, ACARS_POSTAUSGANG) else {
        return AcarsLaden::Spaeter;
    };
    match ablage.read_all() {
        Ok(eintraege) => {
            for e in eintraege {
                match serde_json::from_value::<api_client::LogEntry>(e.position) {
                    Ok(mut z) => {
                        // Zeilen ohne Kennung bekommen jetzt eine — sonst wäre
                        // ihr Neuversuch nicht idempotent.
                        if z.id.is_none() {
                            z.id = Some(uuid::Uuid::new_v4().to_string());
                        }
                        offen.entry(e.pirep_id).or_default().push_back(z);
                    }
                    Err(err) => tracing::warn!(
                        error = %err,
                        "ACARS-Zeile in der Ablage unlesbar — übersprungen"
                    ),
                }
            }
            AcarsLaden::Ok(offen)
        }
        Err(storage::StorageError::Serde(err)) => {
            let ziel = dir.join(format!(
                "acars_queue.defekt-{}.json",
                Utc::now().timestamp()
            ));
            let _ = std::fs::rename(dir.join(ACARS_POSTAUSGANG), &ziel);
            tracing::error!(
                error = %err,
                ziel = ?ziel,
                "ACARS-Postausgang beschädigt — beiseitegelegt"
            );
            AcarsLaden::Ok(offen)
        }
        Err(err) => {
            tracing::warn!(error = %err, "ACARS-Postausgang gerade nicht lesbar — später erneut");
            AcarsLaden::Spaeter
        }
    }
}

fn acars_postausgang_schreiben(app: &AppHandle, offen: &AcarsOffen) -> bool {
    let Ok(dir) = app.path().app_data_dir() else {
        tracing::error!("ACARS-Postausgang: kein Datenordner");
        return false;
    };
    let Ok(ablage) = storage::PositionQueue::open_named(&dir, ACARS_POSTAUSGANG) else {
        tracing::error!("ACARS-Postausgang: Ablage nicht zu öffnen");
        return false;
    };
    let alle: Vec<storage::QueuedPosition> = offen
        .iter()
        .flat_map(|(pirep_id, zeilen)| {
            zeilen.iter().filter_map(move |z| {
                serde_json::to_value(z)
                    .ok()
                    .map(|v| storage::QueuedPosition {
                        pirep_id: pirep_id.clone(),
                        position: v,
                    })
            })
        })
        .collect();
    match ablage.replace(&alle) {
        Ok(()) => true,
        Err(e) => {
            tracing::error!(
                error = ?e,
                "ACARS-Postausgang nicht gesichert — nächster Durchlauf versucht es erneut"
            );
            false
        }
    }
}

/// Sichern; `false`, wenn es nicht geklappt hat (dann versucht der Besitzer
/// es bei jedem Durchlauf erneut — zugestellt wird trotzdem weiter, denn
/// streikt die Platte, ist Zustellen der beste Weg, die Zeilen zu retten).
///
/// Verdrängte Zeilen, die noch nicht in der Quarantäne stehen, kommen mit in
/// die Datei — sonst stünden sie bis zum nächsten Durchlauf nur im Speicher,
/// und ein Absturz dazwischen verlöre sie (Codex-Abnahme 481aef6b).
async fn acars_sichern(app: &AppHandle, offen: &AcarsOffen, verdraengt: &AcarsVerdraengt) -> bool {
    let (app, kopie) = (app.clone(), acars_fuer_datei(offen, verdraengt));
    tokio::task::spawn_blocking(move || acars_postausgang_schreiben(&app, &kopie))
        .await
        .unwrap_or(false)
}

/// Verdrängte Zeilen (PIREP, Zeile) — gehen in die Quarantäne, nie verloren.
type AcarsVerdraengt = Vec<(String, api_client::LogEntry)>;

/// Was in die Datei gehört: alle offenen Zeilen und davor (sie sind älter)
/// die verdrängten, die noch nicht in der Quarantäne stehen.
fn acars_fuer_datei(offen: &AcarsOffen, verdraengt: &AcarsVerdraengt) -> AcarsOffen {
    let mut alle = offen.clone();
    for (p, z) in verdraengt.iter().rev() {
        alle.entry(p.clone()).or_default().push_front(z.clone());
    }
    alle
}

fn acars_aufnehmen(
    offen: &mut AcarsOffen,
    pirep_id: String,
    zeilen: Vec<api_client::LogEntry>,
) -> AcarsVerdraengt {
    offen.entry(pirep_id).or_default().extend(zeilen);
    acars_begrenzen(offen)
}

/// Obergrenze aller offenen Zeilen durchsetzen — älteste zuerst.
fn acars_begrenzen(offen: &mut AcarsOffen) -> AcarsVerdraengt {
    let mut verdraengt = AcarsVerdraengt::new();
    let mut gesamt: usize = offen.values().map(|q| q.len()).sum();
    while gesamt > ACARS_POSTAUSGANG_MAX {
        // Älteste Zeile über alle PIREPs (nach created_at) verwerfen.
        let Some(pirep) = offen
            .iter()
            .filter_map(|(p, q)| q.front().map(|z| (p.clone(), z.created_at.clone())))
            .min_by(|a, b| a.1.cmp(&b.1))
            .map(|(p, _)| p)
        else {
            break;
        };
        if let Some(z) = offen.get_mut(&pirep).and_then(|q| q.pop_front()) {
            verdraengt.push((pirep.clone(), z));
        }
        gesamt -= 1;
        tracing::warn!(pirep_id = %pirep, "ACARS-Postausgang voll — älteste Zeile in die Quarantäne");
    }
    offen.retain(|_, q| !q.is_empty());
    verdraengt
}

/// Ist die phpVMS-Fehlerantwort (RFC 7807) ein `PirepCancelled`? Geprüft
/// wird nur das Feld `type` (`<error_root>/pirep-cancelled`), nicht irgendein
/// Textvorkommen.
fn ist_pirep_abgebrochen(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string))
        .is_some_and(|t| t == "pirep-cancelled" || t.ends_with("/pirep-cancelled"))
}

/// Nach so vielen Tagen werden Zeilen eines nicht (mehr) gefundenen PIREPs
/// verworfen, der nicht der aktive Flug ist.
const ACARS_ABLAUF_TAGE: i64 = 7;

fn acars_ist_aktiver_flug(app: &AppHandle, pirep_id: &str) -> bool {
    app.state::<AppState>()
        .active_flight
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|f| f.pirep_id == pirep_id))
        .unwrap_or(false)
}

/// Ist die jüngste offene Zeile des PIREPs älter als `tage`?
fn acars_aelter_als(offen: &AcarsOffen, pirep_id: &str, tage: i64) -> bool {
    let grenze = Utc::now() - chrono::Duration::days(tage);
    offen.get(pirep_id).is_some_and(|q| {
        q.iter().all(|z| {
            z.created_at
                .as_deref()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .is_some_and(|t| t.with_timezone(&Utc) < grenze)
        })
    })
}

/// Alle Zeilen eines PIREPs verwerfen — phpVMS nimmt für ihn nichts mehr an.
fn acars_pirep_verwerfen(app: &AppHandle, offen: &mut AcarsOffen, pirep_id: &str, grund: &ApiError) {
    tracing::warn!(
        pirep_id,
        error = %grund,
        verworfen = offen.get(pirep_id).map_or(0, |q| q.len()),
        "PIREP nimmt keine ACARS-Zeilen mehr an — Zeilen liegen in der Quarantäne"
    );
    if matches!(grund, ApiError::NotFound) {
        let aktiv = app
            .state::<AppState>()
            .active_flight
            .lock()
            .ok()
            .and_then(|g| g.as_ref().cloned());
        if let Some(flight) =
            aktiv.filter(|f| f.pirep_id == pirep_id && !f.stop.load(Ordering::Relaxed))
        {
            // Eine Panik im fremden Code (z. B. vergiftete Sperre im
            // Aktivitätsprotokoll) darf den einzigen Besitzer des
            // Postausgangs nicht beenden (Codex-Abnahme 481aef6b).
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                handle_remote_cancellation(app, &flight, "POST acars/logs")
            }))
            .is_err()
            {
                tracing::error!(pirep_id, "Abbruch-Behandlung aus dem ACARS-Postausgang ist abgestürzt");
            }
        }
    }
    offen.remove(pirep_id);
}

/// Ein Paket in eine eigene Quarantäne-Datei schreiben
/// (`acars_quarantaene-<pirep>-<zeit>.json`) — nichts wird überschrieben oder
/// neu geschrieben, keine Datei wächst. `true`, wenn sicher geschrieben.
fn acars_quarantaene(app: &AppHandle, pirep_id: &str, paket: &[api_client::LogEntry]) -> bool {
    let Ok(dir) = app.path().app_data_dir() else {
        tracing::error!(pirep_id, "ACARS-Quarantäne: kein Datenordner");
        return false;
    };
    let datei = format!(
        "acars_quarantaene-{}-{}.json",
        pirep_id.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>(),
        uuid::Uuid::new_v4().simple()
    );
    let Ok(ablage) = storage::PositionQueue::open_named(&dir, &datei) else {
        tracing::error!(pirep_id, "ACARS-Quarantäne nicht zu öffnen");
        return false;
    };
    let zeilen: Vec<storage::QueuedPosition> = paket
        .iter()
        .filter_map(|z| {
            serde_json::to_value(z).ok().map(|v| storage::QueuedPosition {
                pirep_id: pirep_id.to_string(),
                position: v,
            })
        })
        .collect();
    match ablage.replace(&zeilen) {
        Ok(()) => true,
        Err(e) => {
            tracing::error!(pirep_id, error = ?e, "ACARS-Quarantäne nicht geschrieben");
            false
        }
    }
}

/// Die Zeilen eines gesendeten Pakets entfernen — über ihre Kennung, nicht
/// über die Position: Während der Anfrage kann sich der Anfang der
/// Warteschlange verschoben haben (Nachladen beim Beenden, Obergrenze).
fn acars_paket_entfernen(offen: &mut AcarsOffen, pirep_id: &str, paket: &[api_client::LogEntry]) {
    let ids: std::collections::HashSet<&str> = paket.iter().filter_map(|z| z.id.as_deref()).collect();
    if let Some(q) = offen.get_mut(pirep_id) {
        q.retain(|z| !z.id.as_deref().is_some_and(|i| ids.contains(i)));
    }
    offen.retain(|_, q| !q.is_empty());
}

/// Die Zeilen des Pakets, die noch im Postausgang stehen. Während der Anfrage
/// kann die Obergrenze Zeilen des Pakets schon verdrängt haben — sie stehen
/// in `verdraengt` (und bis zur Quarantäne mit in der Datei, siehe
/// `acars_fuer_datei`) und sollen nicht ein zweites Mal in die Quarantäne.
fn acars_paket_noch_offen(
    offen: &AcarsOffen,
    pirep_id: &str,
    paket: &[api_client::LogEntry],
) -> Vec<api_client::LogEntry> {
    let Some(q) = offen.get(pirep_id) else {
        return Vec::new();
    };
    let ids: std::collections::HashSet<&str> = q.iter().filter_map(|z| z.id.as_deref()).collect();
    paket
        .iter()
        .filter(|z| z.id.as_deref().is_some_and(|i| ids.contains(i)))
        .cloned()
        .collect()
}

/// Wie oft ein Paket erneut gesendet wird, wenn phpVMS weniger Zeilen
/// gespeichert meldet als gesendet (Datenbankfehler je Zeile werden dort
/// abgefangen und trotzdem mit 200 beantwortet).
const ACARS_TEILWEISE_MAX: u32 = 3;

/// Ablage nachladen, falls noch nicht geschehen; gelesene (ältere) Zeilen
/// kommen vor die schon eingereihten. `true`, wenn die Ablage gelesen ist.
async fn acars_nachladen(
    app: &AppHandle,
    offen: &mut AcarsOffen,
    geladen: &mut bool,
    verdraengt: &mut AcarsVerdraengt,
) -> bool {
    if *geladen {
        return true;
    }
    let app2 = app.clone();
    if let AcarsLaden::Ok(alt) = tokio::task::spawn_blocking(move || acars_postausgang_laden(&app2))
        .await
        .unwrap_or(AcarsLaden::Spaeter)
    {
        for (p, mut q) in alt {
            if let Some(neu) = offen.remove(&p) {
                q.extend(neu);
            }
            offen.insert(p, q);
        }
        verdraengt.extend(acars_begrenzen(offen));
        *geladen = true;
    }
    *geladen
}

/// Für das Beenden: nachladen (falls nötig), sichern und das Ergebnis melden.
async fn acars_sichern_fuer_ende(
    app: &AppHandle,
    offen: &mut AcarsOffen,
    geladen: &mut bool,
    verdraengt: &mut AcarsVerdraengt,
) -> bool {
    let gelesen = acars_nachladen(app, offen, geladen, verdraengt).await;
    if !acars_verdraengte_sichern(app, verdraengt).await {
        // Quarantäne nicht beschreibbar: die Zeilen lieber über die
        // Obergrenze hinaus in den Postausgang zurück, als sie beim Beenden
        // zu verlieren.
        for (p, z) in verdraengt.drain(..) {
            offen.entry(p).or_default().push_back(z);
        }
    }
    gelesen && acars_sichern(app, offen, verdraengt).await
}

/// Verdrängte Zeilen in die Quarantäne schreiben (je PIREP eine Datei).
/// Was nicht geschrieben werden konnte, bleibt in `verdraengt` und wird beim
/// nächsten Durchlauf erneut versucht. `true`, wenn nichts mehr offen ist.
async fn acars_verdraengte_sichern(app: &AppHandle, verdraengt: &mut AcarsVerdraengt) -> bool {
    if verdraengt.is_empty() {
        return true;
    }
    let mut je_pirep: std::collections::BTreeMap<String, Vec<api_client::LogEntry>> =
        std::collections::BTreeMap::new();
    for (p, z) in verdraengt.drain(..) {
        je_pirep.entry(p).or_default().push(z);
    }
    for (p, zeilen) in je_pirep {
        if !acars_in_quarantaene(app, &p, &zeilen).await {
            verdraengt.extend(zeilen.into_iter().map(|z| (p.clone(), z)));
        }
    }
    verdraengt.is_empty()
}

async fn acars_in_quarantaene(app: &AppHandle, pirep_id: &str, zeilen: &[api_client::LogEntry]) -> bool {
    let (app, pid, pk) = (app.clone(), pirep_id.to_string(), zeilen.to_vec());
    tokio::task::spawn_blocking(move || acars_quarantaene(&app, &pid, &pk))
        .await
        .unwrap_or(false)
}

async fn acars_postausgang_besitzer(app: AppHandle, mut rx: tokio::sync::mpsc::UnboundedReceiver<AcarsAuftrag>) {
    // Bis die Ablage gelesen ist, wird sie nicht überschrieben.
    let mut offen = AcarsOffen::new();
    let mut geladen = false;
    let mut ungesichert = false;
    // Je PIREP: nicht vor diesem Zeitpunkt erneut senden, und die letzte Wartezeit.
    let mut pause: std::collections::HashMap<String, (tokio::time::Instant, Duration)> =
        std::collections::HashMap::new();
    // Verdrängte Zeilen, die noch in die Quarantäne müssen.
    let mut verdraengt = AcarsVerdraengt::new();
    // Je PIREP: wie oft das aktuelle Paket nur teilweise gespeichert wurde.
    let mut teilweise: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    loop {
        if !geladen && acars_nachladen(&app, &mut offen, &mut geladen, &mut verdraengt).await {
            ungesichert = true;
        }
        let mut neu = false;
        let mut bestaetigen = Vec::new();
        while let Ok(auftrag) = rx.try_recv() {
            match auftrag {
                AcarsAuftrag::Zeilen(p, z) => {
                    verdraengt.extend(acars_aufnehmen(&mut offen, p, z));
                    neu = true;
                }
                AcarsAuftrag::Sichern(fertig) => bestaetigen.push(fertig),
            }
        }
        // Stehen Verdrängte jetzt in der Quarantäne, müssen sie auch aus der Datei.
        let quarantaene_neu = !verdraengt.is_empty();
        acars_verdraengte_sichern(&app, &mut verdraengt).await;
        if geladen && (neu || ungesichert || quarantaene_neu) {
            ungesichert = !acars_sichern(&app, &offen, &verdraengt).await;
        }
        for fertig in bestaetigen {
            let ok = acars_sichern_fuer_ende(&app, &mut offen, &mut geladen, &mut verdraengt).await;
            ungesichert = !ok;
            let _ = fertig.send(ok);
        }
        // Wartezeiten nur für PIREPs, die noch Zeilen haben — sonst weckte
        // eine abgelaufene Wartezeit die Schleife ohne Pause immer wieder.
        pause.retain(|p, _| offen.contains_key(p));
        teilweise.retain(|p, _| offen.contains_key(p));
        let jetzt = tokio::time::Instant::now();
        let faellig = offen
            .keys()
            .find(|p| pause.get(*p).is_none_or(|(bis, _)| *bis <= jetzt))
            .cloned();
        let Some(pirep_id) = faellig else {
            // Nichts fällig: auf neue Zeilen oder die nächste Wartezeit
            // warten. Ist die Ablage noch nicht gelesen, spätestens in 5 s
            // erneut versuchen.
            let naechste = pause
                .values()
                .map(|(bis, _)| *bis)
                .chain((!geladen).then(|| jetzt + Duration::from_secs(5)))
                .min();
            tokio::select! {
                eingang = rx.recv() => match eingang {
                    Some(AcarsAuftrag::Zeilen(p, z)) => {
                        verdraengt.extend(acars_aufnehmen(&mut offen, p, z));
                        if geladen {
                            ungesichert = !acars_sichern(&app, &offen, &verdraengt).await;
                        }
                    }
                    Some(AcarsAuftrag::Sichern(fertig)) => {
                        let ok = acars_sichern_fuer_ende(&app, &mut offen, &mut geladen, &mut verdraengt).await;
                        ungesichert = !ok;
                        let _ = fertig.send(ok);
                    }
                    None => break,
                },
                _ = async {
                    match naechste {
                        Some(bis) => tokio::time::sleep_until(bis).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {}
            }
            continue;
        };
        let client = app
            .state::<AppState>()
            .client
            .lock()
            .ok()
            .and_then(|g| g.as_ref().cloned());
        let Some(client) = client else {
            // Noch nicht angemeldet — später erneut.
            pause.insert(
                pirep_id,
                (jetzt + Duration::from_secs(30), Duration::from_secs(30)),
            );
            continue;
        };
        let paket: Vec<api_client::LogEntry> = offen[&pirep_id]
            .iter()
            .take(ACARS_PAKET_MAX)
            .cloned()
            .collect();
        // Die Anfrage läuft als eigene Aufgabe; währenddessen nimmt der
        // Besitzer weiter Zeilen an, sichert sie und beantwortet das
        // Beenden — sonst stünden Zeilen, die während einer bis zu 10 s
        // langen Anfrage kommen, beim sofortigen Schließen nach dem
        // Einreichen nicht in der Datei (Gesamtabnahme 12.10.2026).
        let mut anfrage = {
            let (client, pirep_id, paket) = (client.clone(), pirep_id.clone(), paket.clone());
            tokio::spawn(async move {
                let _live = PHPVMS_LIVE_MELDUNG.lock().await;
                client.post_acars_logs_zustellung(&pirep_id, &paket).await
            })
        };
        let zustellung = loop {
            tokio::select! {
                ergebnis = &mut anfrage => {
                    break ergebnis.unwrap_or_else(|e| {
                        api_client::Zustellung::Unklar(ApiError::Network(format!("ACARS-Anfrage abgebrochen: {e}")))
                    });
                }
                eingang = rx.recv() => match eingang {
                    Some(AcarsAuftrag::Zeilen(p, z)) => {
                        verdraengt.extend(acars_aufnehmen(&mut offen, p, z));
                        if geladen {
                            ungesichert = !acars_sichern(&app, &offen, &verdraengt).await;
                        }
                    }
                    Some(AcarsAuftrag::Sichern(fertig)) => {
                        let ok = acars_sichern_fuer_ende(&app, &mut offen, &mut geladen, &mut verdraengt).await;
                        ungesichert = !ok;
                        let _ = fertig.send(ok);
                    }
                    // Eingang zu (Programmende): Antwort noch abwarten.
                    None => {
                        break anfrage.await.unwrap_or_else(|e| {
                            api_client::Zustellung::Unklar(ApiError::Network(format!("ACARS-Anfrage abgebrochen: {e}")))
                        });
                    }
                },
            }
        };
        // Wartezeiten ab der Antwort rechnen, nicht ab dem Start der Anfrage.
        let jetzt = tokio::time::Instant::now();
        let naechste_wartezeit = |pause: &std::collections::HashMap<String, (tokio::time::Instant, Duration)>| {
            pause
                .get(&pirep_id)
                .map(|(_, w)| (*w * 2).min(Duration::from_secs(60)))
                .unwrap_or(Duration::from_secs(2))
        };
        use api_client::Zustellung;
        match zustellung {
            Zustellung::Angekommen { gespeichert } => {
                // Weniger gespeichert als gesendet → begrenzt und idempotent
                // erneut senden. Fehlt die Zahl ganz, ist das ein phpVMS ohne
                // `count` — das kennt vermutlich auch die feste Kennung nicht,
                // ein Neuversuch erzeugte dort Doppelte. Dann gilt das Paket
                // als angekommen (wie bis 2.0.9).
                let fehlt = match gespeichert {
                    api_client::Gespeichert::Zahl(n) => (n as usize) < paket.len(),
                    // Gültige Antwort ohne `count` (älteres phpVMS).
                    api_client::Gespeichert::KeineAngabe => false,
                    // Kaputte Antwort: nicht belegt — begrenzt, idempotent erneut.
                    api_client::Gespeichert::Ungueltig => true,
                };
                let versuche = teilweise.get(&pirep_id).copied().unwrap_or(0);
                if fehlt && versuche < ACARS_TEILWEISE_MAX {
                    // Dank fester Kennung legt ein Neuversuch nur an, was fehlt.
                    teilweise.insert(pirep_id.clone(), versuche + 1);
                    let warten = naechste_wartezeit(&pause);
                    tracing::warn!(
                        pirep_id = %pirep_id,
                        gesendet = paket.len(),
                        gespeichert = ?gespeichert,
                        "phpVMS hat nicht alle ACARS-Zeilen gespeichert — Neuversuch"
                    );
                    pause.insert(pirep_id, (jetzt + warten, warten));
                } else {
                    if fehlt {
                        // Nicht verwerfen: Unbelegte Zeilen kommen in die
                        // Quarantäne — sie gehen nicht verloren und halten
                        // die folgenden Zeilen nicht auf. Dank fester
                        // Kennung lassen sie sich später gefahrlos nachsenden.
                        tracing::error!(
                            pirep_id = %pirep_id,
                            gesendet = paket.len(),
                            gespeichert = ?gespeichert,
                            "phpVMS speichert einen Teil der ACARS-Zeilen dauerhaft nicht — Paket in Quarantäne"
                        );
                        let rest = acars_paket_noch_offen(&offen, &pirep_id, &paket);
                        if !rest.is_empty() && !acars_in_quarantaene(&app, &pirep_id, &rest).await {
                            // Quarantäne nicht geschrieben: im Postausgang
                            // lassen und später erneut versuchen.
                            pause.insert(
                                pirep_id,
                                (jetzt + Duration::from_secs(60), Duration::from_secs(60)),
                            );
                            if geladen {
                                ungesichert = !acars_sichern(&app, &offen, &verdraengt).await;
                            }
                            continue;
                        }
                    }
                    acars_paket_entfernen(&mut offen, &pirep_id, &paket);
                    offen.retain(|_, q| !q.is_empty());
                    pause.remove(&pirep_id);
                    teilweise.remove(&pirep_id);
                    tracing::debug!(pirep_id = %pirep_id, count = paket.len(), "ACARS log lines pushed");
                }
            }
            // Dank fester Kennung ist jeder Neuversuch ohne Doppelte möglich.
            Zustellung::NichtAngekommen(e) | Zustellung::Unklar(e) => {
                let warten = match &e {
                    ApiError::RateLimited { retry_after_seconds } => {
                        Duration::from_secs((*retry_after_seconds).clamp(1, 300))
                    }
                    _ => naechste_wartezeit(&pause),
                };
                tracing::warn!(
                    pirep_id = %pirep_id,
                    error = %e,
                    warten_s = warten.as_secs(),
                    "ACARS-Zustellung gescheitert — Neuversuch folgt"
                );
                pause.insert(pirep_id, (jetzt + warten, warten));
            }
            Zustellung::Abgelehnt(e) => {
                // Was mit den Zeilen geschieht. Gelöscht wird nie: Was nicht
                // zugestellt werden kann, geht vorher in die Quarantäne;
                // scheitert das, bleibt es im Postausgang.
                enum Folge {
                    /// PIREP nimmt nichts mehr an — alle seine Zeilen.
                    PirepRaus,
                    /// Nur dieses Paket ist betroffen (z. B. Validierung).
                    PaketRaus,
                    /// Behalten, nach dieser Pause erneut.
                    Behalten(Duration),
                }
                let folge = match &e {
                    // 404 heißt bei phpVMS „PIREP existiert nicht“ (ein
                    // fremder PIREP liefert 403, `LogRequest::authorize`).
                    // Beim aktiven Flug sofort, sonst erst nach
                    // `ACARS_ABLAUF_TAGE` — bis dahin behalten.
                    ApiError::NotFound
                        if acars_ist_aktiver_flug(&app, &pirep_id)
                            || acars_aelter_als(&offen, &pirep_id, ACARS_ABLAUF_TAGE) =>
                    {
                        Folge::PirepRaus
                    }
                    ApiError::NotFound => Folge::Behalten(Duration::from_secs(3600)),
                    // Abgebrochen (phpVMS `PirepCancelled`, RFC-7807-`type`).
                    ApiError::Server { status: 400, body } if ist_pirep_abgebrochen(body) => {
                        Folge::PirepRaus
                    }
                    // Anmeldung/Adresse ungültig — später erneut.
                    ApiError::Unauthenticated | ApiError::Forbidden | ApiError::InvalidUrl(_) => {
                        Folge::Behalten(Duration::from_secs(300))
                    }
                    _ => Folge::PaketRaus,
                };
                match folge {
                    Folge::Behalten(warten) => {
                        tracing::warn!(
                            pirep_id = %pirep_id,
                            error = %e,
                            warten_s = warten.as_secs(),
                            "ACARS-Zustellung abgelehnt — Zeilen bleiben im Postausgang"
                        );
                        pause.insert(pirep_id, (jetzt + warten, warten));
                    }
                    Folge::PirepRaus => {
                        let zeilen: Vec<api_client::LogEntry> = offen
                            .get(&pirep_id)
                            .map(|q| q.iter().cloned().collect())
                            .unwrap_or_default();
                        if acars_in_quarantaene(&app, &pirep_id, &zeilen).await {
                            acars_pirep_verwerfen(&app, &mut offen, &pirep_id, &e);
                        } else {
                            pause.insert(
                                pirep_id,
                                (jetzt + Duration::from_secs(60), Duration::from_secs(60)),
                            );
                        }
                    }
                    Folge::PaketRaus => {
                        tracing::warn!(
                            pirep_id = %pirep_id,
                            error = %e,
                            count = paket.len(),
                            "ACARS-Paket abgelehnt — in die Quarantäne"
                        );
                        let rest = acars_paket_noch_offen(&offen, &pirep_id, &paket);
                        if rest.is_empty() || acars_in_quarantaene(&app, &pirep_id, &rest).await {
                            acars_paket_entfernen(&mut offen, &pirep_id, &paket);
                            teilweise.remove(&pirep_id);
                        } else {
                            pause.insert(
                                pirep_id,
                                (jetzt + Duration::from_secs(60), Duration::from_secs(60)),
                            );
                        }
                    }
                }
            }
        }
        if geladen {
            ungesichert = !acars_sichern(&app, &offen, &verdraengt).await;
        }
    }
}

#[cfg(test)]
#[path = "tests_lib/acars_postausgang_tests.rs"]
mod acars_postausgang_tests;
