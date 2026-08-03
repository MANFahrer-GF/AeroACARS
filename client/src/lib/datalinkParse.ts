// v1.3.5 (#Datalink-3a) — uplink telex parser for the PDC/CPDLC screen.
//
// Hoppie telex has no structure beyond keywords in free text (PDC replies
// are plain human/network text — see hoppie-protocol::pdc's docs; CPDLC
// uplinks that don't carry a recognized GOLD element render the same way
// here). This recognizes eight keyword fields position-independently and
// puts everything else, unchanged, into `conditions` — never dropped,
// never rewritten. See handoff_datalink_3a/README.md §5 for the spec this
// implements field-for-field, including the exact regexes.
//
// Deliberately NOT smarter than the spec: no unit-stripping, no sentence
// classification, no merging. What the spec doesn't ask for, this doesn't
// do — a parser that "corrects" is one a pilot can no longer trust to
// show the actual wire text.

export interface ParsedUplink {
  squawk: string | null;
  sid: string | null;
  initialClimb: string | null;
  depFreq: string | null;
  rwy: string | null;
  ctot: string | null;
  qnh: string | null;
  atis: string | null;
  /** Every stretch of text no field regex matched, in original order. */
  conditions: string[];
  raw: string;
  /** False only when NONE of the eight fields were found — the caller
   *  then shows raw text as the main content instead of the grid. */
  recognized: boolean;
}

type FieldKey = "squawk" | "sid" | "initialClimb" | "depFreq" | "rwy" | "ctot" | "qnh" | "atis";

/** Order and keywords are the spec's, verbatim — README §5. `\s+` instead
 *  of a literal space so a CPDLC uplink's '@' line breaks (flattened to
 *  spaces below) or a stray double space don't hide an otherwise-valid
 *  match. */
const FIELD_SPECS: { key: FieldKey; regex: RegExp }[] = [
  { key: "squawk", regex: /SQUAWK\s+(\d{4})/g },
  { key: "sid", regex: /VIA\s+([A-Z0-9]+)/g },
  { key: "initialClimb", regex: /INITIAL CLIMB\s+(\d+)/g },
  { key: "depFreq", regex: /NEXT FREQ\s+([\d.]+)/g },
  { key: "rwy", regex: /OFF\s+(\d{1,2}[LRC]?)/g },
  { key: "ctot", regex: /CTOT\s+(\d{4})/g },
  { key: "qnh", regex: /QNH\s+(\d{3,4})/g },
  { key: "atis", regex: /ATIS\s+([A-Z])/g },
];

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

export function parseUplink(rawInput: string): ParsedUplink {
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
  };
  const spans: { start: number; end: number }[] = [];

  for (const { key, regex } of FIELD_SPECS) {
    regex.lastIndex = 0;
    const m = regex.exec(flat);
    if (m) {
      values[key] = m[1];
      spans.push({ start: m.index, end: m.index + m[0].length });
    }
  }
  spans.sort((a, b) => a.start - b.start);

  // Recognized fields split the flattened text into leftover stretches —
  // the gaps BETWEEN them, not the whole remainder joined together. Two
  // clauses either side of an extracted field are not one sentence just
  // because the field between them is gone.
  const conditions: string[] = [];
  let cursor = 0;
  for (const span of spans) {
    const chunk = flat.slice(cursor, span.start).trim();
    if (chunk !== "") conditions.push(chunk);
    cursor = span.end;
  }
  const tail = flat.slice(cursor).trim();
  if (tail !== "") conditions.push(tail);

  return {
    ...values,
    conditions,
    raw,
    recognized: spans.length > 0,
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
