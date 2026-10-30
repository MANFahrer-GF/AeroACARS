/// QS 2026-08-04: `score_label` und `grade_letter` sind ABGELEITETE Werte —
/// beide sind reine Funktionen von `score_numeric`. Gespeichert werden sie
/// trotzdem, einmal beim Einreichen, und danach nie wieder angefasst. Wird
/// die Ableitungsregel spaeter praezisiert (in v0.20.0 passiert: die
/// Label-Schwellen wurden auf die Noten-Schwellen ausgerichtet), tragen
/// Alt-Datensaetze fuer immer das Ergebnis der ALTEN Regel mit sich herum.
///
/// Der Landungs-Tab hatte das mit einer eigenen, in TypeScript nachgebauten
/// Schwellen-Leiter umschifft — und damit die Regel ZWEIMAL im Code stehen,
/// was sofort die naechste Abweichung erzeugte: die Uebersicht rechnete
/// frisch ("SMOOTH"), der Bericht las das eingefrorene Feld ("ACCEPTABLE"),
/// fuer ein und dieselbe Landung (real gemessen: 2 von 31 Fluegen, beide
/// 88 Punkte / Note A).
///
/// Nachhaltige Loesung statt zweier Anzeige-Flicken: die abgeleiteten
/// Felder werden hier beim LESEN neu aus `score_numeric` berechnet. Damit
/// gibt es genau eine Quelle der Wahrheit (`aggregate_score_label` /
/// `letter_grade`), jede Oberflaeche bekommt automatisch denselben Wert,
/// eine kuenftige Regelaenderung wirkt rueckwirkend auf die ganze Historie
/// — und der Nachbau im Frontend kann entfallen. Der Speicher bleibt
/// unangetastet (keine Migration, keine Schreibzugriffe).
///
/// Score-Version 21 (v2.0.5, Thomas: „die Landungen im Client neu
/// berechnen"): Fluege, die noch mit Deckel bewertet wurden (Version
/// 18–20), bekommen Note und Grund aus ihren eingefrorenen Teilnoten neu —
/// mit derselben Funktion wie beim Einreichen. Teilnoten und Version
/// bleiben, wie sie sind; die Server-Sicherung bleibt unberuehrt.
fn normalize_derived_scores(mut r: LandingRecord) -> LandingRecord {
    vref_pauschal_neu_rechnen(&mut r);
    if matches!(r.score_algorithm_version, Some(18..=20)) && r.score_numeric.is_some() {
        if let Some(neu) = landing_scoring::aggregate_master_score(&r.sub_scores) {
            r.score_numeric = Some(neu as i32);
            r.score_deckel = landing_scoring::master_deckel_wirksam(&r.sub_scores);
        }
    }
    // Ohne Note gibt es auch nichts abzuleiten. Ein Datensatz ohne
    // Bewertung (zu dünne Aufzeichnung, siehe `landung_nicht_bewertbar`)
    // behält leere Felder statt eines aus `None` gerechneten "F".
    let Some(n) = r.score_numeric.map(|v| v.clamp(0, 100)) else {
        r.score_label = None;
        r.grade_letter = None;
        return r;
    };
    r.score_label = Some(aggregate_score_label(n as u8).to_string());
    r.grade_letter = Some(letter_grade(n).to_string());
    r
}

/// Bis 2.0.5 stand bei Mustern ohne gemessene Vref ein Pauschalwert je
/// Muster in der Landung (`icao_default`). Beim Lesen rechnet die Regel von
/// heute (`landing_scoring::vref`) sie aus Gewicht und Geschwindigkeit neu;
/// ohne Bezugswert verschwindet die Abweichung. Die Klappenstellung kennt der
/// alte Datensatz nicht — sie nimmt das Urteil deshalb nicht weg.
fn vref_pauschal_neu_rechnen(r: &mut LandingRecord) {
    if r.landing_vref_source.as_deref() != Some("icao_default") {
        return;
    }
    let muster = sim_core::muster_aufloesen(r.aircraft_icao.as_deref(), "", r.aircraft_title.as_deref());
    let vref = landing_scoring::vref::vref_bestimmen(muster.as_deref(), r.landing_weight_kg, None, None);
    r.landing_vref_source = Some(vref.map_or("unknown", |e| e.quelle).to_string());
    r.landing_vref_toleranz_kt = vref.and_then(|e| e.toleranz_kt);
    r.landing_vref_deviation_kt = match (vref, r.landing_speed_kt) {
        (Some(e), Some(ias)) => Some(ias - e.vref_kt),
        _ => None,
    };
}

/// Stand-Merker der Landungsdatei (Größe + Änderungszeit), ohne sie zu
/// lesen. Der Landungs-Tab fragt alle 5 s nur diesen ab und holt die volle
/// Liste erst, wenn er sich ändert — über die LAN-Brücke waren das sonst
/// 9,9 MB je Takt (Feldbefund 28.10.2026, Tablet fror ein).
#[tauri::command(async)]
fn landing_list_stand(app: AppHandle) -> Option<String> {
    open_landing_store(&app).and_then(|s| s.stand().ok())
}

/// List every persisted landing record, newest first. Used by the
/// Landing tab's list view — OHNE die schweren Felder (siehe
/// [`ohne_schwere_felder`]); die Detailansicht holt die volle Landung über
/// `landing_get`.
#[tauri::command(async)]
fn landing_list(app: AppHandle) -> Vec<LandingRecord> {
    open_landing_store(&app)
        .and_then(|s| s.list().ok())
        .unwrap_or_default()
        .into_iter()
        .map(normalize_derived_scores)
        .map(ohne_schwere_felder)
        .collect()
}

/// Eine Landung vollständig, für die Detailansicht und den PDF-Bericht.
/// v0.7.13 entfernt, 28.10.2026 zurück: seit `landing_list` schlank ist.
/// `Ok(None)` = gibt es nicht (mehr), `Err` = Lesefehler (Codex-QS: die
/// Oberfläche bietet nur beim Fehler „Erneut versuchen“ an).
#[tauri::command(async)]
fn landing_get(app: AppHandle, pirep_id: String) -> Result<Option<LandingRecord>, UiError> {
    let store = open_landing_store(&app)
        .ok_or_else(|| UiError::new("landing_get", "Landungsspeicher nicht erreichbar"))?;
    let rec = store
        .get(&pirep_id)
        .map_err(|e| UiError::new("landing_get", e.to_string()))?;
    Ok(rec.map(normalize_derived_scores))
}

/// Lässt die Felder weg, die nur die Detailansicht liest (Spuren, Sprit je
/// Wegpunkt, Gleitpfad, Bahnauswertung). Feldbefund 28.10.2026: die volle
/// Liste waren 9,9 MB, über die LAN-Brücke dauerte das Öffnen des Tabs
/// lange. Übersicht, Kurve, Kennzahlen und der Bestwert-Vergleich der
/// Detailansicht lesen keines dieser Felder (geprüft am Frontend-Code).
fn ohne_schwere_felder(mut r: LandingRecord) -> LandingRecord {
    r.lateral_samples = Vec::new();
    r.touchdown_profile = Vec::new();
    r.approach_samples = Vec::new();
    r.runway_exits = Vec::new();
    r.sub_scores = Vec::new();
    r.sprit = None;
    r.runway_match = None;
    r.sampler_diagnose = None;
    r.gate_window = None;
    r.anflug_gleitpfad = None;
    r.modul_sollband = None;
    r
}

/// Build a *preview* landing record from the currently-active flight.
/// Used by the Landing tab during the rollout / before file: shows the
/// pilot what their landing looks like *right now* without having to
/// wait for the PIREP to be filed. Returns None when there is no
/// active flight or when the touchdown hasn't happened yet.
#[tauri::command(async)]
fn landing_get_current(app: AppHandle, state: tauri::State<'_, AppState>) -> Option<LandingRecord> {
    let flight = {
        let guard = state.active_flight.lock().expect("active_flight lock");
        guard.as_ref().cloned()
    }?;
    let stats = flight.stats.lock().expect("flight stats");
    let snapshot = current_snapshot(&app);
    let sim_kind = read_sim_config(&app).kind;
    let sim_label = match sim_kind {
        SimKind::Off => None,
        SimKind::Msfs2020 | SimKind::Msfs2024 => Some("MSFS"),
        SimKind::XPlane11 | SimKind::XPlane12 => Some("X-PLANE"),
    };
    // v0.7.17 (B-015b QS-Fix): Aircraft-ICAO mit Bid-Fallback — identische
    // Logik wie der Final-Filing-Pfad in `record_landing_for_filed_flight`
    // (siehe lib.rs:~8510). Vorher: Live-Vorschau nutzte nur Snapshot-
    // Aircraft → bei Sim-Disconnect oder X-Plane Web API aus war
    // `aircraft_icao=None` → sub_rollout fiel auf Light-GA-Schwellen
    // zurueck → Pilot sah inkonsistente Cockpit-Vorschau-Scores gegenueber
    // dem spaeter persistierten LandingRecord. QS-Round-1 Befund.
    let aircraft_title = snapshot.as_ref().and_then(|s| s.aircraft_title.as_deref());
    // Sim → Buchung → Flugzeugtitel, an EINER Stelle (siehe
    // `sim_core::muster_aufloesen`). Diese Kette stand hier und an einer
    // zweiten Stelle wortgleich, waehrend drei weitere Stellen kuerzere
    // Ketten benutzten — daran ist EWG248 gescheitert.
    let aufgeloest = stats.aufgeloestes_muster.clone().or_else(|| {
        sim_core::muster_aufloesen(
            snapshot.as_ref().and_then(|s| s.aircraft_icao.as_deref()),
            &flight.aircraft_icao,
            aircraft_title,
        )
    });
    let aircraft_icao = aufgeloest.as_deref();
    // v0.7.18 (B-012): airport_lookup-Closure aus dem AppState.airports-Cache.
    let airport_lookup_data: std::collections::HashMap<String, (f64, f64)> = {
        let guard = state.airports.lock().expect("airports lock");
        guard
            .iter()
            .filter_map(|(k, v)| match (v.lat, v.lon) {
                (Some(la), Some(lo)) => Some((k.clone(), (la, lo))),
                _ => None,
            })
            .collect()
    };
    let airport_lookup = |icao: &str| airport_lookup_data.get(&icao.to_uppercase()).copied();
    build_landing_record(
        &flight,
        &stats,
        sim_label,
        aircraft_icao,
        aircraft_title,
        // Live-Vorschau: ein manueller Divert ist noch nicht deklariert →
        // geplantes Ziel. Ein mid-flight erkannter Divert greift weiterhin
        // über stats.divert_hint im Trust-Check.
        &flight.arr_airport,
        None,
        airport_lookup,
    )
}

/// Delete a landing record. Lets the user clean up bad/test entries
/// from the Landing tab. Best-effort — returns Ok(()) even if the
/// record didn't exist.
#[tauri::command(async)]
fn landing_delete(app: AppHandle, pirep_id: String) -> Result<(), UiError> {
    let Some(store) = open_landing_store(&app) else {
        return Err(UiError::new(
            "landing_store",
            "could not open landing store",
        ));
    };
    let mut all = store
        .list()
        .map_err(|e| UiError::new("landing_read", format!("{e}")))?;
    let before = all.len();
    all.retain(|r| r.pirep_id != pirep_id);
    if all.len() == before {
        return Ok(());
    }
    // upsert one-by-one isn't quite right (it appends); easier: rewrite
    // the file via a private helper. We don't have one — fall back to
    // upserting each remaining row in order. Since LandingStore::upsert
    // dedupes by pirep_id, an upsert chain produces the same set.
    // BUT it doesn't drop rows. Hack: write empty first, then re-add.
    // The store doesn't expose a clear/replace yet — we add one.
    if let Err(e) = store.replace_all(&all) {
        return Err(UiError::new("landing_write", format!("{e}")));
    }
    // v0.20.x QS fix: record the deletion so the next backup merge (or an
    // explicit restore) can't silently resurrect this record from the
    // server's still-existing copy — see `DeletedLandingsTombstone`.
    if let Some(tombstone) = open_deleted_landings_tombstone(&app) {
        if let Err(e) = tombstone.record_deleted(&pirep_id) {
            tracing::warn!(pirep_id, error = %e, "could not record landing-deletion tombstone");
        }
    }
    Ok(())
}

// ---- Site config persistence ----

fn site_config_path(app: &AppHandle) -> Result<PathBuf, UiError> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join(SITE_CONFIG_FILE))
        .map_err(|e| UiError::new("config_path", e.to_string()))
}

fn read_site_config(app: &AppHandle) -> Result<Option<SiteConfig>, UiError> {
    let path = site_config_path(app)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| UiError::new("config_read", e.to_string()))?;
    let cfg: SiteConfig =
        serde_json::from_slice(&bytes).map_err(|e| UiError::new("config_parse", e.to_string()))?;
    Ok(Some(cfg))
}

fn write_site_config(app: &AppHandle, cfg: &SiteConfig) -> Result<(), UiError> {
    let path = site_config_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| UiError::new("config_write", e.to_string()))?;
    }
    let json = serde_json::to_vec_pretty(cfg)
        .map_err(|e| UiError::new("config_serialize", e.to_string()))?;
    std::fs::write(&path, json).map_err(|e| UiError::new("config_write", e.to_string()))
}

fn clear_site_config(app: &AppHandle) -> Result<(), UiError> {
    let path = site_config_path(app)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| UiError::new("config_remove", e.to_string()))?;
    }
    Ok(())
}

// ---- Tauri commands ----

#[derive(Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub commit: Option<&'static str>,
    /// Small credit line for the About / Settings footer.
    pub credit: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "AeroACARS",
        version: env!("CARGO_PKG_VERSION"),
        commit: option_env!("AEROACARS_GIT_SHA"),
        credit: "Made with ❤️ in Gifhorn — by Thomas Kant",
    }
}
