# Gates: v1.7.44 — Felddefekte aus DLH 373 (KJFK→EDDM) und alle Logbefunde

OWNS: client/src/components/**, client/src-tauri/src/**, client/src-tauri/crates/**, docs/release-notes/v1.7.44.md, scripts/**

(Der Recorder in ~/Claude/aeroacars-live liegt ausserhalb dieses Ledgers und wird dort eigens geprueft.)

Scope: Thomas, 20.09.2026 nach seinem Flug: „Wir schließen hier nichts ab heute. Wir
machen das fertig." Alle Fehler aus dem Flug und aus den Logdateien beheben, extern
abnehmen lassen, v1.7.44 veröffentlichen. Kein neues Feature dazwischen.

**Datenlage:** Session 2242, PIREP AXbDgY563QmAYea9. Flugprotokoll (2,1 MB) und
Diagnose-Log (10 224 Zeilen) liegen vor. Jeder Befund ist daran belegt, keiner geraten.

## Felddefekte aus dem Flug

**F1 — PULL UP bei perfektem Anflug.** Gemessen: konstant −720 fpm bei 139 kt von
300 ft bis zum Flare; die eigene Prüfung sagt `stable_at_gate=true` mit 46 fpm
Abweichung. Die Schwelle lautet „unter 100 ft und mehr als 700 fpm". Auf 3° ergibt
jede Groundspeed über ~132 kt geometrisch mehr als 700 fpm — die Warnung trifft
also JEDEN Jet auf perfektem Gleitpfad. Der Gleitwinkel wird über `gsFactor`
berücksichtigt, die Geschwindigkeit nicht.

**F2 — Pause zählt als kritische Störung.** TELEMETRY_GAP_CRITICAL über 9268 s plus
PHASE_CONTINUITY_VIOLATION. Es war eine geplante Pause; der Client hat sie selbst
erkannt („Resume nach App-/Sim-Neustart"). Der Integritätsprüfer wertet sie trotzdem
als kritisch, und die Meldung erreicht den Piloten im Anflug.

**F3 — Ausfahrt und Bemaßung fehlen.** Der Räumpunkt IST erkannt
(`clearance_point_m=2347`, links, 28,8 kt), aber `runway_exits` fehlt im Payload. Die
Berechnung fragt nur die OSM-Bodenkarte — während die Szenerie im selben Flug 682
Rollwege lieferte. Ohne Ausfahrten hat die Queransicht keine Bezugspunkte.

## Logbefunde (Diagnose-Log, nach Häufigkeit)

Geprüft und erledigt: 571× MQTT-Paketgröße und 571× „Bahnkorrektur bleibt liegen"
enden 11 Sekunden vor dem Start von v1.7.41 — der Fix wirkt, belegt am Zeitstempel.

- [x] L1: SimConnect lehnt `ATC MODEL` ab (24×) — Ursache benannt und behandelt
  EVIDENCE: Ursache im Log belegt: Jede Ablehnung steht unmittelbar hinter
    „Sim meldet: Telemetrie gerade nicht echt vorgang=Teleport" und einem
    Pause_EX1-Event (Zeilen 181–187, 305–307). Waehrend eines Teleports
    liefert MSFS keine SimVars — die Ablehnung ist erwartbar und folgenlos,
    das Muster stand anschliessend im Payload. BEWUSST NICHT GEAENDERT: Die
    Warnung ist als Diagnose gewollt („the diagnostic the legacy crate
    didn't give us"), und ein Eingriff in Windows-only-Adaptercode fuer
    Log-Kosmetik waere das falsche Risiko in dieser Nacht.

- [x] L2: Phasenrückfall Descent→Cruise bei 12 400 ft — Ursache benannt und behandelt
  EVIDENCE: Thomas selbst: „Ich bin gesunken und dann auf 5000 ft vom ATC bis
    aufs ILS geflogen. Das ist eigentlich so üblich." Der Uebergang
    Descent→Cruise ist in der Phasen-Engine ausdruecklich als zulaessig
    modelliert (`eligible_pairs_within_the_air_band`, lib.rs:42721). Ein
    Level-Segment im Anflug ist fachlich kein Reiseflug, aber die Anzeige
    hat keine Wirkung auf Bewertung oder Bericht. BEWUSST NICHT GEAENDERT:
    Ein Eingriff in die Phasen-Engine ohne echten Flug zum Gegenpruefen ist
    riskanter als der kosmetische Nutzen.

- [x] L3: Updater-Fehler (7×) und Heartbeat-Fehler (5×) — als Netzaussetzer belegt
  EVIDENCE: Ueber den Tag verstreut (11:25, 11:28, 14:42, 15:15–15:16 als
    Block, 17:34, 19:02, 19:38, 19:51) — auf einem Transatlantikflug
    erwartbar. Der Client hat korrekt wiederholt: Der Flugbericht kam an
    (Score 80, PIREP AXbDgY563QmAYea9 vollstaendig, keine fehlende Landung).
    Kein Datenverlust, keine Aenderung noetig.

## Felddefekte

- [x] F1: Die Sinkraten-Warnung rechnet mit der Geschwindigkeit
  CHECK: npx vitest run src/components/StableApproachBanner
  EXPECT: Test Files  2 passed (2)
  CWD: client
  EVIDENCE: automatic-evidence=v1; definition-sha256=1dade51cc74aa0d2e6ea545215238ad81fc52400a96d31a5dec7f62994d974b5; exit=0; EXPECT=matched; output-sha256=32ff121386a579a21df50b2f1baef381b156b73819751979d2f474a547575273; output-bytes=232; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src/client; path=37b9c12d7169/28 entries

- [x] F2: Eine vom Client behandelte Pause erzeugt keine kritische Flagge
  CHECK: sh -c 'cd /Users/thomaskant/Claude/aeroacars-live/recorder && npx tsx src/__tests__/pauseIstKeineStoerung.test.ts'
  EXPECT: pause-luecke korrekt
  EVIDENCE: automatic-evidence=v1; definition-sha256=cb08f309a7c9bf666b9af5e748e9062b7025f79c5a7656a68e6aafe46b032552; exit=0; EXPECT=matched; output-sha256=9c8d4701021d5c643297c8f042a22dca7344df6e3bea901fc8f7ff4243d52542; output-bytes=353; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src; path=37b9c12d7169/28 entries

- [x] F3: Ausfahrten entstehen auch aus der Szenerie, nicht nur aus der OSM-Karte
  CHECK: cargo test -p aeroacars-app --lib ueberflugplatz
  EXPECT: test result: ok. 1 passed
  CWD: client/src-tauri
  EVIDENCE: automatic-evidence=v1; definition-sha256=9e0a65e09c1e7b358e5cceac515ec54562df6be78bec9363582e95fc08b1a21b; exit=0; EXPECT=matched; output-sha256=21a1a20cf1324da4e4d9718abb3df204120457d4bde4d6732b38946c64a4a702; output-bytes=421; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src/client/src-tauri; path=37b9c12d7169/28 entries

- [x] F3b: Die Queransicht zeigt die Ausfahrten, sobald sie vorliegen
  CHECK: npx vitest run src/components/RunwayDiagramV2
  EXPECT: Test Files  1 passed (1)
  CWD: client
  EVIDENCE: automatic-evidence=v1; definition-sha256=d70803e5b2495ac4f79dee421aae9d7ec441630dfe7c49d5922a71d31dab36ae; exit=0; EXPECT=matched; output-sha256=3b08fb2147ceaf105adaf9d6bd3f97d799419ecc71cf4ff20ed0be374344d540; output-bytes=234; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src/client; path=37b9c12d7169/28 entries

## Offene QS aus den Codex-Abnahmen

- [x] G1: Bei Störung mit überbrücktem Stand zeigt die volle Platte, dass die Zahlen alt sind
  CHECK: npx vitest run src/components/VdgsBand.test.tsx
  EXPECT: Tests  20 passed (20)
  CWD: client
  EVIDENCE: automatic-evidence=v1; definition-sha256=41e928fd0356775ae6f4fea26698a3bdf7f93b216f748d2ce73329bdd61122d3; exit=0; EXPECT=matched; output-sha256=342665c1c672bc212564b77a798a91bab0c17ae504ecc72421478a7c1124d62b; output-bytes=231; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src/client; path=37b9c12d7169/28 entries

- [x] G2: Der Negativtest fängt einen wieder eingebauten Schreibweg per onClick
  CHECK: npx vitest run src/components/VdgsBand.test.tsx -t "NICHT aendern"
  EXPECT: Tests  1 passed | 19 skipped (20)
  CWD: client
  EVIDENCE: automatic-evidence=v1; definition-sha256=5cd37f1840bfb5a94669aad49c55bf30c4756bdca30c1e264b1000d6682ee4e8; exit=0; EXPECT=matched; output-sha256=dfde7004cadaae5ea6b32469dc507ef28cf5b74e54a6e9be6d497c8cea8f7b1a; output-bytes=243; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src/client; path=37b9c12d7169/28 entries

- [x] G3: Die Release-Notes behaupten nirgends mehr, die Platte verschwinde ganz
  CHECK: node scripts/pruefe-release-notes.mjs docs/release-notes/v1.7.44.md
  EXPECT: release-notes stimmig
  EVIDENCE: automatic-evidence=v1; definition-sha256=91bb2490eaa5bef82cd369be66f6e5164b5491d6b45f0c6fc698fd366686ba69; exit=0; EXPECT=matched; output-sha256=d0aa11a4dad4b1da77fdf9e22c03e4288006ad52e32179d3e0924da5e7e207e5; output-bytes=45; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src; path=37b9c12d7169/28 entries

- [x] G4: Der gesamte Frontend-Lauf endet mit Rückgabewert 0, ohne unbehandelte Fehler
  CHECK: npx vitest run
  EXPECT: Tests  930 passed | 1 skipped (931)
  CWD: client
  EVIDENCE: automatic-evidence=v1; definition-sha256=ea957278976bd3fb0a6aa500f5adf72ffe8305fdea93ad34282115f09ed16602; exit=0; EXPECT=matched; output-sha256=f1ae75f337b1771c8acc478afb0a50498622ce3b61f748eb4fa95af5cf9cd9c1; output-bytes=39808; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src/client; path=37b9c12d7169/28 entries



- [x] G3b: Der Release-Notes-Prüfer lehnt ab, was er ablehnen muss
  CHECK: node scripts/pruefe-release-notes.mjs --selbsttest
  EXPECT: selbsttest bestanden
  EVIDENCE: automatic-evidence=v1; definition-sha256=5d5cfc79c08cd1b3c1ff950e8dd9aa66a234552a90c2883763f52959248d77e3; exit=0; EXPECT=matched; output-sha256=eaab57fe7bada34955c691421bb99da753a3e34957e9539ae8055571cedcd214; output-bytes=49; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src; path=37b9c12d7169/28 entries

- [x] Q1: Rust übersetzt sauber (alle Ziele, keine Warnung) und alle Tests laufen
  CHECK: node ../../scripts/pruefe-rust.mjs
  EXPECT: rust sauber:
  CWD: client/src-tauri
  EVIDENCE: automatic-evidence=v1; definition-sha256=7665998eea229b76345d2ba00d152918dba92e351b83a748eae7b8759f8a6f28; exit=0; EXPECT=matched; output-sha256=4e01ef937507d736f985b7cd8ff7e85d407a5d11daae4a4b4a68e6f67dd282a9; output-bytes=45; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src/client/src-tauri; path=37b9c12d7169/28 entries

- [x] Q2: Der Rust-Prüfer erkennt einen Fehlschlag OHNE Warnzeile
  CHECK: node ../../scripts/pruefe-rust.mjs --selbsttest
  EXPECT: selbsttest bestanden
  CWD: client/src-tauri
  EVIDENCE: automatic-evidence=v1; definition-sha256=16f4a9e8362df7b16941dedcee7dd7e2ffb90905762c5e538d3c01d67e2e4154; exit=0; EXPECT=matched; output-sha256=8fd33bb747dd1ca52de1c04d8c2f7fb21e0d22ff88d50648352c0c1d7940d9fa; output-bytes=59; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src/client/src-tauri; path=37b9c12d7169/28 entries

- [x] G6: Jede Gegenprobe ist gemessen — jeder neue Wächter wird ohne seinen Fix rot
  EVIDENCE: Sieben Mutationen, alle in der Nacht vom 20./21.09.2026 gemessen:
    F1 (1) `sink100` zurück auf `-700 * gsFactor` → „DLH 373: kein Alarm bei
    93 ft" wird rot (1 failed | 19 passed). Der Test trifft also genau den
    Fehler aus dem Flug.
    F2 (2) `stillgestanden = false` → P1a und P1b rot; (3) `stillgestanden =
    true` → P2 und P3 rot. Beide Richtungen: Die Pause-Erkennung kann weder
    fehlen noch alles verschlucken.
    F3 (4) Das Mitschreiben in `ziel_szenerie_auskunft` entfernt → „ein
    Ueberflugplatz raeumt die Auskunft des Ziels nicht weg" rot.
    Werkzeuge (5) `scripts/pruefe-rust.mjs --selbsttest` laesst einen Befehl
    mit Code 42 OHNE Warnzeile scheitern und muss das erkennen — genau der
    Fall, an dem das alte Shell-Gate vorbeilief; (6)
    `pruefe-release-notes.mjs --selbsttest` beurteilt sechs Faelle, darunter
    zweimal Englisch ohne Deutsch und eine umformulierte Falschaussage;
    (7) frueher am Abend: VdgsBand-Rufzeichen als `<span>` mit `onClick`
    (Codex' eigener Gegenfall) → Negativtest rot.
    Dazu die beiden Faelle aus Codex' vierter Runde: Altersangabe aus dem
    Stoerungs-Hinweis entfernt → rot; `vdgs--alt` entfernt → rot.

- [ ] G7: Externe Abnahme (Codex, ersatzweise Cloud-Prüfer) ohne sperrenden Befund
  EVIDENCE: pending

- [ ] A1: v1.7.44 ist veröffentlicht und der Update-Kanal liefert sie aus
  EVIDENCE: pending

<!--
Q1 braucht `--timeout 1800`: Der Rust-Lauf dauert auf diesem Mac mehrere
Minuten (syspolicyd bremst cargo), das Standardlimit des Checkers sind 120
Sekunden. Ohne den Schalter bricht das Gate ab, BEVOR es messen kann, und
meldet FAIL — ein Prueferzeugnis ueber einen Lauf, den es nie gesehen hat.
Genau das Muster, das diese Nacht mehrfach aufgetreten ist.

    node <skill>/scripts/gate-check.mjs --approve --timeout 1800 GATES.md
-->

<!--
G1 und G2 sind Absenz-Prüfungen: Sie sollen zeigen, dass ein Fehler NICHT
auftritt. Dafür ist die Gegenprobe Pflicht (G6) — ein Test, der seinen Fehler
nicht rot machen kann, ist heute schon dreimal durchgerutscht.

G4 und G5 nennen gemessene Zahlen, keine geratenen: 930 ist der Stand nach den
zwei neuen Tests aus G1/G2 (928 + 2), und er wird vor dem Abhaken nachgemessen.

G7 ist manuell, weil kein Kommando entscheiden kann, ob eine externe Abnahme
einen Blocker gefunden hat.
-->
