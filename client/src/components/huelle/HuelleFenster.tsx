// v3 (Etappe 5, P-8.01/P-8.10): Bestätigungsdialog der eingebauten Hülle.
//
// Läuft im Fenster `huelle` (eingebautes Frontend, Capability huelle, Texte
// nur aus dem Programm — nie aus dem Laufzeit-Paket, sonst könnte ein
// fremdes Paket den Dialog umformulieren). Zeigt die Aufträge, die die
// Oberfläche mit `shell_request` abgelegt hat: ein Satz, was passiert
// (`befehlstexte.ts`, im Programm), darunter Befehl und Werte (Geheimnisse
// maskiert). „Ausführen“ ruft den Befehl HIER auf und meldet sein Ergebnis
// zurück; „Ablehnen“ oder Fenster schließen lehnt ab.

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, listen } from "../../lib/ipc";
import { Button } from "../ui";
import { befehlstext, werteAnzeige } from "./befehlstexte";
import "./huelle.css";

export interface Auftrag {
  id: number;
  befehl: string;
  werte: Record<string, unknown>;
}

export function HuelleFenster() {
  const { t, i18n } = useTranslation();
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
  const satz = befehlstext(a.befehl, i18n?.resolvedLanguage ?? i18n?.language);
  const werte = werteAnzeige(a.werte);
  return (
    <main className="huelle" role="dialog" aria-labelledby="huelle-titel">
      <h1 id="huelle-titel">{t("huelle.titel")}</h1>
      <p>{t("huelle.frage")}</p>
      {satz && (
        <p data-testid="huelle-satz">
          <strong>{satz}</strong>
        </p>
      )}
      <p>
        <code data-testid="huelle-befehl">{a.befehl}</code>
      </p>
      {werte.length > 0 && <pre data-testid="huelle-werte">{werte.map(([k, v]) => `${k}: ${v}`).join("\n")}</pre>}
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
