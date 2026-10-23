// Was jede Taste zeigt — reine Funktionen vom Zustand auf ein Tastenbild,
// ohne StreamDeck und ohne Netz, damit die Tests sie direkt prüfen können.
//
// Zustand `s`: { erreichbar, status, kabine, hoppie, debrief, jetzt }
//   erreichbar  AeroACARS antwortet überhaupt
//   status      /panel/status (null = kein Flug)
//   kabine      /panel/kabine
//   hoppie      /panel/hoppie (null = nicht verfügbar)
//   debrief     /panel/debrief (null = noch keine Landung)
//   jetzt       Date
//
// Ein Tastenbild: { label, wert, unter, ton, icon, balken?, stufen?, gross? }
//   ton: "aus" | "neutral" | "ok" | "warn" | "crit" | "ctl"

const PHASEN = {
  preflight: ["Vorbereitung", 0, "neutral"],
  boarding: ["Boarding", 0, "neutral"],
  pushback: ["Pushback", 0, "neutral"],
  taxi_out: ["Rollen", 0, "neutral"],
  takeoff_roll: ["Startlauf", 1, "ok"],
  takeoff: ["Start", 1, "ok"],
  climb: ["Steigflug", 1, "ok"],
  cruise: ["Reiseflug", 2, "ok"],
  holding: ["Warteschl.", 3, "warn"],
  descent: ["Sinkflug", 3, "warn"],
  approach: ["Anflug", 4, "warn"],
  final: ["Endanflug", 4, "warn"],
  landing: ["Landung", 4, "warn"],
  taxi_in: ["Rollen", 5, "neutral"],
  blocks_on: ["Am Stand", 5, "neutral"],
  arrived: ["Angekommen", 5, "neutral"],
  pirep_submitted: ["PIREP ab", 5, "neutral"],
};

/** Kurznamen der Kabinenansagen (Slot-Namen aus cabin-announcements). */
export const ANSAGEN = {
  boarding: "Boarding",
  boarding_abgeschlossen: "Boarding fertig",
  begruessung_tueren_zu: "Begrüßung",
  kommando_tueren_scharf: "Türen scharf",
  sicherheit: "Sicherheit",
  kapitaen_begruessung: "Kapitän",
  kommando_start: "Start",
  chime10000_steigflug: "10.000 ft",
  anschnallzeichen_aus: "Gurte aus",
  reiseflug25: "Service",
  reiseflug50: "Reiseflug",
  reiseflug80: "Reiseflug",
  ende_service: "Service-Ende",
  kapitaen_sinkflug: "Sinkflug",
  kabine_sinkflug: "Sinkflug",
  chime10000_sinkflug: "10.000 ft",
  kommando_landung_vorbereiten: "Landung vorb.",
  kommando_plaetze_landung: "Plätze",
  nach_der_landung: "Ankunft",
  kommando_tueren_entschaerfen: "Türen",
  aussteigen: "Aussteigen",
  verspaetung_gate: "Verspätung",
  turbulenz: "Turbulenz",
  turbulenz_vorbei: "Turb. vorbei",
  warteschleife: "Warteschl.",
  durchstarten: "Durchstarten",
  umleitung: "Umleitung",
};

// ---- Formatierung ---------------------------------------------------------

const dez = (n, stellen = 1) => n.toFixed(stellen).replace(".", ",");
const tonnen = (kg) => (kg == null ? "–" : `${dez(kg / 1000)} t`);
const zahl = (n) => (n == null ? null : Number(n));
const prozent = (v) => (v == null ? "–" : `${Math.round(v * 100)} %`);

export function dauer(abIso, bisIso, jetzt) {
  if (!abIso) return null;
  const ab = Date.parse(abIso);
  const bis = bisIso ? Date.parse(bisIso) : jetzt.getTime();
  if (!Number.isFinite(ab) || !Number.isFinite(bis) || bis < ab) return null;
  const min = Math.floor((bis - ab) / 60000);
  return `${Math.floor(min / 60)}:${String(min % 60).padStart(2, "0")}`;
}

export function hoehe(ft) {
  if (ft == null) return "";
  if (ft >= 10000) return `FL${String(Math.round(ft / 100)).padStart(3, "0")}`;
  return `${Math.round(ft / 100) * 100} ft`.replace(/\B(?=(\d{3})+(?!\d))/g, ".");
}

export function wind(m) {
  if (!m || m.wind_speed_kt == null) return null;
  const kt = Math.round(m.wind_speed_kt);
  if (kt === 0) return "still";
  const dir = m.wind_direction_deg == null ? "VRB" : String(Math.round(m.wind_direction_deg) % 360 || 360).padStart(3, "0");
  const boe = m.gust_kt != null ? `G${Math.round(m.gust_kt)}` : "";
  return `${dir}/${String(kt).padStart(2, "0")}${boe}`;
}

const uhr = (d, utc) =>
  utc
    ? `${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}`
    : `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;

// ---- Musik-Stumm ----------------------------------------------------------

/** Stumm = klingt gerade nicht, obwohl eine Lautstärke eingestellt ist. */
export const musikStumm = (k) => !!k && k.musik_live <= 0.001 && k.musik_gespeichert > 0.001;

/** Neue gespeicherte Lautstärke für +/− in Zehnerschritten. */
export const schritt = (wert, richtung) => Math.min(1, Math.max(0, Math.round((wert + richtung * 0.1) * 10) / 10));

// ---- Tasten ---------------------------------------------------------------

const aus = (label, icon) => ({ label, wert: "aus", unter: "AeroACARS", ton: "aus", icon });
const keinFlug = (label, icon) => ({ label, wert: "–", unter: "kein Flug", ton: "aus", icon });

/** Tasten, die nur mit aktivem Flug etwas zeigen. */
function flugTaste(label, icon, bild) {
  return (s) => {
    if (!s.erreichbar) return aus(label, icon);
    if (!s.status) return keinFlug(label, icon);
    return { label, icon, ton: "neutral", ...bild(s.status, s) };
  };
}

function kabinenTaste(label, icon, bild) {
  return (s) => {
    if (!s.erreichbar) return aus(label, icon);
    if (!s.kabine) return { label, icon, wert: "–", unter: "Client zu alt", ton: "aus" };
    return { label, icon, ton: "ctl", ...bild(s.kabine, s) };
  };
}

export function hinweis(st) {
  if (!st.live) return { wert: "Sim weg", unter: "keine Daten", ton: "crit", icon: "warn" };
  if (st.paused_since) return { wert: "Pause", unter: "Sim angehalten", ton: "warn", icon: "pause" };
  if (st.abgabe_sperre) return { wert: "Abgabe", unter: "gesperrt", ton: "crit", icon: "warn" };
  if (st.connection_state && st.connection_state !== "live") return { wert: "Server", unter: "keine Verbindung", ton: "warn", icon: "warn" };
  if (st.divert_hint?.divert_to) return { wert: st.divert_hint.divert_to, unter: "Umleitung?", ton: "warn", icon: "divert" };
  return { wert: "OK", unter: "alles normal", ton: "ok", icon: "ok" };
}

export const TASTEN = {
  phase: {
    name: "Phase und Zurück",
    tip: "Flugphase und Höhe. Drücken führt zurück ins vorherige Profil.",
    icon: "plane",
    bild: (s) => {
      const b = flugTaste("Phase", "plane", (st) => {
        const [text, stufe, ton] = PHASEN[st.phase] ?? [st.phase ?? "–", 0, "neutral"];
        return { wert: text, gross: false, unter: hoehe(st.last_known_alt_ft), ton, stufen: stufe };
      })(s);
      return { ...b, zurueck: true };
    },
  },
  flug: {
    name: "Rufzeichen",
    tip: "Rufzeichen und Strecke.",
    icon: "tag",
    bild: flugTaste("Flug", "tag", (st) => ({ wert: st.callsign || "–", unter: `${st.dpt_airport ?? "?"}›${st.arr_airport ?? "?"}`, klein: true })),
  },
  gates: {
    name: "Gates",
    tip: "Abflug- und Ankunftsgate.",
    icon: "gate",
    bild: flugTaste("Gates", "gate", (st) => ({ wert: `${st.dep_gate || "–"}›${st.arr_gate || "–"}`, unter: "Start › Ziel", klein: true })),
  },
  block: {
    name: "Blockzeit",
    tip: "Zeit seit Off-Block.",
    icon: "clock",
    bild: flugTaste("Block", "clock", (st, s) => ({ wert: dauer(st.block_off_at, st.block_on_at, s.jetzt) ?? "–", unter: st.block_off_at ? "seit Off-Block" : "noch am Stand" })),
  },
  luft: {
    name: "Flugzeit",
    tip: "Zeit seit dem Abheben.",
    icon: "watch",
    bild: flugTaste("In Luft", "watch", (st, s) => ({ wert: dauer(st.takeoff_at, st.landing_at, s.jetzt) ?? "–", unter: st.takeoff_at ? "seit Abheben" : "noch nicht" })),
  },
  sprit: {
    name: "Sprit",
    tip: "Sprit an Bord laut Simulator und geplante Reserve.",
    icon: "fuel",
    bild: flugTaste("Sprit", "fuel", (st) => {
      const kg = zahl(st.sim_fuel_kg ?? st.last_known_fuel_kg);
      const res = zahl(st.planned_reserve_kg);
      const ton = kg != null && res ? (kg <= res ? "crit" : kg <= res * 1.2 ? "warn" : "neutral") : "neutral";
      return { wert: tonnen(kg), unter: res ? `Reserve ${tonnen(res)}` : "an Bord", ton };
    }),
  },
  gewicht: {
    name: "Gewichte",
    tip: "ZFW und Startgewicht, rot über dem Höchstwert.",
    icon: "weight",
    bild: flugTaste("ZFW", "weight", (st) => {
      const zfw = zahl(st.sim_zfw_kg);
      const tow = zahl(st.sim_tow_kg);
      const ueber = (zfw && st.planned_max_zfw_kg && zfw > st.planned_max_zfw_kg) || (tow && st.planned_max_tow_kg && tow > st.planned_max_tow_kg);
      return { wert: tonnen(zfw), unter: `TOW ${tonnen(tow)}`, ton: ueber ? "crit" : "neutral" };
    }),
  },
  ldw: {
    name: "Landegewicht",
    tip: "Geplantes Landegewicht und Höchstwert.",
    icon: "weight",
    bild: flugTaste("LDW Plan", "weight", (st) => {
      const ldw = zahl(st.planned_ldw_kg);
      const max = zahl(st.planned_max_ldw_kg);
      return { wert: tonnen(ldw), unter: max ? `Max ${tonnen(max)}` : "kein OFP", ton: ldw && max && ldw > max ? "crit" : "neutral" };
    }),
  },
  wetter_start: {
    name: "Wetter Start",
    tip: "Wind und QNH am Startflughafen.",
    icon: "wind",
    bild: flugTaste("Start", "wind", (st) => wetterBild(st.dep_metar_decoded, st.dpt_airport)),
  },
  wetter_ziel: {
    name: "Wetter Ziel",
    tip: "Wind und QNH am Zielflughafen.",
    icon: "wind",
    bild: flugTaste("Ziel", "wind", (st) => wetterBild(st.arr_metar_decoded, st.arr_airport)),
  },
  bahn: {
    name: "Landebahn",
    tip: "Erwartete oder angeflogene Landebahn.",
    icon: "runway",
    bild: flugTaste("Bahn", "runway", (st) => ({
      wert: st.approach_runway || st.predicted_runway || "–",
      unter: st.approach_runway ? "im Anflug" : st.predicted_runway ? "erwartet" : "noch offen",
    })),
  },
  ausweich: {
    name: "Ausweichflughafen",
    tip: "Geplanter Ausweichflughafen, gelb bei vermuteter Umleitung.",
    icon: "divert",
    bild: flugTaste("Ausweich", "divert", (st) =>
      st.divert_hint?.divert_to
        ? { wert: st.divert_hint.divert_to, unter: "Umleitung?", ton: "warn" }
        : { wert: st.planned_alternate || "–", unter: "geplant" },
    ),
  },
  landung: {
    name: "Landung",
    tip: "Sinkrate und G-Last nach dem Aufsetzen.",
    icon: "land",
    bild: flugTaste("Landung", "land", (st) =>
      st.landing_rate_fpm == null
        ? { wert: "—", unter: "noch nicht" }
        : { wert: String(Math.round(st.landing_rate_fpm)), unter: `fpm · ${dez(st.landing_g_force ?? 0, 2)} G`, klein: true },
    ),
  },
  bewertung: {
    name: "Landebewertung",
    tip: "Note und Punkte, dieselbe Bewertung wie in App und PIREP.",
    icon: "medal",
    bild: flugTaste("Note", "medal", (st, s) => {
      const d = s.debrief;
      if (!st.landing_at || !d || d.score_numeric == null) return { wert: "—", unter: st.landing_at ? "wird bewertet" : "noch nicht" };
      const ton = { smooth: "ok", acceptable: "ok", firm: "warn", hard: "crit", severe: "crit" }[d.score_label] ?? "neutral";
      return { wert: d.grade_letter || String(d.score_numeric), unter: `${d.score_numeric}/100`, ton };
    }),
  },
  durchstarten: {
    name: "Durchstarten",
    tip: "Durchstartmanöver und Touch-and-Go in diesem Flug.",
    icon: "ga",
    bild: flugTaste("Durchst.", "ga", (st) => ({ wert: String(st.go_around_count ?? 0), unter: `T&G ${st.touch_and_go_count ?? 0}` })),
  },
  hinweis: {
    name: "Hinweis",
    tip: "Die wichtigste Meldung: Sim getrennt, Pause, Abgabe gesperrt, Server, Umleitung.",
    icon: "ok",
    bild: flugTaste("Hinweis", "ok", (st) => hinweis(st)),
  },
  verbindung: {
    name: "Verbindung",
    tip: "Verbindung zum Simulator und zum GSG-Server.",
    icon: "signal",
    bild: flugTaste("Netz", "signal", (st) => {
      const sim = !!st.live;
      const srv = st.connection_state === "live";
      return { wert: `Sim ${sim ? "✓" : "✗"}`, unter: `Server ${srv ? "✓" : "✗"}`, ton: sim && srv ? "ok" : "crit", klein: true };
    }),
  },
  hoppie: {
    name: "Hoppie",
    tip: "Angemeldete Station und offene Freigaben.",
    icon: "msg",
    bild: (s) => {
      if (!s.erreichbar) return aus("Hoppie", "msg");
      const h = s.hoppie;
      if (!h || !h.connected) return { label: "Hoppie", icon: "msg", wert: "–", unter: "nicht verbunden", ton: "aus" };
      const offen = h.pending_uplink_count ?? 0;
      return {
        label: "Hoppie",
        icon: "msg",
        wert: h.logged_on ? h.station_id || "an" : "kein Logon",
        unter: offen ? `${offen} offen` : "nichts offen",
        ton: offen ? "warn" : "neutral",
        klein: true,
      };
    },
  },
  utc: {
    name: "UTC-Uhr",
    tip: "Zulu-Zeit, darunter Ortszeit.",
    icon: "globe",
    bild: (s) => ({ label: "UTC", icon: "globe", wert: `${uhr(s.jetzt, true)}Z`, unter: `lokal ${uhr(s.jetzt, false)}`, ton: "neutral" }),
  },
  tempo: {
    name: "Geschwindigkeit",
    tip: "Geschwindigkeit über Grund und Kurs.",
    icon: "gauge",
    bild: flugTaste("GS", "gauge", (st) => {
      const gs = st.live?.gs_kt ?? st.last_known_gs_kt;
      const kurs = st.last_known_heading_deg;
      return { wert: gs == null ? "–" : `${Math.round(gs)} kt`, unter: kurs == null ? "" : `Kurs ${String(Math.round(kurs) % 360).padStart(3, "0")}°`, klein: true };
    }),
  },
  strecke: {
    name: "Geflogene Strecke",
    tip: "Bisher geflogene Strecke.",
    icon: "route",
    bild: flugTaste("Geflogen", "route", (st) => ({ wert: `${Math.round(st.distance_nm ?? 0)} nm`, unter: "seit Start", klein: true })),
  },
  ansage: {
    name: "Nächste Ansage",
    tip: "Welche Kabinenansage als Nächstes kommt; gelb, wenn sie auf das Anschnallzeichen wartet.",
    icon: "mega",
    bild: kabinenTaste("Ansage", "mega", (k) => {
      if (!k.an) return { wert: "aus", unter: "Ansagen aus", ton: "aus" };
      if (k.stumm) return { wert: "stumm", unter: "Kabine", ton: "crit", icon: "mute" };
      if (!k.flug) return { wert: "–", unter: "kein Flug", ton: "aus" };
      if (k.hinweis === "anschnallzeichen") return { wert: "wartet", unter: "Gurte aus", ton: "warn", icon: "belt", klein: true };
      const n = k.naechste;
      if (!n) return { wert: "–", unter: "nichts mehr", ton: "neutral" };
      const wann = n.wann ? `ca. ${uhr(new Date(n.wann * 1000), true)}Z` : n.faellig ? "gleich" : "";
      return { wert: ANSAGEN[n.slot] ?? n.slot, unter: wann, ton: "neutral", klein: true };
    }),
  },
  kabine_stumm: {
    name: "Kabine stumm",
    tip: "Eine Sekunde halten: Kabine stumm (laufende und wartende Ansagen entfallen). Kurz drücken: Ton wieder an.",
    icon: "spk",
    bild: kabinenTaste("Kabine", "spk", (k) =>
      k.stumm ? { wert: "Stumm", unter: "drücken = an", ton: "crit", icon: "mute", klein: true } : { wert: "Ton an", unter: "halten = stumm", klein: true },
    ),
  },
  ansage_leiser: {
    name: "Ansagen leiser",
    tip: "Ansagen 10 % leiser.",
    icon: "down",
    bild: kabinenTaste("Ansagen", "down", () => ({ gross: "down", unter: "leiser" })),
  },
  ansage_pegel: {
    name: "Ansagen-Lautstärke",
    tip: "Lautstärke der Ansagen.",
    icon: "spk",
    bild: kabinenTaste("Ansagen", "spk", (k) => ({ wert: prozent(k.lautstaerke_gespeichert), balken: k.lautstaerke_live ?? 0 })),
  },
  ansage_lauter: {
    name: "Ansagen lauter",
    tip: "Ansagen 10 % lauter.",
    icon: "up",
    bild: kabinenTaste("Ansagen", "up", () => ({ gross: "up", unter: "lauter" })),
  },
  boarding_musik: {
    name: "Boarding-Musik",
    tip: "Boarding-Musik an oder aus. Aus stoppt sofort.",
    icon: "music",
    bild: kabinenTaste("Boarding", "music", (k) =>
      k.boarding_musik ? { wert: "Musik an", unter: "drücken = aus", klein: true } : { wert: "Musik aus", unter: "drücken = an", ton: "neutral", icon: "musicOff", klein: true },
    ),
  },
  musik_leiser: {
    name: "Musik leiser",
    tip: "Boarding-Musik 10 % leiser.",
    icon: "down",
    bild: kabinenTaste("Musik", "down", () => ({ gross: "down", unter: "leiser" })),
  },
  musik_pegel: {
    name: "Musik-Lautstärke und stumm",
    tip: "Drücken schaltet nur die Musik stumm und wieder laut. Das Stück läuft weiter, die Einstellung bleibt.",
    icon: "music",
    bild: kabinenTaste("Musik", "music", (k) =>
      musikStumm(k)
        ? { wert: "stumm", unter: "drücken = laut", ton: "warn", icon: "musicOff", klein: true }
        : { wert: prozent(k.musik_gespeichert), balken: k.musik_live ?? 0 },
    ),
  },
  musik_lauter: {
    name: "Musik lauter",
    tip: "Boarding-Musik 10 % lauter.",
    icon: "up",
    bild: kabinenTaste("Musik", "up", () => ({ gross: "up", unter: "lauter" })),
  },
  cdm: {
    name: "VATSIM CDM",
    tip: "Öffnet das VATSIM-CDM-Fenster von AeroACARS oder holt es nach vorn.",
    icon: "win",
    bild: (s) => (s.erreichbar ? { label: "VATSIM", icon: "win", wert: "CDM", unter: "öffnen", ton: "ctl", klein: true } : aus("VATSIM", "win")),
  },
};

function wetterBild(m, icao) {
  const w = wind(m);
  if (!w) return { label: icao || "–", wert: "–", unter: "kein METAR" };
  return { label: m.icao || icao, wert: w, unter: m.qnh_hpa != null ? `Q${Math.round(m.qnh_hpa)}` : "", klein: true };
}

// ---- Drehregler (StreamDeck +) --------------------------------------------

/** Seiten von Regler 3 (Flugdaten) und 4 (Wetter), in dieser Reihenfolge. */
export const INFO_SEITEN = ["block", "luft", "sprit", "gewicht", "tempo", "strecke"];
export const WETTER_SEITEN = ["wetter_start", "wetter_ziel", "bahn"];
