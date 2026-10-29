    use super::*;

    // v1.6.11 — welche Bids-Fehler einen zweiten Versuch wert sind.
    //
    // Feldbefund 18.08.2026 (BTI22): EIN verlorenes Paket im Reiseflug schrieb
    // eine rote Fehlerzeile ins Aktivitaetsprotokoll — und die blieb dort
    // stehen, obwohl der naechste Abruf vier Sekunden spaeter klappte. Karte
    // und Overlay zeigen dieses Protokoll, also starrte den Piloten stundenlang
    // ein laengst erledigter Fehler an.
    //
    // Der Zuschnitt ist der heikle Teil: WIEDERHOLEN darf nur, was von selbst
    // vorbeigeht. Ein 401 wiederholt sich beliebig oft mit demselben Ergebnis —
    // dort ist die sofortige Meldung samt technischer Zeile die richtige
    // Antwort, und Wiederholen wuerde sie nur um 1,6 s verzoegern.

    #[test]
    fn netzfehler_und_5xx_sind_vorruebergehend() {
        assert!(bids_error_is_transient(&ApiError::Network(
            "error sending request".into()
        )));
        assert!(bids_error_is_transient(&ApiError::Server {
            status: 500,
            body: String::new(),
        }));
        assert!(bids_error_is_transient(&ApiError::Server {
            status: 503,
            body: String::new(),
        }));
    }

    #[test]
    fn dauerhafte_fehler_werden_nicht_wiederholt() {
        // Gegenprobe zur Regel oben — ohne sie wuerde ein zu weiter Zuschnitt
        // (etwa "alles ausser Ok") unbemerkt durchgehen.
        assert!(!bids_error_is_transient(&ApiError::Unauthenticated));
        assert!(!bids_error_is_transient(&ApiError::Forbidden));
        assert!(!bids_error_is_transient(&ApiError::NotFound));
        assert!(!bids_error_is_transient(&ApiError::BadResponse(
            "field `flight_number` was a string".into()
        )));
        assert!(!bids_error_is_transient(&ApiError::InvalidUrl(
            "nope".into()
        )));
    }

    #[test]
    fn vierhundert_ist_kein_serverfehler() {
        // Die Grenze liegt bei 500. Ein 4xx kommt vom Client und wiederholt
        // sich genauso — der frueher naheliegende `matches!(.., Server { .. })`
        // ohne Statuspruefung waere hier falsch gewesen.
        assert!(!bids_error_is_transient(&ApiError::Server {
            status: 400,
            body: String::new(),
        }));
        assert!(!bids_error_is_transient(&ApiError::Server {
            status: 499,
            body: String::new(),
        }));
    }

    #[test]
    fn zeitbudget_haelt_den_schlechtesten_fall_bei_rund_fuenf_sekunden() {
        // QS-Befund 18.08.2026. Rechnung, nicht Gefuehl: der Verbindungs-Timeout
        // liegt bei 5 s. Ohne Budget waeren drei Versuche + Wartezeiten
        // 3*5000 + 400 + 1200 = 16,6 s. Mit dem Budget ist nach dem ERSTEN
        // Timeout Schluss, weil 5000 ms bereits ueber der Schranke liegen.
        const VERBINDUNGS_TIMEOUT_MS: u128 = 5_000;
        let wartezeiten: u128 = BIDS_RETRY_DELAYS_MS.iter().map(|d| *d as u128).sum();

        // Ohne Schranke: drei volle Timeouts plus beide Wartezeiten.
        let ohne_schranke = 3 * VERBINDUNGS_TIMEOUT_MS + wartezeiten;
        // Mit Schranke: nach dem ersten Timeout ist Schluss, denn 5.000 ms
        // liegen bereits ueber dem Budget.
        let mit_schranke = if VERBINDUNGS_TIMEOUT_MS >= BIDS_RETRY_BUDGET_MS {
            VERBINDUNGS_TIMEOUT_MS
        } else {
            ohne_schranke
        };

        assert_eq!(
            ohne_schranke, 16_600,
            "Rechnung des schlechtesten Falls stimmt nicht mehr"
        );
        assert_eq!(
            mit_schranke, VERBINDUNGS_TIMEOUT_MS,
            "ein einzelner Timeout muss das Budget aufbrauchen, sonst greift die Schranke nicht"
        );

        // Gegenprobe in die andere Richtung: ein SCHNELLER Fehlschlag muss
        // beide Wiederholungen noch zulassen, sonst waere die Schranke zu eng
        // und der eigentliche Zweck (Aussetzer abfangen) dahin.
        let schneller_fehlschlag_ms: u128 = 50;
        let bis_zum_zweiten = schneller_fehlschlag_ms + BIDS_RETRY_DELAYS_MS[0] as u128;
        let bis_zum_dritten =
            bis_zum_zweiten + schneller_fehlschlag_ms + BIDS_RETRY_DELAYS_MS[1] as u128;
        assert!(
            bis_zum_zweiten < BIDS_RETRY_BUDGET_MS,
            "zweiter Versuch faellt aus dem Budget"
        );
        assert!(
            bis_zum_dritten < BIDS_RETRY_BUDGET_MS,
            "dritter Versuch faellt aus dem Budget"
        );
    }

    #[test]
    fn hoechstens_zwei_wiederholungen_und_sie_sind_kurz() {
        // In Summe unter zwei Sekunden: der Pilot soll den Aussetzer gar nicht
        // erst mitbekommen. Waere die Wartezeit lang, wuerde die Liste im
        // Briefing sichtbar haengen.
        assert_eq!(BIDS_RETRY_DELAYS_MS.len(), 2);
        let summe: u64 = BIDS_RETRY_DELAYS_MS.iter().sum();
        assert!(summe < 2_000, "zu lange Gesamtwartezeit: {summe} ms");
        assert!(
            BIDS_RETRY_DELAYS_MS.windows(2).all(|w| w[0] < w[1]),
            "die Wartezeiten sollen wachsen, nicht schrumpfen"
        );
    }

    #[test]
    fn erledigter_fehler_wird_heruntergestuft_statt_geloescht() {
        // Der Eintrag verschwindet NICHT — die Stoerung war echt und gehoert
        // ins Protokoll. Er hoert nur auf, rot zu sein.
        let mut log: VecDeque<ActivityEntry> = VecDeque::new();
        log.push_back(ActivityEntry {
            timestamp: Utc::now(),
            level: ActivityLevel::Error,
            message: BIDS_FAILED_MESSAGE.to_string(),
            detail: Some("network error".into()),
        });
        log.push_back(ActivityEntry {
            timestamp: Utc::now(),
            level: ActivityLevel::Error,
            message: "Ein ganz anderer Fehler".to_string(),
            detail: None,
        });

        entwerte_bids_fehler(&mut log);

        assert!(matches!(log[0].level, ActivityLevel::Info));
        assert_eq!(log[0].message, BIDS_FAILED_MESSAGE, "Text bleibt stehen");
        assert!(log[0].detail.as_deref().unwrap().contains("network error"));
        assert!(log[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("inzwischen wieder erreichbar"));

        // Gegenprobe: fremde Eintraege bleiben unangetastet.
        assert!(matches!(log[1].level, ActivityLevel::Error));
        assert!(log[1].detail.is_none());
    }
