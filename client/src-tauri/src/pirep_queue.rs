    use super::*;
    use std::path::PathBuf;

    pub const DIR_NAME: &str = "pending_pireps";

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct QueuedPirep {
        pub pirep_id: String,
        /// v1.7.15 (N6): der offene Bahn-Nachtrag, beim Einreihen gebaut.
        /// Der Flug ist beim Nachreichen laengst verworfen; ohne dieses
        /// Feld erreichte die bessere Geometrie den Recorder nie.
        #[serde(default)]
        pub bahn_nachtrag: Option<serde_json::Value>,
        /// Die Landung, falls der Server sie noch nicht bestaetigt hat.
        ///
        /// Ohne sie war die Landung verloren, sobald das Einreichen
        /// scheiterte: Der Flug wird danach geschlossen, und der gemerkte
        /// Stand lebt nur im Arbeitsspeicher. Der Worker schickt sie vor
        /// dem Flugbericht — erst die Landung, dann der Bericht, sonst
        /// faellt das Urteil „keine Landung aufgezeichnet" (AAL 1331).
        ///
        /// `#[serde(default)]`: Eintraege aus aelteren Fassungen bleiben
        /// lesbar, sie tragen dann eben keine.
        #[serde(default)]
        pub landung: Option<serde_json::Value>,
        pub bid_id: i64,
        pub airline_icao: String,
        pub flight_number: String,
        pub dpt_airport: String,
        pub arr_airport: String,
        pub body: api_client::FileBody,
        pub queued_at: DateTime<Utc>,
        pub last_attempt_at: Option<DateTime<Utc>>,
        pub attempt_count: u32,
        pub last_error: Option<String>,
        /// v0.7.19 (QS-R2 Finding 2): phpVMS-Flight-ID als Fallback fuer
        /// `delete_bid` wenn der bid_id-Pfad 400/404/422 liefert. Leer-
        /// String fuer pre-v0.7.7-Resume-Files; wird beim Aufruf als
        /// None behandelt. #[serde(default)] damit alte queued-PIREPs
        /// (vor diesem Feld) weiter laden.
        #[serde(default)]
        pub flight_id: String,
        /// v0.12.5 (Spec v0.12.5-divert-and-manual-pirep.md, LE1b): das
        /// fertig gebaute MQTT-PIREP-Payload als JSON. Der Queue-Worker
        /// publisht das nach erfolgreichem phpVMS-`/file` via
        /// `finalize_filed_pirep` — vorher publizierte der Worker GAR
        /// KEIN MQTT-PIREP, gequeuete Diverts/Flüge fehlten dauerhaft in
        /// den VPS-Reports. `#[serde(default)]` = `Value::Null` für alte
        /// queued-Files (vor diesem Feld); der Worker behandelt Null als
        /// „kein MQTT-Publish" statt zu crashen.
        #[serde(default)]
        pub pirep_payload_json: serde_json::Value,
        /// v1.7.17 (Nachgereicht, externe Gegenpruefung Codex, adversarial):
        /// vorher gab der Worker nach `MAX_FAST_ATTEMPTS` (50, ~50 Minuten
        /// bei festem 60s-Takt) fuer immer und STILL auf — nur eine
        /// `tracing::warn!`-Zeile, nichts, was der Pilot je zu sehen bekam.
        /// Ein laengerer phpVMS-Ausfall verlor damit einen bereits
        /// geflogenen, fertigen PIREP dauerhaft und lautlos. Dieses Flag
        /// haelt fest, dass die EINMALIGE, sichtbare Warnung im Activity-
        /// Log schon geschrieben wurde — der Worker gibt seitdem nicht mehr
        /// auf, sondern versucht es nur noch in groesseren Abstaenden
        /// weiter (`SLOW_RETRY_INTERVAL_SECS`), siehe `retry_not_before`.
        #[serde(default)]
        pub dead_letter_notified: bool,
        /// v1.7.17 (Nachgereicht): fruehester Zeitpunkt fuer den naechsten
        /// Versuch — gesetzt entweder aus einem serverseitigen `Retry-After`
        /// (`ApiError::RateLimited`, gedeckelt auf `QUEUE_RATE_LIMIT_CAP_SECS`,
        /// um denselben Klasse von Fehler zu vermeiden, die die
        /// Departure-Gate-Rueckzugs-Meldung durchlaufen hat, siehe
        /// `RATE_LIMIT_WARTEZEIT_OBERGRENZE_SEC`) oder, nach dem
        /// Ueberschreiten von `MAX_FAST_ATTEMPTS`, aus dem langsameren
        /// Takt. `None` heisst: beim naechsten Tick sofort erneut
        /// versuchen (der normale, schnelle Anfangszustand).
        #[serde(default)]
        pub retry_not_before: Option<DateTime<Utc>>,
        /// Codex-Folgefund (adversarial, 05.09.2026, gegen den Fix eine
        /// Zeile ueber diesem hier): `pending_pireps/` ist ein einziges,
        /// GLOBALES Verzeichnis, ungebunden an einen Piloten. `phpvms_logout`
        /// erlaubt ausdruecklich, dass "ein anderer Pilot sich auf derselben
        /// Maschine anmeldet" (siehe dortiger Kommentar) — der Worker laeuft
        /// als Hintergrund-Task ueber die gesamte Prozesslaufzeit und
        /// benutzt bei jedem Tick den GERADE aktuellen `state.client`. Vor
        /// diesem Fix, mit der 50-Versuche-Grenze, war das Zeitfenster fuer
        /// „ein anderer Pilot ist schon eingeloggt, wenn der Eintrag drankommt"
        /// auf ~47 Minuten begrenzt; die Aenderung eine Zeile ueber diesem
        /// hier (unbegrenzte Slow-Phase) macht dasselbe Fenster potenziell
        /// beliebig lang. Ohne Eigentuemer-Pruefung haette der Worker Pilot
        /// As PIREP mit Pilot Bs Credentials eingereicht — im Erfolgsfall
        /// fehlzugeordnet, im 403/404-Fall (als "nicht transient"
        /// klassifiziert) sogar geloescht und damit Pilot As geflogener Flug
        /// dauerhaft verloren. Server-verifizierte Piloten-ID
        /// (`Profile.pilot_id`, siebte Runde — urspruenglich ein aus dem
        /// API-Key abgeleiteter Fingerabdruck, von Codex verworfen: eine
        /// legitime Key-Rotation DESSELBEN Piloten haette dessen eigene
        /// Eintraege dauerhaft in Quarantaene geschickt). `None` bei
        /// Alt-Eintraegen (vor diesem Feld geschrieben) UND wenn beim
        /// Einreihen keine authentifizierte Piloten-ID verfuegbar war —
        /// beide Faelle werden vom Worker als "Eigentuemer unbekannt"
        /// behandelt (Quarantaene: weder einreichen noch loeschen).
        #[serde(default)]
        pub owner_identity: Option<String>,
    }

    fn dir(app: &AppHandle) -> Option<PathBuf> {
        let base = app.path().app_data_dir().ok()?;
        let d = base.join(DIR_NAME);
        std::fs::create_dir_all(&d).ok()?;
        Some(d)
    }

    fn file_path(app: &AppHandle, pirep_id: &str) -> Option<PathBuf> {
        // pirep_id ist alphanumerisch in phpVMS — kein Sanitizing nötig,
        // aber wir bauen den Pfad sicher zusammen.
        let safe: String = pirep_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if safe.is_empty() {
            return None;
        }
        Some(dir(app)?.join(format!("{safe}.json")))
    }

    pub fn enqueue(app: &AppHandle, q: &QueuedPirep) -> Result<(), std::io::Error> {
        let path = file_path(app, &q.pirep_id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "no app_data_dir or invalid pirep_id",
            )
        })?;
        let json = serde_json::to_string_pretty(q)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        std::fs::write(path, json)
    }

    pub fn remove(app: &AppHandle, pirep_id: &str) {
        if let Some(path) = file_path(app, pirep_id) {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn list_all(app: &AppHandle) -> Vec<QueuedPirep> {
        let Some(d) = dir(app) else {
            return vec![];
        };
        let Ok(rd) = std::fs::read_dir(&d) else {
            return vec![];
        };
        let mut out = Vec::new();
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&p) else {
                continue;
            };
            if let Ok(q) = serde_json::from_str::<QueuedPirep>(&content) {
                out.push(q);
            }
        }
        out
    }

    // v0.7.13: `count(app)` entfernt — wurde fuer einen geplanten
    // "X Pireps in Queue"-Indicator vorgesehen, der nie ins UI ging.
    // `list(app)` reicht den Callern. Audit Q4-2026-05.
