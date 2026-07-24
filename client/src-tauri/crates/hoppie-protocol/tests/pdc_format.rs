//! Black-box PDC formatting test, replaying the recorded example
//! fixture through the public API.

use hoppie_protocol::pdc::{format_pdc_request, parse_pdc_reply, PdcRequest};

fn parse_fixture(text: &str) -> (PdcRequest, String) {
    let mut request = None;
    let mut reply = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("REQUEST|") {
            let f: Vec<&str> = rest.split('|').collect();
            request = Some(PdcRequest {
                recipient: f[0].to_string(),
                callsign: f[1].to_string(),
                aircraft_type: f[2].to_string(),
                dep_icao: f[3].to_string(),
                dest_icao: f[4].to_string(),
                stand: f[5].to_string(),
                atis_letter: f[6].to_string(),
            });
        } else if let Some(rest) = line.strip_prefix("REPLY|") {
            reply = Some(rest.to_string());
        }
    }
    (request.expect("fixture must have a REQUEST line"), reply.expect("fixture must have a REPLY line"))
}

#[test]
fn recorded_pdc_exchange_formats_and_parses_via_the_public_api() {
    let (req, reply_text) = parse_fixture(include_str!("fixtures/pdc_request_reply.txt"));

    let formatted = format_pdc_request(&req);
    assert_eq!(
        formatted,
        "REQUEST PREDEP CLEARANCE GSG123 A320 TO EDDM AT EDDF STAND A12 ATIS K"
    );

    let reply = parse_pdc_reply(&reply_text);
    assert_eq!(reply.raw_text, reply_text.trim());
    assert!(reply.raw_text.contains("GSG123"));
}
