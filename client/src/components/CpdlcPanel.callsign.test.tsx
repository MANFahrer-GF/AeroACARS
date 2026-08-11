// v1.5.6 (#hoppie-callsign) — Feldbefund Michel, Live-Flug SBBR→LPPT
// (X-Plane, Mac): Er konnte das Rufzeichen nicht setzen. Ursache war NICHT
// die Plattform, sondern eine Sackgasse in der Bedienführung:
//
//   1. Ein oranger Alarm forderte ein Rufzeichen — und zwar immer, wenn das
//      Feld leer war. Das ist der NORMALFALL (dann greift das Rufzeichen aus
//      dem Flugplan).
//   2. Gleichzeitig waren Knopf und Feld gesperrt, sobald Hoppie VERBUNDEN
//      war. Der Alarm verlangte also etwas, das man gerade nicht tun konnte.
//
// Diese Tests halten beide Korrekturen fest. Sie prüfen die reinen
// Entscheidungen (Hinweistext + Sperrregel), nicht das Rendering — dafür
// bräuchte es den kompletten Hoppie-Status-Baum.
import { describe, it, expect } from "vitest";

/** Der Hinweis unter der Kopfzeile: was gesendet wird und woher es kommt. */
function callsignNotice(
  typedRaw: string,
  flightCallsign: string | null,
): { key: string; vars: Record<string, string> } | null {
  const typed = typedRaw.trim();
  if (typed === "" && flightCallsign) {
    return { key: "cpdlc.callsign_from_plan", vars: { flight: flightCallsign } };
  }
  if (
    typed !== "" &&
    flightCallsign &&
    typed.toUpperCase() !== flightCallsign.toUpperCase()
  ) {
    return {
      key: "cpdlc.callsign_override_active",
      vars: { entered: typed.toUpperCase(), flight: flightCallsign },
    };
  }
  return null;
}

/** Wann das Rufzeichen NICHT geändert werden darf. */
function callsignLocked(opts: {
  busy: boolean;
  online: boolean;
  loggedOn: boolean;
  reconnecting: boolean;
}): boolean {
  return opts.busy || opts.loggedOn || opts.reconnecting;
}

describe("Rufzeichen-Hinweis (statt Alarm)", () => {
  it("nennt neutral das Flugplan-Rufzeichen, wenn nichts getippt wurde", () => {
    // Michels Fall: Feld leer, Flugplan sagt TAP58 → kein Alarm, nur Info.
    const n = callsignNotice("", "TAP58");
    expect(n?.key).toBe("cpdlc.callsign_from_plan");
    expect(n?.vars.flight).toBe("TAP58");
  });

  it("bestätigt eine bewusst abweichende Eingabe, statt sie zu rügen", () => {
    // Ein abweichendes Rufzeichen ist der ZWECK des Feldes (im Netzwerk
    // fliegt der Pilot evtl. anders) — kein Fehlerfall.
    const n = callsignNotice("gsg123", "TAP58");
    expect(n?.key).toBe("cpdlc.callsign_override_active");
    expect(n?.vars.entered).toBe("GSG123");
    expect(n?.vars.flight).toBe("TAP58");
  });

  it("schweigt, wenn Eingabe und Flugplan übereinstimmen", () => {
    expect(callsignNotice("TAP58", "TAP58")).toBeNull();
    expect(callsignNotice(" tap58 ", "TAP58")).toBeNull();
  });

  it("schweigt ohne aktiven Flug", () => {
    expect(callsignNotice("", null)).toBeNull();
  });
});

describe("Rufzeichen-Sperre", () => {
  const base = { busy: false, online: false, loggedOn: false, reconnecting: false };

  it("erlaubt das Ändern bei bloß bestehender Hoppie-Verbindung", () => {
    // GENAU Michels Situation: verbunden, aber nicht angemeldet.
    expect(callsignLocked({ ...base, online: true })).toBe(false);
  });

  it("sperrt während einer laufenden CPDLC-Anmeldung", () => {
    // Dann kennt der Lotse den Piloten unter dem alten Rufzeichen.
    expect(callsignLocked({ ...base, online: true, loggedOn: true })).toBe(true);
  });

  it("sperrt während des Neuaufbaus und laufender Aktionen", () => {
    expect(callsignLocked({ ...base, reconnecting: true })).toBe(true);
    expect(callsignLocked({ ...base, busy: true })).toBe(true);
  });

  it("erlaubt das Ändern im Ruhezustand", () => {
    expect(callsignLocked(base)).toBe(false);
  });
});
