import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Profile } from "../types";

interface Props {
  profile: Profile;
  onLogout: () => void;
}

function formatZulu(now: number): { time: string; suffix: string } {
  const d = new Date(now);
  const hh = d.getUTCHours().toString().padStart(2, "0");
  const mm = d.getUTCMinutes().toString().padStart(2, "0");
  const ss = d.getUTCSeconds().toString().padStart(2, "0");
  return { time: `${hh}:${mm}:${ss}`, suffix: "z" };
}

/**
 * Briefing-2a (README §2): Kopfzeile statt Karte. Das Airline-Logo lebt
 * jetzt an den Flügen selbst (BidsList main card), nicht mehr hier —
 * eine Kopfzeile identifiziert den PILOTEN, nicht die Airline.
 */
export function PilotHeader({ profile, onLogout }: Props) {
  const { t } = useTranslation();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  const zulu = formatZulu(now);

  return (
    <header className="pilot-bar">
      <div className="pilot-bar__left">
        {profile.ident && (
          <span className="pilot-bar__ident">{profile.ident}</span>
        )}
        <span className="pilot-bar__name">{profile.name}</span>
        {(profile.rank?.name || profile.airline?.name) && (
          <span className="pilot-bar__role">
            {[profile.rank?.name, profile.airline?.name]
              .filter(Boolean)
              .join(" · ")}
          </span>
        )}
      </div>
      <div className="pilot-bar__right">
        <span className="pilot-bar__loc">
          <span className="pilot-bar__loc-label">{t("pilot_header.pos_label")}</span>
          <span className="pilot-bar__loc-value">{profile.curr_airport ?? "—"}</span>
        </span>
        <span className="pilot-bar__loc">
          <span className="pilot-bar__loc-label">{t("pilot_header.basis_label")}</span>
          <span className="pilot-bar__loc-value">{profile.home_airport ?? "—"}</span>
        </span>
        <span className="pilot-bar__sep" aria-hidden="true" />
        <span className="pilot-bar__clock">
          {zulu.time}
          <span className="pilot-bar__clock-suffix">{zulu.suffix}</span>
        </span>
        <button
          type="button"
          className="pilot-bar__logout"
          onClick={onLogout}
        >
          {t("actions.logout")}
        </button>
      </div>
    </header>
  );
}
