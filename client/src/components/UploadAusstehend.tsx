// v3 (P-7.24): „Upload ausstehend seit …“ je Flug im Logbuch.
//
// Eingabe-Dokument und Flugprotokoll gehen über die Warteschlange
// `pending_uploads/` an live (P-7.21) und werden wiederholt, bis sie
// ankommen. Solange einer davon liegt, sieht der Pilot es an der Zeile —
// mit dem Grund des letzten Fehlschlags im Tooltip. Hat live für die
// Landung schon eine andere Fassung (HTTP 409), steht dort „Konflikt“: die
// Fassung bleibt liegen, wird aber nicht erneut gesendet (Codex 30.10.).
import { invoke } from "../lib/ipc";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

export interface UploadAusstehend {
  pirep_id: string;
  art: "eingabe" | "protokoll";
  /** Eingereiht seit (Unix-ms). */
  seit_ms: number;
  versuche: number;
  grund: string | null;
  /** live hat schon eine andere Fassung (409): nicht erneut gesendet. */
  konflikt?: boolean;
}

/** Nachfrage-Takt — derselbe wie der Arbeiter (60 s). */
export const TAKT_MS = 60_000;

/** Ausstehende Uploads des angemeldeten Kontos, je PIREP gruppiert. */
export function useUploadsAusstehend(): Record<string, UploadAusstehend[]> {
  const [stand, setStand] = useState<Record<string, UploadAusstehend[]>>({});
  useEffect(() => {
    let aktiv = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const laden = async () => {
      try {
        const roh = await invoke<UploadAusstehend[]>("uploads_ausstehend");
        if (!aktiv) return;
        const neu: Record<string, UploadAusstehend[]> = {};
        for (const u of Array.isArray(roh) ? roh : []) (neu[u.pirep_id] ??= []).push(u);
        setStand(neu);
      } catch (e) {
        console.warn("uploads_ausstehend failed", e);
      }
      if (aktiv) timer = setTimeout(laden, TAKT_MS);
    };
    void laden();
    return () => {
      aktiv = false;
      if (timer) clearTimeout(timer);
    };
  }, []);
  return stand;
}

/** Kennzeichen für die Logbuchzeile — nur, wenn etwas aussteht. */
export function UploadMarke({ eintraege }: { eintraege: UploadAusstehend[] | undefined }) {
  const { t, i18n } = useTranslation();
  if (!eintraege || eintraege.length === 0) return null;
  const seit = Math.min(...eintraege.map((u) => u.seit_ms));
  const wann = new Intl.DateTimeFormat(i18n.language, { dateStyle: "short", timeStyle: "short" }).format(new Date(seit));
  const gruende = eintraege
    .map((u) => u.grund)
    .filter((g): g is string => !!g)
    .map((grund) => t("logbook_view.upload_grund", { grund }));
  const titel = gruende.length > 0 ? gruende.join("\n") : t("logbook_view.upload_noch_kein_versuch");
  if (eintraege.some((u) => u.konflikt)) {
    return (
      <span className="pruefstatus-marke pruefstatus-marke--abgelehnt" data-testid="upload-marke" title={t("logbook_view.upload_konflikt")}>
        {t("logbook_view.upload_konflikt_marke")}
      </span>
    );
  }
  return (
    <span
      className="pruefstatus-marke pruefstatus-marke--in_pruefung"
      data-testid="upload-marke"
      title={titel}
    >
      {t("logbook_view.upload_ausstehend", { seit: wann })}
    </span>
  );
}

/** Kasten in der Detailansicht: je Teil „seit“, Versuche und Grund. */
export function UploadKasten({ eintraege }: { eintraege: UploadAusstehend[] | undefined }) {
  const { t, i18n } = useTranslation();
  if (!eintraege || eintraege.length === 0) return null;
  const fmt = new Intl.DateTimeFormat(i18n.language, { dateStyle: "short", timeStyle: "short" });
  return (
    <div className="pruefstatus-kasten pruefstatus-kasten--in_pruefung" role="status" data-testid="upload-kasten">
      <div className="pruefstatus-kasten__titel">{t("logbook_view.upload_titel")}</div>
      <ul className="pruefstatus-kasten__gruende">
        {eintraege.map((u, i) =>
          u.konflikt ? (
            <li key={`${u.art}-${i}`}>
              {t(`logbook_view.upload_art_${u.art}`)} · {t("logbook_view.upload_konflikt")}
            </li>
          ) : (
            <li key={`${u.art}-${i}`}>
              {t(`logbook_view.upload_art_${u.art}`)} · {t("logbook_view.upload_ausstehend", { seit: fmt.format(new Date(u.seit_ms)) })}
              {" · "}
              {u.grund ? t("logbook_view.upload_grund", { grund: u.grund }) : t("logbook_view.upload_noch_kein_versuch")}
            </li>
          ),
        )}
      </ul>
    </div>
  );
}
