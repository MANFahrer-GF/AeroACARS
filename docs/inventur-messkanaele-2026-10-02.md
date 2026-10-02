# Inventur der Messkanäle und Datenlücken — Stand 02.10.2026

**Zweck:** Statt Einzelfixes je Flugzeug einmal das ganze Bild: Welche Werte fehlen uns bei welchem Simulator und Flugzeug, warum, und welche Regel gilt künftig überall? Dieses Dokument ist nur Auswertung — kein Code. Es ist die Grundlage für **eine** Sammelversion (v1.9.17 ist deshalb absichtlich noch nicht veröffentlicht; der Branch `release/v1.9.17` mit dem A220-Klappenfix liegt bereit).

## 1. Stichprobe und Grenzen

- **Teil A (Payload):** alle 705 Landungen der letzten 60 Tage aus `touchdowns` auf dem Live-Server (nicht überschriebene), 166 verschiedene Payload-Felder. Aufgeschlüsselt nach Simulator, Flugzeugtyp und Client-Version.
- **Teil B (Rohkanäle):** 58 Flugzeugtypen, je die jüngsten bis zu 4 Client-Logs (Positionsschnappschüsse), je Kanal klassifiziert: *liefert Werte* / *konstant* (nie bewegt) / *fehlt ganz* / *gemischt*.
- **Wichtige Einschränkung:** Der Bestand ist ein Gemisch alter und neuer Clients. Nur **60 Landungen** stammen von Clients ab 1.9.0, davon nur **6 von X-Plane**. Aussagen über den *heutigen* Stand von X-Plane sind deshalb dünn.
- **Korrektur einer früheren Aussage:** Ich hatte zunächst geschrieben, bei X-Plane fehle die Landekonfiguration „fast überall“. Das gilt nur für **ältere Clients**. Bei Clients ab 1.9.3 ist sie bei A20N, A21N, B738, B77W, DH8D und E75L vorhanden. Der aktuelle Befund ist **nur BCS3**.

## 2. Das Gesamtbild in einem Satz

Die Bewertung stützt sich auf Messkanäle, die je Simulator und Flugzeug verschieden verfügbar sind; fehlt einer, gibt es weder eine einheitliche Ersatzregel noch eine automatische Meldung — wir merken es erst, wenn jemand ein einzelnes Flugzeug anschaut.

## 3. Befunde (nach Wirkung)

### B1 — TCH (Überflughöhe an der Schwelle) fehlt bei ~85 % der Landungen — **Ursache belegt**

- Clients ab 1.9.0: fehlt bei 51 von 60 Landungen (A320 8/10, A388 7/7, B772 5/6, B738 5/5, A321 4/4, …).
- **Ursache:** Der Wert wird beim Aufsetzen aus dem rollenden **5-Sekunden**-Puffer (`TOUCHDOWN_BUFFER_SECS = 5`, `snapshot_buffer`) gelesen: erste Probe hinter der Schwelle. Bei Verkehrsflugzeugen liegen zwischen Schwellenüberflug und Aufsetzen aber ~10 s; die Probe ist dann längst aus dem Puffer.
- **Beleg (Clients ab 1.8.0, Zeit Schwelle→Aufsetzen aus Aufsetzstrecke und Geschwindigkeit über Grund):** *TCH vorhanden* bei 10 Landungen, Median 4,3 s, **Maximum 4,7 s**; *TCH fehlt* bei 61 Landungen, Median 9,6 s. Über 5 s gibt es **keine einzige** Landung mit Wert (0 von 10).
- **Folge:** Gemessen wird praktisch nur bei kurzen Aufsetzern (GA). `tch_rad_ft` und `tch_hoehengruppe` (seit 1.9.9) fehlen aus demselben Grund.
- Im Code ist TCH als „forensisch/Coaching, keine Note“ vermerkt; das Fehlen bewertet also nichts falsch, die Anzeige ist aber überwiegend leer.

### B2 — Synaptic A220 (BCS3): Landekonfiguration „nicht bewertbar“ — **Ursache teilweise belegt**

- Seit **Client 1.7.3** (25.08.2026) bei allen BCS3-Landungen leer (bis 1.7.2 vorhanden). Heute 18 von 19 BCS3-Landungen der letzten 30 Tage.
- Fahrwerk ist korrekt (Wert 1,0). **Klappenwert** (`L:SYN_FLAP_LEVER`) liest dauerhaft **0,0**, die Raste (`FLAPS HANDLE INDEX`, 5 Rasten) läuft 0→5. Die Regel „Klappenkanal bewegt sich nie → nicht bewertbar“ (v1.6.6) greift.
- **Zwischen v1.7.2 und v1.7.3 gibt es keine Änderung** in `sim-msfs`/`sim-core` (`git log`), der Wechsel am selben Tag ist also **nicht** durch unseren MSFS-Code erklärt. Wahrscheinlich ein Addon-Update (Synaptic) oder MSFS-Update um den 25.08. — **nicht bewiesen**.
- Fix bereit: Branch `fix/a220-klappen-aus-hebelraste` (Raste als Rückfall, nur wenn der Wert 0 liest). Windows-CI grün.

### B3 — Vref-Abweichung fehlt bei Mustern ohne Tabelleneintrag — **Ursache belegt**

- Clients ab 1.6.6: 21 von 594 fehlen: **PC12 6/6, BE60 6/6, LJ35 3/3**, je 1× YK18, DA50, E13L (und Einzelfälle bei BCS3, A388, MD11).
- **Ursache:** Der Sim liefert keine Vref (siehe B6); wir vergleichen gegen unsere eigene Tabelle (`typical_vref_kt`), und diese Muster stehen nicht darin. Der Health-Report meldet solche Muster bereits (`unbekanntes_muster`), die Tabelle wächst aber nur, wenn jemand sie ergänzt.

### B4 — Gleitpfad/Anflugruhe: **kein Defekt**

- Fehlen scheinbar bei 37 von 60 (ab 1.9.0), aber nur, weil die Felder erst mit v1.9.12 kamen. Bei Clients ab 1.9.14: **0 von 20 fehlen**.

### B5 — weitere Felder mit Lücken — **aufgelöst (Untersuchung 02.10.2026)**

- `vs_gelaende_fpm` / `vs_eigensinken_fpm`: 10 von 60 (BCS3 2/4, LJ35 2/3, E195 2/4, …).
- `runway_exits` / `clearance_side`: 4 von 60 (AC11, B738, E190, BCS3).
- **`vs_gelaende_fpm` / `vs_eigensinken_fpm` — Defekt, belegt.** Die Rechnung wandte die Riegel „mindestens 3 verschiedene Werte“ und „höchstens 120 ms Stillstand“ auch auf die **Bodenhöhe** (MSL − AGL) an. Ebenes Gelände steht legitim still, die Aufschlüsselung wurde dann ganz verworfen. Nachgerechnet an 59 von 60 Landungen: bei allen 9 ohne Wert ist die Geländespur konstant, bei allen 50 mit Wert nicht. **Behoben** in der Sammelversion (Bodenhöhe ohne Bewegungsriegel; Geländeanteil 0, Eigensinken = gemessene Rate).
- **`runway_exits` / `clearance_side` — zwei getrennte Ursachen, nicht gebaut.** (1) 3 Landungen mit Client 1.9.8–1.9.10: `rollout_finalized` kam ohne Bahnblock (`rollout_final = false`); ab 1.9.11 sind 14 von 14 vollständig, ein Fix dazwischen ist im Verlauf nicht erkennbar. (2) `runway_exits` wird nur beim Aufsetzen berechnet; fehlt die Platzkarte dann, wird es nicht nachgeholt (AC11/EDHE, EDDM 08L, BCS3/EDDC). Vorschlag (klein bis mittel, **nicht in dieser Version**): Ausfahrten beim Abschluss des Ausrollens neu rechnen, Zustand „keine Karte beim Aufsetzen“ gegen „Karte da, keine Ausfahrt“ speichern, Grund für `bahn: None` im Ereignis mitschicken.
- **LJ35 ohne `wingspan_m`:** Die Tabelle kennt den LJ35 (12,04 m), trotzdem steht die Spannweite nicht im Payload; die Auflösung des Musters bei diesen Flügen habe ich nicht nachvollzogen — **offen**.
- **Kein Defekt, aber leere Anzeige:** `td_third`/`td_in_tdz` fehlen bei Bahnen unter 1200 m (AC11/EDHE, 676 m) — gewollt, die Anzeige sollte „Bahn zu kurz für Aufsetzzone“ sagen statt ein leeres Feld zu zeigen.

### B6 — Rohkanäle je Simulator (Teil B)

Zahlen = Anzahl Flugzeugtypen, bei denen der Kanal in den jüngsten Logs Werte liefert / konstant bleibt / ganz fehlt. Verteilung über Typen, nicht über Landungen.

**MSFS** (49 Typen)

| Kanal | liefert Werte | konstant | fehlt ganz | gemischt |
|---|---|---|---|---|
| Fahrwerk | 44 | 5 | 0 | 0 |
| Klappen (Wert) | 48 | 1 | 0 | 0 |
| Klappen (Raste) | 49 | 0 | 0 | 0 |
| Vref aus Sim | 1 | 2 | 46 | 0 |
| Vapp aus Sim | 1 | 0 | 48 | 0 |
| Spoiler-Hebel | 34 | 15 | 0 | 0 |
| Autobrake | 13 | 1 | 35 | 0 |
| Schubumkehr | 4 | 0 | 45 | 0 |
| Bodenspoiler | 4 | 0 | 45 | 0 |
| N1 | 35 | 0 | 14 | 0 |
| A/THR | 8 | 4 | 37 | 0 |
| Fahrwerkskraft | 2 | 0 | 47 | 0 |
| FMA vertikal | 1 | 2 | 46 | 0 |

**X-Plane** (9 Typen)

| Kanal | liefert Werte | konstant | fehlt ganz | gemischt |
|---|---|---|---|---|
| Fahrwerk | 9 | 0 | 0 | 0 |
| Klappen (Wert) | 3 | 6 | 0 | 0 |
| Klappen (Raste) | 1 | 0 | 8 | 0 |
| Vref aus Sim | 0 | 0 | 9 | 0 |
| Vapp aus Sim | 0 | 0 | 9 | 0 |
| Spoiler-Hebel | 8 | 1 | 0 | 0 |
| Autobrake | 4 | 5 | 0 | 0 |
| Schubumkehr | 0 | 0 | 9 | 0 |
| Bodenspoiler | 0 | 0 | 9 | 0 |
| N1 | 1 | 0 | 8 | 0 |
| A/THR | 2 | 6 | 1 | 0 |
| Fahrwerkskraft | 9 | 0 | 0 | 0 |
| FMA vertikal | 0 | 0 | 9 | 0 |

Auffällig:
- **MSFS:** Fahrwerk und beide Klappenkanäle sind fast überall da. **Die Raste (`flap_handle_index`) liefert bei allen 49 Typen Werte** — als allgemeiner Rückfall für den Klappenwert tauglich. Der Klappen**wert** ist nur beim BCS3 konstant.
- **MSFS:** Vref/Vapp aus dem Sim gibt es praktisch nie (1 von 49) — die eigene Tabelle ist die Hauptquelle, nicht ein Notbehelf. Schubumkehr, Bodenspoiler, Fahrwerkskraft fehlen bei fast allen.
- **X-Plane:** Raste fehlt in **allen** 247 X-Plane-Logs (kein Rückfall möglich). **Korrektur der ersten Fassung:** Die „konstanten“ Klappenwerte bei B742, B752, B763, DH8D, E13L, E170 stammen von Clients ≤ 1.8.2. Mit aktuellen Clients bewegen sich Klappen und Fahrwerk bei allen sechs X-Plane-Typen, von denen es Landungen ab 1.9.3 gibt (B77W, E175, A21N, A20N, DH8D, B738). Für B742, B752, B763, E170, E190, A320, A321 u. a. gibt es **keine Daten eines aktuellen Clients** — dort ist nichts belegt.
- Festes Fahrwerk (C152, C172, C182, C208, DA40) zeigt „konstant“ — das ist korrekt, kein Defekt.

## 4. Was strukturell fehlt

1. **Keine Kanal-Matrix:** Wir führen nirgends, welcher Kanal bei welchem Simulator/Profil Werte liefert. Dieses Dokument ist die erste Fassung, von Hand erzeugt.
2. **Keine einheitliche Rückfall-Regel:** Jeder Kanal hat seine eigene, gewachsene Behandlung (Klappen: Wert → „nicht bewertbar“; Vref: Tabelle → nichts; TCH: Puffer → nichts).
3. **„Nicht gemessen“ ohne Ursache und ohne Zählung:** Der Health-Report nennt unbekannte Muster, aber nicht „Kanal X liefert bei Typ Y seit Version Z nichts“. B2 lief so mindestens **5 Wochen** unbemerkt (25.08. bis 02.10.).

## 5. Entwurf der Regel (zur Entscheidung, noch nicht gebaut)

**Je Messgröße ein Rückfall, einmal definiert, überall angewendet:**

| Messgröße | Reihenfolge | wenn alles fehlt |
|---|---|---|
| Klappenstellung | Wert → Raste (Stufe/Rastenzahl) | „nicht messbar“ + Grund `klappenkanal_tot` |
| Vref | Sim-Wert → Tabelle → Musterfamilie | „nicht messbar“ + Grund `kein_vref` |
| TCH | Dauer-Aufzeichnung der Höhe in Schwellennähe | „nicht messbar“ + Grund |
| Alle | — | zählt im Health-Report je Typ/Version |

Grundsatz bleibt: **ohne Messung keine Note** (kein stilles „bestanden“, keine Strafe).

## 6. Vorgeschlagene Sammelversion (v1.9.17) — **Stand der Umsetzung 02.10.2026**

Gebaut auf Branch `feat/sammelversion-messkanaele`: Maßnahme 1 (Raste als Rückfall für alle MSFS-Muster), 2 (TCH aus langsamer Schwellenspur mit Interpolation), 3 (Vref: PC12, LJ35, BE60, E13L — DA50 und YK18 bewusst **nicht**, Wert unbelegt), 4 (`/api/admin/kanal-luecken` im Recorder + Health-Mail-Modul v0.1.37), dazu B5/A (Geländeanteil). Nicht gebaut: B5/B (Ausfahrten), LJ35-Spannweite, X-Plane-Nachmessung.

| # | Maßnahme | Evidenz | Aufwand |
|---|---|---|---|
| 1 | A220-Klappen aus Raste — **fertig**, auf allen MSFS-Mustern als Rückfall prüfen statt nur A220 | belegt (B2, B6) | klein |
| 2 | TCH-Aufzeichnung verlängern/umbauen (Höhe bei Schwellenüberflug festhalten statt aus 5-s-Puffer) | belegt (B1) | mittel (Bahn ist beim Anflug noch nicht bekannt — Ansatz klären) |
| 3 | Vref-Tabelle: PC12, BE60, LJ35, YK18, DA50, E13L | belegt (B3) | klein, Werte aus FCOM/POH belegen |
| 4 | Health-Report: „Kanal liefert nichts“ je Typ/Version, automatisch | Lücke belegt (Abschnitt 4) | mittel |
| 5 | Ursache B5 (`vs_gelaende`, `runway_exits`) klären | offen | klein–mittel |
| 6 | X-Plane-Stand mit aktuellen Clients nachmessen | zu wenig Daten | braucht Flüge |

**Nicht tun:** weitere Fixes je Flugzeug ohne diese Regel.

## 7. Offene Fragen

- Hat Synaptic um den 25.08.2026 die Variable geändert? (Addon-Changelog prüfen.)
- Soll TCH weiter nur Information bleiben, oder wird es künftig bewertet? Davon hängt der Aufwand für Maßnahme 2 ab.
- Wie viele X-Plane-Landungen mit Client ≥ 1.9.12 gibt es, damit B6/X-Plane belastbar wird?
