    use super::*;

    #[test]
    fn zusammenfassung_der_bildrate() {
        assert_eq!(bildrate_zusammenfassung(std::iter::empty()), None);
        assert_eq!(
            bildrate_zusammenfassung([0.0, f32::NAN, -1.0].into_iter()),
            None,
            "Unsinn ist keine Bildrate"
        );
        let (min, median, max, n) =
            bildrate_zusammenfassung([41.0, 24.0, 0.0, 60.0, 38.0].into_iter()).expect("Werte da");
        assert_eq!((min, median, max, n), (24.0, 41.0, 60.0, 4));
    }

    /// Die Bildrate steht im Flugprotokoll, aber nicht im Sim-Status, den
    /// Oberflaeche und LAN-Fernbedienung bekommen (Codex-QS P2, 2.0.14).
    #[test]
    fn bildrate_nur_im_flugprotokoll_nicht_im_sim_status() {
        let mut snap = SimSnapshot::default();
        snap.sim_bildrate_hz = Some(42.0);
        let protokoll = serde_json::to_value(&snap).expect("Snapshot");
        assert_eq!(
            protokoll["sim_bildrate_hz"],
            serde_json::json!(42.0),
            "Gegenprobe: das Flugprotokoll traegt die Bildrate"
        );
        let status = SimStatus {
            snapshot: Some(snap),
            ..Default::default()
        };
        let status = serde_json::to_value(&status).expect("Status");
        assert!(
            status["snapshot"].get("sim_bildrate_hz").is_none(),
            "der Sim-Status gibt die Bildrate an Oberflaeche und LAN-Bruecke weiter"
        );
        assert!(status["snapshot"].is_object(), "der Snapshot selbst fehlt");
    }

    /// Der Puffer muss das ganze Aufsetzfenster abdecken, sonst fehlen
    /// die ersten Sekunden vor dem Aufsetzen in der Zeile.
    #[test]
    fn der_puffer_deckt_das_aufsetzfenster() {
        assert!(
            BILDRATE_PUFFER_SECS * 1000 > crate::grenzwerte::grenzwerte().landung.touchdown_buffer_secs.wert * 1000 + crate::grenzwerte::grenzwerte().landung.touchdown_post_window_ms.wert,
            "Bildraten-Puffer kuerzer als Vorlauf plus Nachlauf"
        );
    }
