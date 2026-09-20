# Gates: v1.7.44 — die drei Befunde der dritten Codex-Abnahme

OWNS: client/src/components/**, client/src-tauri/src/vdgs.rs, docs/release-notes/v1.7.44.md

Scope: Die drei Befunde aus Codex' dritter Runde schließen, ohne neue aufzureißen —
der überbrückte Altstand muss als gestört erkennbar sein, der Negativtest muss einen
wieder eingebauten Schreibweg fangen, und die Release-Notes dürfen sich nicht selbst
widersprechen. Danach eine vierte Codex-Abnahme.

Auftrag Thomas, 20.09.2026: „alle drei beheben und dann nochmal Codex".

**Befund 1 ist der einzige mit Wirkung auf den Piloten.** Ist der Dienst gestört und
liegt ein Stand aus den letzten zehn Minuten vor, liefert das Backend korrekt
`stand: Some(alt), stoerung: true` — die Anzeige prüft `stoerung` aber nur im
`!stand`-Zweig. Der Pilot sieht dann alte TSAT/CTOT als normale Platte und kann sie für
aktuell halten. Das ist derselbe Fehlertyp wie am Nachmittag: Die Korrektur wirkte eine
Ebene tief, aber nicht bis zur Anzeige.

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
  EVIDENCE: automatic-evidence=v1; definition-sha256=91bb2490eaa5bef82cd369be66f6e5164b5491d6b45f0c6fc698fd366686ba69; exit=0; EXPECT=matched; output-sha256=ed80cc23764e8d7d848c2af1999126bc0d33663a97c526ae6915d3b29f67a748; output-bytes=44; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src; path=37b9c12d7169/28 entries

- [x] G4: Der gesamte Frontend-Lauf endet mit Rückgabewert 0, ohne unbehandelte Fehler
  CHECK: npx vitest run
  EXPECT: Tests  930 passed | 1 skipped (931)
  CWD: client
  EVIDENCE: automatic-evidence=v1; definition-sha256=ea957278976bd3fb0a6aa500f5adf72ffe8305fdea93ad34282115f09ed16602; exit=0; EXPECT=matched; output-sha256=f1ae75f337b1771c8acc478afb0a50498622ce3b61f748eb4fa95af5cf9cd9c1; output-bytes=39808; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src/client; path=37b9c12d7169/28 entries

- [x] G5: Rust-Workspace und cargo check ohne Fehler und ohne Warnung
  CHECK: cargo test --workspace --lib
  EXPECT: test result: ok.
  CWD: client/src-tauri
  EVIDENCE: automatic-evidence=v1; definition-sha256=b8dad0fb9ca592568cec3a8e40e1b6d748bd3cb2246427840edce4933bc98e8e; exit=0; EXPECT=matched; output-sha256=d5c74329b7b539fb242ca4f8b071817e3a2a80ab0cd160169e912c68cbe57a5e; output-bytes=175220; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src/client/src-tauri; path=37b9c12d7169/28 entries

- [x] G5b: cargo check meldet keine einzige Warnung
  CHECK: sh -c 'cargo check --message-format short 2>&1 | grep "^warning" && exit 1; echo "cargo check ohne Warnung"'
  EXPECT: cargo check ohne Warnung
  CWD: client/src-tauri
  EVIDENCE: automatic-evidence=v1; definition-sha256=1a7c439dcbcc74a1fa8529e1c6367c954f9bda83b38326e2f28defb2dad2c6d4; exit=0; EXPECT=matched; output-sha256=036a1209395c8584404a5e479930f8964ed5ceaef6bad8e2862d5e5332d42688; output-bytes=25; shell=/bin/sh; cwd=/Users/thomaskant/Claude/aeroacars-src/client/src-tauri; path=37b9c12d7169/28 entries

- [x] G6: Jede Gegenprobe ist gemessen — jeder neue Wächter wird ohne seinen Fix rot
  EVIDENCE: Drei Wächter, vier Gegenproben, alle am 20.09.2026 gemessen:
    (1) „sagt bei Stoerung, dass die gezeigten Zahlen alt sind" — mit
    `{false && (` statt `{antwort.stoerung && (` → 1 failed | 19 passed.
    (2) Gegenrichtung: mit `{true && (` wird „zeigt ohne Stoerung KEINEN
    Alt-Hinweis" rot → 1 failed | 19 passed. Beide Richtungen greifen, die
    Warnung kann also weder fehlen noch dauerhaft dastehen.
    (3) „laesst das Rufzeichen hier NICHT aendern" — mit genau dem Fall aus
    Codex' Befund 2 (span bleibt, bekommt `onClick` mit Schreibbefehl) →
    1 failed | 19 skipped. Der erste Entwurf dieses Tests blieb dabei grün.
    (4) `scripts/pruefe-release-notes.mjs` gegen die alte Notes-Fassung →
    exit=1 mit benanntem Grund; gegen die neue → exit=0.

- [ ] G7: Vierte Codex-Abnahme ohne Befund, der die Auslieferung sperrt
  EVIDENCE: pending

<!--
G1 und G2 sind Absenz-Prüfungen: Sie sollen zeigen, dass ein Fehler NICHT
auftritt. Dafür ist die Gegenprobe Pflicht (G6) — ein Test, der seinen Fehler
nicht rot machen kann, ist heute schon dreimal durchgerutscht.

G4 und G5 nennen gemessene Zahlen, keine geratenen: 930 ist der Stand nach den
zwei neuen Tests aus G1/G2 (928 + 2), und er wird vor dem Abhaken nachgemessen.

G7 ist manuell, weil kein Kommando entscheiden kann, ob eine externe Abnahme
einen Blocker gefunden hat.
-->
