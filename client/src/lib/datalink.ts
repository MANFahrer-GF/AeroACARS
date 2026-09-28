// v1.3.0 (#Hoppie-PDC-CPDLC) — datalink text presentation.
//
// Hoppie uses '@' as a line break inside message text: "The '@' signs in
// CPDLC messages are line feeds for presentation purposes but do not
// really mean anything" (hoppie.nl/acars/system/tech.html), and the
// protocol notes add that pilot displays "can simply replace @ signs by
// new lines".
//
// This matters in practice: vSMR, the VATSIM UK controller plugin,
// builds its clearance entirely out of them (SMRPlugin.cpp:206-236) —
//   CLR TO @EGLL@ RWY @27R@ DEP @DVR1G@ INIT CLB @FL060@ SQUAWK @1234@
// which without this renders as one unbroken line.
//
// Deliberately the same rule EasyCPDLC uses (MainForm.cs:1018:
// `.Replace("@@", "N/A").Replace("@", Environment.NewLine)`) rather than
// anything cleverer: treating '@' as a paired field delimiter would
// misparse the moment a message contains an odd number of them, and the
// documented meaning is simply "line break".

/** An empty pair of markers means the controller left that field blank. */
const EMPTY_FIELD = "N/A";

/**
 * Convert raw datalink text into display lines.
 *
 * `@@` becomes "N/A", every remaining `@` ends the line, and blank lines
 * are dropped so a trailing marker doesn't leave a gap.
 */
export function datalinkLines(raw: string): string[] {
  return raw
    // Keep the markers around the placeholder so a blank field lands on
    // its own line like every other value. EasyCPDLC substitutes inline,
    // which glues "DEP N/A SQUAWK" together and reads worse.
    .replace(/@@/g, `@${EMPTY_FIELD}@`)
    .split(/[@\n]/)
    .map((line) => line.trim())
    .filter((line) => line !== "");
}

/** The same text as a single string, newline-separated. */
export function formatDatalinkText(raw: string): string {
  return datalinkLines(raw).join("\n");
}

/**
 * v1.9.5 (#hoppie-replay) — a CPDLC instruction as ONE readable line.
 *
 * Controller software (TopSky and friends) wraps every value in '@' and
 * pads fields with `_` and `||`: real uplinks read
 * "CONTACT @LRWW 125.765@_@BUCHAREST RADAR" or
 * "MONITOR ADVISORY @122.800@. THANKS ... @ACC SOFIA@||" (GSG flight
 * logs, 19.09./31.07.2026). Split at every '@' that became
 * "CONTACT / LRWW 125.765 / _ / BUCHAREST RADAR" — four rows for one
 * instruction, with filler shown as content. Here '@' is a space, a
 * token made only of `_`/`|` is dropped, and no space is left before
 * punctuation. The wire text stays available unchanged under "Original".
 */
export function cpdlcLine(raw: string): string {
  return raw
    .replace(/@@/g, ` ${EMPTY_FIELD} `)
    .replace(/@/g, " ")
    .split(/\s+/)
    .filter((token) => token !== "" && !/^[_|]+$/.test(token))
    .join(" ")
    .replace(/ ([.,;:])/g, "$1");
}
