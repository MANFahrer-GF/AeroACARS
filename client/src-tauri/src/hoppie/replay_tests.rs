//! v1.9.5 (#hoppie-replay): REAL field traffic, replayed through the
//! whole receive path.
//!
//! Every packet below is copied verbatim from a GSG flight log
//! (`live:/var/lib/aeroacars-recorder/flight-logs/gsg/<pilot>/<pirep>`,
//! event `datalink`), not written from the documentation. The earlier
//! tests used invented shapes such as `HANDOVER EDGG`; the network sends
//! `HANDOVER @LRWW`, and that difference logged a pilot on to "@LRWW" on
//! 19.09.2026. Nothing here is mocked except the two edges the app
//! itself does not own: the Hoppie server (a local stand-in that records
//! what we send and answers `ok`) and the app's side effects (a recorder
//! instead of the activity log / notifications / settings file).
//!
//! What the pilot does between polls (logon, WILCO) goes through the
//! same session methods and addressing functions the Tauri commands use.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::*;
use crate::hoppie::{
    resolve_reply_station, resolve_wire_mrn, thread_entries, HistoryMeta, HoppieHttp,
    HoppieSession, MinMeta, MsgMeta, TelexEntry, ThreadEntryDto,
};

// ---------------------------------------------------------------------
// Edges: side-effect recorder and Hoppie stand-in
// ---------------------------------------------------------------------

#[derive(Default)]
struct Recorder {
    activity: Mutex<Vec<String>>,
    open_session: Mutex<Option<String>>,
}

impl PollEffects for Recorder {
    fn activity(&self, _level: ActivityLevel, message: String, _detail: Option<String>) {
        self.activity.lock().unwrap().push(message);
    }
    fn datalink(
        &self,
        _direction: &str,
        _channel: &str,
        _station: Option<String>,
        _min: Option<u32>,
        _mrn: Option<u32>,
        _response_code: Option<String>,
        _text: String,
    ) {
    }
    fn notify(&self, _from: &str) {}
    fn set_open_session(&self, _callsign: &str, station: &str) {
        *self.open_session.lock().unwrap() = Some(station.to_string());
    }
    fn clear_open_session(&self) {
        *self.open_session.lock().unwrap() = None;
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

/// Minimal HTTP/1.1 stand-in for `connect.html`: records every form
/// post and answers `ok`.
async fn stand_in() -> (String, Arc<Mutex<Vec<HashMap<String, String>>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/acars/system/connect.html", listener.local_addr().unwrap());
    let received: Arc<Mutex<Vec<HashMap<String, String>>>> = Arc::default();
    let sink = Arc::clone(&received);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let sink = Arc::clone(&sink);
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let body = loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(split) = text.find("\r\n\r\n") {
                        let len = text[..split]
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if buf.len() >= split + 4 + len {
                            break String::from_utf8_lossy(&buf[split + 4..split + 4 + len])
                                .to_string();
                        }
                    }
                };
                let form: HashMap<String, String> = body
                    .split('&')
                    .filter_map(|kv| kv.split_once('='))
                    .map(|(k, v)| (percent_decode(k), percent_decode(v)))
                    .collect();
                sink.lock().unwrap().push(form);
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                    .await;
            });
        }
    });
    (url, received)
}

// ---------------------------------------------------------------------
// The rig
// ---------------------------------------------------------------------

struct Rig {
    session: Arc<StdMutex<HoppieSession>>,
    telex_log: StdMutex<Vec<TelexEntry>>,
    min_meta: StdMutex<MinMeta>,
    history_meta: StdMutex<HistoryMeta>,
    http: HoppieHttp,
    fx: Recorder,
    sent_to_server: Arc<Mutex<Vec<HashMap<String, String>>>>,
}

const CALLSIGN: &str = "DLH4TK";

impl Rig {
    async fn new() -> Self {
        let (url, sent_to_server) = stand_in().await;
        Self {
            session: Arc::new(StdMutex::new(HoppieSession::new("SERVER".into()))),
            telex_log: StdMutex::default(),
            min_meta: StdMutex::default(),
            history_meta: StdMutex::default(),
            http: HoppieHttp::for_test(&url),
            fx: Recorder::default(),
            sent_to_server,
        }
    }

    /// The pilot presses "send logon" — what `send_cpdlc_element` does
    /// under its lock for `DM_REQUEST_LOGON`.
    fn logon(&self, station: &str) -> u32 {
        let spec = hoppie_protocol::elements::find("DM_REQUEST_LOGON").unwrap();
        let resolved = hoppie_protocol::elements::resolve(spec, &[]).unwrap();
        let mut s = self.session.lock().unwrap();
        let msg = s.record_logon_request(
            station,
            spec.response,
            None,
            resolved.filled_text.clone(),
            hoppie_protocol::elements::ParsedElement::Recognized(resolved),
        );
        self.min_meta.lock().unwrap().insert(
            (false, msg.min),
            MsgMeta {
                at: chrono::Utc::now(),
                station: station.to_string(),
                wire_min: None,
            },
        );
        msg.min
    }

    /// The pilot asks for a PDC.
    fn pdc(&self, station: &str) {
        self.session
            .lock()
            .unwrap()
            .note_pdc_request(station, chrono::Utc::now());
    }

    /// The pilot presses WILCO on the uplink `station` sent as wire MIN
    /// `wire_min`. Returns the station the reply is addressed to and the
    /// MRN on the wire — or `Err` exactly where `send_cpdlc_element`
    /// refuses (a superseded uplink) or where the UI never offers the
    /// button (the uplink is not in the CPDLC thread at all).
    fn wilco(&self, station: &str, wire_min: u32) -> Result<(String, Option<u32>), String> {
        let internal = {
            let meta = self.min_meta.lock().unwrap();
            meta.iter()
                .find(|((up, _), m)| *up && m.station == station && m.wire_min == Some(wire_min))
                .map(|((_, min), _)| *min)
                .ok_or_else(|| format!("{station} MIN {wire_min} is not a CPDLC thread entry"))?
        };
        let mut s = self.session.lock().unwrap();
        if s.thread.is_superseded_uplink(internal) {
            return Err(format!("{station} MIN {wire_min} is superseded"));
        }
        if !s.thread.is_uplink_open(internal) {
            return Err(format!("{station} MIN {wire_min} is not open"));
        }
        let (to, mrn) = {
            let meta = self.min_meta.lock().unwrap();
            (
                resolve_reply_station(&meta, Some(internal), &s.addressee()),
                resolve_wire_mrn(&meta, Some(internal)),
            )
        };
        let spec = hoppie_protocol::elements::find("DM0").unwrap();
        let resolved = hoppie_protocol::elements::resolve(spec, &[]).unwrap();
        s.thread.record_sent(
            spec.response,
            Some(internal),
            resolved.filled_text.clone(),
            hoppie_protocol::elements::ParsedElement::Recognized(resolved),
        );
        Ok((to, mrn))
    }

    /// One poll response, as `{FROM TYPE {PACKET}}` envelopes in wire order.
    async fn poll(&self, envelopes: &[(&str, &str, &str)]) {
        let content: String = envelopes
            .iter()
            .map(|(from, kind, packet)| format!("{{{from} {kind} {{{packet}}}}}"))
            .collect();
        process_poll_payload(
            &self.fx,
            &self.http,
            &content,
            &self.session,
            &self.telex_log,
            &self.min_meta,
            &self.history_meta,
            CALLSIGN,
            "test-logon-code",
            false,
        )
        .await;
    }

    fn entries(&self) -> Vec<ThreadEntryDto> {
        thread_entries(&self.telex_log, &self.session, &self.min_meta, &self.history_meta)
    }

    fn entry(&self, text_contains: &str) -> ThreadEntryDto {
        self.entries()
            .into_iter()
            .find(|e| e.text.contains(text_contains))
            .unwrap_or_else(|| panic!("no entry containing {text_contains:?}: {:#?}", self.entries()))
    }

    fn logged_on_to(&self) -> Option<String> {
        let s = self.session.lock().unwrap();
        s.is_logged_on().then(|| s.addressee())
    }

    fn pending_to(&self) -> Option<String> {
        let s = self.session.lock().unwrap();
        s.is_logon_pending().then(|| s.addressee())
    }

    /// Requests the stand-in received with `type=<kind>`, as (to, packet).
    fn sent(&self, kind: &str) -> Vec<(String, String)> {
        self.sent_to_server
            .lock()
            .unwrap()
            .iter()
            .filter(|f| f.get("type").map(String::as_str) == Some(kind))
            .map(|f| (f["to"].clone(), f.get("packet").cloned().unwrap_or_default()))
            .collect()
    }
}

// ---------------------------------------------------------------------
// Replays
// ---------------------------------------------------------------------

/// Flight log 1/RJPZxEvg7Gx1YLnl, 19.09.2026, DLH4TK: Bucharest radar
/// (LRBB) hands over to LRWW. In the field the automatic logon went to
/// "@LRWW", the pilot had to log off and on again by hand, and the
/// CONTACT instruction sent together with the handover was never
/// answered.
#[tokio::test]
async fn real_19_09_lrbb_hands_over_to_lrww() {
    let rig = Rig::new().await;
    rig.logon("LRBB");
    rig.poll(&[
        ("LRBB", "cpdlc", "/data2/5/1/NE/LOGON ACCEPTED"),
        ("LRBB", "cpdlc", "/data2/6//WU/PROCEED DIRECT TO @INVED"),
        ("LRBB", "cpdlc", "/data2/7//NE/CURRENT ATC UNIT@_@LRBB@_@BUCHAREST RADAR"),
    ])
    .await;
    assert_eq!(rig.logged_on_to().as_deref(), Some("LRBB"));
    assert_eq!(*rig.fx.open_session.lock().unwrap(), Some("LRBB".to_string()));
    assert_eq!(rig.wilco("LRBB", 6), Ok(("LRBB".to_string(), Some(6))));

    rig.poll(&[
        ("LRBB", "cpdlc", "/data2/12//WU/CONTACT @LRWW 125.765@_@BUCHAREST RADAR"),
        ("LRBB", "cpdlc", "/data2/13//NE/HANDOVER @LRWW"),
    ])
    .await;
    let logons: Vec<_> = rig.sent("cpdlc").into_iter().filter(|(_, p)| p.contains("REQUEST LOGON")).collect();
    assert_eq!(logons.len(), 1, "exactly one automatic logon: {logons:?}");
    assert_eq!(logons[0].0, "LRWW", "addressed to the station, not to \"@LRWW\"");
    assert_eq!(rig.pending_to().as_deref(), Some("LRWW"));
    // The farewell instruction stays answerable — to LRBB, who sent it.
    assert_eq!(rig.wilco("LRBB", 12), Ok(("LRBB".to_string(), Some(12))));

    let our_logon_min = rig.session.lock().unwrap().pending_logon_min().unwrap();
    rig.poll(&[
        ("LRWW", "cpdlc", &format!("/data2/7/{our_logon_min}/NE/LOGON ACCEPTED")),
        ("LRWW", "cpdlc", "/data2/8//NE/CURRENT ATC UNIT@_@LRWW@_@BUCHAREST RADAR"),
    ])
    .await;
    assert_eq!(rig.logged_on_to().as_deref(), Some("LRWW"));
    assert_eq!(*rig.fx.open_session.lock().unwrap(), Some("LRWW".to_string()));
}

/// Flight log 1/mQD4LeLzE7QavDle, 06.09.2026: LRBB logs the aircraft
/// off and sends its farewell MONITOR instruction AFTER the LOGOFF, in
/// the same poll, then a second LOGOFF.
#[tokio::test]
async fn real_06_09_lrbb_farewell_after_its_logoff() {
    let rig = Rig::new().await;
    let logon = rig.logon("LRBB");
    rig.poll(&[
        ("LRBB", "cpdlc", &format!("/data2/1/{logon}/NE/LOGON ACCEPTED")),
        ("LRBB", "cpdlc", "/data2/2//NE/CURRENT ATC UNIT@_@LRBB@_@BUCHAREST RADAR"),
    ])
    .await;
    assert_eq!(rig.logged_on_to().as_deref(), Some("LRBB"));

    rig.poll(&[
        ("LRBB", "cpdlc", "/data2/9//NE/LOGOFF"),
        (
            "LRBB",
            "cpdlc",
            "/data2/10//WU/NO FURTHER ATC AVAIL MONITOR ADVISORY ON 122.800, THANK YOU FOR USING CPDLC.",
        ),
        ("LRBB", "cpdlc", "/data2/11//NE/CURRENT ATC UNIT@_@LRBB@_@BUCHAREST RADAR"),
    ])
    .await;
    assert_eq!(rig.logged_on_to(), None, "the station ended the session");
    assert_eq!(*rig.fx.open_session.lock().unwrap(), None);
    assert_eq!(
        rig.wilco("LRBB", 10),
        Ok(("LRBB".to_string(), Some(10))),
        "the farewell instruction must be answerable: {:#?}",
        rig.entries()
    );

    rig.poll(&[("LRBB", "cpdlc", "/data2/12//NE/LOGOFF")]).await;
    assert_eq!(rig.logged_on_to(), None);
    assert!(rig.sent("cpdlc").iter().all(|(_, p)| !p.contains("REQUEST LOGON")), "no logon attempt of our own");
}

/// Flight log 1/9PNAQ9MAkAW48NXM, 31.07.2026: Sofia (LBSR) — accept,
/// instruction and unit in ONE poll; later the farewell MONITOR with
/// '@' and '||' decoration; the pilot logs off; the station echoes it.
#[tokio::test]
async fn real_31_07_sofia_session() {
    let rig = Rig::new().await;
    let logon = rig.logon("LBSR");
    rig.poll(&[
        ("LBSR", "cpdlc", &format!("/data2/8/{logon}/NE/LOGON ACCEPTED")),
        ("LBSR", "cpdlc", "/data2/9//WU/PROCEED DIRECT TO @NIKTI"),
        ("LBSR", "cpdlc", "/data2/10//NE/CURRENT ATC UNIT@_@LBSR@_@SOFIA CTR"),
    ])
    .await;
    assert_eq!(rig.logged_on_to().as_deref(), Some("LBSR"));
    assert_eq!(rig.wilco("LBSR", 9), Ok(("LBSR".to_string(), Some(9))));

    rig.poll(&[(
        "LBSR",
        "cpdlc",
        "/data2/19//WU/MONITOR ADVISORY @122.800@. THANKS FOR USING CPDLC, BEST REGARDS @ACC SOFIA@||",
    )])
    .await;
    assert_eq!(rig.wilco("LBSR", 19), Ok(("LBSR".to_string(), Some(19))));
    assert_eq!(rig.logged_on_to().as_deref(), Some("LBSR"), "a MONITOR is not a logoff");
}

/// Flight log 1/Eq980dRXMVkj794n, 29.08.2026: AFRN answers with the
/// unit BEFORE and AFTER the acceptance in one poll, and a request is
/// answered with a WU instruction carrying our MRN.
#[tokio::test]
async fn real_29_08_africa_accept_between_two_unit_messages() {
    let rig = Rig::new().await;
    let logon = rig.logon("AFRN");
    rig.poll(&[
        ("AFRN", "cpdlc", "/data2/74//NE/CURRENT ATC UNIT@_@AFRN@_@AFRICA CTL"),
        ("AFRN", "cpdlc", &format!("/data2/75/{logon}/NE/LOGON ACCEPTED")),
        ("AFRN", "cpdlc", "/data2/76//NE/CURRENT ATC UNIT@_@AFRN@_@AFRICA CTL"),
    ])
    .await;
    assert_eq!(rig.logged_on_to().as_deref(), Some("AFRN"));

    rig.poll(&[("AFRN", "cpdlc", "/data2/96//WU/PROCEED DIRECT TO @GARIN")]).await;
    assert_eq!(rig.wilco("AFRN", 96), Ok(("AFRN".to_string(), Some(96))));

    rig.poll(&[("AFRN", "cpdlc", "/data2/118//NE/LOGOFF")]).await;
    assert_eq!(rig.logged_on_to(), None);
}

/// Flight log 5/D2N6lyMVXXXvwa8X, 23.08.2026: vSMR PDC at EDDF without
/// any CPDLC logon — standby ack and clearance in one poll, WILCO,
/// then CLEARANCE CONFIRMED.
#[tokio::test]
async fn real_23_08_vsmr_pdc_at_eddf() {
    let rig = Rig::new().await;
    rig.pdc("EDDF");
    rig.poll(&[
        (
            "EDDF",
            "cpdlc",
            "/data2/26//NE/FSM 0848 260823 EDDF SWR9PZ@SWR9PZ@ RCD RECEIVED @REQUEST BEING PROCESSED @STANDBY",
        ),
        (
            "EDDF",
            "cpdlc",
            "/data2/27//WU/CLD 0848 260823 EDDF PDC 009 SWR9PZCLRD TO @LSGG@ OFF @18@ VIA @ANEKI2L@ SQUAWK @1000@ NEXT FREQ @121.855@ ATIS @R@ REPORT TOBT AT VATS.IM|VDGS",
        ),
    ])
    .await;
    assert!(!rig.session.lock().unwrap().is_awaiting_pdc_answer(chrono::Utc::now()), "the clearance ended the fast-poll wait");
    assert_eq!(rig.logged_on_to(), None, "a PDC is not a logon");
    assert_eq!(rig.wilco("EDDF", 27), Ok(("EDDF".to_string(), Some(27))));

    rig.poll(&[(
        "EDDF",
        "cpdlc",
        "/data2/28//NE/FSM 0849 260823 EDDF SWR9PZ@SWR9PZ@ CDA RECEIVED @CLEARANCE CONFIRMED",
    )])
    .await;
    assert_eq!(rig.logged_on_to(), None);
}

/// Flight log 1/d1RA9XeY8O7g5RW4, 19.08.2026: LROP's clearance — in the
/// field (older version) the WILCO went to LDZO and the clearance was
/// cancelled. The reply must go to the station that sent the clearance.
#[tokio::test]
async fn real_19_08_lrop_pdc_reply_goes_to_lrop() {
    let rig = Rig::new().await;
    rig.pdc("LROP");
    rig.poll(&[
        (
            "LROP",
            "cpdlc",
            "/data2/1//NE/FSM 0842 260819 LROP WMT4TK@WMT4TK@ RCD RECEIVED @REQUEST BEING PROCESSED @STANDBY",
        ),
        (
            "LROP",
            "cpdlc",
            "/data2/2//WU/CLD 0843 260819 LROP PDC 001 WMT4TKCLRD TO @EDDB@ OFF @08L@ VIA @SOKRU1K@ CLIMB @FL280@ SQUAWK @1000@ NEXT FREQ @121.855@ ATIS REQ STARTUP ON @121.855",
        ),
    ])
    .await;
    assert_eq!(rig.wilco("LROP", 2), Ok(("LROP".to_string(), Some(2))));
}

/// Flight log 4/PQZk23KBgyOqQ0AN, 18.09.2026: EDDM refuses a PDC with
/// "FLIGHT PLAN NOT HELD" — must end the PDC wait, must not touch any
/// logon, and must be shown to the pilot.
#[tokio::test]
async fn real_18_09_eddm_pdc_refused_flight_plan_not_held() {
    let rig = Rig::new().await;
    rig.pdc("EDDM");
    rig.poll(&[(
        "EDDM",
        "cpdlc",
        "/data2/47//NE/FSM 2024 260918 ---- DLH2AS@DLH2AS@ RCD REJECTED @FLIGHT PLAN NOT HELD @REVERT TO VOICE PROCEDURES",
    )])
    .await;
    assert!(!rig.session.lock().unwrap().is_awaiting_pdc_answer(chrono::Utc::now()));
    assert_eq!(rig.logged_on_to(), None);
    assert!(!rig.entry("FLIGHT PLAN NOT HELD").superseded.unwrap_or(false), "shown, not greyed out");
}

/// Flight log 27/jMVADGy661PKakQE, 07.08.2026: Lisboa answers a request
/// with UNABLE (MRN = the request). That is NOT a logon refusal.
#[tokio::test]
async fn real_07_08_lisboa_unable_to_a_request_keeps_the_session() {
    let rig = Rig::new().await;
    let logon = rig.logon("LPZE");
    rig.poll(&[
        ("LPZE", "cpdlc", &format!("/data2/3/{logon}/NE/LOGON ACCEPTED")),
        ("LPZE", "cpdlc", "/data2/4//NE/CURRENT ATC UNIT@_@LPZE@_@LISBOA CTL"),
    ])
    .await;
    let request = {
        let spec = hoppie_protocol::elements::find("DM67").unwrap();
        let resolved = hoppie_protocol::elements::resolve(spec, &["REQUEST DESCENT TO 80".into()]).unwrap();
        let (msg, _) = rig.session.lock().unwrap().thread.record_sent(
            spec.response,
            None,
            resolved.filled_text.clone(),
            hoppie_protocol::elements::ParsedElement::Recognized(resolved),
        );
        msg.min
    };
    rig.poll(&[("LPZE", "cpdlc", &format!("/data2/7/{request}/NE/UNABLE"))]).await;
    assert_eq!(rig.logged_on_to().as_deref(), Some("LPZE"));
}

/// A logon sent twice to the same station (pilot presses again after
/// "no answer") — then the station's answer arrives.
#[tokio::test]
async fn resent_logon_is_still_accepted() {
    let rig = Rig::new().await;
    rig.logon("LRWW");
    let second = rig.logon("LRWW");
    rig.poll(&[("LRWW", "cpdlc", &format!("/data2/7/{second}/NE/LOGON ACCEPTED"))]).await;
    assert_eq!(rig.logged_on_to().as_deref(), Some("LRWW"));
}

/// The farewell exemption is for the station actually in control. A
/// stranger sending LOGOFF plus an instruction in one batch gets
/// nothing: the instruction stays untrusted, not answerable.
#[tokio::test]
async fn a_stranger_cannot_use_a_logoff_to_smuggle_in_an_instruction() {
    let rig = Rig::new().await;
    let logon = rig.logon("LRBB");
    rig.poll(&[("LRBB", "cpdlc", &format!("/data2/1/{logon}/NE/LOGON ACCEPTED"))]).await;
    rig.poll(&[
        ("XXXX", "cpdlc", "/data2/9//NE/LOGOFF"),
        ("XXXX", "cpdlc", "/data2/10//WU/CLIMB TO @FL410"),
    ])
    .await;
    assert_eq!(rig.logged_on_to().as_deref(), Some("LRBB"), "a stranger's LOGOFF ends nothing");
    assert!(rig.wilco("XXXX", 10).is_err(), "and its instruction is not answerable");
    assert!(rig.entry("CLIMB TO").superseded.unwrap_or(false), "shown greyed out");
}

/// Without a session end in the batch, an instruction of the controlling
/// station is ordinary — and a LATER handover (next poll) still
/// supersedes it, exactly as before.
#[tokio::test]
async fn an_instruction_left_open_across_polls_is_still_superseded_by_a_later_handover() {
    let rig = Rig::new().await;
    let logon = rig.logon("LRBB");
    rig.poll(&[
        ("LRBB", "cpdlc", &format!("/data2/5/{logon}/NE/LOGON ACCEPTED")),
        ("LRBB", "cpdlc", "/data2/6//WU/PROCEED DIRECT TO @INVED"),
    ])
    .await;
    rig.poll(&[("LRBB", "cpdlc", "/data2/13//NE/HANDOVER @LRWW")]).await;
    assert_eq!(rig.wilco("LRBB", 6), Err("LRBB MIN 6 is superseded".to_string()));
}

/// External QS (Codex, 28.09.2026) P1: a PDC refusal from the station we
/// are ALSO logging on to must not end the logon. Real packet: EDDM,
/// 18.09.2026 (flight log 4/PQZk23KBgyOqQ0AN).
#[tokio::test]
async fn a_pdc_refusal_does_not_cancel_a_logon_to_the_same_station() {
    let rig = Rig::new().await;
    let logon = rig.logon("EDDM");
    rig.pdc("EDDM");
    rig.poll(&[(
        "EDDM",
        "cpdlc",
        "/data2/47//NE/FSM 2024 260918 ---- DLH2AS@DLH2AS@ RCD REJECTED @FLIGHT PLAN NOT HELD @REVERT TO VOICE PROCEDURES",
    )])
    .await;
    assert_eq!(rig.pending_to().as_deref(), Some("EDDM"), "the logon is still pending");
    rig.poll(&[("EDDM", "cpdlc", &format!("/data2/48/{logon}/NE/LOGON ACCEPTED"))]).await;
    assert_eq!(rig.logged_on_to().as_deref(), Some("EDDM"));
}

/// External QS (Codex, 28.09.2026) P1: acceptance, farewell instruction
/// and handover all in the FIRST poll after the logon. Built from the
/// real 19.09. packets.
#[tokio::test]
async fn accept_farewell_and_handover_in_one_poll() {
    let rig = Rig::new().await;
    let logon = rig.logon("LRBB");
    rig.poll(&[
        ("LRBB", "cpdlc", &format!("/data2/5/{logon}/NE/LOGON ACCEPTED")),
        ("LRBB", "cpdlc", "/data2/12//WU/CONTACT @LRWW 125.765@_@BUCHAREST RADAR"),
        ("LRBB", "cpdlc", "/data2/13//NE/HANDOVER @LRWW"),
    ])
    .await;
    assert_eq!(rig.pending_to().as_deref(), Some("LRWW"));
    assert_eq!(rig.wilco("LRBB", 12), Ok(("LRBB".to_string(), Some(12))));
}

/// …but a station we asked for a logon that REFUSES and hands over in
/// the same poll has no farewell rights.
#[tokio::test]
async fn a_refusing_pending_station_gets_no_farewell_rights() {
    let rig = Rig::new().await;
    let logon = rig.logon("LRBB");
    rig.poll(&[
        ("LRBB", "cpdlc", &format!("/data2/5/{logon}/NE/UNABLE")),
        ("LRBB", "cpdlc", "/data2/12//WU/CLIMB TO @FL410"),
        ("LRBB", "cpdlc", "/data2/13//NE/LOGOFF"),
    ])
    .await;
    assert_eq!(rig.logged_on_to(), None);
    assert!(rig.wilco("LRBB", 12).is_err());
}
