/// Best-effort: drop the bid that was used for this flight. phpVMS does NOT
/// auto-consume bids on PIREP file, so without this the booked flight stays
/// in the pilot's bid list even after Accepted.
///
/// v0.7.19 GAF-707 (QS-R1 Finding 1): Bei transientem Fehler wird der
/// Bid-Drop in die persistente `PendingBidCleanupQueue` enqueued — der
/// Background-Worker retried das spaeter. So bleibt nach Accident-FILED
/// kein Aircraft + Bid auf phpVMS haengen wenn der delete_bid-Aufruf
/// transient versagt (Netz weg, 5xx). Spec §Pending Bid Cleanup Queue.
///
/// v0.7.19 (QS-R2 Finding 2): `flight_id` wird als Fallback mitgegeben.
/// Wenn `delete_bid(bid_id)` 400/422 liefert (VA-Plugin akzeptiert
/// `bid_id` nicht), versucht die `delete_bid`-Implementation den
/// `flight_id`-Body-Pfad; 404 heisst seit 13.11.2026 „schon weg". Die
/// Aufrufer hier sind nur `flight_end`/`flight_cancel` direkt nach
/// file/cancel des laufenden Flugs: Seit dessen Start konnte auf diesem
/// Flug kein zweiter eigener PIREP entstehen (der Flugstart uebernimmt einen
/// offenen eigenen PIREP desselben Flugs), und die Buchung ist die dieses
/// Flugs. Der nachgereichte PIREP (`queued_filed`) laeuft NICHT hier durch —
/// zwischen Start und Nachreichen kann viel Zeit liegen; er geht ueber die
/// Warteschlange und `bid_aufraeumen_ausfuehren`. Ohne den
/// Fallback haette der Background-Worker 8x denselben kaputten Pfad
/// retried. Empty-String-`flight_id` (pre-v0.7.7-Bid) wird zu None
/// gemapped.
/// v0.7.19 (QS-R3 Finding 1): Pure-function-Helper fuer
/// `consume_bid_best_effort` — entscheidet welche der zwei IDs an
/// `delete_bid` weitergegeben werden. Extrahiert damit der Path-
/// Selection-Logik unit-testbar bleibt ohne dass ein `Client` (HTTP-
/// I/O) im Test-Path noetig ist.
///
/// Returns:
/// - `None` wenn beide IDs fehlen (Caller skipt komplett).
/// - `Some((bid_id_opt, flight_id_opt))` sonst — leerer flight_id-
///   String wird als None behandelt (pre-v0.7.7-Resume-Files).
pub(crate) fn resolve_bid_cleanup_args(
    bid_id: i64,
    flight_id: Option<&str>,
) -> Option<(Option<i64>, Option<String>)> {
    let flight_id_opt = flight_id.filter(|s| !s.is_empty()).map(|s| s.to_string());
    if bid_id <= 0 && flight_id_opt.is_none() {
        return None;
    }
    let bid_id_opt = if bid_id > 0 { Some(bid_id) } else { None };
    Some((bid_id_opt, flight_id_opt))
}

async fn consume_bid_best_effort(
    app: &AppHandle,
    client: &Client,
    pirep_id: &str,
    bid_id: i64,
    flight_id: Option<&str>,
    reason: &str,
    owner_identity: Option<String>,
) {
    // v0.7.19 (QS-R3 Finding 1): nur skippen wenn BEIDE IDs fehlen.
    // Frueher: `if bid_id <= 0 { return; }` — hat den flight_id-only-
    // Pfad still verschluckt. Im Orphan-Cleanup-/Accident-Filing-Pfad
    // kann es legitim vorkommen dass nur die phpVMS-flight_id bekannt
    // ist (z. B. bei VAs ohne separates bid_id-Feld).
    let Some((bid_id_opt, flight_id_owned)) = resolve_bid_cleanup_args(bid_id, flight_id) else {
        return;
    };
    let flight_id_opt = flight_id_owned.as_deref();
    match client.delete_bid(bid_id_opt, flight_id_opt).await {
        Ok(()) => tracing::info!(
            ?bid_id_opt,
            ?flight_id_opt,
            "bid removed after PIREP filing"
        ),
        Err(e) => {
            tracing::warn!(
                ?bid_id_opt,
                ?flight_id_opt,
                pirep_id,
                error = %e,
                "delete_bid failed — enqueueing for pending_bid_cleanup retry"
            );
            enqueue_pending_bid_cleanup(
                app,
                pirep_id,
                bid_id_opt,
                flight_id_owned.clone(),
                reason,
                owner_identity,
            )
            .await;
        }
    }
}

/// Nachgereichter PIREP (`queued_filed`): Buchung nicht direkt loeschen,
/// sondern einreihen — der Drain im selben Tick prueft dann Entscheidung,
/// laufende Fluege und Server, bevor er loescht (Befund 13.11.2026).
async fn bid_nach_nachreichen_einreihen(
    app: &AppHandle,
    pirep_id: &str,
    bid_id: i64,
    flight_id: Option<&str>,
    owner_identity: Option<String>,
) {
    if let Some((bid_id, flight_id)) = resolve_bid_cleanup_args(bid_id, flight_id) {
        enqueue_pending_bid_cleanup(app, pirep_id, bid_id, flight_id, "queued_filed", owner_identity)
            .await;
    }
}

/// v0.7.19 GAF-707: Helper-Wrapper um PendingBidCleanupQueue::enqueue
/// damit Caller nicht das app_data_dir aufloesen muessen. Schluckt
/// alle I/O-Fehler (warn-log) — diese Queue ist ein Best-Effort-Backup
/// fuer den primaeren delete_bid-Pfad und darf NIE den File-Flow
/// brechen.
async fn enqueue_pending_bid_cleanup(
    app: &AppHandle,
    pirep_id: &str,
    bid_id: Option<i64>,
    flight_id: Option<String>,
    reason: &str,
    owner_identity: Option<String>,
) {
    let Ok(dir) = app.path().app_data_dir() else {
        tracing::warn!(
            pirep_id,
            "pending_bid_cleanup: konnte app_data_dir nicht aufloesen — Eintrag verloren"
        );
        return;
    };
    let queue = match storage::PendingBidCleanupQueue::open(dir) {
        Ok(q) => q,
        Err(e) => {
            tracing::warn!(
                pirep_id,
                error = %e,
                "pending_bid_cleanup: konnte Queue nicht oeffnen"
            );
            return;
        }
    };
    let entry = storage::PendingBidCleanup {
        pirep_id: pirep_id.to_string(),
        bid_id,
        flight_id,
        reason: reason.to_string(),
        created_at: Utc::now(),
        last_attempt_at: None,
        attempts: 0,
        owner_identity,
    };
    // Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): dasselbe
    // Schloss wie `drain_pending_bid_cleanup` — beide machen ein Lese-
    // Aendere-Schreibe auf dieselbe Datei, ohne dieses Schloss haette der
    // Worker mitten zwischen seinem eigenen `read_all()` und `replace()`
    // genau diesen neuen Eintrag wieder verlieren koennen.
    let state = app.state::<AppState>();
    let _lock = state.pending_bid_cleanup_lock.lock().await;
    if let Err(e) = queue.enqueue(entry) {
        tracing::warn!(
            pirep_id,
            error = %e,
            "pending_bid_cleanup: enqueue fehlgeschlagen"
        );
    } else {
        tracing::info!(
            pirep_id,
            bid_id = ?bid_id,
            reason,
            "pending_bid_cleanup: Eintrag persistiert fuer Background-Retry"
        );
    }
}

/// Was mit einem Aufraeum-Eintrag passiert, der seine Versuche ausgereizt hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AusgereizterBidEintrag {
    /// Der Bid existiert im eigenen Konto nicht mehr — Ziel erreicht.
    Verwerfen,
    /// Bid noch da, fremder Eintrag oder Bid-Liste nicht abrufbar.
    Behalten,
}

/// Entscheidet ueber einen ausgereizten `pending_bid_cleanup`-Eintrag.
///
/// `eigene_bids` ist `None`, wenn der Abruf der eigenen Bids gescheitert
/// ist — dann wird NICHTS verworfen, ein Netzfehler darf die Warteschlange
/// nicht leeren. Ein Eintrag, der nachweislich einem ANDEREN Konto gehoert,
/// bleibt in Quarantaene: die Bid-Liste zeigt nur das aktuelle Konto und
/// sagt ueber seinen Bid nichts aus. Ohne `bid_id` entscheidet die
/// `flight_id` — hier nur als Grund zum BEHALTEN, nie als Eigentumsbeweis.
fn ausgereizter_bid_eintrag(
    eintrag_eigentuemer: Option<&str>,
    aktuelle_identitaet: &str,
    bid_id: Option<i64>,
    flight_id: Option<&str>,
    eigene_bids: Option<&[(i64, String)]>,
) -> AusgereizterBidEintrag {
    let Some(bids) = eigene_bids else {
        return AusgereizterBidEintrag::Behalten;
    };
    if eintrag_eigentuemer.is_some()
        && !pirep_queue_eintrag_gehoert_aktuellem_piloten(eintrag_eigentuemer, aktuelle_identitaet)
    {
        return AusgereizterBidEintrag::Behalten;
    }
    let noch_vorhanden = match (bid_id, flight_id) {
        (Some(id), _) => bids.iter().any(|(b_id, _)| *b_id == id),
        (None, Some(flug)) => bids.iter().any(|(_, b_flug)| b_flug == flug),
        (None, None) => false,
    };
    if noch_vorhanden {
        AusgereizterBidEintrag::Behalten
    } else {
        AusgereizterBidEintrag::Verwerfen
    }
}

/// Was ein Aufraeumversuch fuer die Buchung eines ALTEN Flugs tun darf.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BidAufraeumen {
    /// `delete_bid` mit genau diesen Argumenten; `flight_id` nur, wenn auf
    /// dem Flug keine fremde (neuere) eigene Buchung liegt.
    Loeschen(Option<i64>, Option<String>),
    /// Die alte Buchung existiert nicht mehr — Ziel erreicht.
    Erledigt,
    /// Nicht sicher entscheidbar, nichts loeschen.
    Liegenlassen,
}

/// Entscheidet, ob und wie die Buchung eines alten PIREP geloescht werden
/// darf, ohne eine neuere Buchung desselben Flugs zu treffen (Befund
/// 13.11.2026). phpVMS `removeBid()` loescht ALLE Buchungen des Piloten auf
/// der `flight_id`, und ab GSG CoreFixes 1.12.6 bricht der Server dabei einen
/// laufenden PIREP am Boden auf diesem Flug ab.
///
/// Tragend: `BidService::addBid` gibt eine vorhandene eigene Buchung zurueck
/// statt eine zweite anzulegen — je Pilot und Flug gibt es hoechstens eine.
/// Mit `bid_id` reicht deshalb der Abgleich der ID: liegt auf dem Flug eine
/// eigene Buchung mit ANDERER ID, ist das die neue und die alte ist weg.
/// Ohne `bid_id` entscheidet die Anlagezeit der Buchung gegen die des alten
/// PIREP (`pirep_erstellt`, beides Serveruhr): nur eine VOR dem PIREP
/// angelegte Buchung ist die alte. `eigene_bids` ist `None`, wenn der Abruf
/// scheiterte — dann nie ueber `flight_id`.
fn bid_aufraeum_entscheidung(
    bid_id: Option<i64>,
    flight_id: Option<&str>,
    eigene_bids: Option<&[api_client::Bid]>,
    pirep_erstellt: Option<&str>,
) -> BidAufraeumen {
    let zeit = |s: Option<&str>| {
        s.and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc))
    };
    let Some(bids) = eigene_bids else {
        return match bid_id {
            Some(id) => BidAufraeumen::Loeschen(Some(id), None),
            None => BidAufraeumen::Liegenlassen,
        };
    };
    let auf_flug: Vec<&api_client::Bid> = match flight_id {
        Some(f) => bids.iter().filter(|b| b.flight_id == f).collect(),
        None => Vec::new(),
    };
    match bid_id {
        Some(id) if bids.iter().any(|b| b.id == id) => {
            let rueckfall = auf_flug.iter().all(|b| b.id == id);
            BidAufraeumen::Loeschen(
                Some(id),
                flight_id.filter(|_| rueckfall).map(str::to_string),
            )
        }
        Some(_) => BidAufraeumen::Erledigt,
        None => {
            if auf_flug.is_empty() {
                return BidAufraeumen::Erledigt;
            }
            let Some(pirep) = zeit(pirep_erstellt) else {
                return BidAufraeumen::Liegenlassen;
            };
            let anlage: Vec<Option<_>> =
                auf_flug.iter().map(|b| zeit(b.created_at.as_deref())).collect();
            match anlage.as_slice() {
                [Some(t)] if *t < pirep => BidAufraeumen::Loeschen(
                    Some(auf_flug[0].id),
                    flight_id.map(str::to_string),
                ),
                a if a.iter().all(|t| t.is_some_and(|t| t >= pirep)) => BidAufraeumen::Erledigt,
                _ => BidAufraeumen::Liegenlassen,
            }
        }
    }
}

/// Gehoert die Buchung zu einem laufenden Flug (aktiv oder Resume-Kandidat
/// auf der Platte), der nicht `pirep_id` ist? Dann darf sie kein Aufraeumen
/// fuer einen alten PIREP loeschen — auch nicht per `bid_id`: der Pilot
/// kann denselben Flug mit derselben, nie geloeschten Buchung neu gestartet
/// haben, und CoreFixes 1.12.6 braeche dessen PIREP ab.
fn bid_gehoert_laufendem_flug(
    app: &AppHandle,
    pirep_id: &str,
    bid_id: Option<i64>,
    flight_id: Option<&str>,
) -> bool {
    let aktiv = {
        let state = app.state::<AppState>();
        let guard = state.active_flight.lock().expect("active_flight lock");
        guard.as_ref().is_some_and(|f| {
            bid_trifft_laufenden_flug(pirep_id, bid_id, flight_id, (&f.pirep_id, f.bid_id, &f.flight_id))
        })
    };
    aktiv
        || read_persisted_flight(app).is_some_and(|p| {
            bid_trifft_laufenden_flug(pirep_id, bid_id, flight_id, (&p.pirep_id, p.bid_id, &p.flight_id))
        })
}

/// Reiner Vergleich fuer `bid_gehoert_laufendem_flug`: `laufend` ist
/// (pirep_id, bid_id, flight_id) des laufenden Flugs.
fn bid_trifft_laufenden_flug(
    pirep_id: &str,
    bid_id: Option<i64>,
    flight_id: Option<&str>,
    laufend: (&str, i64, &str),
) -> bool {
    let (l_pirep, l_bid, l_flug) = laufend;
    l_pirep != pirep_id
        && (bid_id.is_some_and(|id| id > 0 && id == l_bid)
            || flight_id.is_some_and(|f| !f.is_empty() && f == l_flug))
}

/// Lokale Sperre fuer die Hintergrund-/Panel-Wege (Drain, Orphan): laufender
/// Flug wie `bid_gehoert_laufendem_flug`, dazu ein gerade startender Flug
/// (`flight_setup_in_progress`). Der Start haelt die Sperre vom Bid-Abruf bis
/// nach dem prefile — davor kennt der Server den neuen PIREP noch nicht, und
/// die Buchung, die der Start gerade benutzt, darf nicht verschwinden. Nur
/// gelesen, nie geholt (sonst scheiterte ein Flugstart daran), und NICHT in
/// `consume_bid_best_effort`: `flight_end`/`flight_cancel` halten die Sperre
/// selbst und wuerden sich aussperren.
fn bid_aufraeumen_lokal_gesperrt(
    app: &AppHandle,
    pirep_id: &str,
    bid_id: Option<i64>,
    flight_id: Option<&str>,
) -> bool {
    app.state::<AppState>().flight_setup_in_progress.load(Ordering::SeqCst)
        || bid_gehoert_laufendem_flug(app, pirep_id, bid_id, flight_id)
}

/// Ergebnis von `bid_aufraeumen_ausfuehren`.
#[derive(Debug)]
enum BidAufraeumErgebnis {
    /// `delete_bid` ging durch, mit dieser gesendeten `bid_id`.
    Geloescht(Option<i64>),
    /// Die alte Buchung existiert nicht mehr (Entscheidung oder 404).
    Erledigt,
    /// Alte Buchung nicht sicher bestimmbar — nichts geloescht.
    Liegenlassen,
    /// Ein anderer eigener Flug laeuft auf der Buchung/dem Flug (lokal oder
    /// laut Server) — nichts geloescht.
    LaufenderFlug,
    /// Die laufenden PIREPs liessen sich nicht abfragen — nichts geloescht.
    ServerUnpruefbar,
    /// `delete_bid` scheiterte.
    Fehlgeschlagen(api_client::ApiError),
}

/// Laeuft laut Server ein ANDERER eigener PIREP auf einem Flug, den das
/// Loeschen trifft? GSG CoreFixes 1.12.6 bricht beim Loeschen einer Buchung
/// jeden IN_PROGRESS-PIREP des Piloten auf deren `flight_id` ab. `PirepSummary`
/// kennt keine `bid_id`, deshalb zaehlt der Flug. Im Zweifel sperren: ist
/// kein Flug bekannt, sperrt jeder andere laufende PIREP; ein PIREP ohne
/// `flight_id` in der Antwort ebenso.
fn server_pirep_sperrt(laufende: &[api_client::PirepSummary], pirep_id: &str, fluege: &[&str]) -> bool {
    laufende.iter().any(|p| {
        p.id != pirep_id
            && (fluege.is_empty()
                || p.flight_id.as_deref().is_none_or(|f| fluege.contains(&f)))
    })
}

/// Raeumt die Buchung eines ALTEN PIREP auf — die einzige Stelle, an der
/// Drain und Orphan-Weg `delete_bid` rufen (Befund 13.11.2026):
/// 1. `bid_aufraeum_entscheidung` (keine neuere Buchung treffen),
/// 2. `lokal_laufend` (laufender oder startender Flug auf diesem Geraet),
/// 3. unmittelbar vor dem DELETE die eigenen IN_PROGRESS-PIREPs vom Server —
///    deckt das zweite Geraet und das Fenster zwischen prefile und
///    `save_active_flight` ab. Scheitert die Abfrage: nicht loeschen.
///
/// Zwischen Abfrage und DELETE bleibt ein Fenster von einer Anfragedauer,
/// das sich clientseitig nicht schliessen laesst.
async fn bid_aufraeumen_ausfuehren(
    client: &Client,
    pirep_id: &str,
    bid_id: Option<i64>,
    flight_id: Option<&str>,
    eigene_bids: Option<&[api_client::Bid]>,
    pirep_erstellt: Option<&str>,
    lokal_laufend: impl Fn() -> bool,
) -> BidAufraeumErgebnis {
    let (bid_arg, flight_arg) =
        match bid_aufraeum_entscheidung(bid_id, flight_id, eigene_bids, pirep_erstellt) {
            BidAufraeumen::Loeschen(b, f) => (b, f),
            BidAufraeumen::Erledigt => return BidAufraeumErgebnis::Erledigt,
            BidAufraeumen::Liegenlassen => return BidAufraeumErgebnis::Liegenlassen,
        };
    if lokal_laufend() {
        return BidAufraeumErgebnis::LaufenderFlug;
    }
    let mut fluege: Vec<&str> = [flight_arg.as_deref(), flight_id]
        .into_iter()
        .flatten()
        .filter(|f| !f.is_empty())
        .collect();
    if let (Some(id), Some(bids)) = (bid_arg, eigene_bids) {
        fluege.extend(bids.iter().filter(|b| b.id == id).map(|b| b.flight_id.as_str()));
    }
    match client.get_user_pireps_in_progress().await {
        Ok(laufende) if server_pirep_sperrt(&laufende, pirep_id, &fluege) => {
            return BidAufraeumErgebnis::LaufenderFlug;
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(pirep_id, error = %e, "Bid-Aufraeumen: laufende PIREPs nicht abfragbar — nichts geloescht");
            return BidAufraeumErgebnis::ServerUnpruefbar;
        }
    }
    match client.delete_bid(bid_arg, flight_arg.as_deref()).await {
        Ok(()) => BidAufraeumErgebnis::Geloescht(bid_arg),
        Err(api_client::ApiError::NotFound) => BidAufraeumErgebnis::Erledigt,
        Err(e) => BidAufraeumErgebnis::Fehlgeschlagen(e),
    }
}

/// Was der Drain mit einem Eintrag nach `bid_aufraeumen_ausfuehren` tut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrainFolge {
    /// Ziel erreicht, Eintrag entfernen.
    Entfernen,
    /// Bleibt, ohne dass ein Versuch zaehlt (laufender Flug).
    WartetOhneVersuch,
    /// Bleibt, ein Versuch zaehlt.
    VersuchZaehlt,
}

fn drain_folge(ergebnis: &BidAufraeumErgebnis) -> DrainFolge {
    match ergebnis {
        BidAufraeumErgebnis::Geloescht(_) | BidAufraeumErgebnis::Erledigt => DrainFolge::Entfernen,
        BidAufraeumErgebnis::LaufenderFlug => DrainFolge::WartetOhneVersuch,
        BidAufraeumErgebnis::Liegenlassen
        | BidAufraeumErgebnis::ServerUnpruefbar
        | BidAufraeumErgebnis::Fehlgeschlagen(_) => DrainFolge::VersuchZaehlt,
    }
}

/// v0.7.19 GAF-707: drain-Pass fuer den PendingBidCleanupQueue. Wird im
/// spawn_pirep_queue_worker-Tick mit aufgerufen (selbe 60s-Frequenz).
/// Cap auf 8 Versuche pro Eintrag. Danach wird der Eintrag einmal pro Tick
/// gegen die eigenen Bids gehalten und verworfen, sobald der Bid nicht mehr
/// existiert (siehe `ausgereizter_bid_eintrag`).
async fn drain_pending_bid_cleanup(app: &AppHandle, client: &Client, aktuelle_identitaet: &str) {
    const MAX_ATTEMPTS: u32 = 8;
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let Ok(queue) = storage::PendingBidCleanupQueue::open(dir) else {
        return;
    };
    // Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): ueber den
    // ganzen read_all() -> verarbeiten -> replace()-Zyklus gehalten
    // (dasselbe Schloss wie `enqueue_pending_bid_cleanup`) — sonst haette
    // ein `enqueue` GENAU in dieser Luecke den frisch hinzugekommenen
    // Eintrag beim `replace(&survivors)` wieder verloren. `tokio::sync::
    // Mutex` statt `std::sync::Mutex`, weil der Guard hier ueber mehrere
    // echte Awaits (`get_bids`, `delete_bid`) hinweg gehalten wird.
    let state = app.state::<AppState>();
    let _lock = state.pending_bid_cleanup_lock.lock().await;
    let entries = match queue.read_all() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "pending_bid_cleanup drain: read_all failed");
            return;
        }
    };
    if entries.is_empty() {
        return;
    }
    tracing::info!(count = entries.len(), "pending_bid_cleanup drain tick");

    let mut survivors: Vec<storage::PendingBidCleanup> = Vec::new();
    // Lazily geholt (hoechstens einmal pro Tick) und nur wenn ueberhaupt ein
    // Eintrag ohne passenden Eigentuemer oder ein ausgereizter Eintrag
    // auftaucht.
    // Aeusseres `None` = noch nicht gefragt, inneres `None` = Abruf
    // gescheitert. Die Reklamierung unten behandelt einen gescheiterten
    // Abruf wie eine leere Liste; der ausgereizte Zweig darf das NICHT —
    // sonst wuerde ein Netzfehler alle Eintraege verwerfen.
    let mut eigene_bids: Option<Option<Vec<api_client::Bid>>> = None;
    for mut e in entries {
        if e.attempts >= MAX_ATTEMPTS {
            // Befund 21.09.2026 (Diagnose-Logs): bei Peter lagen vier
            // Eintraege aus Juni/Juli, deren PIREPs laengst angenommen und
            // deren Bids laengst weg waren. Sie warnten jede Minute, fuer
            // immer — die „orphan-cleanup UI", auf die die Meldung verwies,
            // gibt es im Frontend nicht. Ein ausgereizter Eintrag wird jetzt
            // einmal gegen die eigenen Bids gehalten: existiert der Bid
            // nicht mehr, ist das Ziel des Eintrags erreicht.
            let fremder_eintrag = e.owner_identity.is_some()
                && !pirep_queue_eintrag_gehoert_aktuellem_piloten(
                    e.owner_identity.as_deref(),
                    aktuelle_identitaet,
                );
            if eigene_bids.is_none() && !fremder_eintrag {
                eigene_bids = Some(client.get_bids().await.ok());
            }
            let bid_liste: Option<Vec<(i64, String)>> = eigene_bids
                .as_ref()
                .and_then(|abruf| abruf.as_ref())
                .map(|bids| bids.iter().map(|b| (b.id, b.flight_id.clone())).collect());
            match ausgereizter_bid_eintrag(
                e.owner_identity.as_deref(),
                aktuelle_identitaet,
                e.bid_id,
                e.flight_id.as_deref(),
                bid_liste.as_deref(),
            ) {
                AusgereizterBidEintrag::Verwerfen => {
                    tracing::info!(
                        pirep_id = %e.pirep_id,
                        attempts = e.attempts,
                        "pending_bid_cleanup: Bid existiert nicht mehr — Eintrag entfernt"
                    );
                }
                AusgereizterBidEintrag::Behalten => {
                    tracing::debug!(
                        pirep_id = %e.pirep_id,
                        attempts = e.attempts,
                        "pending_bid_cleanup: ausgereizt, Bid noch vorhanden oder nicht pruefbar — bleibt liegen"
                    );
                    survivors.push(e);
                }
            }
            continue;
        }
        // Codex-Folgefund (adversarial, 05.09.2026, neunte Runde): diese
        // Warteschlange hatte GAR KEINE Kontobindung — jeder Eintrag wurde
        // mit dem gerade angemeldeten `client` verarbeitet, unabhaengig
        // davon wer ihn eingereiht hatte. Gleiches Muster wie beim
        // Haupt-PIREP-Queue (siehe `pirep_queue_eintrag_gehoert_aktuellem_
        // piloten`): `None` heisst „unbekannt", nicht „frei" — vor der
        // endgueltigen Quarantaene EINMAL serverseitig nachfragen
        // (`GET /api/user/bids`, serverseitig auf den eingeloggten Piloten
        // gefiltert), ob der Bid TROTZDEM dem aktuellen Account gehoert
        // (Alt-Eintrag von vor diesem Feld, oder API-Key-Rotation).
        if !pirep_queue_eintrag_gehoert_aktuellem_piloten(
            e.owner_identity.as_deref(),
            aktuelle_identitaet,
        ) {
            if eigene_bids.is_none() {
                eigene_bids = Some(client.get_bids().await.ok());
            }
            // Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): NUR
            // `bid_id` beweist Eigentum — `flight_id` bezeichnet den
            // GEPLANTEN Flug, nicht den Bid, und mehrere Piloten koennen
            // legitim je einen eigenen Bid auf DENSELBEN Flug haben. Ein
            // `flight_id`-Treffer in `eigene_bids` haette Pilot As
            // Cleanup-Eintrag faelschlich Pilot B zuschreiben koennen, nur
            // weil B zufaellig ebenfalls einen Bid fuer dieselbe Flugnummer
            // haelt — mit anschliessendem `delete_bid` waere DANN Bs
            // eigener, unbeteiligter Bid geloescht worden. `flight_id`
            // bleibt weiterhin ein gueltiger FALLBACK fuer den eigentlichen
            // `delete_bid`-Aufruf selbst (dort serverseitig ohnehin auf den
            // aufrufenden Account beschraenkt) — nur nicht als Beweis HIER.
            let gehoert_doch_dem_aktuellen_account = e.bid_id.is_some_and(|bid_id| {
                eigene_bids
                    .as_ref()
                    .and_then(|abruf| abruf.as_ref())
                    .is_some_and(|bids| bids.iter().any(|b| b.id == bid_id))
            });
            if gehoert_doch_dem_aktuellen_account {
                tracing::info!(
                    pirep_id = %e.pirep_id,
                    "pending_bid_cleanup: serverseitig als eigener Bid bestaetigt — reklamiert"
                );
                e.owner_identity = Some(aktuelle_identitaet.to_string());
            } else {
                tracing::warn!(
                    pirep_id = %e.pirep_id,
                    eigentuemer_bekannt = e.owner_identity.is_some(),
                    "pending_bid_cleanup: gehoert nicht dem aktuell eingeloggten Account — \
                     bleibt unangetastet in Quarantaene"
                );
                survivors.push(e);
                continue;
            }
        }
        // Befund 13.11.2026: Entscheidung, lokale Sperre und Server-Abfrage
        // laufen in `bid_aufraeumen_ausfuehren` unmittelbar vor dem DELETE.
        // 404 auf `delete_bid` heisst dort „schon weg" (Befund 21.09.2026:
        // vorher zaehlte das als Fehlschlag, bis der Eintrag ausgereizt war).
        if eigene_bids.is_none() {
            eigene_bids = Some(client.get_bids().await.ok());
        }
        let pirep_erstellt = if e.bid_id.is_none() {
            client.get_pirep(&e.pirep_id).await.ok().and_then(|p| p.created_at)
        } else {
            None
        };
        let ergebnis = bid_aufraeumen_ausfuehren(
            client,
            &e.pirep_id,
            e.bid_id,
            e.flight_id.as_deref(),
            eigene_bids.as_ref().and_then(|abruf| abruf.as_deref()),
            pirep_erstellt.as_deref(),
            || bid_aufraeumen_lokal_gesperrt(app, &e.pirep_id, e.bid_id, e.flight_id.as_deref()),
        )
        .await;
        match drain_folge(&ergebnis) {
            DrainFolge::Entfernen => {
                tracing::info!(
                    pirep_id = %e.pirep_id,
                    reason = %e.reason,
                    ?ergebnis,
                    "pending_bid_cleanup: alte Buchung geloescht oder schon weg — Eintrag entfernt"
                );
            }
            DrainFolge::WartetOhneVersuch => {
                tracing::debug!(
                    pirep_id = %e.pirep_id,
                    "pending_bid_cleanup: auf Buchung/Flug laeuft ein anderer Flug — wartet"
                );
                survivors.push(e);
            }
            DrainFolge::VersuchZaehlt => {
                e.attempts += 1;
                e.last_attempt_at = Some(Utc::now());
                tracing::warn!(
                    pirep_id = %e.pirep_id,
                    attempt = e.attempts,
                    ?ergebnis,
                    "pending_bid_cleanup: nichts geloescht — bleibt in Queue"
                );
                survivors.push(e);
            }
        }
    }
    if let Err(e) = queue.replace(&survivors) {
        tracing::warn!(error = %e, "pending_bid_cleanup drain: queue write failed");
    }
}

#[cfg(test)]
#[path = "tests_lib/ausgereizter_bid_eintrag_tests.rs"]
mod ausgereizter_bid_eintrag_tests;

#[cfg(test)]
#[path = "tests_lib/bid_aufraeum_entscheidung_tests.rs"]
mod bid_aufraeum_entscheidung_tests;
