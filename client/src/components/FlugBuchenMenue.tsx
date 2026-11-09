// „Flug buchen“ in der Kopfzeile der Buchungsliste: Buchen bleibt auf der Webseite
// (Entscheidung vom 21.06.2026), das Menü führt nur auf die vier Buchungswege.
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

const WEGE = [
  { pfad: "/flights", schluessel: "bids.buchen_flugplan" },
  { pfad: "/live-schedules", schluessel: "bids.buchen_live" },
  { pfad: "/dfreeflight", schluessel: "bids.buchen_freiflug" },
  { pfad: "/adventures", schluessel: "bids.buchen_adventures" },
] as const;

interface Props {
  baseUrl: string;
  onOeffnen: (url: string) => void;
}

export function FlugBuchenMenue({ baseUrl, onOeffnen }: Props) {
  const { t } = useTranslation();
  const [offen, setOffen] = useState(false);
  const wurzel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!offen) return;
    const klick = (e: MouseEvent) => {
      if (!wurzel.current?.contains(e.target as Node)) setOffen(false);
    };
    const taste = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOffen(false);
    };
    document.addEventListener("mousedown", klick);
    window.addEventListener("keydown", taste);
    return () => {
      document.removeEventListener("mousedown", klick);
      window.removeEventListener("keydown", taste);
    };
  }, [offen]);

  return (
    <div className="bids__buchen" ref={wurzel}>
      <button type="button" className="bids__refresh" aria-haspopup="menu" aria-expanded={offen} onClick={() => setOffen((o) => !o)}>
        {t("bids.buchen")}
      </button>
      {offen && (
        <div className="bids__buchen-menue" role="menu">
          {WEGE.map((w) => (
            <button
              key={w.pfad}
              type="button"
              role="menuitem"
              onClick={() => {
                setOffen(false);
                onOeffnen(`${baseUrl.replace(/\/$/, "")}${w.pfad}`);
              }}
            >
              {t(w.schluessel)}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
