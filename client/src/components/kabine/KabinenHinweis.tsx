// Hinweis im Cockpit (Thomas 18.10.2026): Steht das Anschnallzeichen im
// Reiseflug noch an, wartet die Kabine mit dem Service — wie in echt. Damit
// der Pilot es merkt, ohne die Seite „Kabine“ offen zu haben. Nur Anzeige.
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { isTauri } from "../../lib/ipc";
import { ueberblick } from "../../lib/kabinenansagen";
import { Notice } from "../ui";

export function KabinenHinweis() {
  const { t } = useTranslation();
  const [hinweis, setHinweis] = useState<string | null>(null);
  useEffect(() => {
    if (!isTauri) return;
    let weg = false;
    const holen = () =>
      void ueberblick()
        .then((d) => !weg && setHinweis(d?.grund ? null : (d?.hinweis ?? null)))
        .catch(() => !weg && setHinweis(null));
    holen();
    const iv = setInterval(holen, 10_000);
    return () => {
      weg = true;
      clearInterval(iv);
    };
  }, []);
  if (hinweis !== "anschnallzeichen") return null;
  return (
    <Notice
      tone="warn"
      level={t("kabine.hinweis.anschnallzeichen.titel")}
      data-testid="kabine-hinweis-cockpit"
      detail={<span>{t("kabine.hinweis.anschnallzeichen.text")}</span>}
    />
  );
}
