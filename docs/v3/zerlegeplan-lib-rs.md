# AeroACARS 3.0 — Zerlegeplan für `client/src-tauri/src/lib.rs` (Etappe 0)

Stand 29.10.2026, Zweig `v3` (ab v2.1.24 / 43b0d4e1). Vorgabe: Pflichtenheft P-2.01 und Kap. 15
(GSG-Repo `docs/AeroACARS-3.0-Pflichtenheft.md`): `lib.rs` nach Themen zerlegen, **ohne
Verhaltensänderung**.

## Regeln für jeden Schritt

1. Nur verschieben: Sichtbarkeit (`pub(crate)`) und Pfade dürfen sich ändern, Namen, Logik,
   Kommentare und Einrückung nicht.
2. Nachweis je Schritt: `cargo test --workspace -- --list` vorher = nachher (Diff leer),
   `cargo test --workspace` grün auf dem Mac, nach dem Push Windows-CI grün mit gleicher Testzahl.
3. Ein Schritt = ein Commit.
4. Was `cargo check` auf dem Mac als tot meldet, ist oft Windows-Code: nicht löschen.
5. Vor jedem schweren Build prüfen, ob Thomas fliegt (live, `flight_sessions`, Pilot 1).
6. Wächtertests, die `lib.rs` lesen (`include_str!("../lib.rs")`, `tests/musterquelle.rs`,
   `tests/spur_verdrahtung.rs`), müssen nach jeder Verschiebung weiter dasselbe prüfen; zieht
   Code aus `lib.rs` weg, wird der Wächter auf die neue Datei erweitert, nie abgeschwächt.

## Ausgangslage (gemessen)

| Größe | Wert |
|---|---|
| `lib.rs` vor Etappe 0 | 91 037 Zeilen |
| davon eingebettete Testmodule | 107 Module, 34 876 Zeilen |
| Programmmodule eingebettet | `pirep_queue` (Z. 22 770), `nachtrag_queue` (Z. 23 287) |
| Tauri-Befehle in `lib.rs` | 67 von 183 (Rest schon in eigenen Dateien) |
| Wissensgraph (graphify, Code-Teil, v3) | 897 Knoten aus `lib.rs`, 46 Gemeinschaften; die sechs größten (182, 137, 124, 118, 79, 75 Knoten) reichen je über fast die ganze Datei |
| Tests Mac / Windows-CI | 3 500 / 3 727 bestanden, je 15 ignoriert |

## Schritte

| Schritt | Inhalt | Risiko |
|---|---|---|
| **1** ✅ | 107 Testmodule nach `src/tests_lib/<name>.rs` (`#[cfg(test)] #[path] mod x;`), Inhalt byte-gleich, relative `include_*!`-Pfade mit `../` | null |
| 2a | `pirep_queue`, `nachtrag_queue` nach `src/<name>.rs` (gleicher Mechanismus) | null |
| 2b | 67 Befehle nach `src/befehle/<bereich>.rs` (flight 17, phpvms 6, telemetrie 5, xplane 4, …); Helfer bleiben vorerst in `lib.rs` und werden `pub(crate)`; `generate_handler!` bekommt die neuen Pfade | gering |
| 2c | in sich geschlossene Graph-Gemeinschaften mit zusammenhängendem Bereich: ACARS-Postausgang (C57, Z. 36 790–37 156), `LandingScore` (C141), MSFS-Flare (C10), Segment/Null-Island (C131), Flicker (C135), Bahndisziplin-Takt (C95), Bahn-Upgrade (C77), Touchdown-Korrelation (C26), Sprit-Plan (C13), `AircraftLimits` (C87) | gering |
| 2d | die großen, verflochtenen Gemeinschaften: Zustand (`AppState`, `FlightStats`, `ActiveFlight` → `zustand.rs`), Wiederaufnahme/Lage nach Lücke, Bodenphasen, Abgabe-Sperre/`validate_for_filing`, Positions-Takt. Erst den Graphen nach 2a–2c neu bauen und je Gemeinschaft Aufrufer außerhalb zählen | mittel |
| 3 | Eingangsbau `scoring_eingang`/`fill_v2_rollout_fields` als eigenes Modul (Vorbereitung Eingabe-Dokument, Pflichtenheft Kap. 4) | mittel |

Der Graph wird mit `graphify update client/src-tauri/src --no-cluster` und
`graphify cluster-only client/src-tauri/src --no-viz --no-label` gebaut (kein Sprachmodell nötig);
`graphify-out/` ist per `.gitignore` ausgeschlossen.
