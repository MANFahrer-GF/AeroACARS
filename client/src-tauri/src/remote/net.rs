//! Network helpers for the LAN remote-control server: the private-peer
//! gate, the candidate-URL builder, and the QR-SVG renderer.
//!
//! All three are pure (no I/O beyond `if-addrs` interface enumeration),
//! so they are unit-tested directly without spinning up the server.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// True iff `ip` is an address we are willing to accept a remote-control
/// connection FROM. The server controls a real PIREP, so we hard-restrict
/// to the local machine + the private LAN ranges:
///
/// - loopback (`127.0.0.0/8`, `::1`) — the desktop app's own browser,
/// - RFC1918 (`10/8`, `172.16/12`, `192.168/16`) — typical home/office LAN,
/// - link-local (`169.254/16`, `fe80::/10`) — APIPA / direct-cable setups,
/// - unique-local IPv6 (`fc00::/7`) — IPv6 private range.
///
/// Everything else (public/global addresses) is rejected at the socket
/// before any auth check, so the server can never be reached from the
/// internet even if a port-forward is misconfigured.
pub fn is_private_peer(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_v4(v4),
        IpAddr::V6(v6) => is_private_v6(v6),
    }
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()        // 127.0.0.0/8
        || ip.is_private()  // 10/8, 172.16/12, 192.168/16
        || ip.is_link_local() // 169.254.0.0/16 (APIPA)
}

fn is_private_v6(ip: Ipv6Addr) -> bool {
    // IPv4-mapped (`::ffff:a.b.c.d`) peers can appear on dual-stack
    // sockets; unwrap and apply the v4 rule so a LAN IPv4 client reaching
    // us over a v6 listener is not falsely rejected.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_private_v4(v4);
    }
    if ip.is_loopback() {
        return true; // ::1
    }
    let seg = ip.segments();
    // fe80::/10 link-local.
    let link_local = (seg[0] & 0xffc0) == 0xfe80;
    // fc00::/7 unique-local (ULA) — covers fc00::/8 and fd00::/8.
    let unique_local = (seg[0] & 0xfe00) == 0xfc00;
    link_local || unique_local
}

/// True iff a `SocketAddr` peer is on the private LAN / loopback.
pub fn is_private_socket(addr: SocketAddr) -> bool {
    is_private_peer(addr.ip())
}

/// Build the `http://<ip>:<port>` connect URLs the settings panel shows
/// and the QR encodes. One entry per private (RFC1918 / link-local /
/// loopback) IPv4 interface, deduped, with loopback sorted LAST so the
/// first entry is a real LAN address a tablet can actually reach.
///
/// IPv6 is intentionally omitted from the *displayed* URLs: most tablets
/// pair via a typed/scanned IPv4 literal, and a bracketed v6 URL is both
/// fragile and confusing. The server still *accepts* private v6 peers
/// (see [`is_private_peer`]); this only governs what we advertise.
pub fn lan_urls(port: u16) -> Vec<String> {
    let mut loopback: Vec<String> = Vec::new();
    let mut lan: Vec<String> = Vec::new();

    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            let ip = iface.ip();
            // Only advertise IPv4 (see doc comment).
            let IpAddr::V4(v4) = ip else { continue };
            if !is_private_v4(v4) {
                continue;
            }
            let url = format!("http://{v4}:{port}");
            if v4.is_loopback() {
                if !loopback.contains(&url) {
                    loopback.push(url);
                }
            } else if !lan.contains(&url) {
                lan.push(url);
            }
        }
    }

    // Real LAN addresses first; loopback as a last-resort fallback (e.g.
    // when the host is offline and only `127.0.0.1` exists).
    lan.append(&mut loopback);
    lan
}

/// Render `target` (the primary connect URL + `?pin=`) to a scannable QR
/// code as an inline `<svg>` `data:` URL the React `<img>` can use
/// directly. On any encode failure (e.g. a URL too long for the largest
/// QR version — not possible for our short LAN URLs) returns an empty
/// string; the panel falls back to showing the URL + PIN as text.
pub fn qr_svg(target: &str) -> String {
    use qrcode::render::svg;
    use qrcode::{EcLevel, QrCode};

    let Ok(code) = QrCode::with_error_correction_level(target, EcLevel::M) else {
        return String::new();
    };
    let svg_xml = code
        .render::<svg::Color>()
        .min_dimensions(220, 220)
        .quiet_zone(true)
        .dark_color(svg::Color("#0f172a")) // slate-900 — matches the app theme
        .light_color(svg::Color("#ffffff"))
        .build();

    // Inline as a UTF-8 data URL (no base64 needed for text/SVG).
    let encoded = utf8_percent_encode_svg(&svg_xml);
    format!("data:image/svg+xml;charset=utf-8,{encoded}")
}

/// Minimal percent-encoding for inlining SVG markup into a `data:` URL.
/// We only need to escape the handful of characters that break the URL
/// (`#`, `%`, `<`, `>`, `"`, and raw whitespace newlines); everything
/// else in our generated SVG is ASCII-safe.
fn utf8_percent_encode_svg(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len() + svg.len() / 8);
    for ch in svg.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '"' => out.push_str("%22"),
            '\n' | '\r' => {} // strip line breaks — they add no value inline
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).unwrap())
    }
    fn v6(s: &str) -> IpAddr {
        IpAddr::V6(Ipv6Addr::from_str(s).unwrap())
    }

    #[test]
    fn accepts_rfc1918_v4() {
        assert!(is_private_peer(v4("10.0.0.5")));
        assert!(is_private_peer(v4("172.16.4.9")));
        assert!(is_private_peer(v4("172.31.255.1")));
        assert!(is_private_peer(v4("192.168.1.42")));
    }

    #[test]
    fn accepts_loopback_and_link_local_v4() {
        assert!(is_private_peer(v4("127.0.0.1")));
        assert!(is_private_peer(v4("169.254.10.10")));
    }

    #[test]
    fn rejects_public_v4() {
        assert!(!is_private_peer(v4("8.8.8.8")));
        assert!(!is_private_peer(v4("1.1.1.1")));
        // 172.32 is OUTSIDE the 172.16/12 private block.
        assert!(!is_private_peer(v4("172.32.0.1")));
        // 192.169 is OUTSIDE 192.168/16.
        assert!(!is_private_peer(v4("192.169.0.1")));
    }

    #[test]
    fn handles_v6_ranges() {
        assert!(is_private_peer(v6("::1"))); // loopback
        assert!(is_private_peer(v6("fe80::1"))); // link-local
        assert!(is_private_peer(v6("fc00::1"))); // ULA
        assert!(is_private_peer(v6("fd12:3456::1"))); // ULA
        assert!(!is_private_peer(v6("2606:4700::1"))); // public (Cloudflare)
    }

    #[test]
    fn v4_mapped_v6_uses_v4_rule() {
        // ::ffff:192.168.1.5 → private; ::ffff:8.8.8.8 → public.
        assert!(is_private_peer(v6("::ffff:192.168.1.5")));
        assert!(!is_private_peer(v6("::ffff:8.8.8.8")));
    }

    #[test]
    fn is_private_socket_delegates_to_ip() {
        let priv_addr = SocketAddr::from_str("192.168.0.10:8765").unwrap();
        let pub_addr = SocketAddr::from_str("8.8.8.8:8765").unwrap();
        assert!(is_private_socket(priv_addr));
        assert!(!is_private_socket(pub_addr));
    }

    #[test]
    fn qr_svg_produces_data_url() {
        let url = qr_svg("http://192.168.1.10:8765/?pin=123456");
        assert!(url.starts_with("data:image/svg+xml;charset=utf-8,"));
        // The QR module is escaped, so the raw '<' of "<svg" became "%3C".
        assert!(url.contains("%3Csvg"));
        // No raw newlines survive the inline encoding.
        assert!(!url.contains('\n'));
    }
}
