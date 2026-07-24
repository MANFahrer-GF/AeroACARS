//! Black-box (public-API-only) decode/encode tests, complementing the
//! inline `#[cfg(test)]` unit tests in `src/cpdlc.rs`.

use hoppie_protocol::cpdlc::{decode, encode, CpdlcMessage, ResponseRequirement};
use hoppie_protocol::elements::{Direction, ParsedElement};

#[test]
fn worked_example_from_the_community_docs_round_trips_through_the_public_api() {
    // devHazz/hoppie-acars-docs, Messaging/CPDLC Format.md: LRBL -> SWR160.
    let msg = decode("/data2/3//WU/PROCEED DIRECT TO @UDROS", Direction::Uplink)
        .expect("worked example must decode");
    assert_eq!(msg.min, 3);
    assert_eq!(msg.mrn, None);
    assert_eq!(msg.response, ResponseRequirement::WilcoUnable);
    assert!(matches!(msg.parsed, ParsedElement::Recognized(_)));

    // Our own re-encode of the same fields (with the '@' stripped, since
    // it never means anything to us — see elements.rs docs) reproduces
    // an equivalent wire packet.
    let reencoded = encode(&CpdlcMessage {
        element_text: "PROCEED DIRECT TO UDROS".to_string(),
        ..msg
    });
    assert_eq!(reencoded, "/data2/3//WU/PROCEED DIRECT TO UDROS");
}

#[test]
fn worked_reply_from_the_community_docs_round_trips_through_the_public_api() {
    let msg =
        decode("/data2/8/3/N/WILCO", Direction::Downlink).expect("worked reply must decode");
    assert_eq!(msg.min, 8);
    assert_eq!(msg.mrn, Some(3));
    assert_eq!(encode(&msg), "/data2/8/3/N/WILCO");
}
