// Bausteine der uebersetzbaren Aktivitaetszeilen (3.0.1). Die Vollstaendigkeit
// der Texte sichert `client/src/lib/aktivitaetSchluessel.test.ts`.
use super::*;
use serde_json::json;

#[test]
fn text_ohne_eigene_details_reicht_das_detail_roh_durch() {
    // Sonst zeigte die Oberflaeche bei gesetztem Text kein Detail mehr —
    // auch der rohe Serverfehler waere weg.
    let t = aktivitaet_text_fertig(Some(akt("activity.lan.start_failed", &[])), Some("connection refused & <x>")).unwrap();
    assert_eq!(t.detail, vec![AktivitaetTeil::text("activity.roh", &[("text", &"connection refused & <x>")])]);
}

#[test]
fn eigene_details_bleiben_wie_sie_sind() {
    let t = akt("activity.login.relogin", &[]).und("activity.login.relogin_detail", &[]);
    assert_eq!(aktivitaet_text_fertig(Some(t.clone()), Some("deutscher Rueckfall")), Some(t));
    assert_eq!(aktivitaet_text_fertig(None, Some("x")), None);
    let ohne = akt("activity.resume.bereit", &[]);
    assert_eq!(aktivitaet_text_fertig(Some(ohne.clone()), None), Some(ohne));
}

#[test]
fn werte_sind_fertige_texte() {
    let t = AktivitaetTeil::text("activity.cpdlc.handover", &[("next", &"EDGG"), ("n", &42u32)]);
    assert_eq!(t.werte["next"], json!("EDGG"));
    assert_eq!(t.werte["n"], json!("42"));
}

#[test]
fn tabelle_kennt_ihre_saetze_und_laesst_fremdes_roh() {
    assert_eq!(teil_aus_text("b", &[("a", "activity.x"), ("b", "activity.y")]).schluessel, "activity.y");
    let roh = teil_aus_text("HTTP 500", &[("a", "activity.x")]);
    assert_eq!(roh.schluessel, "activity.roh");
    assert_eq!(roh.werte["text"], json!("HTTP 500"));
}

#[test]
fn zaehlstand_der_ruhe_regel_wird_erkannt() {
    let t = sim_warte_teil(&sim_ruhe_zaehlstand(7, 20));
    assert_eq!(t.schluessel, "activity.sim.ruhig_zaehlstand");
    assert_eq!((t.werte["s"].clone(), t.werte["n"].clone()), (json!("7"), json!("20")));
    assert_eq!(sim_warte_teil("Der Simulator liefert keine frischen Werte.").schluessel, "activity.sim.nicht_frisch");
}

#[test]
fn sonstiger_netzfehler_traegt_den_rohen_rest() {
    let t = net_fehler_teil("Netzwerk-Fehler: HTTP 502 Bad Gateway");
    assert_eq!(t.schluessel, "activity.netz.sonst");
    assert_eq!(t.werte["fehler"], json!("HTTP 502 Bad Gateway"));
    assert_eq!(net_fehler_teil("Server-Antwort-Timeout. Wiederversuch automatisch.").schluessel, "activity.netz.timeout");
}

#[test]
fn lage_teil_traegt_die_zahlen_wie_der_deutsche_satz() {
    let t = lage_abweichung_teil(&LageAbweichung::ZuWeitWeg { drift_nm: 12.345, erlaubt_nm: 5.0 });
    assert_eq!(t.schluessel, "activity.lage.zu_weit");
    assert_eq!((t.werte["nm"].clone(), t.werte["erlaubt"].clone()), (json!("12.3"), json!("5.0")));
}

#[test]
fn touchdown_ohne_g_nennt_hopser_im_detail() {
    let t = touchdown_text(-182.4, None, false, 2, None);
    assert_eq!(t.zeile, AktivitaetTeil::text("activity.touchdown.ohne_g", &[("vs", &"-182")]));
    assert_eq!(t.detail, vec![AktivitaetTeil::text("activity.touchdown.bounces", &[("n", &2u8)])]);
    let mit_g = touchdown_text(-95.0, Some(1.234), true, 0, None);
    assert_eq!(mit_g.zeile.werte["g"], json!("1.23"));
    assert!(mit_g.detail.is_empty());
}
