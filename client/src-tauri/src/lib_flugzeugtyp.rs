/// Loose check: does the aircraft title from MSFS appear to mention the given
/// ICAO code? Used as a permissive backup when ATC MODEL parses to one code
/// but the title says something completely different.
///
/// "Mention" includes the ICAO code AND any of its known long-form
/// aliases — e.g. for `icao = "A359"` the title "Airbus A350-900"
/// counts as a match. Without this, a pilot on a real GSG flight
/// (Emirates UAE770 EK770, A359 bid, A350-900 sim) got blocked
/// with "Aircraft mismatch" even though it's the same aircraft.
// v0.8.3: `title_mentions_icao(&str, &str)` (Test-Wrapper ohne VPS-
// Override) entfernt — wurde von keinem Test mehr aufgerufen seit die
// `_with_extra`-Variante alleinige Single-Source-of-Truth ist. Wer die
// Default-Variante braucht, ruft `_with_extra(title, icao, &HashMap::new())`.

/// v0.8.0: title-substring-Check mit VPS-Alias-Override. Long-form
/// strings aus dem Sim ("mg hjet ha420 [Preset Default]") werden
/// gegen die hardcoded + VPS-Alias-Liste auf substring-Match geprüft.
fn title_mentions_icao_with_extra(
    title: &str,
    icao: &str,
    vps_aliases: &HashMap<String, Vec<String>>,
) -> bool {
    let title_upper = title.to_uppercase();
    let icao_upper = icao.to_uppercase();
    if title_upper.contains(&icao_upper) {
        return true;
    }
    if aircraft_aliases(&icao_upper)
        .iter()
        .any(|alias| title_upper.contains(alias))
    {
        return true;
    }
    if let Some(extra) = vps_aliases.get(&icao_upper) {
        if extra
            .iter()
            .any(|alias| title_upper.contains(alias.as_str()))
        {
            return true;
        }
    }
    false
}

/// Bidirectional alias table for aircraft type identification.
/// phpVMS stores `aircraft.icao` as the ICAO 4-letter type code
/// (`A359`, `B738`, `B77W`), but MSFS's `ATC MODEL` and `TITLE`
/// SimVars often expose the marketing/long form (`A350-900`,
/// `737-800`, `777-300ER`). Both sides refer to the same airframe;
/// our match logic must accept either.
///
/// Returns the list of long-form aliases for the given ICAO code,
/// PLUS the inverse — if `icao` looks like a marketing form, the
/// aliases include the corresponding ICAO code so a sim that
/// reports the ICAO can still match a long-form title and vice
/// versa. Empty list = no known aliases (fall through to
/// strict-string comparison).
///
/// Live bug 2026-05-04: Emirates A359 bid blocked because sim
/// loaded "A350-900 (No Cabin)" — same aircraft, different name.
fn aircraft_aliases(code: &str) -> &'static [&'static str] {
    // Match against the uppercased input. Long-form aliases are
    // partial substrings (we only need ONE to be present in the
    // sim title) — so "A350-900" alone catches "Airbus A350-900",
    // "A350-900 No Cabin", "Asobo A350-900", etc.
    match code {
        // ---- Airbus ----
        // A220 family
        "BCS1" => &["A220-100", "CS100"],
        "BCS3" => &["A220-300", "CS300"],
        // A320 family
        "A318" => &["A318"],
        "A319" => &["A319"],
        "A20N" => &["A320NEO", "A320-NEO", "A320 NEO"],
        "A320" => &["A320"],
        "A21N" => &["A321NEO", "A321-NEO", "A321 NEO"],
        "A321" => &["A321"],
        // A330 family
        "A332" => &["A330-200"],
        "A333" => &["A330-300"],
        "A338" => &["A330-800"],
        "A339" => &["A330-900"],
        // A330-Frachter — VA-/SimBrief-Alias. Qatar Cargo / Turkish
        // Cargo / Etihad Cargo flogen A330-200F.
        "A332F" => &["A330-200F", "A330-200 FREIGHTER", "A332F"],
        // A340 family
        "A342" => &["A340-200"],
        "A343" => &["A340-300"],
        "A345" => &["A340-500"],
        "A346" => &["A340-600"],
        // A350 family — the bug
        // v0.7.4 (Round-1 Review): "A350"-Substring-Alias entfernt
        // weil A359-Bid faelschlich auch A350-1000 matchte. Spec
        // §3.3 "Anti-Patterns": Aliase muessen spezifisch genug sein
        // dass nur die echte Familie matched. "A350-900" reicht weil
        // alle bekannten Sim-Adapter (Asobo/iniBuilds/etc.) den
        // Variant-Suffix immer mitliefern.
        "A359" => &["A350-900"],
        "A35K" => &["A350-1000"],
        // A380
        "A388" => &["A380-800", "A380"],

        // ---- Boeing ----
        // 717
        "B712" => &["717-200", "717"],
        // 737 NG family
        "B736" => &["737-600"],
        "B737" => &["737-700"],
        "B738" => &["737-800"],
        "B739" => &["737-900"],
        // 737 MAX
        "B37M" => &["737 MAX 7", "737-7", "737MAX-7"],
        "B38M" => &["737 MAX 8", "737-8", "737MAX-8"],
        "B39M" => &["737 MAX 9", "737-9", "737MAX-9"],
        "B3XM" => &["737 MAX 10", "737-10"],
        // 747
        "B741" => &["747-100"],
        "B742" => &["747-200"],
        "B744" => &["747-400"],
        "B748" => &["747-8"],
        // 747-Frachter — VA-/SimBrief-Alias (nicht streng ICAO Doc 8643).
        // Spec docs/spec/aircraft-type-match.md §4 + §7.3/§7.4: beide
        // Richtungen erlaubt (Strict-Cargo aufgehoben 16.09.2026 — die
        // Oeffnung laeuft ueber `frachter_basismuster`); B744 (Pax-Bid)
        // akzeptiert via
        // Long-Form-Substring "747-400" auch -400F-Frachter (Cargo-
        // Pragmatismus). Lufthansa Cargo / Atlas / Cargolux flogen
        // historisch B744F + heute B748F.
        // v0.7.4: " FREIGHTER" Long-Form ergaenzt (Round-1 Review)
        // weil manche Sim-Addons den Title als "747-400 Freighter"
        // ohne den F-Suffix-Kompakt schreiben.
        "B74F" => &["747-400F", "747-400 FREIGHTER", "B74F"],
        "B748F" => &["747-8F", "747-8 FREIGHTER", "B748F"],
        // 757
        "B752" => &["757-200"],
        "B753" => &["757-300"],
        // 757-Frachter — VA-/SimBrief-Alias. DHL/UPS/FedEx-Klassiker.
        "B752F" => &["757-200F", "757-200 FREIGHTER", "B752F"],
        // 767
        "B762" => &["767-200"],
        "B763" => &["767-300"],
        "B764" => &["767-400"],
        // 767-Frachter — VA-/SimBrief-Alias. FedEx/UPS-Klassiker.
        "B762F" => &["767-200F", "767-200 FREIGHTER", "B762F"],
        "B763F" => &["767-300F", "767-300 FREIGHTER", "B763F"],
        // 777
        "B772" => &["777-200"],
        "B77L" => &["777-200LR", "777-200 LR"],
        "B773" => &["777-300"],
        "B77W" => &["777-300ER", "777-300 ER"],
        "B77F" => &["777F", "777-200F"],
        // 787
        "B788" => &["787-8"],
        "B789" => &["787-9"],
        "B78X" => &["787-10"],

        // ---- McDonnell Douglas ----
        // MD-11 family (Pax + Frachter — Live-Bug 2026-05-10:
        // Martinair Cargo MPH62 Bid mit ICAO "MD11" wurde geblockt
        // weil TFDi Design im Sim "MD-11F" meldet. Cargo-Bids fliegen
        // legitim auch die F-Variante; "MD11" alias matched daher
        // sowohl "MD-11" als auch "MD11F"-Substring im Title).
        "MD11" => &["MD-11", "MD11"],
        "MD11F" => &["MD-11F", "MD11F"],

        // ---- Embraer ----
        "E170" => &["170"],
        "E175" => &["175"],
        "E190" => &["190"],
        "E195" => &["195"],
        "E290" => &["E190-E2", "E2-190"],
        "E295" => &["E195-E2", "E2-195"],

        // ---- Honda Aircraft Company ----
        // HondaJet HA-420 — Live-Bug 2026-05-13 (GSG / Michel D., MSFS):
        // Bid hat ICAO "HDJT" (offizieller Doc-8643 Type-Designator),
        // MSFS ATC-MODEL liefert aber "HA420" (Hersteller-Marketing-
        // Designation) und der TITLE ist typisch "mg hjet ha420
        // [Preset Default]". Ohne Alias schlägt der Pre-Flight-Check
        // mit "Aircraft mismatch: bid wants HDJT, sim has HA420" fehl.
        // Bidirektional eingetragen damit beide Vergleichsrichtungen
        // (bid-vs-sim und sim-vs-bid) matchen.
        "HDJT" => &["HA420", "HA-420", "HONDAJET", "HJET"],
        "HA420" => &["HDJT", "HA-420", "HONDAJET", "HJET"],

        // No alias known — fall through to strict comparison.
        _ => &[],
    }
}

/// Symmetric: do these two aircraft type strings refer to the
/// same airframe? Tries both directions: are `actual`'s long-form
/// aliases mentioned by `expected`? And are `expected`'s aliases
/// mentioned by `actual`? Plus strict equality fallback.
/// Test-/Compatibility-Wrapper ohne VPS-Override.
#[cfg(test)]
fn aircraft_types_match(expected: &str, actual: &str) -> bool {
    aircraft_types_match_with_extra(expected, actual, &HashMap::new())
}

/// v0.8.0: alias-aware comparison with VPS-managed override-table
/// merged on top of the hardcoded `aircraft_aliases()`. The
/// `vps_aliases` map is the result of fetching
/// `/api/aircraft-aliases` from the recorder; key = bid-ICAO
/// uppercase, value = list of additional alias substrings the sim
/// might report. VPS aliases are ADDITIVE — a hardcoded mismatch
/// (e.g. an A330 family) is never relaxed by an empty VPS table.

/// Frachter-Muster → das Passagier-Muster derselben Baureihe.
///
/// Bewusst eine LISTE und keine Regel ueber die Zeichenkette: `MD11F` ist
/// ein Frachter, `M20F` eine Mooney-Baureihe. Eintraege stammen aus der
/// Aliastabelle oben (dort stehen die Frachter mit eigenem Schluessel).
fn frachter_basismuster(code: &str) -> Option<&'static str> {
    match code {
        "A332F" => Some("A332"),
        "B74F" => Some("B744"),
        "B748F" => Some("B748"),
        "B752F" => Some("B752"),
        "B762F" => Some("B762"),
        "B763F" => Some("B763"),
        "B77F" => Some("B77L"),
        "MD11F" => Some("MD11"),
        _ => None,
    }
}

fn aircraft_types_match_with_extra(
    expected: &str,
    actual: &str,
    vps_aliases: &HashMap<String, Vec<String>>,
) -> bool {
    let exp = expected.to_uppercase();
    let act = actual.to_uppercase();
    if exp == act {
        return true;
    }
    // Externe QS (Codex, 16.09.2026) P1: seit fuenfstellige Sim-Kennungen
    // die Normalisierung ueberstehen (`414AW` statt frueher „nichts"),
    // treffen sie hier auf den gebuchten Code (`C414`). Ohne die
    // Schreibweisen-Kaskade — dieselbe, die Grenzen und Spurweite schon
    // benutzen — meldete der Abgleich Mismatch und der Auto-Start blieb
    // aus. Beide Seiten werden jetzt durch die Kaskade gezogen.
    //
    // Die Kaskade streicht dabei auch ein angehaengtes `F` — das ist hier
    // ZU GROB: bei der Mooney `M20F` ist das `F` ein Buchstabe der
    // Baureihe, kein Frachtermerkmal (externe QS, Codex 16.09.2026).
    // Die Frachter-Oeffnung steht deshalb weiter unten als BENANNTE
    // Liste, nicht als Zeichenketten-Regel.
    let ist_f_kennung = |c: &str| {
        let ohne: String = c.chars().filter(|z| *z != '-' && *z != ' ').collect();
        (4..=6).contains(&ohne.chars().count()) && ohne.ends_with('F')
    };
    if ist_f_kennung(&exp) == ist_f_kennung(&act) {
        let exp_kandidaten = muster_kandidaten(&exp);
        let act_kandidaten = muster_kandidaten(&act);
        if exp_kandidaten
            .iter()
            .any(|e| act_kandidaten.iter().any(|a| a == e))
        {
            return true;
        }
    }

    // Frachter-Buchung + Passagier-Muster im Simulator: erlaubt, seit
    // 16.09.2026 (Entscheidung Thomas K., Spec §7.4). AeroACARS kann einen
    // Frachter nicht verlaesslich erkennen — ein Add-on, das seine MD-11F
    // als `MD-11` meldet, kostete den Piloten sonst den Auto-Start fuer
    // eine Schreibweise des Herstellers. Die Oeffnung laeuft ueber die
    // BENANNTEN Frachter-Muster (nicht ueber ein abgeschnittenes `F`),
    // damit sie fuer jede Schreibweise gleich wirkt — `B74F` gegen
    // `747-400` genauso wie `MD11F` gegen `MD-11` — und Muster wie `M20F`
    // nicht beruehrt. Der Aufruf ist rekursiv, aber garantiert EINSTUFIG:
    // keine Basis dieser Tabelle steht selbst als Schluessel darin.
    if let Some(basis) = frachter_basismuster(&exp) {
        if aircraft_types_match_with_extra(basis, &act, vps_aliases) {
            return true;
        }
    }
    // Either side might be the short ICAO form, the other the
    // long marketing form. Check both directions against the
    // hardcoded table first (= offline-safe baseline).
    if aircraft_aliases(&exp)
        .iter()
        .any(|alias| act.contains(alias))
        || aircraft_aliases(&act)
            .iter()
            .any(|alias| exp.contains(alias))
    {
        return true;
    }
    // VPS-pflegbare Overrides — additive zur hardcoded Tabelle.
    if let Some(extra) = vps_aliases.get(&exp) {
        if extra.iter().any(|alias| act.contains(alias.as_str())) {
            return true;
        }
    }
    if let Some(extra) = vps_aliases.get(&act) {
        if extra.iter().any(|alias| exp.contains(alias.as_str())) {
            return true;
        }
    }
    false
}

/// v0.13.16: zentraler, alias-bewusster Aircraft-Typ-Vergleich. Genutzt
/// sowohl vom `flight_start`/`flight_start_manual`-Mismatch-Gate ALS AUCH
/// vom Auto-Start-Watcher (Option C), damit beide identisch entscheiden —
/// inklusive VPS-Aliase und Title-Substring-Fallback.
///
/// **Permissiv, wenn eine Seite unbekannt ist** (analog zum Gate, das nur
/// bei beidseitig bekanntem Typ blockt): kann der erwartete oder der Sim-
/// Typ nicht aufgelöst werden ⇒ kein Block (`true`). So verhindert ein
/// fehlgeschlagener `get_aircraft` im Watcher-Vorabcheck nie einen
/// legitimen Start.
fn aircraft_type_matches_sim(
    expected_icao: Option<&str>,
    sim_icao: Option<&str>,
    sim_title: &str,
    vps_aliases: &HashMap<String, Vec<String>>,
) -> bool {
    match (expected_icao, sim_icao) {
        (Some(expected), Some(actual)) => {
            aircraft_types_match_with_extra(expected, actual, vps_aliases)
                || title_mentions_icao_with_extra(sim_title, expected, vps_aliases)
        }
        _ => true,
    }
}
