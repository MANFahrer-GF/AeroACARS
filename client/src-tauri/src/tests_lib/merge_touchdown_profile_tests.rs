    use super::*;
    use chrono::TimeZone;
    use std::collections::VecDeque;

    fn td() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 31, 10, 40, 1).unwrap()
    }

    fn pp(t_ms: i32, vs_fpm: f32) -> TouchdownProfilePoint {
        TouchdownProfilePoint {
            t_ms,
            vs_fpm,
            g_force: 1.0,
            agl_ft: 0.0,
            on_ground: t_ms >= 0,
            heading_true_deg: 0.0,
            groundspeed_kt: 120.0,
            indicated_airspeed_kt: 120.0,
            pitch_deg: 0.0,
            bank_deg: 0.0,
        }
    }

    fn tel(t_ms: i64, vs_fpm: f32) -> TelemetrySample {
        TelemetrySample {
            fuel_kg: 0.0,
            at: td() + chrono::Duration::milliseconds(t_ms),
            vs_fpm,
            g_force: 1.0,
            g_semibody: None,
            on_ground: t_ms >= 0,
            agl_ft: 0.0,
            msl_ft: 0.0 + 500.0,
            heading_true_deg: 0.0,
            groundspeed_kt: 120.0,
            indicated_airspeed_kt: 120.0,
            true_airspeed_kt: 120.0,
            lat: 0.0,
            lon: 0.0,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            gear_normal_force_n: None,
            total_weight_kg: None,
        }
    }

    /// Der Kern-Fix: snapshot endet bei +500 ms, post_buffer haelt -5000..+10000.
    /// Ergebnis muss bis ~10 s reichen — und im Overlap (≤ +500) KEINE Dubletten.
    #[test]
    fn extends_to_ten_seconds_without_overlap_duplicates() {
        // Snapshot: -4000..+500 in 500-ms-Schritten (Streamer-Tick-Slice).
        let snapshot: Vec<TouchdownProfilePoint> = (-8..=1).map(|i| pp(i * 500, -200.0)).collect();
        assert_eq!(snapshot.last().unwrap().t_ms, 500);

        // Post-Buffer: -5000..+10000 in 200-ms-Schritten (50-Hz-Sampler, hier
        // grob), enthaelt also den Overlap-Bereich UND die vollen 10 s.
        let mut post: VecDeque<TelemetrySample> = VecDeque::new();
        let mut t = -5000_i64;
        while t <= 10_000 {
            post.push_back(tel(t, -150.0));
            t += 200;
        }

        let merged = merge_touchdown_profile(&snapshot, &post, td(), crate::grenzwerte::grenzwerte().landung.post_td_profile_ms.wert);

        // Reicht jetzt bis ~10 s statt nur +500 ms.
        let max_t = merged.iter().map(|p| p.t_ms).max().unwrap();
        assert!(
            max_t >= 9_800,
            "profile should extend to ~10 s, got {max_t} ms"
        );

        // Nichts ueber dem 10-s-Fenster.
        assert!(merged.iter().all(|p| p.t_ms <= crate::grenzwerte::grenzwerte().landung.post_td_profile_ms.wert));

        // Keine doppelten Zeitstempel (Overlap sauber dedupliziert).
        let mut ts: Vec<i32> = merged.iter().map(|p| p.t_ms).collect();
        let len_before = ts.len();
        ts.sort_unstable();
        ts.dedup();
        assert_eq!(
            ts.len(),
            len_before,
            "no duplicate t_ms allowed in the overlap"
        );

        // Monoton steigend (Pre-TD-snapshot, dann angehaengte Post-TD-Samples).
        assert!(merged.windows(2).all(|w| w[0].t_ms < w[1].t_ms));
    }

    /// Ohne post_buffer bleibt es beim snapshot — kein Absturz, keine Aenderung.
    #[test]
    fn empty_post_buffer_keeps_snapshot_only() {
        let snapshot: Vec<TouchdownProfilePoint> = (-8..=0).map(|i| pp(i * 500, -200.0)).collect();
        let merged = merge_touchdown_profile(&snapshot, &VecDeque::new(), td(), crate::grenzwerte::grenzwerte().landung.post_td_profile_ms.wert);
        assert_eq!(merged.len(), snapshot.len());
        assert_eq!(merged.last().unwrap().t_ms, 0);
    }
