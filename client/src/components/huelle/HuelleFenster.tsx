// v3 (Etappe 5, P-8.01/P-8.10): Bestätigungsdialog der eingebauten Hülle.
//
// Läuft im Fenster `huelle` (eingebautes Frontend, Capability huelle, Texte
// nur aus dem Programm — nie aus dem Laufzeit-Paket, sonst könnte ein
// fremdes Paket den Dialog umformulieren). Zeigt die Aufträge, die die
// Oberfläche mit `shell_request` abgelegt hat, mit Befehl und Werten im
// Klartext. „Ausführen“ ruft den Befehl HIER auf und meldet sein Ergebnis
// zurück; „Ablehnen“ oder Fenster schließen lehnt ab.

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, listen } from "../../lib/ipc";
import { Button } from "../ui";
import "./huelle.css";

export interface Auftrag {
  id: number;
  befehl: string;
  werte: Record<string, unknown>;
}

export function HuelleFenster() {
  const { t } = useTranslation();
  const [auftraege, setAuftraege] = useState<Auftrag[]>([]);
  const [laeuft, setLaeuft] = useState(false);

  const laden = useCallback(async () => {
    setAuftraege(await invoke<Auftrag[]>("shell_auftraege").catch(() => []));
  }, []);

  useEffect(() => {
    void laden();
    let aus: (() => void) | undefined;
    let ende = false;
    void listen("huelle-auftrag", () => void laden()).then((u) => {
      if (ende) u();
      else aus = u;
    });
    return () => {
      ende = true;
      aus?.();
    };
  }, [laden]);

  async function antworten(a: Auftrag, ja: boolean) {
    setLaeuft(true);
    try {
      if (!ja) {
        await invoke("shell_auftrag_beantworten", { id: a.id, ok: false });
      } else {
        try {
          const ergebnis = await invoke(a.befehl, a.werte);
          await invoke("shell_auftrag_beantworten", { id: a.id, ok: true, ergebnis: ergebnis ?? null });
        } catch (e) {
          await invoke("shell_auftrag_beantworten", { id: a.id, ok: false, fehler: e ?? "fehler" });
        }
      }
    } finally {
      setLaeuft(false);
      await laden();
    }
  }

  const a = auftraege[0];
  if (!a) return <main className="huelle">{t("huelle.leer")}</main>;
  const werte = Object.keys(a.werte ?? {}).length ? JSON.stringify(a.werte, null, 2) : null;
  return (
    <main className="huelle" role="dialog" aria-labelledby="huelle-titel">
      <h1 id="huelle-titel">{t("huelle.titel")}</h1>
      <p>{t("huelle.frage")}</p>
      <p>
        <code data-testid="huelle-befehl">{a.befehl}</code>
      </p>
      {werte && <pre data-testid="huelle-werte">{werte}</pre>}
      {auftraege.length > 1 && <p>{t("huelle.weitere", { anzahl: auftraege.length - 1 })}</p>}
      <div className="huelle__knoepfe">
        <Button variant="quiet" disabled={laeuft} onClick={() => void antworten(a, false)}>
          {t("huelle.ablehnen")}
        </Button>
        <Button variant="primary" disabled={laeuft} onClick={() => void antworten(a, true)}>
          {t("huelle.ausfuehren")}
        </Button>
      </div>
    </main>
  );
}
