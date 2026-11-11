//! WASM-Host fuer die Paketmodule: Bewertung (Pflichtenheft P-4.30, P-4.31,
//! P-4.33) und Kabinenplaner (P-3.58).
//!
//! wasmi: reines Rust, kein JIT, gleich auf Mac und Windows. Jede Bewertung
//! laeuft in einer frischen Instanz mit Rechenzeitgrenze („Fuel") und
//! Speichergrenze (64 MB); ein Fehler im Modul kann den Client so weder
//! aufhaengen noch seinen Speicher fuellen. Der Kabinenplaner behaelt eine
//! [`Instanz`] den ganzen Flug (Zustand im Modulspeicher), mit denselben
//! Grenzen je Aufruf.

use wasmi::{Config, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc};

/// Speichergrenze des Moduls (P-4.30).
pub const SPEICHER_MAX: usize = 64 * 1024 * 1024;

/// Was schiefgehen kann (P-4.33: dann eingebaute Fassung, dann `modulfehler`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fehler {
    /// Modul laesst sich nicht laden oder hat die Exporte nicht.
    Modul(String),
    /// Rechenzeit erschoepft.
    Fuel,
    /// Absturz oder Speicher erschoepft im Modul.
    Abbruch(String),
    /// Ausgabe ist kein UTF-8.
    Ausgabe,
}

impl std::fmt::Display for Fehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fehler::Modul(t) => write!(f, "Modul: {t}"),
            Fehler::Fuel => write!(f, "Rechenzeitgrenze erreicht"),
            Fehler::Abbruch(t) => write!(f, "Abbruch im Modul: {t}"),
            Fehler::Ausgabe => write!(f, "Ausgabe ungueltig"),
        }
    }
}

/// Ein geladenes Bewertungsmodul.
pub struct Host {
    engine: Engine,
    modul: Module,
}

impl Host {
    /// Modul laden und pruefen (Exporte `memory`, `alloc`, `bewerte`).
    pub fn neu(wasm: &[u8]) -> Result<Self, Fehler> {
        Self::neu_mit(wasm, &["memory", "alloc", "bewerte"])
    }

    /// Modul laden und pruefen, ob es diese Exporte hat.
    pub fn neu_mit(wasm: &[u8], exporte: &[&str]) -> Result<Self, Fehler> {
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let modul = Module::new(&engine, wasm).map_err(|e| Fehler::Modul(e.to_string()))?;
        for &name in exporte {
            if modul.get_export(name).is_none() {
                return Err(Fehler::Modul(format!("Export {name} fehlt")));
            }
        }
        Ok(Self { engine, modul })
    }

    /// Ein Eingabe-Dokument (JSON) bewerten, hoechstens `fuel` Rechenschritte.
    pub fn bewerten(&self, eingabe: &str, fuel: u64) -> Result<String, Fehler> {
        self.bewerten_mit_verbrauch(eingabe, fuel).map(|(t, _)| t)
    }

    /// Wie [`Host::bewerten`], dazu der Verbrauch an Fuel (Messung P-4.32).
    pub fn bewerten_mit_verbrauch(&self, eingabe: &str, fuel: u64) -> Result<(String, u64), Fehler> {
        self.aufrufen("bewerte", eingabe, fuel)
    }

    /// Ob das Modul den Export `name` hat. Neue Exporte sind optional: ein
    /// aelteres Paket hat sie nicht, dann rechnet der Client nativ.
    pub fn hat_export(&self, name: &str) -> bool {
        self.modul.get_export(name).is_some()
    }

    /// Aufsetz-Messung (Export `messe`, DLH9901 11.11.2026), dazu der
    /// Verbrauch an Fuel.
    pub fn messe(&self, eingabe: &str, fuel: u64) -> Result<(String, u64), Fehler> {
        self.aufrufen("messe", eingabe, fuel)
    }

    /// Soll-Band des Anflugs (Export `sollband`, P-4.21).
    pub fn sollband(&self, eingabe: &str, fuel: u64) -> Result<String, Fehler> {
        self.aufrufen("sollband", eingabe, fuel).map(|(t, _)| t)
    }

    /// Einen Export mit JSON-Ein-/Ausgabe aufrufen (frische Instanz).
    fn aufrufen(&self, export: &str, eingabe: &str, fuel: u64) -> Result<(String, u64), Fehler> {
        // Der Vorrat gilt fuer Start UND Aufruf zusammen.
        let mut instanz = self.instanz(export, fuel)?;
        instanz.rufen(eingabe, fuel)
    }

    /// Eine Instanz, die ueber viele Aufrufe von `export` bestehen bleibt
    /// (Kabinenplaner: eine je Flug). `fuel` gilt fuer den Start.
    pub fn instanz(&self, export: &str, fuel: u64) -> Result<Instanz, Fehler> {
        let grenzen = StoreLimitsBuilder::new().memory_size(SPEICHER_MAX).build();
        let mut store: Store<StoreLimits> = Store::new(&self.engine, grenzen);
        store.limiter(|g| g);
        store.set_fuel(fuel).map_err(|e| Fehler::Modul(e.to_string()))?;
        let linker = Linker::<StoreLimits>::new(&self.engine);
        let instanz = linker
            .instantiate_and_start(&mut store, &self.modul)
            .map_err(|e| abbruch(&mut store, e))?;
        let speicher = instanz
            .get_memory(&store, "memory")
            .ok_or_else(|| Fehler::Modul("memory fehlt".into()))?;
        let alloc = instanz
            .get_typed_func::<u32, u32>(&store, "alloc")
            .map_err(|e| Fehler::Modul(e.to_string()))?;
        let funktion = instanz
            .get_typed_func::<(u32, u32), u64>(&store, export)
            .map_err(|e| Fehler::Modul(e.to_string()))?;
        Ok(Instanz { store, speicher, alloc, funktion })
    }
}

/// Eine laufende Modulinstanz: Ihr Speicher (und damit der Zustand des
/// Moduls) bleibt zwischen den Aufrufen erhalten.
pub struct Instanz {
    store: Store<StoreLimits>,
    speicher: Memory,
    alloc: TypedFunc<u32, u32>,
    funktion: TypedFunc<(u32, u32), u64>,
}

impl Instanz {
    /// Den Export mit `eingabe` aufrufen, hoechstens `fuel` Rechenschritte;
    /// dazu der Verbrauch.
    pub fn aufrufen(&mut self, eingabe: &str, fuel: u64) -> Result<(String, u64), Fehler> {
        self.store.set_fuel(fuel).map_err(|e| Fehler::Modul(e.to_string()))?;
        self.rufen(eingabe, fuel)
    }

    /// Belegter Modulspeicher in Bytes (Messung: waechst er im Flug?).
    pub fn speicher_bytes(&self) -> usize {
        self.speicher.data_size(&self.store)
    }

    /// Aufruf mit dem gesetzten Vorrat; `fuel` ist der Vorrat vor dem Start.
    fn rufen(&mut self, eingabe: &str, fuel: u64) -> Result<(String, u64), Fehler> {
        let store = &mut self.store;
        let bytes = eingabe.as_bytes();
        let n = u32::try_from(bytes.len()).map_err(|_| Fehler::Abbruch("Eingabe zu gross".into()))?;
        let ptr = self.alloc.call(&mut *store, n).map_err(|e| abbruch(store, e))?;
        self.speicher
            .write(&mut *store, ptr as usize, bytes)
            .map_err(|e| Fehler::Abbruch(e.to_string()))?;
        let r = self.funktion.call(&mut *store, (ptr, n)).map_err(|e| abbruch(store, e))?;
        let (aus_ptr, aus_len) = ((r >> 32) as usize, (r & 0xffff_ffff) as usize);
        // Erst pruefen, dann anlegen: Die Laenge kommt vom Modul, ein
        // fehlerhaftes Modul koennte sonst Gigabytes im Client anfordern
        // (Codex 29.10.).
        if aus_ptr.checked_add(aus_len).is_none_or(|ende| ende > self.speicher.data_size(&*store)) {
            return Err(Fehler::Ausgabe);
        }
        let mut aus = vec![0u8; aus_len];
        self.speicher
            .read(&*store, aus_ptr, &mut aus)
            .map_err(|e| Fehler::Abbruch(e.to_string()))?;
        let verbraucht = fuel - store.get_fuel().unwrap_or(0);
        String::from_utf8(aus).map(|t| (t, verbraucht)).map_err(|_| Fehler::Ausgabe)
    }
}

fn abbruch(_store: &mut Store<StoreLimits>, e: wasmi::Error) -> Fehler {
    // wasmi bricht ab, bevor der Vorrat auf 0 steht (die naechste Anweisung
    // braucht mehr, als uebrig ist) — deshalb am Fehlercode erkennen.
    if e.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel) {
        Fehler::Fuel
    } else {
        Fehler::Abbruch(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use landing_scoring::wasm::{bewerten_json, Eingabe};
    use landing_scoring::LandingScoringInput;

    const MODUL: &[u8] = include_bytes!("../../../../../bundle/module/bewertung.wasm");
    const VIEL: u64 = 1_000_000_000;
    /// Gleich `bewertung.rs` `MESS_FUEL_GRENZE` im Client.
    const MESS_FUEL: u64 = 199_229_590;

    fn faelle() -> Vec<Eingabe> {
        let mut v = Vec::new();
        for vs in [-60.0, -180.0, -320.0, -650.0, -1100.0] {
            for g in [None, Some(1.05), Some(1.6), Some(2.4)] {
                for hops in [0, 1, 3] {
                    v.push(Eingabe {
                        landung: LandingScoringInput {
                            vs_fpm: Some(vs),
                            peak_g_load: g,
                            bounce_count: Some(hops),
                            ..Default::default()
                        },
                    });
                }
            }
        }
        v
    }

    /// P-4.31: dasselbe Ergebnis wie das Crate nativ — Byte fuer Byte.
    #[test]
    fn wasm_rechnet_wie_nativ() {
        let host = Host::neu(MODUL).unwrap();
        for e in faelle() {
            let json = serde_json::to_string(&e).unwrap();
            assert_eq!(host.bewerten(&json, VIEL).unwrap(), bewerten_json(&json), "{json}");
        }
    }

    /// Referenz fuer andere Hosts (Server, Node): je Zeile Eingabe und die
    /// Ausgabe des Crates nativ. `REFERENZ_JSONL=<datei> cargo test -p
    /// bewertung-host referenz_schreiben -- --ignored`.
    #[test]
    #[ignore]
    fn referenz_schreiben() {
        let zeilen: Vec<String> = faelle()
            .iter()
            .map(|e| {
                let ein = serde_json::to_string(e).unwrap();
                serde_json::json!({ "eingabe": ein, "ausgabe": bewerten_json(&ein) }).to_string()
            })
            .collect();
        std::fs::write(std::env::var("REFERENZ_JSONL").unwrap(), zeilen.join("\n") + "\n").unwrap();
    }

    #[test]
    #[ignore]
    fn verbrauch_messen() {
        let host = Host::neu(MODUL).unwrap();
        let mut v: Vec<u64> = faelle()
            .iter()
            .map(|e| host.bewerten_mit_verbrauch(&serde_json::to_string(e).unwrap(), VIEL).unwrap().1)
            .collect();
        v.sort_unstable();
        println!("FUEL min {} median {} max {}", v[0], v[v.len() / 2], v[v.len() - 1]);
    }

    /// CI (Schritt „Bewertungsmodul gleich dem Quellstand“): Ein frisch aus
    /// dem Quellstand gebautes Modul (`BEWERTUNG_WASM_FRISCH`) rechnet alle
    /// Faelle genau wie das eingecheckte und wie das Crate nativ. Bytegleich
    /// sind Mac- und Linux-Build nicht (29.10.: Funktionsabschnitt weicht ab,
    /// auch mit fester Toolchain und einer Codegen-Einheit) — verglichen
    /// wird deshalb, was das Modul rechnet.
    #[test]
    fn frisch_gebautes_modul_rechnet_wie_das_eingecheckte() {
        let Ok(pfad) = std::env::var("BEWERTUNG_WASM_FRISCH") else {
            return;
        };
        let frisch = Host::neu(&std::fs::read(pfad).unwrap()).unwrap();
        let eingecheckt = Host::neu(MODUL).unwrap();
        for e in faelle() {
            let json = serde_json::to_string(&e).unwrap();
            let a = frisch.bewerten(&json, VIEL).unwrap();
            assert_eq!(a, eingecheckt.bewerten(&json, VIEL).unwrap(), "{json}");
            assert_eq!(a, bewerten_json(&json), "{json}");
        }
        let ein = mess_anfrage_aus_fixture("dlh9901_touchdown_window.json.gz");
        assert_eq!(frisch.messe(&ein, VIEL).unwrap().0, eingecheckt.messe(&ein, VIEL).unwrap().0, "messe");
    }

    #[test]
    fn rechenzeitgrenze_bricht_ab() {
        let host = Host::neu(MODUL).unwrap();
        let json = serde_json::to_string(&faelle()[0]).unwrap();
        assert_eq!(host.bewerten(&json, 10), Err(Fehler::Fuel));
    }

    /// Codex 29.10.: Ein Modul, das eine riesige Ausgabelaenge meldet, darf
    /// im Client keinen Speicher in dieser Groesse anlegen.
    #[test]
    fn ausgabe_ausserhalb_des_speichers_wird_abgewiesen() {
        let wat = r#"(module (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32) i32.const 16)
            (func (export "bewerte") (param i32 i32) (result i64) i64.const 0x00000010FFFFFFF0))"#;
        let host = Host::neu(&wat::parse_str(wat).unwrap()).unwrap();
        assert_eq!(host.bewerten("{}", VIEL), Err(Fehler::Ausgabe));
    }

    /// Export `sollband` rechnet wie das Crate — bis auf die letzte
    /// Binaerstelle: `tan` im Modul (Rust-libm in WASM) und nativ (macOS-libm)
    /// unterscheiden sich dort (29.10.: …546 gegen …547). Genau deshalb
    /// laufen Regeln nur im Modul (P-4.31); Mac, Windows und live rechnen mit
    /// derselben Modulfassung bitgleich.
    #[test]
    fn sollband_wie_nativ() {
        let host = Host::neu(MODUL).unwrap();
        let ein = r#"{"proben":[{"gs_kt":140},{"gs_kt":20},{"gs_kt":138,"is_flare":true},{"gs_kt":136,"t_ms":-300}],"winkel_grad":3.5}"#;
        let wasm: Vec<landing_scoring::sollband::Punkt> = serde_json::from_str(&host.sollband(ein, VIEL).unwrap()).unwrap();
        let nativ: Vec<landing_scoring::sollband::Punkt> = serde_json::from_str(&landing_scoring::sollband::sollband_json(ein)).unwrap();
        assert_eq!(wasm.iter().map(|p| p.index).collect::<Vec<_>>(), [0, 3]);
        for (w, n) in wasm.iter().zip(&nativ) {
            assert_eq!(w.index, n.index);
            assert!((w.soll - n.soll).abs() < 1e-9 && (w.oben - n.oben).abs() < 1e-9 && (w.unten - n.unten).abs() < 1e-9);
        }
    }

    /// Anfrage an `messe` aus einem echten Aufsetzfenster und den Grenzwerten
    /// des eingecheckten Pakets (wie der Client sie baut, `mess_anfrage`).
    pub(crate) fn mess_anfrage_aus_fixture(datei: &str) -> String {
        use std::io::Read;
        let pfad = format!("{}/../../tests/fixtures/{datei}", env!("CARGO_MANIFEST_DIR"));
        let mut text = String::new();
        flate2::read::GzDecoder::new(std::fs::File::open(pfad).unwrap()).read_to_string(&mut text).unwrap();
        let f: serde_json::Value = serde_json::from_str(&text).unwrap();
        let ms = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis();
        let kante = ms(f["edge_at"].as_str().unwrap());
        let proben: Vec<serde_json::Value> = f["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let mut p = p.clone();
                p["t_ms"] = (ms(p["at"].as_str().unwrap()) - kante).into();
                p
            })
            .collect();
        serde_json::json!({ "simulator": f["sim"], "proben": proben, "parameter": mess_parameter() }).to_string()
    }

    /// Mess-Grenzwerte aus dem eingecheckten Paket (wie `mess_anfrage` im Client).
    fn mess_parameter() -> serde_json::Value {
        let landung: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../bundle/grenzwerte/landung.json")).unwrap();
        let aufsetzen: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../bundle/grenzwerte/aufsetzen.json")).unwrap();
        let mut parameter = serde_json::json!({ "fenster_ms": aufsetzen["luecken_fenster_vor_ms"]["wert"] });
        for k in [
            "agl_min_samples", "agl_min_verschiedene", "agl_max_stillstand_ms", "agl_min_spanne_s",
            "agl_max_plausibel_fpm", "probe_toleranz_fpm", "td_agl_max_at_window_start_ft",
            "msfs_flare_agl_fit_half_window_ms", "msfs_flare_agl_fit_min_samples", "msfs_flare_min_distinct_agl",
            "msfs_flare_max_median_dt_ms", "msfs_flare_min_end_fit_distinct_agl", "msfs_flare_min_end_agl_span_ft",
            "msfs_flare_balloon_tol_ft", "msfs_flare_pitch_nosedown_veto_deg", "msfs_flare_detect_floor_fpm",
        ] {
            parameter[k] = landung[k]["wert"].clone();
        }
        parameter
    }

    /// Korpus: `messe` im eingecheckten Modul gegen das Crate nativ ueber alle
    /// Aufsetzer eines Auszugs (`{log, sim, proben}` je Zeile, Proben mit
    /// `t_ms`), dazu die Fuel-Verteilung. Nicht im normalen Lauf:
    ///   MESS_KORPUS_EIN=<anfragen.jsonl.gz> cargo test -p bewertung-host --release messe_korpus -- --ignored --nocapture
    #[test]
    #[ignore = "Korpus-Lauf, nur auf Anforderung (MESS_KORPUS_EIN)"]
    fn messe_korpus_wie_nativ() {
        use landing_scoring::aufsetz_messung::{messen_json, Messung};
        use std::io::BufRead;
        let host = Host::neu(MODUL).unwrap();
        let datei = std::fs::File::open(std::env::var("MESS_KORPUS_EIN").unwrap()).unwrap();
        let (mut fuel, mut abweichend, mut n) = (Vec::new(), Vec::new(), 0usize);
        let mut dauer_ms: Vec<f64> = Vec::new();
        for zeile in std::io::BufReader::new(flate2::read::GzDecoder::new(datei)).lines() {
            let o: serde_json::Value = serde_json::from_str(&zeile.unwrap()).unwrap();
            let ein = serde_json::json!({ "simulator": o["sim"], "proben": o["proben"], "parameter": mess_parameter() }).to_string();
            let t = std::time::Instant::now();
            let (aus, f) = host.messe(&ein, VIEL).unwrap();
            dauer_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            fuel.push(f);
            n += 1;
            let w: Messung = serde_json::from_str(&aus).unwrap();
            let v: Messung = serde_json::from_str(&messen_json(&ein)).unwrap();
            let nah = |a: Option<f32>, b: Option<f32>| match (a, b) {
                (Some(a), Some(b)) => (a - b).abs() < 1e-3,
                (a, b) => a == b,
            };
            let gleich = (w.vs_at_edge_fpm, w.vs_geometrie_fpm, w.vs_geometrie_ungefiltert_fpm, w.vs_simvar_edge_fpm, w.vs_gelaende_fpm, w.vs_eigensinken_fpm)
                == (v.vs_at_edge_fpm, v.vs_geometrie_fpm, v.vs_geometrie_ungefiltert_fpm, v.vs_simvar_edge_fpm, v.vs_gelaende_fpm, v.vs_eigensinken_fpm)
                && w.vs_at_edge_quelle == v.vs_at_edge_quelle
                && (w.flare_detected, w.flare_quality_score) == (v.flare_detected, v.flare_quality_score)
                && nah(w.vs_at_flare_end_fpm, v.vs_at_flare_end_fpm)
                && nah(w.peak_vs_pre_flare_fpm, v.peak_vs_pre_flare_fpm)
                && nah(w.flare_reduction_fpm, v.flare_reduction_fpm)
                && nah(w.flare_dvs_dt_fpm_per_sec, v.flare_dvs_dt_fpm_per_sec)
                && w.msfs.as_ref().map(|h| &h.flare_vs_source) == v.msfs.as_ref().map(|h| &h.flare_vs_source);
            if !gleich {
                abweichend.push(o["log"].as_str().unwrap_or("").to_string());
            }
        }
        fuel.sort_unstable();
        dauer_ms.sort_by(f64::total_cmp);
        println!("messe Laufzeit (dieser Bau): median {:.1} ms, max {:.1} ms", dauer_ms[n / 2], dauer_ms[n - 1]);
        println!("messe Korpus: {n} Aufsetzer, Fuel min {} median {} max {}, abweichend {:?}", fuel[0], fuel[n / 2], fuel[n - 1], abweichend);
        assert!(abweichend.is_empty());
        assert!(fuel[n - 1] < MESS_FUEL, "groesster Verbrauch {} ueber der Grenze", fuel[n - 1]);
    }

    /// Export `messe` rechnet wie das Crate. Sinkrate, Quelle und Wahl
    /// bitgleich (keine Winkelfunktion im Weg); die Abfang-Endpunkte tragen
    /// `cos(pitch)` und duerfen in der letzten Binaerstelle abweichen
    /// (Rust-libm in WASM gegen macOS-libm, wie bei `sollband_wie_nativ`).
    /// Dazu die Rechenzeit gegen [`MESS_FUEL`] (der Client nimmt dieselbe
    /// Grenze, `bewertung.rs` `MESS_FUEL_GRENZE`).
    #[test]
    fn messe_wie_nativ_am_echten_fenster() {
        use landing_scoring::aufsetz_messung::{messen_json, Messung};
        let host = Host::neu(MODUL).unwrap();
        assert!(host.hat_export("messe"), "eingechecktes Modul ohne Export messe");
        let ein = mess_anfrage_aus_fixture("dlh9901_touchdown_window.json.gz");
        let (aus, fuel) = host.messe(&ein, VIEL).unwrap();
        println!("FUEL messe DLH9901 (684 Proben): {fuel}");
        assert!(fuel < MESS_FUEL, "Fuel {fuel} ueber der Grenze {MESS_FUEL}");
        let w: Messung = serde_json::from_str(&aus).unwrap();
        let n: Messung = serde_json::from_str(&messen_json(&ein)).unwrap();
        assert_eq!((w.vs_at_edge_fpm, w.vs_geometrie_fpm, w.vs_geometrie_ungefiltert_fpm), (n.vs_at_edge_fpm, n.vs_geometrie_fpm, n.vs_geometrie_ungefiltert_fpm));
        assert_eq!((w.vs_simvar_edge_fpm, &w.vs_at_edge_quelle, w.vs_gelaende_fpm, w.vs_eigensinken_fpm), (n.vs_simvar_edge_fpm, &n.vs_at_edge_quelle, n.vs_gelaende_fpm, n.vs_eigensinken_fpm));
        assert_eq!((w.flare_detected, w.flare_quality_score, w.messung_fassung), (n.flare_detected, n.flare_quality_score, n.messung_fassung));
        let nah = |a: Option<f32>, b: Option<f32>| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() < 1e-3,
            (a, b) => a == b,
        };
        assert!(nah(w.vs_at_flare_end_fpm, n.vs_at_flare_end_fpm) && nah(w.peak_vs_pre_flare_fpm, n.peak_vs_pre_flare_fpm));
        assert!(nah(w.flare_reduction_fpm, n.flare_reduction_fpm) && nah(w.flare_dvs_dt_fpm_per_sec, n.flare_dvs_dt_fpm_per_sec));
        assert_eq!(w.msfs.as_ref().map(|h| &h.flare_vs_source), n.msfs.as_ref().map(|h| &h.flare_vs_source));
    }

    /// Ein Modul ohne `messe` (Pakete vor dieser Fassung) laedt weiter; der
    /// Client erkennt das am fehlenden Export und rechnet nativ.
    #[test]
    fn modul_ohne_messe_laedt_und_meldet_den_export_nicht() {
        let wat = r#"(module (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32) i32.const 16)
            (func (export "bewerte") (param i32 i32) (result i64) i64.const 0))"#;
        let host = Host::neu(&wat::parse_str(wat).unwrap()).unwrap();
        assert!(!host.hat_export("messe"));
        assert!(matches!(host.messe("{}", VIEL), Err(Fehler::Modul(_))));
    }

    #[test]
    fn kaputtes_modul_wird_abgewiesen() {
        assert!(matches!(Host::neu(b"\0asm kaputt"), Err(Fehler::Modul(_))));
    }

    #[test]
    fn ungueltige_eingabe_kommt_als_fehlertext_zurueck() {
        let host = Host::neu(MODUL).unwrap();
        assert!(host.bewerten("{\"x\":1}", VIEL).unwrap().contains("fehler"));
    }
}
