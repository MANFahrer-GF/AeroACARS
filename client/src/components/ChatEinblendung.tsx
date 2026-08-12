// Kurze Einblendung bei einem eingehenden Zuruf.
//
// Feldbefund Thomas (12.08.2026): „bekommen die Piloten bei
// Direktnachrichten keine visuelle Benachrichtigung, wenn sie nicht im
// Chat sind? Ton kommt, aber wenn man nicht weiß, was das für ein Ton
// ist, naja."
//
// Genau so war es: Es gab den Ton und ein Zählerplättchen am Chat-Eintrag
// der Seitenleiste. Wer im Vollbild fliegt, hört also etwas und erfährt
// nichts — und beim ersten Mal weiß niemand, wofür das Geräusch steht.
//
// Drei Entscheidungen, die im Code nicht offensichtlich sind:
//
//   1. Die Einblendung zeigt Absender UND Textanfang. Ein „Sie haben eine
//      neue Nachricht" würde den Piloten zum Wechseln zwingen — genau das,
//      was der Chat vermeiden soll. Die meisten Zurufe sind so kurz, dass
//      sie ganz hineinpassen.
//   2. In den Phasen, in denen der Ton schweigt (Start, Endanflug,
//      Landung), erscheint auch nichts. Dieselbe Regel, eine Quelle:
//      `lautstaerkeFuerPhase`. Im Endanflug hat nichts das Recht, die
//      Aufmerksamkeit zu holen — der Zuruf wartet im Chat.
//   3. Sie verschwindet von selbst. Ein Zuruf ist flüchtig; ein Kasten,
//      den man wegklicken MUSS, wäre im Cockpit eine Zumutung. Wer ihn
//      anklickt, landet im Chat.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { lautstaerkeFuerPhase } from "../lib/chatTon";
import { Notice } from "./ui";

/** Wie lange die Einblendung steht. Lang genug zum Lesen, kurz genug, um
 *  im Anflug nicht im Weg zu sein. */
const SICHTBAR_MS = 7000;

export interface EingehenderZuruf {
  id: number;
  text: string;
  von_pilot_id: string;
  an_pilot_id?: string | null;
  callsign?: string | null;
  anzeigename?: string | null;
}

/** Absenderkennung der Flugleitung — kein Pilot kann die haben. */
const OPS_ID = "__ops";

export function ChatEinblendung({
  zuruf,
  phase,
  onOeffnen,
}: {
  /** Der zuletzt eingegangene Zuruf, oder null. */
  zuruf: EingehenderZuruf | null;
  /** Aktuelle Flugphase — steuert, ob überhaupt eingeblendet wird. */
  phase: string | null;
  onOeffnen: () => void;
}) {
  const { t } = useTranslation();
  const [sichtbar, setSichtbar] = useState<EingehenderZuruf | null>(null);
  const gezeigt = useRef<number | null>(null);

  useEffect(() => {
    if (!zuruf) return;
    // Denselben Zuruf nicht zweimal einblenden (der Empfänger in App.tsx
    // kann bei einem erneuten Aufbau dieselbe Nachricht nochmals liefern).
    if (gezeigt.current === zuruf.id) return;
    if (lautstaerkeFuerPhase(phase) === "still") return;
    gezeigt.current = zuruf.id;
    setSichtbar(zuruf);
    const id = setTimeout(() => setSichtbar(null), SICHTBAR_MS);
    return () => clearTimeout(id);
  }, [zuruf, phase]);

  const oeffnen = useCallback(() => {
    setSichtbar(null);
    onOeffnen();
  }, [onOeffnen]);

  if (!sichtbar) return null;

  const vonOps = sichtbar.von_pilot_id === OPS_ID;
  const direkt = sichtbar.an_pilot_id != null;
  const wer = sichtbar.anzeigename ?? sichtbar.callsign ?? sichtbar.von_pilot_id;

  const marke = vonOps
    ? t("chat.einblendung_ops", "Flugleitung")
    : direkt
      ? t("chat.einblendung_direkt", "Direkt an dich")
      : t("chat.einblendung_alle", "Zuruf");

  return (
    <Notice
      floating
      role="status"
      aria-live="polite"
      tone={vonOps ? "warn" : "info"}
      level={`${marke} · ${wer}`}
      detail={sichtbar.text}
      className="chat-einblendung"
      onClick={oeffnen}
      actions={
        <button type="button" className="chat-einblendung__oeffnen" onClick={oeffnen}>
          {t("chat.einblendung_oeffnen", "Öffnen")}
        </button>
      }
    />
  );
}
