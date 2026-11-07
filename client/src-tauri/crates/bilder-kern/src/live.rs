//! Aufbau der Anfrage an `POST /api/bilder` und `GET /api/bilder/status`
//! (P-7.25 „Vertrag“, `recorder/src/bilder.ts`).
//!
//! Felder in dieser Reihenfolge: `sendung_id`, `geo_stufe`, je Bild die Datei
//! `bild:<bild_id>` und das JSON `meta:<bild_id>`, zuletzt `text` — der
//! Pilotentext steht hinten, damit er die Spoiler davor nicht aufbrechen kann.
//! `bild_id` und `sendung_id` sind UUIDs (der Server prueft das). Der Kern
//! baut die Anfrage nur und deutet die Antwort nicht; Grenzen wie „hoechstens 5
//! Bilder“, „500 Zeichen“, „4 MB“ prueft der Server und stehen als Wert im Paket.

use serde_json::Value;

use crate::speicher::ist_jpeg;
use crate::Fehler;

#[derive(Debug, Clone, PartialEq)]
pub struct Teil {
    pub name: String,
    pub dateiname: Option<String>,
    pub content_type: Option<&'static str>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LiveBild {
    pub bild_id: String,
    pub jpeg: Vec<u8>,
    pub meta: Value,
}

/// Kanonische Schreibweise 8-4-4-4-12 (Hex, beliebige Gross-/Kleinschreibung).
pub fn ist_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

pub fn senden_teile(sendung_id: &str, text: Option<&str>, geo_stufe: &str, bilder: &[LiveBild]) -> Result<Vec<Teil>, Fehler> {
    if !ist_uuid(sendung_id) {
        return Err(Fehler::Eingabe("sendung_id muss eine UUID sein".into()));
    }
    if geo_stufe.is_empty() || geo_stufe.len() > 32 || !geo_stufe.bytes().all(|c| c.is_ascii_lowercase() || c == b'_') {
        return Err(Fehler::Eingabe("geo_stufe unbrauchbar".into()));
    }
    if bilder.is_empty() {
        return Err(Fehler::Eingabe("Keine Bilder zum Senden".into()));
    }
    let mut teile = vec![
        Teil { name: "sendung_id".into(), dateiname: None, content_type: None, bytes: sendung_id.as_bytes().to_vec() },
        Teil { name: "geo_stufe".into(), dateiname: None, content_type: None, bytes: geo_stufe.as_bytes().to_vec() },
    ];
    let mut gesehen: Vec<&str> = Vec::new();
    for b in bilder {
        if !ist_uuid(&b.bild_id) {
            return Err(Fehler::Eingabe("bild_id muss eine UUID sein".into()));
        }
        if gesehen.contains(&b.bild_id.as_str()) {
            return Err(Fehler::Eingabe("bild_id kommt doppelt vor".into()));
        }
        gesehen.push(&b.bild_id);
        if !ist_jpeg(&b.jpeg) {
            return Err(Fehler::Format(format!("Bild {} ist kein JPEG", b.bild_id)));
        }
        if !b.meta.is_object() {
            return Err(Fehler::Eingabe(format!("meta zu Bild {} muss ein JSON-Objekt sein", b.bild_id)));
        }
        teile.push(Teil {
            name: format!("bild:{}", b.bild_id),
            dateiname: Some(format!("{}.jpg", b.bild_id)),
            content_type: Some("image/jpeg"),
            bytes: b.jpeg.clone(),
        });
        teile.push(Teil {
            name: format!("meta:{}", b.bild_id),
            dateiname: Some(format!("{}.json", b.bild_id)),
            content_type: Some("application/json"),
            bytes: serde_json::to_vec(&b.meta).map_err(|e| Fehler::Eingabe(e.to_string()))?,
        });
    }
    if let Some(t) = text.filter(|t| !t.is_empty()) {
        teile.push(Teil { name: "text".into(), dateiname: None, content_type: None, bytes: t.as_bytes().to_vec() });
    }
    Ok(teile)
}

fn kommt_vor(heu: &[u8], nadel: &[u8]) -> bool {
    !nadel.is_empty() && heu.windows(nadel.len()).any(|w| w == nadel)
}

/// Eine Grenze, die in keinem Teil vorkommt.
pub fn grenze_waehlen(teile: &[Teil]) -> String {
    loop {
        let g = format!("----AeroACARS{}", uuid::Uuid::new_v4().simple());
        let n = format!("--{g}");
        if !teile.iter().any(|t| kommt_vor(&t.bytes, n.as_bytes())) {
            return g;
        }
    }
}

pub fn multipart_bauen(grenze: &str, teile: &[Teil]) -> Vec<u8> {
    let mut k = Vec::new();
    for t in teile {
        k.extend_from_slice(format!("--{grenze}\r\n").as_bytes());
        let mut d = format!("Content-Disposition: form-data; name=\"{}\"", t.name);
        if let Some(f) = &t.dateiname {
            d.push_str(&format!("; filename=\"{f}\""));
        }
        k.extend_from_slice(d.as_bytes());
        k.extend_from_slice(b"\r\n");
        if let Some(ct) = t.content_type {
            k.extend_from_slice(format!("Content-Type: {ct}\r\n").as_bytes());
        }
        k.extend_from_slice(b"\r\n");
        k.extend_from_slice(&t.bytes);
        k.extend_from_slice(b"\r\n");
    }
    k.extend_from_slice(format!("--{grenze}--\r\n").as_bytes());
    k
}

pub fn senden_url(basis: &str) -> String {
    format!("{}/api/bilder", basis.trim_end_matches('/'))
}

pub fn status_url(basis: &str, sendung_id: Option<&str>) -> Result<String, Fehler> {
    let u = format!("{}/api/bilder/status", basis.trim_end_matches('/'));
    match sendung_id {
        None => Ok(u),
        Some(s) if ist_uuid(s) => Ok(format!("{u}?sendung_id={s}")),
        Some(_) => Err(Fehler::Eingabe("sendung_id muss eine UUID sein".into())),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) const S: &str = "11111111-2222-4333-8444-555555555555";
    pub(crate) const B1: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    pub(crate) const B2: &str = "99999999-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    fn jpeg(n: u8) -> Vec<u8> {
        vec![0xFF, 0xD8, 0xFF, 0xE0, n, 0xFF, 0xD9]
    }

    fn bilder() -> Vec<LiveBild> {
        vec![
            LiveBild { bild_id: B1.into(), jpeg: jpeg(1), meta: json!({"fassung": 1, "breite": 50.1}) },
            LiveBild { bild_id: B2.into(), jpeg: jpeg(2), meta: json!({"fassung": 1}) },
        ]
    }

    /// Zerlegt einen Multipart-Koerper unabhaengig vom Aufbau (nur Test).
    type Zerlegt = Vec<(String, Option<String>, Option<String>, Vec<u8>)>;

    pub(crate) fn zerlegen(grenze: &str, body: &[u8]) -> Zerlegt {
        let trenn = format!("--{grenze}");
        let text = body;
        let mut teile = Vec::new();
        let mut pos = 0usize;
        let find = |von: usize, n: &[u8]| text[von..].windows(n.len()).position(|w| w == n).map(|p| p + von);
        assert!(text.starts_with(trenn.as_bytes()));
        loop {
            let start = pos + trenn.len();
            if text[start..].starts_with(b"--\r\n") {
                assert_eq!(start + 4, text.len(), "nichts nach dem Abschluss");
                return teile;
            }
            assert!(text[start..].starts_with(b"\r\n"));
            let kopf_ende = find(start + 2, b"\r\n\r\n").unwrap();
            let kopf = String::from_utf8(text[start + 2..kopf_ende].to_vec()).unwrap();
            let naechste = find(kopf_ende + 4, format!("\r\n{trenn}").as_bytes()).unwrap();
            let inhalt = text[kopf_ende + 4..naechste].to_vec();
            let mut name = None;
            let mut datei = None;
            let mut ct = None;
            for z in kopf.split("\r\n") {
                if let Some(rest) = z.strip_prefix("Content-Disposition: form-data; ") {
                    for p in rest.split("; ") {
                        if let Some(v) = p.strip_prefix("name=\"") {
                            name = Some(v.trim_end_matches('"').to_string());
                        }
                        if let Some(v) = p.strip_prefix("filename=\"") {
                            datei = Some(v.trim_end_matches('"').to_string());
                        }
                    }
                } else if let Some(v) = z.strip_prefix("Content-Type: ") {
                    ct = Some(v.to_string());
                }
            }
            teile.push((name.unwrap(), datei, ct, inhalt));
            pos = naechste + 2;
        }
    }

    #[test]
    fn aufbau_entspricht_dem_vertrag() {
        let teile = senden_teile(S, Some("Schoener Anflug ||x||"), "verdeckt", &bilder()).unwrap();
        let g = grenze_waehlen(&teile);
        let body = multipart_bauen(&g, &teile);
        let z = zerlegen(&g, &body);
        let namen: Vec<&str> = z.iter().map(|t| t.0.as_str()).collect();
        assert_eq!(
            namen,
            ["sendung_id", "geo_stufe", &format!("bild:{B1}"), &format!("meta:{B1}"), &format!("bild:{B2}"), &format!("meta:{B2}"), "text"],
            "Reihenfolge laut Vertrag, Pilotentext zuletzt"
        );
        assert_eq!(z[0].3, S.as_bytes());
        assert_eq!(z[1].3, b"verdeckt");
        // Dateien haben Dateinamen und JPEG-Typ, Texte nicht
        assert_eq!((z[2].1.as_deref(), z[2].2.as_deref()), (Some(&*format!("{B1}.jpg")), Some("image/jpeg")));
        assert_eq!(z[2].3, jpeg(1));
        assert_eq!(z[3].2.as_deref(), Some("application/json"));
        assert_eq!(serde_json::from_slice::<Value>(&z[3].3).unwrap(), json!({"fassung": 1, "breite": 50.1}));
        assert_eq!((z[0].1.as_ref(), z[0].2.as_ref(), z[6].1.as_ref()), (None, None, None));
        assert_eq!(z[6].3, "Schoener Anflug ||x||".as_bytes());
    }

    #[test]
    fn text_fehlt_wenn_leer_und_umlaute_bleiben_utf8() {
        let teile = senden_teile(S, Some(""), "aus", &bilder()[..1]).unwrap();
        assert!(!teile.iter().any(|t| t.name == "text"));
        let teile = senden_teile(S, Some("Gr\u{fc}\u{df}e \u{2708}"), "sichtbar", &bilder()[..1]).unwrap();
        assert_eq!(teile.last().unwrap().bytes, "Gr\u{fc}\u{df}e \u{2708}".as_bytes());
    }

    #[test]
    fn grenze_kommt_im_inhalt_nie_vor() {
        let mut b = bilder();
        // Ein Bild, das eine „Grenze“ nachahmt, darf die Wahl nicht stoeren
        b[0].jpeg.extend_from_slice(b"\r\n------AeroACARS0000\r\n");
        let teile = senden_teile(S, None, "verdeckt", &b).unwrap();
        let g = grenze_waehlen(&teile);
        let body = multipart_bauen(&g, &teile);
        let n = format!("--{g}");
        let mal = body.windows(n.len()).filter(|w| *w == n.as_bytes()).count();
        assert_eq!(mal, teile.len() + 1, "eine Grenze je Teil plus Abschluss, nicht im Inhalt");
    }

    #[test]
    fn ungueltiges_scheitert_bevor_gesendet_wird() {
        let ok = bilder();
        assert!(senden_teile("keine-uuid", None, "verdeckt", &ok).is_err());
        assert!(senden_teile(S, None, "", &ok).is_err());
        assert!(senden_teile(S, None, "Verdeckt\r\n", &ok).is_err());
        assert!(senden_teile(S, None, "verdeckt", &[]).is_err());
        let mut b = bilder();
        b[0].bild_id = "../x".into();
        assert!(senden_teile(S, None, "verdeckt", &b).is_err());
        let mut b = bilder();
        b[1].bild_id = B1.into();
        assert!(senden_teile(S, None, "verdeckt", &b).is_err(), "doppelte bild_id");
        let mut b = bilder();
        b[0].jpeg = b"PNG".to_vec();
        assert!(matches!(senden_teile(S, None, "verdeckt", &b), Err(Fehler::Format(_))));
        let mut b = bilder();
        b[0].meta = json!([1]);
        assert!(senden_teile(S, None, "verdeckt", &b).is_err());
    }

    #[test]
    fn adressen() {
        assert_eq!(senden_url("https://live.example/"), "https://live.example/api/bilder");
        assert_eq!(status_url("https://live.example", None).unwrap(), "https://live.example/api/bilder/status");
        assert_eq!(status_url("https://live.example", Some(S)).unwrap(), format!("https://live.example/api/bilder/status?sendung_id={S}"));
        assert!(status_url("https://live.example", Some("x&y=1")).is_err());
    }
}
