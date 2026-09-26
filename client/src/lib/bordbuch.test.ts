import { describe, expect, it } from "vitest";
import { bilanz, meilensteine, routine, sichtbar, zelle, type Eintrag, type Punkt, type Schalter } from "./bordbuch";

const p = (regel: Punkt["regel"], schalter: Schalter, status: Punkt["status"], art: Punkt["art"] = "pflicht"): Punkt => ({
  regel, schalter, abschnitt: "start", art, status, auto_status: status,
  zeit: null, hoehe_ft: null, stellung: null, grund: null, markiert_at: null,
});

const eintrag = (tag: number, punkte: Punkt[], aus_grund: string | null = null): Eintrag => ({
  schema: 1, pirep_id: `P${tag}`, erstellt_at: `2026-09-${String(tag).padStart(2, "0")}T10:00:00Z`,
  updated_at: "", client_version: "t", flug: {}, klasse: "airliner", klasse_quelle: "icao",
  regelwerk: "ifr", nacht_start: false, nacht_landung: false, zeitquelle: "sim",
  eingeschaltet: ["beacon", "strobes"], aus_grund, punkte,
  rollen_max_abflug_kt: 0, rollen_max_ankunft_kt: 0, rolltempo_grenze_kt: 30, profil: [],
});

describe("Bordbuch-Anzeige", () => {
  it("zählt wie Rust: nur eingeschaltete, messbare Pflichtpunkte; nach ATC zählt", () => {
    const ps = [
      p("beacon_anlassen", "beacon", "erledigt"),
      p("strobes_start", "strobes", "nach_atc"),
      p("nav_lichter", "nav", "diesmal_ohne"),
      p("tcas_start", "transponder", "nicht_messbar"),
      p("apu_reiseflug", "apu", "erledigt", "bestaetigung"),
      p("spoiler_landung", "spoiler", "diesmal_ohne"),
    ];
    expect(bilanz(ps, ["beacon", "strobes", "nav", "transponder", "apu"])).toEqual({ ok: 2, von: 3 });
  });

  it("Bestätigung erscheint nur, wenn erledigt; ‚kam nicht vor' und Ausgeschaltetes nie", () => {
    const an: Schalter[] = ["apu", "beacon"];
    expect(sichtbar(p("apu_reiseflug", "apu", "erledigt", "bestaetigung"), an)).toBe(true);
    expect(sichtbar(p("apu_reiseflug", "apu", "diesmal_ohne", "bestaetigung"), an)).toBe(false);
    expect(sichtbar(p("beacon_anlassen", "beacon", "nicht_anwendbar"), an)).toBe(false);
    expect(sichtbar(p("beacon_anlassen", "beacon", "diesmal_ohne"), an)).toBe(true);
    expect(sichtbar(p("strobes_start", "strobes", "erledigt"), an)).toBe(false);
    expect(sichtbar(p("klappen_start", "beacon", "erledigt", null), an)).toBe(false);
  });

  it("Routine: Serie bricht nur bei ‚diesmal ohne', nicht bei ‚nicht messbar'", () => {
    const e = [
      eintrag(1, [p("beacon_anlassen", "beacon", "diesmal_ohne")]),
      eintrag(2, [p("beacon_anlassen", "beacon", "erledigt")]),
      eintrag(3, [p("beacon_anlassen", "beacon", "nicht_messbar")]),
      eintrag(4, [p("beacon_anlassen", "beacon", "nach_atc")]),
      eintrag(5, [p("beacon_anlassen", "beacon", "erledigt")]),
      eintrag(6, [p("beacon_anlassen", "beacon", "erledigt")], "ga"), // aus → zählt nicht
    ];
    const [z] = routine(e, ["beacon"]);
    expect(z.zellen).toEqual(["ohne", "ok", "nm", "atc", "ok"]);
    expect(z.serie).toBe(3);
    expect(z.laengste).toBe(3);
    expect(meilensteine([z])).toEqual([{ art: "rekord", schalter: "beacon", serie: 3 }]);
  });

  it("zwei Punkte je Schalter (Rolltempo ab/an): ein ‚diesmal ohne' färbt", () => {
    const e = eintrag(1, [
      { ...p("rolltempo_abflug", "rolltempo", "erledigt") },
      { ...p("rolltempo_ankunft", "rolltempo", "diesmal_ohne") },
    ]);
    expect(zelle(e, "rolltempo")).toBe("ohne");
    expect(zelle(e, "beacon")).toBe("leer");
  });
});
