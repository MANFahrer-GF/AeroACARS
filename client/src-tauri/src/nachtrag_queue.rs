    use super::*;
    use std::path::{Path, PathBuf};

    const DIR_NAME: &str = "bahn_nachtraege";
    /// Einreihen 30 s + Publish 20 s + Luft.
    const ZUSTELL_FRIST: std::time::Duration = std::time::Duration::from_secs(75);

    #[derive(Serialize, Deserialize)]
    struct Ablage {
        va_prefix: String,
        pilot_id: String,
        nachtrag: serde_json::Value,
    }

    fn sicher(s: &str) -> String {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect()
    }

    fn wurzel(app: &AppHandle) -> Option<PathBuf> {
        app.path().app_data_dir().ok().map(|b| b.join(DIR_NAME))
    }

    fn dir_in(base: &Path, va: &str, pilot: &str) -> Option<PathBuf> {
        let (va, pilot) = (sicher(va), sicher(pilot));
        if va.is_empty() || pilot.is_empty() {
            return None;
        }
        Some(base.join(va).join(pilot))
    }

    /// Dateiname aus `pirep_id` + Aufsetzzeit + REVISION — reine Funktion.
    ///
    /// Die Revision im Namen (Runde 14, High 2): Ein PUBACK loescht genau
    /// die Datei, die gesendet wurde. Eine neuere Revision ist eine andere
    /// Datei — der Streamer kann sie nicht mehr unter dem Leser des
    /// ACK-Tasks wegtauschen. Nullgefuellt, damit die Reihenfolge im Ordner
    /// die Revisionsreihenfolge ist (der Worker schickt aelteste zuerst; der
    /// Recorder weist Ueberholtes ueber den Riegel ab).
    pub fn dateiname(pirep_id: &str, touchdown_at: i64, revision: Option<u32>) -> Option<String> {
        let safe = sicher(pirep_id);
        if safe.is_empty() {
            return None;
        }
        Some(format!(
            "{safe}-{touchdown_at}-r{:06}.json",
            revision.unwrap_or(0)
        ))
    }

    fn io_err(msg: impl Into<String>) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::Other, msg.into())
    }

    pub fn serialisieren(
        va: &str,
        pilot: &str,
        nachtrag: &aeroacars_mqtt::TouchdownRolloutFinalizedPayload,
    ) -> Result<String, std::io::Error> {
        let ablage = Ablage {
            va_prefix: va.to_string(),
            pilot_id: pilot.to_string(),
            nachtrag: serde_json::to_value(nachtrag).map_err(|e| io_err(e.to_string()))?,
        };
        serde_json::to_string_pretty(&ablage).map_err(|e| io_err(e.to_string()))
    }

    /// Liest eine Ablage-Datei — ueber `aus_json`, nie ueber `from_value`
    /// (flatten liest einen fehlenden Block sonst als Some(default)) — und
    /// nur, wenn die Identitaet IN der Datei zur erwarteten passt.
    pub fn lesen(
        text: &str,
        va: &str,
        pilot: &str,
    ) -> Result<aeroacars_mqtt::TouchdownRolloutFinalizedPayload, String> {
        let ablage: Ablage = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if ablage.va_prefix != va || ablage.pilot_id != pilot {
            return Err(format!(
                "Nachtrag gehoert {}/{}, nicht {va}/{pilot}",
                ablage.va_prefix, ablage.pilot_id
            ));
        }
        aeroacars_mqtt::TouchdownRolloutFinalizedPayload::aus_json(ablage.nachtrag)
            .map_err(|e| e.to_string())
    }

    pub fn enqueue_in(
        base: &Path,
        va: &str,
        pilot: &str,
        nachtrag: &aeroacars_mqtt::TouchdownRolloutFinalizedPayload,
    ) -> Result<PathBuf, std::io::Error> {
        let d = dir_in(base, va, pilot).ok_or_else(|| io_err("Identitaet leer"))?;
        std::fs::create_dir_all(&d)?;
        let name = dateiname(
            &nachtrag.pirep_id,
            nachtrag.touchdown_at,
            nachtrag.herkunft.bahn_revision,
        )
        .ok_or_else(|| io_err("pirep_id unbrauchbar"))?;
        let path = d.join(name);
        // Atomar: erst die Temp-Datei, dann `rename` (ersetzt auf allen
        // drei Plattformen). Ein `fs::write` auf die Zieldatei liess einen
        // Leser zwischendurch eine halbe Datei sehen — und `None` aus
        // `revision_in_datei` war frueher ein Loeschgrund (Runde 13, High 3).
        let tmp = d.join(format!(
            ".{}.tmp",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("nachtrag")
        ));
        std::fs::write(&tmp, serialisieren(va, pilot, nachtrag)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }

    /// Legt den Nachtrag ab UND schreibt ihn ins lokale JSONL-Flight-Log.
    ///
    /// Laut Doku bei `senden` unten laufen ALLE VIER Aufrufer (Streamer,
    /// Einreichen mit/ohne Handle, gequeuter Nachtrag) durch diese
    /// Funktion — der eine Punkt, an dem der Nachtrag lokal aufgezeichnet
    /// werden kann, ohne dass MQTT dafuer erreichbar sein muss. Vorher
    /// ging dieses Event NUR ueber die durable MQTT-Retry-Queue raus —
    /// ein Flug, der nie live ankam (Gap-Fill-Import), hatte die
    /// Ausroll-Feinwerte deshalb STRUKTURELL nie im JSONL (ITY 1358,
    /// 07.09.2026). `record_event` ist best-effort/fehlerschluckend wie
    /// an jeder anderen Aufrufstelle — ein Logging-Fehler darf die Ablage
    /// selbst nicht verhindern.
    pub fn enqueue(
        app: &AppHandle,
        va: &str,
        pilot: &str,
        nachtrag: &aeroacars_mqtt::TouchdownRolloutFinalizedPayload,
    ) -> Result<PathBuf, std::io::Error> {
        let base = wurzel(app).ok_or_else(|| io_err("kein app_data_dir"))?;
        let path = enqueue_in(&base, va, pilot, nachtrag)?;
        record_event(
            app,
            &nachtrag.pirep_id,
            &FlightLogEvent::RolloutFinalized {
                timestamp: Utc::now(),
                payload: serde_json::to_value(nachtrag).unwrap_or(serde_json::Value::Null),
            },
        );
        Ok(path)
    }

    /// Ohne Handle: die Identitaet kommt aus dem Schluesselbund — dieselbe,
    /// mit der die naechste Verbindung aufgebaut wird. Fehlt sie, gibt es
    /// keinen Mandanten, dem der Nachtrag gehoeren koennte: Fehler, nicht
    /// „irgendwo ablegen".
    pub fn enqueue_offline(
        app: &AppHandle,
        nachtrag: &aeroacars_mqtt::TouchdownRolloutFinalizedPayload,
    ) -> Result<PathBuf, std::io::Error> {
        let va = secrets::load_api_key(MQTT_KEYRING_VA)
            .ok()
            .flatten()
            .ok_or_else(|| io_err("keine MQTT-Identitaet im Schluesselbund (VA)"))?;
        let pilot = secrets::load_api_key(MQTT_KEYRING_PILOT_ID)
            .ok()
            .flatten()
            .ok_or_else(|| io_err("keine MQTT-Identitaet im Schluesselbund (Pilot)"))?;
        enqueue(app, &va, &pilot, nachtrag)
    }

    /// Alles Wartende EINES Mandanten — Pfad und Inhalt muessen passen.
    pub fn list_in(
        base: &Path,
        va: &str,
        pilot: &str,
    ) -> Vec<(PathBuf, aeroacars_mqtt::TouchdownRolloutFinalizedPayload)> {
        let Some(d) = dir_in(base, va, pilot) else {
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
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            match lesen(&text, va, pilot) {
                Ok(n) => out.push((p, n)),
                Err(e) => {
                    tracing::warn!(pfad = %p.display(), error = %e, "Bahn-Nachtrag in der Ablage bleibt liegen")
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Entfernt die zugestellte Datei — und NUR die. Seit die Revision im
    /// Namen steht, ist das die gesendete Datei und keine andere; kein
    /// Lesen-dann-Loeschen mehr, kein Fenster (Runde 14, High 2). Das ist
    /// die EINZIGE Stelle, die eine Ablage-Datei loescht.
    pub fn entfernen(path: &Path) -> bool {
        match std::fs::remove_file(path) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(pfad = %path.display(), error = %e, "zugestellte Ablage-Datei nicht entfernt");
                false
            }
        }
    }

    /// DER Sendeweg: Ablage zuerst, dann senden, Datei faellt mit der
    /// Zustellmeldung. Alle vier Aufrufer (Streamer, Einreichen, gequeuter
    /// Nachtrag, Worker-Altbestand) gehen hier durch.
    /// Die groesste Nutzlast, die der Sendeweg annimmt. Etwas kleiner als
    /// die Grenze im MQTT-Crate, damit Thema und Kopf sicher passen.
    const MAX_NACHTRAG_BYTES: usize = 60 * 1024;

    /// Bringt einen zu grossen Nachtrag auf Sendegroesse, indem die Spur
    /// ausgeduennt wird — jeder zweite Punkt, so oft wie noetig.
    ///
    /// # Warum es das gibt
    ///
    /// Bis v1.7.40 wurde ein zu grosser Nachtrag jede Minute erneut
    /// versucht. rumqttc verweigert ihn und meldet das als
    /// Verbindungsfehler; der Client warf daraufhin die Leitung weg und
    /// baute fuenf Sekunden spaeter neu auf — im Minutentakt, tagelang
    /// (Pilot 5, 20.09.2026: 625 Fehlversuche an einem Tag). Wer in so
    /// einem Fenster aufsetzte, verlor seine Landungsmeldung.
    ///
    /// Ausduennen statt wegwerfen: Die Spur ist Beiwerk der Anzeige, die
    /// Korrektur selbst (Raeumpunkt, Kantenabstand, Bahn) bleibt ganz.
    /// Gibt `false`, wenn auch ohne Spur nichts passt — dann ist der
    /// Nachtrag strukturell unzustellbar und hat in der Ablage nichts
    /// mehr verloren.
    pub fn auf_sendegroesse_bringen(
        nachtrag: &mut aeroacars_mqtt::TouchdownRolloutFinalizedPayload,
    ) -> bool {
        let gross = |n: &aeroacars_mqtt::TouchdownRolloutFinalizedPayload| {
            serde_json::to_vec(n).map(|v| v.len()).unwrap_or(usize::MAX)
        };
        if gross(nachtrag) <= MAX_NACHTRAG_BYTES {
            return true;
        }
        for _ in 0..12 {
            // Die Spur liegt in der Bahn-Gruppe (`#[serde(flatten)]`).
            let Some(spur) = nachtrag
                .bahn
                .as_mut()
                .and_then(|b| b.lateral_samples.as_mut())
            else {
                break;
            };
            if spur.len() < 2 {
                break;
            }
            let ausgeduennt: Vec<_> = spur
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 2 == 0)
                .map(|(_, p)| p.clone())
                .collect();
            *spur = ausgeduennt;
            if gross(nachtrag) <= MAX_NACHTRAG_BYTES {
                return true;
            }
        }
        // Ohne Spur bleibt der Rest — passt der auch nicht, ist Schluss.
        if let Some(b) = nachtrag.bahn.as_mut() {
            b.lateral_samples = None;
            b.vorherige_durchgaenge = None;
        }
        gross(nachtrag) <= MAX_NACHTRAG_BYTES
    }

    /// Räumt die Ablage BEIM START auf — unabhängig von der Verbindung.
    ///
    /// # Warum beim Start und nicht im Minutentakt
    ///
    /// Der Worker läuft erst eine Minute nach dem Start, und genau diese
    /// erste Minute hat bei Pilot 5 tagelang die Leitung gekostet: Eine
    /// 10.379 Byte grosse Korrektur wurde gesendet, rumqttc verweigerte sie,
    /// der Client warf die Verbindung weg. Wer den Client startet, soll die
    /// Altlast los sein, BEVOR zum ersten Mal gesendet wird — und ohne dass
    /// ein Pilot im Dateisystem etwas löschen muss (Thomas, 20.09.2026).
    ///
    /// Geht über ALLE Ordner der Ablage, nicht nur den aktuellen Piloten:
    /// Wer den Zugang wechselt, schleppt die alte Datei sonst weiter mit.
    pub fn aufraeumen(app: &AppHandle) -> (usize, usize) {
        let Some(base) = wurzel(app) else {
            return (0, 0);
        };
        aufraeumen_in(&base)
    }

    /// Der Kern — ohne `AppHandle`, damit er prüfbar ist.
    pub fn aufraeumen_in(base: &Path) -> (usize, usize) {
        let base = base.to_path_buf();
        let mut geschrumpft = 0;
        let mut aussortiert = 0;
        // Die Ablage ist flach: <wurzel>/<va>/<pilot>/*.json. Mehr als zwei
        // Ebenen gibt es nicht — die Grenze verhindert, dass ein
        // verirrter Verweis das Aufraeumen beim Start im Kreis laufen
        // laesst (`is_dir` folgt Symlinks).
        let mut ordner = vec![(base, 0u8)];
        let mut dateien: Vec<PathBuf> = Vec::new();
        while let Some((d, tiefe)) = ordner.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for eintrag in rd.flatten() {
                let p = eintrag.path();
                // `file_type` folgt keinem Symlink, `is_dir` schon.
                let art = eintrag.file_type().ok();
                if art.is_some_and(|t| t.is_symlink()) {
                    continue;
                }
                if art.is_some_and(|t| t.is_dir()) {
                    if tiefe < 2 {
                        ordner.push((p, tiefe + 1));
                    }
                } else if p.extension().and_then(|s| s.to_str()) == Some("json") {
                    dateien.push(p);
                }
            }
        }
        for pfad in dateien {
            let Ok(text) = std::fs::read_to_string(&pfad) else {
                continue;
            };
            // Bewusst über die Ablage-Hülle, damit va_prefix und pilot_id
            // erhalten bleiben — die Datei wird ja nur ersetzt.
            let Ok(ablage) = serde_json::from_str::<Ablage>(&text) else {
                continue;
            };
            // ÜBER `aus_json`, nicht über ein rohes `from_value`: Die
            // Bahn-Gruppe haengt an `#[serde(flatten)] Option<BahnWire>`,
            // und ein rohes Einlesen macht aus dem FEHLENDEN Block einen
            // leeren — der dann als „alles zurueckgesetzt" rausginge.
            let gelesen =
                aeroacars_mqtt::TouchdownRolloutFinalizedPayload::aus_json(ablage.nachtrag.clone());
            let Ok(mut nachtrag) = gelesen else {
                continue;
            };
            if serde_json::to_vec(&nachtrag).map(|v| v.len()).unwrap_or(0) <= MAX_NACHTRAG_BYTES {
                continue;
            }
            let pirep_id = nachtrag.pirep_id.clone();
            if !auf_sendegroesse_bringen(&mut nachtrag) {
                tracing::error!(
                    pirep_id,
                    pfad = %pfad.display(),
                    "Ablage-Aufräumen: Korrektur passt auch ohne Spur nicht — entfernt"
                );
                if entfernen(&pfad) {
                    aussortiert += 1;
                }
                continue;
            }
            // Atomar wie `enqueue_in`: erst Temp, dann `rename`. Ein
            // `fs::write` auf die Zieldatei liesse einen Leser eine halbe
            // Datei sehen — genau der Fehler, den die Ablage schon einmal
            // hatte (Runde 13, High 3).
            let tmp = pfad.with_file_name(format!(
                ".{}.tmp",
                pfad.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("nachtrag")
            ));
            match serialisieren(&ablage.va_prefix, &ablage.pilot_id, &nachtrag)
                .and_then(|t| std::fs::write(&tmp, t))
                .and_then(|()| std::fs::rename(&tmp, &pfad))
            {
                Ok(()) => {
                    geschrumpft += 1;
                    tracing::info!(
                        pirep_id,
                        pfad = %pfad.display(),
                        "Ablage-Aufräumen: Korrektur ausgedünnt, damit sie sendbar ist"
                    );
                }
                Err(e) => tracing::warn!(
                    pirep_id,
                    error = %e,
                    "Ablage-Aufräumen: ausgedünnte Korrektur nicht geschrieben"
                ),
            }
        }
        (geschrumpft, aussortiert)
    }

    pub fn senden(
        app: &AppHandle,
        sender: &aeroacars_mqtt::NachtragSender,
        nachtrag: aeroacars_mqtt::TouchdownRolloutFinalizedPayload,
    ) -> Result<(), std::io::Error> {
        let pirep_id = nachtrag.pirep_id.clone();
        let revision = nachtrag.herkunft.bahn_revision;
        // Schon beim ERSTEN Versand auf Sendegroesse bringen — sonst
        // scheitert er garantiert, und die Korrektur kaeme erst beim
        // naechsten Worker-Takt an (QS 20.09.2026).
        //
        // KEIN frueher Abbruch: Was auch ausgeduennt nicht passt, wird
        // trotzdem abgelegt — die Ablage ist der einzige Ort, an dem eine
        // Korrektur ueberleben kann. Das Aussortieren uebernimmt `drain`.
        // Der Abbruch in dieser Funktion gehoert allein der fehlenden
        // Ablage (Runde 13, High 2), siehe Test
        // `ohne_ablage_wird_nicht_gesendet_und_die_sperre_bleibt`.
        let mut nachtrag = nachtrag;
        if !auf_sendegroesse_bringen(&mut nachtrag) {
            tracing::error!(
                pirep_id,
                "Bahnkorrektur passt auch ausgeduennt nicht in eine Nachricht"
            );
        }
        // ⚠ Ohne Ablage KEIN Senden (Runde 13, High 2): Vorher ging der
        // Nachtrag bei voller Platte trotzdem raus, der Aufrufer setzte
        // Sperre und Fahne — und wenn der Transport dann scheiterte, gab es
        // weder Datei noch neuen Versuch. Jetzt bleibt der Fehler beim
        // Aufrufer, und Sperre/Fahne bleiben stehen: naechster Tick, naechster
        // Versuch.
        let pfad = match enqueue(app, &sender.va_prefix, &sender.pilot_id, &nachtrag) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(
                    pirep_id,
                    error = %e,
                    "Bahn-Nachtrag nicht in die Ablage geschrieben — nicht gesendet, Sperre bleibt"
                );
                return Err(e);
            }
        };
        let empfang = sender.senden(nachtrag);
        tauri::async_runtime::spawn(async move {
            let zugestellt = matches!(
                tokio::time::timeout(ZUSTELL_FRIST, empfang).await,
                Ok(Ok(true))
            );
            if zugestellt {
                tracing::info!(pirep_id, revision = ?revision, "Bahnkorrektur zugestellt (PUBACK)");
                entfernen(&pfad);
            } else {
                tracing::warn!(
                    pirep_id,
                    revision = ?revision,
                    "Bahnkorrektur nicht zugestellt — bleibt in der Ablage, der Worker versucht es wieder"
                );
            }
        });
        Ok(())
    }

    /// Schickt alles Wartende dieses Mandanten, eins nach dem anderen, und
    /// raeumt weg, was bestaetigt wurde.
    pub async fn drain(app: &AppHandle, sender: &aeroacars_mqtt::NachtragSender) -> usize {
        let Some(base) = wurzel(app) else {
            return 0;
        };
        let mut gesendet = 0;
        for (pfad, nachtrag) in list_in(&base, &sender.va_prefix, &sender.pilot_id) {
            let revision = nachtrag.herkunft.bahn_revision;
            let pirep_id = nachtrag.pirep_id.clone();
            // Erst auf Sendegroesse bringen. Was auch ausgeduennt nicht
            // passt, bleibt NICHT liegen: Es wuerde die Leitung jede
            // Minute erneut kosten.
            let mut nachtrag = nachtrag;
            if !auf_sendegroesse_bringen(&mut nachtrag) {
                tracing::error!(
                    pirep_id,
                    pfad = %pfad.display(),
                    "Bahnkorrektur passt auch ausgeduennt nicht in eine Nachricht — aussortiert"
                );
                entfernen(&pfad);
                continue;
            }
            let zugestellt = matches!(
                tokio::time::timeout(ZUSTELL_FRIST, sender.senden(nachtrag)).await,
                Ok(Ok(true))
            );
            if zugestellt {
                tracing::info!(pirep_id, revision = ?revision, "Bahnkorrektur aus der Ablage nachgeschickt");
                entfernen(&pfad);
                gesendet += 1;
            } else {
                tracing::warn!(
                    pirep_id,
                    "Bahnkorrektur aus der Ablage nicht zugestellt — bleibt liegen"
                );
                // Liegt die Leitung, liegt sie fuer alle: nicht weiter
                // probieren, naechster Tick.
                break;
            }
        }
        gesendet
    }
