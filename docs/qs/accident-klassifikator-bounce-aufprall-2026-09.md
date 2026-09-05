# Prüfstand — Unfall-Klassifikator sah den Aufprall nach einem Bounce nie

**Gefunden:** Thomas' Bugreport (05.09.2026), gestützt auf Live-Datenbank-
Auswertung (`ssh gsg`, DB `phpvmsva`).

## Der Befund

PIREP `JGXrGwlwa4aOGZpn` (31.08.2026, OY-SYA B763, AeroACARS 1.7.12) landete
mit −3007 fpm bei 5,88 G Spitze — weit über der Confirmed-Accident-Schwelle
(|V/S| ≥ 1500 fpm UND G ≥ 3,0, unabhängig vom Bahn-Match) und mit Bahn-Match
(LIMJ/28). Trotzdem blieb `accident` unbesetzt. Das PIREP-Feld `touchdowns`
zeigte „1 (1 T&G)" — nur ein Touch-and-Go war erfasst, der eigentliche,
katastrophale Aufsetzer fehlte im Touchdown-Verlauf komplett, obwohl seine
Landedaten (`landing-rate`, `landing-g-force`) korrekt im PIREP standen.

Zum Vergleich: PIREP `QG60nvzMvNlAGg0V` (04.09.2026, ein einzelner
Touchdown, kein Go-Around) wurde korrekt als Unfall geflaggt — die
Klassifikator-Logik selbst (`classify_accident_heuristic`, `accident.rs`)
und die Multi-Touchdown-Aggregation in `apply_accident_heuristic` (Spec
§Leitentscheidung 7: "Max-Severity über alle Touchdowns") sind beide
nachweislich korrekt gebaut — das Problem lag woanders.

## Root Cause

`apply_accident_heuristic` hat **genau eine** produktive Aufrufstelle
(`client/src-tauri/src/lib.rs`, Zeile ~26733 vor dieser Änderung) — im
Touchdown-Sampler (`spawn_touchdown_sampler`), am Ende eines ~10-Sekunden-
Dump-Fensters pro Touchdown (`TOUCHDOWN_POST_WINDOW_MS`).

Damit der Sampler einen ZWEITEN Touchdown überhaupt als neuen Edge erkennt,
muss `stats.sampler_touchdown_at` zuvor auf `None` zurückgesetzt worden
sein — das passiert nur in einem eigenen Reset-Block, der wiederum verlangt,
dass (a) der Dump des VORHERIGEN Touchdowns bereits abgeschlossen ist
(`touchdown_window_dumped_at.is_some()`) UND (b) das Flugzeug über die
T&G-AGL-Schwelle (100 ft) geklettert ist.

Bedingung (a) ist der Fehler: Bei einem **Bounce, der innerhalb der ersten
10 Sekunden nach dem ersten Touchdown wieder aufsetzt** — real gemessen:
Touch-and-Go bei −1499 fpm/4,67 G, wenige Sekunden später der eigentliche
Aufprall bei −3007 fpm/5,88 G — war der erste Dump zum Zeitpunkt des
zweiten Aufsetzens noch gar nicht fertig. Der Reset-Block feuerte nie, der
Sampler blieb permanent auf dem ersten (harmlosen) Touchdown "gelatcht",
und der zweite (katastrophale) Touchdown fiel beim
`stats.sampler_touchdown_at.is_none()`-Guard des Edge-Detectors lautlos
durch. `apply_accident_heuristic` sah ihn nie — der Klassifikator selbst
lief nur einmal, auf den harmlosen T&G, und fand dort zu Recht nichts
(−1499 fpm liegt knapp UNTER der 1500-fpm-Schwelle).

Die Basis-Felder (`landing_peak_g_force`, `landing_rate_fpm`) wurden trotzdem
korrekt mit den Aufprallwerten befüllt, weil deren Stempel-Pfad
(`step_flight_at`/`stamp_touchdown_metadata`) unabhängig vom Sampler läuft —
das erklärt, warum die rohen Landedaten im PIREP stimmten, aber die
Unfall-Klassifikation nicht griff.

## Gefixt

Neue reine Entscheidungsfunktion `touchdown_dump_faellig(elapsed_ms,
agl_ft)`: der Dump gilt jetzt fällig, wenn ENTWEDER die vollen 10 Sekunden
verstrichen sind (Normalfall, unverändert) ODER das Flugzeug — noch
innerhalb des laufenden Fensters — wieder über die T&G-AGL-Schwelle
geklettert ist. Ein Frühdump nutzt den bis dahin gesammelten Puffer (5 s
Pre-TD + die paar Sekunden Post-TD, die schon da sind) statt der vollen 10 s
— für die unfallrelevanten Werte (V/S und G am/kurz nach dem Aufsetzen)
reicht das, die kritischen Samples liegen ohnehin am Anfang des Fensters.

Der bestehende Klettern-nach-Dump-Reset-Block (unverändert) sieht danach
noch im selben Tick `touchdown_window_dumped_at.is_some() && agl > 100 ft`
und re-armiert den Sampler sofort — der zweite Touchdown bekommt seinen
eigenen Dump samt eigenem `apply_accident_heuristic`-Aufruf.

**Warum nicht stattdessen `sampler_touchdown_at` direkt beim Klettern
zurücksetzen (ohne Dump-Zwang)?** Das würde den laufenden Dump für den
ERSTEN Touchdown abbrechen (der Dump-Trigger hängt an
`stats.sampler_touchdown_at.is_some()`) — der harmlose T&G bekäme dann gar
keine Analyse mehr statt einer verkürzten. Der gewählte Weg (früh dumpen,
dann erst zurücksetzen) verliert keine Klassifikation, nur etwas
Puffertiefe.

## Tests

`touchdown_dump_faellig_tests` (4 Tests) — die reine Bedingung isoliert:
kein Dump vor Fenster+Klettern, Dump nach vollem Fenster auch ohne
Klettern, Dump sofort bei frühem Klettern, Gegenprobe für die
`>`-vs-`>=`-Grenze bei exakt 100 ft. **Gegenprobe durchgeführt:** mit der
alten Bedingung (`elapsed_ms >= TOUCHDOWN_POST_WINDOW_MS` allein) schlägt
`fruehes_klettern_ueber_die_tg_schwelle_macht_den_dump_sofort_faellig`
zuverlässig fehl — bestätigt, dass der Test die Regression wirklich fängt.

`gaf707_bounce_in_aufprall_tests` (1 Test) — Reproduktion des realen Falls
mit den echten Zahlen (T&G −1499 fpm/4,67 G, dann Aufprall −3007 fpm/5,88 G,
Bahn-Match LIMJ/28): ruft `apply_accident_heuristic` zweimal auf demselben
`FlightStats` auf (wie es nach dem Fix tatsächlich beide Male passiert) und
prüft, dass der T&G allein keinen Unfall auslöst, der Aufprall danach aber
als `Confirmed(Impact)` erkannt wird. Dieser Test allein beweist NICHT die
Sampler-Verdrahtung (der Sampler-Loop bleibt Tauri-gebunden, nicht isoliert
testbar) — dafür steht `touchdown_dump_faellig_tests`. Zusammen decken
beide die volle Kette: Verdrahtung UND Klassifikations-Ergebnis.

## Nicht behoben (bewusst außerhalb des Umfangs)

* Die FSM-seitige `touchdown_events`-Liste (für `touchdown_count`/PIREP-
  Notizen-Anzeige, getrennt vom Sampler) hat ein STRUKTURELL ähnliches
  Risiko: der `FinalLanding`-Push wartet `TOUCH_AND_GO_WATCH_SECS` (30 s)
  lang ab, bevor er einen Touchdown endgültig NICHT als T&G verbucht. Endet
  der Flug (Absturz-Session-Ende, manuelles Beenden) innerhalb dieser 30
  Sekunden, bleibt der Eintrag im Touchdown-Verlauf ebenfalls aus — das ist
  aber ein reines Anzeige-/Zähl-Problem (`touchdown_count` im PIREP), nicht
  die Unfall-Erkennung selbst (die hängt nur am Sampler, s. o., und ist mit
  diesem Fix repariert). Im konkreten Bugreport-PIREP war das vermutlich
  MIT betroffen (`touchdowns` zeigte „1 (1 T&G)" statt „2"), aber außerhalb
  des mit Thomas abgestimmten Auftrags (der explizit auf
  `classify_accident_heuristic`/die Unfall-Erkennung zielte). Als möglicher
  Folgefund vorgemerkt, falls die Touchdown-ZÄHLUNG im PIREP für Piloten
  weiterhin falsch aussieht, obwohl die Unfall-Erkennung jetzt korrekt
  greift.

## Nachtrag (05.09.2026): Codex-Folgefund — stehengebliebenes G vom ersten Touchdown

Adversarial-Review gegen den obigen Fix (Frueh-Dump beim Klettern) fand:
`landing_peak_g_force` — genau das Feld, das `apply_accident_heuristic` als
`peak_g_load` liest — ist ein RUNNING-MAX ueber die gesamte Session
(`if g > cur { s.landing_peak_g_force = Some(g) }` im Sampler-Dump-Handler),
kein bedingungsloses Ueberschreiben wie Lat/Lon. Der Sampler-Klettern-Reset
setzte es NICHT zurueck. Ergebnis: ein harter, aber V/S-technisch harmloser
erster Touchdown (hohes G, V/S unter der Schwelle) haette zusammen mit
einem zweiten, eigentlich unauffaelligen Touchdown (normales G, aber V/S
ueber der Schwelle) faelschlich Confirmed(Impact) ausgeloest — zwei
physisch getrennte Ereignisse zu einem Falsch-Alarm vermischt. Die
"haerterer Wert gewinnt"-Doktrin (v1.6.3) gilt INNERHALB eines Touchdowns
fuer mehrere Messquellen desselben Ereignisses, nicht ueber zwei
verschiedene Touchdowns hinweg.

**Gefixt:** `s.landing_peak_g_force = None;` ergaenzt im Sampler-Klettern-
Reset-Block, direkt neben den bereits dort zurueckgesetzten `landing_lat`/
`landing_lon`.

**Tests:** ein Verhaltens-Test zeigt, dass `apply_accident_heuristic` bei
korrekt zurueckgesetztem G richtig klassifiziert — er ersetzt
`landing_peak_g_force` aber manuell und uebt damit NICHT den echten
Reset-Pfad im Sampler aus (Tauri-gebunden, nicht isoliert testbar, wie der
Rest der Sampler-Infrastruktur). Ein Quelltext-Wächter
(`multi_td_klettern_reset_setzt_auch_landing_peak_g_force_zurueck`) deckt
genau diese Luecke: er verlangt die Reset-Zeile im Klettern-Block selbst.
Gegenprobe durchgefuehrt: mit der Zeile entfernt schlaegt der Wächter
zuverlaessig fehl.

## Release

Dieser Fix braucht einen regulären Client-Release
([[aeroacars-release]]-Skill) — **nicht ohne Thomas' ausdrückliche
Freigabe**, siehe [[feedback-aeroacars-release-signoff]]. Bis dahin bleibt
er unveröffentlicht auf `main`.
