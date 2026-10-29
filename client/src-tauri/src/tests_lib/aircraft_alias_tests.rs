    use super::aircraft_types_match;

    /// Live bug 2026-05-04: Emirates UAE770 EK770 (A359 bid)
    /// blocked because the sim loaded "A350-900 (No Cabin)" — both
    /// names refer to the same airframe.
    #[test]
    fn a359_matches_a350_900_long_form() {
        assert!(aircraft_types_match("A359", "A350-900"));
        assert!(aircraft_types_match("A350-900", "A359"));
    }

    #[test]
    fn b738_matches_737_800() {
        assert!(aircraft_types_match("B738", "737-800"));
        assert!(aircraft_types_match("737-800", "B738"));
    }

    #[test]
    fn b77w_matches_777_300er() {
        assert!(aircraft_types_match("B77W", "777-300ER"));
        assert!(aircraft_types_match("777-300ER", "B77W"));
        assert!(aircraft_types_match("B77W", "777-300 ER"));
    }

    #[test]
    fn a20n_matches_a320neo() {
        assert!(aircraft_types_match("A20N", "A320NEO"));
        assert!(aircraft_types_match("A20N", "A320-NEO"));
        assert!(aircraft_types_match("A20N", "A320 NEO"));
    }

    #[test]
    fn b789_matches_787_9() {
        assert!(aircraft_types_match("B789", "787-9"));
    }

    #[test]
    fn unrelated_types_dont_match() {
        // Real mismatch should still be blocked.
        assert!(!aircraft_types_match("B738", "A320"));
        assert!(!aircraft_types_match("A359", "B77W"));
    }

    #[test]
    fn case_insensitive() {
        assert!(aircraft_types_match("a359", "a350-900"));
        assert!(aircraft_types_match("A359", "a350-900"));
    }

    #[test]
    fn strict_equality_still_works_for_unaliased() {
        assert!(aircraft_types_match("DH8D", "DH8D"));
        assert!(!aircraft_types_match("DH8D", "DH8C"));
    }

    /// Live bug 2026-05-10: Martinair Cargo MPH62 (MD11 bid mit
    /// PH-MCU) wurde geblockt weil TFDi Design im Sim "MD-11F"
    /// meldet. MD11 + MD11F sind gleiche Familie, Cargo-Bid darf
    /// die F-Variante fliegen.
    #[test]
    fn md11_matches_md_11f_long_form() {
        // ICAO bid "MD11" sollte sim-side "MD-11F" / "MD11F" akzeptieren
        assert!(aircraft_types_match("MD11", "MD-11F"));
        assert!(aircraft_types_match("MD11", "MD11F"));
        assert!(aircraft_types_match("MD11", "MD-11"));
        // umgekehrt: ICAO bid "MD11F" sollte sim-side "MD-11F" akzeptieren
        assert!(aircraft_types_match("MD11F", "MD-11F"));
        // case-insensitive
        assert!(aircraft_types_match("md11", "md-11f"));
    }

    #[test]
    fn md11_does_not_match_unrelated_widebodies() {
        // MD11 darf NICHT mit anderen Widebodies matchen
        assert!(!aircraft_types_match("MD11", "B77W"));
        assert!(!aircraft_types_match("MD11", "A359"));
        assert!(!aircraft_types_match("MD11", "B748"));
    }

    /// Live bug 2026-05-13 (GSG / Michel D., MSFS): HondaJet-Bid mit
    /// ICAO "HDJT" geblockt, weil MSFS den ATC-MODEL-String "HA420"
    /// (Hersteller-Marketing-Designation) liefert und der TITLE typisch
    /// "mg hjet ha420 [Preset Default]" ist. Beide Codes meinen dasselbe
    /// Aircraft — bidirektional in der Alias-Tabelle eingetragen.
    #[test]
    fn hdjt_matches_ha420_hondajet() {
        // Bid (HDJT) gegen sim ATC-MODEL (HA420)
        assert!(aircraft_types_match("HDJT", "HA420"));
        // Inverse: Bid (HA420 — falls eine VA das so führt) gegen sim (HDJT)
        assert!(aircraft_types_match("HA420", "HDJT"));
        // Bid HDJT gegen Sim-TITLE (Substring-Match auf Long-Form)
        assert!(aircraft_types_match(
            "HDJT",
            "mg hjet ha420 [Preset Default]"
        ));
        assert!(aircraft_types_match("HDJT", "Asobo HondaJet HA-420"));
        // HA-420 (mit Bindestrich, wie Honda offiziell schreibt)
        assert!(aircraft_types_match("HDJT", "HA-420"));
    }

    #[test]
    fn hdjt_does_not_match_unrelated_jets() {
        // HondaJet darf NICHT mit anderen Light-Jets matchen
        assert!(!aircraft_types_match("HDJT", "C25A")); // CitationJet
        assert!(!aircraft_types_match("HDJT", "E50P")); // Phenom 100
        assert!(!aircraft_types_match("HDJT", "TBM9")); // TBM 930
    }

    /// v0.8.1: Vendor-Tag-Prefix-Strip im clean_atc_model. Live-Bug
    /// GSG/Sven M 2026-05-13: Cessna Citation X — MSFS-Addon (Flysimware
    /// vermutet) schickt "$$:C750" als ATC-MODEL statt nur "C750".
    /// Ohne den Strip war exp=C750 act=$$:C750 ein Mismatch.
    #[test]
    fn vendor_prefix_dollar_dollar_colon_gets_stripped() {
        use super::clean_atc_model;
        assert_eq!(clean_atc_model("$$:C750"), Some("C750".to_string()));
        assert_eq!(clean_atc_model("$$:b738"), Some("B738".to_string()));
        // case-insensitive (uppercase happens early in the function)
        assert_eq!(clean_atc_model("##:A320"), Some("A320".to_string()));
    }

    #[test]
    fn normal_atc_model_passes_through_unchanged() {
        use super::clean_atc_model;
        // Regular ICAO codes without vendor prefix bleiben unverändert.
        assert_eq!(clean_atc_model("C750"), Some("C750".to_string()));
        assert_eq!(clean_atc_model("B738"), Some("B738".to_string()));
        assert_eq!(clean_atc_model("HDJT"), Some("HDJT".to_string()));
    }

    #[test]
    fn tt_text_token_still_rejected() {
        use super::clean_atc_model;
        // TT:-Prefix war schon vor v0.8.1 als None-return behandelt
        // (text-token, kein echter ICAO). Verhalten darf nicht ändern —
        // sonst würde der neue Strip-Pfad fälschlich den TT-Suffix als
        // ICAO liefern. `TT` ist purer Buchstabe → kein "vendor tag",
        // greift den is_vendor_tag-Check nicht.
        assert_eq!(clean_atc_model("TT:CESSNA"), None);
    }

    #[test]
    fn atccom_with_ac_model_still_extracts_icao() {
        use super::clean_atc_model;
        // Bestehender Pfad (AC_MODEL-substring) extrahiert die ICAO
        // VOR dem TT/ATCCOM-Reject-Check. Verhalten darf nicht ändern.
        assert_eq!(
            clean_atc_model("ATCCOM.AC_MODEL_C750.0.TEXT"),
            Some("C750".to_string())
        );
    }

    #[test]
    fn empty_after_strip_falls_back_gracefully() {
        use super::clean_atc_model;
        // Nur Prefix ohne Inhalt → kein gültiger ICAO. Wir liefern den
        // Original-String zurück (None wäre disruptiv für die UI).
        assert_eq!(clean_atc_model("$$:"), Some("$$:".to_string()));
    }

    #[test]
    fn aircraft_mismatch_resolves_with_prefix_strip() {
        use super::clean_atc_model;
        // End-to-end: vorher war exp=C750, act=$$:C750 mismatch.
        // Mit Strip ist act jetzt C750 und strict-equality matched.
        let raw_sim = "$$:C750";
        let bid_icao = "C750";
        let sim_clean = clean_atc_model(raw_sim).unwrap();
        assert_eq!(sim_clean, bid_icao);
        assert!(aircraft_types_match(bid_icao, &sim_clean));
    }

    /// v0.8.0: VPS-managed alias-overrides werden additiv zur hardcoded
    /// Tabelle akzeptiert. Demonstration: ein VA-Admin fügt im Webapp-
    /// Tab "wenn TBM9 erwartet, akzeptiere TBM-930" hinzu.
    #[test]
    fn vps_aliases_extend_hardcoded_table() {
        use super::{aircraft_types_match_with_extra, title_mentions_icao_with_extra};
        let mut vps: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        // Hardcoded hat keinen TBM-Alias — VA-Admin pflegt:
        vps.insert(
            "TBM9".to_string(),
            vec!["TBM-930".to_string(), "TBM930".to_string()],
        );
        assert!(aircraft_types_match_with_extra("TBM9", "TBM-930", &vps));
        assert!(aircraft_types_match_with_extra("TBM9", "TBM930", &vps));
        // Bidirektional (act ist die ICAO-Form, exp die Long-Form):
        assert!(aircraft_types_match_with_extra("TBM-930", "TBM9", &vps));
        // Title-Substring-Match: Sim-Title enthält den VPS-Alias.
        assert!(title_mentions_icao_with_extra(
            "Asobo TBM-930 [v1]",
            "TBM9",
            &vps,
        ));
        // Negativ-Sanity — anderes Aircraft matched nicht.
        assert!(!aircraft_types_match_with_extra("TBM9", "B738", &vps));
        // Wenn die VPS-Map leer ist, ist das Verhalten identisch zur
        // hardcoded-only-Variante:
        let empty = std::collections::HashMap::new();
        assert!(!aircraft_types_match_with_extra("TBM9", "TBM-930", &empty));
    }

    /// v0.13.16: der gemeinsame `aircraft_type_matches_sim`-Helper, den
    /// BEIDE — das flight_start-Gate UND der Auto-Start-Watcher (Option C)
    /// — nutzen. Deckt den Live-Case 2026-06-03 ab (Michael K / DWW306,
    /// D-CFHZ): Bid-ICAO "E55P" (Embraer Phenom 300) vs MSFS-ATC-Model
    /// "PHENOM 300E". Ohne Alias ein Mismatch (Phenom fehlt in der
    /// hardcoded Embraer-Sektion); mit einem VPS-Alias E55P→["PHENOM 300"]
    /// muss es matchen — und zwar im Gate wie im Watcher identisch.
    /// Externe QS (Codex, 16.09.2026) P1: `A380X` (FBW A380X) ist ein
    /// Fuenfsteller — er ueberlebt die Normalisierung jetzt und darf
    /// deshalb NICHT in einer Sackgasse enden. Die Kaskade muss ueber die
    /// Verkuerzung `A380` bis zur Variante `A388` durchziehen, sonst lebt
    /// der UAE-52-Fehler in dieser Schreibweise weiter.
    #[test]
    fn a380_schreibweisen_fuehren_alle_zur_spurweite() {
        for roh in ["A380X", "A380", "A380-800", "A388"] {
            let spur = crate::muster_fuer_typtabelle(Some(roh), |m| {
                landing_scoring::spurweite::spurweite_m(m)
            });
            assert_eq!(spur, Some(14.30), "{roh} muss die A388-Spurweite finden");
        }
    }

    /// Externe QS (Codex, 16.09.2026) P1: Fuenfsteller erreichen jetzt den
    /// Auto-Start-Abgleich. Ohne die Kaskade meldete `414AW` gegen die
    /// Buchung `C414` einen Mismatch — der Flug waere nicht mehr von
    /// selbst gestartet.
    #[test]
    fn auto_start_erkennt_fuenfstellige_sim_kennungen() {
        let leer = std::collections::HashMap::new();
        assert!(super::aircraft_type_matches_sim(
            Some("C414"),
            Some("414AW"),
            "Cessna 414AW Chancellor",
            &leer
        ));
        // Ein echter Mismatch bleibt einer.
        assert!(!super::aircraft_type_matches_sim(
            Some("C414"),
            Some("B738"),
            "Boeing 737-800",
            &leer
        ));
    }

    #[test]
    fn aircraft_type_matches_sim_e55p_phenom_via_vps_alias() {
        use super::aircraft_type_matches_sim;
        let empty = std::collections::HashMap::new();
        // Ohne Alias: echter Mismatch → Block (Gate zeigt Warnung,
        // Watcher feuert nicht).
        assert!(!aircraft_type_matches_sim(
            Some("E55P"),
            Some("PHENOM 300E"),
            "Asobo Phenom 300E",
            &empty,
        ));
        // Mit VPS-Alias E55P→"PHENOM 300": matcht (Substring greift auch
        // für "PHENOM 300E").
        let mut vps: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        vps.insert("E55P".to_string(), vec!["PHENOM 300".to_string()]);
        assert!(aircraft_type_matches_sim(
            Some("E55P"),
            Some("PHENOM 300E"),
            "Asobo Phenom 300E",
            &vps,
        ));
    }

    /// v0.15.12: der Resume-Mismatch-Check (`apply_pause_resume`, F7) nutzt
    /// jetzt denselben `aircraft_type_matches_sim`-Helper wie Gate + Watcher.
    /// Vorher war das ein stumpfer `snap_icao != bid_icao` Direktvergleich,
    /// der WEDER VPS- NOCH hardcoded-Aliase las → Falsch-Warnungen nach
    /// Resume. Dieser Test fixiert die zwei Klassen, die der alte Pfad
    /// fälschlich anmeckerte:
    ///   (a) VPS-Alias-Fall (Live 2026-06-06, tip.kant.ovh): X-Plane-Phenom
    ///       meldet acf_ICAO "PHENOM30" (OHNE Leerzeichen) vs Bid "E55P".
    ///       Der ältere Alias "PHENOM 300" deckt das NICHT ab (Substring
    ///       greift nicht); der neue "PHENOM30" schon.
    ///   (b) hardcoded-Fall: Bid "A359" vs Sim-Langform "A350-900" matcht
    ///       schon über die eingebaute Tabelle — ganz ohne VPS-Alias.
    #[test]
    fn aircraft_type_matches_sim_resume_v01512_aliases_apply() {
        use super::aircraft_type_matches_sim;

        // (a) Älterer Alias "PHENOM 300" deckt "PHENOM30" NICHT ab → Mismatch.
        let mut vps_old: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        vps_old.insert("E55P".to_string(), vec!["PHENOM 300".to_string()]);
        assert!(!aircraft_type_matches_sim(
            Some("E55P"),
            Some("PHENOM30"),
            "",
            &vps_old,
        ));

        // (a) Neuer Alias "PHENOM30" greift exakt → keine Resume-Warnung mehr.
        let mut vps_new: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        vps_new.insert(
            "E55P".to_string(),
            vec!["PHENOM 300".to_string(), "PHENOM30".to_string()],
        );
        assert!(aircraft_type_matches_sim(
            Some("E55P"),
            Some("PHENOM30"),
            "",
            &vps_new,
        ));

        // (b) Hardcoded-Tabelle greift auf dem Resume-Pfad jetzt ebenfalls:
        // A359-Bid vs A350-900-Langform → Match, ganz ohne VPS-Alias.
        let empty = std::collections::HashMap::new();
        assert!(aircraft_type_matches_sim(
            Some("A359"),
            Some("A350-900"),
            "",
            &empty,
        ));
    }

    /// Permissiv, wenn eine Seite unbekannt ist (analog zum Gate, das nur
    /// bei beidseitig bekanntem Typ blockt). Wichtig fürs Option-C-Watcher-
    /// Verhalten: ein fehlgeschlagener get_aircraft (expected = None) darf
    /// NIE einen legitimen Auto-Start blocken.
    #[test]
    fn aircraft_type_matches_sim_permissive_when_unknown() {
        use super::aircraft_type_matches_sim;
        let empty = std::collections::HashMap::new();
        // expected unbekannt (get_aircraft fehlgeschlagen) → permissiv.
        assert!(aircraft_type_matches_sim(
            None,
            Some("PHENOM 300E"),
            "x",
            &empty
        ));
        // sim-icao unbekannt (clean_atc_model lieferte None) → permissiv.
        assert!(aircraft_type_matches_sim(Some("E55P"), None, "x", &empty));
        // beide unbekannt → permissiv.
        assert!(aircraft_type_matches_sim(None, None, "", &empty));
        // Echter, beidseitig bekannter Mismatch bleibt blockiert.
        assert!(!aircraft_type_matches_sim(
            Some("B738"),
            Some("A320"),
            "",
            &empty
        ));
    }

    /// v0.13.17: Airborne-Rescue-Bedingung. Greift NUR in Boden-/Pre-Takeoff-
    /// Phasen + klar luftig + kein Reload-Glitch. Schuetzt vor dem „ganzer
    /// Flug haengt in Pushback"-Bug (Live IRM1140/IBE778).
    #[test]
    fn airborne_rescue_fires_only_when_stuck_on_ground_and_airborne() {
        use super::phase_stuck_on_ground_while_airborne as rescue;
        use sim_core::FlightPhase as P;
        // Stuck in Pushback aber bei FL (on_ground=false, AGL hoch) → rescue.
        assert!(rescue(P::Pushback, false, 38000.0, false));
        assert!(rescue(P::TaxiOut, false, 1200.0, false));
        assert!(rescue(P::TakeoffRoll, false, 600.0, false));
        // Am Boden → nie rescue.
        assert!(!rescue(P::Pushback, true, 0.0, false));
        // Niedrige AGL (Bounce/Glitch) → kein rescue.
        assert!(!rescue(P::Pushback, false, 50.0, false));
        // Reload-/Slew-Glitch-Tick → kein rescue, selbst wenn „luftig".
        assert!(!rescue(P::Pushback, false, 38000.0, true));
        // Normale Luft-Phasen sind keine Boden-Phasen → nie rescue
        // (verhindert Eingriff in den Normalfluss).
        assert!(!rescue(P::Climb, false, 5000.0, false));
        assert!(!rescue(P::Cruise, false, 38000.0, false));
        assert!(!rescue(P::Approach, false, 2000.0, false));
    }

    /// v0.16.9 (BCS8/FF767): das Rescue-Latch-Sicherheitsnetz füllt die
    /// Takeoff-Stats nach, wenn der echte →Takeoff-Übergang übersprungen
    /// wurde — sonst bleibt der OFP-Fuel-Sub-Score „no_actual_burn"-skipped.
    #[test]
    fn rescue_latch_fills_missing_takeoff_stats() {
        use super::{latch_takeoff_stats_if_missing, FlightStats};
        use chrono::Utc;
        let mut stats = FlightStats::default();
        let mut snap = sim_core::SimSnapshot::default();
        snap.fuel_total_kg = 15_930.5;
        snap.total_weight_kg = Some(105_741.0);
        snap.pitch_deg = 14.8; // Climb-Pitch — darf NICHT als Takeoff-Pitch landen
        let now = Utc::now();

        latch_takeoff_stats_if_missing(&mut stats, &snap, now);

        assert_eq!(stats.takeoff_at, Some(now));
        assert_eq!(stats.takeoff_fuel_kg, Some(15_930.5));
        assert_eq!(stats.takeoff_weight_kg, Some(105_741.0));
        // Attitude NUR vom Sampler — der Rescue-Snapshot steckt im Climb,
        // sein Pitch würde die Tail-Strike-Erkennung falsch füttern.
        assert_eq!(stats.takeoff_pitch_deg, None);
        assert_eq!(stats.takeoff_bank_deg, None);
    }

    /// Sampler-Werte (frame-genauer Wheels-up) werden bevorzugt, wenn der
    /// 50Hz-Sampler den Edge trotz festhängender Phase gesehen hat.
    #[test]
    fn rescue_latch_prefers_sampler_edge() {
        use super::{latch_takeoff_stats_if_missing, FlightStats};
        use chrono::Utc;
        let mut stats = FlightStats::default();
        let edge_at = Utc::now() - chrono::Duration::seconds(42);
        stats.sampler_takeoff_at = Some(edge_at);
        stats.sampler_takeoff_pitch_deg = Some(8.2);
        stats.sampler_takeoff_bank_deg = Some(-0.6);
        let mut snap = sim_core::SimSnapshot::default();
        snap.fuel_total_kg = 9_000.0;
        let now = Utc::now();

        latch_takeoff_stats_if_missing(&mut stats, &snap, now);

        assert_eq!(stats.takeoff_at, Some(edge_at));
        assert_eq!(stats.takeoff_pitch_deg, Some(8.2));
        assert_eq!(stats.takeoff_bank_deg, Some(-0.6));
    }

    /// get_or_insert-Semantik: bereits gelatchte Werte (echter Takeoff,
    /// Resume-Restore) werden NIE überschrieben.
    #[test]
    fn rescue_latch_never_overwrites() {
        use super::{latch_takeoff_stats_if_missing, FlightStats};
        use chrono::Utc;
        let mut stats = FlightStats::default();
        let real_takeoff = Utc::now() - chrono::Duration::seconds(90);
        stats.takeoff_at = Some(real_takeoff);
        stats.takeoff_fuel_kg = Some(17_146.1);
        stats.takeoff_weight_kg = Some(138_305.0);
        stats.takeoff_pitch_deg = Some(7.5);
        stats.takeoff_bank_deg = Some(0.1);
        let mut snap = sim_core::SimSnapshot::default();
        snap.fuel_total_kg = 15_000.0;
        snap.total_weight_kg = Some(100_000.0);

        latch_takeoff_stats_if_missing(&mut stats, &snap, Utc::now());

        assert_eq!(stats.takeoff_at, Some(real_takeoff));
        assert_eq!(stats.takeoff_fuel_kg, Some(17_146.1));
        assert_eq!(stats.takeoff_weight_kg, Some(138_305.0));
        assert_eq!(stats.takeoff_pitch_deg, Some(7.5));
        assert_eq!(stats.takeoff_bank_deg, Some(0.1));
    }

    /// Der echte Takeoff-Latch (extrahiert, Overwrite-Semantik) — Verhalten
    /// byte-identisch zu den drei früheren Inline-Blöcken: Snapshot-Fallback
    /// für Pitch/Bank, ZFW+Fuel-Fallback fürs Gewicht.
    #[test]
    fn real_takeoff_latch_keeps_legacy_semantics() {
        use super::{latch_takeoff_stats, FlightStats};
        use chrono::Utc;
        let mut stats = FlightStats::default();
        let mut snap = sim_core::SimSnapshot::default();
        snap.fuel_total_kg = 6_000.0;
        snap.pitch_deg = 9.1;
        snap.bank_deg = -0.3;
        snap.total_weight_kg = None;
        snap.zfw_kg = Some(50_000.0);
        let now = Utc::now();

        latch_takeoff_stats(&mut stats, &snap, now);

        assert_eq!(stats.takeoff_at, Some(now));
        assert_eq!(stats.takeoff_fuel_kg, Some(6_000.0));
        // Ohne Sampler-Werte: Snapshot-Fallback (wie bisher).
        assert_eq!(stats.takeoff_pitch_deg, Some(9.1));
        assert_eq!(stats.takeoff_bank_deg, Some(-0.3));
        // total_weight fehlt → ZFW + Fuel.
        assert_eq!(stats.takeoff_weight_kg, Some(56_000.0));
    }

    /// v0.13.18: FOB-abgeleiteter Fuel-Flow-Fallback (Aerosoft/iniBuilds
    /// A340-600 — `ENG FUEL FLOW PPH`-SimVar tot, FOB läuft aber runter).
    #[test]
    fn fuel_flow_derived_from_fob_delta() {
        use super::derive_fuel_flow_kg_per_h as ff;
        // Fenster zu kurz → None (Aufrufer hält den letzten Wert, kein Flackern).
        assert_eq!(ff(30_000.0, 29_950.0, 30.0), None);
        // 100 kg in 60 s → 6000 kg/h.
        let r = ff(30_000.0, 29_900.0, 60.0).unwrap();
        assert!((r - 6000.0).abs() < 1.0, "erwartete ~6000, war {r}");
        // Tankung (FOB steigt) → 0, NICHT negativ.
        assert_eq!(ff(29_900.0, 30_500.0, 60.0), Some(0.0));
        // Unplausibler Sprung (Dump/Reset) → 0 (geclampt auf FF_MAX).
        assert_eq!(ff(50_000.0, 0.0, 60.0), Some(0.0));
        // Triebwerke aus (kein Burn) → 0.
        assert_eq!(ff(20_000.0, 20_000.0, 90.0), Some(0.0));
    }

    // ─── v0.16.10 Fuel-Used-Fallback (FOB-Delta, Live-Befund GLO4587) ───

    /// PMDG 738: SimVar-fuel_used bleibt 0.0 während der FOB runterläuft
    /// → Fallback aus Baseline−FOB. Baseline latcht beim ersten Tick.
    #[test]
    fn fuel_used_fallback_derives_from_fob_delta() {
        use super::fuel_used_effective_kg as fu;
        let mut initial: Option<f32> = None;
        // Erster Tick: Baseline latcht, Delta 0 → SimVar-Wert (0) durchgereicht.
        assert_eq!(fu(&mut initial, 0.0, 8_000.0, true, None), 0.0);
        assert_eq!(initial, Some(8_000.0));
        // Kleines Delta (< 5 kg, Mess-Rauschen am Gate) → weiter 0.
        assert_eq!(fu(&mut initial, 0.0, 7_997.0, true, Some(8_000.0)), 0.0);
        // Cruise: 1500 kg verbrannt, SimVar tot → FOB-Delta.
        let used = fu(&mut initial, 0.0, 6_500.0, false, Some(6_520.0));
        assert!((used - 1_500.0).abs() < 0.01, "erwartete 1500, war {used}");
        // Baseline bleibt stabil über die Ticks.
        assert_eq!(initial, Some(8_000.0));
    }

    /// Kommt die SimVar je zum Leben (≥ 0.5 kg), gewinnt sie — auch wenn
    /// das FOB-Delta größer wäre.
    #[test]
    fn fuel_used_fallback_prefers_live_simvar() {
        use super::fuel_used_effective_kg as fu;
        let mut initial: Option<f32> = Some(8_000.0);
        // SimVar lebt (1234 kg) → authoritativ, kein Delta-Override.
        assert_eq!(
            fu(&mut initial, 1_234.0, 6_500.0, false, Some(6_510.0)),
            1_234.0
        );
        // Knapp unter der Alive-Schwelle (< 0.5) zählt als tot → Delta.
        let used = fu(&mut initial, 0.4, 6_500.0, false, Some(6_500.0));
        assert!((used - 1_500.0).abs() < 0.01, "erwartete 1500, war {used}");
    }

    /// Mid-Flight-/Gate-Refuel: FOB steigt > 50 kg über die Baseline →
    /// Baseline re-latcht aufs neue Maximum, Delta startet wieder bei ~0
    /// und wird NIE negativ.
    #[test]
    fn fuel_used_fallback_relatches_on_refuel() {
        use super::fuel_used_effective_kg as fu;
        let mut initial: Option<f32> = Some(8_000.0);
        // FOB leicht über Baseline (< 50 kg drüber, z.B. Dichte-Jitter):
        // KEIN Re-Latch, Delta negativ → SimVar-Wert (0), nicht negativ.
        assert_eq!(fu(&mut initial, 0.0, 8_030.0, true, Some(8_000.0)), 0.0);
        assert_eq!(initial, Some(8_000.0));
        // Echtes Refuel: 8000 → 12000 kg → Re-Latch auf 12000.
        assert_eq!(fu(&mut initial, 0.0, 12_000.0, true, Some(8_030.0)), 0.0);
        assert_eq!(initial, Some(12_000.0));
        // Danach zählt das Delta von der NEUEN Baseline.
        let used = fu(&mut initial, 0.0, 11_000.0, false, Some(11_050.0));
        assert!((used - 1_000.0).abs() < 0.01, "erwartete 1000, war {used}");
    }

    /// v0.16.10 QS (M1): Gate-DEFUEL — instantaner FOB-Abfall > 200 kg
    /// VOR dem Takeoff re-latcht die Baseline nach unten. Ohne den
    /// Guard wäre fuel_used den ganzen Flug um die Defuel-Menge zu hoch
    /// (SimBrief-Korrektur nach unten am Gate).
    #[test]
    fn fuel_used_fallback_relatches_down_on_gate_defuel() {
        use super::fuel_used_effective_kg as fu;
        let mut initial: Option<f32> = Some(10_000.0);
        // Defuel am Gate: 10 t → 7 t in einem Tick → Re-Latch auf 7 t.
        assert_eq!(fu(&mut initial, 0.0, 7_000.0, true, Some(10_000.0)), 0.0);
        assert_eq!(initial, Some(7_000.0));
        // Danach zählt das Delta von der NEUEN Baseline.
        let used = fu(&mut initial, 0.0, 6_400.0, false, Some(6_420.0));
        assert!((used - 600.0).abs() < 0.01, "erwartete 600, war {used}");
    }

    /// v0.16.10 QS (M1): gradueller Boden-Burn (APU/Taxi, wenige kg pro
    /// Tick) bleibt unter der 200-kg-Tick-Schwelle → KEIN Re-Latch,
    /// fuel_used akkumuliert normal — auch wenn der KUMULIERTE Abfall
    /// längst > 200 kg ist.
    #[test]
    fn fuel_used_fallback_gradual_ground_burn_keeps_baseline() {
        use super::fuel_used_effective_kg as fu;
        let mut initial: Option<f32> = Some(8_000.0);
        let mut prev = 8_000.0_f32;
        let mut fob = 8_000.0_f32;
        // 50 Ticks à 8 kg Taxi-/APU-Burn = 400 kg kumuliert.
        for _ in 0..50 {
            fob -= 8.0;
            let _ = fu(&mut initial, 0.0, fob, true, Some(prev));
            prev = fob;
        }
        assert_eq!(initial, Some(8_000.0));
        let used = fu(&mut initial, 0.0, fob, true, Some(prev));
        assert!((used - 400.0).abs() < 0.5, "erwartete ~400, war {used}");
    }

    /// v0.16.10 QS (M1): airborne re-latcht NIE nach unten — ein großer
    /// Drop (Fuel-Jettison / Sensor-Glitch) zählt konservativ als
    /// verbraucht, die Baseline bleibt stehen.
    #[test]
    fn fuel_used_fallback_airborne_drop_never_relatches() {
        use super::fuel_used_effective_kg as fu;
        let mut initial: Option<f32> = Some(8_000.0);
        let used = fu(&mut initial, 0.0, 5_000.0, false, Some(8_000.0));
        assert!((used - 3_000.0).abs() < 0.01, "erwartete 3000, war {used}");
        assert_eq!(initial, Some(8_000.0));
    }

    /// FOB = 0 (Sim-Reload-Glitch / leerer Tank) darf die Baseline nicht
    /// auf 0 latchen.
    #[test]
    fn fuel_used_fallback_ignores_zero_fob_for_latch() {
        use super::fuel_used_effective_kg as fu;
        let mut initial: Option<f32> = None;
        assert_eq!(fu(&mut initial, 0.0, 0.0, true, None), 0.0);
        assert_eq!(initial, None);
        // Erster echter Fuel-Tick latcht dann normal.
        assert_eq!(fu(&mut initial, 0.0, 5_000.0, true, Some(0.0)), 0.0);
        assert_eq!(initial, Some(5_000.0));
    }

    // ─── v0.16.10 Premium-Activity-Log: DebouncedLogState ───────────────

    /// Erster Wert latcht IMMER stumm; eine Änderung committet erst nach
    /// dem Debounce-Fenster.
    #[test]
    fn debounced_log_state_first_value_latches_silently() {
        use super::DebouncedLogState;
        use chrono::Duration;
        let t0 = chrono::Utc::now();
        let mut s: DebouncedLogState<String> = DebouncedLogState::default();
        // Erster Wert → stumm.
        assert!(!s.update("NAV | ALT".to_string(), t0, 3));
        // Gleicher Wert → nichts.
        assert!(!s.update("NAV | ALT".to_string(), t0 + Duration::seconds(1), 3));
        // Änderung: Tick 1 startet den Timer, noch kein Log.
        assert!(!s.update("LOC | G/S".to_string(), t0 + Duration::seconds(10), 3));
        // 2 s später — Fenster (3 s) noch nicht voll.
        assert!(!s.update("LOC | G/S".to_string(), t0 + Duration::seconds(12), 3));
        // 3 s später — committet, log jetzt.
        assert!(s.update("LOC | G/S".to_string(), t0 + Duration::seconds(13), 3));
        // Direkt danach: Wert ist gelatcht → kein Doppel-Log.
        assert!(!s.update("LOC | G/S".to_string(), t0 + Duration::seconds(14), 3));
    }

    /// Ein Puls kürzer als das Fenster produziert GAR KEINEN Eintrag
    /// (Original-Motivation: Fenix-LVar-Pulse, Annunciator-Tests).
    #[test]
    fn debounced_log_state_swallows_short_pulse() {
        use super::DebouncedLogState;
        use chrono::Duration;
        let t0 = chrono::Utc::now();
        let mut s: DebouncedLogState<bool> = DebouncedLogState::default();
        assert!(!s.update(false, t0, 2)); // stumm gelatcht
                                          // Puls: true für 1 s, dann wieder false.
        assert!(!s.update(true, t0 + Duration::seconds(5), 2));
        assert!(!s.update(false, t0 + Duration::seconds(6), 2));
        // Zurück auf dem gelatchten Wert → Pending verworfen; auch ein
        // erneutes true muss das Fenster wieder VOLL halten.
        assert!(!s.update(true, t0 + Duration::seconds(7), 2));
        assert!(s.update(true, t0 + Duration::seconds(9), 2));
    }

    /// Wechselt der Pending-Wert mitten im Fenster, startet der Timer neu
    /// (nur ein STABILER neuer Wert committet).
    #[test]
    fn debounced_log_state_restarts_timer_on_pending_change() {
        use super::DebouncedLogState;
        use chrono::Duration;
        let t0 = chrono::Utc::now();
        let mut s: DebouncedLogState<String> = DebouncedLogState::default();
        assert!(!s.update("CL".to_string(), t0, 3));
        assert!(!s.update("FLX/MCT".to_string(), t0 + Duration::seconds(10), 3));
        // Anderer Wert bevor FLX/MCT stabil war → Timer neu ab hier.
        assert!(!s.update("TOGA".to_string(), t0 + Duration::seconds(12), 3));
        // 2 s — für TOGA noch nicht voll (auch wenn FLX/MCT-Start 5 s her ist).
        assert!(!s.update("TOGA".to_string(), t0 + Duration::seconds(14), 3));
        assert!(s.update("TOGA".to_string(), t0 + Duration::seconds(15), 3));
    }

    /// v0.16.10 QS (B1): `update_first_logs` — der ERSTE Wert loggt
    /// SOFORT (Approach-Speeds: erster Some-Wert = FMS-Perf-Eingabe-
    /// Zeitstempel), danach normale Latch+Debounce-Semantik.
    #[test]
    fn debounced_log_state_first_logs_variant() {
        use super::DebouncedLogState;
        use chrono::Duration;
        let t0 = chrono::Utc::now();
        type Key = (Option<i64>, Option<i64>, Option<i64>);
        let mut s: DebouncedLogState<Key> = DebouncedLogState::default();
        // Erster Wert (5-kt-Bucket-Key) → loggt sofort.
        assert!(s.update_first_logs((Some(28), Some(27), None), t0, 10));
        // Gleicher Bucket → still (1-kt-Drift bleibt im Bucket).
        assert!(!s.update_first_logs((Some(28), Some(27), None), t0 + Duration::seconds(1), 10));
        // Bucket-Wechsel: erst nach 10 s Stabilität.
        assert!(!s.update_first_logs((Some(29), Some(28), None), t0 + Duration::seconds(20), 10));
        assert!(!s.update_first_logs((Some(29), Some(28), None), t0 + Duration::seconds(25), 10));
        assert!(s.update_first_logs((Some(29), Some(28), None), t0 + Duration::seconds(31), 10));
        // Danach gelatcht → kein Doppel-Log.
        assert!(!s.update_first_logs((Some(29), Some(28), None), t0 + Duration::seconds(32), 10));
    }

    // ─── v0.16.10 QS (M5): Connect-Grace für die Premium-Logger ─────────

    /// Allererster Tick öffnet das Warmup-Fenster; innerhalb der 10 s
    /// wird alles geblockt, danach ist der Stream frei.
    #[test]
    fn premium_log_warmup_blocks_first_seconds_after_stream_start() {
        use super::premium_log_warmup_active as warm;
        use chrono::Duration;
        let t0 = chrono::Utc::now();
        let mut last: Option<chrono::DateTime<chrono::Utc>> = None;
        let mut until: Option<chrono::DateTime<chrono::Utc>> = None;
        // Allererster Tick → Warmup aktiv.
        assert!(warm(&mut last, &mut until, t0));
        // Innerhalb des 10-s-Fensters bleibt alles geblockt.
        assert!(warm(&mut last, &mut until, t0 + Duration::seconds(5)));
        assert!(warm(&mut last, &mut until, t0 + Duration::seconds(9)));
        // Nach Ablauf: frei (erster Post-Grace-Wert latcht dann stumm
        // über die First-Silent-Semantik der Tracker).
        assert!(!warm(&mut last, &mut until, t0 + Duration::seconds(11)));
        assert!(!warm(&mut last, &mut until, t0 + Duration::seconds(13)));
    }

    /// Eine Tick-Lücke > 15 s (Sim-Disconnect/Pause/Resume) öffnet das
    /// Warmup-Fenster erneut; ein normaler 0,5-3-s-Takt nicht.
    #[test]
    fn premium_log_warmup_reopens_after_tick_gap() {
        use super::premium_log_warmup_active as warm;
        use chrono::Duration;
        let t0 = chrono::Utc::now();
        let mut last: Option<chrono::DateTime<chrono::Utc>> = None;
        let mut until: Option<chrono::DateTime<chrono::Utc>> = None;
        // Initial-Warmup durchlaufen, Stream läuft frei.
        assert!(warm(&mut last, &mut until, t0));
        assert!(!warm(&mut last, &mut until, t0 + Duration::seconds(11)));
        assert!(!warm(&mut last, &mut until, t0 + Duration::seconds(13)));
        // Lücke von 20 s (> Gap-Schwelle 15 s) → Re-Start, Warmup öffnet.
        let t1 = t0 + Duration::seconds(33);
        assert!(warm(&mut last, &mut until, t1));
        assert!(warm(&mut last, &mut until, t1 + Duration::seconds(9)));
        // Fenster abgelaufen → wieder frei.
        assert!(!warm(&mut last, &mut until, t1 + Duration::seconds(11)));
    }

    // ─── v0.16.10 Premium-Activity-Log: Format-Helper ────────────────────

    /// Generic-FMA-Zeile: None-Teile weggelassen, alles None ⇒ None.
    #[test]
    fn fmt_fma_line_omits_none_parts() {
        use super::fmt_fma_line;
        assert_eq!(
            fmt_fma_line(Some("NAV"), Some("ALT"), Some("SPEED")),
            Some("NAV | ALT | SPEED".to_string())
        );
        assert_eq!(
            fmt_fma_line(Some("LOC"), None, Some("SPEED")),
            Some("LOC | SPEED".to_string())
        );
        assert_eq!(
            fmt_fma_line(None, Some("G/S"), None),
            Some("G/S".to_string())
        );
        assert_eq!(fmt_fma_line(None, None, None), None);
        // Leere/Whitespace-Strings zählen wie None.
        assert_eq!(fmt_fma_line(Some(""), Some("  "), None), None);
        assert_eq!(
            fmt_fma_line(Some(""), Some("VNAV PTH"), None),
            Some("VNAV PTH".to_string())
        );
    }

    /// V-Speeds-Zeile: Rundung, FLEX-Suffix, TOGA-Suffix, FLEX > TOGA.
    #[test]
    fn fmt_vspeeds_line_suffix_rules() {
        use super::fmt_vspeeds_line;
        // Basis (keine FLEX, kein Gate).
        assert_eq!(
            fmt_vspeeds_line(141.6, 146.4, 153.0, None, None),
            "V-Speeds: V1 142 · VR 146 · V2 153"
        );
        // FLEX gesetzt → FLEX-Suffix.
        assert_eq!(
            fmt_vspeeds_line(142.0, 146.0, 153.0, Some(47.2), None),
            "V-Speeds: V1 142 · VR 146 · V2 153 · FLEX 47°C"
        );
        // Kein FLEX, Hebel im TOGA-Detent → TOGA-Suffix.
        assert_eq!(
            fmt_vspeeds_line(142.0, 146.0, 153.0, None, Some("TOGA")),
            "V-Speeds: V1 142 · VR 146 · V2 153 · TOGA"
        );
        // FLEX hat Vorrang vor TOGA (FLEX-Takeoff mit Lever kurz in TOGA).
        assert_eq!(
            fmt_vspeeds_line(142.0, 146.0, 153.0, Some(40.0), Some("TOGA")),
            "V-Speeds: V1 142 · VR 146 · V2 153 · FLEX 40°C"
        );
        // Andere Gates (CL) erzeugen KEIN Suffix.
        assert_eq!(
            fmt_vspeeds_line(142.0, 146.0, 153.0, None, Some("CL")),
            "V-Speeds: V1 142 · VR 146 · V2 153"
        );
    }

    /// Approach-Speeds-Zeile: None-Teile weggelassen, alles None ⇒ None.
    #[test]
    fn fmt_approach_speeds_line_omits_none() {
        use super::fmt_approach_speeds_line;
        assert_eq!(
            fmt_approach_speeds_line(Some(138.4), Some(132.0), Some(135.0)),
            Some("Approach-Speeds: VAPP 138 · VLS 132 · VREF 135".to_string())
        );
        // Airbus-Fall: nur VAPP+VLS.
        assert_eq!(
            fmt_approach_speeds_line(Some(138.0), Some(132.0), None),
            Some("Approach-Speeds: VAPP 138 · VLS 132".to_string())
        );
        // Boeing-Fall: nur VREF.
        assert_eq!(
            fmt_approach_speeds_line(None, None, Some(141.0)),
            Some("Approach-Speeds: VREF 141".to_string())
        );
        assert_eq!(fmt_approach_speeds_line(None, None, None), None);
    }

    // ─── v0.16.10 Premium-Activity-Log: Einmal-pro-Landung + Below-G/S ──

    /// Umkehrschub/Ground-Spoiler: Rising-Edge am Boden loggt genau
    /// einmal pro Landung; airborne re-armt (Touch-and-Go-Szenario).
    #[test]
    fn once_per_landing_rising_edge_logs_once_and_rearms_airborne() {
        use super::once_per_landing_rising_edge as edge;
        let mut last_seen: Option<bool> = None;
        let mut logged = false;
        // None-Quelle (Profil liefert das Feld nicht) → nie.
        assert!(!edge(None, true, true, &mut last_seen, &mut logged));
        // Erster Wert latcht stumm (auch am Boden).
        assert!(!edge(Some(false), true, true, &mut last_seen, &mut logged));
        // Rising-Edge am Boden (Rollout-Speed) → loggt.
        assert!(edge(Some(true), true, true, &mut last_seen, &mut logged));
        // Hält an / flackert → kein zweites Log in derselben Landung.
        assert!(!edge(Some(true), true, true, &mut last_seen, &mut logged));
        assert!(!edge(Some(false), true, true, &mut last_seen, &mut logged));
        assert!(!edge(Some(true), true, true, &mut last_seen, &mut logged));
        // Touch-and-Go: airborne → re-arm…
        assert!(!edge(Some(false), false, true, &mut last_seen, &mut logged));
        // …nächste Landung bekommt wieder einen Eintrag.
        assert!(edge(Some(true), true, true, &mut last_seen, &mut logged));
    }

    /// Rising-Edge in der Luft loggt NICHT (Umkehrschub-Test im Flug /
    /// Spoiler-Lever-Bewegung zählen nicht als Landung).
    #[test]
    fn once_per_landing_rising_edge_ignores_airborne_edge() {
        use super::once_per_landing_rising_edge as edge;
        let mut last_seen: Option<bool> = None;
        let mut logged = false;
        assert!(!edge(Some(false), false, true, &mut last_seen, &mut logged));
        assert!(!edge(Some(true), false, true, &mut last_seen, &mut logged));
        // Wieder am Boden mit value=true: kein NEUER Rising-Edge
        // (true→true) → kein Log; erst false→true am Boden loggt.
        assert!(!edge(Some(true), true, true, &mut last_seen, &mut logged));
        assert!(!edge(Some(false), true, true, &mut last_seen, &mut logged));
        assert!(edge(Some(true), true, true, &mut last_seen, &mut logged));
    }

    /// Resume mitten im Rollout (erster Wert = true) latcht stumm.
    #[test]
    fn once_per_landing_rising_edge_first_true_is_silent() {
        use super::once_per_landing_rising_edge as edge;
        let mut last_seen: Option<bool> = None;
        let mut logged = false;
        assert!(!edge(Some(true), true, true, &mut last_seen, &mut logged));
        assert!(!edge(Some(true), true, true, &mut last_seen, &mut logged));
    }

    /// v0.16.10 QS (Minor 6): Rising-Edge am Boden UNTER der
    /// Groundspeed-Schwelle (Taxi-Lever-Check, Spoiler-Test am Stand)
    /// loggt NICHT — und verbraucht den Einmal-pro-Landung-Latch auch
    /// nicht (eine echte spätere Landung loggt weiterhin).
    #[test]
    fn once_per_landing_rising_edge_requires_groundspeed() {
        use super::once_per_landing_rising_edge as edge;
        let mut last_seen: Option<bool> = None;
        let mut logged = false;
        assert!(!edge(Some(false), true, false, &mut last_seen, &mut logged));
        // Taxi-Check: Reverser kurz gezogen bei 15 kt → kein Log…
        assert!(!edge(Some(true), true, false, &mut last_seen, &mut logged));
        assert!(
            !logged,
            "Taxi-Check darf den Einmal-Latch nicht verbrauchen"
        );
        assert!(!edge(Some(false), true, false, &mut last_seen, &mut logged));
        // …die echte Landung später loggt normal (airborne → Touchdown
        // → Reverser bei Rollout-Speed).
        assert!(!edge(Some(false), false, true, &mut last_seen, &mut logged));
        assert!(edge(Some(true), true, true, &mut last_seen, &mut logged));
    }

    /// Below-G/S: einmal pro Approach, re-arm erst nach 30 s clear;
    /// erster Wert true latcht stumm als "schon geloggt".
    #[test]
    fn below_gs_logs_once_then_rearms_after_clear_window() {
        use super::below_gs_should_log as gs;
        use chrono::Duration;
        let t0 = chrono::Utc::now();
        let mut last_seen: Option<bool> = None;
        let mut logged = false;
        let mut clear_since: Option<chrono::DateTime<chrono::Utc>> = None;
        // Erster Wert (false) latcht stumm.
        assert!(!gs(
            false,
            t0,
            &mut last_seen,
            &mut logged,
            &mut clear_since
        ));
        // Alert feuert → loggt.
        assert!(gs(
            true,
            t0 + Duration::seconds(5),
            &mut last_seen,
            &mut logged,
            &mut clear_since
        ));
        // Flackern (clear < 30 s, wieder true) → kein zweites Log.
        assert!(!gs(
            false,
            t0 + Duration::seconds(10),
            &mut last_seen,
            &mut logged,
            &mut clear_since
        ));
        assert!(!gs(
            true,
            t0 + Duration::seconds(15),
            &mut last_seen,
            &mut logged,
            &mut clear_since
        ));
        // Alert klärt sich und BLEIBT > 30 s clear → re-arm…
        assert!(!gs(
            false,
            t0 + Duration::seconds(20),
            &mut last_seen,
            &mut logged,
            &mut clear_since
        ));
        assert!(!gs(
            false,
            t0 + Duration::seconds(55),
            &mut last_seen,
            &mut logged,
            &mut clear_since
        ));
        // …der nächste Alert loggt wieder.
        assert!(gs(
            true,
            t0 + Duration::seconds(60),
            &mut last_seen,
            &mut logged,
            &mut clear_since
        ));
    }

    /// Below-G/S: beim Connect bereits aktiver Alert zählt als geloggt
    /// (kein nachträglicher Eintrag), erst der NÄCHSTE Approach loggt.
    #[test]
    fn below_gs_first_true_latches_as_already_logged() {
        use super::below_gs_should_log as gs;
        use chrono::Duration;
        let t0 = chrono::Utc::now();
        let mut last_seen: Option<bool> = None;
        let mut logged = false;
        let mut clear_since: Option<chrono::DateTime<chrono::Utc>> = None;
        // Erster Wert TRUE → stumm + als geloggt gelatcht.
        assert!(!gs(true, t0, &mut last_seen, &mut logged, &mut clear_since));
        assert!(logged);
        // Kurzes clear + wieder true (< 30 s) → weiter still.
        assert!(!gs(
            false,
            t0 + Duration::seconds(5),
            &mut last_seen,
            &mut logged,
            &mut clear_since
        ));
        assert!(!gs(
            true,
            t0 + Duration::seconds(10),
            &mut last_seen,
            &mut logged,
            &mut clear_since
        ));
    }

    // ─── v0.7.3 Cargo-Aliases (Spec §4 HOHE-Prio Arbeitsliste) ──────
    // Pro Familie 1 Match-Test + 1 Mismatch-Test (Spec Leitprinzip).

    #[test]
    fn b748f_matches_747_8_freighter() {
        assert!(aircraft_types_match("B748F", "747-8F"));
        assert!(aircraft_types_match("B748F", "747-8 Freighter"));
        // B748 (Pax-Bid) akzeptiert via Long-Form "747-8" Substring
        // auch -8F-Frachter — Cargo-Pragmatismus §7.3.
        assert!(aircraft_types_match("B748", "747-8 Freighter"));
    }

    #[test]
    fn b748f_does_not_match_other_widebodies() {
        assert!(!aircraft_types_match("B748F", "B77W"));
        assert!(!aircraft_types_match("B748F", "A388"));
    }

    #[test]
    fn b74f_matches_747_400_freighter() {
        assert!(aircraft_types_match("B74F", "747-400F"));
        assert!(!aircraft_types_match("B74F", "B763F"));
    }

    #[test]
    fn b752f_matches_757_200f() {
        assert!(aircraft_types_match("B752F", "757-200F"));
        // B752 (Pax-Bid) akzeptiert auch -200F via "757-200" Substring
        assert!(aircraft_types_match("B752", "757-200F"));
        assert!(!aircraft_types_match("B752F", "B763"));
    }

    #[test]
    fn b763f_matches_767_300f() {
        assert!(aircraft_types_match("B763F", "767-300F"));
        // B763 (Pax-Bid) akzeptiert auch -300F via "767-300" Substring
        assert!(aircraft_types_match("B763", "767-300F"));
        assert!(!aircraft_types_match("B763F", "A332F"));
    }

    #[test]
    fn b762f_matches_767_200f() {
        assert!(aircraft_types_match("B762F", "767-200F"));
        assert!(!aircraft_types_match("B762F", "B763F"));
    }

    #[test]
    fn a332f_matches_a330_200f() {
        assert!(aircraft_types_match("A332F", "A330-200F"));
        // A332 (Pax-Bid) akzeptiert auch -200F via "A330-200" Substring
        assert!(aircraft_types_match("A332", "A330-200F"));
        assert!(!aircraft_types_match("A332F", "B763F"));
    }

    // ─── v0.7.4 Round-1 Review Polish ───────────────────────────────

    /// P1: " FREIGHTER" Long-Form muss fuer alle Cargo-Aliase matchen
    /// (manche Sim-Addons schreiben "757-200 Freighter" statt nur "F").
    #[test]
    fn cargo_aliases_match_freighter_long_form() {
        assert!(aircraft_types_match("B74F", "747-400 Freighter"));
        assert!(aircraft_types_match("B748F", "747-8 Freighter"));
        assert!(aircraft_types_match("B752F", "757-200 Freighter"));
        assert!(aircraft_types_match("B763F", "767-300 Freighter"));
        assert!(aircraft_types_match("B762F", "767-200 Freighter"));
        assert!(aircraft_types_match("A332F", "A330-200 Freighter"));
    }

    /// Frachter-Buchung + Passagier-Muster im Simulator: **akzeptiert**,
    /// seit 16.09.2026. Vorher war das je nach Schreibweise mal
    /// blockiert (`MD-11`), mal nicht (`MD11`) — und zwar ohne dass
    /// AeroACARS den Unterschied wirklich sehen konnte: Erkannt wird nur
    /// ein `F` am Ende der Kennung oder „Freighter" im Titel. Ein
    /// Add-on, das seine MD-11F als `MD-11` meldet, kostete den Piloten
    /// den Auto-Start fuer eine Schreibweise, nicht fuer das falsche
    /// Flugzeug. Entscheidung Thomas K.: verglichen wird das
    /// Grundmuster; wer Fracht fliegt, nimmt den Frachter — das ist
    /// Dispatch-Disziplin, nicht Sache dieser Zeichenkette.
    ///
    /// Der Rest der Zuordnung bleibt streng: ein echtes anderes Muster
    /// (A320 statt B738) faellt weiterhin durch.
    #[test]
    fn cargo_bid_akzeptiert_pax_sim_derselben_baureihe() {
        assert!(aircraft_types_match("MD11F", "MD-11"));
        assert!(aircraft_types_match("MD11F", "MD11"));
        assert!(aircraft_types_match("MD11F", "MD-11F"));
        // Externe QS (Codex, 16.09.2026): die Oeffnung muss fuer JEDE
        // Schreibweise gleich wirken, nicht nur fuer die MD-11 — vorher
        // war `B74F` gegen `747-400` weiterhin ein Mismatch.
        assert!(aircraft_types_match("B74F", "747-400"));
        assert!(aircraft_types_match("B748F", "747-8"));
        assert!(aircraft_types_match("B752F", "757-200"));
        assert!(aircraft_types_match("B762F", "767-200"));
        assert!(aircraft_types_match("B763F", "767-300"));
        assert!(aircraft_types_match("A332F", "A330-200"));
        // Ein echtes anderes Muster bleibt ein Mismatch.
        assert!(!aircraft_types_match("MD11F", "B763"));
        assert!(!aircraft_types_match("B763F", "A320"));
        assert!(!aircraft_types_match("B748F", "A388"));
    }

    /// Nicht jedes `F` am Ende ist ein Frachter: bei der Mooney `M20F` ist
    /// es ein Buchstabe der Baureihe. Die Oeffnung laeuft deshalb ueber
    /// eine benannte Liste, nicht ueber die Zeichenkette (externe QS,
    /// Codex 16.09.2026 P2).
    #[test]
    fn variantenbuchstabe_f_oeffnet_nichts() {
        assert!(super::frachter_basismuster("M20F").is_none());
        assert!(!aircraft_types_match("M20F", "M20"));
        assert!(super::frachter_basismuster("MD11F").is_some());
    }

    #[test]
    fn pax_bid_accepts_cargo_sim_pragmatism() {
        // Pax-Bid → Cargo-Sim akzeptiert (umgekehrte Richtung okay)
        assert!(aircraft_types_match("B744", "747-400F"));
        assert!(aircraft_types_match("B748", "747-8F"));
        assert!(aircraft_types_match("B752", "757-200F"));
        assert!(aircraft_types_match("B763", "767-300F"));
        assert!(aircraft_types_match("B762", "767-200F"));
        assert!(aircraft_types_match("A332", "A330-200F"));
        assert!(aircraft_types_match("MD11", "MD-11F"));
    }

    /// P2: A359 darf NICHT A350-1000 matchen (war v0.7.3 Bug durch
    /// "A350"-Substring-Alias). v0.7.4 hat den Alias entfernt.
    #[test]
    fn a359_does_not_match_a350_1000() {
        assert!(!aircraft_types_match("A359", "A350-1000"));
        // umgekehrt
        assert!(!aircraft_types_match("A35K", "A350-900"));
        // Sanity: was matchen soll, matched weiter
        assert!(aircraft_types_match("A359", "A350-900"));
        assert!(aircraft_types_match("A35K", "A350-1000"));
    }
