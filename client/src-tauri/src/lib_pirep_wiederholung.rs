/// v0.5.49 — Auto-Retry für PIREP-File mit exponentiellem Backoff.
///
/// Versucht `client.file_pirep(pirep_id, body)` bis zu 3× — Initialversuch
/// + 2 Retries. Backoffs: 5s, 30s. Bei TRANSIENTEM Fehler (Netz weg,
/// Timeout, 5xx, 408, 429) wird retried. Bei HARD-Fehler (4xx außer
/// retry-fähig) sofort raus damit der Pilot was machen kann (z.B. 422
/// validation error — der Pilot muss den PIREP korrigieren).
///
/// Wenn auch nach 3× nicht durch: `Err(last_error)`. Der Caller in
/// `flight_end()` queued dann via `pirep_queue::enqueue` und meldet
/// Success an die UI (Pilot kann weiter, Worker erledigt's später).
async fn file_pirep_with_retry(
    client: &Client,
    pirep_id: &str,
    body: &api_client::FileBody,
) -> Result<(), ApiError> {
    const ATTEMPTS: u32 = 3;
    const BACKOFFS_SEC: [u64; 2] = [5, 30]; // wait BEFORE attempt 2 and 3
    let mut last_err: Option<ApiError> = None;
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            let secs = BACKOFFS_SEC[(attempt as usize - 1).min(BACKOFFS_SEC.len() - 1)];
            tracing::info!(pirep_id, attempt, backoff_sec = secs, "PIREP file retry");
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        }
        let ergebnis = {
            let _live = PHPVMS_LIVE_MELDUNG.lock().await;
            client.file_pirep(pirep_id, body).await
        };
        match ergebnis {
            Ok(()) => return Ok(()),
            Err(e) if !is_transient_pirep_error(&e) => return Err(e),
            Err(e) => {
                tracing::warn!(pirep_id, attempt, error = %e, "PIREP file attempt failed (transient)");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or(ApiError::Network(
        "file_pirep_with_retry: no error captured".into(),
    )))
}

/// True wenn der Fehler nach Backoff retried werden soll (Netz, Timeout, 5xx, 408, 429).
/// False bei harten 4xx (validation, auth, …) oder 404 (PIREP wurde gecancelt).
fn is_transient_pirep_error(e: &ApiError) -> bool {
    match e {
        // 404 = PIREP wurde gecancelt → kein Retry, Caller muss reagieren
        ApiError::NotFound => false,
        // Auth-Fehler → kein Retry, Pilot muss neu einloggen
        ApiError::Unauthenticated | ApiError::Forbidden => false,
        // 5xx vom Server, 429 Rate-Limit, 408 Timeout → transient
        ApiError::Server { status, .. } => *status == 408 || (*status >= 500 && *status < 600),
        ApiError::RateLimited { .. } => true,
        // Network = Connect/Timeout/RST/etc. → klassisch transient
        ApiError::Network(_) => true,
        // InvalidUrl / BadResponse → wir können's nicht reparieren
        ApiError::InvalidUrl(_) | ApiError::BadResponse(_) => false,
    }
}

/// Best-effort PIREP-Feld-Post mit KURZEM Retry bei transienten Fehlern
/// (Netz, Timeout, 5xx) — kein dauerhaftes Nachfassen ueber Ticks/Neustarts
/// hinweg wie `file_pirep_with_retry` (das waere fuer ein Live-Anzeigefeld
/// unverhaeltnismaessig), aber auch nicht der erste Versuch als einzige
/// Chance. Harte Fehler (4xx, PIREP gecancelt) brechen sofort ab.
///
/// ⚠ v1.7.17 Runde 7 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): die Rueckzugs-Meldung fuer einen erkannten veralteten
/// `dep_gate` war ein einzelner Fire-and-Forget-Versuch (`post_void` ohne
/// Wiederholung) — ein einziger transienter Netzfehler liess den
/// bekanntlich falschen Server-Wert dann fuer den Rest des Fluges
/// unkorrigiert stehen, ohne jede zweite Chance. Bewusst NICHT die volle,
/// von Codex vorgeschlagene dauerhafte/geordnete Warteschlange gebaut:
/// das reine Umsortierungs-Risiko (die URSPRUENGLICHE Live-Meldung trifft
/// NACH der Rueckzugs-Meldung ein) braeuchte eine Serverseitige
/// Versionierung, die es nicht gibt, und wuerde einen mehrminuetigen
/// Netz-Stau ausgerechnet auf DER EINEN, laengst abgeschlossenen Anfrage
/// voraussetzen — unverhaeltnismaessiger Aufwand fuer ein derart schmales
/// Restrisiko, das obendrein exakt dem bestehenden, ueberall in dieser
/// Datei akzeptierten Fire-and-Forget-Modell fuer Live-Felder entspricht.
///
/// ⚠ v1.7.17 Runde 8 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): `ApiError::RateLimited` traegt eine vom Server diktierte
/// `retry_after_seconds` — die erste Fassung klassifizierte sie zwar als
/// transient, ignorierte den Wert selbst aber und wartete trotzdem nur die
/// festen 2s/8s. Bei jeder vom Server verlangten Wartezeit ueber 10s
/// (der API-Client faellt sogar auf 60s zurueck, wenn der Header fehlt)
/// waren alle drei Versuche VOR Ablauf der Sperre verbraucht — die
/// Rueckzugs-Meldung gab dann exakt in dem Fall auf, fuer den `RateLimited`
/// ueberhaupt existiert. Die Wartezeit vor dem naechsten Versuch kommt
/// jetzt aus dem Fehler selbst, wenn er einer ist.
const PIREP_FELD_RETRY_BACKOFFS_SEC: [u64; 2] = [2, 8];

/// Obergrenze fuer eine vom Server diktierte `Retry-After`-Wartezeit, bis
/// zu der noch gewartet wird — darueber wird AUFGEGEBEN statt gewartet.
///
/// ⚠ v1.7.17 Runde 9 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): `retry_after_seconds` kommt ungeprueft aus einem
/// geparsten HTTP-Header. Ohne Obergrenze koennte ein extremer Wert
/// (Server-Bug, kaputte Zwischenstation) die Deadline-Berechnung in
/// `tokio::time::sleep` ueberlaufen lassen (`Instant + Duration` kann
/// bei absurd grossen Werten paniken) oder den Task praktisch fuer immer
/// schlafen legen.
///
/// ⚠ v1.7.17 Runde 10 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): die Runde-9-Grenze (5 Minuten) war viel enger als das
/// tatsaechliche Ueberlaufrisiko und haette einen LEGITIMEN, laengeren
/// Retry-After (z. B. 15-60 Minuten) verkuerzt — auf eine Stunde
/// angehoben, um das zu vermeiden.
///
/// ⚠ v1.7.17 Runde 11 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): Runde 10 war ihrerseits zu grosszuegig — mit bis zu drei
/// Versuchen UND jeweils bis zu einer Stunde Wartezeit haette die
/// Rueckzugs-Meldung bis zu ZWEI Stunden in einem einzelnen, NICHT
/// persistenten, abgetrennten Task schlafen koennen. Endet der Flug oder
/// die App in diesem Fenster (durchaus realistisch bei zwei Stunden,
/// anders als bei Sekunden), ist die Korrektur ersatzlos weg — genau der
/// Fall, den diese ganze Rueckzugs-Meldung eigentlich verhindern soll.
/// Die Loesung ist NICHT nochmal eine andere Zahl, sondern ein anderes
/// Prinzip: ab hier wird nicht mehr GEDECKELT (was bei einer echten
/// langen Sperre sofort wieder scheitern wuerde, Runde-8-Fehler), sondern
/// AUFGEGEBEN — ehrliches Eingestehen "diese Korrektur lohnt den
/// Prozess-Ueberlebens-Zeitraum nicht mehr", statt eine Zahl zu raten,
/// die gleichzeitig kurz genug fuer die Prozesslebensdauer und lang genug
/// fuer jede legitime Sperre sein muesste — ein Widerspruch, den keine
/// einzelne Konstante aufloesen kann, solange der Task nicht persistiert.
/// Eine Minute deckt jede realistische, kurzlebige Drosselung eines
/// internen Custom-Field-Endpunkts ab; eine Sperre, die laenger dauert,
/// ist ohnehin ein Fall fuer eine dauerhafte Warteschlange, nicht fuer
/// einen abgetrennten In-Memory-Task — deren Bau bleibt aus der
/// Runde-7/9-Abwaegung heraus bewusst ausserhalb dieses Umfangs (reines
/// Live-Anzeigefeld, kein kritischer Datenpfad).
const RATE_LIMIT_WARTEZEIT_OBERGRENZE_SEC: u64 = 60;

/// Wie lange VOR dem naechsten Versuch gewartet werden soll, nachdem
/// dieser Versuch mit `fehler` gescheitert ist — `None` heisst „kein
/// weiterer Versuch" (harter, nicht-transienter Fehler).
///
/// ⚠ v1.7.17 Runde 8 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): als EIGENE, reine Funktion ausgelagert, weil genau diese
/// Entscheidung (insbesondere: `RateLimited` mit der VOM SERVER
/// diktierten Wartezeit, nicht dem festen Schema) der Kern des
/// Runde-8-Befundes war und sich unabhaengig vom Netzwerk-Code pruefen
/// laesst.
///
/// ⚠ v1.7.17 Runde 9 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): die Servervorgabe wird jetzt gegen
/// `RATE_LIMIT_WARTEZEIT_OBERGRENZE_SEC` geprueft, statt ihr ungeprueft
/// zu vertrauen — siehe deren Doku.
///
/// ⚠ v1.7.17 Runde 11 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): NICHT mehr deckeln (Runde 9/10), sondern AUFGEBEN, wenn
/// die Servervorgabe die Obergrenze uebersteigt — siehe die ausfuehrliche
/// Begruendung an `RATE_LIMIT_WARTEZEIT_OBERGRENZE_SEC`. Ein Deckel haette
/// bei einer echten langen Sperre sofort wieder denselben Fehlschlag
/// provoziert (Runde 8); Aufgeben ist ehrlicher als eine Zahl vorzutaeuschen.
fn naechste_wartezeit_fuer_pirep_feld_retry(fehler: &ApiError, attempt: usize) -> Option<u64> {
    match fehler {
        ApiError::RateLimited {
            retry_after_seconds,
        } if *retry_after_seconds <= RATE_LIMIT_WARTEZEIT_OBERGRENZE_SEC => {
            Some(*retry_after_seconds)
        }
        ApiError::RateLimited { .. } => None,
        e if !is_transient_pirep_error(e) => None,
        _ => Some(
            PIREP_FELD_RETRY_BACKOFFS_SEC[attempt.min(PIREP_FELD_RETRY_BACKOFFS_SEC.len() - 1)],
        ),
    }
}

async fn post_pirep_fields_mit_kurzem_retry(
    client: &Client,
    pirep_id: &str,
    fields: &std::collections::HashMap<String, String>,
) {
    const ATTEMPTS: u32 = 3;
    let mut naechste_wartezeit_sec: Option<u64> = None;
    for attempt in 0..ATTEMPTS {
        if let Some(secs) = naechste_wartezeit_sec.take() {
            tracing::info!(
                pirep_id,
                attempt,
                backoff_sec = secs,
                "PIREP field post retry"
            );
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        }
        match client.post_pirep_fields(pirep_id, fields).await {
            Ok(()) => return,
            Err(e) => match naechste_wartezeit_fuer_pirep_feld_retry(&e, attempt as usize) {
                None => {
                    // ⚠ Zwei ganz verschiedene Gruende landen hier: ein
                    // harter, nicht-transienter Fehler ODER eine
                    // Rate-Limit-Sperre, die laenger dauert, als dieser
                    // abgetrennte, nicht persistente Task warten darf
                    // (Runde 11) — beide enden gleich: aufgeben, nicht
                    // raten.
                    tracing::warn!(pirep_id, error = %e, "pirep field post failed (non-transient or rate-limit too long, giving up)");
                    return;
                }
                Some(secs) => {
                    tracing::warn!(pirep_id, attempt, error = %e, wartezeit_sec = secs, "pirep field post attempt failed (transient)");
                    naechste_wartezeit_sec = Some(secs);
                }
            },
        }
    }
    tracing::warn!(pirep_id, "pirep field post exhausted retries (non-fatal)");
}

#[cfg(test)]
#[path = "tests_lib/pirep_feld_retry_tests.rs"]
mod pirep_feld_retry_tests;
