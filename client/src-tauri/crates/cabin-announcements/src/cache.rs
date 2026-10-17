//! Audio-Cache: derselbe Text mit derselben Stimme wird nie zweimal
//! bezahlt. Der Schlüssel ist ein FNV-1a-Hash (stabil über Rust-Versionen,
//! anders als `DefaultHasher`) aus Anbieter, Stimme, Sprache und Text.

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(daten: &[u8], mut h: u64) -> u64 {
    for b in daten {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Dateiname für die Audiodatei, z. B. "azure-3f2a…c1.mp3".
/// Leerraum am Rand und mehrfache Leerzeichen zählen nicht als Unterschied.
pub fn dateiname(anbieter: &str, stimme: &str, sprache: &str, text: &str, endung: &str) -> String {
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut h = FNV_OFFSET;
    for teil in [anbieter, stimme, sprache, text.as_str()] {
        h = fnv1a(teil.as_bytes(), h);
        h = fnv1a(&[0x1f], h); // Trenner, damit "ab"+"c" ≠ "a"+"bc"
    }
    let anbieter: String = anbieter
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    format!("{anbieter}-{h:016x}.{endung}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gleich_bleibt_gleich_leerraum_egal() {
        let a = dateiname("azure", "de-CH-JanNeural", "de-CH", "Guten  Abend ", "mp3");
        let b = dateiname("azure", "de-CH-JanNeural", "de-CH", " Guten Abend", "mp3");
        assert_eq!(a, b);
        assert!(a.starts_with("azure-") && a.ends_with(".mp3"));
    }

    #[test]
    fn stimme_text_und_trenner_machen_den_unterschied() {
        let basis = dateiname("azure", "de-CH-JanNeural", "de-CH", "Hallo", "mp3");
        assert_ne!(basis, dateiname("azure", "de-AT-IngridNeural", "de-CH", "Hallo", "mp3"));
        assert_ne!(basis, dateiname("azure", "de-CH-JanNeural", "de-CH", "Hallo!", "mp3"));
        assert_ne!(dateiname("ab", "c", "", "", "x"), dateiname("a", "bc", "", "", "x"));
    }

    #[test]
    fn bekannter_wert_bleibt_stabil() {
        // Fester Wert: ändert er sich, sind alle vorhandenen Cache-Dateien
        // verwaist — dann bewusst entscheiden, nicht nebenbei.
        assert_eq!(
            dateiname("piper", "de_DE-thorsten-high", "de-DE", "Guten Morgen", "wav"),
            "piper-833a5f35159f503b.wav"
        );
    }
}
