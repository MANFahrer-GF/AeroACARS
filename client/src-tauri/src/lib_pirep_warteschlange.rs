/// Nach wie vielen Versuchen im festen 60s-Takt (~50 Minuten) der Worker
/// die einmalige, sichtbare Dead-Letter-Warnung ausloest und in die
/// LANGSAME Phase wechselt — siehe `pirep_queue_dead_letter_warnung_faellig`
/// und `pirep_queue_slow_phase_wartezeit_sec`.
///
/// ⚠ Nachgereicht (externe Gegenpruefung Codex, adversarial, waehrend der
/// v1.7.17-QS-Runde entdeckt, aber eine vorbestehende, fuer JEDE PIREP-
/// Einreichung geltende Luecke seit v0.5.49): vorher gab der Worker hier
/// fuer immer und STILL auf — nur eine `tracing::warn!`-Zeile, die kein
/// Pilot je zu sehen bekam. Ein Ausfall/eine Sperre laenger als ~50
/// Minuten verlor damit einen bereits geflogenen, fertigen PIREP
/// dauerhaft und lautlos.
const PIREP_QUEUE_MAX_FAST_ATTEMPTS: u32 = 50;

/// Wartezeit zwischen Versuchen, NACHDEM `PIREP_QUEUE_MAX_FAST_ATTEMPTS`
/// ueberschritten ist. Der Worker gibt seitdem nicht mehr auf, sondern
/// versucht es nur noch seltener — genug, um bei einem laengeren Ausfall
/// nicht weiter im 60s-Takt gegen denselben Fehler zu rennen, kurz genug,
/// um eine Erholung des Servers innerhalb von Stunden, nicht Tagen, zu
/// bemerken.
const PIREP_QUEUE_SLOW_RETRY_INTERVAL_SECS: i64 = 1800;

/// Obergrenze fuer ein vom Server diktiertes `Retry-After` in DIESER
/// Warteschlange. Bewusst GROSSZUEGIGER als
/// `RATE_LIMIT_WARTEZEIT_OBERGRENZE_SEC` (eine Stunde statt eine Minute):
/// jene Grenze schuetzt einen ABGETRENNTEN, nicht persistenten Tokio-Task
/// vor stundenlangem Schlaf (siehe dessen Dokumentation, v1.7.17 Runde
/// 11) — diese Warteschlange dagegen ist ein DAUERHAFTER, plattenbasierter
/// Hintergrund-Worker, der ohnehin jede Minute erneut prueft; ein
/// laengeres Warten kostet hier nur einen uebersprungenen Tick, kein
/// verlorenes Ergebnis.
const PIREP_QUEUE_RATE_LIMIT_CAP_SECS: u64 = 3600;

/// Ob dieser Queue-Eintrag in DIESEM Tick ueberhaupt versucht werden soll
/// — `false` heisst: eine Wartezeit (Retry-After oder der langsame Takt)
/// ist noch nicht abgelaufen, einfach zum naechsten Eintrag weitergehen.
fn pirep_queue_eintrag_ist_faellig(
    retry_not_before: Option<DateTime<Utc>>,
    jetzt: DateTime<Utc>,
) -> bool {
    match retry_not_before {
        Some(not_before) => jetzt >= not_before,
        None => true,
    }
}

/// Ob VOR diesem Versuch die einmalige, sichtbare Dead-Letter-Warnung
/// ausgeloest werden soll — nur beim ERSTEN Ueberschreiten der Schnell-
/// Phase, nie danach erneut (sonst Log-Spam bei jedem weiteren Versuch).
fn pirep_queue_dead_letter_warnung_faellig(attempt_count: u32, dead_letter_notified: bool) -> bool {
    attempt_count >= PIREP_QUEUE_MAX_FAST_ATTEMPTS && !dead_letter_notified
}

/// Wartezeit (Sekunden) vor dem naechsten Versuch, NACHDEM dieser
/// Versuch (mit dem hier uebergebenen, bereits erhoehten Zaehler)
/// fehlgeschlagen ist — `None` in der schnellen Phase (naechster Tick
/// versucht sofort erneut, unveraendertes Verhalten seit v0.5.49).
fn pirep_queue_slow_phase_wartezeit_sec(attempt_count_nach_diesem_versuch: u32) -> Option<i64> {
    if attempt_count_nach_diesem_versuch >= PIREP_QUEUE_MAX_FAST_ATTEMPTS {
        Some(PIREP_QUEUE_SLOW_RETRY_INTERVAL_SECS)
    } else {
        None
    }
}

/// Codex-Folgefund (adversarial, 05.09.2026): darf der Worker DIESEN Eintrag
/// mit dem GERADE eingeloggten Client bearbeiten? `None` als Eintrags-
/// Eigentuemer (Alt-Eintraege von vor diesem Feld, oder kein Client beim
/// Einreihen verfuegbar) zaehlt bewusst als "unbekannt", NICHT als
/// "niemandes, also freigegeben" — sonst wuerde ausgerechnet der Fall, in
/// dem wir am wenigsten wissen, am grosszuegigsten behandelt.
fn pirep_queue_eintrag_gehoert_aktuellem_piloten(
    eintrag_eigentuemer: Option<&str>,
    aktuelle_identitaet: &str,
) -> bool {
    eintrag_eigentuemer == Some(aktuelle_identitaet)
}

#[cfg(test)]
#[path = "tests_lib/pirep_queue_dead_letter_tests.rs"]
mod pirep_queue_dead_letter_tests;

#[cfg(test)]
/// Codex-Folgefund (adversarial, 05.09.2026): die unbegrenzte Slow-Phase
/// von oben macht das Zeitfenster, in dem ein ANDERER Pilot inzwischen auf
/// derselben Maschine eingeloggt sein kann, potenziell beliebig lang — ohne
/// Eigentuemer-Pruefung haette der Worker Pilot As PIREP mit Pilot Bs
/// Credentials eingereicht (oder geloescht, wenn der Server 403/404 sagt).
#[path = "tests_lib/pirep_queue_eigentuemer_tests.rs"]
mod pirep_queue_eigentuemer_tests;

#[cfg(test)]
/// Codex-Folgefund (adversarial, 05.09.2026, zweite Runde): eine reine
/// Fingerabdruck-Pruefung ohne Reklamier-Pfad wuerde JEDEN Alt-Eintrag
/// (`owner_identity: None`, von vor diesem Feld) und jeden Eintrag nach
/// einer API-Key-Rotation DESSELBEN Piloten dauerhaft verwaisen lassen —
/// "no-ship", weil es beim Update selbst jeden wartenden PIREP verliert.
/// Der Worker-Loop bleibt Tauri-/async-gebunden und nicht isoliert testbar
/// (wie der Rest dieser Infrastruktur) — ein Quelltext-Wächter faengt
/// trotzdem, wenn der Server-Reklamier-Pfad ersatzlos verschwindet.
#[path = "tests_lib/pirep_queue_reklamieren_wiring_tests.rs"]
mod pirep_queue_reklamieren_wiring_tests;

/// Codex-Folgefund (adversarial, 05.09.2026, neunte Runde): drei getrennte
/// Luecken in derselben Konten-Isolations-Serie — keine davon betrifft eine
/// bereits gefixte Fehlerklasse aus den Runden 1-8.
#[cfg(test)]
#[path = "tests_lib/konten_isolierung_runde_neun_wiring_tests.rs"]
mod konten_isolierung_runde_neun_wiring_tests;

/// Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): sieben weitere,
/// voneinander unabhaengige Luecken in derselben Serie.
#[cfg(test)]
#[path = "tests_lib/konten_isolierung_runde_zehn_wiring_tests.rs"]
mod konten_isolierung_runde_zehn_wiring_tests;

/// Codex-Folgefund (adversarial, 05.09.2026, elfte Runde): fuenf weitere
/// Luecken rund um den MQTT-Lebenszyklus und den Resume-Pfad.
#[cfg(test)]
#[path = "tests_lib/konten_isolierung_runde_elf_wiring_tests.rs"]
mod konten_isolierung_runde_elf_wiring_tests;

/// Codex-Folgefund (adversarial, 05.09.2026, zwoelfte Runde): der MQTT-
/// Lebenszyklus blieb der Rest-Herd — zwei weitere Luecken dort plus ein
/// verspaeteter Profil-Refresh, der eine neue Sitzung ueberschreiben
/// konnte.
#[cfg(test)]
#[path = "tests_lib/konten_isolierung_runde_zwoelf_wiring_tests.rs"]
mod konten_isolierung_runde_zwoelf_wiring_tests;

/// Codex-Folgefund (adversarial, 05.09.2026, dreizehnte Runde): nach drei
/// Runden Einzel-Patches gegen denselben MQTT-Lebenszyklus (10, 11, 12),
/// darunter zwei echte Widersprueche zwischen fruheren Fixes, wurde der
/// Session-Epochen-Zaehler eingefuehrt (`session_epoch`) — ein einziges,
/// autoritatives Signal statt verstreuter Feld-Vergleiche.
#[cfg(test)]
#[path = "tests_lib/konten_isolierung_runde_dreizehn_wiring_tests.rs"]
mod konten_isolierung_runde_dreizehn_wiring_tests;

/// Codex-Folgefund (adversarial, 05.09.2026, vierzehnte Runde): eine
/// gezielte Pruefung des Runde-13-Umbaus selbst fand zwei echte
/// Regressionen (Session-Snapshot in `phpvms_get_bids`, fehlender
/// Epochen-Bump in beiden Restore-Ablehnungs-Zweigen von `phpvms_load_
/// session`) — beide hier gefixt. Der Rest der Runde-14-Befunde
/// (Atomaritaets-Grenzen des Epochen-Modells selbst, weitere Konsumenten
/// ohne Epochen-Bindung) ist als bekanntes Restrisiko dokumentiert, siehe
/// `docs/qs/pirep-infrastruktur-haertung-2026-09.md`.
#[cfg(test)]
#[path = "tests_lib/konten_isolierung_runde_vierzehn_wiring_tests.rs"]
mod konten_isolierung_runde_vierzehn_wiring_tests;

fn spawn_pirep_queue_worker(app: AppHandle) {
    // v0.5.50 — `tauri::async_runtime::spawn` statt `tokio::spawn`.
    // Diese Funktion wird aus dem synchronen `.setup()`-Closure
    // aufgerufen — auf macOS ist da noch kein tokio-Runtime-Context
    // aktiv, weshalb `tokio::spawn` in v0.5.49 mit „no reactor running"
    // panic'd und die App sofort nach Update-Restart geschlossen wurde
    // (Pilot-Report: Mac öffnet nicht mehr nach v0.5.49-Update).
    // `tauri::async_runtime::spawn` nutzt explizit Tauris managed Runtime
    // und funktioniert auch im Setup-Context auf allen Plattformen.
    tauri::async_runtime::spawn(async move {
        const TICK: std::time::Duration = std::time::Duration::from_secs(60);
        loop {
            tokio::time::sleep(TICK).await;
            let queued = pirep_queue::list_all(&app);
            let state = app.state::<AppState>();
            // Wartende Bahn-Nachtraege ZUERST — sie brauchen nur MQTT, nicht
            // den phpVMS-Client, darum vor dem Client-Riegel.
            {
                let sender = state
                    .mqtt
                    .lock()
                    .await
                    .as_ref()
                    .map(|h| h.nachtrag_sender());
                if let Some(sender) = sender {
                    let n = nachtrag_queue::drain(&app, &sender).await;
                    if n > 0 {
                        tracing::info!(anzahl = n, "Bahn-Nachtraege aus der Ablage geschickt");
                    }
                }
            }
            // Client aus dem AppState holen — könnte fehlen wenn Pilot
            // nicht eingeloggt ist (= keine Connection). Dann skippen.
            //
            // Codex-Folgefund (adversarial, 05.09.2026, achte Runde): Client
            // UND Piloten-ID werden hier bewusst als EIN atomarer Schnappschuss
            // erfasst, VOR jedem Await in diesem Tick — nicht der Client
            // hier, die Identitaet erst spaeter (wie zuvor). Dazwischen liegt
            // `drain_pending_bid_cleanup(...).await`, ein echter Await-Punkt;
            // ein Logout/Login-Kontowechsel in genau dieser Luecke haette
            // Pilot As Client mit Pilot Bs Identitaet gepaart — Ergebnis: ein
            // Warteschlangen-Eintrag, der Pilot B tatsaechlich gehoert,
            // besteht die Eigentuemer-Pruefung, wird aber mit Pilot As
            // Credentials eingereicht (und bei einem 403/404 faelschlich
            // geloescht statt in Quarantaene zu bleiben).
            //
            // Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): das
            // allein reichte NICHT — zwei UNABHAENGIGE Mutexe geben keine
            // gemeinsame Atomaritaets-Garantie, auch ohne Await dazwischen,
            // wenn Leser (hier) und Schreiber (`phpvms_login`/`phpvms_load_
            // session`) auf verschiedenen OS-Threads laufen koennen (Tauris
            // Multi-Thread-Runtime erlaubt genau das). `aktuelle_session_
            // atomar` haelt BEIDE Locks gleichzeitig und schliesst diese
            // Luecke wirklich.
            let Some((client, pilot_id, _epoche)) = aktuelle_session_atomar(&state) else {
                continue;
            };
            let aktuelle_identitaet = pilot_id.to_string();

            // v0.7.19 GAF-707 (QS-R1 Finding 1): Pending-Bid-Cleanup-Queue
            // bei jedem Tick mit drainen — unabhaengig davon ob auch
            // pireps queued sind. Sonst bleibt der Bid nach einem
            // erfolgreichen FILED + transientem delete_bid-Fail haengen
            // bis zufaellig ein anderer PIREP queued wird.
            drain_pending_bid_cleanup(&app, &client, &aktuelle_identitaet).await;

            if queued.is_empty() {
                continue;
            }
            tracing::info!(queued_count = queued.len(), "PIREP-queue worker tick");
            // Codex-Folgefund (adversarial, 05.09.2026, zweite Runde): eine
            // reine Fingerabdruck-Pruefung wuerde JEDEN Alt-Eintrag (von vor
            // diesem Feld, `owner_identity: None`) fuer immer in Quarantaene
            // lassen — auch wenn derselbe Pilot, der ihn eingereiht hat,
            // laengst wieder eingeloggt ist. Derselbe Ausfall bei einer
            // API-Key-Rotation DESSELBEN Piloten (neuer Fingerabdruck, alter
            // Eintrag). Vor der endgueltigen Quarantaene EINMAL pro Tick
            // serverseitig nachfragen (`GET /api/user/pireps?state=0`,
            // serverseitig auf den eingeloggten Piloten gefiltert): steht
            // der Eintrag dort noch als IN_PROGRESS, gehoert er unabhaengig
            // vom lokalen Fingerabdruck demselben Account — reklamieren
            // (Fingerabdruck neu schreiben) statt verwaisen lassen.
            let mut server_pireps_in_progress: Option<Vec<String>> = None;
            for mut q in queued {
                // `pending_pireps/` ist global, nicht pro Pilot. Seit die
                // Slow-Phase oben nicht mehr aufgibt, kann ein Eintrag lange
                // genug liegen, dass ein ANDERER Pilot inzwischen auf
                // derselben Maschine eingeloggt ist (`phpvms_logout` erlaubt
                // das ausdruecklich). Ohne diese Pruefung wuerde der Worker
                // Pilot As PIREP mit Pilot Bs Credentials einreichen —
                // Erfolg: falsch zugeordnet; 403/404 (nicht-transient):
                // geloescht, Pilot As Flug dauerhaft verloren.
                if !pirep_queue_eintrag_gehoert_aktuellem_piloten(
                    q.owner_identity.as_deref(),
                    &aktuelle_identitaet,
                ) {
                    if server_pireps_in_progress.is_none() {
                        server_pireps_in_progress = Some(
                            client
                                .get_user_pireps_in_progress()
                                .await
                                .map(|liste| liste.into_iter().map(|p| p.id).collect())
                                .unwrap_or_default(),
                        );
                    }
                    let serverseitig_bestaetigt = server_pireps_in_progress
                        .as_ref()
                        .is_some_and(|ids| ids.iter().any(|id| id == &q.pirep_id));
                    if serverseitig_bestaetigt {
                        tracing::info!(
                            pirep_id = %q.pirep_id,
                            eigentuemer_bekannt = q.owner_identity.is_some(),
                            "pirep_queue: Eintrag serverseitig als IN_PROGRESS des aktuellen \
                             Accounts bestaetigt — reklamiert (Alt-Eintrag oder API-Key-Rotation)"
                        );
                        q.owner_identity = Some(aktuelle_identitaet.clone());
                        if let Err(e) = pirep_queue::enqueue(&app, &q) {
                            tracing::warn!(pirep_id = %q.pirep_id, error = %e, "pirep_queue: Reklamieren fehlgeschlagen (Persistieren)");
                        }
                        // Faellt durch zur normalen Verarbeitung unten — der
                        // Eintrag ist jetzt wie jeder andere eigene.
                    } else {
                        // Quarantaene statt Einreichen ODER Loeschen: weder
                        // Versuchszaehler noch Retry-Zeit werden angefasst,
                        // der Eintrag liegt einfach weiter, bis der richtige
                        // Pilot wieder eingeloggt ist (oder das VA-Team
                        // manuell eingreift).
                        tracing::warn!(
                            pirep_id = %q.pirep_id,
                            eigentuemer_bekannt = q.owner_identity.is_some(),
                            "pirep_queue: Eintrag gehoert nicht dem aktuell eingeloggten Piloten — \
                             Quarantaene, weder eingereicht noch geloescht"
                        );
                        continue;
                    }
                }
                // Nachgereicht (externe Gegenpruefung Codex, adversarial):
                // eine Wartezeit (Retry-After oder der langsame Takt nach
                // der Dead-Letter-Warnung) ist noch nicht abgelaufen —
                // einfach zum naechsten Eintrag weitergehen, ohne den
                // Versuchszaehler anzufassen.
                if !pirep_queue_eintrag_ist_faellig(q.retry_not_before, Utc::now()) {
                    continue;
                }
                // Nachgereicht: EINMALIG sichtbar warnen, statt fuer immer
                // still aufzugeben — siehe `PIREP_QUEUE_MAX_FAST_ATTEMPTS`.
                if pirep_queue_dead_letter_warnung_faellig(q.attempt_count, q.dead_letter_notified)
                {
                    q.dead_letter_notified = true;
                    if let Err(e) = pirep_queue::enqueue(&app, &q) {
                        tracing::warn!(pirep_id = %q.pirep_id, error = %e, "pirep_queue: persisting dead-letter flag failed");
                    }
                    tracing::error!(pirep_id = %q.pirep_id, attempts = q.attempt_count, "queued PIREP still not filed after many attempts — notifying pilot, will keep retrying more slowly");
                    log_activity_handle(
                        &app,
                        ActivityLevel::Warn,
                        format!(
                            "PIREP konnte nach {} Versuchen noch nicht eingereicht werden: {} {} – {}",
                            q.attempt_count,
                            format_callsign(&q.airline_icao, &q.flight_number),
                            q.dpt_airport,
                            q.arr_airport,
                        ),
                        Some(format!(
                            "Letzter Fehler: {}. Der Flug bleibt in der Warteschlange und wird \
                             weiter versucht (jetzt in groesseren Abstaenden) — bei anhaltenden \
                             Problemen bitte beim VA-Team melden (PIREP-ID {}).",
                            q.last_error.clone().unwrap_or_else(|| "unbekannt".to_string()),
                            q.pirep_id,
                        )),
                    );
                }
                q.attempt_count += 1;
                q.last_attempt_at = Some(Utc::now());
                // ERST die Landung, DANN der Flugbericht.
                //
                // Trifft der Bericht ohne Landung ein, faellt das Urteil
                // „keine Landung aufgezeichnet", und das laesst sich nicht
                // mehr umdrehen, sobald die Webseite es abgeholt hat
                // (AAL 1331, 20.09.2026). Die Wiederholung ist
                // ungefaehrlich: Der Recorder erkennt dieselbe Landung an
                // Pilot und Zeitstempel.
                // Wie beim Publish nach dem Filing: Zwischen dem
                // Sitzungs-Schnappschuss am Tick-Anfang und hier liegen
                // mehrere Wartepunkte. Ein Kontowechsel dazwischen wuerde
                // Pilot As Landung ueber Pilot Bs Verbindung schicken
                // (Abnahme 20.09.2026, dieselbe Fehlerklasse wie beim
                // PIREP-Publish).
                let identitaet_vor_landung = state
                    .authenticated_pilot_id
                    .lock()
                    .expect("authenticated_pilot_id lock")
                    .map(|id| id.to_string())
                    == Some(aktuelle_identitaet.clone());
                if !identitaet_vor_landung && q.landung.is_some() {
                    // Laut sagen, wie der PIREP-Pfad weiter unten auch.
                    // Still verwerfen waere das Schlimmste: Der Bericht
                    // geht gleich darauf raus, der Eintrag wird entfernt,
                    // und auf DIESEM Pfad traegt auch das Flugprotokoll
                    // nichts nach — der Upload haengt am selben
                    // Identitaets-Riegel (Abnahme 20.09.2026).
                    tracing::warn!(
                        pirep_id = %q.pirep_id,
                        "pirep_queue: Konto wechselte waehrend der Verarbeitung — \
                         die Landung wird NICHT nachgeschickt"
                    );
                }
                if let Some(json) = q.landung.clone().filter(|_| identitaet_vor_landung) {
                    let gelesen = serde_json::from_value::<aeroacars_mqtt::TouchdownPayload>(json);
                    let gesendet = match gelesen {
                        Ok(payload) => {
                            // `client_version` bleibt LEER. Der Typ ist
                            // `&'static str` und laesst sich nicht
                            // zurueckgelesen; die Version der laufenden App
                            // einzutragen waere falsch, weil das Feld
                            // gerade dazu dient, Messumstellungen in der
                            // Datenbank nachzurechnen. Lieber keine Angabe
                            // als eine falsche (Abnahme 20.09.2026).
                            let ack = {
                                let mqtt = state.mqtt.lock().await;
                                mqtt.as_ref().map(|h| h.touchdown_bestaetigt(payload))
                            };
                            match ack {
                                Some(rx) => tokio::time::timeout(LANDUNG_ACK_FRIST, rx)
                                    .await
                                    .ok()
                                    .and_then(|r| r.ok())
                                    .unwrap_or(false),
                                None => false,
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                pirep_id = %q.pirep_id,
                                error = %e,
                                "Landung aus der Warteschlange ist nicht lesbar"
                            );
                            true // nichts zu senden — den Bericht nicht aufhalten
                        }
                    };
                    if gesendet {
                        tracing::info!(
                            pirep_id = %q.pirep_id,
                            "Landung aus der Warteschlange nachgeschickt"
                        );
                        q.landung = None;
                    } else {
                        // Bleibt liegen, solange der Eintrag lebt. Geht der
                        // Bericht gleich darauf durch, wird der Eintrag
                        // entfernt — die Landung ist dann NICHT verloren:
                        // Der Worker laedt anschliessend das Flugprotokoll
                        // hoch, und darin steht sie (`record_event` beim
                        // Aufsetzen). Sie kommt dann spaeter an als der
                        // Bericht, und der Flug kann in der Pruefliste
                        // landen — genau der Rest, der vom AAL-1331-Fall
                        // uebrig bleibt.
                        tracing::warn!(
                            pirep_id = %q.pirep_id,
                            "Landung aus der Warteschlange nicht zugestellt — \
                             das Flugprotokoll traegt sie nach"
                        );
                    }
                }
                let ergebnis = {
                    let _live = PHPVMS_LIVE_MELDUNG.lock().await;
                    client.file_pirep(&q.pirep_id, &q.body).await
                };
                match ergebnis {
                    Ok(()) => {
                        tracing::info!(pirep_id = %q.pirep_id, "queued PIREP filed successfully by background worker");
                        pirep_queue::remove(&app, &q.pirep_id);
                        log_activity_handle(
                            &app,
                            ActivityLevel::Info,
                            format!(
                                "Gequeueter PIREP nachträglich eingereicht: {} {} – {}",
                                format_callsign(&q.airline_icao, &q.flight_number),
                                q.dpt_airport,
                                q.arr_airport,
                            ),
                            Some(format!(
                                "Nach {} Versuch(en) — Verbindung war wieder da",
                                q.attempt_count
                            )),
                        );
                        // Best-effort Bid-Cleanup. v0.7.19 (QS-R1 Finding 1):
                        // transientes Scheitern landet in pending_bid_cleanup
                        // und wird im naechsten Tick erneut probiert.
                        consume_bid_best_effort(
                            &app,
                            &client,
                            &q.pirep_id,
                            q.bid_id,
                            // v0.7.19 (QS-R2 Finding 2): flight_id-Fallback
                            // aus dem persistierten QueuedPirep mitgeben.
                            Some(q.flight_id.as_str()),
                            "queued_filed",
                            Some(aktuelle_identitaet.clone()),
                        )
                        .await;
                        // Selber Tick: Pending-Bid-Cleanup-Queue drainen
                        drain_pending_bid_cleanup(&app, &client, &aktuelle_identitaet).await;
                        // Codex-Folgefund (adversarial, 05.09.2026, neunte
                        // Runde): das phpVMS-Filing oben nutzt den am
                        // Tick-Anfang erfassten, zu `aktuelle_identitaet`
                        // passenden `client` — sicher. Die MQTT-Publish- und
                        // JSONL-Upload-Schritte darunter lesen aber ERNEUT
                        // `state.mqtt` bzw. frisch aus dem Keyring, NACH
                        // mehreren Awaits seit dem Schnappschuss. Ein
                        // Kontowechsel in genau dieser Luecke wuerde Pilot
                        // As bereits erfolgreich eingereichten PIREP mit
                        // Pilot Bs MQTT-Verbindung/Zugangsdaten weitersenden
                        // — beide Kanaele sind ausdruecklich als Best-Effort
                        // markiert, werden hier deshalb bewusst nur
                        // uebersprungen (nicht nachgeholt) statt das Filing
                        // selbst zu gefaehrden.
                        let identitaet_noch_aktuell = state
                            .authenticated_pilot_id
                            .lock()
                            .expect("authenticated_pilot_id lock")
                            .map(|id| id.to_string())
                            == Some(aktuelle_identitaet.clone());
                        if !identitaet_noch_aktuell {
                            tracing::warn!(
                                pirep_id = %q.pirep_id,
                                "pirep_queue: Konto wechselte waehrend der Verarbeitung — \
                                 MQTT-Publish und JSONL-Upload fuer diesen Eintrag uebersprungen \
                                 (PIREP wurde bereits korrekt eingereicht)"
                            );
                        } else {
                            // v0.12.5 (LE1b): JSONL-PirepFiled-Event schreiben
                            // + MQTT-PIREP nachträglich publishen — sonst fehlt
                            // der gequeuete Flug dauerhaft in den VPS-Reports.
                            // QS-P1: `finalize_filed_pirep` schreibt das JSONL-
                            // Event IMMER (auch ohne MQTT-Handle) — Voraussetzung
                            // für den Recorder-seitigen JSONL-Gap-Fill.
                            // Null = altes queued-File ohne das Feld → skip.
                            if !q.pirep_payload_json.is_null() {
                                // P-7.21: Eingabe vor dem PIREP bei live.
                                eingabe_vor_dem_bericht(&app).await;
                                let mqtt = state.mqtt.lock().await;
                                finalize_filed_pirep(
                                    &app,
                                    mqtt.as_ref(),
                                    &q.pirep_id,
                                    q.pirep_payload_json.clone(),
                                    None,
                                    q.bahn_nachtrag.clone(),
                                );
                            } else if let Some(json) = q.bahn_nachtrag.clone() {
                                // Altbestand ohne PIREP-JSON, aber mit Nachtrag:
                                // den Nachtrag trotzdem schicken — und beide
                                // Fehlerfaelle MELDEN (Runde 4, N19).
                                let mqtt = state.mqtt.lock().await;
                                match (
                                    mqtt.as_ref(),
                                    aeroacars_mqtt::TouchdownRolloutFinalizedPayload::aus_json(
                                        json,
                                    ),
                                ) {
                                    (Some(handle), Ok(nachtrag)) => {
                                        if let Err(e) = nachtrag_queue::senden(
                                            &app,
                                            &handle.nachtrag_sender(),
                                            nachtrag,
                                        ) {
                                            tracing::error!(pirep_id = %q.pirep_id, error = %e, "Bahn-Nachtrag aus der Warteschlange nicht abgelegt — verloren (PIREP ist eingereicht)");
                                        }
                                    }
                                    (None, Ok(nachtrag)) => {
                                        if let Err(e) =
                                            nachtrag_queue::enqueue_offline(&app, &nachtrag)
                                        {
                                            tracing::error!(pirep_id = %q.pirep_id, error = %e, "Ablage nicht beschreibbar");
                                        }
                                    }
                                    (_, Err(e)) => tracing::warn!(
                                        pirep_id = %q.pirep_id,
                                        error = %e,
                                        "Bahnkorrektur aus der Warteschlange (ohne PIREP-JSON) ist nicht lesbar"
                                    ),
                                }
                            }
                            // Best-effort: JSONL-Upload (wenn das Recorder-File noch da ist)
                            spawn_flight_log_upload(
                                &app,
                                q.pirep_id.clone(),
                                Some(aktuelle_identitaet.clone()),
                                false,
                            );
                        }
                    }
                    Err(e) => {
                        // v0.8.3 (#1): Hard-Errors NICHT mehr endlos
                        // re-enqueuen. Vorher: jeder Fehler — auch HTTP 400
                        // "PIREP is read-only" (= phpVMS hat den PIREP
                        // bereits als ACCEPTED markiert, lehnt jedes
                        // weitere /file ab) — wurde als transient behandelt
                        // und alle 60 s nochmal probiert, bis MAX_ATTEMPTS
                        // erreicht war. Pilot sah 50 Min lang rote
                        // "PIREP file failed"-Spam-Eintraege im Activity-Log
                        // (Reported 2026-05-17 Svenny, BE36-Session 819).
                        //
                        // Jetzt: bei nicht-transienten Fehlern den Queue-
                        // Eintrag verwerfen + EINEN sauberen Activity-Log-
                        // Hinweis schreiben. Bei transienten Fehlern (Netz
                        // weg, 5xx, 408, 429) bleibt das alte Verhalten.
                        if !is_transient_pirep_error(&e) {
                            pirep_queue::remove(&app, &q.pirep_id);
                            tracing::warn!(
                                pirep_id = %q.pirep_id,
                                error = %e,
                                "queued PIREP rejected by server — removing from queue"
                            );
                            log_activity_handle(
                                &app,
                                ActivityLevel::Warn,
                                format!(
                                    "PIREP {} serverseitig abgelehnt — Queue-Eintrag entfernt",
                                    format_callsign(&q.airline_icao, &q.flight_number),
                                ),
                                Some(format!(
                                    "{}. Falls Du den Flug filen wolltest: phpVMS-Web pruefen \
                                     (PIREP evtl. bereits eingereicht oder gecancelt).",
                                    friendly_net_error(&e),
                                )),
                            );
                            // Best-effort: JSONL-Upload trotzdem versuchen
                            // — Forensik-Log ist auch dann wertvoll wenn
                            // der PIREP nicht (mehr) angenommen wurde.
                            // (Fix #6 — siehe Worker-Success-Pfad oben.)
                            // Codex-Folgefund (adversarial, 05.09.2026,
                            // zehnte Runde): dieser Zweig rief bisher OHNE
                            // jede Identitaets-Bindung auf — die Pruefung
                            // sitzt jetzt in `spawn_flight_log_upload`
                            // selbst (zum Zeitpunkt der tatsaechlichen
                            // Ausfuehrung, nicht hier beim Spawn).
                            spawn_flight_log_upload(
                                &app,
                                q.pirep_id.clone(),
                                Some(aktuelle_identitaet.clone()),
                                false,
                            );
                            continue;
                        }
                        // Nachgereicht (externe Gegenpruefung Codex,
                        // adversarial): eine vom Server diktierte
                        // `Retry-After`-Wartezeit gilt unabhaengig von der
                        // schnellen/langsamen Phase — sie IMMER zu
                        // ignorieren und trotzdem jede Minute erneut zu
                        // versuchen waere kein guter API-Umgang.
                        let retry_after_secs = if let ApiError::RateLimited {
                            retry_after_seconds,
                        } = &e
                        {
                            let capped =
                                (*retry_after_seconds).min(PIREP_QUEUE_RATE_LIMIT_CAP_SECS) as i64;
                            tracing::warn!(pirep_id = %q.pirep_id, retry_after_seconds, capped, "queued PIREP rate-limited — honoring server Retry-After");
                            Some(capped)
                        } else {
                            pirep_queue_slow_phase_wartezeit_sec(q.attempt_count)
                        };
                        q.retry_not_before = retry_after_secs
                            .map(|secs| Utc::now() + chrono::Duration::seconds(secs));
                        q.last_error = Some(friendly_net_error(&e));
                        // Update das File mit dem neuen attempt-count + Error
                        if let Err(e) = pirep_queue::enqueue(&app, &q) {
                            tracing::warn!(pirep_id = %q.pirep_id, error = %e, "pirep_queue: persisting retry state failed — attempt-count/last_error may be lost");
                        }
                        tracing::warn!(
                            pirep_id = %q.pirep_id,
                            attempt = q.attempt_count,
                            error = %e,
                            "queued PIREP file failed (transient) — will retry next tick"
                        );
                    }
                }
            }
        }
    });
}
