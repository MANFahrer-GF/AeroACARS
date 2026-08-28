//! RREF UDP protocol encode/decode.
//!
//! Packet format (X-Plane 10/11/12, identical):
//!
//! ## Subscription request (we → X-Plane:49000)
//!
//! ```text
//! offset  size  field
//! 0       4     ASCII "RREF"
//! 4       1     null padding
//! 5       4     freq    (i32 little-endian, Hz; 0 to unsubscribe)
//! 9       4     index   (i32 little-endian, our chosen handle)
//! 13      400   dataref name (ASCII, NUL-terminated, padded with NUL)
//! ```
//!
//! Total = 413 bytes per request.
//!
//! ## Response stream (X-Plane → our local port)
//!
//! ```text
//! offset  size  field
//! 0       4     ASCII "RREF"
//! 4       1     null
//! 5..     N×8   index (i32 LE) + value (f32 LE) pairs
//! ```
//!
//! One UDP packet may carry many `(index, value)` pairs to amortise
//! header overhead.

/// Wire size of an RREF request packet.
pub const RREF_REQUEST_SIZE: usize = 413;

/// Wire size of one (index, value) pair in a response.
pub const RREF_PAIR_SIZE: usize = 8;

/// Header magic for both directions.
pub const RREF_MAGIC: &[u8; 4] = b"RREF";

/// Build an RREF subscription request. Pass `freq = 0` to unsubscribe.
///
/// Panics if `dataref` is longer than 399 bytes (1 reserved for the
/// terminating NUL). DataRef names are short by convention (~40-60
/// chars), so this is a programmer-error guard rather than a runtime
/// concern.
pub fn encode_request(freq: i32, index: i32, dataref: &str) -> Vec<u8> {
    let mut buf = vec![0u8; RREF_REQUEST_SIZE];
    buf[0..4].copy_from_slice(RREF_MAGIC);
    // buf[4] stays 0 (NUL padding)
    buf[5..9].copy_from_slice(&freq.to_le_bytes());
    buf[9..13].copy_from_slice(&index.to_le_bytes());
    let name_bytes = dataref.as_bytes();
    assert!(name_bytes.len() < 400, "dataref name too long: {dataref}");
    buf[13..13 + name_bytes.len()].copy_from_slice(name_bytes);
    // Remainder of the 400-byte name field stays 0 (NUL padding).
    buf
}

/// One decoded `(index, value)` pair from an RREF response.
#[derive(Debug, Clone, Copy)]
pub struct RrefValue {
    pub index: i32,
    pub value: f32,
}

/// Decode an RREF response packet into its (index, value) pairs.
/// Returns an empty Vec if the packet is too short or doesn't carry
/// the expected magic — bad packets are dropped silently with a
/// trace-level log so a misconfigured X-Plane (e.g. sending the old
/// "DATA" data-set output instead of RREF) doesn't spam at warn.
pub fn decode_response(bytes: &[u8]) -> Vec<RrefValue> {
    if bytes.len() < 5 {
        tracing::trace!(len = bytes.len(), "RREF packet too short");
        return Vec::new();
    }
    if &bytes[0..4] != RREF_MAGIC {
        tracing::trace!(magic = ?&bytes[0..4.min(bytes.len())], "non-RREF packet ignored");
        return Vec::new();
    }
    let payload = &bytes[5..];
    let pair_count = payload.len() / RREF_PAIR_SIZE;
    let mut out = Vec::with_capacity(pair_count);
    for i in 0..pair_count {
        let off = i * RREF_PAIR_SIZE;
        let index = i32::from_le_bytes([
            payload[off],
            payload[off + 1],
            payload[off + 2],
            payload[off + 3],
        ]);
        let value = f32::from_le_bytes([
            payload[off + 4],
            payload[off + 5],
            payload[off + 6],
            payload[off + 7],
        ]);
        out.push(RrefValue { index, value });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip_short_dataref() {
        let pkt = encode_request(50, 7, "sim/flightmodel/position/vh_ind_fpm");
        assert_eq!(pkt.len(), RREF_REQUEST_SIZE);
        assert_eq!(&pkt[0..4], b"RREF");
        assert_eq!(pkt[4], 0);
        assert_eq!(i32::from_le_bytes(pkt[5..9].try_into().unwrap()), 50);
        assert_eq!(i32::from_le_bytes(pkt[9..13].try_into().unwrap()), 7);
        // Name: ASCII bytes, NUL-padded.
        let raw_name = &pkt[13..];
        let nul_pos = raw_name.iter().position(|&b| b == 0).unwrap();
        let name = std::str::from_utf8(&raw_name[..nul_pos]).unwrap();
        assert_eq!(name, "sim/flightmodel/position/vh_ind_fpm");
    }

    #[test]
    fn unsubscribe_uses_zero_freq() {
        let pkt = encode_request(0, 3, "sim/foo");
        assert_eq!(i32::from_le_bytes(pkt[5..9].try_into().unwrap()), 0);
    }

    #[test]
    fn decode_single_pair() {
        // Manually craft: "RREF" + NUL + (index=42 LE) + (value=-91.5 LE)
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RREF");
        buf.push(0);
        buf.extend_from_slice(&42_i32.to_le_bytes());
        buf.extend_from_slice(&(-91.5_f32).to_le_bytes());
        let pairs = decode_response(&buf);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].index, 42);
        assert!((pairs[0].value - (-91.5)).abs() < 1e-6);
    }

    #[test]
    fn decode_multiple_pairs_in_one_packet() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RREF");
        buf.push(0);
        for &(i, v) in &[(1_i32, 1.0_f32), (2, 2.0), (3, 3.0)] {
            buf.extend_from_slice(&i.to_le_bytes());
            buf.extend_from_slice(&v.to_le_bytes());
        }
        let pairs = decode_response(&buf);
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[2].index, 3);
        assert!((pairs[2].value - 3.0).abs() < 1e-6);
    }

    #[test]
    fn decode_rejects_non_rref_packets() {
        let buf = b"DATA\0\x00\x00\x00\x00\x00\x00\x00\x00";
        let pairs = decode_response(buf);
        assert!(pairs.is_empty());
    }

    #[test]
    fn decode_handles_truncated_packets() {
        // Header + partial pair (4 bytes index, 0 bytes value) → no
        // complete pair → empty result.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RREF");
        buf.push(0);
        buf.extend_from_slice(&42_i32.to_le_bytes());
        let pairs = decode_response(&buf);
        assert!(pairs.is_empty());
    }
}
