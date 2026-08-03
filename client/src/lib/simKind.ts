/**
 * Map a SimKind string to the brand label shown to the pilot. Pilots want to
 * see WHICH sim is connected, not the generic word "Simulator". Falls back to
 * "SIM" when nothing is selected so the pill/status line never goes blank.
 */
export function simKindLabel(kind: string | undefined): string {
  switch (kind) {
    case "msfs2024":
    case "msfs2020":
      return "MSFS";
    case "xplane11":
    case "xplane12":
      return "X-PLANE";
    case "off":
      return "SIM OFF";
    default:
      return "SIM";
  }
}
