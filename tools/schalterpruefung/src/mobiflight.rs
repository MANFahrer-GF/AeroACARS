//! MobiFlight-WASM-Protokoll (Paket `mobiflight-event-module` v1.0.1).
//!
//! Plattformunabhängig: Kodieren der Befehle, Deuten der Antworten, Sammeln
//! der LVar-Liste, Aufteilen auf Clients. Die SimConnect-Seite steht in
//! `sim.rs`.
//!
//! Quelle — NICHT geraten: github.com/MobiFlight/MobiFlight-WASM-Module,
//! Tag 1.0.1 (= Commit cb4764a), Datei `src/Sources/Code/Module.cpp`:
//!
//! * Z. 23–27: Datenbereiche heißen `<Client>.LVars`, `.StringVars`,
//!   `.Command`, `.Response`; der Standard-Client heißt `MobiFlight`.
//! * Z. 29: `MOBIFLIGHT_MESSAGE_SIZE = 1024` — Befehle und Antworten sind
//!   1024-Byte-Blöcke mit NUL-terminiertem Text (Z. 202–212 `SendResponse`
//!   kopiert immer 1024 Byte ab dem Textanfang, hinter dem NUL steht also
//!   Datenmüll — nur bis zum ersten NUL lesen).
//! * Z. 41: `MOBIFLIGHT_MAX_VARS_PER_FRAME = 30` — das Modul liest je Client
//!   und Frame nur 30 Variablen reihum (Z. 435–455). Deshalb verteilen wir die
//!   LVars auf mehrere eigene Clients: Jeder Client bekommt eigene 30 je Frame.
//! * Z. 225–242 `ListLVars`: zählt `get_name_of_named_variable(i)` für
//!   i = 0..1000 — die Liste endet also spätestens bei 1000 Namen. Jeder Name
//!   kommt als EIGENE Antwort; davor `MF.LVars.List.Start`, danach
//!   `MF.LVars.List.End` (Z. 661–666).
//! * Z. 288–330 `RegisterFloatSimVar`: `MF.SimVars.Add.<Code>` legt die
//!   Variable als float32 an Offset `Anzahl*4` im Bereich `<Client>.LVars` ab;
//!   Reihenfolge der Add-Befehle = Reihenfolge im Bereich.
//! * Z. 467: `<Client>.LVars` ist 4096 Byte groß → höchstens 1024 Floats je
//!   Client.
//! * Z. 507–515: Das Modul liest `.Command` mit `ON_SET` + `CHANGED` — ein
//!   Befehl, der byte-gleich zum vorigen ist, kommt NICHT an. Der offizielle
//!   Connector schickt deshalb `MF.DummyCmd` dazwischen
//!   (MobiFlight-Connector `WasmModuleClient.cs`, `DummyCommand`).
//! * Z. 519–556 `RegisterNewClient`: `MF.Clients.Add.<Name>` legt die vier
//!   Bereiche an bzw. benutzt einen gleichnamigen Client weiter (Neustart des
//!   Werkzeugs); Antwort `MF.Clients.Add.<Name>.Finished` auf dem Kanal des
//!   ANFRAGENDEN Clients (Z. 215–221, 709).
//! * Z. 651–653 `MF.Ping` → `MF.Pong`; Z. 668–673 `MF.Version.Get` →
//!   `MF.Version.<v>`; Z. 656–659 `MF.SimVars.Clear` (ohne Antwort).
//! * Z. 676–685 `MF.SimVars.Set.` SCHREIBT in den Simulator — wird von diesem
//!   Werkzeug nie gesendet (siehe `befehl_kodieren`).

pub const NACHRICHT_GROESSE: usize = 1024;
pub const LVARS_BEREICH_GROESSE: usize = 4096;
/// Obergrenze der Modul-Liste (Module.cpp Z. 228).
pub const LISTE_MAX: usize = 1000;
/// LVars je eigenem Client. 30 je Frame → 150 Variablen sind nach 5 Frames
/// einmal komplett aufgefrischt; bei 1000 LVars sind das 7 Clients.
pub const BLOCK: usize = 150;
const _: () = assert!(BLOCK * 4 <= LVARS_BEREICH_GROESSE);
pub const STANDARD_CLIENT: &str = "MobiFlight";
pub const EIGENER_CLIENT_PRAEFIX: &str = "AeroACARS_SP";
pub const DUMMY: &str = "MF.DummyCmd";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Antwort {
    Pong,
    Version(String),
    ListeStart,
    ListeEnde,
    ClientAngelegt(String),
    /// Alles andere — innerhalb einer laufenden Liste ist es ein LVar-Name.
    Text(String),
}

/// Text einer Antwort: bis zum ersten NUL, ohne Randleerzeichen.
pub fn antwort_text(bytes: &[u8]) -> String {
    let ende = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..ende]).trim().to_string()
}

pub fn antwort_deuten(bytes: &[u8]) -> Antwort {
    let t = antwort_text(bytes);
    if t == "MF.Pong" {
        Antwort::Pong
    } else if t == "MF.LVars.List.Start" {
        Antwort::ListeStart
    } else if t == "MF.LVars.List.End" {
        Antwort::ListeEnde
    } else if let Some(v) = t.strip_prefix("MF.Version.") {
        Antwort::Version(v.to_string())
    } else if let Some(rest) = t.strip_prefix("MF.Clients.Add.") {
        match rest.strip_suffix(".Finished") {
            Some(name) => Antwort::ClientAngelegt(name.to_string()),
            None => Antwort::Text(t),
        }
    } else {
        Antwort::Text(t)
    }
}

/// Befehl als 1024-Byte-Block mit NUL-Ende. Verweigert jeden schreibenden
/// Befehl — das Werkzeug liest nur.
pub fn befehl_kodieren(befehl: &str) -> Result<[u8; NACHRICHT_GROESSE], String> {
    if befehl.starts_with("MF.SimVars.Set") {
        return Err("Schreibende Befehle sind in diesem Werkzeug gesperrt".into());
    }
    let b = befehl.as_bytes();
    if b.len() >= NACHRICHT_GROESSE {
        return Err(format!("Befehl zu lang ({} Byte)", b.len()));
    }
    if b.contains(&0) {
        return Err("Befehl enthält NUL".into());
    }
    let mut out = [0u8; NACHRICHT_GROESSE];
    out[..b.len()].copy_from_slice(b);
    Ok(out)
}

/// Rechencode für eine LVar. `None`, wenn der Name die Klammersyntax
/// sprengen würde (solche Namen werden übersprungen und gezählt).
pub fn lvar_code(name: &str) -> Option<String> {
    let n = name.trim();
    if n.is_empty() || n.contains(')') || n.contains('(') || n.len() > 200 {
        return None;
    }
    Some(format!("(L:{n})"))
}

/// Sammelt die LVar-Namen zwischen `Start` und `Ende`.
#[derive(Debug, Default)]
pub struct ListenSammler {
    laeuft: bool,
    pub fertig: bool,
    pub namen: Vec<String>,
}

impl ListenSammler {
    pub fn aufnehmen(&mut self, a: &Antwort) {
        match a {
            Antwort::ListeStart => {
                self.laeuft = true;
                self.fertig = false;
                self.namen.clear();
            }
            Antwort::ListeEnde => {
                if self.laeuft {
                    self.laeuft = false;
                    self.fertig = true;
                }
            }
            Antwort::Text(t)
                if self.laeuft && !t.is_empty() && !self.namen.iter().any(|n| n == t) =>
            {
                self.namen.push(t.clone());
            }
            _ => {}
        }
    }
}

/// Ein eigener MobiFlight-Client mit seinem Anteil an LVars.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientBlock {
    pub name: String,
    /// Namen (ohne `L:`) in Registrierungsreihenfolge = Offset/4.
    pub lvars: Vec<String>,
}

pub fn bloecke_bilden(lvars: &[String]) -> Vec<ClientBlock> {
    lvars
        .chunks(BLOCK)
        .enumerate()
        .map(|(i, c)| ClientBlock {
            name: format!("{EIGENER_CLIENT_PRAEFIX}{}", i + 1),
            lvars: c.to_vec(),
        })
        .collect()
}

/// Liest n float32 (little endian) aus dem LVars-Bereich.
pub fn floats_lesen(bytes: &[u8], n: usize) -> Vec<Option<f64>> {
    (0..n)
        .map(|i| {
            let o = i * 4;
            bytes
                .get(o..o + 4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64)
        })
        .collect()
}

/// Merkt sich den zuletzt gesendeten Befehl je Kanal, damit ein byte-gleicher
/// Befehl nicht im `CHANGED`-Filter des Moduls verschwindet.
#[derive(Debug, Default)]
pub struct Kanal {
    letzter: Option<String>,
}

impl Kanal {
    /// Die tatsächlich zu sendenden Befehle (ggf. mit Dummy davor).
    pub fn folge(&mut self, befehl: &str) -> Vec<String> {
        let mut v = Vec::new();
        if self.letzter.as_deref() == Some(befehl) {
            v.push(DUMMY.to_string());
        }
        v.push(befehl.to_string());
        self.letzter = Some(befehl.to_string());
        v
    }
}

/// „ATCCOM.AC_MODEL_A388.0.text" / „ATCCOM.AC_MODEL A388.0.text" → „A388".
pub fn icao_aus_atc_model(roh: &str) -> Option<String> {
    let t = roh.trim();
    if t.is_empty() {
        return None;
    }
    let kern = if let Some(p) = t.find("AC_MODEL") {
        let rest = &t[p + "AC_MODEL".len()..];
        let rest = rest.trim_start_matches(['_', ' ']);
        rest.split('.').next().unwrap_or("").trim().to_string()
    } else {
        t.to_string()
    };
    let k = kern.to_ascii_uppercase();
    if (2..=4).contains(&k.len()) && k.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(k)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(text: &str) -> Vec<u8> {
        let mut v = vec![0u8; NACHRICHT_GROESSE];
        v[..text.len()].copy_from_slice(text.as_bytes());
        v
    }

    #[test]
    fn antworten_werden_gedeutet() {
        assert_eq!(antwort_deuten(&block("MF.Pong")), Antwort::Pong);
        assert_eq!(
            antwort_deuten(&block("MF.Version.1.0.1")),
            Antwort::Version("1.0.1".into())
        );
        assert_eq!(
            antwort_deuten(&block("MF.LVars.List.Start")),
            Antwort::ListeStart
        );
        assert_eq!(
            antwort_deuten(&block("MF.LVars.List.End")),
            Antwort::ListeEnde
        );
        assert_eq!(
            antwort_deuten(&block("MF.Clients.Add.AeroACARS_SP1.Finished")),
            Antwort::ClientAngelegt("AeroACARS_SP1".into())
        );
        assert_eq!(
            antwort_deuten(&block("INI_STROBE_SWITCH")),
            Antwort::Text("INI_STROBE_SWITCH".into())
        );
    }

    #[test]
    fn muell_hinter_dem_nul_wird_ignoriert() {
        // SendResponse kopiert immer 1024 Byte — hinter dem NUL steht Rest.
        let mut b = block("MF.Pong");
        b[8..20].copy_from_slice(b"XXXXXXXXXXXX");
        assert_eq!(antwort_deuten(&b), Antwort::Pong);
    }

    #[test]
    fn liste_wird_zwischen_start_und_ende_gesammelt() {
        let mut s = ListenSammler::default();
        for t in [
            "MF.Pong", // vor Start: ignorieren
            "MF.LVars.List.Start",
            "A32NX_X",
            "INI_STROBE",
            "INI_STROBE", // doppelt
            "MF.LVars.List.End",
            "NACH_ENDE",
        ] {
            s.aufnehmen(&antwort_deuten(&block(t)));
        }
        assert!(s.fertig);
        assert_eq!(s.namen, vec!["A32NX_X", "INI_STROBE"]);
    }

    #[test]
    fn befehle_sind_nul_terminiert_und_schreiben_ist_gesperrt() {
        let b = befehl_kodieren("MF.Ping").unwrap();
        assert_eq!(&b[..7], b"MF.Ping");
        assert!(b[7..].iter().all(|x| *x == 0));
        assert!(befehl_kodieren("MF.SimVars.Set.1 (>L:X)").is_err());
        assert!(befehl_kodieren(&"x".repeat(1024)).is_err());
    }

    #[test]
    fn gleicher_befehl_bekommt_dummy_davor() {
        let mut k = Kanal::default();
        assert_eq!(k.folge("MF.Ping"), vec!["MF.Ping"]);
        assert_eq!(k.folge("MF.Ping"), vec![DUMMY, "MF.Ping"]);
        assert_eq!(k.folge("MF.Version.Get"), vec!["MF.Version.Get"]);
    }

    #[test]
    fn bloecke_und_floats() {
        let namen: Vec<String> = (0..320).map(|i| format!("V{i}")).collect();
        let b = bloecke_bilden(&namen);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].name, "AeroACARS_SP1");
        assert_eq!(b[2].lvars.len(), 20);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&2.5f32.to_le_bytes());
        assert_eq!(floats_lesen(&bytes, 3), vec![Some(1.0), Some(2.5), None]);
    }

    #[test]
    fn lvar_code_und_icao() {
        assert_eq!(lvar_code("INI_X").unwrap(), "(L:INI_X)");
        assert!(lvar_code("BAD)NAME").is_none());
        assert_eq!(
            icao_aus_atc_model("ATCCOM.AC_MODEL_A388.0.text").unwrap(),
            "A388"
        );
        assert_eq!(
            icao_aus_atc_model("ATCCOM.AC_MODEL A359.0.text").unwrap(),
            "A359"
        );
        assert_eq!(icao_aus_atc_model("BCS3").unwrap(), "BCS3");
        assert!(icao_aus_atc_model("A350-900 Marketing").is_none());
        assert!(icao_aus_atc_model("").is_none());
    }
}
