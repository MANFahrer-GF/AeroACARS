    #[test]
    fn worker_reklamiert_unbekannte_eintraege_serverseitig_statt_sie_endgueltig_aufzugeben() {
        const SRC: &str = include_str!("../lib.rs");
        // Nadel aus zwei Teilen zusammengesetzt, NICHT als ein Literal: der
        // Test liegt VOR der echten Funktion im Quelltext, und
        // `include_str!` liest die GESAMTE Datei einschliesslich dieser
        // Zeile selbst — ein einzelnes Literal-Suchmuster faende sich
        // sonst selbst zuerst statt der echten Funktionsdefinition weiter
        // unten (derselbe Fehler, der hier beinahe reingerutscht waere).
        // Bewusst OHNE die oeffnende geschweifte Funktions-Klammer am Ende:
        // ein einzelnes, in einem String-Literal unausgeglichenes
        // Klammerzeichen wuerde die naive Klammer-Zaehlung in
        // tests/angeschlossen.rs's Test-Ueberspring-Logik aus dem Takt
        // bringen und den Rest der Datei faelschlich als "noch im
        // Testmodul" behandeln (real passiert, siehe Nachtrag in der
        // Doku).
        let nadel = format!("{}{}", "fn spawn_pirep_queue_worker", "(app: AppHandle)");
        let start = SRC
            .find(&nadel)
            .expect("spawn_pirep_queue_worker nicht mehr gefunden — Test anpassen, nicht loeschen");
        let rest = &SRC[start..];
        let ende = rest[1..].find("\nfn ").map(|i| i + 1).unwrap_or(rest.len());
        let funktionskoerper = &rest[..ende];
        assert!(
            funktionskoerper.contains("get_user_pireps_in_progress"),
            "der Worker fragt vor der Quarantaene nicht mehr serverseitig nach — \
             Alt-Eintraege (owner_identity: None) und Eintraege nach einer \
             API-Key-Rotation blieben dann fuer immer liegen"
        );
        assert!(
            funktionskoerper.contains("owner_identity = Some(aktuelle_identitaet"),
            "der Worker schreibt den Fingerabdruck nach serverseitiger \
             Bestaetigung nicht mehr um — der Eintrag wuerde beim naechsten \
             Tick wieder als fremd gelten"
        );
    }

    /// Codex-Folgefund (adversarial, 05.09.2026, achte Runde): `client` und
    /// `authenticated_pilot_id` muessen als EIN atomarer Schnappschuss
    /// erfasst werden, VOR jedem Await im Tick — sonst kann ein Logout/
    /// Login-Kontowechsel zwischen den beiden Lesungen (z. B. waehrend
    /// `drain_pending_bid_cleanup(...).await`) Pilot As Client mit Pilot Bs
    /// Identitaet paaren. Ergebnis: ein Warteschlangen-Eintrag, der
    /// tatsaechlich Pilot B gehoert, besteht die Eigentuemer-Pruefung, wird
    /// aber mit Pilot As Credentials eingereicht.
    ///
    /// Seit der zehnten Runde geschieht diese Erfassung ueber
    /// `aktuelle_session_atomar` (haelt beide Mutexe gleichzeitig — zwei
    /// getrennte Lesungen waren KEINE echte Garantie gegen Thread-
    /// Nebenlaeufigkeit, nur gegen Awaits dazwischen). Der Test prueft
    /// jetzt diesen Aufruf statt der frueheren Einzel-Feld-Lesung.
    #[test]
    fn client_und_piloten_id_werden_vor_dem_ersten_await_gemeinsam_erfasst() {
        const SRC: &str = include_str!("../lib.rs");
        let nadel = format!("{}{}", "fn spawn_pirep_queue_worker", "(app: AppHandle)");
        let start = SRC
            .find(&nadel)
            .expect("spawn_pirep_queue_worker nicht mehr gefunden — Test anpassen, nicht loeschen");
        let rest = &SRC[start..];
        let ende = rest[1..].find("\nfn ").map(|i| i + 1).unwrap_or(rest.len());
        let funktionskoerper = &rest[..ende];
        let erfassung_pos = funktionskoerper.find("aktuelle_session_atomar(&state)");
        let await_pos = funktionskoerper
            .find("drain_pending_bid_cleanup(&app, &client, &aktuelle_identitaet).await");
        match (erfassung_pos, await_pos) {
            (Some(e), Some(a)) => assert!(
                e < a,
                "aktuelle_session_atomar wird nicht mehr VOR dem ersten Await \
                 (drain_pending_bid_cleanup) gelesen — ein Kontowechsel \
                 dazwischen koennte Client und Identitaet wieder \
                 auseinanderlaufen lassen"
            ),
            _ => panic!(
                "aktuelle_session_atomar-Aufruf oder drain_pending_bid_cleanup-Await \
                 im Worker nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }
