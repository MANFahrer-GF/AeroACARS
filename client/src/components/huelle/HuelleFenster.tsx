// v3 (Etappe 5, P-8.01/P-8.10): Bestätigungsdialog der eingebauten Hülle.
//
// Läuft im Fenster `huelle` (eingebautes Frontend, Capability huelle, Texte
// nur aus dem Programm — nie aus dem Laufzeit-Paket, sonst könnte ein
// fremdes Paket den Dialog umformulieren). Zeigt die Aufträge, die die
// Oberfläche mit `shell_request` abgelegt hat: ein Satz, was passiert
// (`befehlstexte.ts`, im Programm) als konkrete Frage mit Verb-Knopf;
// Befehl und Werte (Geheimnisse maskiert) nur unter „Technische Details“.
// Der Knopf ruft den Befehl HIER auf und meldet sein Ergebnis zurück;
// „Abbrechen“ oder Fenster schließen lehnt ab.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, listen } from "../../lib/ipc";
import { Button } from "../ui";
import { RAHMEN, dialogText, sprache, werteAnzeige } from "./befehlstexte";
import "./huelle.css";

export interface Auftrag {
  id: number;
  befehl: string;
  werte: Record<string, unknown>;
}

export function HuelleFenster() {
  const { i18n } = useTranslation();
  const r = RAHMEN[sprache(i18n?.resolvedLanguage ?? i18n?.language)];
  const [auftraege, setAuftraege] = useState<Auftrag[]>([]);
  const [laeuft, setLaeuft] = useState(false);
  const flaeche = useRef<HTMLElement>(null);
  const angefordert = useRef(0);

  // 3.0.0-beta.2.1 (Thomas 31.10.): Das Fenster bekommt die Höhe seines
  // Inhalts — vorher 560×440, Inhalt nur im oberen Drittel, Release Notes
  // dahinter verdeckt. Auch beim Aufklappen der Details.
  useEffect(() => {
    const el = flaeche.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    // Thomas 07.11.: Am Mac gilt die Hoehe samt Titelleiste, die Knoepfe waren abgeschnitten.
    // Was vom Angeforderten nicht im Fenster ankommt, wird deshalb nachgemessen und aufgeschlagen.
    let takt: ReturnType<typeof setTimeout> | undefined;
    const pruefen = () => {
      const hoehe = Math.ceil(el.getBoundingClientRect().height);
      if (hoehe <= 0) return;
      const rand = angefordert.current > 0 ? Math.max(0, angefordert.current - window.innerHeight) : 0;
      if (hoehe + rand === angefordert.current) return;
      angefordert.current = hoehe + rand;
      invoke("fenster_an_inhalt_anpassen", { benoetigtHoehe: hoehe + rand }).catch(() => {});
    };
    // Erst wenn das Fenster steht: ein Wert mitten im Umbau wuerde es springen lassen.
    const melden = () => {
      clearTimeout(takt);
      takt = setTimeout(pruefen, 40);
    };
    const beobachter = new ResizeObserver(melden);
    beobachter.observe(el);
    window.addEventListener("resize", melden);
    melden();
    return () => {
      clearTimeout(takt);
      beobachter.disconnect();
      window.removeEventListener("resize", melden);
    };
  });

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
  if (!a) return <main ref={flaeche} className="huelle">{r.leer}</main>;
  const text = dialogText(a.befehl, a.werte, i18n?.resolvedLanguage ?? i18n?.language);
  const werte = werteAnzeige(a.werte);
  // Die Frage steht oben, die Erklaerung dazu ruhig darunter.
  const [kopf, ...rest] = text ? text.frage.split(/(?<=\?)\s+/) : [r.titel, r.unbekannt];
  return (
    <main ref={flaeche} className="huelle" role="dialog" aria-labelledby="huelle-titel" aria-describedby="huelle-satz">
      <div id="huelle-satz" data-testid="huelle-satz" className="huelle__satz">
        <h1 id="huelle-titel">{kopf}</h1>
        {rest.length > 0 && <p className="huelle__frage">{rest.join(" ")}</p>}
      </div>
      {auftraege.length > 1 && <p className="huelle__weitere">{r.weitere.replace("{{anzahl}}", String(auftraege.length - 1))}</p>}
      <details className="huelle__details" open={!text}>
        <summary>{r.details}</summary>
        <code data-testid="huelle-befehl">{a.befehl}</code>
        {werte.length > 0 && <pre data-testid="huelle-werte">{werte.map(([k, v]) => `${k}: ${v}`).join("\n")}</pre>}
      </details>
      {laeuft ? (
        <p className="huelle__laeuft" role="status" data-testid="huelle-laeuft">
          {r.laeuft}
        </p>
      ) : (
        <div className="huelle__knoepfe">
          <Button variant="quiet" onClick={() => void antworten(a, false)}>
            {r.abbrechen}
          </Button>
          <Button variant="primary" onClick={() => void antworten(a, true)}>
            {text ? text.knopf : r.ausfuehren}
          </Button>
        </div>
      )}
    </main>
  );
}
