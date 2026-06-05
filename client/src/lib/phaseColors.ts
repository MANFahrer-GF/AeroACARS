// Phasen-Farben + Label — portiert aus der VPS-Live-Map (aeroacars-live
// `src/data/phaseColors.ts`), damit der Flieger-Marker dieselbe phasenabhängige
// Färbung + dasselbe Look-and-Feel hat wie live.kant.ovh.
//
// Tolerant gegenüber zwei Quellen:
//   • eigener Flug: FlightPhase snake_case ("taxi_out", "cruise", …)
//   • VA (/api/acars): status_text wie "En Route", "Final Approach", …

const PHASE_COLOR: Record<string, string> = {
  PREFLIGHT: "#94a3b8",
  BOARDING: "#cbd5e1",
  PUSHBACK: "#14b8a6",
  TAXI_OUT: "#fbbf24",
  TAKEOFF_ROLL: "#fb923c",
  TAKEOFF: "#f97316",
  CLIMB: "#22d3ee",
  CRUISE: "#10b981",
  HOLDING: "#c084fc",
  DESCENT: "#3b82f6",
  APPROACH: "#f59e0b",
  FINAL: "#ef4444",
  LANDING: "#dc2626",
  TAXI_IN: "#94a3b8",
  BLOCKS_ON: "#64748b",
  ARRIVED: "#475569",
  PIREP_SUBMITTED: "#334155",
};

// Aliasse: /api/acars-Status-Texte + Varianten → kanonische Keys.
const PHASE_ALIAS: Record<string, string> = {
  EN_ROUTE: "CRUISE",
  ENROUTE: "CRUISE",
  INITIAL_CLIMB: "CLIMB",
  FINAL_APPROACH: "FINAL",
  ON_PUSH_BACK: "PUSHBACK",
  PUSH_BACK: "PUSHBACK",
  TAKE_OFF: "TAKEOFF",
  TAXI: "TAXI_OUT",
  TAXIING: "TAXI_OUT",
  LANDED: "ARRIVED",
  ON_BLOCK: "BLOCKS_ON",
  PARKED: "ARRIVED",
};

function normalizePhase(p: string | null | undefined): string {
  if (!p) return "";
  const k = String(p).toUpperCase().trim().replace(/[\s-]+/g, "_");
  return PHASE_ALIAS[k] ?? k;
}

/** Phasen-Farbe (Hex). Unbekannt → neutrales Slate. */
export function phaseColor(phase: string | null | undefined): string {
  return PHASE_COLOR[normalizePhase(phase)] ?? "#94a3b8";
}

const PHASE_LABEL: Record<string, string> = {
  PREFLIGHT: "Preflight",
  BOARDING: "Boarding",
  PUSHBACK: "Pushback",
  TAXI_OUT: "Taxi Out",
  TAKEOFF_ROLL: "T/O Roll",
  TAKEOFF: "Takeoff",
  CLIMB: "Climb",
  CRUISE: "Cruise",
  HOLDING: "Holding",
  DESCENT: "Descent",
  APPROACH: "Approach",
  FINAL: "Final",
  LANDING: "Landing",
  TAXI_IN: "Taxi In",
  BLOCKS_ON: "Blocks On",
  ARRIVED: "Arrived",
  PIREP_SUBMITTED: "Eingereicht",
};

/** Hübsches Phasen-Label ("taxi_out" → "Taxi Out"). */
export function phaseLabel(phase: string | null | undefined): string {
  if (!phase) return "—";
  const k = normalizePhase(phase);
  return PHASE_LABEL[k] ?? String(phase).replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}
