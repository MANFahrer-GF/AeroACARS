// v0.16.23: Tests fuer den `no_simbrief_identifier`-Pfad im shared
// Refresh-Error-Formatter. `flight_refresh_route_only` (Route-Sync) gibt
// diesen Code zurueck wenn der Pilot keinen SimBrief-Identifier
// konfiguriert hat — der Formatter muss daraus einen actionable Notice
// machen ("set your SimBrief username in Settings").

import { describe, it, expect } from "vitest";
import type { TFunction } from "i18next";
import { formatRefreshError } from "./refreshErrorFormatter";

// Minimaler t-Mock: gibt den Key zurueck (plus serialisierte Params), damit
// wir verifizieren koennen WELCHER i18n-Key getroffen wird, ohne die echten
// Uebersetzungen zu laden.
const t = ((key: string, opts?: Record<string, unknown>) => {
  if (opts && Object.keys(opts).length > 0) {
    return `${key}|${JSON.stringify(opts)}`;
  }
  return key;
}) as unknown as TFunction;

describe("formatRefreshError — no_simbrief_identifier (v0.16.23)", () => {
  it("maps no_simbrief_identifier to the actionable Settings notice (cockpit)", () => {
    const out = formatRefreshError(
      { code: "no_simbrief_identifier", message: "no SimBrief identifier configured" },
      t,
      "cockpit",
    );
    expect(out).not.toBeNull();
    expect(out?.text).toBe("bids.no_simbrief_identifier");
    expect(out?.tone).toBe("warn");
  });

  it("maps no_simbrief_identifier the same way in the bidlist context", () => {
    // Anders als phase_locked/no_simbrief_link ist dieser Code KEIN benigner
    // still-Code — der Pilot muss in jedem Kontext erfahren dass er einen
    // Identifier setzen soll.
    const out = formatRefreshError(
      { code: "no_simbrief_identifier" },
      t,
      "bidlist",
    );
    expect(out?.text).toBe("bids.no_simbrief_identifier");
    expect(out?.tone).toBe("warn");
  });

  it("still hard-blocks a DEP/ARR mismatch with structured JSON details", () => {
    // Sicherstellen dass der Route-Sync-Mismatch (gleicher Code wie der
    // Full-OFP-Refresh) weiterhin den reichen Notice rendert.
    const details = JSON.stringify({
      active_callsigns: "GSG100",
      active_dpt: "EDDF",
      active_arr: "EGLL",
      sb_callsign: "GSG200",
      sb_origin: "EDDM",
      sb_dest: "LFPG",
    });
    const out = formatRefreshError(
      { code: "ofp_does_not_match_active_flight", message: details },
      t,
      "cockpit",
    );
    expect(out?.tone).toBe("warn");
    expect(out?.text).toContain("bids.ofp_does_not_match_active_flight");
    // Die geparsten Params muessen durchgereicht werden.
    expect(out?.text).toContain("EDDM");
    expect(out?.text).toContain("LFPG");
  });
});
