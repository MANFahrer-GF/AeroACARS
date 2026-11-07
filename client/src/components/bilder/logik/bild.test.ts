import { describe, expect, it } from "vitest";
import { alsBase64, istJpeg, pixelMasse, qualitaetWaehlen, sha256Hex, zielMasse } from "./bildverarbeitung";
import { app1Anzahl, dms, exifEinsetzen, exifGpsLesen, exifGpsSegment } from "./exifGps";
import {
  FEHLER_CODES, antwortEinarbeiten, aufrufFehler, auswahlNachSenden, fehlerCodeNormal, offeneIds, sendeStart, sendungIdWaehlen, zusammenfassung,
  type Sendestand,
} from "./sendezustand";

/** Kleinstes gueltiges JPEG-Geruest mit SOF0 (Breite 640, Hoehe 480) und einem eigenen APP1. */
function jpegMit(app1 = false): Uint8Array {
  const sof = [0xff, 0xc0, 0, 11, 8, 0x01, 0xe0, 0x02, 0x80, 1, 1, 0x11, 0];
  const a1 = app1 ? [0xff, 0xe1, 0, 8, 0x58, 0x4d, 0x50, 0, 0, 0] : [];
  return Uint8Array.from([0xff, 0xd8, ...a1, ...sof, 0xff, 0xda, 0, 4, 0, 0, 1, 2, 3, 0xff, 0xd9]);
}

describe("Verkleinern", () => {
  it("laengste Kante hoechstens 1600, Seitenverhaeltnis bleibt, nie vergroessern", () => {
    expect(zielMasse(2560, 1440, 1600)).toEqual({ breite: 1600, hoehe: 900, verkleinert: true });
    expect(zielMasse(1440, 2560, 1600)).toEqual({ breite: 900, hoehe: 1600, verkleinert: true });
    expect(zielMasse(800, 600, 1600)).toEqual({ breite: 800, hoehe: 600, verkleinert: false });
    expect(zielMasse(1600, 1600, 1600).verkleinert).toBe(false);
    expect(() => zielMasse(0, 10, 1600)).toThrow();
  });
  it("Qualitaetsleiter: erste Stufe unter der Grenze, sonst die kleinste Stufe", async () => {
    const gross = (q: number) => Promise.resolve(new Uint8Array(Math.round(q * 1000)));
    expect((await qualitaetWaehlen(gross, [0.9, 0.7, 0.5], 800)).length).toBe(700);
    expect((await qualitaetWaehlen(gross, [0.9, 0.7, 0.5], 100)).length).toBe(500);
    expect((await qualitaetWaehlen(gross, [0.9], 5000)).length).toBe(900);
  });
  it("Pixelmasse aus JPEG und PNG", () => {
    expect(pixelMasse(jpegMit())).toEqual({ breite: 640, hoehe: 480 });
    const png = new Uint8Array(24);
    png.set([0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 0x0a, 0, 0, 0, 0x05, 0xa0]);
    expect(pixelMasse(png)).toEqual({ breite: 2560, hoehe: 1440 });
    expect(pixelMasse(Uint8Array.from([1, 2, 3, 4]))).toBeNull();
    expect(istJpeg(jpegMit())).toBe(true);
  });
  it("SHA-256 und Base64 stimmen mit bekannten Werten", async () => {
    const abc = Uint8Array.from([97, 98, 99]);
    expect(await sha256Hex(abc)).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    expect(alsBase64(abc)).toBe("YWJj");
    expect(alsBase64(Uint8Array.from([97, 98]))).toBe("YWI=");
    expect(alsBase64(Uint8Array.from([97]))).toBe("YQ==");
    expect(alsBase64(new Uint8Array())).toBe("");
  });
});

describe("EXIF-GPS", () => {
  it("Grad, Minuten, Sekunden", () => {
    expect(dms(50.5)).toEqual([[50, 1], [30, 1], [0, 10000]]);
    expect(dms(-8.57)[0]).toEqual([8, 1]);
    expect(dms(0.99999999999)[0][0]).toBe(1); // Uebertrag Sekunden -> Minuten -> Grad
  });
  const fall = [
    { n: "Nord/Ost", breite: 50.0333, laenge: 8.5706, hoehe_ft: 354 },
    { n: "Sued/West", breite: -33.9461, laenge: -118.4085, hoehe_ft: 126 },
    { n: "unter dem Meer", breite: 31.7, laenge: 35.4, hoehe_ft: -1400 },
  ];
  for (const f of fall) {
    it(`Rundlauf ${f.n}`, () => {
      const seg = exifGpsSegment({ breite: f.breite, laenge: f.laenge, hoehe_ft: f.hoehe_ft, kurs: 270, zeit_utc: "2026-11-07T10:20:30Z" });
      const jpeg = exifEinsetzen(jpegMit(), seg);
      const g = exifGpsLesen(jpeg)!;
      expect(g.breite).toBeCloseTo(f.breite, 4);
      expect(g.laenge).toBeCloseTo(f.laenge, 4);
      expect(g.hoehe_m!).toBeCloseTo(f.hoehe_ft * 0.3048, 1);
      expect(pixelMasse(jpeg)).toEqual({ breite: 640, hoehe: 480 });
    });
  }
  it("Segment ist wohlgeformt: Laenge, Kopf, genau ein APP1, andere APP1 entfernt", () => {
    const seg = exifGpsSegment({ breite: 1, laenge: 2, hoehe_ft: null, kurs: null, zeit_utc: null });
    expect([seg[0], seg[1]]).toEqual([0xff, 0xe1]);
    expect(((seg[2] << 8) | seg[3])).toBe(seg.length - 2);
    expect(String.fromCharCode(...seg.subarray(4, 8))).toBe("Exif");
    const jpeg = exifEinsetzen(jpegMit(true), seg);
    expect(app1Anzahl(jpegMit(true))).toBe(1);
    expect(app1Anzahl(jpeg)).toBe(1);
    expect([jpeg[0], jpeg[1], jpeg[2], jpeg[3]]).toEqual([0xff, 0xd8, 0xff, 0xe1]);
    expect(String.fromCharCode(...jpeg.subarray(jpeg.length - 14, jpeg.length - 12)).length).toBe(2);
    expect(jpeg[jpeg.length - 1]).toBe(0xd9);
  });
  it("Gegenprobe: ohne Hoehe keine Hoehenangabe; kein JPEG wirft", () => {
    const jpeg = exifEinsetzen(jpegMit(), exifGpsSegment({ breite: 1, laenge: 2, hoehe_ft: null, kurs: null, zeit_utc: null }));
    expect(exifGpsLesen(jpeg)!.hoehe_m).toBeNull();
    expect(() => exifEinsetzen(Uint8Array.from([1, 2, 3, 4, 5]), new Uint8Array())).toThrow();
  });
});

describe("Sendezustand", () => {
  const leer: Sendestand = {};
  it("Start setzt alle gewaehlten auf sendet", () => {
    const s = sendeStart(leer, ["a", "b"], "S1");
    expect(s.a).toEqual({ phase: "sendet", sendungId: "S1" });
  });
  it("Antwort: angenommen, gesendet, abgelehnt mit Code und retry_after; fehlendes Bild gilt als unklar", () => {
    const s = antwortEinarbeiten(sendeStart(leer, ["a", "b", "c", "d"], "S1"), ["a", "b", "c", "d"], "S1", {
      bilder: [
        { bild_id: "a", status: "angenommen" },
        { bild_id: "b", status: "gesendet" },
        { bild_id: "c", status: "abgelehnt", code: "kontingent", retry_after: 300 },
      ],
      kontingent: { rest: 0, grenze: 12, retry_after: 300 },
    });
    expect(s.a.phase).toBe("angenommen");
    expect(s.b.phase).toBe("gesendet");
    expect(s.c).toMatchObject({ phase: "fehlgeschlagen", code: "kontingent", retryAfter: 300 });
    expect(s.d).toMatchObject({ phase: "fehlgeschlagen", code: "discord_unklar" });
  });
  it("jeder Servercode hat einen Platz, Unbekanntes wird unbekannt", () => {
    for (const c of ["zu_gross", "kontingent", "discord_fehler", "discord_unklar", "ungueltig", "konflikt", "nicht_eingerichtet"]) {
      expect(fehlerCodeNormal(c)).toBe(c);
      expect(FEHLER_CODES).toContain(c);
    }
    expect(fehlerCodeNormal("neu_vom_server")).toBe("unbekannt");
    expect(fehlerCodeNormal(undefined)).toBe("unbekannt");
  });
  it("Aufruf scheitert: Codes des Programms werden abgebildet", () => {
    const f = (code: string, ra?: unknown) => aufrufFehler(leer, ["a"], "S", code, ra).a;
    expect(f("live_nicht_erreichbar").code).toBe("server_nicht_erreichbar");
    expect(f("live_antwort_fehlt").code).toBe("discord_unklar");
    expect(f("live_nicht_angemeldet").code).toBe("nicht_angemeldet");
    expect(f("live_kontingent", "600")).toMatchObject({ code: "kontingent", retryAfter: 600 });
    expect(f("live_fehler").code).toBe("server_fehler");
    expect(f("irgendwas").code).toBe("unbekannt");
  });
  it("Fehlgeschlagene bleiben ausgewaehlt, Gesendete und Angenommene verlassen die Auswahl", () => {
    const s: Sendestand = {
      a: { phase: "gesendet", sendungId: "S" }, b: { phase: "angenommen", sendungId: "S" },
      c: { phase: "fehlgeschlagen", sendungId: "S", code: "zu_gross" }, d: { phase: "sendet", sendungId: "S" },
    };
    expect([...auswahlNachSenden(new Set(["a", "b", "c", "d", "e"]), s)]).toEqual(["c", "d", "e"]);
  });
  it("Wiederholung gleicher fehlgeschlagener Bilder behaelt die sendung_id, sonst neu", () => {
    const s: Sendestand = {
      a: { phase: "fehlgeschlagen", sendungId: "S1", code: "discord_unklar" }, b: { phase: "fehlgeschlagen", sendungId: "S1", code: "discord_unklar" },
      c: { phase: "gesendet", sendungId: "S0" }, d: { phase: "fehlgeschlagen", sendungId: "S2" },
    };
    const neu = () => "NEU";
    expect(sendungIdWaehlen(s, ["a", "b"], neu)).toBe("S1");
    expect(sendungIdWaehlen(s, ["a", "d"], neu)).toBe("NEU");
    expect(sendungIdWaehlen(s, ["a", "x"], neu)).toBe("NEU");
    expect(sendungIdWaehlen(s, ["c"], neu)).toBe("NEU"); // bewusst erneut senden = neue Nachricht
    expect(sendungIdWaehlen(s, [], neu)).toBe("NEU");
  });
  it("offene Bilder und Zusammenfassung", () => {
    const s: Sendestand = {
      a: { phase: "angenommen", sendungId: "S" }, b: { phase: "fehlgeschlagen", sendungId: "S", code: "server_nicht_erreichbar" },
      c: { phase: "fehlgeschlagen", sendungId: "S", code: "discord_unklar" }, d: { phase: "gesendet", sendungId: "S" },
    };
    expect(offeneIds(s)).toEqual([{ id: "a", sendungId: "S" }]);
    expect(zusammenfassung(s, ["a", "b", "c", "d"])).toEqual({ sendet: 1, fehlgeschlagen: 2, gesendet: 1, unklar: true });
  });
});
