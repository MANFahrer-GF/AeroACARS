// Beispieldaten für Vorschau und Entwurf: ein Flug AIB441 NZCH–YBBN in vier Phasen.
const jetzt = Date.parse("2026-10-23T19:16:00Z");
const metar = (icao, dir, kt, q, extra = {}) => ({ icao, wind_direction_deg: dir, wind_speed_kt: kt, qnh_hpa: q, visibility_m: 9999, cloud_layers: [{ cover: "SCT", base_ft: 3500 }], temperature_c: 18, dewpoint_c: 11, ...extra });
const basis = {
  callsign: "AIB 441", dpt_airport: "NZCH", arr_airport: "YBBN", dep_gate: "28", arr_gate: "75", connection_state: "live",
  planned_block_fuel_kg: 61200, planned_reserve_kg: 4600, planned_burn_kg: 14100, sim_zfw_kg: 317800, sim_tow_kg: 366500, planned_max_zfw_kg: 330000, planned_max_tow_kg: 380000,
  planned_alternate: "YSSY", predicted_runway: "19R",
  flugzeiten: { plan_off_block: jetzt / 1000 - 3600, plan_on_block: jetzt / 1000 + 9000 },
  dep_metar_decoded: metar("NZCH", 360, 3, 1018), arr_metar_decoded: metar("YBBN", 160, 12, 1013, { gust_kt: 22 }),
};
const kabine = (extra = {}) => ({ an: true, stumm: false, lautstaerke_live: 0.8, lautstaerke_gespeichert: 0.8, musik_live: 0.2, musik_gespeichert: 0.2, boarding_musik: true, flug: true, grund: null, belegt: false, hinweis: null, ...extra });
export const PHASEN = {
  boarding: { erreichbar: true, jetzt, status: { ...basis, phase: "boarding", live: { gs_kt: 0, altitude_msl_ft: 120, fuel_total_kg: 61200 }, sim_fuel_kg: 61200 },
    kabine: kabine({ naechste: { slot: "begruessung_tueren_zu", wann: null } }) },
  reiseflug: { erreichbar: true, jetzt, status: { ...basis, phase: "cruise", block_off_at: "2026-10-23T18:32:00Z", takeoff_at: "2026-10-23T18:42:00Z", sim_fuel_kg: 47500, distance_nm: 232,
    live: { gs_kt: 501, ias_kt: 268, vertical_speed_fpm: 0, altitude_msl_ft: 38120, altitude_pressure_ft: 38000, wind_dir_deg: 270, wind_speed_kt: 62, ete_min: 158, fuel_total_kg: 47500 } },
    kabine: kabine({ boarding_musik: false, naechste: { slot: "reiseflug50", wann: jetzt / 1000 + 1500 }, folgende: [{ slot: "reiseflug50", wann: jetzt / 1000 + 1500 }, { slot: "reiseflug80", wann: jetzt / 1000 + 4800 }, { slot: "ende_service", wann: jetzt / 1000 + 6600 }, { slot: "kapitaen_sinkflug", wann: jetzt / 1000 + 8400 }] }) },
  anflug: { erreichbar: true, jetzt, status: { ...basis, phase: "approach", block_off_at: "2026-10-23T18:32:00Z", takeoff_at: "2026-10-23T18:42:00Z", sim_fuel_kg: 9800, approach_runway: "19R", distance_nm: 1610,
    live: { gs_kt: 152, ias_kt: 141, vertical_speed_fpm: -760, altitude_msl_ft: 2480, altitude_pressure_ft: 2500, wind_dir_deg: 160, wind_speed_kt: 14, headwind_kt: 12, crosswind_kt: 7, ete_min: 6, fuel_total_kg: 9800 } },
    kabine: kabine({ boarding_musik: false, naechste: { slot: "kommando_plaetze_landung", wann: null } }) },
  gelandet: { erreichbar: true, jetzt, status: { ...basis, phase: "taxi_in", block_off_at: "2026-10-23T18:32:00Z", takeoff_at: "2026-10-23T18:42:00Z", landing_at: "2026-10-23T22:01:00Z", sim_fuel_kg: 8700, approach_runway: "19R", distance_nm: 1688,
    landing_rate_fpm: -142, landing_g_force: 1.18, live: { gs_kt: 18, altitude_msl_ft: 13, fuel_total_kg: 8700 } },
    debrief: { score_numeric: 92, grade_letter: "A", score_label: "smooth" },
    kabine: kabine({ boarding_musik: true, naechste: { slot: "aussteigen", wann: null } }) },
};
