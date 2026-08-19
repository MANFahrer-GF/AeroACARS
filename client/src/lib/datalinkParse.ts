// v1.3.5 (#Datalink-3a) — uplink telex parser for the PDC/CPDLC screen.
//
// Hoppie telex has no structure beyond keywords in free text (PDC replies
// are plain human/network text — see hoppie-protocol::pdc's docs; CPDLC
// uplinks that don't carry a recognized GOLD element render the same way
// here). This recognizes the keyword fields position-independently and
// puts everything else, unchanged, into `conditions` — never dropped,
// never rewritten.
//
// Deliberately NOT smarter than needed: no unit-stripping, no sentence
// classification, no merging. A parser that "corrects" is one a pilot can
// no longer trust to show the actual wire text.
//
// v1.6.12 (#pdc-station), from a real LROP clearance that read
// "…CLRD TO@EDDB@OFF@08L@VIA@SOKRU1K@CLIMB@FL280@SQUAWK@1000@NEXT FREQ@
// 121.855@ATIS REQ STARTUP ON@121.855":
//   - "CLIMB FL280" is how clearances actually word the initial climb;
//     only the literal "INITIAL CLIMB 280" was recognized, so the cell
//     stayed empty while the value sat in the conditions line.
//   - "ATIS REQ STARTUP ON" matched the ATIS-letter rule and displayed
//     ATIS "R". An invented letter is worse than an empty cell: it is
//     the one field a pilot reads off and repeats on frequency.
//   - the destination ("CLRD TO EDDB") and the message's own header
//     ("CLD 0843 260819 LROP PDC 001") had nowhere to go and were shown
//     as if the controller had attached them as conditions.

export interface PdcHeader {
  /** `CLD` = clearance delivery, `FSM` = the ground system's status
   *  message (e.g. "ACK NOT RECEIVED / CLEARANCE CANCELLED"). */
  kind: string;
  /** HHMM, as sent. */
  time: string;
  /** DDMMYY, as sent. */
  date: string;
  icao: string;
  /** "PDC 001" on a clearance, the callsign on an FSM. */
  ref: string;
}

export interface ParsedUplink {
  squawk: string | null;
  sid: string | null;
  initialClimb: string | null;
  depFreq: string | null;
  rwy: string | null;
  ctot: string | null;
  qnh: string | null;
  atis: string | null;
  dest: string | null;
  /** The DCL-style header line, when the message carries one. Shown as
   *  the card's reference line — NOT as a condition, and never on its
   *  own enough to call the message parsed (see `recognized`). */
  header: PdcHeader | null;
  /** Every stretch of text no rule matched, in original order. */
  conditions: string[];
  raw: string;
  /** False when NONE of the value fields were found — the caller then
   *  shows raw text as the main content instead of the grid. */
  recognized: boolean;
}

type FieldKey =
  | "squawk"
  | "sid"
  | "initialClimb"
  | "depFreq"
  | "rwy"
  | "ctot"
  | "qnh"
  | "atis"
  | "dest";

/** `\s+` instead of a literal space so a CPDLC uplink's '@' line breaks
 *  (flattened to spaces below) or a stray double space don't hide an
 *  otherwise-valid match. */
const FIELD_SPECS: { key: FieldKey; regex: RegExp }[] = [
  { key: "squawk", regex: /SQUAWK\s+(\d{4})/g },
  // QS 19.08.2026: "VIA <token>" alone read a taxi instruction's
  // "TAXI VIA N4" as the SID "N4", and "CLIMB VIA SID" as the SID
  // "SID". A published SID/STAR designator is a name plus a validity
  // digit and optional suffix (ICAO Doc 8168): SOKRU1K, MARUN2F,
  // DOMUX2N. Requiring that shape costs nothing — anything else stays
  // in the conditions text, where it can be read but not mistaken for
  // a departure.
  { key: "sid", regex: /VIA\s+([A-Z]{2,7}\d[A-Z]?)(?![A-Z0-9])/g },
  // "CLIMB FL280", "CLIMB TO FL280", "INITIAL CLIMB 5000", "CLIMB 5000FT".
  // The unit stays in the value: FL280 and 5000 are not the same thing,
  // and rewriting either one is the parser deciding what ATC meant.
  {
    key: "initialClimb",
    regex: /(?:INITIAL\s+)?CLIMB(?:\s+TO)?\s+(FL\s?\d{2,3}|\d{3,5}\s?(?:FT)?)(?![\dA-Z])/g,
  },
  { key: "depFreq", regex: /(?:NEXT|DEP|DEPARTURE)\s+FREQ\s+([\d.]+)/g },
  { key: "rwy", regex: /(?:OFF|RWY|RUNWAY)\s+(\d{1,2}[LRC]?)(?![\dA-Z])/g },
  // The trailing Z is optional but common ("CTOT 1436Z") — without it
  // in the pattern, a lone "Z" was left behind as a condition.
  { key: "ctot", regex: /CTOT\s+(\d{4})Z?/g },
  // hPa or inHg — both are real, neither is rewritten into the other.
  { key: "qnh", regex: /QNH\s+(\d{3,4}|\d{2}\.\d{2})/g },
  // The letter must stand alone. Without the lookahead, "ATIS REQ
  // STARTUP ON 121.855" reported ATIS "R" and left "EQ STARTUP ON
  // 121.855" behind as a condition.
  { key: "atis", regex: /ATIS\s+(?:INFO\s+)?([A-Z])(?![A-Z])/g },
  // Spelled out rather than pattern-matched around "CL…": the loose
  // version also matched things that only looked like it. The verbs are
  // the same set the clearance detector uses (see DatalinkHistory's
  // isTelexClearance). The sender glues the callsign to it
  // ("WMT4TKCLRD TO EDDB"), so no word boundary in front.
  { key: "dest", regex: /(?:CLEARED|CLRD|CLR|CLD|CL)\s+TO\s+([A-Z]{4})(?![A-Z])/g },
];

/** Four-letter words that are NOT an airport, however much they look
 *  like an ICAO code after "CLEARED TO". "CLEARED TO LAND RWY 08L" was
 *  reporting a destination of "LAND". Same principle as the ATIS
 *  lookahead: an empty cell is a gap, a wrong cell is a lie — and the
 *  text itself is never dropped, it stays in the conditions line. */
const NOT_AN_AIRPORT = new Set(["LAND", "TAXI", "HOLD", "PUSH", "STOP", "EXIT", "JOIN", "LINE"]);

/** "CLD 0843 260819 LROP PDC 001" / "FSM 0853 260819 LROP WMT4TK" — the
 *  DCL message header. Same shape in both directions of that protocol;
 *  matched only at the start, where it belongs. */
const HEADER_REGEX = /^(CLD|FSM)\s+(\d{4})\s+(\d{6})\s+([A-Z]{4})\s+(PDC\s+\d+|[A-Z0-9]+)/;

/** Hoppie's '@' is a presentation line break, not wire structure (see
 *  lib/datalink.ts's docs) — flattened to whitespace here so a field split
 *  across a line break still matches. `raw` on the result stays the
 *  trimmed, UNFLATTENED text; only the working copy used to find fields
 *  and slice conditions is flattened. */
function flattenForParsing(text: string): string {
  return text
    .replace(/@@/g, " N/A ")
    .replace(/@/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function pushCondition(into: string[], chunk: string, ownCallsign?: string | null): void {
  const text = chunk.trim();
  if (text === "") return;
  if (ownCallsign && text === ownCallsign.trim().toUpperCase()) return;
  into.push(text);
}

/**
 * @param ownCallsign  The aircraft's own callsign, when known. A chunk
 *   that is nothing BUT that callsign is dropped from `conditions`: the
 *   clearance addressing us is not an instruction from the station, and
 *   a lone "WMT4TK" under "conditions of the station" reads like one.
 *   Only an exact, standalone match is dropped — "WMT4TK CONTACT GROUND"
 *   stays whole — and the original text is always one click away.
 */
export function parseUplink(rawInput: string, ownCallsign?: string | null): ParsedUplink {
  const raw = rawInput.trim();
  const flat = flattenForParsing(raw);
  const values: Record<FieldKey, string | null> = {
    squawk: null,
    sid: null,
    initialClimb: null,
    depFreq: null,
    rwy: null,
    ctot: null,
    qnh: null,
    atis: null,
    dest: null,
  };
  const spans: { start: number; end: number }[] = [];
  let fieldCount = 0;

  for (const { key, regex } of FIELD_SPECS) {
    regex.lastIndex = 0;
    const m = regex.exec(flat);
    if (m) {
      if (key === "dest" && NOT_AN_AIRPORT.has(m[1])) continue;
      values[key] = m[1].replace(/\s+/g, "");
      spans.push({ start: m.index, end: m.index + m[0].length });
      fieldCount += 1;
    }
  }

  const headerMatch = HEADER_REGEX.exec(flat);
  const header: PdcHeader | null = headerMatch
    ? {
        kind: headerMatch[1],
        time: headerMatch[2],
        date: headerMatch[3],
        icao: headerMatch[4],
        ref: headerMatch[5].replace(/\s+/g, " "),
      }
    : null;
  if (headerMatch) {
    spans.push({ start: headerMatch.index, end: headerMatch.index + headerMatch[0].length });
  }
  spans.sort((a, b) => a.start - b.start);

  // Recognized fields split the flattened text into leftover stretches —
  // the gaps BETWEEN them, not the whole remainder joined together. Two
  // clauses either side of an extracted field are not one sentence just
  // because the field between them is gone.
  const conditions: string[] = [];
  let cursor = 0;
  for (const span of spans) {
    // Two rules can overlap (a header that ends inside a field, say).
    // Skipping keeps the slicing monotonic instead of re-emitting text.
    if (span.start < cursor) {
      cursor = Math.max(cursor, span.end);
      continue;
    }
    pushCondition(conditions, flat.slice(cursor, span.start), ownCallsign);
    cursor = span.end;
  }
  pushCondition(conditions, flat.slice(cursor), ownCallsign);

  return {
    ...values,
    header,
    conditions,
    raw,
    // A header alone is not a parsed clearance: an FSM status message
    // ("ACK NOT RECEIVED …") carries one and no values at all, and must
    // still render as plain text rather than a grid of eight dashes.
    recognized: fieldCount > 0,
  };
}

/** CTOT is a bare "HHMM" on the wire (e.g. "1436"); the grid shows it as
 *  a time like every other timestamp in the app ("14:36z"). Presentation
 *  only — the parsed value itself stays the raw digit string. */
export function formatCtot(ctot: string): string {
  if (!/^\d{3,4}$/.test(ctot)) return ctot;
  const padded = ctot.padStart(4, "0");
  return `${padded.slice(0, 2)}:${padded.slice(2)}z`;
}

/** The header as one compact reference line: "PDC 001 · LROP · 0843z". */
export function formatHeader(header: PdcHeader): string {
  const ref = header.kind === "FSM" ? `FSM ${header.ref}` : header.ref;
  return `${ref} · ${header.icao} · ${header.time}z`;
}
