//! Rueckmeldung der Bildaufnahme zum Hoeren: WAV-Bytes (PCM, 16 Bit, mono),
//! rein berechnet, ohne Datei und ohne Geraet. Abgespielt wird in
//! [`crate::plattform::ton_abspielen`], damit man eine Aufnahme auch hoert,
//! wenn AeroACARS im Hintergrund liegt und der Simulator im Vollbild vorn steht.
//!
//! * [`erfolg_wav`]    kurzes doppeltes Klicken wie ein Ausloeser (rund 80 ms)
//! * [`fehlschlag_wav`] tiefer Ton (rund 250 ms)

/// Abtastrate in Hz.
const RATE: u32 = 44_100;

/// Anzahl Abtastwerte fuer `ms` Millisekunden.
fn anzahl(ms: u32) -> usize {
    (RATE as usize * ms as usize) / 1000
}

/// RIFF/WAVE-Kopf (44 Byte) plus die Abtastwerte als 16-Bit-PCM, little endian.
fn wav(samples: &[i16]) -> Vec<u8> {
    let daten = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + daten as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + daten).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes()); // Laenge des fmt-Blocks
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2).to_le_bytes()); // Bytes je Sekunde
    b.extend_from_slice(&2u16.to_le_bytes()); // Bytes je Abtastwert
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&daten.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

/// Zwei kurze Klicks (je 25 ms, im Abstand von 45 ms) in einem Puffer von 80 ms:
/// ein heller Ton mit Rauschanteil und schnellem, exponentiellem Abklingen.
pub fn erfolg_wav() -> Vec<u8> {
    let gesamt = anzahl(80);
    let mut s = vec![0i16; gesamt];
    let mut rausch: u32 = 0x1234_5678; // feste Folge: gleiches Klicken bei jedem Aufruf
    for start_ms in [0u32, 45] {
        let a0 = anzahl(start_ms);
        let laenge = anzahl(25).min(gesamt - a0);
        for i in 0..laenge {
            let t = i as f32 / RATE as f32;
            rausch = rausch.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let n = (rausch >> 16) as f32 / 32_768.0 - 1.0;
            let huellkurve = (-t * 220.0).exp();
            let ton = (2.0 * std::f32::consts::PI * 2_200.0 * t).sin();
            let v = (0.65 * ton + 0.35 * n) * huellkurve * 0.8;
            s[a0 + i] = (v * f32::from(i16::MAX)) as i16;
        }
    }
    wav(&s)
}

/// Tiefer Ton (170 Hz, 250 ms) mit weichem Ein- und Ausblenden.
pub fn fehlschlag_wav() -> Vec<u8> {
    let gesamt = anzahl(250);
    let blende = anzahl(15);
    let s: Vec<i16> = (0..gesamt)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let rein = (i as f32 / blende as f32).min(1.0);
            let raus = ((gesamt - i) as f32 / blende as f32).min(1.0);
            let v = (2.0 * std::f32::consts::PI * 170.0 * t).sin() * 0.6 * rein * raus;
            (v * f32::from(i16::MAX)) as i16
        })
        .collect();
    wav(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(w: &[u8]) -> Vec<i16> {
        w[44..].chunks_exact(2).map(|p| i16::from_le_bytes([p[0], p[1]])).collect()
    }

    fn kopf_pruefen(w: &[u8]) {
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(&w[36..40], b"data");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize, w.len() - 8, "RIFF-Laenge");
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize, w.len() - 44, "data-Laenge");
        assert_eq!(u16::from_le_bytes(w[20..22].try_into().unwrap()), 1, "PCM");
        assert_eq!(u16::from_le_bytes(w[22..24].try_into().unwrap()), 1, "mono");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), RATE);
        assert_eq!(u32::from_le_bytes(w[28..32].try_into().unwrap()), RATE * 2, "Bytes je Sekunde");
        assert_eq!(u16::from_le_bytes(w[32..34].try_into().unwrap()), 2, "Blockgroesse");
        assert_eq!(u16::from_le_bytes(w[34..36].try_into().unwrap()), 16, "Bit");
    }

    #[test]
    fn erfolg_hat_wav_kopf_dauer_und_ist_nicht_stumm() {
        let w = erfolg_wav();
        kopf_pruefen(&w);
        let s = samples(&w);
        assert_eq!(s.len(), anzahl(80));
        assert!(s.iter().map(|v| i32::from(*v).abs()).max().unwrap() > 8_000, "Klick muss hoerbar laut sein");
    }

    #[test]
    fn erfolg_klickt_zweimal_mit_stille_dazwischen() {
        let s = samples(&erfolg_wav());
        let laut = |von: usize, bis: usize| s[von..bis].iter().map(|v| i32::from(*v).abs()).max().unwrap();
        assert!(laut(0, anzahl(10)) > 4_000, "erster Klick");
        assert!(laut(anzahl(45), anzahl(55)) > 4_000, "zweiter Klick");
        assert!(laut(anzahl(32), anzahl(44)) < 1_500, "dazwischen fast still: {}", laut(anzahl(32), anzahl(44)));
    }

    #[test]
    fn fehlschlag_hat_wav_kopf_dauer_und_ist_nicht_stumm() {
        let w = fehlschlag_wav();
        kopf_pruefen(&w);
        let s = samples(&w);
        assert_eq!(s.len(), anzahl(250));
        assert!(s.iter().map(|v| i32::from(*v).abs()).max().unwrap() > 10_000);
        // weich: Anfang und Ende leise (kein Knacken)
        assert!(i32::from(s[0]).abs() < 200 && i32::from(*s.last().unwrap()).abs() < 200);
    }

    #[test]
    fn fehlschlag_ist_tiefer_als_erfolg() {
        let nulldurchgaenge = |s: &[i16]| s.windows(2).filter(|p| (p[0] >= 0) != (p[1] >= 0)).count() as f32;
        let (e, f) = (samples(&erfolg_wav()), samples(&fehlschlag_wav()));
        let rate = |s: &[i16]| nulldurchgaenge(s) / s.len() as f32;
        assert!(rate(&f) < rate(&e) / 3.0, "{} gegen {}", rate(&f), rate(&e));
    }

    #[test]
    fn erzeugung_ist_stabil() {
        assert_eq!(erfolg_wav(), erfolg_wav());
        assert_eq!(fehlschlag_wav(), fehlschlag_wav());
    }
}
