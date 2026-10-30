// Zusatzfelder (V9, Faehigkeit `zusatz-1`): das schemaoffene Objekt `zusatz`
// der Landung als schlichte, aufklappbare Schluessel/Wert-Liste.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN).
//
// Der Abschnitt kennt keinen Schluessel: Das Bewertungsmodul darf Felder
// liefern, die es beim Bau dieses Programms noch nicht gab. Gezeigt wird
// nur Text — React setzt jeden Wert als Textknoten ein, es wird nichts
// ausgefuehrt und kein HTML eingesetzt. Verschachteltes erscheint als
// eingerueckter JSON-Text. Ohne Zusatz erscheint kein Abschnitt.

import { useTranslation } from "react-i18next";
import "./rohdatenAbschnitt.css";
import "./zusatzAbschnitt.css";
import { useDruck } from "../lib/druck";

/** Ein Wert als Text: einfache Werte wie sie sind, Verschachteltes als JSON. */
export function zusatzWertText(v: unknown): string {
  if (v === null) return "null";
  if (typeof v === "string") return v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  try {
    return JSON.stringify(v, null, 2) ?? "";
  } catch {
    return "";
  }
}

export function ZusatzAbschnitt({ record }: { record: { zusatz?: Record<string, unknown> | null } }) {
  const { t } = useTranslation();
  const druck = useDruck();
  const z = record.zusatz;
  if (!z || typeof z !== "object" || Array.isArray(z)) return null;
  const schluessel = Object.keys(z);
  if (schluessel.length === 0) return null;
  return (
    <details className="landing-section rohdaten zusatz" open={druck || undefined}>
      <summary>{t("landing.zusatz.title")}</summary>
      <div className="rohdaten__liste">
        {schluessel.map((k) => (
          <div className="rohdaten__zeile" key={k}>
            <span className="rohdaten__k">{k}</span>
            <code className="rohdaten__v zusatz__v">{zusatzWertText(z[k])}</code>
          </div>
        ))}
      </div>
      <p className="rohdaten__hinweis">{t("landing.zusatz.hinweis")}</p>
    </details>
  );
}
