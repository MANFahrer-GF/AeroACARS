import { describe, expect, it } from "vitest";
import { GRENZEN, TABELLE, tabelleLesen } from "./grenzen";
import { bildMetaBauen, datenStufe } from "./meta";
import { entscheide, gruppeFuer, massePassen } from "./uebernahmeRegel";
import { serienBilden } from "./serien";
import { KEIN_FILTER, bytesText, filtern, gruppieren, platzJeFlug, seitenweise } from "./gruppen";
import { bereinigen, umschalten } from "./auswahl";
import { kontingentAnzeige, reichtKontingent, retrySekunden } from "./kontingent";
import { vorschauBauen } from "./nachrichtVorschau";
import { eintrag, fund, meta } from "./testhilfen";
import { fundLesen, kontextLesen, listeLesen } from "./normalisieren";
import tabelle from "../../../../../bundle/tabellen/bilder.json";

describe("Tabelle aus dem Paket", () => {
  it("liest Grenzen und Metadatenliste", () => {
    expect(GRENZEN.max_je_sendung).toBe(5);
    expect(GRENZEN.max_kante_px).toBe(1600);
    expect(TABELLE.metadaten.map((m) => m.feld)).toContain("naechster_flugplatz");
    expect(TABELLE.geoVorgabe).toBe("verdeckt");
  });
  it("Gegenprobe: eine kaputte Datei scheitert laut", () => {
    const kaputt = JSON.parse(JSON.stringify(tabelle));
    delete kaputt.grenzen.max_je_sendung;
    expect(() => tabelleLesen(kaputt)).toThrow(/max_je_sendung/);
    const ohneVorgabe = JSON.parse(JSON.stringify(tabelle));
    ohneVorgabe.geo.vorgabe = "weg";
    expect(() => tabelleLesen(ohneVorgabe)).toThrow(/geo/);
  });
});

describe("bild_meta Fassung 1", () => {
  it("fehlende Werte bleiben leer, nie 0", () => {
    const m = bildMetaBauen({}, "keine", null);
    expect(m).toMatchObject({ fassung: 1, hoehe_ft: null, gs_kt: null, kurs: null, breite: null, quelle: "keine", pirep_id: null });
    expect(m.zeit_utc).toBeNull();
  });
  it("Zeit kommt sonst aus der Datei, nicht aus dem Namen", () => {
    expect(bildMetaBauen({}, "keine", Date.UTC(2026, 10, 7, 9, 30)).zeit_utc).toBe("2026-11-07T09:30:00.000Z");
  });
  it("Breite ohne Laenge ist keine Position; Hoehe und Kurs entfallen dann ebenfalls", () => {
    const m = bildMetaBauen({ breite: 50, hoehe_ft: 1000, kurs: 90, naechster_flugplatz: "EDDF" }, "protokoll", 1);
    expect([m.breite, m.laenge, m.hoehe_ft, m.kurs, m.naechster_flugplatz]).toEqual([null, null, null, null, null]);
  });
  it("Werte ausserhalb des Bereichs werden verworfen statt geklemmt", () => {
    const m = bildMetaBauen({ breite: 95, laenge: 8, kurs: 361, hoehe_ft: 1e6 }, "sim", 1);
    expect(m.breite).toBeNull();
    expect(m.kurs).toBeNull();
    const ok = bildMetaBauen({ breite: 50, laenge: 8, kurs: 360, hoehe_ft: -50 }, "sim", 1);
    expect([ok.kurs, ok.hoehe_ft]).toEqual([360, -50]);
  });
  it("Stufen der Daten", () => {
    expect(datenStufe(meta())).toBe("mit_daten");
    expect(datenStufe(meta({ breite: null, laenge: null }))).toBe("nur_flug");
    expect(datenStufe(meta({ breite: null, laenge: null, pirep_id: null }))).toBe("ohne_daten");
  });
});

describe("Uebernahmeregel", () => {
  const hand = [{ pfad: "/h", aktiv: true, original_entfernen: false, art: "hand" }];
  it("Standard wird verschoben, ohne Vordergrund-Regel", () => {
    expect(entscheide(fund({ art: "standard", sim_vordergrund: null }), [])).toEqual({ tat: "uebernehmen", modus: "verschieben" });
    expect(entscheide(fund({ art: "standard", sim_vordergrund: false }), [])).toEqual({ tat: "uebernehmen", modus: "verschieben" });
  });
  it("aufgespuert: nur mit Simulator im Vordergrund, nur kopiert", () => {
    expect(entscheide(fund({ art: "aufgespuert", sim_vordergrund: true }), [])).toEqual({ tat: "uebernehmen", modus: "kopieren" });
    expect(entscheide(fund({ art: "aufgespuert", sim_vordergrund: false }), [])).toEqual({ tat: "verwerfen", grund: "vordergrund" });
    expect(entscheide(fund({ art: "aufgespuert", sim_vordergrund: null }), [])).toEqual({ tat: "verwerfen", grund: "vordergrund" });
  });
  it("aufgespuert: Pixelmasse muessen zum Simulatorfenster passen, fehlt das Fenster, wird nicht abgelehnt", () => {
    const f = fund({ art: "aufgespuert", sim_vordergrund: true, sim_fenster: { breite: 2560, hoehe: 1440 } });
    expect(entscheide(f, []).tat).toBe("uebernehmen");
    expect(entscheide({ ...f, px_breite: 1113, px_hoehe: 627 }, [])).toEqual({ tat: "verwerfen", grund: "masse" });
    expect(entscheide({ ...f, sim_fenster: null, px_breite: 1113, px_hoehe: 627 }, []).tat).toBe("uebernehmen");
    expect(massePassen({ px_breite: null, px_hoehe: null, sim_fenster: { breite: 1, hoehe: 1 } })).toBe(true);
  });
  it("hand: keine Vordergrund-Regel, kopiert; verschoben nur mit Freigabe der Quelle", () => {
    expect(entscheide(fund({ art: "hand", quelle: "/h", sim_vordergrund: false }), hand)).toEqual({ tat: "uebernehmen", modus: "kopieren" });
    expect(entscheide(fund({ art: "hand", quelle: "/h" }), [{ ...hand[0], original_entfernen: true }])).toEqual({ tat: "uebernehmen", modus: "verschieben" });
  });
  it("HDR und nicht uebernehmbare Dateien werden verworfen", () => {
    expect(entscheide(fund({ uebernehmbar: false, format: "jxr" }), [])).toEqual({ tat: "verwerfen", grund: "format" });
  });
  it("Ordnername: PIREP oder Ordner fuer ohne Flug; kein Pfad-Ausbruch", () => {
    expect(gruppeFuer("abc-123", "ohne-flug")).toBe("abc-123");
    expect(gruppeFuer(null, "ohne-flug")).toBe("ohne-flug");
    expect(gruppeFuer("../x", "ohne-flug")).toBe("ohne-flug");
  });
});

describe("Serien, Gruppen, Filter, Seiten", () => {
  const t = (s: number) => new Date(Date.UTC(2026, 10, 7, 10, 0, s)).toISOString();
  it("benachbarte Bilder werden zur Serie, einzelne bleiben einzeln", () => {
    const bilder = [0, 10, 20, 600].map((s) => eintrag({ meta: { zeit_utc: t(s) } }));
    const k = serienBilden(bilder, GRENZEN.serien_abstand_s, GRENZEN.serien_min_bilder);
    expect(k.map((x) => x.art)).toEqual(["serie", "bild"]);
    const serie = k[0];
    expect(serie.art === "serie" && serie.bilder.length).toBe(3);
  });
  it("zwei nahe Bilder sind noch keine Serie (Grenze aus dem Paket)", () => {
    const bilder = [0, 10].map((s) => eintrag({ meta: { zeit_utc: t(s) } }));
    expect(serienBilden(bilder, GRENZEN.serien_abstand_s, GRENZEN.serien_min_bilder).every((x) => x.art === "bild")).toBe(true);
  });
  const liste = [
    eintrag({ id: "a", meta: { pirep_id: "P1", zeit_utc: t(0) }, sidecar: { text: "Sonnenuntergang", favorit: true } }),
    eintrag({ id: "b", meta: { pirep_id: "P2", zeit_utc: t(5000), rufzeichen: "DLH5" }, sidecar: { sendungen: [{ sendung_id: "s", zeit_ms: 1, geo_stufe: "verdeckt", status: "gesendet" }] } }),
    eintrag({ id: "c", meta: { pirep_id: null, breite: null, laenge: null, zeit_utc: t(9000) } }),
    eintrag({ id: "x", fertig: false }),
  ];
  it("unvollstaendige Eintraege sind keine Bilder", () => {
    expect(filtern(liste, KEIN_FILTER).map((e) => e.id).sort()).toEqual(["a", "b", "c"]);
  });
  it("Gruppen: neuester Flug zuerst, Ohne Flug zuletzt", () => {
    const g = gruppieren(filtern(liste, KEIN_FILTER), GRENZEN);
    expect(g.map((x) => x.schluessel)).toEqual(["P2", "P1", ""]);
    expect(g[0].rufzeichen).toBe("DLH5");
  });
  it("Suche, Favoriten, gesendet/offen, Flug", () => {
    const ids = (f: Partial<typeof KEIN_FILTER>) => filtern(liste, { ...KEIN_FILTER, ...f }).map((e) => e.id);
    expect(ids({ suche: "sonnen" })).toEqual(["a"]);
    expect(ids({ suche: "dlh5 a320" })).toEqual(["b"]);
    expect(ids({ favoriten: true })).toEqual(["a"]);
    expect(ids({ gesendet: "gesendet" })).toEqual(["b"]);
    expect(ids({ gesendet: "offen" })).toEqual(["a", "c"]);
    expect(ids({ flug: "" })).toEqual(["c"]);
    expect(ids({ suche: "gibtesnicht" })).toEqual([]);
  });
  it("seitenweise ueber Kacheln, Gruppen werden pro Seite geschnitten", () => {
    const viele = Array.from({ length: 7 }, (_, i) => eintrag({ id: `v${i}`, meta: { pirep_id: i < 4 ? "P1" : "P2", zeit_utc: t(i * 3600) } }));
    const g = gruppieren(viele, GRENZEN);
    const s0 = seitenweise(g, 0, 3);
    const s1 = seitenweise(g, 1, 3);
    const s9 = seitenweise(g, 9, 3);
    expect(s0.seiten).toBe(3);
    expect(s0.gruppen.reduce((n, x) => n + x.kacheln.length, 0)).toBe(3);
    expect(s1.gruppen.reduce((n, x) => n + x.kacheln.length, 0)).toBe(3);
    expect(s9.seite).toBe(2);
    expect(s9.gruppen.reduce((n, x) => n + x.kacheln.length, 0)).toBe(1);
  });
  it("Platz je Flug und Bytes-Text", () => {
    const m = platzJeFlug([eintrag({ meta: { pirep_id: "P1" }, bytes: 500 }), eintrag({ meta: { pirep_id: "P1" }, bytes: 700 }), eintrag({ meta: { pirep_id: null }, bytes: 5 })]);
    expect(m.get("P1")).toBe(1200);
    expect(m.get("")).toBe(5);
    expect(bytesText(2 * 1024 * 1024, "de")).toBe("2,0 MB");
    expect(bytesText(900)).toBe("900 B");
  });
});

describe("Auswahl und Kontingent", () => {
  it("umschalten und bereinigen", () => {
    let a = umschalten(new Set(), "x");
    a = umschalten(a, "y");
    expect([...a]).toEqual(["x", "y"]);
    expect([...umschalten(a, "x")]).toEqual(["y"]);
    expect([...bereinigen(a, ["y", "z"])]).toEqual(["y"]);
  });
  it("Kontingentanzeige: Rest, leer mit Minuten, unbekannt", () => {
    expect(kontingentAnzeige({ rest: 9, grenze: 12, retry_after: 0 })).toEqual({ art: "rest", rest: 9, grenze: 12 });
    expect(kontingentAnzeige({ rest: 0, grenze: 12, retry_after: 125 })).toEqual({ art: "leer", minuten: 3 });
    expect(kontingentAnzeige({ rest: 0, grenze: 12, retry_after: 0 })).toEqual({ art: "leer", minuten: 1 });
    expect(kontingentAnzeige(null)).toEqual({ art: "unbekannt" });
  });
  it("reicht das Kontingent; retry_after aus Zahl oder Text", () => {
    expect(reichtKontingent(3, { rest: 2, grenze: 12, retry_after: 0 })).toBe(false);
    expect(reichtKontingent(2, { rest: 2, grenze: 12, retry_after: 0 })).toBe(true);
    expect(reichtKontingent(5, null)).toBe(true);
    expect(retrySekunden("120")).toBe(120);
    expect(retrySekunden(1.2)).toBe(2);
    expect(retrySekunden("morgen")).toBeUndefined();
    expect(retrySekunden(0)).toBeUndefined();
  });
});

describe("Vorschau der Nachricht (Spiegel des Servers)", () => {
  const m = meta({ rufzeichen: "GSG101", abflug: "EDDF", ziel: "LEPA" });
  it("Kopf, Ortszeile je Stufe, Text zuletzt", () => {
    const v = vorschauBauen("Thomas K", [m], "verdeckt", "Schoen!", 2000);
    expect(v.kopf).toBe("Thomas K · GSG101 · EDDF\u2013LEPA · Airbus A320 (A320)");
    expect(v.geo).toEqual([{ text: "10:00 UTC · 50.030° N 8.570° E · 12000 ft · 270° · EDDF", verdeckt: true, nummer: null }]);
    expect(vorschauBauen("T", [m], "sichtbar", "", 2000).geo[0].verdeckt).toBe(false);
    expect(vorschauBauen("T", [m], "aus", "", 2000).geo).toEqual([]);
  });
  it("ohne Position keine Ortszeile; mehrere Bilder werden nummeriert", () => {
    expect(vorschauBauen("T", [meta({ breite: null, laenge: null })], "sichtbar", "", 2000).geo).toEqual([]);
    const v = vorschauBauen("T", [m, m], "sichtbar", "", 2000);
    expect(v.geo.map((g) => g.nummer)).toEqual([1, 2]);
  });
  it("Formatierung in festen Feldern wird entfernt", () => {
    expect(vorschauBauen("a||b**", [meta({ rufzeichen: "X||Y" })], "aus", "", 2000).kopf).not.toMatch(/[|*]/);
  });
  it("Kuerzung wird gemeldet", () => {
    expect(vorschauBauen("T", [m], "aus", "x".repeat(2100), 2000).gekuerzt).toBe(true);
    expect(vorschauBauen("T", [m], "aus", "kurz", 2000).gekuerzt).toBe(false);
  });
});

describe("Antworten des Programms lesen", () => {
  it("Fund mit verschachtelter Basis: Pixelmasse bleiben Pixel", () => {
    const f = fundLesen({ fund_id: "a", breite: 2560, hoehe: 1440, meta_basis: { breite: 50.1, laenge: 8.2, pirep_id: "P" }, art: "aufgespuert", sim_vordergrund: true });
    expect(f).toMatchObject({ px_breite: 2560, px_hoehe: 1440, art: "aufgespuert" });
    expect(f?.basis).toMatchObject({ breite: 50.1, laenge: 8.2, pirep_id: "P" });
  });
  it("Fund flach: breite neben laenge ist die Breite", () => {
    const f = fundLesen({ fund_id: "a", px_breite: 100, px_hoehe: 50, breite: 50.1, laenge: 8.2 });
    expect([f?.px_breite, f?.basis.breite]).toEqual([100, 50.1]);
    expect(fundLesen({ nichts: 1 })).toBeNull();
  });
  it("fehlende Zahlen bleiben null", () => {
    expect(fundLesen({ fund_id: "a" })?.basis).toEqual({});
  });
  it("Kontext: Staende und Spur", () => {
    const k = kontextLesen({ staende: [{ zeit: "z", quelle: "protokoll", stand: { breite: 1, laenge: 2 } }, { quelle: "komisch" }], spur: [[1, 2], { lat: 3, lon: 4 }, "x"] });
    expect(k.staende.map((s) => s.quelle)).toEqual(["protokoll", "keine"]);
    expect(k.spur).toEqual([[1, 2], [3, 4]]);
    expect(kontextLesen([]).staende).toEqual([]);
  });
  it("Liste ohne Funde und Altbestand liefert leere Felder", () => {
    const l = listeLesen({ eintraege: [{ id: "x", vollstaendig: true }, { kaputt: 1 }] });
    expect(l.eintraege).toHaveLength(1);
    expect(l.funde).toEqual([]);
    expect(l.altbestand).toEqual([]);
  });
});
