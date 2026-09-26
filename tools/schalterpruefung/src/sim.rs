//! SimConnect-Seite (nur Windows): Verbindung, MobiFlight-Kanäle, Messen.
//!
//! Das Werkzeug LIEST nur. Geschrieben wird ausschließlich in die
//! MobiFlight-Befehlsbereiche (`<Client>.Command`) — das ist der Weg, auf dem
//! man dem Modul sagt, welche LVars es liefern soll. `MF.SimVars.Set.` (der
//! einzige Befehl, der Simulatorwerte ändert) sperrt `befehl_kodieren`.

use std::collections::HashMap;
use std::ffi::CString;
use std::time::{Duration, Instant};

use crate::auswertung::Werte;
use crate::mobiflight::{
    self, antwort_deuten, befehl_kodieren, Antwort, ClientBlock, Kanal, ListenSammler,
    LVARS_BEREICH_GROESSE, NACHRICHT_GROESSE,
};

#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code
)]
#[allow(clippy::all)]
mod sys {
    include!(concat!(env!("OUT_DIR"), "/simconnect_bindings.rs"));
}

pub const BEENDET: &str = "Der Simulator wurde beendet (SimConnect-Verbindung weg).";

/// Standard-SimVars, die zusätzlich zu den LVars beobachtet werden.
pub const STANDARD_SIMVARS: &[(&str, &str)] = &[
    ("LIGHT BEACON", "Bool"),
    ("LIGHT STROBE", "Bool"),
    ("LIGHT NAV", "Bool"),
    ("LIGHT LANDING", "Bool"),
    ("LIGHT LANDING:1", "Bool"),
    ("LIGHT LANDING:2", "Bool"),
    ("LIGHT LANDING:3", "Bool"),
    ("LIGHT LANDING:4", "Bool"),
    ("LIGHT TAXI", "Bool"),
    ("TRANSPONDER STATE:1", "Enum"),
    ("CABIN SEATBELTS ALERT SWITCH", "Bool"),
    ("AUTO BRAKE SWITCH CB", "Number"),
    ("SPOILERS ARMED", "Bool"),
    ("SPOILERS HANDLE POSITION", "Percent Over 100"),
    ("APU SWITCH", "Bool"),
    ("APU PCT RPM", "Percent Over 100"),
    ("BRAKE PARKING POSITION", "Bool"),
];

// ── Kennungen (eigener Namensraum dieser SimConnect-Verbindung) ────────────
const TITEL_DEF: u32 = 1;
const ATC_DEF: u32 = 2;
const SV_DEF: u32 = 10; // + Index
const TITEL_REQ: u32 = 0x0100_0000;
const ATC_REQ: u32 = 0x0180_0000;
const SV_REQ: u32 = 0x1000_0000;
const BL_REQ: u32 = 0x2000_0000;
const RESP_REQ: u32 = 0x3000_0000;

/// Client-Daten-IDs je MobiFlight-Kanal: Kanal k (0 = Standard-Client
/// „MobiFlight", 1 = Liste, 2.. = LVar-Blöcke).
fn ids(k: u32) -> (u32, u32, u32) {
    // (Command-Bereich, Response-Bereich, LVars-Bereich)
    (100 + 3 * k, 101 + 3 * k, 102 + 3 * k)
}
fn defs(k: u32) -> (u32, u32, u32) {
    // (Command-Def, Response-Def, LVar-Block-Def)
    (1000 + 3 * k, 1001 + 3 * k, 1002 + 3 * k)
}

const CLIENT_DATA_REQUEST_FLAG_CHANGED: u32 = 1; // SimConnect.h: SIMCONNECT_CLIENT_DATA_REQUEST_FLAG_CHANGED

extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut std::ffi::c_void;
}

static SIMCONNECT_DLL: &[u8] = include_bytes!(env!("SCHALTER_SIMCONNECT_DLL"));

/// Die eingebettete SimConnect.dll bereitstellen und laden, BEVOR der erste
/// SimConnect-Aufruf passiert (die Exe lädt sie verzögert, siehe build.rs).
pub fn dll_bereitstellen() -> Result<(), String> {
    let dir = std::env::temp_dir().join("AeroACARS-Schalterpruefung");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let p = dir.join("SimConnect.dll");
    let passt = std::fs::metadata(&p)
        .map(|m| m.len() as usize == SIMCONNECT_DLL.len())
        .unwrap_or(false);
    if !passt {
        std::fs::write(&p, SIMCONNECT_DLL).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = p
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let h = unsafe { LoadLibraryW(wide.as_ptr()) };
    if h.is_null() {
        return Err(format!(
            "SimConnect.dll konnte nicht geladen werden ({}): {}",
            p.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[derive(Debug)]
enum Msg {
    Open(String),
    Quit,
    Ausnahme { code: u32, send_id: u32 },
    ObjektDaten { req: u32, bytes: Vec<u8> },
    ClientDaten { req: u32, bytes: Vec<u8> },
    Sonst,
}

struct MfKanal {
    name: String,
    k: u32,
    kanal: Kanal,
}

struct Block {
    block: ClientBlock,
    kanal_idx: usize,
}

pub struct Sim {
    h: sys::HANDLE,
    pub simulator: String,
    kanaele: Vec<MfKanal>,
    bloecke: Vec<Block>,
    simvars: Vec<(String, bool)>,
    sv_send_ids: HashMap<u32, usize>,
    pub abgelehnt: Vec<String>,
    gen: u32,
    pub mf_version: Option<String>,
    pub uebersprungen: Vec<String>,
}

unsafe impl Send for Sim {}

impl Drop for Sim {
    fn drop(&mut self) {
        if !self.h.is_null() {
            unsafe { sys::SimConnect_Close(self.h) };
            self.h = std::ptr::null_mut();
        }
    }
}

fn hr_ok(hr: i32, was: &str) -> Result<(), String> {
    if hr == 0 {
        Ok(())
    } else {
        Err(format!("{was}: HRESULT 0x{:08X}", hr as u32))
    }
}

fn c_text(bytes: &[u8]) -> String {
    let ende = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..ende]).trim().to_string()
}

impl Sim {
    /// Verbindung öffnen. `Err` = MSFS läuft nicht (oder noch nicht bereit).
    pub fn verbinden() -> Result<Self, String> {
        let name = CString::new("AeroACARS Schalterpruefung").unwrap();
        let mut h: sys::HANDLE = std::ptr::null_mut();
        let hr = unsafe {
            sys::SimConnect_Open(
                &mut h,
                name.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
            )
        };
        hr_ok(hr, "SimConnect_Open")?;
        let mut sim = Self {
            h,
            simulator: String::new(),
            kanaele: Vec::new(),
            bloecke: Vec::new(),
            simvars: Vec::new(),
            sv_send_ids: HashMap::new(),
            abgelehnt: Vec::new(),
            gen: 0,
            mf_version: None,
            uebersprungen: Vec::new(),
        };
        // Auf die OPEN-Meldung warten — erst dann ist die Verbindung wirklich da.
        let mut name = String::new();
        let da = sim.pumpen(Duration::from_secs(5), |m| {
            if let Msg::Open(n) = m {
                name = n.clone();
                true
            } else {
                false
            }
        })?;
        if !da {
            return Err("MSFS hat die Verbindung nicht bestätigt".into());
        }
        sim.simulator = name;
        sim.feste_definitionen()?;
        Ok(sim)
    }

    fn naechste(&mut self) -> Result<Option<Msg>, String> {
        let mut p: *mut sys::SIMCONNECT_RECV = std::ptr::null_mut();
        let mut cb: sys::DWORD = 0;
        let hr = unsafe { sys::SimConnect_GetNextDispatch(self.h, &mut p, &mut cb) };
        if hr != 0 || p.is_null() || cb == 0 {
            // E_FAIL = Warteschlange leer. Andere Fehler zeigen sich als Quit
            // bzw. als ausbleibende Antworten.
            return Ok(None);
        }
        let recv = unsafe { &*p };
        let id = recv.dwID;
        let total = cb as usize;
        let payload = |header: usize| -> Vec<u8> {
            let start = header - std::mem::size_of::<sys::DWORD>();
            if total <= start {
                return Vec::new();
            }
            unsafe {
                std::slice::from_raw_parts((p as *const u8).add(start), total - start).to_vec()
            }
        };
        let m = if id == sys::SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_OPEN as u32 {
            let o = unsafe { &*(p as *const sys::SIMCONNECT_RECV_OPEN) };
            let feld = o.szApplicationName;
            let n: String = feld
                .iter()
                .take_while(|c| **c != 0)
                .map(|c| *c as u8 as char)
                .collect();
            let (ma, mi) = (o.dwApplicationVersionMajor, o.dwApplicationVersionMinor);
            Msg::Open(format!("{} {ma}.{mi}", n.trim()))
        } else if id == sys::SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_QUIT as u32 {
            Msg::Quit
        } else if id == sys::SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_EXCEPTION as u32 {
            let e = unsafe { &*(p as *const sys::SIMCONNECT_RECV_EXCEPTION) };
            Msg::Ausnahme {
                code: e.dwException,
                send_id: e.dwSendID,
            }
        } else if id == sys::SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_SIMOBJECT_DATA as u32 {
            let d = unsafe { &*(p as *const sys::SIMCONNECT_RECV_SIMOBJECT_DATA) };
            let req = d.dwRequestID;
            Msg::ObjektDaten {
                req,
                bytes: payload(std::mem::size_of::<sys::SIMCONNECT_RECV_SIMOBJECT_DATA>()),
            }
        } else if id == sys::SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_CLIENT_DATA as u32 {
            let d = unsafe { &*(p as *const sys::SIMCONNECT_RECV_CLIENT_DATA) };
            let req = d._base.dwRequestID;
            Msg::ClientDaten {
                req,
                bytes: payload(std::mem::size_of::<sys::SIMCONNECT_RECV_CLIENT_DATA>()),
            }
        } else {
            Msg::Sonst
        };
        Ok(Some(m))
    }

    /// Nachrichten abholen, bis `f` true liefert oder die Zeit um ist.
    /// Ausnahmen werden nebenbei ausgewertet; Quit ist ein Fehler.
    fn pumpen(&mut self, dauer: Duration, mut f: impl FnMut(&Msg) -> bool) -> Result<bool, String> {
        let ende = Instant::now() + dauer;
        loop {
            while let Some(m) = self.naechste()? {
                match &m {
                    Msg::Quit => return Err(BEENDET.into()),
                    Msg::Ausnahme { send_id, .. } => {
                        if let Some(i) = self.sv_send_ids.get(send_id).copied() {
                            self.simvars[i].1 = false;
                            let n = self.simvars[i].0.clone();
                            if !self.abgelehnt.contains(&n) {
                                self.abgelehnt.push(n);
                            }
                        }
                    }
                    _ => {}
                }
                if f(&m) {
                    return Ok(true);
                }
            }
            if Instant::now() >= ende {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn letzte_send_id(&self) -> Option<u32> {
        let mut id: sys::DWORD = 0;
        let hr = unsafe { sys::SimConnect_GetLastSentPacketID(self.h, &mut id) };
        (hr == 0).then_some(id)
    }

    fn feste_definitionen(&mut self) -> Result<(), String> {
        for (def, name) in [(TITEL_DEF, "TITLE"), (ATC_DEF, "ATC MODEL")] {
            let n = CString::new(name).unwrap();
            let hr = unsafe {
                sys::SimConnect_AddToDataDefinition(
                    self.h,
                    def,
                    n.as_ptr(),
                    std::ptr::null(),
                    sys::SIMCONNECT_DATATYPE_SIMCONNECT_DATATYPE_STRING256,
                    0.0,
                    u32::MAX,
                )
            };
            hr_ok(hr, name)?;
        }
        for (i, (name, einheit)) in STANDARD_SIMVARS.iter().enumerate() {
            let n = CString::new(*name).unwrap();
            let e = CString::new(*einheit).unwrap();
            let hr = unsafe {
                sys::SimConnect_AddToDataDefinition(
                    self.h,
                    SV_DEF + i as u32,
                    n.as_ptr(),
                    e.as_ptr(),
                    sys::SIMCONNECT_DATATYPE_SIMCONNECT_DATATYPE_FLOAT64,
                    0.0,
                    u32::MAX,
                )
            };
            self.simvars.push((name.to_string(), hr == 0));
            if hr != 0 {
                self.abgelehnt.push(name.to_string());
            } else if let Some(sid) = self.letzte_send_id() {
                self.sv_send_ids.insert(sid, i);
            }
        }
        // Asynchrone Ablehnungen (unbekannter Name) einsammeln.
        self.pumpen(Duration::from_millis(500), |_| false)?;
        Ok(())
    }

    /// TITLE und ATC MODEL des Nutzerflugzeugs. `Ok(None)` = kein Flugzeug
    /// (Hauptmenü o. Ä.).
    pub fn flugzeug(&mut self) -> Result<Option<(String, String)>, String> {
        self.gen = self.gen.wrapping_add(1) & 0xFFFF;
        let (tr, ar) = (TITEL_REQ + self.gen, ATC_REQ + self.gen);
        for (req, def) in [(tr, TITEL_DEF), (ar, ATC_DEF)] {
            let hr = unsafe {
                sys::SimConnect_RequestDataOnSimObject(
                    self.h,
                    req,
                    def,
                    sys::SIMCONNECT_OBJECT_ID_USER,
                    sys::SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_ONCE,
                    0,
                    0,
                    0,
                    0,
                )
            };
            hr_ok(hr, "RequestDataOnSimObject(TITLE)")?;
        }
        let (mut titel, mut atc) = (None, None);
        self.pumpen(Duration::from_secs(3), |m| {
            if let Msg::ObjektDaten { req, bytes } = m {
                if *req == tr {
                    titel = Some(c_text(bytes));
                } else if *req == ar {
                    atc = Some(c_text(bytes));
                }
            }
            titel.is_some() && atc.is_some()
        })?;
        match titel {
            Some(t) if !t.is_empty() => Ok(Some((t, atc.unwrap_or_default()))),
            _ => Ok(None),
        }
    }

    // ── MobiFlight ────────────────────────────────────────────────────────

    /// Befehls- und Antwortbereich eines MobiFlight-Clients anbinden.
    /// Gleiche Schritte wie der offizielle Connector
    /// (`SimConnectCache.cs::InitializeClientDataAreas`): Map + Create (ein
    /// ALREADY_CREATED ist harmlos) + Antwort-Anforderung ON_SET/CHANGED.
    fn kanal_anbinden(&mut self, name: &str) -> Result<usize, String> {
        let k = self.kanaele.len() as u32;
        let (cmd, resp, _) = ids(k);
        let (cmd_def, resp_def, _) = defs(k);
        for (bereich, suffix) in [(cmd, ".Command"), (resp, ".Response")] {
            let n = CString::new(format!("{name}{suffix}")).unwrap();
            let hr = unsafe { sys::SimConnect_MapClientDataNameToID(self.h, n.as_ptr(), bereich) };
            hr_ok(hr, "MapClientDataNameToID")?;
            unsafe {
                sys::SimConnect_CreateClientData(self.h, bereich, NACHRICHT_GROESSE as u32, 0)
            };
        }
        for def in [cmd_def, resp_def] {
            let hr = unsafe {
                sys::SimConnect_AddToClientDataDefinition(
                    self.h,
                    def,
                    0,
                    NACHRICHT_GROESSE as u32,
                    0.0,
                    u32::MAX,
                )
            };
            hr_ok(hr, "AddToClientDataDefinition")?;
        }
        let hr = unsafe {
            sys::SimConnect_RequestClientData(
                self.h,
                resp,
                RESP_REQ + k,
                resp_def,
                sys::SIMCONNECT_CLIENT_DATA_PERIOD_SIMCONNECT_CLIENT_DATA_PERIOD_ON_SET,
                CLIENT_DATA_REQUEST_FLAG_CHANGED,
                0,
                0,
                0,
            )
        };
        hr_ok(hr, "RequestClientData(Response)")?;
        self.kanaele.push(MfKanal {
            name: name.to_string(),
            k,
            kanal: Kanal::default(),
        });
        // Alten Inhalt des Antwortbereichs (z. B. „MF.Pong" vom letzten Lauf
        // oder vom MobiFlight-Connector) abholen und verwerfen — er darf nicht
        // als frische Antwort gelten.
        self.pumpen(Duration::from_millis(500), |_| false)?;
        let idx = self.kanaele.len() - 1;
        // Den CHANGED-Filter des Moduls auf dem Befehlskanal neutralisieren.
        self.senden(idx, mobiflight::DUMMY)?;
        Ok(idx)
    }

    fn senden(&mut self, idx: usize, befehl: &str) -> Result<(), String> {
        let folge = self.kanaele[idx].kanal.folge(befehl);
        let (cmd, _, _) = ids(self.kanaele[idx].k);
        let (cmd_def, _, _) = defs(self.kanaele[idx].k);
        for b in folge {
            let mut puffer = befehl_kodieren(&b)?;
            let hr = unsafe {
                sys::SimConnect_SetClientData(
                    self.h,
                    cmd,
                    cmd_def,
                    0,
                    0,
                    NACHRICHT_GROESSE as u32,
                    puffer.as_mut_ptr() as *mut std::ffi::c_void,
                )
            };
            hr_ok(hr, "SetClientData(Command)")?;
        }
        Ok(())
    }

    /// Antworten eines Kanals abholen, bis `f` true liefert.
    fn warte_auf(
        &mut self,
        idx: usize,
        dauer: Duration,
        mut f: impl FnMut(&Antwort) -> bool,
    ) -> Result<bool, String> {
        let req = RESP_REQ + self.kanaele[idx].k;
        let mut version = None;
        let r = self.pumpen(dauer, |m| match m {
            Msg::ClientDaten { req: r, bytes } if *r == req => {
                let a = antwort_deuten(bytes);
                if let Antwort::Version(v) = &a {
                    version = Some(v.clone());
                }
                f(&a)
            }
            _ => false,
        });
        if version.is_some() {
            self.mf_version = version;
        }
        r
    }

    /// Version abfragen + Ping; true, sobald ein frisches `MF.Pong` kommt.
    /// Da das Modul Befehle der Reihe nach abarbeitet, heißt das Pong auch:
    /// alle vorher gesendeten Befehle sind verarbeitet. Die Version davor
    /// sorgt dafür, dass das Pong sich vom alten Inhalt unterscheidet und den
    /// CHANGED-Filter passiert.
    fn synchron(&mut self, idx: usize, dauer: Duration) -> Result<bool, String> {
        self.senden(idx, "MF.Version.Get")?;
        self.senden(idx, "MF.Ping")?;
        self.warte_auf(idx, dauer, |a| *a == Antwort::Pong)
    }

    /// Standard-Client „MobiFlight" anbinden und anpingen.
    pub fn mobiflight_pruefen(&mut self) -> Result<bool, String> {
        if self.kanaele.is_empty() {
            self.kanal_anbinden(mobiflight::STANDARD_CLIENT)?;
        }
        self.synchron(0, Duration::from_secs(5))
    }

    fn client_anlegen(&mut self, namen: &[String]) -> Result<(), String> {
        for n in namen {
            self.senden(0, &format!("MF.Clients.Add.{n}"))?;
        }
        if !self.synchron(0, Duration::from_secs(10))? {
            return Err("MobiFlight-Modul hat das Anlegen der Clients nicht bestätigt".into());
        }
        Ok(())
    }

    /// Komplette LVar-Liste über einen eigenen Client.
    pub fn lvars_holen(&mut self) -> Result<Vec<String>, String> {
        let name = format!("{}L", mobiflight::EIGENER_CLIENT_PRAEFIX);
        let idx = match self.kanaele.iter().position(|k| k.name == name) {
            Some(i) => i,
            None => {
                self.client_anlegen(std::slice::from_ref(&name))?;
                self.kanal_anbinden(&name)?
            }
        };
        self.senden(idx, "MF.SimVars.Clear")?;
        self.senden(idx, "MF.LVars.List")?;
        let mut s = ListenSammler::default();
        let fertig = self.warte_auf(idx, Duration::from_secs(20), |a| {
            s.aufnehmen(a);
            s.fertig
        })?;
        if !fertig {
            return Err(format!(
                "LVar-Liste kam nicht vollständig an ({} Namen, kein Listenende)",
                s.namen.len()
            ));
        }
        Ok(s.namen)
    }

    /// Alle LVars abonnieren — verteilt auf eigene Clients zu je
    /// `mobiflight::BLOCK` Stück. `fortschritt(fertig, gesamt)`.
    pub fn abonnieren(
        &mut self,
        lvars: &[String],
        mut fortschritt: impl FnMut(usize, usize),
    ) -> Result<(), String> {
        let mut gueltig = Vec::new();
        for n in lvars {
            if mobiflight::lvar_code(n).is_some() {
                gueltig.push(n.clone());
            } else {
                self.uebersprungen.push(n.clone());
            }
        }
        let bloecke = mobiflight::bloecke_bilden(&gueltig);
        let namen: Vec<String> = bloecke.iter().map(|b| b.name.clone()).collect();
        self.client_anlegen(&namen)?;
        let gesamt = gueltig.len();
        let mut fertig = 0;
        for b in bloecke {
            let idx = self.kanal_anbinden(&b.name)?;
            let k = self.kanaele[idx].k;
            let (_, _, lv_bereich) = ids(k);
            let (_, _, lv_def) = defs(k);
            let n = CString::new(format!("{}.LVars", b.name)).unwrap();
            let hr =
                unsafe { sys::SimConnect_MapClientDataNameToID(self.h, n.as_ptr(), lv_bereich) };
            hr_ok(hr, "MapClientDataNameToID(LVars)")?;
            unsafe {
                sys::SimConnect_CreateClientData(
                    self.h,
                    lv_bereich,
                    LVARS_BEREICH_GROESSE as u32,
                    0,
                )
            };
            let hr = unsafe {
                sys::SimConnect_AddToClientDataDefinition(
                    self.h,
                    lv_def,
                    0,
                    (b.lvars.len() * 4) as u32,
                    0.0,
                    u32::MAX,
                )
            };
            hr_ok(hr, "AddToClientDataDefinition(LVars)")?;
            self.senden(idx, "MF.SimVars.Clear")?;
            for (i, lv) in b.lvars.iter().enumerate() {
                let code = mobiflight::lvar_code(lv).expect("oben geprüft");
                self.senden(idx, &format!("MF.SimVars.Add.{code}"))?;
                if i % 25 == 24 {
                    self.pumpen(Duration::from_millis(20), |_| false)?;
                }
            }
            if !self.synchron(idx, Duration::from_secs(15))? {
                return Err(format!("MobiFlight hat {} nicht bestätigt", b.name));
            }
            fertig += b.lvars.len();
            fortschritt(fertig, gesamt);
            self.bloecke.push(Block {
                block: b,
                kanal_idx: idx,
            });
        }
        Ok(())
    }

    /// Namen aller beobachteten Variablen in Messreihenfolge.
    pub fn variablen(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .bloecke
            .iter()
            .flat_map(|b| b.block.lvars.iter().map(|n| format!("L:{n}")))
            .collect();
        v.extend(self.simvars.iter().map(|(n, _)| format!("A:{n}")));
        v
    }

    /// Eine Momentaufnahme aller Variablen.
    pub fn messen(&mut self) -> Result<Werte, String> {
        self.gen = self.gen.wrapping_add(1) & 0xFFFF;
        let basis = self.gen * 64;
        let mut offen: HashMap<u32, (bool, usize)> = HashMap::new(); // req → (ist_block, index)
        for (i, b) in self.bloecke.iter().enumerate() {
            let k = self.kanaele[b.kanal_idx].k;
            let (_, _, bereich) = ids(k);
            let (_, _, def) = defs(k);
            let req = BL_REQ + basis + i as u32;
            let hr = unsafe {
                sys::SimConnect_RequestClientData(
                    self.h,
                    bereich,
                    req,
                    def,
                    sys::SIMCONNECT_CLIENT_DATA_PERIOD_SIMCONNECT_CLIENT_DATA_PERIOD_ONCE,
                    0,
                    0,
                    0,
                    0,
                )
            };
            hr_ok(hr, "RequestClientData(LVars)")?;
            offen.insert(req, (true, i));
        }
        for (i, (_, aktiv)) in self.simvars.iter().enumerate() {
            if !aktiv {
                continue;
            }
            let req = SV_REQ + basis + i as u32;
            let hr = unsafe {
                sys::SimConnect_RequestDataOnSimObject(
                    self.h,
                    req,
                    SV_DEF + i as u32,
                    sys::SIMCONNECT_OBJECT_ID_USER,
                    sys::SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_ONCE,
                    0,
                    0,
                    0,
                    0,
                )
            };
            if hr == 0 {
                offen.insert(req, (false, i));
            }
        }
        let mut block_werte: Vec<Option<Vec<Option<f64>>>> = vec![None; self.bloecke.len()];
        let mut sv_werte: Vec<Option<f64>> = vec![None; self.simvars.len()];
        let laengen: Vec<usize> = self.bloecke.iter().map(|b| b.block.lvars.len()).collect();
        self.pumpen(Duration::from_millis(2500), |m| {
            let (req, bytes) = match m {
                Msg::ClientDaten { req, bytes } | Msg::ObjektDaten { req, bytes } => (req, bytes),
                _ => return false,
            };
            if let Some((ist_block, i)) = offen.remove(req) {
                if ist_block {
                    block_werte[i] = Some(mobiflight::floats_lesen(bytes, laengen[i]));
                } else if bytes.len() >= 8 {
                    let mut b8 = [0u8; 8];
                    b8.copy_from_slice(&bytes[..8]);
                    sv_werte[i] = Some(f64::from_le_bytes(b8));
                }
            }
            offen.is_empty()
        })?;
        let mut w: Werte = Vec::new();
        for (i, bw) in block_werte.into_iter().enumerate() {
            match bw {
                Some(v) => w.extend(v),
                None => w.extend(std::iter::repeat(None).take(laengen[i])),
            }
        }
        w.extend(sv_werte);
        Ok(w)
    }

    /// Eigene Clients leeren, damit das Modul nicht weiter für uns liest.
    pub fn aufraeumen(&mut self) {
        for idx in 1..self.kanaele.len() {
            let _ = self.senden(idx, "MF.SimVars.Clear");
        }
        let _ = self.pumpen(Duration::from_millis(200), |_| false);
    }
}
