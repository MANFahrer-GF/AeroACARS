//! SSML für Azure. Der Fehler von Universal Announcer (Issue #116: immer
//! `xml:lang="en-US"`, Vorlagentext escaped vorgelesen) wird hier vermieden:
//! `xml:lang` ist immer das Locale der Stimme, der Text wird sauber escaped.

/// Escaped `& < > " '` für XML-Text und -Attribute.
pub fn escape(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\'' => s.push_str("&apos;"),
            _ => s.push(c),
        }
    }
    s
}

/// Locale aus dem Azure-Stimmennamen: "de-CH-JanNeural" → "de-CH".
pub fn locale_aus_stimme(stimme: &str) -> Option<&str> {
    let mut teile = stimme.splitn(3, '-');
    let sprache = teile.next()?;
    let land = teile.next()?;
    if sprache.len() == 2 && land.len() == 2 && teile.next().is_some() {
        Some(&stimme[..5])
    } else {
        None
    }
}

/// Baut das Azure-SSML für einen Text. `tempo` z. B. "-5%" (leer = normal).
pub fn azure(text: &str, stimme: &str, tempo: &str) -> Option<String> {
    let locale = locale_aus_stimme(stimme)?;
    let inhalt = escape(text.trim());
    let inhalt = if tempo.is_empty() {
        inhalt
    } else {
        format!("<prosody rate=\"{}\">{inhalt}</prosody>", escape(tempo))
    };
    Some(format!(
        "<speak version=\"1.0\" xmlns=\"http://www.w3.org/2001/10/synthesis\" xml:lang=\"{locale}\">\
         <voice name=\"{}\">{inhalt}</voice></speak>",
        escape(stimme)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_kommt_aus_der_stimme_nicht_fest_en_us() {
        let s = azure("Grüezi mitenand", "de-CH-JanNeural", "").unwrap();
        assert!(s.contains("xml:lang=\"de-CH\""));
        assert!(s.contains("<voice name=\"de-CH-JanNeural\">Grüezi mitenand</voice>"));
        assert!(!s.contains("en-US"));
    }

    #[test]
    fn text_wird_escaped_nicht_als_ssml_gelesen() {
        let s = azure("Fish & Chips <break/>", "en-GB-RyanNeural", "").unwrap();
        assert!(s.contains("Fish &amp; Chips &lt;break/&gt;"));
    }

    #[test]
    fn tempo_optional() {
        let s = azure("Hallo", "de-AT-IngridNeural", "-5%").unwrap();
        assert!(s.contains("<prosody rate=\"-5%\">Hallo</prosody>"));
    }

    #[test]
    fn unbrauchbarer_stimmenname() {
        assert_eq!(locale_aus_stimme("Jenny"), None);
        assert!(azure("x", "Jenny", "").is_none());
    }
}
