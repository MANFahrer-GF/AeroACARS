    fn koerper<'a>(src: &'a str, kopf: &str) -> &'a str {
        let start = src
            .find(kopf)
            .unwrap_or_else(|| panic!("{kopf} nicht gefunden"));
        let rest = &src[start + kopf.len()..];
        let ende = ["\nfn ", "\nasync fn ", "\npub fn ", "\n#[tauri::command]"]
            .iter()
            .filter_map(|m| rest.find(m))
            .min()
            .unwrap_or(rest.len());
        &rest[..ende]
    }

    #[test]
    fn bordbuch_beeinflusst_weder_pirep_noch_landung() {
        let src = include_str!("../lib.rs");
        for kopf in [
            "\nfn build_pirep_payload(",
            "\nfn finalize_filed_pirep(",
            "\nfn emit_landing_finalized(",
            "\nfn record_landing_for_filed_flight(",
        ] {
            let k = koerper(src, kopf);
            assert!(k.len() > 200, "{kopf}: Körper zu kurz — Suche kaputt?");
            assert!(
                !k.contains("bordbuch"),
                "{kopf} liest das Bordbuch — das darf nie in PIREP/Landung fliessen"
            );
        }
        // Gegenprobe: der Wächter findet das Wort, wo es hingehört.
        assert!(koerper(src, "\nfn bordbuch_tick(").contains("bordbuch::tick"));
        // Die Landebewertung ist ein eigenes Crate und kennt das Modul nicht.
        for datei in [
            include_str!("../../crates/landing-scoring/src/lib.rs"),
            include_str!("../../crates/landing-scoring/src/gate.rs"),
        ] {
            assert!(!datei.contains("bordbuch"));
        }
    }
