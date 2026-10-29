//! WASM-Host fuer das Bewertungsmodul (Pflichtenheft P-4.30, P-4.31, P-4.33).
//!
//! wasmi: reines Rust, kein JIT, gleich auf Mac und Windows. Jede Bewertung
//! laeuft in einer frischen Instanz mit Rechenzeitgrenze („Fuel") und
//! Speichergrenze (64 MB); ein Fehler im Modul kann den Client so weder
//! aufhaengen noch seinen Speicher fuellen.

use wasmi::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

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
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let modul = Module::new(&engine, wasm).map_err(|e| Fehler::Modul(e.to_string()))?;
        for name in ["memory", "alloc", "bewerte"] {
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
        let bewerte = instanz
            .get_typed_func::<(u32, u32), u64>(&store, "bewerte")
            .map_err(|e| Fehler::Modul(e.to_string()))?;
        let bytes = eingabe.as_bytes();
        let n = u32::try_from(bytes.len()).map_err(|_| Fehler::Abbruch("Eingabe zu gross".into()))?;
        let ptr = alloc.call(&mut store, n).map_err(|e| abbruch(&mut store, e))?;
        speicher
            .write(&mut store, ptr as usize, bytes)
            .map_err(|e| Fehler::Abbruch(e.to_string()))?;
        let r = bewerte.call(&mut store, (ptr, n)).map_err(|e| abbruch(&mut store, e))?;
        let (aus_ptr, aus_len) = ((r >> 32) as usize, (r & 0xffff_ffff) as usize);
        let mut aus = vec![0u8; aus_len];
        speicher
            .read(&store, aus_ptr, &mut aus)
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

    #[test]
    fn rechenzeitgrenze_bricht_ab() {
        let host = Host::neu(MODUL).unwrap();
        let json = serde_json::to_string(&faelle()[0]).unwrap();
        assert_eq!(host.bewerten(&json, 10), Err(Fehler::Fuel));
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
