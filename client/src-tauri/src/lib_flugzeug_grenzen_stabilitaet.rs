/// v0.5.26: ICAO-Kategorie-basierte Default-Limits fuer Wing-Strike +
/// Vref. Wenn DBasic Tech-Limits-DB nicht verfuegbar oder PMDG-FMC-
/// Vref nicht gesetzt, nutzen wir diese Conservative-Defaults.
///
/// Quellen: aircraft type certification data sheets (TCDS), FCOMs.
/// Werte sind „typisch" fuer Standard-MTOW-Konfiguration. Pilot/VA
/// kann via DBasic-Override fuer spezifische Variante setzen.
struct AircraftLimits {
    max_bank_landing_deg: f32,
    /// Nur noch Anker fuer die Klappenraster-Korrektur (`KonfigKontext`).
    /// Fuer die Vref-Abweichung der Landung gilt er seit 2.0.6 NICHT mehr —
    /// ein Wert je Muster ohne Gewicht zeigte 94 % falsche Abweichungen
    /// (siehe `landing_scoring::vref`).
    typical_vref_kt: Option<f32>,
    /// Kam dieser Satz aus dem Rueckfall statt aus der Tabelle?
    ///
    /// Die Tabelle ist eine Positivliste. Fehlt ein Muster, rechnete die
    /// Wing-Strike-Bewertung gegen generische 8° und die Vref-Abweichung
    /// blieb leer — LAUTLOS. Gemessen am 16.08.2026 gegen die gebuchte
    /// GSG-Flotte (`phpvmspireps` JOIN `phpvmsaircraft`, 84 Muster ueber
    /// 3794 PIREPs): 31 Muster ohne Eintrag, zusammen 496 Fluege (13 %).
    /// Aufgefallen ist es erst nach Wochen, weil eine leere Kennzahl
    /// aussieht wie "war halt nicht messbar".
    ///
    /// Der Code hat das schon dreimal nachgetragen (v0.8.3: King Air,
    /// Baron/Seneca, Bonanza) und die Klasse nie geschlossen. Deshalb
    /// wandert die Information jetzt mit, statt im Nichts zu enden.
    is_fallback: bool,
}

/// Weitere Schreibweisen, unter denen dasselbe Muster in der Tabelle stehen
/// koennte. Laeuft NUR, wenn weder der Rohwert noch [`normalize_icao_type`]
/// getroffen haben.
///
/// Die Marketingnamen ("PHENOM 300E", "A350-900", Baureihen ohne Variante)
/// pflegt `sim-core::map_model_name_to_icao` — eine Liste, ein Korpus, ein
/// Test. Hier stehen nur die Formregeln, die sich nicht dorthin verschieben
/// lassen: sie zerlegen einen Code auf Verdacht, und ob das Ergebnis Sinn
/// ergibt, weiss allein die Tabelle, gegen die hier probiert wird.
/// `normalize_icao_type` muesste dieselbe Zerlegung blind zurueckgeben und
/// wuerde damit echte Designatoren beschaedigen — aus "B76F" (Tabellen-
/// eintrag) wuerde "B76".
///
/// Zweite, unabhaengige Messung am 16.08.2026 — diesmal ueber die 827
/// aufgezeichneten Recorder-Logs, also ueber das, was der SIMULATOR meldet
/// (die Zahlen an `AircraftLimits::is_fallback` zaehlen dagegen die
/// gebuchten Muster): 130 der 827 Fluege (16 %) kamen mit einer
/// Schreibweise an, die die Tabelle nicht kennt.
fn muster_kandidaten(upper: &str) -> Vec<String> {
    let mut aus = Vec::new();
    let mut schieben = |k: String| {
        if !k.is_empty() && !aus.contains(&k) {
            aus.push(k);
        }
    };

    // Bewusst ueber `chars`, nicht ueber Byte-Indizes: hier kommt an, was der
    // Simulator als Mustername meldet, und das ist beliebiger Text. Ein
    // `[..4]` auf einem Mehrbyte-Zeichen wuerde die Bewertung mitten im
    // Anflug in eine Panik reissen.
    let zeichen: Vec<char> = upper.chars().filter(|c| *c != '-' && *c != ' ').collect();
    let ohne_trenner: String = zeichen.iter().collect();
    schieben(ohne_trenner.clone());
    // Frachter-Suffix: MD11F → MD11, E190F → E190.
    if let Some(r) = ohne_trenner.strip_suffix('F') {
        schieben(r.to_string());
    }
    // Varianten-Anhang: C680+ → C680, C182Q → C182.
    if let Some(r) = ohne_trenner.strip_suffix('+') {
        schieben(r.to_string());
    }
    if zeichen.len() == 5 && zeichen[4].is_ascii_alphabetic() {
        schieben(zeichen[..4].iter().collect());
    }
    // Baureihe ohne Variante: der Sim meldet nur die Familie. Zugeordnet
    // wird die Variante, die die Flotte tatsaechlich fliegt. Bewusst hier
    // und nicht in `map_model_name_to_icao`: diese Codes sind schon
    // vierstellig und plausibel, eine Zuordnung dort wuerde die
    // Musteridentitaet des ganzen Clients gegen eine geratene Variante
    // tauschen (Auto-Start-Abgleich, PIREP). Hier bestaetigt die Tabelle
    // das Ergebnis, ein Fehlgriff kostet nichts.
    fn familie_von(code: &str) -> Option<&'static str> {
        match code {
            "A300" => Some("A306"),
            "A330" => Some("A333"),
            "A340" => Some("A343"),
            "A380" => Some("A388"),
            "B747" => Some("B744"),
            "B777" => Some("B77W"),
            "B787" => Some("B789"),
            // 02.10.2026 (Inventur, LJ35): Das Learjet-35A-Addon meldet als
            // ATC-Modell „35A" statt des ICAO-Codes LJ35. Der Code war
            // „brauchbar" (kein Marketing-Name), loeste aber keine Tabelle
            // auf: Vref und Spannweite fehlten bei jedem Flug. Hier, in der
            // Tabellenaufloesung, und NICHT in `map_model_name_to_icao` —
            // dort wuerde es die Musteridentitaet des Clients (Auto-Start,
            // PIREP) aendern; fuer den Buchungsabgleich gibt es den Alias
            // auf dem Server.
            "35A" | "35" => Some("LJ35"),
            _ => None,
        }
    }
    // Externe QS (Codex, 16.09.2026) P1: die Familienregel galt NUR fuer
    // die rohe Kennung. `A380X` (FBW A380X meldet das als ATC-Modell)
    // wurde dadurch zu `A380` verkuerzt — und dort blieb es stehen, weil
    // die Regel den verkuerzten Kandidaten nie mehr sah. Ergebnis: genau
    // der A380-Fehler von UAE 52, nur ueber eine andere Schreibweise.
    // Jetzt laeuft sie ueber JEDEN bisher gefundenen Kandidaten.
    // Fuehrende Werksnummer wie 414AW (Cessna 414) → C414.
    if zeichen.len() >= 3 && zeichen[..3].iter().all(char::is_ascii_digit) {
        let zahl: String = zeichen[..3].iter().collect();
        schieben(format!("C{zahl}"));
    }

    // Familienregel zum Schluss ueber JEDEN gefundenen Kandidaten — nicht
    // nur ueber die rohe Kennung. Externe QS (Codex, 16.09.2026) P1:
    // `A380X` wurde zu `A380` verkuerzt und blieb dort stehen, weil die
    // Regel den verkuerzten Kandidaten nie mehr sah; damit fehlte genau
    // die A388-Aufloesung, um die es bei UAE 52 ging.
    let familien: Vec<String> = aus
        .iter()
        .filter_map(|k| familie_von(k).map(str::to_string))
        .collect();
    for f in familien {
        if !aus.contains(&f) {
            aus.push(f);
        }
    }

    aus
}

fn aircraft_limits_for(icao: &str) -> AircraftLimits {
    let upper = icao.to_uppercase();
    let direkt = aircraft_limits_exakt(upper.as_str());
    if !direkt.is_fallback {
        return direkt;
    }
    // Marketingnamen zuerst — dieselbe Uebersetzung, die auch der
    // Health-Report und die Profil-Aufloesung benutzen.
    if let Some(n) = normalize_icao_type(&upper) {
        let treffer = aircraft_limits_exakt(&n);
        if !treffer.is_fallback {
            return treffer;
        }
    }
    for kandidat in muster_kandidaten(upper.as_str()) {
        let treffer = aircraft_limits_exakt(&kandidat);
        if !treffer.is_fallback {
            return treffer;
        }
    }
    direkt
}

/// Dieselbe Kaskade wie `aircraft_limits_for` (Rohwert → `normalize_icao_type`
/// → `muster_kandidaten`), aber generisch fuer jede andere Typtabellen-Abfrage
/// nutzbar, die `Option<&str>` nimmt und bei einem unbekannten Muster `None`
/// liefert.
///
/// # Warum es das braucht (BCS3317/A306, Vereinheitlichung 09/2026)
///
/// `aircraft_limits_for` bricht die Sim-Schreibweise seit v0.8.x bereits auf
/// echte Tabellen-Treffer herunter — u.a. "A300" (Baureihe ohne Variante,
/// die z.B. das Payware-A300F meldet) auf "A306" ueber `muster_kandidaten`
/// (Zeile ~8339). `bahn_felder()`s Spurweite-/Spannweite-Abfrage
/// (`landing_scoring::spurweite::spurweite_m`/`spannweite_m`) hatte diese
/// Kaskade NICHT — sie schlug den rohen `icao`-Wert direkt in der Typtabelle
/// nach, ohne die bereits vorhandene, korpus-getestete Aufloesung zu nutzen.
/// Ergebnis: BCS3317 (A306, MSFS meldet "A300") bekam "track_width_unknown"
/// obwohl "A306" in der Tabelle steht UND `muster_kandidaten` den Kandidaten
/// schon kennt — nur eben nicht hier befragt wurde. Diese Funktion macht die
/// Kaskade wiederverwendbar statt sie ein zweites Mal zu duplizieren.
fn muster_fuer_typtabelle<T>(
    icao: Option<&str>,
    mut nachschlagen: impl FnMut(Option<&str>) -> Option<T>,
) -> Option<T> {
    let roh = icao?;
    if let Some(v) = nachschlagen(Some(roh)) {
        return Some(v);
    }
    let upper = roh.to_uppercase();
    if let Some(n) = normalize_icao_type(&upper) {
        if let Some(v) = nachschlagen(Some(&n)) {
            return Some(v);
        }
    }
    for kandidat in muster_kandidaten(upper.as_str()) {
        if let Some(v) = nachschlagen(Some(&kandidat)) {
            return Some(v);
        }
    }
    None
}

fn aircraft_limits_exakt(upper_str: &str) -> AircraftLimits {
    match upper_str {
        // Heavy / Wide-Body
        "B741" | "B742" | "B743" | "B744" | "B748" => AircraftLimits {
            max_bank_landing_deg: 9.0,
            typical_vref_kt: Some(160.0),
            is_fallback: false,
        },
        "B772" | "B773" | "B77L" | "B77W" | "B77F" | "B778" | "B779" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(150.0),
            is_fallback: false,
        },
        "B788" | "B789" | "B78X" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(145.0),
            is_fallback: false,
        },
        "A332" | "A333" | "A338" | "A339" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(140.0),
            is_fallback: false,
        },
        "A359" | "A35K" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(140.0),
            is_fallback: false,
        },
        "A388" => AircraftLimits {
            max_bank_landing_deg: 7.0,
            typical_vref_kt: Some(145.0),
            is_fallback: false,
        },
        // Narrow-Body Jets
        "A318" | "A319" | "A320" | "A20N" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(135.0),
            is_fallback: false,
        },
        "A321" | "A21N" => AircraftLimits {
            max_bank_landing_deg: 7.0,
            typical_vref_kt: Some(140.0),
            is_fallback: false,
        }, // A321 tail-strike-prone
        "B736" | "B737" | "B738" | "B739" | "B73X" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(140.0),
            is_fallback: false,
        },
        "B38M" | "B39M" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(140.0),
            is_fallback: false,
        },
        "B752" | "B753" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(140.0),
            is_fallback: false,
        },
        "B762" | "B763" | "B764" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(140.0),
            is_fallback: false,
        },
        // Regional Jets
        "E170" | "E175" | "E190" | "E195" | "E290" | "E295" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(130.0),
            is_fallback: false,
        },
        "CRJ7" | "CRJ9" | "CRJX" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(135.0),
            is_fallback: false,
        },
        // Business Jets
        "CL30" | "CL35" | "CL60" | "CL64" | "CL65" => AircraftLimits {
            max_bank_landing_deg: 6.0,
            typical_vref_kt: Some(115.0),
            is_fallback: false,
        },
        "GLF5" | "GLF6" | "GLEX" | "G280" | "GL5T" | "GL7T" => AircraftLimits {
            max_bank_landing_deg: 7.0,
            typical_vref_kt: Some(120.0),
            is_fallback: false,
        },
        "C25A" | "C25B" | "C25C" | "C525" | "C56X" | "C68A" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(105.0),
            is_fallback: false,
        },
        // Turboprops — Regional/Commuter
        "DH8A" | "DH8B" | "DH8C" | "DH8D" | "AT43" | "AT72" | "AT75" | "AT76" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(110.0),
            is_fallback: false,
        },
        // Beechcraft King Air family (turboprop). v0.8.3: duplicate B190
        // removed, BE9L/BE10/BE20/BE30 hinzugefuegt — vorher fielen alle
        // King-Air-Varianten ausser B190/B350 auf den 8°/None-Fallback.
        "B190" | "B350" | "BE9L" | "BE10" | "BE20" | "BE30" => AircraftLimits {
            max_bank_landing_deg: 10.0,
            typical_vref_kt: Some(95.0),
            is_fallback: false,
        },
        // GA Twins (piston). v0.8.3: BE58 (Baron) + PA34 (Seneca) +
        // AEST (Aerostar) hinzugefuegt — Reports zeigten GSG-Piloten mit
        // Baron/Seneca landeten mit Airliner-Bank-Limits (8° statt 15°).
        // 02.10.2026 (Inventur): BE60 (Duke) nachgetragen — typischer
        // Anflug ~85 kt wie die uebrigen Kolben-Twins. Wert typisch, nicht
        // aus dem POH abgeleitet — vor Verlass Thomas abstimmen.
        "BE58" | "BE60" | "BE76" | "PA34" | "PA44" | "AEST" => AircraftLimits {
            max_bank_landing_deg: 15.0,
            typical_vref_kt: Some(85.0),
            is_fallback: false,
        },
        // GA Singles
        "C172" | "C152" | "C150" | "C162" => AircraftLimits {
            max_bank_landing_deg: 15.0,
            typical_vref_kt: Some(65.0),
            is_fallback: false,
        },
        "C182" | "C206" | "C208" => AircraftLimits {
            max_bank_landing_deg: 12.0,
            typical_vref_kt: Some(75.0),
            is_fallback: false,
        },
        "DA40" | "DA42" | "DA62" => AircraftLimits {
            max_bank_landing_deg: 12.0,
            typical_vref_kt: Some(75.0),
            is_fallback: false,
        },
        "P28A" | "PA28" | "PA32" | "PA46" | "P28R" | "PA24" => AircraftLimits {
            max_bank_landing_deg: 12.0,
            typical_vref_kt: Some(75.0),
            is_fallback: false,
        },
        "SR20" | "SR22" | "SR2T" => AircraftLimits {
            max_bank_landing_deg: 12.0,
            typical_vref_kt: Some(80.0),
            is_fallback: false,
        },
        // Beechcraft Bonanza family (single-engine piston, Svenny's Bird).
        // v0.8.3: zuvor war BE36 nicht aufgelistet → Airliner-Fallback.
        "BE33" | "BE35" | "BE36" => AircraftLimits {
            max_bank_landing_deg: 15.0,
            typical_vref_kt: Some(75.0),
            is_fallback: false,
        },
        // Mooney + andere High-Performance Singles
        "M20P" | "M20T" | "MU2" => AircraftLimits {
            max_bank_landing_deg: 12.0,
            typical_vref_kt: Some(80.0),
            is_fallback: false,
        },

        // ─── Nachgetragen 02.10.2026 (Inventur der Messkanaele) ───────────
        //
        // Die Vref-Abweichung fehlte bei diesen Mustern, weil sie nicht in
        // der Tabelle standen (Sim liefert keine Vref, siehe Inventur B3).
        // Werte TYPISCH (Familienwerte), nicht aus dem jeweiligen POH/FCOM
        // abgeleitet — vor Verlass Thomas abstimmen.
        // Pilatus PC-12: Anflug mit Landeklappen 40°, ~85 KIAS.
        "PC12" => AircraftLimits {
            max_bank_landing_deg: 12.0,
            typical_vref_kt: Some(85.0),
            is_fallback: false,
        },
        // Learjet 35: Vref je nach Gewicht ~120-135 kt.
        "LJ35" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(125.0),
            is_fallback: false,
        },
        // Hawker 800XP (H25B), 04.10.2026 (Inventur: 3/3 Landungen ohne
        // Vref-Abweichung). Belege: Hawker-800XP-Leistungsarbeitsblatt
        // VREF 113 kt (Scribd, Gewicht nicht genannt); NTSB/AAR-16/03:
        // 125 kt bei 22.286 lb, Klappen 45 (Hawker 700A, Ueberladung).
        // Max. Landegewicht 23.350 lb. 115 kt = typisches Landegewicht,
        // Familienwert — vor Verlass Thomas abstimmen.
        "H25B" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(115.0),
            is_fallback: false,
        },

        // ─── Nachgetragen 16.08.2026 nach Messung gegen die echte Flotte ──
        //
        // 31 von 84 gebuchten Mustern fehlten, 496 von 3794 Fluegen (13 %).
        // Vref recherchiert und mit Thomas abgestimmt; Bank-Grenzen aus der
        // jeweiligen Klasse wie im Bestand daruber.
        //
        // Bizjets
        "E55P" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(110.0),
            is_fallback: false,
        }, // Phenom 300: Anflug 109 kt (AOPA/FlyRadius)
        "C750" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(115.0),
            is_fallback: false,
        },
        "C680" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(108.0),
            is_fallback: false,
        },
        "HDJT" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(110.0),
            is_fallback: false,
        },
        "FA50" => AircraftLimits {
            max_bank_landing_deg: 7.0,
            typical_vref_kt: Some(113.0),
            is_fallback: false,
        },
        "SF50" => AircraftLimits {
            max_bank_landing_deg: 12.0,
            typical_vref_kt: Some(88.0),
            is_fallback: false,
        },
        // Grossraum / aeltere Muster
        "MD11" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(155.0),
            is_fallback: false,
        }, // Boeing nennt 155 kt
        "A346" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(145.0),
            is_fallback: false,
        },
        "A343" | "A342" | "A345" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(138.0),
            is_fallback: false,
        },
        "A306" | "A30B" | "A310" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(138.0),
            is_fallback: false,
        },
        "L101" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(138.0),
            is_fallback: false,
        },
        "MD81" | "MD82" | "MD83" | "MD87" | "MD88" | "MD90" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(138.0),
            is_fallback: false,
        },
        "B76F" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(140.0),
            is_fallback: false,
        },
        // Narrow-Body / Regional
        "BCS1" | "BCS3" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(128.0),
            is_fallback: false,
        }, // A220: 125-130
        "RJ85" | "RJ1H" | "B461" | "B462" | "B463" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(122.0),
            is_fallback: false,
        },
        "F28" | "F70" | "F100" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(122.0),
            is_fallback: false,
        },
        // 02.10.2026: E13L (Legacy 600, E135-Familie) nachgetragen.
        "E135" | "E145" | "E13L" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(128.0),
            is_fallback: false,
        },
        // Turboprop / GA
        "P180" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: Some(112.0),
            is_fallback: false,
        },
        "TBM7" | "TBM8" | "TBM9" => AircraftLimits {
            max_bank_landing_deg: 12.0,
            typical_vref_kt: Some(85.0),
            is_fallback: false,
        },
        "AC11" | "AC90" => AircraftLimits {
            max_bank_landing_deg: 12.0,
            typical_vref_kt: Some(78.0),
            is_fallback: false,
        },
        "C414" | "C421" | "C310" | "C340" => AircraftLimits {
            max_bank_landing_deg: 12.0,
            typical_vref_kt: Some(98.0),
            is_fallback: false,
        },
        "BE23" | "BE24" => AircraftLimits {
            max_bank_landing_deg: 15.0,
            typical_vref_kt: Some(70.0),
            is_fallback: false,
        },
        "C185" | "C180" | "C170" => AircraftLimits {
            max_bank_landing_deg: 15.0,
            typical_vref_kt: Some(68.0),
            is_fallback: false,
        },
        "GA8" => AircraftLimits {
            max_bank_landing_deg: 15.0,
            typical_vref_kt: Some(68.0),
            is_fallback: false,
        },

        // BEWUSST OHNE Vref, aber MIT Eintrag: fuer diese Muster gibt es
        // keine belastbare oeffentliche Zahl. Ein geratener Wert in einer
        // Bewertung waere schlimmer als eine leere Kennzahl. Der Eintrag
        // sorgt dafuer, dass sie NICHT als "unbekanntes Muster" gemeldet
        // werden — sie sind bekannt, nur ohne Referenzgeschwindigkeit.
        //
        // Hubschrauber: Wing-Strike wird fuer sie ohnehin uebersprungen
        // (siehe `category.is_rotorcraft()`), und ein Vref ergibt hier
        // keinen Sinn.
        "A109" | "A139" | "H145" | "EC35" | "EC45" | "H125" | "H135" => AircraftLimits {
            max_bank_landing_deg: 20.0,
            typical_vref_kt: None,
            is_fallback: false,
        },
        // Militaer und Sonderfaelle
        "A400" | "EUFI" | "CONC" => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: None,
            is_fallback: false,
        },

        // Fallback fuer unbekannte — ab hier wird gemeldet, nicht geschwiegen.
        _ => AircraftLimits {
            max_bank_landing_deg: 8.0,
            typical_vref_kt: None,
            is_fallback: true,
        },
    }
}

/// v0.5.25 Approach-Stability v2 — kompletter Result-Struct.
#[derive(Debug, Clone, Default)]
pub struct ApproachStabilityV2 {
    pub vs_deviation_fpm: Option<f32>,
    pub max_vs_deviation_below_500_fpm: Option<f32>,
    pub bank_stddev_filtered_deg: Option<f32>,
    /// v0.20.1: V/S-Streuung aus DEMSELBEN Fenster wie `bank_stddev_filtered_deg`.
    /// Der Stabilitaets-Sub-Score bewertet beide Achsen — sie muessen dieselbe
    /// Messung sein, sonst ist die Note aus zwei Wirklichkeiten gemischt.
    pub vs_stddev_filtered_fpm: Option<f32>,
    pub runway_changed_late: bool,
    pub stable_at_gate: Option<bool>,
    pub window_sample_count: u32,
    /// V/S-Jerk: mean |Δvs| sample-to-sample. Sim/Aircraft-agnostic.
    pub vs_jerk_fpm: Option<f32>,
    /// IAS-Stddev im Gate (Speed-Stability).
    pub ias_stddev_kt: Option<f32>,
    /// Mind. ein Sample im Gate hatte V/S < -1000 fpm. `None` wenn das
    /// Gate weniger als 3 Samples hatte (siehe Modul-Kommentar an
    /// `FlightStats::approach_excessive_sink`) — NICHT `false`.
    pub excessive_sink: Option<bool>,
    /// Gear+Flaps am 1000-ft-Sample in Landing-Konfig?
    pub stable_config: Option<bool>,
    /// Score-Version 20: Fahrt-Streuung von 500 ft bis zum Flare.
    pub ias_stddev_unter_500_kt: Option<f32>,
    /// Score-Version 20: Gear+Flaps am 500-ft-Sample.
    pub stable_config_500: Option<bool>,
    /// HAT (statt AGL) als Window-Filter genutzt?
    pub used_hat: bool,
    /// v0.5.26: Stable bei DA (200 ft AGL/HAT) erreicht?
    pub stable_at_da: Option<bool>,
    /// v0.5.26: Anzahl Stall-Warning-Samples im gesamten Approach-Buffer.
    pub stall_warning_count: u32,
    /// Score-Version 24: Wind, Soll-Sinkrate, Sinkgrenze und laengster
    /// Sinklauf im Gate — Kontext fuer die Anzeige, keine Note.
    pub wind: Option<landing_scoring::anflug_wind::AnflugWind>,
}

// Mindestdauer unter der Sinkgrenze, ab der „excessive sink" gilt.
//
// v1.9.16: Bis dahin genuegte EIN Sample. Ein einzelner Ausreisser —
// Boe, Sim-Zittern — machte den Anflug zu „excessive". Jetzt zaehlt die
// Grenze (−1000 fpm bei 3°, mit dem Gleitwinkel skaliert), wenn sie
// mindestens 10 s am Stueck unterschritten wird. Die Grenze selbst
// bleibt hart; nur kurzes Ueberschiessen kostet nichts.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> sink_toleranz_ms (v2.1.24: 10_000)
// Dasselbe am 200-ft-Tor (2.1.6, SWR 255 EBBR 20.10.2026: −1180 fpm und
// 32 kt unter Vref, trotzdem „stabil bei 200 ft“). Unter 200 ft dauert der
// Anflug nur ~11–20 s; 10 s am Stueck waren dort nie zu erreichen, ein
// einzelnes Sample knapp ueber der Grenze riss den Lauf. Die Haelfte des
// Fensters genuegt.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> da_sink_toleranz_ms (v2.1.24: 5_000)
// Fahrt am 200-ft-Tor: so weit unter der typischen Vref des Musters ist
// der Anflug nicht stabil, egal wie gleichmaessig er ist. Die Toleranz
// deckt leichte Gewichte (Vref liegt je nach Masse ±10 kt um den typischen Wert).
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> da_vref_toleranz_kt (v2.1.24: 15.0)

// Score-Version 24: Bezugs-Grundgeschwindigkeit der Sinkgrenze (kt).
//
// Die Grenze −1000 fpm (bei 3°) entspricht bei 145 kt ueber Grund etwa
// dem 1,3-fachen der Soll-Sinkrate des Gleitpfads. Wer schneller ueber
// Grund fliegt — Rueckenwind, schnelle Anflugmuster —, muss physikalisch
// staerker sinken; seine Grenze waechst deshalb mit der Grundgeschwindig-
// keit mit. Darunter bleibt sie bei −1000 fpm: Die Regel wird nur
// LOCKERER, nie strenger (Thomas 16.10.2026 nach JBU1500 MUVR, Korpus
// 1068 Anfluege: 0 Fluege neu mit Abzug, 23 ohne). Die 145 kt sind eine
// Wahl, keine Herleitung: der Median-Jet-Anflug, an dem die alte feste
// Grenze offenbar geeicht war.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> sink_gs_bezug_kt (v2.1.24: 145.0)

/// Sinkgrenze eines Messpunkts in fpm (negativ): −1000 × Gleitwinkel-
/// Faktor, ab 145 kt ueber Grund im Verhaeltnis der Grundgeschwindigkeit
/// tiefer. Unlesbare Grundgeschwindigkeit → die feste Grenze.
fn sinkgrenze_fpm(gs_kt: f32, gs_factor: f64) -> f64 {
    let gs = f64::from(gs_kt);
    let faktor = if gs.is_finite() {
        (gs / crate::grenzwerte::grenzwerte().anflug_kern.sink_gs_bezug_kt.wert).max(1.0)
    } else {
        1.0
    };
    -1000.0 * gs_factor * faktor
}

/// Laengster zusammenhaengender Lauf unter der Sinkgrenze.
#[derive(Clone, Copy)]
struct SinkLauf<'a> {
    start: &'a ApproachBufferSample,
    ende: &'a ApproachBufferSample,
    spitze_fpm: f32,
}

impl SinkLauf<'_> {
    fn dauer_ms(&self) -> i64 {
        (self.ende.at - self.start.at).num_milliseconds()
    }
}

fn laengster_sinklauf<'a>(
    samples: &[&'a ApproachBufferSample],
    gs_factor: f64,
) -> Option<SinkLauf<'a>> {
    let mut bester: Option<SinkLauf<'a>> = None;
    let mut lauf: Option<SinkLauf<'a>> = None;
    let mut vorher: Option<DateTime<Utc>> = None;
    for &s in samples {
        // Eine Luecke im Buffer (Sim-Pause: der Zaehler friert ein) darf
        // keinen Lauf ueberbruecken — zwei Samples vor und nach der Pause
        // sind keine 10 s Sinkflug.
        if vorher.is_some_and(|v| (s.at - v).num_milliseconds() > crate::grenzwerte::grenzwerte().anflug_kern.sink_max_luecke_ms.wert) {
            lauf = None;
        }
        vorher = Some(s.at);
        if f64::from(s.vs_fpm) < sinkgrenze_fpm(s.gs_kt, gs_factor) {
            let l = lauf.get_or_insert(SinkLauf {
                start: s,
                ende: s,
                spitze_fpm: s.vs_fpm,
            });
            l.ende = s;
            l.spitze_fpm = l.spitze_fpm.min(s.vs_fpm);
            if bester.as_ref().is_none_or(|b| l.dauer_ms() > b.dauer_ms()) {
                bester = Some(*l);
            }
        } else {
            lauf = None;
        }
    }
    bester
}

// Groesster Abstand zweier Samples innerhalb eines Laufs (Kadenz ~1 s).
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> sink_max_luecke_ms (v2.1.24: 4000)

/// Landekonfiguration (Fahrwerk + Klappen) an einem Tor-Sample. `alle` =
/// alle Samples im 1000-ft-Gate (fuer die Pruefung, ob der Klappenkanal
/// ueberhaupt lesbar ist). `None` = nicht bewertbar.
fn landekonfiguration(
    gate_entry: &ApproachBufferSample,
    alle: &[&ApproachBufferSample],
    konfig: &KonfigKontext,
) -> Option<bool> {
    let gear_ok = gate_entry.gear_position >= 0.99;
    // v0.12.1 (Stream C LE11): detect an unreadable flaps dataref.
    // Study-level X-Plane add-ons (Hot-Start CL650 etc.) don't drive
    // the standard flap dataref → flaps_position stays 0 the whole
    // approach. If gear is down and the gate-entry speed is in the
    // landing range, flaps-0 is a broken reading, not "pilot forgot
    // flaps" — report the config as not-assessable (None) rather than
    // a false "INCOMPLETE" fail. A genuine flapless approach would be
    // far faster than FLAPS_UNREADABLE_MAX_IAS_KT.
    //
    // v1.6.1 (Befund ITY 4TK, Synaptic A220): die Grenze lag bei
    // < 0.01 und fing damit nur TOTE Datarefs. Ein falsch SKALIERTER
    // Wert (A220-Klappenhebel doppelt normalisiert → 0.2 statt 1.0)
    // rutschte durch und wurde als „Pilot hat die Klappen vergessen"
    // bewertet. Physik-Argument wie oben: Fahrwerk unten und
    // Landegeschwindigkeit mit ≤ 25 % Klappen ist keine plausible
    // Konfiguration, sondern ein kaputter oder fehlskalierter
    // Messwert → nicht bewertbar statt falsch-INCOMPLETE. Fängt
    // die ganze Fehlerklasse für alle künftigen Profile.
    let flaps_all_implausible = alle
        .iter()
        .all(|s| s.flaps_position < crate::grenzwerte::grenzwerte().anflug_kern.flaps_implausible_below.wert);
    let flaps_unreadable =
        flaps_all_implausible && gear_ok && gate_entry.ias_kt < crate::grenzwerte::grenzwerte().anflug_kern.flaps_unreadable_max_ias_kt.wert;

    // v1.6.6: der allgemeine Fall des Obigen. Wenn sich der Klappenwert im
    // GANZEN Flug kein einziges Mal bewegt hat, traegt der Kanal keine
    // Information — egal welchen Wert er anzeigt und egal wie schnell das
    // Flugzeug am Tor war. Der Check darueber faengt dasselbe nur unter
    // 160 kt; im Korpus (827 Fluege) rutschten damit genau die schweren
    // Muster durch, die am Tor legitim schneller sind: B77W, A339, MD11,
    // A346, A20N, DH8D — 15 Fluege, alle mit einem toten Dataref als
    // „Klappen vergessen" gewertet.
    let flaps_tot = konfig.flaps_bewegt == Some(false);

    if !gear_ok {
        // Das Fahrwerk ist eindeutig: oben ist oben. Ob die Klappen
        // messbar sind, aendert daran nichts — dieser Zweig haelt die
        // Bewertung dort streng, wo sie es immer war (bisher implizit,
        // weil der Riegel darunter `gear_ok` verlangte).
        Some(false)
    } else if flaps_tot || flaps_unreadable {
        None
    } else {
        let mut flaps_ok = gate_entry.flaps_position >= crate::grenzwerte::grenzwerte().anflug_kern.flaps_landing_min.wert;
        if !flaps_ok {
            // v1.6.6: fremdes Rastermass. Nicht jedes Muster skaliert seine
            // Klappen auf 1,0 — die Fokker 28 im Sim endet bei 0,667, die
            // Falcon 50 ebenso. Steht der Hebel am Tor schon auf dem
            // Maximum DIESES Fluges und passt die Geschwindigkeit ins
            // stabilisierte Fenster (Vref…Vref+20), dann misst die absolute
            // 0,70-Schwelle das Raster des Sims, nicht den Piloten.
            //
            // Die Geschwindigkeit ist hier der Riegel, nicht Beiwerk: ohne
            // sie wuerde jede Landung mit Anflugklappen als „Landeklappen"
            // durchgehen, weil ein einzelner Flug nie zeigt, welche Stufen
            // es noch gaebe. Genau daran scheitern im Korpus die echten
            // Faelle — DA40 mit T/O-Klappen bei Vref+43, Phenom 300 bei
            // Vref+59, A320 mit CONF 2 bei 215 kt — und sie bleiben
            // korrekt bewertet.
            //
            // Kein Vref (Muster nicht in der Tabelle) → Regel bleibt aus.
            // Der Health-Report meldet solche Muster separat, damit die
            // Tabelle waechst statt die Schwelle aufzuweichen.
            if let (Some(max_im_flug), Some(vref)) =
                (konfig.flaps_max_im_flug, konfig.typical_vref_kt)
            {
                let am_eigenen_maximum = gate_entry.flaps_position >= max_im_flug - 0.01
                    && gate_entry.flaps_position >= crate::grenzwerte::grenzwerte().anflug_kern.flaps_implausible_below.wert;
                if am_eigenen_maximum && gate_entry.ias_kt <= vref + crate::grenzwerte::grenzwerte().anflug_kern.stabilisiert_ueber_vref_kt.wert {
                    flaps_ok = true;
                }
            }
        }
        // `gear_ok` ist hier beweisbar true — der Zweig oben hat den
        // Fall abgefangen.
        Some(flaps_ok)
    }
}

/// v0.5.25: Stable-Approach-Gate-konformes Stability-Maß.
///
/// FAA AC 120-71B / EASA SUPP-32 definieren Stable-Approach-Gate als
/// 1000 ft HAT (Height Above Touchdown). Innerhalb des Gates muessen
/// alle Parameter im Toleranzband sein, sonst go-around.
///
/// Window-Logik:
///   * Wenn `arr_airport_elevation_ft` bekannt: HAT = msl_ft − elevation,
///     Filter auf HAT ≤ 1000 ft (= ueber Mountain-Airports korrekt)
///   * Sonst: Fallback auf AGL ≤ 1000 ft (= AGL fluktuiert ueber Berg-
///     ruecken aber funktioniert für Flachland-Airports)
///
/// Stability-Metrics (alle im Gate-Window):
///   1. V/S-Jerk = mean |vs[i]−vs[i−1]| (PRIMAER, sim/aircraft-agnostic)
///   2. Bank-σ filtered (Vector-Windows ausgenommen, 5s nach RWY-Change)
///   3. IAS-σ (Speed-Stability)
///   4. Excessive-Sink-Flag (≥1 Sample mit V/S < -1000 fpm)
///   5. Stable-Config-Flag (Gear≥99% AND Flaps≥70% am gate-Eintritt)
///
/// Sekundaer (informativ, NICHT score-relevant):
///   6. V/S-Deviation vs 3°-ILS-Profil (target_vs = -gs_kt × 5.31)
///      — falsch fuer GA-Visual-Approach, deshalb nur informativ
// Seit Score-Version 23 rufen nur noch Tests die Fassung ohne spaetes Final.
#[cfg(test)]
fn compute_approach_stability_v2(
    buf: &std::collections::VecDeque<ApproachBufferSample>,
    arr_elevation_ft: Option<f32>,
    td_ts: Option<DateTime<Utc>>,
    glideslope_angle_deg: Option<f64>,
    window: Option<(DateTime<Utc>, chrono::Duration)>,
    konfig: KonfigKontext,
) -> ApproachStabilityV2 {
    compute_approach_stability_v2_mit_final(
        buf,
        arr_elevation_ft,
        td_ts,
        glideslope_angle_deg,
        window,
        konfig,
        None,
    )
}

/// Wie `compute_approach_stability_v2`, dazu Score-Version 23: bei einem
/// erkannten spaeten Final (`spaetes`) zaehlen Querneigungs- und V/S-Streuung
/// erst ab dem Ausrichten auf die Bahn — die Kurven davor sind das
/// Verfahren, keine Unruhe. Dafuer muss das Final rechtzeitig erreicht sein
/// (`SpaetesFinal::rechtzeitig`, 300 ft + Fluegel waagerecht), sonst ist das
/// Gate nicht stabil. `None` = exakt das bisherige Verhalten.
fn compute_approach_stability_v2_mit_final(
    buf: &std::collections::VecDeque<ApproachBufferSample>,
    arr_elevation_ft: Option<f32>,
    td_ts: Option<DateTime<Utc>>,
    // v0.15.17: echter Gleitpfad-Winkel aus den Navdaten (Navigraph). Steile
    // Anflüge (z.B. EGLC 5,5°) haben physikalisch höhere Sink-Raten — ein fixer
    // 3°-Vergleich wertet sie sonst fälschlich als „excessive/unstable" (Live-
    // Report Michael, London City). `None` (Runway nicht aufgelöst / nur
    // OurAirports-Fallback / kein ILS) → Faktor 1,0 → IDENTISCH zum bisherigen
    // 3°-Verhalten (Audit-Invariante: keine Regression ohne Daten).
    glideslope_angle_deg: Option<f64>,
    // v0.16.6: optionales Zeit-Fenster (touchdown_ts, duration) — nur Samples
    // innerhalb `duration` VOR dem Touchdown (und keine danach) werden
    // gewertet. Genutzt vom Sampler-Pfad (Bush-Flüge, FSM erreicht Landing
    // nie): dort kann der Buffer durch das phasen-unabhängige Sampling einen
    // 38-min-Low-Level-Cruise enthalten, der das 1000-ft-Gate sonst mit
    // Manöver-Rauschen vergiftet. `None` = EXAKT das bisherige Verhalten
    // (FSM-Pfad + alle bestehenden Tests — Audit-Invariante).
    window: Option<(DateTime<Utc>, chrono::Duration)>,
    // v1.6.6: Musterwissen fuer die Landekonfiguration (Vref + Klappen-Raster
    // dieses Fluges). `Default::default()` = alles None = bisheriges Verhalten.
    konfig: KonfigKontext,
    spaetes: Option<&spaetes_final::SpaetesFinal>,
) -> ApproachStabilityV2 {
    // Fenster-Filter zuerst, dann identische Auswertung via Rekursion mit
    // `None` — so existiert die eigentliche Mess-Logik nur einmal.
    if let Some((td, dur)) = window {
        let filtered = approach_buffer_window(buf, td, dur);
        return compute_approach_stability_v2_mit_final(
            &filtered,
            arr_elevation_ft,
            td_ts,
            glideslope_angle_deg,
            None,
            konfig,
            spaetes,
        );
    }

    let mut out = ApproachStabilityV2::default();

    // Skalierungsfaktor relativ zum 3°-Standard. tan-basiert (physikalisch
    // korrekt: V/S ∝ tan(Gleitwinkel)). Plausibilitäts-Clamp 2,0–7,5° — alles
    // außerhalb gilt als unzuverlässige Navdaten → Fallback 3° (Faktor 1,0).
    // Bei exakt 3,0° ist der Faktor exakt 1,0 → bit-identisch zur alten Logik.
    let gs_factor: f64 = glideslope_angle_deg
        .filter(|g| (2.0..=7.5).contains(g))
        .map(|g| g.to_radians().tan() / 3.0_f64.to_radians().tan())
        .unwrap_or(1.0);

    // 1) Window-Filter: HAT bevorzugt, AGL als Fallback.
    let use_hat = arr_elevation_ft.is_some();
    out.used_hat = use_hat;
    let height_for = |s: &ApproachBufferSample| -> f32 {
        match arr_elevation_ft {
            Some(elev) => s.msl_ft - elev,
            None => s.agl_ft,
        }
    };
    // v0.5.46: zusaetzlicher Flare-Cutoff (Adrian-Feedback).
    // Pilot-Pull in den letzten 3 sec vor TD ist ein Manoever, kein
    // Stabilitaets-Indikator. Wenn td_ts None (= Berechnung mid-flight
    // bevor TD erreicht), nur Hoehen-Filter.
    let gate_samples: Vec<&ApproachBufferSample> = buf
        .iter()
        .filter(|s| {
            let h = height_for(s);
            let in_height_band = h > 0.0 && h <= 1000.0;
            let pre_flare = match td_ts {
                Some(td) => (td - s.at).num_milliseconds() >= crate::grenzwerte::grenzwerte().anflug_kern.approach_flare_cutoff_ms.wert,
                None => true,
            };
            in_height_band && pre_flare
        })
        .collect();
    out.window_sample_count = gate_samples.len() as u32;
    if gate_samples.len() < 3 {
        return out;
    }

    // 2) RWY-Change-Detection ueber GANZEN Buffer.
    let runway_changes: Vec<DateTime<Utc>> = {
        let mut changes = Vec::new();
        let mut prev_rwy: Option<&str> = None;
        for s in buf.iter() {
            let curr = s.selected_runway.as_deref();
            if let (Some(p), Some(c)) = (prev_rwy, curr) {
                if p != c {
                    changes.push(s.at);
                    if height_for(s) < 1500.0 {
                        out.runway_changed_late = true;
                    }
                }
            }
            if curr.is_some() {
                prev_rwy = curr;
            }
        }
        changes
    };
    let in_vector_window = |t: DateTime<Utc>| -> bool {
        runway_changes
            .iter()
            .any(|&change| (t - change).num_milliseconds().abs() <= 5_000)
    };

    // 3) V/S-Jerk (PRIMAER): mean |Δvs| sample-to-sample.
    //    Sim/Aircraft-agnostic — funktioniert für Jet, Turboprop, GA gleich.
    if gate_samples.len() >= 2 {
        let mut jerk_sum = 0.0_f64;
        for w in gate_samples.windows(2) {
            jerk_sum += (w[1].vs_fpm - w[0].vs_fpm).abs() as f64;
        }
        out.vs_jerk_fpm = Some((jerk_sum / (gate_samples.len() - 1) as f64) as f32);
    }

    // 4) IAS-Stddev (Speed-Stability).
    let n = gate_samples.len() as f64;
    let mean_ias = gate_samples.iter().map(|s| s.ias_kt as f64).sum::<f64>() / n;
    let var_ias = gate_samples
        .iter()
        .map(|s| (s.ias_kt as f64 - mean_ias).powi(2))
        .sum::<f64>()
        / n;
    out.ias_stddev_kt = Some(var_ias.sqrt() as f32);

    // 5) Excessive-Sink-Flag. v0.15.17: Schwelle mit dem Gleitwinkel skaliert —
    // bei 3° bleibt's −1000 fpm, bei 5,5° (z.B. EGLC) ~−1834 fpm, sodass ein
    // korrekt geflogener Steilanflug nicht fälschlich „excessive" ist.
    // Score-Version 24: die Grenze waechst ab 145 kt ueber Grund mit
    // (`sinkgrenze_fpm`) — nur lockerer, nie strenger.
    let sinklauf = laengster_sinklauf(&gate_samples, gs_factor);
    out.excessive_sink = Some(sinklauf.is_some_and(|l| l.dauer_ms() >= crate::grenzwerte::grenzwerte().anflug_kern.sink_toleranz_ms.wert));
    {
        let mut wind = landing_scoring::anflug_wind::wind_zusammenfassen(
            &gate_samples
                .iter()
                .map(|s| (s.headwind_kt, s.crosswind_kt))
                .collect::<Vec<_>>(),
        );
        let gs: Vec<f64> = gate_samples
            .iter()
            .map(|s| f64::from(s.gs_kt))
            .filter(|v| v.is_finite())
            .collect();
        if !gs.is_empty() {
            let gs_mittel = gs.iter().sum::<f64>() / gs.len() as f64;
            wind.gs_mittel_kt = Some(gs_mittel as f32);
            // Dieselbe Rechnung wie `target_vs` (V/S-Deviation weiter unten).
            wind.soll_sinkrate_fpm = Some((-gs_mittel * 5.31 * gs_factor) as f32);
        }
        let grenzen: Vec<f64> = gate_samples
            .iter()
            .map(|s| sinkgrenze_fpm(s.gs_kt, gs_factor))
            .collect();
        wind.sinkgrenze_fpm = Some((grenzen.iter().sum::<f64>() / grenzen.len() as f64) as f32);
        wind.sinklauf = sinklauf.map(|l| landing_scoring::anflug_wind::SinkLaufInfo {
            dauer_s: l.dauer_ms() as f32 / 1000.0,
            von_ft: height_for(l.start),
            bis_ft: height_for(l.ende),
            spitze_fpm: l.spitze_fpm,
        });
        out.wind = Some(wind);
    }

    // 6) Stable-Config: Gear+Flaps am 1000-ft-Sample (= aeltester
    //    Sample im Gate, = der mit hoechster Hoehe).
    if let Some(gate_entry) = gate_samples
        .iter()
        // NaN-safe: a missing/unreadable SimVar height yields NaN, and
        // `partial_cmp` returns None on NaN — `.unwrap()` would PANIC mid-scoring.
        // Treat NaN as Equal (no reorder) instead, like the sort at the IAS path.
        .max_by(|a, b| {
            height_for(a)
                .partial_cmp(&height_for(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        out.stable_config = landekonfiguration(gate_entry, &gate_samples, &konfig);
    }

    // 6b) Score-Version 20 (RYR73): dasselbe am 500-ft-Tor plus die Fahrt-
    //     Streuung darunter — fuer „spaet stabilisiert" (anflug_urteil).
    //     Nur das LETZTE zusammenhaengende Stueck unter 500 ft (Codex-QS):
    //     nach Durchstarten/Touch-and-Go liegen im Puffer noch Punkte des
    //     vorigen Anflugs, getrennt durch den Steigflug ueber 500 ft.
    let mut unter_500: Vec<&ApproachBufferSample> = gate_samples
        .iter()
        .rev()
        .take_while(|s| height_for(s) <= 500.0)
        .copied()
        .collect();
    unter_500.reverse();
    if unter_500.len() >= 3 {
        let n = unter_500.len() as f64;
        let mittel = unter_500.iter().map(|s| s.ias_kt as f64).sum::<f64>() / n;
        let var = unter_500
            .iter()
            .map(|s| (s.ias_kt as f64 - mittel).powi(2))
            .sum::<f64>()
            / n;
        out.ias_stddev_unter_500_kt = Some(var.sqrt() as f32);
        if let Some(tor) = unter_500.iter().max_by(|a, b| {
            height_for(a)
                .partial_cmp(&height_for(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            out.stable_config_500 = landekonfiguration(tor, &gate_samples, &konfig);
        }
    }

    // 7) V/S-Deviation vs 3° ILS-Profil (sekundaer, nur informativ).
    let mut sum_dev = 0.0_f64;
    let mut max_dev_below_500 = 0.0_f32;
    for s in &gate_samples {
        // v0.15.17: Ziel-V/S am ECHTEN Gleitwinkel (gs_factor); bei 3° = ×1,0.
        let target_vs = -(s.gs_kt as f64) * 5.31 * gs_factor;
        let dev = (s.vs_fpm as f64 - target_vs).abs();
        sum_dev += dev;
        // v0.12.1 (Stream C LE12): the "< 500 ft" band excludes the
        // flare / ground-effect regime — a V/S excursion below
        // MAX_VS_DEV_FLOOR_FT is the flare, not approach instability.
        let h = height_for(s);
        if h <= 500.0 && h >= crate::grenzwerte::grenzwerte().anflug_kern.max_vs_dev_floor_ft.wert {
            let dev_f32 = dev as f32;
            if dev_f32 > max_dev_below_500 {
                max_dev_below_500 = dev_f32;
            }
        }
    }
    let mean_dev = sum_dev / gate_samples.len() as f64;
    out.vs_deviation_fpm = Some(mean_dev as f32);
    if max_dev_below_500 > 0.0 {
        out.max_vs_deviation_below_500_fpm = Some(max_dev_below_500);
    }

    // 8) Stddevs filtered — Bank UND V/S aus DEMSELBEN Fenster.
    //
    // v0.20.1: Die V/S-Streuung wurde bisher nur im Legacy-Pfad gerechnet —
    // anderes Fenster (bis 1500 ft AGL statt 1000 ft HAT), ohne Vektor-Filter.
    // Beide Werte flossen aber in DENSELBEN Stabilitaets-Sub-Score. Eine Formel
    // aus zwei unvergleichbaren Messungen: im VPS-Korpus hatten 18 Landungen
    // null Samples im v2-Fenster und trotzdem eine Legacy-V/S-Streuung.
    //
    // Beide Achsen teilen sich jetzt Fenster, Filter und Mindest-Stichprobe.
    // Score-Version 23: beim spaeten Final erst ab dem Ausrichten.
    let vector_filtered: Vec<&ApproachBufferSample> = gate_samples
        .iter()
        .filter(|s| !in_vector_window(s.at))
        .filter(|s| spaetes.is_none_or(|sf| s.at >= sf.ab))
        .copied()
        .collect();
    out.bank_stddev_filtered_deg = stability_stddev(vector_filtered.iter().map(|s| s.bank_deg));
    out.vs_stddev_filtered_fpm = stability_stddev(vector_filtered.iter().map(|s| s.vs_fpm));

    // 9) Composite Stable-At-Gate Indikator.
    //    PRIMARY-Maße: jerk < 100 AND bank_sd < 5 AND ias_sd < 10
    //                AND no_excessive_sink AND stable_config
    let jerk_ok = out.vs_jerk_fpm.map(|j| j < 100.0).unwrap_or(false);
    let bank_ok = out
        .bank_stddev_filtered_deg
        .map(|b| b < 5.0)
        .unwrap_or(true);
    let ias_ok = out.ias_stddev_kt.map(|i| i < 10.0).unwrap_or(true);
    let config_ok = out.stable_config.unwrap_or(true); // None = unbekannt, kein blocker
                                                       // At this point in the function we are always past the `gate_samples.len()
                                                       // < 3` early return, so `out.excessive_sink` is always `Some` (set just
                                                       // above) — `unwrap_or(false)` is a defensive fallback, never actually hit.
    let final_ok = spaetes.is_none_or(|sf| sf.rechtzeitig);
    let stable = jerk_ok
        && bank_ok
        && ias_ok
        && !out.excessive_sink.unwrap_or(false)
        && config_ok
        && final_ok;
    out.stable_at_gate = Some(stable);

    // 10) v0.5.26: Stable-At-DA (200 ft) — gleicher Composite-Check
    //     aber mit den Samples die im 200-ft-Gate lagen (= subset).
    //     2.1.6 (Codex-Review): ohne die letzten 3 s vor dem Aufsetzen, wie
    //     das 1000-ft-Tor — im Abfangen faellt die Fahrt planmaessig, und die
    //     neue Vref-Pruefung soll das Abfangen nicht als „zu langsam“ lesen.
    let da_samples: Vec<&ApproachBufferSample> = buf
        .iter()
        .filter(|s| {
            let h = height_for(s);
            let pre_flare = match td_ts {
                Some(td) => (td - s.at).num_milliseconds() >= crate::grenzwerte::grenzwerte().anflug_kern.approach_flare_cutoff_ms.wert,
                None => true,
            };
            h > 0.0 && h <= 200.0 && pre_flare
        })
        .collect();
    if da_samples.len() >= 3 {
        let n_da = da_samples.len() as f64;
        // V/S-Jerk im 200-ft-Gate
        let mut jerk_sum = 0.0_f64;
        for w in da_samples.windows(2) {
            jerk_sum += (w[1].vs_fpm - w[0].vs_fpm).abs() as f64;
        }
        let da_jerk = jerk_sum / (da_samples.len() - 1) as f64;
        // Bank-σ im 200-ft-Gate (gefiltert)
        let da_bank: Vec<f32> = da_samples
            .iter()
            .filter(|s| !in_vector_window(s.at))
            .map(|s| s.bank_deg)
            .collect();
        let da_bank_sd = if da_bank.len() >= 3 {
            let mean = da_bank.iter().map(|&b| b as f64).sum::<f64>() / da_bank.len() as f64;
            let var = da_bank
                .iter()
                .map(|&b| (b as f64 - mean).powi(2))
                .sum::<f64>()
                / da_bank.len() as f64;
            var.sqrt() as f32
        } else {
            0.0
        };
        let da_ias_mean = da_samples.iter().map(|s| s.ias_kt as f64).sum::<f64>() / n_da;
        let da_ias_var = da_samples
            .iter()
            .map(|s| (s.ias_kt as f64 - da_ias_mean).powi(2))
            .sum::<f64>()
            / n_da;
        let da_ias_sd = da_ias_var.sqrt() as f32;
        // v0.15.18: dieselbe Gleitwinkel-Skalierung wie das 1000-ft-Gate
        // (Sinkgrenze oben). Der v0.15.17-London-Fix skalierte
        // NUR das 1000-ft-Gate — das 200-ft-DA-Gate blieb fix bei −1000 fpm.
        // Auf einem korrekt geflogenen Steilanflug (z.B. ENTC/4°, EGLC/5,5°)
        // liegt die Soll-Sinkrate am DA-Gate physikalisch schon über −1000 fpm
        // (4° @143 kt ≈ −1013 fpm), sodass `stable_at_da` dort mathematisch NIE
        // erreichbar war. Jetzt winkel-korrekt → identisch zum 1000-ft-Gate.
        // Score-Version 24: dieselbe GS-abhaengige Grenze wie das Gate.
        // 2.1.6: unter 200 ft genuegt ein Sinklauf von 5 s (`DA_SINK_TOLERANZ_MS`),
        // und die Fahrt muss in der Naehe der typischen Vref liegen — bisher
        // zaehlte nur die Streuung, nicht die Lage (SWR 255: 32 kt zu langsam
        // und −1180 fpm galten als „stabil bei 200 ft“). Ohne bekannte Vref
        // (Muster unbekannt) entfaellt die Fahrtpruefung.
        let da_excess_sink =
            laengster_sinklauf(&da_samples, gs_factor).is_some_and(|l| l.dauer_ms() >= crate::grenzwerte::grenzwerte().anflug_kern.da_sink_toleranz_ms.wert);
        let da_zu_langsam = konfig
            .typical_vref_kt
            .is_some_and(|vref| da_ias_mean < f64::from(vref - crate::grenzwerte::grenzwerte().anflug_kern.da_vref_toleranz_kt.wert));
        // Strenger Cutoff bei DA: jerk < 80, bank < 3°, ias < 8 kt
        let da_stable = da_jerk < 80.0 && da_bank_sd < 3.0 && da_ias_sd < 8.0 && !da_excess_sink && !da_zu_langsam;
        out.stable_at_da = Some(da_stable);
    }

    // 11) v0.5.26: Stall-Warning-Counter ueber gesamten Buffer.
    out.stall_warning_count = buf.iter().filter(|s| s.stall_warning).count() as u32;

    out
}
