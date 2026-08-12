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
import { invoke, listen, openExternal } from "../lib/ipc";
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
  /** Ob dieser Pilot einen Zuruf empfangen kann. Flüge über die
   *  Stratos-Brücke und ältere Clients hören auf dem Rückkanal nicht zu —
   *  eine Nachricht an sie sähe abgeschickt aus und käme nie an. Fehlt das
   *  Feld (älterer Server), gilt der Pilot als erreichbar. */
  erreichbar?: boolean;
  /** Flug schon abgeschlossen — der Kollege ist im Nachlauf und
   *  verschwindet gleich aus der Liste. */
  gelandet?: boolean;
}

/** Absenderkennung der Flugleitung — kein Pilot kann die haben. */
const OPS_ID = "__ops";

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
  /** Der Server laesst gerade niemanden reden — kein laufender Flug. */
  const [amBoden, setAmBoden] = useState(false);
  const [sendet, setSendet] = useState(false);
  const listeRef = useRef<HTMLDivElement>(null);
  const feldRef = useRef<HTMLInputElement>(null);

  const tippenGesperrt = phase != null && KEINE_TASTATUR.has(phase);

  // ── Einstieg und Abgleich: Verlauf + wer fliegt ──────────────────────
  //
  // Beides in EINEM Weg, aus zwei Gründen:
  //
  //   1. Wettlauf beim Öffnen. Vorher setzte der Verlauf-Abruf die Liste
  //      hart (`setNachrichten(v.nachrichten)`). Traf ein Zuruf über MQTT
  //      ein, WÄHREND der Abruf noch lief, war er danach weg — der Abruf
  //      kannte ihn ja noch nicht. Jetzt wird zusammengeführt statt
  //      ersetzt; die Kennung entscheidet, was doppelt ist.
  //
  //   2. Löcher nach einem Verbindungsabriss. Wer im Funkschatten war
  //      (Tablet aus, WLAN weg, Rechner im Schlaf), hat die Zurufe dieser
  //      Zeit nie bekommen. Deshalb wird nicht nur beim Öffnen abgeglichen,
  //      sondern auch im Takt und immer dann, wenn das Fenster wieder
  //      sichtbar wird.
  const letzterAbgleich = useRef(0);
  const abgleichen = useCallback(async (erzwingen = false) => {
    // QS 12.08.2026: Der Abgleich hängt auch am Sichtbarwerden des
    // Fensters. Wer zwischen Sim und Client hin und her klickt, löst ihn
    // dabei im Sekundentakt aus — über die LAN-Brücke wäre das eine Kette
    // von Anfragen für nichts. Häufiger als alle 15 Sekunden lohnt es
    // ohnehin nicht; der laufende Betrieb kommt über MQTT.
    const jetzt = Date.now();
    if (!erzwingen && jetzt - letzterAbgleich.current < 15_000) return;
    letzterAbgleich.current = jetzt;
    try {
      const v = await invoke<{ nachrichten: ChatNachricht[]; kein_laufender_flug?: boolean }>("chat_verlauf");
      const geholt = v?.nachrichten ?? [];
      // Der Server sagt, ob ueberhaupt geredet werden darf. Vorher stand
      // das Eingabefeld auch am Boden bereit, und der Zuruf verschwand
      // lautlos — "kommt keine durch, gut so, aber transparent ist das
      // nicht" (Thomas, 12.08.2026).
      setAmBoden(v?.kein_laufender_flug === true);
      setNachrichten((alt) => {
        const bekannt = new Set(alt.map((m) => m.id));
        const neue = geholt.filter((m) => !bekannt.has(m.id));
        if (neue.length === 0) return alt;
        return [...alt, ...neue].sort((a, b) => a.ts - b.ts).slice(-200);
      });
    } catch { /* leer starten, MQTT füllt nach */ }
    try {
      const p = await invoke<{ teilnehmer: ChatTeilnehmer[] }>("chat_teilnehmer");
      setTeilnehmer(p?.teilnehmer ?? []);
    } catch { /* dito */ }
  }, []);

  useEffect(() => {
    void abgleichen(true);
    // Zwei Minuten reichen — das ist keine Live-Karte. Der Takt holt
    // zugleich die Teilnehmer nach: wer landet, verschwindet; wer startet,
    // kommt dazu.
    const id = setInterval(() => { void abgleichen(true); }, 120_000);
    const beiSichtbar = () => {
      if (document.visibilityState === "visible") void abgleichen();
    };
    document.addEventListener("visibilitychange", beiSichtbar);
    return () => {
      clearInterval(id);
      document.removeEventListener("visibilitychange", beiSichtbar);
    };
  }, [abgleichen]);

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
        {/* Feldbefund 12.08.2026: Am Boden stand hier "0 in der Luft" — und
            das stimmte nicht. Der Server verrät einem, der nicht fliegt,
            aus gutem Grund nicht, wer gerade unterwegs ist; die leere
            Antwort heißt also "wir wissen es nicht", nicht "niemand".
            Eine Null zu zeigen, wo Unkenntnis herrscht, ist die
            unehrlichste aller Anzeigen. */}
        {amBoden ? (
          <span className="chat__wer chat__wer--unbekannt">
            {t("chat.wer_unbekannt", "Wer fliegt, siehst du im Flug")}
          </span>
        ) : (
          <span className="chat__wer">
            <span className="chat__punkt" />
            {t("chat.in_der_luft", { count: teilnehmer.length, defaultValue: "{{count}} in der Luft" })}
          </span>
        )}
        {/* Die Regeln stehen dort, wo man sie braucht — nicht nur auf einer
            Rechtsseite, die niemand aufschlaegt.
            Feldbefund 12.08.2026: "kein Hinweis zu Datenschutz (Link)". Der
            Knopf war zwar da, sah aber aus wie ein Zustandstext ("12 h
            Gedaechtnis · Flugleitung liest mit") — niemand erkannte darin
            einen Verweis. Jetzt steht die Sache daneben und der Verweis
            heisst, was er ist. */}
        <span className="chat__regeln-kurz">
          {t("chat.regeln", "12 h Gedächtnis · Flugleitung liest mit")}
        </span>
        <button
          type="button"
          className="chat__regeln"
          onClick={() => void openExternal("https://german-sky-group.eu/page/impressum").catch(() => {})}
          title={t("chat.regeln_titel", "Datenschutz zum Pilotenchat öffnen")}
        >
          {t("chat.datenschutz", "Datenschutz")} ↗
        </button>
      </div>

      {sortiert.length > 0 && (
        <div className="chat__leiste" aria-label={t("chat.wer_fliegt", "Wer gerade fliegt")}>
          {sortiert.map((p) => (
            <button
              key={p.pilot_id}
              type="button"
              disabled={p.erreichbar === false}
              className={`chat__pilot${empfaenger?.pilot_id === p.pilot_id ? " chat__pilot--ziel" : ""}${p.erreichbar === false ? " chat__pilot--stumm" : ""}`}
              title={
                p.erreichbar === false
                  ? t("chat.nicht_erreichbar_titel", "Dieser Pilot fliegt mit einem Client ohne Chat — eine Nachricht käme nicht an.")
                  : t("chat.direkt_an", { name: p.anzeigename ?? p.callsign ?? p.pilot_id, defaultValue: "Direkt an {{name}} schreiben" })
              }
              onClick={() => {
                setEmpfaenger(p);
                if (!tippenGesperrt) feldRef.current?.focus();
              }}
            >
              <span className="chat__pilot-name">{p.anzeigename ?? p.pilot_id}</span>
              <span className="chat__pilot-ruf">{p.callsign ?? "—"}</span>
              <span className="chat__pilot-weg">
                {p.erreichbar === false
                  ? t("chat.nicht_erreichbar", "kein Chat")
                  : p.gelandet
                    ? t("chat.gelandet", { ort: p.arr ?? "?", defaultValue: "gelandet in {{ort}}" })
                    : `${p.dep ?? "?"} → ${p.arr ?? "?"}`}
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
          // Die Flugleitung ist kein Kollege — das muss man sehen, ohne den
          // Namen zu lesen.
          const vonOps = n.von_pilot_id === OPS_ID;
          return (
            <div
              key={n.id}
              className={`chat__msg${eigen ? " chat__msg--eigen" : ""}${direkt && !vonOps ? " chat__msg--direkt" : ""}${vonOps ? " chat__msg--ops" : ""}`}
            >
              <time>{uhrzeit(n.ts)}</time>
              <div>
                {vonOps && (
                  <span className="chat__marke chat__marke--ops">
                    {t("chat.marke_ops", "FLUGLEITUNG")}
                  </span>
                )}
                {direkt && !vonOps && (
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

      {!amBoden && (
      <div className="chat__schnell">
        <span className="chat__schnell-marke">{t("chat.zuruf", "Zuruf")}</span>
        {schnellzurufe.map((z) => (
          <button key={z} type="button" onClick={() => void senden(z)} disabled={sendet}>
            {z}
          </button>
        ))}
      </div>
      )}

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

      {amBoden ? (
        <div className="chat__zu">
          <strong>{t("chat.zu_titel", "Der Chat ist offen, solange du fliegst.")}</strong>
          <span>
            {t(
              "chat.zu_text",
              "Sobald die Aufzeichnung läuft, kannst du mitreden — und noch 30 Minuten nach dem Flugbericht.",
            )}
          </span>
          <button
            type="button"
            className="chat__zu-link"
            onClick={() => void openExternal("https://german-sky-group.eu/page/impressum").catch(() => {})}
          >
            {t("chat.datenschutz", "Datenschutz")} ↗
          </button>
        </div>
      ) : tippenGesperrt ? (
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
