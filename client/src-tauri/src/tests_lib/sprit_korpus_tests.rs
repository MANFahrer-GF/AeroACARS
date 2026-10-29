    use super::*;
    use std::collections::HashMap;

    fn zahl(s: &str) -> Option<f64> {
        let t = s
            .trim()
            .trim_end_matches(" kg")
            .trim_start_matches("FL")
            .replace('+', "");
        t.split_whitespace().next()?.parse().ok()
    }

    fn gc_nm(a: f64, b: f64, c: f64, d: f64) -> f64 {
        ::geo::distance_m(a, b, c, d) / 1852.0
    }

    #[test]
    #[ignore = "braucht den Sprit-Korpus (Flugspuren, nicht im Repo)"]
    fn sprit_korpus_gegen_echte_fluege() {
        let Ok(dir) = std::env::var("SPRIT_KORPUS") else {
            panic!("SPRIT_KORPUS nicht gesetzt — ohne Daten ist dieser Lauf wertlos");
        };
        let aus =
            std::env::var("SPRIT_KORPUS_AUS").unwrap_or_else(|_| "/tmp/sprit-korpus.tsv".into());
        let lies = |n: &str| {
            std::fs::read_to_string(format!("{dir}/{n}")).unwrap_or_else(|e| panic!("{n}: {e}"))
        };

        // meta: pid dep arr icao dlat dlon elev reserve ldg_field ...
        let mut meta: HashMap<String, Vec<String>> = HashMap::new();
        for z in lies("meta.tsv").lines() {
            let f: Vec<String> = z.split('\t').map(String::from).collect();
            if f.len() > 8 {
                meta.insert(f[0].clone(), f);
            }
        }
        // routen: pid \t json
        let mut routen: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
        for z in lies("routen.tsv").lines() {
            if let Some((pid, js)) = z.split_once('\t') {
                if let Ok(serde_json::Value::Array(a)) = serde_json::from_str(js) {
                    routen.insert(pid.to_string(), a);
                }
            }
        }
        // spuren: pid ts lat lon alt fuel gs agl
        let mut spuren: HashMap<String, Vec<[f64; 7]>> = HashMap::new();
        for z in lies("tracks.tsv").lines() {
            let f: Vec<&str> = z.split('\t').collect();
            if f.len() < 8 {
                continue;
            }
            let w: Vec<f64> = f[1..8].iter().filter_map(|x| x.parse().ok()).collect();
            if w.len() == 7 {
                spuren
                    .entry(f[0].to_string())
                    .or_default()
                    .push([w[0], w[1], w[2], w[3], w[4], w[5], w[6]]);
            }
        }

        let mut zeilen = vec![
            "flug\tmuster\tstrecke_rt\tstatus\tbis_sinkflug_pct\tanflug_pct\tstrecke_anflug_nm\t\
             plan_strecke_anflug_nm\tplan_bis_kg\tplan_anflug_kg\tplan_trip_kg\tofp_trip_kg"
                .to_string(),
        ];
        for z in lies("navlogs_all.tsv").lines() {
            let f: Vec<&str> = z.split('\t').collect();
            if f.len() < 5 {
                continue;
            }
            let pid = f[0];
            let (Some(m), Some(spur)) = (meta.get(pid), spuren.get_mut(pid)) else {
                continue;
            };
            let idents: Vec<&str> = f[1].split_whitespace().collect();
            let dists: Vec<f32> = f[2]
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            let fuels: Vec<f32> = f[3]
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            let alts: Vec<f32> = f[4]
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            let kopf = format!("{pid}\t{}\t{}->{}", m[3], m[1], m[2]);
            if idents.len() != dists.len()
                || idents.len() != fuels.len()
                || !idents.contains(&"TOD")
            {
                zeilen.push(format!("{kopf}\tkein_navlog\t\t\t\t\t\t\t"));
                continue;
            }
            let (Some(dlat), Some(dlon)) = (zahl(&m[4]), zahl(&m[5])) else {
                continue;
            };

            // Koordinaten je Fix: benannte aus der Route, TOC/TOD nach
            // Streckenanteil zwischen ihre Nachbarn — so rechnet SimBrief sie.
            let route = routen.get(pid).cloned().unwrap_or_default();
            let mut koord: Vec<Option<(f64, f64)>> = vec![None; idents.len()];
            let mut j = 0;
            for (i, name) in idents.iter().enumerate() {
                for k in j..route.len() {
                    if route[k]["name"].as_str() == Some(name) {
                        if let (Some(la), Some(lo)) =
                            (route[k]["lat"].as_f64(), route[k]["lon"].as_f64())
                        {
                            koord[i] = Some((la, lo));
                            j = k + 1;
                        }
                        break;
                    }
                }
            }
            let n = idents.len();
            if koord[n - 1].is_none() {
                koord[n - 1] = Some((dlat, dlon));
            }
            let mut kum = vec![0.0f64; n];
            for i in 1..n {
                kum[i] = kum[i - 1] + dists[i] as f64;
            }
            let bekannt: Vec<usize> = (0..n).filter(|i| koord[*i].is_some()).collect();
            for i in 0..n {
                if koord[i].is_some() {
                    continue;
                }
                let vor = bekannt.iter().rev().find(|b| **b < i).copied();
                let nach = bekannt.iter().find(|b| **b > i).copied();
                if let (Some(v), Some(h)) = (vor, nach) {
                    let t = (kum[i] - kum[v]) / (kum[h] - kum[v]).max(1e-9);
                    let (a, b) = (koord[v].unwrap(), koord[h].unwrap());
                    koord[i] = Some((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
                }
            }
            if koord.iter().any(|c| c.is_none()) {
                zeilen.push(format!("{kopf}\tkeine_route\t\t\t\t\t\t\t"));
                continue;
            }
            let fixes: Vec<api_client::RouteFix> = (0..n)
                .map(|i| api_client::RouteFix {
                    ident: idents[i].into(),
                    lat: koord[i].unwrap().0,
                    lon: koord[i].unwrap().1,
                    kind: "wpt".into(),
                    sprit_bis_hier_kg: Some(fuels[i]),
                    segment_nm: Some(dists[i]),
                    hoehe_ft: alts.get(i).copied(),
                    zeit_bis_hier_s: None,
                    ..Default::default()
                })
                .collect();

            spur.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
            let luft: Vec<[f64; 7]> = spur.iter().copied().filter(|t| t[6] > 200.0).collect();
            if luft.len() < 20 {
                zeilen.push(format!("{kopf}\tzu_wenig_track\t\t\t\t\t\t\t"));
                continue;
            }
            // Einheit am Landesprit-Feld des Clients kalibrieren (kg/lbs).
            let mut faktor = 1.0f64;
            if let Some(ldg) = zahl(&m[8]) {
                let r = luft[luft.len() - 1][4] / ldg;
                if (1.9..2.5).contains(&r) {
                    faktor = 1.0 / 2.204_62;
                }
            }

            let mut st = FlightStats::default();
            st.planned_waypoints = fixes;
            st.planned_arr_ref_pos = Some((dlat, dlon));
            st.arr_airport_elevation_ft = zahl(&m[6]).map(|v| v as f32);
            st.planned_cruise_alt_ft = alts.iter().copied().fold(None, |a: Option<f64>, v| {
                Some(a.map_or(v as f64, |x| x.max(v as f64)))
            });
            // Der Trip aus dem OFP-KOPF, wie ihn der echte Flug bekommt — nicht
            // aus dem Navlog. Sonst waeren beide Quellen per Konstruktion
            // gleich, und der Vergleich in `pruef-korpus.mjs` koennte nie rot
            // werden (QS-Befund E6, 18.09.2026).
            let ofp_trip = zahl(&m[11]).map(|v| v as f32);
            st.planned_burn_kg = ofp_trip;
            st.planned_reserve_kg = zahl(&m[7]).map(|v| v as f32);
            st.takeoff_fuel_kg = Some((luft[0][4] * faktor) as f32);
            let mut letzte: Option<[f64; 7]> = None;
            for t in &luft {
                let dt = letzte.map(|l| t[0] - l[0]);
                if let Some(l) = letzte {
                    st.distance_nm += gc_nm(l[1], l[2], t[1], t[2]);
                }
                // DER ausgelieferte Code.
                sprit_tick(
                    &mut st,
                    t[1],
                    t[2],
                    t[3],
                    (t[4] * faktor) as f32,
                    false,
                    false,
                    false,
                    false,
                    dt,
                );
                letzte = Some(*t);
            }
            st.landing_fuel_kg = Some((luft[luft.len() - 1][4] * faktor) as f32);
            if st.sprit_vergleichspunkt_kg.is_none() {
                zeilen.push(format!("{kopf}\tkein_vergleichspunkt\t\t\t\t\t\t\t\t"));
                continue;
            }
            let a = sprit_auswertung_aus(&st, false);
            let pct = |p: &Option<landing_scoring::sprit::Phase>| {
                p.as_ref()
                    .map(|x| x.abweichung_pct.to_string())
                    .unwrap_or_default()
            };
            let opt = |v: Option<f32>| v.map(|x| x.to_string()).unwrap_or_default();
            zeilen.push(format!(
                "{kopf}\tok\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                pct(&a.bis_sinkflug),
                pct(&a.anflug),
                opt(a.strecke_anflug_nm),
                opt(a.plan_strecke_anflug_nm),
                opt(a.bis_sinkflug.as_ref().map(|p| p.plan_kg)),
                opt(a.anflug.as_ref().map(|p| p.plan_kg)),
                opt(fuels.last().copied()),
                opt(ofp_trip),
            ));
        }
        std::fs::write(&aus, zeilen.join("\n") + "\n").expect("Ergebnis schreiben");
        let ok = zeilen.iter().filter(|z| z.contains("\tok\t")).count();
        eprintln!(
            "SPRIT-KORPUS: {} Fluege, {ok} mit Messpunkt -> {aus}",
            zeilen.len() - 1
        );
        assert!(ok > 0, "kein einziger Flug mit Messpunkt");
    }
