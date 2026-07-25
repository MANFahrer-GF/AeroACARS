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
  const label = t(`cpdlc.station_${kind}`, { station: status.station });

  return (
    <span className={`cpdlc-station-badge cpdlc-station-badge--${kind}`} title={status.reason ?? undefined}>
      <span className="cpdlc-station-badge__dot" aria-hidden="true" />
      {label}
    </span>
  );
}
