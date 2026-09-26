//! Lauf-Protokoll: alle Messpunkte, Auswertungen, freie Ereignisse — plus
//! Live-Protokoll (JSONL, nach jedem Ereignis geflusht + fsync) und
//! Endbericht (JSON + TXT).
//!
//! Plattformunabhängig (auf dem Mac getestet).

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

use crate::auswertung::{self, gleich, Ergebnis, Werte};

#[derive(Debug, Clone, Serialize, Default)]
pub struct LaufInfo {
    pub werkzeug_version: String,
    pub simulator: String,
    pub titel: String,
    pub atc_model: String,
    pub icao: Option<String>,
    pub mobiflight_version: String,
    /// Tatsächlich abonniert (MobiFlight-Liste ∪ Zusatzliste, ohne
    /// übersprungene Namen).
    pub lvars_gesamt: usize,
    /// So viele Namen lieferte `MF.LVars.List`.
    pub lvars_mobiflight: usize,
    /// Größe der eingebetteten Zusatz-Namensliste des Skripts.
    pub lvars_zusatzliste: usize,
    /// Davon nicht schon in der MobiFlight-Liste.
    pub lvars_zusatz_neu: usize,
    /// Welcher Weg die LVars liest.
    pub lesewege: String,
    pub lvars_direkt_abgelehnt: Vec<String>,
    /// Anfangsmessung je Gruppe: abonniert / geliefert / ≠ 0.
    pub diagnose_gruppen: Vec<crate::diagnose::GruppenDiagnose>,
    /// Anfangswerte der Stichprobe auf beiden Wegen (L: direkt, MF:L:).
    pub stichprobe: Vec<crate::diagnose::Probe>,
    /// Beobachtete Input-Events (B:, nur Zahlen-Events). 0 = MSFS lieferte keine.
    pub input_events: usize,
    /// Text-Input-Events, die übergangen wurden.
    pub input_events_text: usize,
    /// Das MobiFlight-Modul listet höchstens 1000 LVars (Module.cpp Z. 228).
    pub lvar_liste_moeglicherweise_gekappt: bool,
    pub lvars_uebersprungen: Vec<String>,
    pub simvars_abgelehnt: Vec<String>,
    pub skript: String,
    pub beginn: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Messpunkt {
    pub nr: usize,
    pub zeit: String,
    pub anlass: String,
    #[serde(skip)]
    pub werte: Werte,
}

#[derive(Debug, Clone, Serialize)]
pub struct StellungNachweis {
    pub stellung: String,
    pub messpunkt_a: usize,
    pub messpunkt_b: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchalterBericht {
    pub schalter: String,
    pub durchgang: usize,
    /// true, wenn der Pilot diesen Schalter danach nochmal gemessen hat.
    pub ersetzt: bool,
    pub uebersprungen: bool,
    pub stellungen: Vec<StellungNachweis>,
    pub ergebnis: Ergebnis,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Aenderung {
    pub variable: String,
    pub alt: Option<f64>,
    pub neu: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FreiEreignis {
    pub bezeichnung: String,
    pub messpunkt: usize,
    pub aenderungen: Vec<Aenderung>,
}

pub struct Lauf {
    pub info: LaufInfo,
    pub variablen: Vec<String>,
    pub messpunkte: Vec<Messpunkt>,
    /// Messpunkt-Index (0-basiert) der ersten Ruhemessung = Bezug für
    /// „geändert".
    pub ruhe_bezug: Option<usize>,
    pub global_unruhig: BTreeSet<usize>,
    pub schalter: Vec<SchalterBericht>,
    pub frei: Vec<FreiEreignis>,
    /// Alle Variablen, die sich irgendwann gegenüber der Ruhemessung
    /// geändert haben — über den GANZEN Lauf, auch außerhalb des Skripts.
    pub geaendert: BTreeSet<usize>,
    pub ordner: PathBuf,
    pub stamm: String,
    live: Option<File>,
    pub live_fehler: Option<String>,
    pub abgeschlossen: bool,
}

/// Dateinamen-tauglich: nur A–Z, a–z, 0–9, '-', '_'; höchstens 60 Zeichen.
pub fn dateiname_teil(s: &str) -> String {
    let mut out = String::new();
    let mut unterstrich = false;
    for c in s.trim().chars() {
        let ok = c.is_ascii_alphanumeric() || c == '-';
        if ok {
            out.push(c);
            unterstrich = false;
        } else if !unterstrich && !out.is_empty() {
            out.push('_');
            unterstrich = true;
        }
    }
    let t: String = out.trim_end_matches('_').chars().take(60).collect();
    if t.is_empty() {
        "Flugzeug".into()
    } else {
        t
    }
}

pub fn jetzt() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

impl Lauf {
    pub fn neu(info: LaufInfo, variablen: Vec<String>, ordner: PathBuf) -> Self {
        let flugzeug = if info.titel.trim().is_empty() {
            info.icao.clone().unwrap_or_default()
        } else {
            info.titel.clone()
        };
        let stamm = format!(
            "{}-{}",
            dateiname_teil(&flugzeug),
            chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
        );
        let mut lauf = Self {
            info,
            variablen,
            messpunkte: Vec::new(),
            ruhe_bezug: None,
            global_unruhig: BTreeSet::new(),
            schalter: Vec::new(),
            frei: Vec::new(),
            geaendert: BTreeSet::new(),
            ordner,
            stamm,
            live: None,
            live_fehler: None,
            abgeschlossen: false,
        };
        let pfad = lauf.live_pfad();
        match OpenOptions::new().create(true).append(true).open(&pfad) {
            Ok(f) => lauf.live = Some(f),
            Err(e) => lauf.live_fehler = Some(format!("{}: {e}", pfad.display())),
        }
        lauf
    }

    pub fn live_pfad(&self) -> PathBuf {
        self.ordner.join(format!("live-{}.jsonl", self.stamm))
    }
    pub fn json_pfad(&self) -> PathBuf {
        self.ordner.join(format!("{}.json", self.stamm))
    }
    pub fn txt_pfad(&self) -> PathBuf {
        self.ordner.join(format!("{}.txt", self.stamm))
    }

    /// Eine Zeile ans Live-Protokoll — sofort geflusht und auf die Platte
    /// gezwungen, damit `tail -f` über SMB sie sieht und ein Absturz sie nicht
    /// mitnimmt.
    pub fn live(&mut self, typ: &str, mut daten: Value) {
        if let Value::Object(m) = &mut daten {
            m.insert("typ".into(), json!(typ));
            m.insert("zeit".into(), json!(jetzt()));
        }
        if let Some(f) = self.live.as_mut() {
            let zeile = serde_json::to_string(&daten).unwrap_or_default();
            let r = writeln!(f, "{zeile}")
                .and_then(|_| f.flush())
                .and_then(|_| f.sync_all());
            if let Err(e) = r {
                self.live_fehler = Some(e.to_string());
            }
        }
    }

    pub fn start_ereignis(&mut self) {
        let d = json!({
            "info": self.info,
            "variablen_gesamt": self.variablen.len(),
        });
        self.live("start", d);
    }

    /// Neuer Messpunkt; aktualisiert die Liste der geänderten Variablen.
    /// Gibt (Nummer, neu geänderte Indizes) zurück.
    pub fn messpunkt(&mut self, anlass: &str, werte: Werte) -> (usize, Vec<usize>) {
        let nr = self.messpunkte.len() + 1;
        let mut neu = Vec::new();
        if let Some(r) = self.ruhe_bezug {
            let bezug = &self.messpunkte[r].werte;
            for (i, w) in werte.iter().enumerate() {
                if w.is_some()
                    && !gleich(bezug.get(i).copied().flatten(), *w)
                    && self.geaendert.insert(i)
                {
                    neu.push(i);
                }
            }
        }
        self.messpunkte.push(Messpunkt {
            nr,
            zeit: jetzt(),
            anlass: anlass.to_string(),
            werte,
        });
        (nr, neu)
    }

    /// Nach der Ruhemessung: Bezug setzen, unruhige Variablen merken.
    pub fn ruhe_abschliessen(&mut self, von: usize, bis: usize) {
        self.ruhe_bezug = Some(von);
        let reihe: Vec<&Werte> = self.messpunkte[von..bis].iter().map(|m| &m.werte).collect();
        self.global_unruhig = auswertung::unruhige(&reihe);
        let unruhig: Vec<&str> = self
            .global_unruhig
            .iter()
            .map(|i| self.variablen[*i].as_str())
            .collect();
        // Fehlende (nie gelieferte) gesondert zählen, sie sind kein Rauschen.
        let fehlend = self.messpunkte[von]
            .werte
            .iter()
            .filter(|w| w.is_none())
            .count();
        let d = json!({
            "messpunkte": [von + 1, bis],
            "unruhig_ausgeschlossen": unruhig,
            "davon_nicht_geliefert": fehlend,
        });
        self.live("ruhemessung", d);
    }

    /// Werte aller geänderten Variablen an einem Messpunkt, als Name → Wert.
    fn geaenderte_werte(&self, nr: usize) -> serde_json::Map<String, Value> {
        let mut m = serde_json::Map::new();
        let werte = &self.messpunkte[nr - 1].werte;
        for i in &self.geaendert {
            m.insert(
                self.variablen[*i].clone(),
                json!(werte.get(*i).copied().flatten()),
            );
        }
        m
    }

    pub fn stellung_gemessen(
        &mut self,
        schalter: &str,
        stellung: &str,
        a: usize,
        b: usize,
        neu_geaendert: &[usize],
    ) {
        let wa = self.geaenderte_werte(a);
        let wb = self.geaenderte_werte(b);
        let neu: Vec<&str> = neu_geaendert
            .iter()
            .map(|i| self.variablen[*i].as_str())
            .collect();
        let d = json!({
            "schalter": schalter,
            "stellung": stellung,
            "messpunkte": [a, b],
            "neu_geaendert": neu,
            "geaendert_gesamt": self.geaendert.len(),
            "werte_a": wa,
            "werte_b": wb,
        });
        self.live("stellung_gemessen", d);
    }

    pub fn schalter_ausgewertet(&mut self, bericht: SchalterBericht) {
        // Frühere Durchgänge desselben Schalters als ersetzt markieren.
        for s in self.schalter.iter_mut() {
            if s.schalter == bericht.schalter {
                s.ersetzt = true;
            }
        }
        let top5: Vec<String> = bericht
            .ergebnis
            .kandidaten
            .iter()
            .take(5)
            .map(|k| k.zeile())
            .collect();
        let d = json!({
            "schalter": bericht.schalter,
            "durchgang": bericht.durchgang,
            "uebersprungen": bericht.uebersprungen,
            "kandidaten_gesamt": bericht.ergebnis.kandidaten.len(),
            "top5": top5,
            "unruhig_bei_diesem_schalter": bericht.ergebnis.unruhig_bei_diesem_schalter,
        });
        self.schalter.push(bericht);
        self.live("schalter_ausgewertet", d);
    }

    /// Freier Modus: ruhige Variablen, die sich seit dem Messpunkt `vorher`
    /// geändert haben. `a`/`b` sind die zwei neuen Messpunkte.
    pub fn frei_aenderungen(&self, vorher: usize, a: usize, b: usize) -> Vec<Aenderung> {
        let alt = &self.messpunkte[vorher - 1].werte;
        let wa = &self.messpunkte[a - 1].werte;
        let wb = &self.messpunkte[b - 1].werte;
        let mut v = Vec::new();
        for i in 0..self.variablen.len() {
            if self.global_unruhig.contains(&i) {
                continue;
            }
            let (x, y) = (wa.get(i).copied().flatten(), wb.get(i).copied().flatten());
            if !gleich(x, y) {
                continue; // unruhig in dieser Messung
            }
            let o = alt.get(i).copied().flatten();
            if !gleich(o, x) {
                v.push(Aenderung {
                    variable: self.variablen[i].clone(),
                    alt: o,
                    neu: x.expect("gleich() verlangt Some"),
                });
            }
        }
        v
    }

    pub fn frei_ereignis(&mut self, e: FreiEreignis, neu_geaendert: &[usize]) {
        let neu: Vec<&str> = neu_geaendert
            .iter()
            .map(|i| self.variablen[*i].as_str())
            .collect();
        let wb = self.geaenderte_werte(e.messpunkt);
        let d = json!({
            "bezeichnung": e.bezeichnung,
            "messpunkt": e.messpunkt,
            "aenderungen": e.aenderungen,
            "neu_geaendert": neu,
            "geaendert_gesamt": self.geaendert.len(),
            "werte": wb,
        });
        self.frei.push(e);
        self.live("frei_ereignis", d);
    }

    fn verlauf(&self) -> Vec<Value> {
        self.geaendert
            .iter()
            .map(|i| {
                let werte: Vec<Option<f64>> = self
                    .messpunkte
                    .iter()
                    .map(|m| m.werte.get(*i).copied().flatten())
                    .collect();
                json!({ "variable": self.variablen[*i], "werte_je_messpunkt": werte })
            })
            .collect()
    }

    pub fn bericht_json(&self, grund: &str) -> Value {
        let unruhig: Vec<&str> = self
            .global_unruhig
            .iter()
            .map(|i| self.variablen[*i].as_str())
            .collect();
        json!({
            "info": self.info,
            "ende": jetzt(),
            "ende_grund": grund,
            "variablen_beobachtet": self.variablen.len(),
            "unruhig_in_ruhemessung": unruhig,
            "schalter": self.schalter,
            "freier_modus": self.frei,
            "messpunkte": self.messpunkte,
            "geaenderte_variablen": self.verlauf(),
        })
    }

    pub fn bericht_txt(&self, grund: &str) -> String {
        let i = &self.info;
        let mut t = String::new();
        let mut z = |s: String| {
            t.push_str(&s);
            t.push_str("\r\n");
        };
        z("AeroACARS-Schalterprüfung — Bericht".into());
        z("====================================".into());
        z(format!("Flugzeug (TITLE):   {}", i.titel));
        z(format!(
            "ATC MODEL / ICAO:   {} / {}",
            i.atc_model,
            i.icao.clone().unwrap_or_else(|| "?".into())
        ));
        z(format!("Simulator:          {}", i.simulator));
        z(format!("Zeit:               {} bis {}", i.beginn, jetzt()));
        z(format!("Ende:               {grund}"));
        z(format!("MobiFlight-Modul:   {}", i.mobiflight_version));
        z(format!(
            "LVars MobiFlight:   {}{}",
            i.lvars_mobiflight,
            if i.lvar_liste_moeglicherweise_gekappt {
                "  (!) 1000er-Grenze des MobiFlight-Moduls erreicht — es gibt vermutlich mehr"
            } else {
                "  (1000er-Grenze nicht erreicht)"
            }
        ));
        z(format!(
            "LVars Zusatzliste:  {} ({} davon nicht in der MobiFlight-Liste)",
            i.lvars_zusatzliste, i.lvars_zusatz_neu
        ));
        z(format!("LVars abonniert:    {}", i.lvars_gesamt));
        z(format!("Leseweg:            {}", i.lesewege));
        if !i.lvars_direkt_abgelehnt.is_empty() {
            z(format!(
                "Von SimConnect abgelehnt: {}",
                i.lvars_direkt_abgelehnt.join(", ")
            ));
        }
        if !i.diagnose_gruppen.is_empty() {
            z("Diagnose Anfangsmessung (abonniert / geliefert / ≠ 0):".into());
            for g in &i.diagnose_gruppen {
                z(format!(
                    "    {:<40} {:>4} / {:>4} / {:>4}{}",
                    g.gruppe,
                    g.abonniert,
                    g.geliefert,
                    g.ungleich_null,
                    if g.verdaechtig { "  (!) alles 0" } else { "" }
                ));
            }
        }
        for p in &i.stichprobe {
            z(format!(
                "    Stichprobe {:<36} = {}",
                p.variable,
                p.wert
                    .map(auswertung::wert_text)
                    .unwrap_or_else(|| "-".into())
            ));
        }
        z(format!(
            "Input-Events (B:):  {} ({} Text-Events übergangen)",
            i.input_events, i.input_events_text
        ));
        if !i.lvars_uebersprungen.is_empty() {
            z(format!(
                "LVars übersprungen: {}",
                i.lvars_uebersprungen.join(", ")
            ));
        }
        if !i.simvars_abgelehnt.is_empty() {
            z(format!(
                "SimVars abgelehnt:  {}",
                i.simvars_abgelehnt.join(", ")
            ));
        }
        z(format!("Skript:             {}", i.skript));
        z(format!("Werkzeug:           {}", i.werkzeug_version));
        z(String::new());
        z(format!(
            "In der Ruhemessung unruhig (ausgeschlossen): {}",
            self.global_unruhig.len()
        ));
        for idx in &self.global_unruhig {
            z(format!("    {}", self.variablen[*idx]));
        }
        for (n, s) in self.schalter.iter().enumerate() {
            z(String::new());
            let st: Vec<&str> = s.stellungen.iter().map(|x| x.stellung.as_str()).collect();
            z(format!(
                "== {}. {} ({}){}{} ==",
                n + 1,
                s.schalter,
                st.join(", "),
                if s.durchgang > 1 {
                    format!(" — Durchgang {}", s.durchgang)
                } else {
                    String::new()
                },
                if s.ersetzt {
                    " — ERSETZT (später nochmal gemessen)"
                } else {
                    ""
                }
            ));
            if s.uebersprungen {
                z("   übersprungen".into());
                continue;
            }
            let nachweis: Vec<String> = s
                .stellungen
                .iter()
                .map(|x| format!("{}=#{}/#{}", x.stellung, x.messpunkt_a, x.messpunkt_b))
                .collect();
            z(format!("   Messpunkte: {}", nachweis.join(", ")));
            if s.ergebnis.kandidaten.is_empty() {
                z("   Keine Variable gefunden, die diesem Schalter folgt.".into());
            }
            for (k, kand) in s.ergebnis.kandidaten.iter().take(15).enumerate() {
                z(format!("   {:>2}. {}", k + 1, kand.zeile()));
            }
            if s.ergebnis.kandidaten.len() > 15 {
                z(format!(
                    "       … und {} weitere (siehe JSON)",
                    s.ergebnis.kandidaten.len() - 15
                ));
            }
            if !s.ergebnis.unruhig_bei_diesem_schalter.is_empty() {
                z(format!(
                    "   Unruhig innerhalb einer Stellung (ausgeschlossen): {}",
                    s.ergebnis.unruhig_bei_diesem_schalter.join(", ")
                ));
            }
        }
        if !self.frei.is_empty() {
            z(String::new());
            z("== Freier Modus ==".into());
            for e in &self.frei {
                z(format!(
                    "   #{} „{}“: {} Änderung(en)",
                    e.messpunkt,
                    e.bezeichnung,
                    e.aenderungen.len()
                ));
                for a in &e.aenderungen {
                    z(format!("      {}", aenderung_zeile(a)));
                }
            }
        }
        z(String::new());
        z(format!(
            "== Alle Variablen, die sich gegenüber der Ruhemessung geändert haben ({}) ==",
            self.geaendert.len()
        ));
        z("   Messpunkte:".into());
        for m in &self.messpunkte {
            z(format!("      #{:<3} {}  {}", m.nr, m.zeit, m.anlass));
        }
        for idx in &self.geaendert {
            let werte: Vec<String> = self
                .messpunkte
                .iter()
                .map(|m| match m.werte.get(*idx).copied().flatten() {
                    Some(w) => auswertung::wert_text(w),
                    None => "-".into(),
                })
                .collect();
            let unruhig = if self.global_unruhig.contains(idx) {
                " (unruhig)"
            } else {
                ""
            };
            z(format!(
                "   {}{}: {}",
                self.variablen[*idx],
                unruhig,
                werte.join(" ")
            ));
        }
        t
    }

    /// Endbericht schreiben (JSON + TXT) und `ende` ins Live-Protokoll.
    pub fn abschliessen(&mut self, grund: &str) -> Result<(PathBuf, PathBuf), String> {
        let json_text =
            serde_json::to_string_pretty(&self.bericht_json(grund)).map_err(|e| e.to_string())?;
        let jp = self.json_pfad();
        let tp = self.txt_pfad();
        schreiben(&jp, json_text.as_bytes())?;
        schreiben(&tp, self.bericht_txt(grund).as_bytes())?;
        if !self.abgeschlossen {
            let d = json!({
                "grund": grund,
                "bericht_json": jp.display().to_string(),
                "bericht_txt": tp.display().to_string(),
                "geaendert_gesamt": self.geaendert.len(),
            });
            self.live("ende", d);
        }
        self.abgeschlossen = true;
        Ok((jp, tp))
    }

    /// Zwischenstand (nach jedem Schalter): Endbericht überschreiben, ohne
    /// `ende`-Ereignis.
    pub fn zwischenstand(&self) -> Result<(), String> {
        let json_text =
            serde_json::to_string_pretty(&self.bericht_json("Zwischenstand (Lauf noch aktiv)"))
                .map_err(|e| e.to_string())?;
        schreiben(&self.json_pfad(), json_text.as_bytes())?;
        schreiben(
            &self.txt_pfad(),
            self.bericht_txt("Zwischenstand (Lauf noch aktiv)")
                .as_bytes(),
        )
    }
}

pub fn aenderung_zeile(a: &Aenderung) -> String {
    let alt = a
        .alt
        .map(auswertung::wert_text)
        .unwrap_or_else(|| "-".into());
    format!("{}: {} → {}", a.variable, alt, auswertung::wert_text(a.neu))
}

fn schreiben(p: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = File::create(p).map_err(|e| format!("{}: {e}", p.display()))?;
    f.write_all(bytes)
        .map_err(|e| format!("{}: {e}", p.display()))?;
    f.flush().map_err(|e| e.to_string())?;
    let _ = f.sync_all();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordner() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "schalterpruefung-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn dateinamen() {
        assert_eq!(dateiname_teil("A380-800 RR Basic"), "A380-800_RR_Basic");
        assert_eq!(dateiname_teil("A350-900 (No Cabin)"), "A350-900_No_Cabin");
        assert_eq!(dateiname_teil("  "), "Flugzeug");
        assert!(dateiname_teil(&"x".repeat(100)).len() <= 60);
    }

    #[test]
    fn ganzer_lauf_mit_live_protokoll_und_bericht() {
        let o = ordner();
        let info = LaufInfo {
            titel: "A380-800 RR Basic".into(),
            ..Default::default()
        };
        let vars: Vec<String> = vec!["L:STROBE_SW".into(), "L:BLINK".into(), "L:WING_LT".into()];
        let mut l = Lauf::neu(info, vars, o.clone());
        l.start_ereignis();
        // Ruhe: BLINK zappelt.
        l.messpunkt("Ruhe 1", vec![Some(0.), Some(0.), Some(0.)]);
        l.messpunkt("Ruhe 2", vec![Some(0.), Some(1.), Some(0.)]);
        l.ruhe_abschliessen(0, 2);
        assert_eq!(
            l.global_unruhig.iter().copied().collect::<Vec<_>>(),
            vec![1]
        );
        let (a, _) = l.messpunkt("STROBE=ON a", vec![Some(2.), Some(0.), Some(0.)]);
        let (b, neu) = l.messpunkt("STROBE=ON b", vec![Some(2.), Some(1.), Some(0.)]);
        // STROBE wurde schon in a erfasst; neu ist hier nur der Blinker.
        assert_eq!(neu, vec![1]);
        assert!(l.geaendert.contains(&0));
        l.stellung_gemessen("STROBE", "ON", a, b, &[0]);
        // Freier Modus: WING_LT außerhalb des Skripts.
        let (fa, na) = l.messpunkt("Frei a", vec![Some(2.), Some(0.), Some(1.)]);
        let (fb, _) = l.messpunkt("Frei b", vec![Some(2.), Some(1.), Some(1.)]);
        assert_eq!(na, vec![2]);
        let aend = l.frei_aenderungen(b, fa, fb);
        assert_eq!(
            aend,
            vec![Aenderung {
                variable: "L:WING_LT".into(),
                alt: Some(0.),
                neu: 1.
            }]
        );
        assert_eq!(aenderung_zeile(&aend[0]), "L:WING_LT: 0 → 1");
        l.frei_ereignis(
            FreiEreignis {
                bezeichnung: "Wing Lights".into(),
                messpunkt: fb,
                aenderungen: aend,
            },
            &na,
        );
        let (jp, tp) = l.abschliessen("fertig").unwrap();
        let live = std::fs::read_to_string(l.live_pfad()).unwrap();
        let typen: Vec<String> = live
            .lines()
            .map(|z| {
                serde_json::from_str::<Value>(z).unwrap()["typ"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            typen,
            vec![
                "start",
                "ruhemessung",
                "stellung_gemessen",
                "frei_ereignis",
                "ende"
            ]
        );
        let j: Value = serde_json::from_str(&std::fs::read_to_string(jp).unwrap()).unwrap();
        let g = j["geaenderte_variablen"].as_array().unwrap();
        // BLINK zählt auch als geändert (Verlauf!), ist aber als unruhig markiert.
        let namen: Vec<&str> = g.iter().map(|v| v["variable"].as_str().unwrap()).collect();
        assert_eq!(namen, vec!["L:STROBE_SW", "L:BLINK", "L:WING_LT"]);
        assert_eq!(g[2]["werte_je_messpunkt"].as_array().unwrap().len(), 6);
        let txt = std::fs::read_to_string(tp).unwrap();
        assert!(txt.contains("L:WING_LT: 0 → 1"));
        assert!(txt.contains("L:BLINK (unruhig)"));
        assert!(txt.contains("LVars abonniert:"));
        assert!(txt.contains("Input-Events (B:):"));
        assert!(txt.contains("1000er-Grenze"));
        // Start-Ereignis trägt die Zähler.
        let start: Value = serde_json::from_str(live.lines().next().unwrap()).unwrap();
        for f in [
            "lvars_mobiflight",
            "lvars_zusatzliste",
            "lvars_zusatz_neu",
            "lvars_gesamt",
            "lvar_liste_moeglicherweise_gekappt",
        ] {
            assert!(
                start["info"].get(f).is_some(),
                "{f} fehlt im start-Ereignis"
            );
        }
        // Ein zweites Abschließen (Strg+C nach Ende) schreibt kein zweites `ende`.
        l.abschliessen("nochmal").unwrap();
        let live2 = std::fs::read_to_string(l.live_pfad()).unwrap();
        assert_eq!(live2.lines().count(), 5);
        let _ = std::fs::remove_dir_all(o);
    }
}
