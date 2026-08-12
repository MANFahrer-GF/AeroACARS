// Pilotenchat — kurze Zurufe unter denen, die gerade fliegen.
//
// Kein Discord-Ersatz. Der Zweck ist, im Cockpit erreichbar zu bleiben:
// Alt-Tab kostet im Sim spürbar, und genau diesen Griff soll man sich sparen.
// Daraus folgt der Zuschnitt — keine Themenstränge, keine Dateien, keine
// Reaktionen. Ein Zuruf, mehr nicht.
//
// Drei Entscheidungen, die im Code nicht offensichtlich sind:
//
//   1. Der Tastaturfokus wird NIE von selbst geholt. Eine eingehende
//      Nachricht darf dem Piloten nicht die Tastatur wegnehmen — im
//      randlosen Vollbild gingen seine nächsten Tastendrücke sonst in den
//      Chat statt in den Sim. Wer schreiben will, klickt bewusst hinein und
//      sieht dann einen Warnstreifen.
//   2. Ab dem Endanflug verschwindet das Eingabefeld. Es bleiben die
//      Schnellzurufe, die keine Tastatur brauchen. Dieselbe Regel wie beim
//      Sim-Panel: ruhig, wenn es eng wird.
//   3. Ein Klick auf einen Namen adressiert eine Direktnachricht. Über dem
//      Feld steht dann sichtbar, an wen sie geht — man soll nie versehentlich
//      an alle schreiben, wenn man einen einzelnen meinte.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, listen } from "../lib/ipc";
import { Button } from "./ui";
import "./chat.css";

export interface ChatNachricht {
  id: number;
  va_prefix: string;
  von_pilot_id: string;
  an_pilot_id?: string | null;
  ts: number;
  text: string;
  callsign?: string | null;
  anzeigename?: string | null;
}

export interface ChatTeilnehmer {
  pilot_id: string;
  callsign?: string | null;
  dep?: string | null;
  arr?: string | null;
  anzeigename?: string | null;
}

/** Phasen, in denen Tippen nichts verloren hat. */
const KEINE_TASTATUR = new Set(["FINAL", "LANDING", "TAKEOFF_ROLL", "TAKEOFF"]);

function uhrzeit(ts: number): string {
  const d = new Date(ts);
  return `${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}`;
}

export function ChatView({
  eigenePilotId,
  phase,
  onGelesen,
}: {
  eigenePilotId: string | null;
  /** Aktuelle Flugphase — hier nur noch für die Tastatur-Sperre. */
  phase: string | null;
  /** Meldet der Hülle, dass alles gelesen ist (Zähler am Menü zurücksetzen). */
  onGelesen?: () => void;
}) {
  const { t } = useTranslation();
  const [nachrichten, setNachrichten] = useState<ChatNachricht[]>([]);
  const [teilnehmer, setTeilnehmer] = useState<ChatTeilnehmer[]>([]);
  const [entwurf, setEntwurf] = useState("");
  const [empfaenger, setEmpfaenger] = useState<ChatTeilnehmer | null>(null);
  const [tastaturImChat, setTastaturImChat] = useState(false);
  const [sendet, setSendet] = useState(false);
  const listeRef = useRef<HTMLDivElement>(null);
  const feldRef = useRef<HTMLInputElement>(null);

  const tippenGesperrt = phase != null && KEINE_TASTATUR.has(phase);

  // ── Einstieg: Verlauf + wer fliegt ───────────────────────────────────
  useEffect(() => {
    let abgebrochen = false;
    void (async () => {
      try {
        const v = await invoke<{ nachrichten: ChatNachricht[] }>("chat_verlauf");
        if (!abgebrochen) setNachrichten(v?.nachrichten ?? []);
      } catch { /* leer starten, MQTT füllt nach */ }
      try {
        const p = await invoke<{ teilnehmer: ChatTeilnehmer[] }>("chat_teilnehmer");
        if (!abgebrochen) setTeilnehmer(p?.teilnehmer ?? []);
      } catch { /* dito */ }
    })();
    return () => { abgebrochen = true; };
  }, []);

  // Teilnehmerliste im Takt nachziehen: wer landet, verschwindet; wer
  // startet, kommt dazu. Zwei Minuten reichen — das ist keine Live-Karte.
  useEffect(() => {
    const id = setInterval(() => {
      void invoke<{ teilnehmer: ChatTeilnehmer[] }>("chat_teilnehmer")
        .then((p) => setTeilnehmer(p?.teilnehmer ?? []))
        .catch(() => { /* nächster Takt */ });
    }, 120_000);
    return () => clearInterval(id);
  }, []);

  // ── Eingehende Zurufe ────────────────────────────────────────────────
  useEffect(() => {
    const p = listen<ChatNachricht>("chat-nachricht", (e) => {
      const n = e.payload;
      setNachrichten((alt) => {
        // Doppelte abwehren: derselbe Zuruf kann über den Verlauf UND über
        // MQTT hereinkommen, wenn beides kurz hintereinander passiert.
        if (alt.some((m) => m.id === n.id)) return alt;
        return [...alt, n].slice(-200);
      });
      // Kein Ton hier: der laeuft im dauerhaften Empfaenger in App.tsx.
      // Wuerde er auch hier liegen, klaenge er bei offenem Chat doppelt —
      // und bei geschlossenem gar nicht, was der Befund vom 12.08. war.
    });
    return () => { void p.then((ab) => ab()); };
  }, []);

  // Ans Ende scrollen, wenn etwas dazukommt.
  useEffect(() => {
    const el = listeRef.current;
    if (el) el.scrollTop = el.scrollHeight;
    onGelesen?.();
  }, [nachrichten, onGelesen]);

  // Im Endanflug den Fokus abgeben, falls jemand noch im Feld stand.
  useEffect(() => {
    if (tippenGesperrt) {
      feldRef.current?.blur();
      setTastaturImChat(false);
    }
  }, [tippenGesperrt]);

  const senden = useCallback(async (text: string) => {
    const sauber = text.trim();
    if (!sauber || sendet) return;
    setSendet(true);
    try {
      await invoke<boolean>("chat_senden", {
        text: sauber,
        anPilotId: empfaenger?.pilot_id ?? null,
      });
      setEntwurf("");
      setEmpfaenger(null);
    } catch {
      /* Fehlschlag ist sichtbar: die Nachricht taucht nicht auf. */
    } finally {
      setSendet(false);
    }
  }, [empfaenger, sendet]);

  const andere = useMemo(
    () => teilnehmer.filter((p) => p.pilot_id !== eigenePilotId),
    [teilnehmer, eigenePilotId],
  );

  // Wer dasselbe Ziel hat, steht vorn — „wer ist auch nach Catania
  // unterwegs" ist im Flug die eigentliche Frage.
  const meinZiel = teilnehmer.find((p) => p.pilot_id === eigenePilotId)?.arr ?? null;
  const sortiert = useMemo(() => {
    if (!meinZiel) return andere;
    return [...andere].sort((a, b) => {
      const az = a.arr === meinZiel ? 0 : 1;
      const bz = b.arr === meinZiel ? 0 : 1;
      return az - bz;
    });
  }, [andere, meinZiel]);

  const schnellzurufe = [
    t("chat.quick.gate", "Bin gleich am Gate"),
    t("chat.quick.wer", "Wer ist noch unterwegs?"),
    t("chat.quick.delay", "Habe Verspätung"),
    t("chat.quick.taxi", "Rolle raus"),
    t("chat.quick.down", "Bin runter"),
  ];

  return (
    <div className="chat">
      <div className="chat__kopf">
        <span className="chat__titel">{t("chat.title", "Pilotenchat")}</span>
        <span className="chat__wer">
          <span className="chat__punkt" />
          {t("chat.in_der_luft", { count: teilnehmer.length, defaultValue: "{{count}} in der Luft" })}
        </span>
      </div>

      {sortiert.length > 0 && (
        <div className="chat__leiste" aria-label={t("chat.wer_fliegt", "Wer gerade fliegt")}>
          {sortiert.map((p) => (
            <button
              key={p.pilot_id}
              type="button"
              className={`chat__pilot${empfaenger?.pilot_id === p.pilot_id ? " chat__pilot--ziel" : ""}`}
              title={t("chat.direkt_an", { name: p.anzeigename ?? p.callsign ?? p.pilot_id, defaultValue: "Direkt an {{name}} schreiben" })}
              onClick={() => {
                setEmpfaenger(p);
                if (!tippenGesperrt) feldRef.current?.focus();
              }}
            >
              <span className="chat__pilot-name">{p.anzeigename ?? p.pilot_id}</span>
              <span className="chat__pilot-ruf">{p.callsign ?? "—"}</span>
              <span className="chat__pilot-weg">
                {p.dep ?? "?"} → {p.arr ?? "?"}
              </span>
            </button>
          ))}
        </div>
      )}

      <div className="chat__log" ref={listeRef} role="log" aria-live="polite">
        {nachrichten.length === 0 && (
          <div className="chat__leer">
            {t("chat.leer", "Noch nichts gesagt. Die letzten zwölf Stunden sind zu sehen.")}
          </div>
        )}
        {nachrichten.map((n) => {
          const eigen = n.von_pilot_id === eigenePilotId;
          const direkt = n.an_pilot_id != null;
          return (
            <div
              key={n.id}
              className={`chat__msg${eigen ? " chat__msg--eigen" : ""}${direkt ? " chat__msg--direkt" : ""}`}
            >
              <time>{uhrzeit(n.ts)}</time>
              <div>
                {direkt && (
                  <span className="chat__marke">
                    {eigen
                      ? t("chat.marke_an", "DIREKT")
                      : t("chat.marke_von", "NUR AN DICH")}
                  </span>
                )}
                <span className="chat__who">{n.callsign ?? n.von_pilot_id}</span>
                {n.anzeigename && <span className="chat__name">{eigen ? t("chat.du", "du") : n.anzeigename}</span>}
                {n.text}
              </div>
            </div>
          );
        })}
      </div>

      <div className="chat__schnell">
        <span className="chat__schnell-marke">{t("chat.zuruf", "Zuruf")}</span>
        {schnellzurufe.map((z) => (
          <button key={z} type="button" onClick={() => void senden(z)} disabled={sendet}>
            {z}
          </button>
        ))}
      </div>

      {empfaenger && (
        <div className="chat__an-wen">
          <span>
            {t("chat.direkt_an_kurz", "Direkt an")}{" "}
            <b>{empfaenger.anzeigename ?? empfaenger.pilot_id}</b>
          </span>
          <span className="chat__an-ruf">{empfaenger.callsign}</span>
          <button type="button" className="chat__an-weg" onClick={() => setEmpfaenger(null)}>
            {t("chat.an_alle", "an alle stattdessen")}
          </button>
        </div>
      )}

      {tippenGesperrt ? (
        <div className="chat__gesperrt">
          {t(
            "chat.gesperrt",
            "Endanflug. Tippen ist weggeräumt — nur noch Zurufe auf Knopfdruck. Lesen geht weiter.",
          )}
        </div>
      ) : (
        <form
          className="chat__eingabe"
          onSubmit={(e) => {
            e.preventDefault();
            void senden(entwurf);
          }}
        >
          <input
            ref={feldRef}
            value={entwurf}
            onChange={(e) => setEntwurf(e.target.value)}
            onFocus={() => setTastaturImChat(true)}
            onBlur={() => setTastaturImChat(false)}
            onKeyDown={(e) => {
              if (e.key === "Escape") feldRef.current?.blur();
            }}
            maxLength={280}
            placeholder={
              empfaenger
                ? t("chat.platzhalter_direkt", { name: empfaenger.anzeigename ?? empfaenger.pilot_id, defaultValue: "Direkt an {{name}} …" })
                : t("chat.platzhalter", "Kurz zurufen …")
            }
            aria-label={t("chat.eingabe", "Nachricht schreiben")}
          />
          <Button type="submit" disabled={sendet || !entwurf.trim()}>
            {t("chat.senden", "Senden")}
          </Button>
        </form>
      )}

      {tastaturImChat && (
        <div className="chat__fokus" role="status">
          <span>{t("chat.fokus", "Die Tastatur liegt jetzt im Chat — der Sim bekommt nichts.")}</span>
          <kbd>Esc</kbd>
          <span>{t("chat.fokus_zurueck", "gibt sie zurück.")}</span>
        </div>
      )}
    </div>
  );
}
