// v1.2.3 (#Hoppie-PDC-CPDLC) — "is anybody there?" indicator.
//
// Three states, deliberately distinct: online, not online, and
// couldn't-check. Collapsing the last two into "offline" would state
// something we don't know — a network hiccup is not the same as an
// empty controller position.

import { useTranslation } from "react-i18next";
import type { StationStatus } from "../hooks/useStationOnline";

export function StationBadge({ status }: { status: StationStatus | null }) {
  const { t } = useTranslation();

  // Nothing typed, or the first check hasn't come back yet. Showing a
  // placeholder here would be noise on every keystroke.
  if (!status) return null;

  const kind = status.reason ? "unknown" : status.online ? "online" : "offline";
  // Two lengths on purpose. The badge sits in the same flex column as the
  // station input, so its width IS the field's width — rendering the full
  // sentence there (the offline one runs past 100 characters) stretched the
  // input across the panel and pushed the buttons below the fold. Short
  // label on screen, full explanation on hover.
  const label = t(`cpdlc.station_${kind}_short`);
  const explanation = t(`cpdlc.station_${kind}`, { station: status.station });

  return (
    <span
      className={`cpdlc-station-badge cpdlc-station-badge--${kind}`}
      title={status.reason ? `${explanation} (${status.reason})` : explanation}
    >
      <span className="cpdlc-station-badge__dot" aria-hidden="true" />
      {label}
    </span>
  );
}
