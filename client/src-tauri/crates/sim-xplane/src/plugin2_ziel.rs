//! Protokoll 2 im Adapter: was mit Status, Werten und Flugzeugmeldung des
//! Plugins geschieht (ADR-0004, Abschnitt 7).
//!
//! ## Abos
//!
//! | Abo | Inhalt | Rate |
//! |---|---|---|
//! | 1 | Katalog: jeder `CATALOG`-Name, jede Profil-Ersetzung aller Profile, jede Profil-Probe | 50 Hz |
//! | 2 | Zusatzwerte des Telemetrie-Monitors (nur solange er offen ist) | 20 Hz |
//! | 3–16 | „Flugzeug vermessen": alle Namen aus `LISTE`, je Abo ≤ 8192 | 5 Hz |
//!
//! Abo 1 enthaelt die Ersetzungen **aller** Profile, nicht nur des aktiven.
//! So bleibt es die ganze Sitzung unveraendert — ein Profilwechsel ist nur
//! eine andere Zuordnung beim Anwenden, kein neues Abo (bei RREF musste neu
//! abonniert werden, weil Katalog-Indizes die Drahtindizes waren).
//!
//! ## Existenz
//!
//! Der Status je Name ersetzt die Web-API-Bestaetigung (`addon_vorhanden`):
//! eine Add-on-Quelle gilt, wenn das Plugin sie „da" meldet; „fehlt" leert
//! das Feld. Einzige Ausnahme sind Namen unter `laminar/` (siehe
//! [`ist_kern`]).
//!
//! ## Uebergabe RREF ⇄ Plugin
//!
//! Der RREF-Faden ruht, solange Katalogwerte des Plugins frisch sind
//! ([`AdapterShared::p2_frisch`], hoechstens [`FRISCH`] alt). Er bestellt
//! dann seine Abos ab und reicht die Quellen weiter, die bei ihm galten
//! (`vorab`). Versiegen die Plugin-Werte, uebernimmt er sofort wieder:
//! Profil und geltende Quellen kommen aus [`AdapterShared::p2_uebergabe`],
//! die Felder behalten bis zum ersten RREF-Wert den letzten Plugin-Wert —
//! nichts faellt auf 0.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::adapter::{desired_profile, grundname, AdapterShared, ConnectionState};
use crate::dataref::{addon_quelle, FieldId, CATALOG};
use crate::plugin2::{
    anfrage_liste, zahl_fuer, AboWunsch, Ereignis, NameStatus, Wert, Ziel, MAX_ABO_ID,
};
use crate::profile::{build_active_catalog, DatarefOverride, ValueMapping, PROFILES};
use crate::vermessung::P2Messung;
use crate::web_api::AircraftInfo;
use crate::zusatz::ZUSATZ_HZ;
use crate::SUBSCRIPTION_HZ;

pub(crate) const ABO_KATALOG: u8 = 1;
pub(crate) const ABO_ZUSATZ: u8 = 2;
pub(crate) const ABO_MESSUNG_AB: u8 = 3;
/// Rate der Vermessungs-Abos — die Messung vergleicht Staende von Hand,
/// schneller braucht es nicht, und tausende Namen kosten das Plugin Zeit.
pub(crate) const MESSUNG_HZ: u32 = 5;
/// Katalogwerte des Plugins gelten so lange als frisch; danach uebernimmt
/// RREF wieder (ADR: Rueckfall nach 3 s).
pub(crate) const FRISCH: Duration = Duration::from_secs(3);

/// Namen im Namensraum von X-Planes eigenen Flugzeugen (`laminar/…`).
///
/// Gemessen 27.09.2026: `laminar/B738/*` ist bei JEDEM geladenen Flugzeug
/// registriert (X-Plane-Kern), nur das Lesen des Werts scheitert ohne 737.
/// Meldet ein Plugin solche Namen nur anhand der Registrierung als „da",
/// galte bei jedem Flugzeug das 737-Profil und der 737-Transponderknopf
/// (0 = TEST) — derselbe Fehler, den RREF bis v1.9.3 hatte. Deshalb gilt
/// ein Kern-Name nur mit einem zweiten Beleg: Wert ≠ 0 bei diesem Flugzeug,
/// Web-API-Bestaetigung oder Bestaetigung des RREF-Pfads vor der Sitzung.
pub(crate) fn ist_kern(name: &str) -> bool {
    name.starts_with("laminar/")
}

/// Was ein Name im Katalog-Abo speist.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Rolle {
    /// `CATALOG[i]`.
    Katalog(usize),
    /// Ersetzung eines Profils fuer das Feld von `CATALOG[ci]`.
    Profil {
        profil: usize,
        ci: usize,
        mapping: ValueMapping,
    },
    /// Probe eines Profils.
    Probe(usize),
}

/// Das Katalog-Abo, einmal gebaut: eindeutige Namen und ihre Rollen.
pub(crate) struct KatalogPlan {
    pub namen: Arc<Vec<String>>,
    pub rollen: Vec<Vec<Rolle>>,
    pub kern: Vec<bool>,
    /// Name-Index der Probe je Profil (Reihenfolge wie `PROFILES`).
    pub probe_index: Vec<usize>,
}

pub(crate) fn katalog_plan() -> KatalogPlan {
    fn platz(
        name: &'static str,
        index: &mut HashMap<&'static str, usize>,
        namen: &mut Vec<String>,
        rollen: &mut Vec<Vec<Rolle>>,
    ) -> usize {
        *index.entry(name).or_insert_with(|| {
            namen.push(name.to_string());
            rollen.push(Vec::new());
            namen.len() - 1
        })
    }
    let mut index = HashMap::new();
    let mut namen = Vec::new();
    let mut rollen = Vec::new();
    for (ci, e) in CATALOG.iter().enumerate() {
        let li = platz(e.name, &mut index, &mut namen, &mut rollen);
        rollen[li].push(Rolle::Katalog(ci));
    }
    let mut probe_index = Vec::with_capacity(PROFILES.len());
    for (pi, p) in PROFILES.iter().enumerate() {
        for o in p.overrides {
            let Some(ci) = CATALOG.iter().position(|e| e.field == o.field) else {
                continue;
            };
            let li = platz(o.dataref, &mut index, &mut namen, &mut rollen);
            rollen[li].push(Rolle::Profil {
                profil: pi,
                ci,
                mapping: o.mapping,
            });
        }
        let li = platz(p.probe_dataref, &mut index, &mut namen, &mut rollen);
        rollen[li].push(Rolle::Probe(pi));
        probe_index.push(li);
    }
    let kern = namen.iter().map(|n| ist_kern(n)).collect();
    KatalogPlan {
        namen: Arc::new(namen),
        rollen,
        kern,
        probe_index,
    }
}

/// Stand der Plugin-Sitzung im Adapter.
#[derive(Default)]
pub(crate) struct P2Stand {
    pub offen: bool,
    /// Je Katalog-Name: `Some(true)` da, `Some(false)` fehlt, `None` unbekannt.
    pub status: Vec<Option<bool>>,
    /// Kern-Namen (Index) mit einem Wert ≠ 0 bei diesem Flugzeug.
    pub kern_nicht_null: HashSet<usize>,
    /// Grundnamen, die beim RREF-Pfad vor der Sitzung galten.
    pub vorab: HashSet<String>,
    /// Aktives Profil laut Plugin (Index in `PROFILES`).
    pub profil: Option<usize>,
    /// Flugzeugmeldung des Plugins (nur waehrend der Sitzung).
    pub flugzeug: Option<AircraftInfo>,
    /// Laufende Vermessung ueber das Plugin.
    pub messung: Option<Arc<P2Messung>>,
    pub naechste_liste: u32,
}

fn status_von(st: &P2Stand, li: usize) -> Option<bool> {
    st.status.get(li).copied().flatten()
}

/// Gilt der Name `li` des Katalog-Abos als vorhanden?
fn gilt(plan: &KatalogPlan, st: &P2Stand, web: Option<&HashSet<String>>, li: usize) -> bool {
    if status_von(st, li) != Some(true) {
        return false;
    }
    if !plan.kern.get(li).copied().unwrap_or(false) || st.kern_nicht_null.contains(&li) {
        return true;
    }
    let g = grundname(&plan.namen[li]);
    web.is_some_and(|w| w.contains(g)) || st.vorab.contains(g)
}

/// Profil aus dem Plugin-Stand. Die Probe gilt nach [`gilt`]; der Titel
/// (aus der Flugzeugmeldung, also frisch) zaehlt nur, solange die Probe
/// nicht als fehlend gemeldet ist — wie beim RREF-Pfad.
fn profil_neu(shared: &AdapterShared, st: &mut P2Stand, web: Option<&HashSet<String>>) {
    let plan = &shared.p2_plan;
    let frisch: Vec<bool> = plan
        .probe_index
        .iter()
        .map(|&li| gilt(plan, st, web, li))
        .collect();
    let gesehen: Vec<bool> = plan
        .probe_index
        .iter()
        .zip(&frisch)
        .map(|(&li, &f)| f || status_von(st, li) == Some(false))
        .collect();
    let titel = st.flugzeug.as_ref().and_then(|f| f.descrip.clone());
    let neu = desired_profile(titel.as_deref(), &frisch, &gesehen);
    if neu != st.profil {
        match neu {
            Some(pi) => tracing::info!(
                profil = PROFILES[pi].name,
                "X-Plane-Plugin: Flugzeugprofil aktiv"
            ),
            None => tracing::info!("X-Plane-Plugin: kein Flugzeugprofil — Standardkatalog"),
        }
        st.profil = neu;
        *shared.active_catalog.lock() = build_active_catalog(neu.map(|pi| &PROFILES[pi]));
    }
}

/// Flugzeugmeldung des Plugins → [`AircraftInfo`], befuellt wie die Web-API:
/// `descrip` = `acf_descrip`, `ui_name` = `acf_ui_name`. Seit Plugin 1.0
/// kommt `titel` = UI-Name mit `beschreibung` = `acf_descrip`; aeltere
/// Plugins senden nur `titel`, und der war `acf_descrip`.
pub(crate) fn aircraft_aus_meldung(
    icao: Option<String>,
    titel: Option<String>,
    beschreibung: Option<String>,
    pfad: Option<String>,
) -> AircraftInfo {
    let (descrip, ui_name) = match beschreibung {
        Some(b) => (Some(b), titel),
        None => (titel, None),
    };
    AircraftInfo {
        descrip,
        ui_name,
        icao,
        relative_path: pfad,
        ..AircraftInfo::default()
    }
}

/// Kennung fuer den Schnappschuss: (Titel, ICAO, Kennzeichen). Das Plugin
/// hat Vorrang; die Web-API fuellt, was es nicht meldet. Das Kennzeichen
/// (nur Web-API) nur, wenn die Web-API dasselbe Flugzeug meint — sie fragt
/// alle 30 s und kann nach einem Wechsel noch das alte liefern.
pub(crate) fn kennung_ueberlagern(
    plugin: Option<&AircraftInfo>,
    web: &AircraftInfo,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(p) = plugin else {
        return (web.descrip.clone(), web.icao.clone(), web.tailnum.clone());
    };
    let dasselbe = p.relative_path.is_none() || p.relative_path == web.relative_path;
    let titel = p
        .descrip
        .clone()
        .or_else(|| dasselbe.then(|| web.descrip.clone()).flatten());
    let icao = p
        .icao
        .clone()
        .or_else(|| dasselbe.then(|| web.icao.clone()).flatten());
    let kennz = dasselbe.then(|| web.tailnum.clone()).flatten();
    (titel, icao, kennz)
}

/// UI-Name (`acf_ui_name`) fuer den Schnappschuss — gleiche Vorrangregel
/// wie [`kennung_ueberlagern`]: Plugin zuerst, die Web-API nur, wenn sie
/// dasselbe Flugzeug meint.
pub(crate) fn ui_name_ueberlagern(
    plugin: Option<&AircraftInfo>,
    web: &AircraftInfo,
) -> Option<String> {
    let leer = |s: &Option<String>| s.as_ref().filter(|x| !x.trim().is_empty()).cloned();
    let Some(p) = plugin else {
        return leer(&web.ui_name);
    };
    let dasselbe = p.relative_path.is_none() || p.relative_path == web.relative_path;
    leer(&p.ui_name).or_else(|| dasselbe.then(|| leer(&web.ui_name)).flatten())
}

impl AdapterShared {
    fn jetzt_ms(&self) -> u64 {
        (self.p2_basis.elapsed().as_millis() as u64).max(1)
    }

    /// Liefern die Katalogwerte des Plugins gerade? Dann ruht RREF.
    pub(crate) fn p2_frisch(&self) -> bool {
        if !self.p2_sitzung.load(Ordering::SeqCst) {
            return false;
        }
        let ms = self.p2_katalog_ms.load(Ordering::SeqCst);
        ms != 0 && self.jetzt_ms().saturating_sub(ms) < FRISCH.as_millis() as u64
    }

    /// Zeitpunkt der letzten Katalogwerte des Plugins.
    pub(crate) fn p2_letzte_werte(&self) -> Option<Instant> {
        let ms = self.p2_katalog_ms.load(Ordering::SeqCst);
        (ms != 0).then(|| self.p2_basis + Duration::from_millis(ms))
    }

    /// Fuer den Rueckfall auf RREF: aktives Profil und die Grundnamen der
    /// Add-on-Quellen und Proben, die unter dem Plugin galten.
    pub(crate) fn p2_uebergabe(&self) -> (Option<usize>, HashSet<String>) {
        let web = self.addon_vorhanden.lock().clone();
        let st = self.p2.lock();
        let plan = &self.p2_plan;
        let mut menge = HashSet::new();
        for li in 0..plan.namen.len() {
            let zaehlt = plan.rollen[li].iter().any(|r| match r {
                Rolle::Katalog(ci) => addon_quelle(CATALOG[*ci].field),
                Rolle::Probe(_) => true,
                Rolle::Profil { .. } => false,
            });
            if zaehlt && gilt(plan, &st, web.as_ref(), li) {
                menge.insert(grundname(&plan.namen[li]).to_string());
            }
        }
        (st.profil, menge)
    }

    /// Vom RREF-Pfad beim Ruhen: diese Quellen galten bei ihm.
    pub(crate) fn p2_vorab_setzen(&self, vorab: HashSet<String>) {
        let web = self.addon_vorhanden.lock().clone();
        let mut st = self.p2.lock();
        st.vorab = vorab;
        // Eine Kern-Probe (737) kann dadurch jetzt gelten.
        if st.offen {
            profil_neu(self, &mut st, web.as_ref());
        }
    }

    /// Flugzeugmeldung des Plugins, solange die Sitzung besteht.
    pub(crate) fn p2_flugzeug(&self) -> Option<AircraftInfo> {
        let st = self.p2.lock();
        if st.offen {
            st.flugzeug.clone()
        } else {
            None
        }
    }

    fn katalog_status(&self, liste: Vec<(usize, NameStatus)>) {
        let plan = &self.p2_plan;
        let web = self.addon_vorhanden.lock().clone();
        let mut leeren: Vec<FieldId> = Vec::new();
        {
            let mut st = self.p2.lock();
            if st.status.len() != plan.namen.len() {
                st.status = vec![None; plan.namen.len()];
            }
            let erster = st.status.iter().all(Option::is_none);
            for (li, s) in liste {
                if li >= plan.namen.len() {
                    continue;
                }
                let da = s.da();
                let alt = st.status[li];
                st.status[li] = Some(da);
                if !da {
                    st.kern_nicht_null.remove(&li);
                    if alt != Some(false) {
                        for r in &plan.rollen[li] {
                            if let Rolle::Katalog(ci) = r {
                                if addon_quelle(CATALOG[*ci].field) {
                                    leeren.push(CATALOG[*ci].field);
                                }
                            }
                        }
                    }
                }
            }
            if erster {
                let da = st.status.iter().filter(|s| **s == Some(true)).count();
                let fehlt: Vec<&str> = st
                    .status
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| **s == Some(false))
                    .map(|(i, _)| plan.namen[i].as_str())
                    .collect();
                tracing::info!(
                    da,
                    fehlt = fehlt.len(),
                    "X-Plane-Plugin: Status des Katalogs"
                );
                tracing::debug!(?fehlt, "X-Plane-Plugin: fehlende Katalognamen");
            }
            profil_neu(self, &mut st, web.as_ref());
        }
        if !leeren.is_empty() {
            let mut p = self.parsed.lock();
            for f in leeren {
                p.addon_leeren(f);
            }
        }
    }

    fn katalog_werte(&self, v: Vec<(usize, Wert)>) {
        let plan = &self.p2_plan;
        let web = self.addon_vorhanden.lock().clone();
        let mut st = self.p2.lock();
        if !st.offen {
            return;
        }
        if st.status.len() != plan.namen.len() {
            st.status = vec![None; plan.namen.len()];
        }
        // Ein Wert beweist das Dasein („ein fehlender Name liefert nie einen
        // Wert", ADR); ein Kern-Name braucht dazu einen Wert ≠ 0.
        let mut profil_pruefen = false;
        for (li, w) in &v {
            let li = *li;
            if li >= plan.namen.len() {
                continue;
            }
            if st.status[li] != Some(true) {
                st.status[li] = Some(true);
                profil_pruefen = true;
            }
            if plan.kern[li]
                && zahl_fuer(&plan.namen[li], w).is_some_and(|x| x != 0.0)
                && st.kern_nicht_null.insert(li)
            {
                profil_pruefen = true;
            }
        }
        if profil_pruefen {
            profil_neu(self, &mut st, web.as_ref());
        }
        let profil = st.profil;
        let ersetzt: &[DatarefOverride] = profil.map(|pi| PROFILES[pi].overrides).unwrap_or(&[]);
        let verbunden = {
            let mut parsed = self.parsed.lock();
            let mut seen = self.seen.lock();
            let mut last = self.last_values.lock();
            for (li, w) in &v {
                let li = *li;
                if li >= plan.namen.len() {
                    continue;
                }
                let Some(x) = zahl_fuer(&plan.namen[li], w) else {
                    continue;
                };
                for r in &plan.rollen[li] {
                    let ci = match *r {
                        Rolle::Katalog(ci) => {
                            let feld = CATALOG[ci].field;
                            if ersetzt.iter().any(|o| o.field == feld) {
                                continue;
                            }
                            if addon_quelle(feld) && !gilt(plan, &st, web.as_ref(), li) {
                                continue;
                            }
                            parsed.apply_field_f64(feld, x);
                            ci
                        }
                        Rolle::Profil {
                            profil: pi,
                            ci,
                            mapping,
                        } => {
                            if Some(pi) != profil {
                                continue;
                            }
                            if let Some(m) = mapping.map(x as f32) {
                                parsed.apply_field(CATALOG[ci].field, m);
                            }
                            ci
                        }
                        Rolle::Probe(_) => continue,
                    };
                    if let Some(s) = seen.get_mut(ci) {
                        *s = true;
                    }
                    if let Some(s) = last.get_mut(ci) {
                        *s = x as f32;
                    }
                }
            }
            parsed.got_first_packet
        };
        drop(st);
        self.p2_katalog_ms.store(self.jetzt_ms(), Ordering::SeqCst);
        if verbunden {
            let mut s = self.state.lock();
            if *s != ConnectionState::Connected {
                *s = ConnectionState::Connected;
                tracing::info!("X-Plane: erste Werte vom Plugin → Connected");
            }
        }
    }

    fn flugzeug_melden(
        &self,
        icao: Option<String>,
        titel: Option<String>,
        beschreibung: Option<String>,
        pfad: Option<String>,
    ) {
        let neu = aircraft_aus_meldung(icao, titel, beschreibung, pfad);
        let web = self.addon_vorhanden.lock().clone();
        let wechsel = {
            let mut st = self.p2.lock();
            let wechsel = st
                .flugzeug
                .as_ref()
                .is_some_and(|a| a.relative_path != neu.relative_path);
            if wechsel {
                st.kern_nicht_null.clear();
                st.vorab.clear();
            }
            tracing::info!(
                titel = ?neu.descrip,
                icao = ?neu.icao,
                pfad = ?neu.relative_path,
                wechsel,
                "X-Plane-Plugin: Flugzeug"
            );
            st.flugzeug = Some(neu);
            profil_neu(self, &mut st, web.as_ref());
            wechsel
        };
        if wechsel {
            // Quellen des alten Flugzeugs weg; der neue Status und die
            // Werte des Plugins fuellen sie gleich wieder.
            let mut p = self.parsed.lock();
            for e in CATALOG.iter().filter(|e| addon_quelle(e.field)) {
                p.addon_leeren(e.field);
            }
        }
    }

    fn messung(&self) -> Option<Arc<P2Messung>> {
        self.p2.lock().messung.clone()
    }
}

impl Ziel for AdapterShared {
    fn wunsch_generation(&self) -> u64 {
        self.p2_wunsch_gen.load(Ordering::SeqCst)
    }

    fn wuensche(&self) -> Vec<AboWunsch> {
        let mut w = vec![AboWunsch {
            id: ABO_KATALOG,
            rate: SUBSCRIPTION_HZ,
            namen: Arc::clone(&self.p2_plan.namen),
        }];
        let zusatz = self.zusatz.lock().datarefs();
        if !zusatz.is_empty() {
            w.push(AboWunsch {
                id: ABO_ZUSATZ,
                rate: ZUSATZ_HZ as u32,
                namen: Arc::new(zusatz),
            });
        }
        if let Some(m) = self.messung() {
            for (k, teil) in m.teile().into_iter().enumerate() {
                let Some(id) = u8::try_from(k)
                    .ok()
                    .and_then(|k| ABO_MESSUNG_AB.checked_add(k))
                    .filter(|id| *id <= MAX_ABO_ID)
                else {
                    break;
                };
                w.push(AboWunsch {
                    id,
                    rate: MESSUNG_HZ,
                    namen: teil,
                });
            }
        }
        w
    }

    fn anfragen(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.p2_anfragen.lock())
    }

    fn ereignis(&self, e: Ereignis) {
        match e {
            Ereignis::SitzungAuf { .. } => {
                // Werte einer frueheren Sitzung zaehlen nicht als frisch.
                self.p2_katalog_ms.store(0, Ordering::SeqCst);
                self.p2_sitzung.store(true, Ordering::SeqCst);
                let mut st = self.p2.lock();
                st.offen = true;
                st.status = vec![None; self.p2_plan.namen.len()];
                st.kern_nicht_null.clear();
                st.flugzeug = None;
            }
            Ereignis::SitzungZu => {
                {
                    let mut st = self.p2.lock();
                    st.offen = false;
                    st.flugzeug = None;
                }
                // Sofort zurueck auf RREF, nicht erst nach FRISCH. Der
                // Zeitpunkt der letzten Werte bleibt stehen: ab ihm zaehlt
                // der Stale-Waechter des RREF-Pfads.
                self.p2_sitzung.store(false, Ordering::SeqCst);
            }
            Ereignis::Status { abo, namen, st } => match abo {
                ABO_KATALOG => self.katalog_status(st),
                ABO_ZUSATZ => {
                    let mut z = self.zusatz.lock();
                    for (li, s) in st {
                        if !s.da() {
                            z.plugin_wert(li, &namen[li], None);
                        }
                    }
                }
                _ => {
                    if let Some(m) = self.messung() {
                        m.status(abo, &namen, st);
                    }
                }
            },
            Ereignis::Werte { abo, namen, v } => match abo {
                ABO_KATALOG => self.katalog_werte(v),
                ABO_ZUSATZ => {
                    let mut z = self.zusatz.lock();
                    for (li, w) in v {
                        let x = zahl_fuer(&namen[li], &w);
                        z.plugin_wert(li, &namen[li], x);
                    }
                }
                _ => {
                    if let Some(m) = self.messung() {
                        m.werte(abo, &namen, v);
                    }
                }
            },
            Ereignis::Flugzeug {
                icao,
                titel,
                beschreibung,
                pfad,
            } => self.flugzeug_melden(icao, titel, beschreibung, pfad),
            Ereignis::Liste {
                id,
                teil,
                teile,
                namen,
            } => {
                if let Some(m) = self.messung() {
                    m.liste_teil(id, teil, teile, namen);
                }
            }
            Ereignis::Fehler { grund, id, abo } => {
                // Abo 1 (Katalog) / 2 (Monitor) vom Plugin abgelehnt — z. B.
                // `speicher_limit` (Bytebudget des Plugins), `speicher`,
                // `zu_viele_namen`. Die Sitzung meldet es mit Rueckoff erneut
                // an; bis dahin laufen die Werte weiter ueber RREF (RREF ruht
                // nur, solange Katalogwerte des Plugins frisch sind).
                if let Some(a) = abo.filter(|a| *a < ABO_MESSUNG_AB) {
                    tracing::warn!(
                        abo = a,
                        grund = %grund,
                        "X-Plane-Plugin lehnt Abo ab — Werte weiter ueber RREF"
                    );
                }
                if let Some(m) = self.messung() {
                    // Fehler zur laufenden `LISTE` (nicht verfuegbar, Speicher …):
                    // die Messung faellt auf die Web-API zurueck.
                    if grund == "liste_nicht_verfuegbar" || id == Some(m.liste_id) {
                        m.liste_fehler(grund.clone());
                    }
                    // Ein Mess-Abo abgelehnt (zu viele Namen, Speicher …).
                    if let Some(a) = abo.filter(|a| *a >= ABO_MESSUNG_AB) {
                        m.abo_gescheitert(a, grund);
                    }
                }
            }
            Ereignis::AboOhneAntwort { abo, .. } => {
                // Die Sitzung versucht es weiter (Rueckoff). Die Vermessung
                // wartet nicht: dieser Lauf nimmt die Web-API (QS AP7 H1).
                if abo >= ABO_MESSUNG_AB {
                    if let Some(m) = self.messung() {
                        m.abo_gescheitert(abo, format!("Abo {abo} ohne Status"));
                    }
                } else {
                    tracing::warn!(
                        abo,
                        "X-Plane-Plugin: Abo ohne Status — Werte kommen weiter ueber RREF"
                    );
                }
            }
        }
    }
}

/// Zugang zur laufenden Plugin-Sitzung fuer „Flugzeug vermessen". Nur
/// ueber [`crate::XPlaneAdapter::plugin_zugang`] zu bekommen, und nur, wenn
/// gerade eine Sitzung besteht.
#[derive(Clone)]
pub struct PluginZugang {
    pub(crate) shared: Arc<AdapterShared>,
}

impl PluginZugang {
    pub(crate) fn offen(&self) -> bool {
        self.shared.p2.lock().offen
    }

    /// Messung anmelden und die Namensliste anfordern. Liefert die ID der
    /// `LISTE`-Anfrage.
    pub(crate) fn messung_starten(
        &self,
        neu: impl FnOnce(u32) -> Arc<P2Messung>,
    ) -> Arc<P2Messung> {
        let m = {
            let mut st = self.shared.p2.lock();
            st.naechste_liste = st.naechste_liste.wrapping_add(1).max(1);
            let m = neu(st.naechste_liste);
            st.messung = Some(Arc::clone(&m));
            m
        };
        self.shared
            .p2_anfragen
            .lock()
            .push(anfrage_liste(m.liste_id));
        m
    }

    /// Die Namen der Messung stehen fest — Abos 3.. anmelden.
    pub(crate) fn wunsch_geaendert(&self) {
        self.shared.p2_wunsch_gen.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn messung_aktiv(&self, m: &Arc<P2Messung>) -> bool {
        let st = self.shared.p2.lock();
        st.offen && st.messung.as_ref().is_some_and(|x| Arc::ptr_eq(x, m))
    }

    pub(crate) fn messung_beenden(&self, m: &Arc<P2Messung>) {
        let mut st = self.shared.p2.lock();
        if st.messung.as_ref().is_some_and(|x| Arc::ptr_eq(x, m)) {
            st.messung = None;
            drop(st);
            self.wunsch_geaendert();
        }
    }

    pub(crate) fn flugzeug(&self) -> Option<AircraftInfo> {
        self.shared.p2_flugzeug()
    }

    /// `host:port` der Web-API dieses Adapters (Rueckfall der Vermessung).
    pub(crate) fn web_api_host(&self) -> String {
        let b = self.shared.web_api.trim_end_matches('/');
        b.strip_prefix("http://").unwrap_or(b).to_string()
    }
}

#[cfg(test)]
mod tests {
    /// Plugin 1.0 und aeltere Plugins fuellen `AircraftInfo` wie die Web-API.
    #[test]
    fn flugzeugmeldung_wie_web_api() {
        let neu = super::aircraft_aus_meldung(
            Some("A20N".into()),
            Some("ToLiSs A320 Hi Def".into()),
            Some("A320 with high fidelity system modelling".into()),
            Some("Aircraft/ToLissA320_V1p1p7/a320.acf".into()),
        );
        assert_eq!(neu.ui_name.as_deref(), Some("ToLiSs A320 Hi Def"));
        assert_eq!(
            neu.descrip.as_deref(),
            Some("A320 with high fidelity system modelling"),
            "Schnappschuss-Titel bleibt die Beschreibung"
        );
        assert_eq!(neu.anzeige_titel().as_deref(), Some("ToLiSs A320 Hi Def"));
        let alt = super::aircraft_aus_meldung(
            None,
            Some("A320 with high fidelity system modelling".into()),
            None,
            None,
        );
        assert_eq!(alt.ui_name, None, "altes Plugin: titel war acf_descrip");
        assert_eq!(
            alt.anzeige_titel().as_deref(),
            Some("A320 with high fidelity system modelling")
        );
    }

    use super::*;

    /// Jeder Katalogname, jede Ersetzung und jede Probe steht genau einmal
    /// im Abo; Rollen zeigen auf die richtigen Felder.
    #[test]
    fn katalog_plan_deckt_alles_ab() {
        let p = katalog_plan();
        let menge: HashSet<&String> = p.namen.iter().collect();
        assert_eq!(menge.len(), p.namen.len(), "Namen doppelt");
        for (ci, e) in CATALOG.iter().enumerate() {
            let li = p.namen.iter().position(|n| n == e.name).unwrap();
            assert!(p.rollen[li]
                .iter()
                .any(|r| matches!(r, Rolle::Katalog(c) if *c == ci)));
        }
        for (pi, prof) in PROFILES.iter().enumerate() {
            let li = p.probe_index[pi];
            assert_eq!(p.namen[li], prof.probe_dataref);
            for o in prof.overrides {
                let li = p.namen.iter().position(|n| n == o.dataref).unwrap();
                assert!(p.rollen[li].iter().any(|r| matches!(
                    r,
                    Rolle::Profil { profil, ci, .. } if *profil == pi && CATALOG[*ci].field == o.field
                )));
            }
        }
        // CL650: Probe und Klappen-Ersetzung sind derselbe Name — zwei Rollen.
        let cl = p.probe_index[0];
        assert_eq!(p.rollen[cl].len(), 2);
        assert!(p.namen.len() < crate::plugin2::MAX_NAMEN);
    }

    #[test]
    fn kern_namen() {
        assert!(ist_kern("laminar/B738/knob/transponder_pos"));
        assert!(ist_kern("laminar/a333/switches/strobe_pos"));
        assert!(!ist_kern("AirbusFBW/AP1Engage"));
        assert!(!ist_kern("sim/cockpit2/switches/strobe_lights_on"));
    }

    /// Kern-Name: „da" allein reicht nicht; Wert ≠ 0, Web-API oder RREF-
    /// Vorab macht ihn gueltig. Andere Namen gelten mit „da".
    #[test]
    fn kern_name_braucht_zweiten_beleg() {
        let p = katalog_plan();
        let knopf = p
            .namen
            .iter()
            .position(|n| n == "laminar/B738/knob/transponder_pos")
            .unwrap();
        let toliss = p
            .namen
            .iter()
            .position(|n| n == "AirbusFBW/AP1Engage")
            .unwrap();
        let mut st = P2Stand {
            status: vec![None; p.namen.len()],
            ..P2Stand::default()
        };
        assert!(!gilt(&p, &st, None, toliss));
        st.status[toliss] = Some(true);
        st.status[knopf] = Some(true);
        assert!(gilt(&p, &st, None, toliss));
        assert!(!gilt(&p, &st, None, knopf));
        let web: HashSet<String> = ["laminar/B738/knob/transponder_pos".to_string()].into();
        assert!(gilt(&p, &st, Some(&web), knopf));
        st.vorab.insert("laminar/B738/knob/transponder_pos".into());
        assert!(gilt(&p, &st, None, knopf));
        st.vorab.clear();
        st.kern_nicht_null.insert(knopf);
        assert!(gilt(&p, &st, None, knopf));
        st.status[knopf] = Some(false);
        assert!(!gilt(&p, &st, Some(&web), knopf), "fehlt schlaegt alles");
    }

    #[test]
    fn kennung_plugin_vor_web_api() {
        let web = AircraftInfo {
            descrip: Some("Alt".into()),
            icao: Some("B738".into()),
            tailnum: Some("D-ABCD".into()),
            relative_path: Some("Aircraft/B737/b738.acf".into()),
            ..AircraftInfo::default()
        };
        assert_eq!(
            kennung_ueberlagern(None, &web),
            (
                Some("Alt".into()),
                Some("B738".into()),
                Some("D-ABCD".into())
            )
        );
        let plugin = AircraftInfo {
            descrip: Some("A330-300".into()),
            icao: Some("A333".into()),
            relative_path: Some("Aircraft/A330/a330.acf".into()),
            ..AircraftInfo::default()
        };
        // Anderes Flugzeug als die (veraltete) Web-API: kein fremdes Kennzeichen.
        assert_eq!(
            kennung_ueberlagern(Some(&plugin), &web),
            (Some("A330-300".into()), Some("A333".into()), None)
        );
        let gleich = AircraftInfo {
            descrip: None,
            relative_path: Some("Aircraft/B737/b738.acf".into()),
            ..AircraftInfo::default()
        };
        assert_eq!(
            kennung_ueberlagern(Some(&gleich), &web),
            (
                Some("Alt".into()),
                Some("B738".into()),
                Some("D-ABCD".into())
            )
        );
    }

    /// UI-Name: Plugin vor Web-API, die Web-API nur fuer dasselbe Flugzeug.
    #[test]
    fn ui_name_plugin_vor_web_api() {
        let web = AircraftInfo {
            ui_name: Some("Zibo 737-800".into()),
            relative_path: Some("Aircraft/B737/b738.acf".into()),
            ..AircraftInfo::default()
        };
        assert_eq!(
            ui_name_ueberlagern(None, &web).as_deref(),
            Some("Zibo 737-800")
        );
        let anderes = AircraftInfo {
            relative_path: Some("Aircraft/A330/a330.acf".into()),
            ..AircraftInfo::default()
        };
        assert_eq!(
            ui_name_ueberlagern(Some(&anderes), &web),
            None,
            "veraltete Web-API"
        );
        let gleich = AircraftInfo {
            relative_path: Some("Aircraft/B737/b738.acf".into()),
            ..AircraftInfo::default()
        };
        assert_eq!(
            ui_name_ueberlagern(Some(&gleich), &web).as_deref(),
            Some("Zibo 737-800")
        );
        let eigen = AircraftInfo {
            ui_name: Some("ToLiSs A320 Hi Def".into()),
            ..AircraftInfo::default()
        };
        assert_eq!(
            ui_name_ueberlagern(Some(&eigen), &web).as_deref(),
            Some("ToLiSs A320 Hi Def")
        );
        let leer = AircraftInfo {
            ui_name: Some(" ".into()),
            ..AircraftInfo::default()
        };
        assert_eq!(ui_name_ueberlagern(None, &leer), None);
    }
}
