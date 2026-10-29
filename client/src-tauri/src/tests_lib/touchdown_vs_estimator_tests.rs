    use super::*;
    use chrono::TimeZone;
    use std::collections::VecDeque;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap()
    }

    fn make_sample(at: DateTime<Utc>, agl_ft: f32, vs_fpm: f32) -> TelemetrySample {
        TelemetrySample {
            // Diese Vorrichtung prueft Hoehe und Sinkrate; der Sprit spielt
            // hier keine Rolle.
            fuel_kg: 0.0,
            at,
            vs_fpm,
            g_force: 1.0,
            g_semibody: None,
            on_ground: false,
            agl_ft,
            msl_ft: agl_ft + 500.0,
            heading_true_deg: 0.0,
            groundspeed_kt: 130.0,
            indicated_airspeed_kt: 130.0,
            true_airspeed_kt: 130.0,
            lat: 0.0,
            lon: 0.0,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            // v0.7.0 — Forensik-v2 fields, default None fuer alte Tests
            gear_normal_force_n: None,
            total_weight_kg: None,
        }
    }

    /// Test 1: Rebound-VSI is positive but AGL is geometrically
    /// dropping → result must be NEGATIVE.
    /// Reproduces the original pilot bug (V/S +57 fpm at G 1.52).
    #[test]
    fn agl_estimator_overrides_positive_rebound_vsi() {
        let td = t0();
        let mut buffer = VecDeque::new();
        // Last 1 second of approach: AGL drops 25 → 0 ft.
        // VS readings are positive (rebound) but geometry says
        // descent at -25 ft/sec = -1500 fpm avg.
        for ms in 0..=1000 {
            if ms % 100 != 0 {
                continue;
            }
            let agl = 25.0 - (ms as f32 / 1000.0) * 25.0;
            let vs = 50.0; // positive rebound — VSI lies
            buffer.push_back(make_sample(
                td - chrono::Duration::milliseconds(1000 - ms),
                agl.max(0.0),
                vs,
            ));
        }
        let est = estimate_xplane_touchdown_vs_from_agl(&buffer, td)
            .expect("should estimate from AGL despite positive VSI");
        assert!(
            est.fpm < 0.0,
            "expected negative descent rate, got {} fpm",
            est.fpm
        );
        assert!(
            est.fpm < -500.0,
            "expected steep descent (~-1500 fpm), got {} fpm",
            est.fpm
        );
    }

    /// Test 2: Pre-flare steep descent (-1300 fpm @ 900 ft AGL)
    /// followed by gentle touchdown (-200 fpm @ 0 ft).
    /// Estimator must IGNORE the high-altitude steep descent.
    #[test]
    fn agl_estimator_ignores_pre_flare_high_altitude_descent() {
        let td = t0();
        let mut buffer = VecDeque::new();
        // 9 seconds ago: AGL 900 ft, VS -1300 fpm (steep descent)
        buffer.push_back(make_sample(
            td - chrono::Duration::seconds(9),
            900.0,
            -1300.0,
        ));
        // Last 1 second: smooth flare from 30 → 0 ft AGL = -1800 fpm
        // Wait, let me make this gentler: 3 → 0 ft over 1s = -180 fpm
        for ms in 0..=1000 {
            if ms % 100 != 0 {
                continue;
            }
            let agl = 3.0 - (ms as f32 / 1000.0) * 3.0;
            buffer.push_back(make_sample(
                td - chrono::Duration::milliseconds(1000 - ms),
                agl.max(0.0),
                -200.0,
            ));
        }
        let est = estimate_xplane_touchdown_vs_from_agl(&buffer, td)
            .expect("should pick the close-to-ground tier");
        // Should be gentle (~-180 fpm), NOT the -1300 from 900 ft.
        assert!(
            est.fpm > -500.0,
            "expected gentle descent (NOT pre-flare -1300 contamination), got {} fpm",
            est.fpm
        );
    }

    /// Test 3: Butter landing — slow flare, gentle touchdown.
    /// Estimator should give ~-100 fpm, not the original approach descent.
    #[test]
    fn agl_estimator_butter_landing() {
        let td = t0();
        let mut buffer = VecDeque::new();
        // Last 2 seconds: AGL 4 → 1 ft (= -90 fpm avg, butter)
        for ms in 0..=2000 {
            if ms % 200 != 0 {
                continue;
            }
            let agl = 4.0 - (ms as f32 / 2000.0) * 3.0;
            buffer.push_back(make_sample(
                td - chrono::Duration::milliseconds(2000 - ms),
                agl,
                -100.0,
            ));
        }
        let est = estimate_xplane_touchdown_vs_from_agl(&buffer, td)
            .expect("should give a butter result");
        assert!(est.fpm < 0.0, "expected negative descent, got {}", est.fpm);
        assert!(
            est.fpm > -300.0,
            "expected gentle butter (~-100 to -200 fpm), got {} fpm",
            est.fpm
        );
    }

    /// Test 4: All VS readings are positive (extreme rebound) but
    /// AGL drops geometrically. AGL wins, result is negative.
    #[test]
    fn agl_estimator_wins_when_all_vs_positive() {
        let td = t0();
        let mut buffer = VecDeque::new();
        // Last 1 second: AGL 10 → 0 ft, VS readings all positive (impossible
        // physically but happens in sim post-touchdown rebound)
        for ms in 0..=1000 {
            if ms % 100 != 0 {
                continue;
            }
            let agl = 10.0 - (ms as f32 / 1000.0) * 10.0;
            buffer.push_back(make_sample(
                td - chrono::Duration::milliseconds(1000 - ms),
                agl.max(0.0),
                100.0, // all positive!
            ));
        }
        let est = estimate_xplane_touchdown_vs_from_agl(&buffer, td)
            .expect("AGL should give answer despite all-positive VS");
        assert!(
            est.fpm < 0.0,
            "expected negative descent from AGL geometry, got {} fpm (VS readings would have given positive)",
            est.fpm
        );
    }

    /// negative_only filter test: positives are dropped.
    #[test]
    fn negative_only_drops_positive_values() {
        assert_eq!(negative_only(Some(57.0)), None);
        assert_eq!(negative_only(Some(0.0)), None);
        assert_eq!(negative_only(Some(-200.0)), Some(-200.0));
        assert_eq!(negative_only(None), None);
        assert_eq!(negative_only(Some(f32::NAN)), None);
    }
