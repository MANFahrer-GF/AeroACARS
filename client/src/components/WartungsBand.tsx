import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Notice } from "./ui";
import { STATUS_TAKT_MS, statusHolen, text3, type StatusHinweis } from "../lib/statusHinweis";

/** Zeit in UTC, kurz (Piloten rechnen in Zulu, wie die Statusseite). */
function zeit(iso: string, sprache: string): string {
  return new Intl.DateTimeFormat(sprache, {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    timeZone: "UTC",
  }).format(new Date(iso));
}

/**
 * v3 (P-9.12): Band „Wartung geplant/läuft" und dezenter Hinweis auf
 * Störungen, aus `status.json` von live (beim Start und alle 10 Minuten).
 * Den Text der Wartung schreibt der Betreiber in der Webapp (DE/EN/IT); die
 * Rahmentexte stehen unter `wartung.*` im Paket. Ohne Netz oder ohne
 * Eintrag: nichts.
 */
export function WartungsBand() {
  const { t, i18n } = useTranslation();
  const [hinweis, setHinweis] = useState<StatusHinweis | null>(null);

  useEffect(() => {
    let aktiv = true;
    const holen = () => {
      void statusHolen().then((h) => {
        if (aktiv) setHinweis(h);
      });
    };
    holen();
    const takt = setInterval(holen, STATUS_TAKT_MS);
    return () => {
      aktiv = false;
      clearInterval(takt);
    };
  }, []);

  if (!hinweis) return null;
  const sprache = i18n.language;
  const w = hinweis.wartung;
  if (w) {
    const art = w.notfall ? "notfall" : w.aktiv ? "laeuft" : "geplant";
    return (
      <Notice
        className="wartungs-band"
        tone={w.aktiv ? "warn" : "info"}
        level={t(`wartung.${art}`)}
        title={t("wartung.zeitraum", { von: zeit(w.von, sprache), bis: zeit(w.bis, sprache) })}
        detail={text3(w.text, sprache) || undefined}
      />
    );
  }
  if (hinweis.stoerungen.length === 0) return null;
  return (
    <Notice
      className="wartungs-band wartungs-band--stoerung"
      tone="info"
      level={t("wartung.stoerung")}
      detail={hinweis.stoerungen.map((n) => text3(n, sprache)).join(", ")}
    />
  );
}
