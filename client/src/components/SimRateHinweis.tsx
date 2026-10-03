// Leiser Hinweis, wenn die Sim-Rate laenger nicht auf 1× steht (Anlass TGW 882,
// 02.10.2026: drei Stunden unbemerkt auf 0,25×). Nur Anzeige — er blockiert
// nichts und veraendert keine Wertung.
import { useTranslation } from "react-i18next";
import { useSimRateHinweis, simRateText } from "../hooks/useSimRateHinweis";
import { Notice } from "./ui";

export function SimRateHinweis({
  rate,
  aktiv,
}: {
  rate: number | null | undefined;
  aktiv: boolean;
}) {
  const { t, i18n } = useTranslation();
  const abweichend = useSimRateHinweis(rate, aktiv);
  if (abweichend === null) return null;
  const langsamer = abweichend < 1;
  return (
    <Notice
      tone="warn"
      level={t("simrate.titel", { rate: simRateText(abweichend, i18n.language) })}
      data-testid="simrate-hinweis"
      detail={<span>{t(langsamer ? "simrate.langsamer" : "simrate.schneller")}</span>}
    />
  );
}
