# Aircraft-Type-Match — Architektur, Aliases, Maintenance

**Status:** v1.2 — **Approved for implementation** (Stand AeroACARS v0.7.3 + Pending v0.7.4 Polish)
**Cutoff:** Forward-only — gilt fuer alle Bids ab AeroACARS v0.7.2
**Vorgaenger:** keine — initial Spec nach Live-Bug 2026-05-10 (MPH62 MD-11)
**Goal:** Pilot wird NIE wieder von einer Cargo-/Variant-Bid blockiert obwohl er das richtige Flugzeug geladen hat. Gleichzeitig: echter Mismatch (z.B. A320 statt B738) wird zuverlaessig erkannt und blockiert.

---

## Leitprinzip

Aircraft-Type-Matching soll Piloten nicht unnoetig blockieren, aber offensichtliche falsche Flugzeugtypen weiterhin erkennen.

Wir bevorzugen **pragmatische, nachvollziehbare Aliases** gegenueber perfekter ICAO-Strenge. ICAO Doc 8643 ist die Referenz, aber phpVMS-, SimBrief- und VA-Daten koennen in der Praxis abweichen. Solche Abweichungen sind erlaubt, wenn sie dokumentiert und mit mindestens einem positiven und einem negativen Test abgesichert sind.

Ziel ist **nicht**, jede moegliche Variante vorab perfekt zu modellieren. Ziel ist, neue False-Positive-Blocker schnell, kontrolliert und ohne breite Wildcards zu beheben.

### Die drei harten Regeln

1. **Keine extrem breiten Aliases** wie `A3`, `747`, `MD`, `AIRBUS` — Substring-Match ist sensitiv, breite Patterns kollidieren mit unverwandten Familien.
2. **Jeder neue Alias bekommt mindestens einen Match-Test** (Bid-ICAO ↔ erwarteter Sim-Title).
3. **Jeder neue Alias bekommt mindestens einen offensichtlichen Mismatch-Test** (gegen eine unverwandte Familie).

Alles andere in dieser Spec ist Empfehlung und Arbeitsliste, nicht Pflicht.

---

## 0. Warum dieses Dokument

Bei Pilot-Tests sind in 2026 zwei Live-Bugs aufgetreten:

1. **2026-05-04 — Emirates UAE770 (A359-Bid):** Sim hat "A350-900 (No Cabin)" geladen, AeroACARS blockierte mit `aircraft_mismatch` weil ICAO-Code `A359` strict gegen Sim-Title `A350-900` verglichen wurde. → Aliases-Tabelle eingefuehrt.
2. **2026-05-10 — Martinair Cargo MPH62 (MD11-Bid):** Sim hat "TFDi Design MD-11F" geladen, AeroACARS blockierte. → MD-11-Familie hatte keinen Eintrag in der Aliases-Tabelle (klassischer Vergessen-Eintrag, B777F existierte schon).

**Befund:** Aliases-Tabelle wird **ad-hoc gewartet** wenn der naechste Live-Bug auftritt. Das ist nicht systematisch — wir brauchen:
- Klare Architektur was matched warum
- Liste aller bekannten ICAO-Familien die Piloten realistisch fliegen
- Test-Matrix die jede Familie pro Bug-Anfaelligkeit deckt
- Maintenance-Workflow wenn ein neuer Live-Bug kommt

Diese Spec ist die Antwort. Kein neues Verhalten — sondern Dokumentation des Status-Quo + Lueckenanalyse + Prozess.

---

## 1. Scope

### Was DRIN ist

| # | Inhalt |
|---|---|
| §2 | Aktuelle Architektur (`aircraft_aliases` + `aircraft_types_match` + `title_mentions_icao`) |
| §3 | Inventur der vorhandenen Aliases (52 Eintraege per v0.7.3 + Polish v0.7.4 Pending) |
| §4 | Lueckenanalyse: bekannte fehlende Aircraft-Familien |
| §5 | Test-Matrix pro Aircraft-Familie + Bug-Klasse |
| §6 | Maintenance-Workflow (was tun wenn ein Live-Bug kommt) |
| §7 | Datenkontrakt-Garantien (was matched, was nicht) |
| §8 | Backward-Compat + Migrations-Strategie |

### Was BEWUSST NICHT in dieser Spec

- **Algorithmus-Refactor.** Die Substring-Match-Logik (`alias.iter().any(|a| title.contains(a))`) bleibt. Sie ist pragmatisch und funktioniert seit 2026-05-04.
- **Strict-Mode pro VA.** Manche VAs wollen evtl. striktere Matches (z.B. nur exakte Frachter-Variante). Das ist v0.8.x-Diskussion, nicht hier.
- **Type-Database-Sync mit phpVMS.** Wir spiegeln die phpVMS-`aircrafts.icao`-Werte nicht nach AeroACARS — wir matchen tolerant gegen den vom Sim gemeldeten Title.

---

## 2. Architektur

### 2.1 Komponenten

```
                  ┌─────────────────────────┐
phpVMS Bid ──────▶│   expected_icao         │
("MD11")          │   (4-letter ICAO code)  │
                  └────────────┬────────────┘
                               │
                               ▼
                  ┌─────────────────────────┐
Sim Snapshot ────▶│ aircraft_types_match()  │ ──▶  bool
("MD11F" + Title  │  + title_mentions_icao  │
 "TFDi Design     │                         │
  MD-11F PW...")  └────────────┬────────────┘
                               │
                               ▼
                  ┌─────────────────────────┐
                  │ aircraft_aliases()      │  ◀──  pflegbare
                  │   "MD11" → ["MD-11",    │       Tabelle
                  │             "MD11"]     │
                  └─────────────────────────┘
```

### 2.2 Pruef-Reihenfolge

```rust
// lib.rs:5215
let types_match_loose   = aircraft_types_match(expected, actual);
let title_supports      = title_mentions_icao(&sim_title, expected);
if !types_match_loose && !title_supports {
    return Err(UiError::new("aircraft_mismatch", ...));
}
```

**Alternative-Pruefungen, BEIDE muessen fehlschlagen** damit blockiert wird:

| Pruefung | Pfad |
|---|---|
| `aircraft_types_match(expected, actual)` | ICAO ↔ ICAO mit Aliases (z.B. `MD11` ↔ `MD11F`) |
| `title_mentions_icao(sim_title, expected)` | Title-Substring-Suche (z.B. "TFDi Design **MD-11F**...") |

Das macht den Match **tolerant** — sobald EINE der beiden Pruefungen passt, geht der Flug los. Strict-Mismatch (z.B. A320 statt B738) wird nur dann blockiert wenn **beide** Pruefungen fehlschlagen.

### 2.3 `aircraft_aliases` Format

```rust
fn aircraft_aliases(code: &str) -> &'static [&'static str] {
    match code {
        "MD11"  => &["MD-11", "MD11"],
        "MD11F" => &["MD-11F", "MD11F"],
        ...
        _ => &[],  // Fallback: strict equality only
    }
}
```

**Aliases sind Substring-Patterns die im Sim-Title gesucht werden.** Es ist **kein bidirektionales Mapping** ICAO↔ICAO sondern eine Liste von Strings die "wahrscheinlich im Title vorkommen wenn das richtige Flugzeug geladen ist".

### 2.4 Symmetrie

`aircraft_types_match` prueft **beide Richtungen**:

```rust
aircraft_aliases(&exp).iter().any(|alias| act.contains(alias))
    || aircraft_aliases(&act).iter().any(|alias| exp.contains(alias))
```

Damit funktioniert sowohl:
- Bid `MD11` + Sim-ICAO `MD11F` → `aliases("MD11")=["MD-11", "MD11"]`, `"MD11F".contains("MD11")` ✓
- Bid `MD11F` + Sim-ICAO `MD-11F` → `aliases("MD11F")=["MD-11F", "MD11F"]`, `"MD-11F".contains("MD-11F")` ✓

---

## 3. Inventur (Stand v0.7.3 + v0.7.4 Pending)

**52 ICAO-Codes** mit expliziten Aliases. Pro Familie:

### 3.1 Airbus (17)

| Familie | ICAO-Codes mit Alias |
|---|---|
| A220 | `BCS1`, `BCS3` |
| A320 ceo+neo | `A318`, `A319`, `A320`, `A321`, `A20N`, `A21N` |
| A330 | `A332`, `A333`, `A338`, `A339` |
| **A330 Cargo** (NEU v0.7.3) | `A332F` |
| A340 | `A342`, `A343`, `A345`, `A346` |
| A350 | `A359`, `A35K` |
| A380 | `A388` |

### 3.2 Boeing (27)

| Familie | ICAO-Codes mit Alias |
|---|---|
| 717 | `B712` |
| 737 NG | `B736`, `B737`, `B738`, `B739` |
| 737 MAX | `B37M`, `B38M`, `B39M`, `B3XM` |
| 747 | `B741`, `B742`, `B744`, `B748` |
| **747 Cargo** (NEU v0.7.3) | `B74F`, `B748F` |
| 757 | `B752`, `B753` |
| **757 Cargo** (NEU v0.7.3) | `B752F` |
| 767 | `B762`, `B763`, `B764` |
| **767 Cargo** (NEU v0.7.3) | `B762F`, `B763F` |
| 777 | `B772`, `B77L`, `B773`, `B77W`, `B77F` |
| 787 | `B788`, `B789`, `B78X` |

### 3.3 McDonnell Douglas (2 — v0.7.2)

| Familie | ICAO-Codes mit Alias |
|---|---|
| MD-11 | `MD11`, `MD11F` |

### 3.4 Embraer (6)

| Familie | ICAO-Codes mit Alias |
|---|---|
| ERJ | `E170`, `E175`, `E190`, `E195` |
| E2 | `E290`, `E295` |

### 3.5 Test-Coverage (die 22 Faelle DIESER Spec; Stand 16.09.2026)

> Die Tabelle listet die Faelle, die zu dieser Spec gehoeren — nicht den
> gesamten Modulbestand. `aircraft_alias_tests` in `client/src-tauri/src/lib.rs`
> ist seither auf 64 Tests gewachsen (externe QS 16.09.2026: die alte
> Formulierung las sich wie eine Gesamtzahl).

| Test | Was es deckt |
|---|---|
| `a359_matches_a350_900_long_form` | Live-Bug 2026-05-04 (Emirates A359) |
| `b738_matches_737_800` | 737-NG-Pfad |
| `b77w_matches_777_300er` | 777-300ER mit + ohne Leerzeichen |
| `a20n_matches_a320neo` | NEO-Variation NEO/-NEO/" NEO" |
| `b789_matches_787_9` | 787-Pfad |
| `unrelated_types_dont_match` | False-Positive-Schutz B738 ≠ A320 |
| `case_insensitive` | Lowercase-Inputs |
| `strict_equality_still_works_for_unaliased` | DH8D fallback |
| `md11_matches_md_11f_long_form` | Live-Bug 2026-05-10 (MD-11) |
| `md11_does_not_match_unrelated_widebodies` | MD11 ≠ B77W/A359/B748 |
| `b748f_matches_747_8_freighter` | 747-8F + " Freighter" Long-Form (v0.7.3) |
| `b748f_does_not_match_other_widebodies` | B748F ≠ B77W/A388 |
| `b74f_matches_747_400_freighter` | 747-400F (v0.7.3) |
| `b752f_matches_757_200f` | 757-200F + Pax-Bid B752 akzeptiert -200F (v0.7.3) |
| `b763f_matches_767_300f` | 767-300F (v0.7.3) |
| `b762f_matches_767_200f` | 767-200F (v0.7.3) |
| `a332f_matches_a330_200f` | A330-200F (v0.7.3) |
| **`cargo_aliases_match_freighter_long_form`** (v0.7.4) | **Alle 6 Cargo-Aliase matchen "X-XX Freighter" Long-Form** |
| **`cargo_bid_akzeptiert_pax_sim_derselben_baureihe`** (16.09.2026) | **Cargo-Bid + Pax-Sim AKZEPTIERT — ersetzt `cargo_bid_strict_against_pax_sim`, siehe §7.4** |
| **`pax_bid_accepts_cargo_sim_pragmatism`** (v0.7.4) | **Pax-Bid + Cargo-Sim akzeptiert (umgekehrte Richtung okay)** |
| **`a359_does_not_match_a350_1000`** (v0.7.4) | **A359 ≠ A350-1000 (vorher matched faelschlich via "A350"-Substring)** |
| **`variantenbuchstabe_f_oeffnet_nichts`** (16.09.2026) | **Nicht jedes `F` ist ein Frachter: `M20F` (Mooney) oeffnet NICHT gegen `M20` — die §7.4-Oeffnung laeuft nur ueber die benannte Tabelle `frachter_basismuster`** |

---

## 4. Arbeitsliste — wahrscheinlich nuetzliche Aliases

Diese ICAO-Codes haben **keinen Alias** in der Tabelle. Wenn ein Pilot mit einem davon gegen einen Mismatch-Block laeuft, ist der Maintenance-Workflow aus §6 der direkte Pfad. Die Liste ist **Arbeitsliste, nicht Pflichtpaket** — proaktive Erweiterung nur wenn echte VA-Daten zeigen dass die Familie tatsaechlich genutzt wird.

> **Hinweis:** Manche dieser ICAO-Codes (z.B. `B748F`, `B763F`) sind nicht streng ICAO Doc 8643. Sie werden aber in phpVMS-/SimBrief-/VA-Daten praktisch verwendet. Bei Umsetzung: kurzer Kommentar im Code "VA-/SimBrief-Alias" damit klar ist warum es kein offizieller Code ist.

### 4.1 Cargo-Varianten

**HOHE-Prio-Liste in v0.7.3 abgearbeitet** (B74F, B748F, B752F, B762F, B763F, A332F). Verbleibt:

| ICAO-Code | Real-Name | Sim-Adapter (typisch) | Status |
|---|---|---|---|
| `B74S` | 747SP (Klassiker, sehr selten geflogen) | div. | **fehlt** — geringe Prio |
| `B764F` | 767-400F (existiert real nicht) | — | n/a |
| `B788F` | 787-Frachter (existiert nicht real) | — | n/a |
| `A338F` | A330-800F (selten, Lufthansa-Cargo angekuendigt) | (selten) | **fehlt** — bei Bedarf |
| `A33F` / `A330F` | Generischer A330F | div. | **fehlt** — geringe Prio (VAs sollten spezifisch A332F nutzen) |

### 4.2 Regional / Turboprop (MITTLERE Prio — kleinere VAs, Trainings-Bids)

| ICAO-Code | Real-Name | Sim-Adapter | Status |
|---|---|---|---|
| `AT72` | ATR 72 | Asobo, FlightSimGroup | **fehlt** |
| `AT76` | ATR 72-600 | gleicher Adapter | **fehlt** |
| `AT43` | ATR 42 | seltener | **fehlt** |
| `DH8D` | Dash 8 Q400 | Aerosoft, Asobo | (strict OK weil Sim oft "DHC8-400" + Pilot bid auch DH8D) |
| `DH8C` | Dash 8 Q300 | seltener | **fehlt** |
| `CRJ7` | CRJ-700 | Aerosoft | **fehlt** |
| `CRJ9` | CRJ-900 | Aerosoft | **fehlt** |
| `CRJX` | CRJ-1000 | (selten) | **fehlt** |

### 4.3 Andere Hersteller (NIEDRIGE Prio — Spezialfaelle)

| ICAO-Code | Real-Name | Sim-Adapter | Status |
|---|---|---|---|
| `MD80` / `MD82` / `MD83` / `MD88` | MD-80-Familie | LeonardoSH | **fehlt** |
| `MD90` | MD-90 | (sehr selten) | **fehlt** |
| `F70` / `F100` | Fokker 70/100 | Just Flight | **fehlt** |
| `BCS3` ist da | A220-300 | — | OK |
| `SU95` | Sukhoi Superjet 100 | SimCol | **fehlt** |
| `C25A` / `C56X` | Citation CJ2/Excel | Asobo | (strict OK weil Sim meistens "CJ2" oder "Citation" — pruefen) |

### 4.4 Generic-Aliases die wir absichtlich nicht haben

- **`A330` ohne Variant** — wird nicht gepflegt, weil phpVMS-Standard nur die spezifischen `A332/A333/A338/A339` kennt. Falls eine VA generisch `A330` als Bid-ICAO benutzt → das ist VA-Datenfehler, nicht AeroACARS-Bug.

---

## 5. Test-Matrix (Empfehlung)

Pro **neuer Aircraft-Familie** sind die zwei harten Regeln aus dem Leitprinzip Pflicht:
- mindestens 1 Match-Test (Long-Form oder Variant)
- mindestens 1 offensichtlicher Mismatch-Test (gegen unverwandte Familie)

Daruebergehend gibt es **Empfehlungen**, je nach Familie sinnvoll oder unnoetig:

| Test-Klasse | Wann sinnvoll | Pflicht? |
|---|---|---|
| **Long-Form-Match** | Jede Familie wo Sim-Title eine Marketing-Form benutzt (`A350-900` statt `A359`) | nein, aber Standard |
| **Cargo-Variant-Match** | Familien mit Frachter-Variante wo Pax-Bid auch Cargo-Sim akzeptieren soll | nein, abhaengig von VA-Praxis |
| ~~Strict-Cargo-Match~~ | **aufgehoben 16.09.2026, siehe §7.4** | — |
| **Unrelated-Mismatch** | Familien die optisch verwechselt werden koennten (MD-11 vs B747, A350 vs B777) | **ja, eine der zwei harten Regeln** |
| **Case-Insensitive** | Lowercase-Inputs (rare in Praxis weil phpVMS uppercase) | nein |
| **Variant-Suffix-Variations** | Familien mit NEO/MAX/ER/F-Suffix die im Title verschieden geschrieben werden | nein, aber empfohlen wenn drei oder mehr Schreibweisen existieren |

### 5.1 Verifikations-Befehl

```sh
cargo test --lib aircraft_alias_tests
```

Aktuell (v0.7.3 + v0.7.4 Pending, plus §7.4 vom 16.09.2026): die 22 in §3.5 gelisteten Faelle decken 17 Familien — alle mit den zwei harten Regeln (Match + Mismatch). Der Modulbestand ist groesser (64 Tests). Mehr Tests sind willkommen, aber kein Release-Blocker.

### 5.2 Empfohlene Coverage-Erweiterung (Backlog, nicht Pflicht)

Wenn beim Pflegen mal Zeit ist, koennen die folgenden Long-Form-Tests nachgereicht werden — sie wuerden zukuenftige Blocker noch besser absichern:

- A330/A340/B747/B757/B767/E-Familien Long-Form-Match (alle haben Aliases, aber keinen expliziten Match-Test)

Das ist eine **Arbeitsliste**, kein Pflichtpaket. Wer einen davon mitnimmt wenn er sowieso die Datei aenderet, freuen wir uns.

---

## 6. Maintenance-Workflow

Wenn ein Pilot mit `aircraft_mismatch` blockiert wird obwohl er das richtige Flugzeug hat:

### 6.1 Schritt-fuer-Schritt

1. **Banner-Text aufschreiben** — enthaelt `bid wants <ICAO> (<reg>), sim has <ICAO_actual> (title "<title>")`
2. **Verifizieren** dass es ein false-positive ist (Pax-ICAO vs Frachter im Sim, Long-Form-Title etc.)
3. **Familie identifizieren** — gehoert das Flugzeug ins selbe Airframe wie die Bid?
4. **Aliases-Tabelle erweitern** in `client/src-tauri/src/lib.rs:408-487`:
   ```rust
   "<ICAO_BID>"    => &["<Long-Form-Substring>", "<ICAO_BID>"],
   "<ICAO_ACTUAL>" => &["<Long-Form-Substring>", "<ICAO_ACTUAL>"],  // bei Cargo separat
   ```
5. **Tests hinzufuegen** in `aircraft_alias_tests` — laut Leitprinzip nur 2 sind Pflicht:
   - **mindestens 1 Match-Test** (Bid-ICAO ↔ erwarteter Sim-Title)
   - **mindestens 1 Mismatch-Test** (gegen unverwandte Familie)
   Mehr Tests sind willkommen aber kein Release-Blocker.
6. **Spec updaten** — Inventur §3 erweitern, Lueckenanalyse §4 reduzieren
7. **Hotfix-Release** (Patch-Version, z.B. v0.7.X+1)

### 6.2 Beispiel-Diff (v0.7.2 MD-11 Hotfix)

```diff
+ // ---- McDonnell Douglas ----
+ "MD11"  => &["MD-11", "MD11"],
+ "MD11F" => &["MD-11F", "MD11F"],

  // ---- Embraer ----

+ #[test]
+ fn md11_matches_md_11f_long_form() {
+     assert!(aircraft_types_match("MD11", "MD-11F"));
+     assert!(aircraft_types_match("MD11", "MD11F"));
+     ...
+ }
```

5 Zeilen Tabelle + 2 Tests + Spec-Update + CHANGELOG-Eintrag. Ende-zu-Ende ~30 Min inkl. Tag/Build/Release.

### 6.3 Anti-Patterns (NICHT machen)

- **Generic-Wildcard-Match wie `"MD11" => &["MD"]`** — wuerde A220 (CSeries hat keinen "MD" aber andere Hersteller schon) oder weitere McDonnell Douglas-Modelle (MD-80, MD-90) mit-matchen. Aliases muessen **spezifisch genug** sein dass nur die echte Familie matched.
- **`acknowledge_aircraft_mismatch` Bypass dauerhaft setzen** — der Modal-Bypass im VFR-Pfad ist absichtlich Pro-Flug, nicht Pro-Pilot. Wer das Aliase-Problem mit "Trotzdem starten" loest, lernt dass die Alias-Tabelle nicht gepflegt wird.
- **Sim-Title-Spezifika hardcoden** — `"TFDi Design MD-11F"` als Alias ist falsch, weil PMDG/Captain-Sim/etc. andere Title-Strings produzieren. Aliase sind die **gemeinsame Familien-Bezeichnung** ("MD-11F"), nicht der Vendor-Praefix.

---

## 7. Daten-Vertrag

### 7.1 Was matched (Garantie)

| Bid-ICAO | Sim-Title (typisch) | Match? |
|---|---|---|
| `A359` | "Airbus A350-900 [Asobo]" | ✓ via Long-Form-Alias |
| `A359` | "A350-900 (No Cabin)" | ✓ via Long-Form-Alias |
| `B738` | "PMDG 737-800" | ✓ via Long-Form-Alias |
| `B77W` | "PMDG 777-300ER" | ✓ via Long-Form-Alias |
| `A20N` | "A320NEO Asobo" | ✓ via NEO-Variant-Alias |
| `MD11` | "TFDi Design MD-11F PW4462 (Low Poly Cabin)" | ✓ via §3.3 (v0.7.2+) |
| `MD11F` | "TFDi Design MD-11F PW4462 (Low Poly Cabin)" | ✓ via §3.3 (v0.7.2+) |
| `B77F` | "PMDG 777-200F" | ✓ via §3.2 |
| `<ICAO>` | "<Long Form including ICAO>" | ✓ via title_mentions_icao Fallback |

### 7.2 Was NICHT matched (Garantie)

| Bid-ICAO | Sim-Title | Match? |
|---|---|---|
| `B738` | "Airbus A320 NEO" | ✗ unrelated families |
| `MD11` | "Boeing 747-8" | ✗ unrelated widebodies |
| `A359` | "Boeing 777-300ER" | ✗ unrelated widebodies |
| `B788` | "Asobo 787-9" | ✗ verschiedene Variant (B788 ≠ B789) |

### 7.3 Cargo-Pragmatismus

**Pax-Bid + Cargo-Sim** (z.B. `MD11` Bid + `MD-11F` Sim-Title): wir akzeptieren das. Begruendung: Cargo-Variante hat groesseren Frachtraum, kann aber problemlos eine Pax-Strecke fliegen. Wenn eine VA das fuer ihre Dispatch-Disziplin nicht will, ist das eine **VA-Daten-Entscheidung** (in phpVMS strikt `MD11F` als Bid-ICAO setzen).

**Cargo-Bid + Pax-Sim** (z.B. `MD11F` Bid + `MD-11` Sim-Title — JustFlight Pax-Variante): war bis 16.09.2026 **strict** geblockt (Begruendung damals: Pax-Compartment hat keine Cargo-Lasten-Verteilung). **Aufgehoben — siehe §7.4.**


### 7.4 Strict-Cargo aufgehoben (16.09.2026)

Die Gegenrichtung — **Cargo-Bid + Pax-Sim** — galt bis hierher als Mismatch
(Compartment-Unterschied). Das ist aufgehoben. Grund: AeroACARS kann einen
Frachter gar nicht verlaesslich erkennen. Es sieht nur ein `F` am Ende der
Kennung oder das Wort „Freighter" im Titel. Ein Add-on, das seine MD-11F
schlicht als `MD-11` meldet — verbreitet —, sah dadurch aus wie ein
Passagierflugzeug.

Die Regel wirkte ausserdem nur je nach Schreibweise: `MD11F` gegen `MD-11`
blockierte, gegen `MD11` nicht (Teilstring-Treffer der Aliastabelle, externe
QS 16.09.2026). Sie bestrafte also die Schreibweise des Herstellers, nicht das
falsche Flugzeug — und ein Mismatch kostet den Piloten den automatischen Start.

**Jetzt:** Eine **benannte Tabelle** `frachter_basismuster()` (lib.rs) bildet
jedes Frachter-Muster auf das Passagier-Muster derselben Baureihe ab — `A332F`
→ `A332`, `B74F` → `B744`, `B748F` → `B748`, `B752F` → `B752`, `B762F` →
`B762`, `B763F` → `B763`, `B77F` → `B77L`, `MD11F` → `MD11`. Der Vergleich
laeuft danach einmal mit der Basis erneut durch die volle Aliasmaschinerie, so
dass `B74F` gegen `747-400` genauso trifft wie `MD11F` gegen `MD-11`.
**Bewusst KEINE Zeichenketten-Regel** ueber ein abgeschnittenes `F`: bei der
Mooney `M20F` ist das `F` ein Buchstabe der Baureihe (externe QS 16.09.2026,
Test `variantenbuchstabe_f_oeffnet_nichts`). Wer Fracht fliegt, nimmt den
Frachter — Dispatch-Disziplin, keine Aufgabe dieses Vergleichs
(Entscheidung Thomas K.).
Gewichte und Zuladung haengen an SimBrief, die Bewertungsgrenzen am
Grundmuster. Der Rest der Zuordnung bleibt streng: A320 gegen B738 faellt
weiterhin durch. Test: `cargo_bid_akzeptiert_pax_sim_derselben_baureihe`
(ersetzt `cargo_bid_strict_against_pax_sim`).

### 7.5 Bekannte Edge-Cases (Stand v0.7.4 Pending)

| Bid-ICAO | Sim-Title | Match? |
|---|---|---|
| `MD11` | "DC-10-30" | ✗ — DC-10 ist Vorlauefer, nicht MD-11 |
| `B748` | "Boeing 747-8 Freighter (PMDG)" | ✓ via Long-Form "747-8" Substring — Pax-Bid akzeptiert Frachter, gewuenscht (Cargo-Pragmatismus §7.3) |
| `B748F` | "Boeing 747-8" (Pax-Sim) | ✓ seit 16.09.2026 (§7.4, vorher strict geblockt) — Test: `cargo_bid_akzeptiert_pax_sim_derselben_baureihe` |
| `A359` | "Airbus A350-1000" | ✗ strict geblockt seit v0.7.4 (Polish) — A359-Alias wurde von ["A350-900", "A350"] auf nur ["A350-900"] eingeengt. Test: `a359_does_not_match_a350_1000` |

---

## 8. Backward-Compat + Migrations-Strategie

- **Aliases sind additiv.** Eintraege werden nur hinzugefuegt, nie entfernt — sonst wuerden bestehende Pilot-PIREPs ploetzlich blockiert werden.
- **Test-Coverage retro-aktiv.** Wenn ein neuer Test einen unrelated-mismatch zeigt, der vorher als false-positive durchging, dokumentieren wir das in §7.3 als "edge case" statt zu blocken.
- **VA-spezifische Aliases**: nicht in dieser Tabelle. Wenn z.B. eine VA "MD11" bewusst nur als reine Pax-Variante zaehlen will, ist das ein **VA-Daten-Job** (in phpVMS `aircraft.icao = MD11F` setzen wenn Cargo). AeroACARS macht keine VA-spezifische Logik.

### 8.1 Wann Aliases REMOVEN okay waere

- Wenn ein bestehender Alias **nachweislich** false-positives produziert (z.B. weil zwei Hersteller den gleichen Long-Form-String benutzen). Dann: Alias durch praeziseren ersetzen + Test der den Edge-Case fixt.
- Beispiel: angenommen ein hypothetischer Sukhoi-SU95-Title "Superjet-100" wuerde von einem A350-Alias "100" gematched werden. Dann waere "100" als alias zu generisch — Pruefung: `aircraft_aliases("A35K") = ["A350-1000"]` ist spezifisch genug, kein Konflikt.

---

## 9. Anhang A — Code-Anker

| Datei | Zeile | Inhalt |
|---|---|---|
| `client/src-tauri/src/lib.rs` | 380-390 | `title_mentions_icao` |
| `client/src-tauri/src/lib.rs` | 408-497 | `aircraft_aliases` Tabelle |
| `client/src-tauri/src/lib.rs` | 494-508 | `aircraft_types_match` |
| `client/src-tauri/src/lib.rs` | 5209-5234 | Match-Aufruf + Block-Logik in `flight_start` (IFR) |
| `client/src-tauri/src/lib.rs` | 5953-5970 | Match-Aufruf + Warning-Logik in `flight_start_manual` (VFR) |
| `client/src-tauri/src/lib.rs` | 17078-17158 | `aircraft_alias_tests` Modul |

---

## 10. Glossar

- **ICAO-Code (4-letter):** Eindeutiger Aircraft-Type-Code laut ICAO Doc 8643. Beispiele: `A359`, `B738`, `MD11`, `MD11F`. **Source-of-Truth fuer phpVMS-Bid-Definitionen.**
- **Sim-Title:** Volltext-String den der Simulator in der `TITLE`-SimVar liefert. Beispiele: "Airbus A350-900 [Asobo]", "TFDi Design MD-11F PW4462". Vendor-spezifisch.
- **Long-Form-Alias:** Substring-Pattern das im Sim-Title vorkommt wenn das richtige Flugzeug geladen ist. Beispiel: `"A350-900"` ist Alias fuer `A359`.
- **Cargo-Variant:** F-Suffix (z.B. `MD11F`, `B77F`). Eigene ICAO weil Cargo-Compartment + andere MTOW.
- **Pax-Variant:** Standard-ICAO ohne F-Suffix. Pax-Bid akzeptiert einen Cargo-Sim (gleiche Familie). Seit 16.09.2026 gilt auch die Gegenrichtung: Cargo-Bid akzeptiert den Pax-Sim derselben Baureihe (§7.4) — AeroACARS kann einen Frachter nicht verlaesslich erkennen.

---

**Ende der Spec v1.2 — Leitplanke statt Regelwerk. Drei harte Regeln aus dem Leitprinzip sind Pflicht, alles andere Empfehlung. v0.7.3 hat die HOHE-Prio-Cargo-Familien (B74F/B748F/B752F/B762F/B763F/A332F) eingebaut. v0.7.4 Pending-Polish: " FREIGHTER" Long-Form fuer alle Cargo-Aliase + Cargo-Bid-vs-Pax-Sim Strict-Tests + A359-Alias narrowed (kein A350-1000-False-Positive mehr). **Nachtrag 16.09.2026:** die Strict-Tests sind aufgehoben — Cargo-Bid akzeptiert den Pax-Sim derselben Baureihe (§7.4, Test `cargo_bid_akzeptiert_pax_sim_derselben_baureihe`).**
