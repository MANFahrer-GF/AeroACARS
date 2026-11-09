// Crew-Story-Fenster je Buchung (Konzept vom 09.11.2026, Fähigkeit `modul-api-1`).
// Die Regeln liegen im Kabinenansagen-Modul: Dieses Fenster lädt den Serverstand,
// speichert per Knopf und zeigt das Prüfergebnis. Jede Zeile von Text und Gründen
// geht als Text in React, nie als HTML (die Gründe kommen vom Modell).
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import { displayCallsign } from "../lib/callsign";
import type { Flight } from "../types";

const PFAD = "/api/kabinenansagen/briefing";
const MAX_RUECKFALL = 500;
/** Läuft die Prüfung woanders schon: so oft im Abstand nachsehen, dann aufgeben. */
const LAEUFT_VERSUCHE = 5;
const LAEUFT_WARTEN_MS = 4000;

interface ModulAntwort {
  status: number;
  daten: unknown;
}

interface Stand {
  text?: unknown;
  hash?: unknown;
  pruefung?: { status?: unknown; gruende?: unknown } | null;
  gesperrt?: unknown;
  max?: unknown;
  fehler?: unknown;
  status?: unknown;
  gruende?: unknown;
}

type PruefStatus = "prueft" | "uebernommen" | "teilweise" | "abgelehnt" | "nicht_geprueft";
interface Pruefung {
  status: PruefStatus;
  gruende: string[];
}

type Meldung = { art: "ok" | "fehler"; schluessel: string };

function modulApi(methode: "GET" | "POST", pfad: string, args: Record<string, unknown>) {
  return invoke<ModulAntwort>("modul_api", { methode, pfad, ...args });
}

function alsStand(a: ModulAntwort): Stand {
  return a.daten && typeof a.daten === "object" ? (a.daten as Stand) : {};
}

function textAus(wert: unknown): string {
  return typeof wert === "string" ? wert : "";
}

function pruefungAus(status: unknown, gruende: unknown): Pruefung {
  const bekannt = status === "uebernommen" || status === "teilweise" || status === "abgelehnt";
  return {
    status: bekannt ? status : "nicht_geprueft",
    gruende: Array.isArray(gruende) ? gruende.filter((g): g is string => typeof g === "string" && g !== "") : [],
  };
}

/** Schlüssel für einen Aufruf, der gar keine Antwort gebracht hat. */
function ipcFehler(e: unknown): string {
  const code = (e as { code?: unknown } | null)?.code;
  return code === "modul_api_anmeldung" ? "anmeldung" : "netz";
}

/** Schlüssel für eine Antwort, die weder Erfolg noch 423/409 ist. */
function antwortFehler(a: ModulAntwort, sonst: string): string {
  const fehler = alsStand(a).fehler;
  if (a.status === 401) return "anmeldung";
  if (a.status === 403 && fehler === "nicht_freigeschaltet") return "kein_zugang";
  if (a.status === 404 && fehler === "flug_unbekannt") return "flug_unbekannt";
  if (a.status === 422) return "ungueltig";
  if (a.status === 429) return "zu_oft";
  return sonst;
}

function neueQuelle(): string {
  // Kein crypto.randomUUID: Über die LAN-Brücke läuft die Seite ohne HTTPS.
  return Array.from({ length: 16 }, () => Math.floor(Math.random() * 36).toString(36)).join("");
}

interface Props {
  flight: Flight;
  /** Buchung des laufenden Flugs: nur lesen, auch ohne Antwort des Servers. */
  nurLesen?: boolean;
  /** Programm ohne `modul-api-1` und ohne OFP: nur der Hinweis. */
  ohneKern?: boolean;
  onClose: () => void;
}

export function CrewStoryFenster({ flight, nurLesen = false, ohneKern = false, onClose }: Props) {
  const { t } = useTranslation();
  const [zustand, setZustand] = useState<"laedt" | "bereit" | "fehler">("laedt");
  const [ladeFehler, setLadeFehler] = useState("server");
  const [text, setText] = useState("");
  const [gespeichert, setGespeichert] = useState("");
  const [gesperrt, setGesperrt] = useState(nurLesen);
  const [max, setMax] = useState(MAX_RUECKFALL);
  const [pruefung, setPruefung] = useState<Pruefung | null>(null);
  const [meldung, setMeldung] = useState<Meldung | null>(null);
  const [serverText, setServerText] = useState<string | null>(null);
  const [speichert, setSpeichert] = useState(false);
  const [schliessenFragen, setSchliessenFragen] = useState(false);
  const basis = useRef("");
  const rev = useRef(0);
  const quelle = useRef(neueQuelle());
  // Jede neue Prüfung und jedes Schließen macht ältere Antworten ungültig.
  const pruefLauf = useRef(0);

  const pruefen = useCallback(
    async (hash: string, versuch = 0) => {
      const lauf = ++pruefLauf.current;
      setPruefung({ status: "prueft", gruende: [] });
      let a: ModulAntwort;
      try {
        a = await modulApi("POST", `${PFAD}/pruefen`, { koerper: { flight_id: flight.id, hash } });
      } catch {
        if (pruefLauf.current === lauf) setPruefung({ status: "nicht_geprueft", gruende: [] });
        return;
      }
      if (pruefLauf.current !== lauf) return;
      const d = alsStand(a);
      if (a.status === 200) {
        setPruefung(pruefungAus(d.status, d.gruende));
      } else if (a.status === 423) {
        setGesperrt(true);
        setPruefung({ status: "nicht_geprueft", gruende: [] });
      } else if (a.status === 409 && d.fehler === "laeuft" && versuch < LAEUFT_VERSUCHE) {
        window.setTimeout(() => {
          if (pruefLauf.current === lauf) void pruefen(hash, versuch + 1);
        }, LAEUFT_WARTEN_MS);
      } else if (a.status === 409 && d.fehler === "veraltet") {
        setPruefung(null);
      } else {
        setPruefung({ status: "nicht_geprueft", gruende: [] });
      }
    },
    [flight.id],
  );

  const laden = useCallback(async () => {
    setZustand("laedt");
    let a: ModulAntwort;
    try {
      a = await modulApi("GET", PFAD, { abfrage: { flight_id: flight.id } });
    } catch (e) {
      setLadeFehler(ipcFehler(e));
      setZustand("fehler");
      return;
    }
    if (a.status !== 200) {
      setLadeFehler(antwortFehler(a, "server"));
      setZustand("fehler");
      return;
    }
    const d = alsStand(a);
    const serverStand = textAus(d.text);
    const hash = textAus(d.hash);
    const zu = nurLesen || d.gesperrt === true;
    basis.current = hash;
    setText(serverStand);
    setGespeichert(serverStand);
    setGesperrt(zu);
    if (typeof d.max === "number" && d.max > 0) setMax(d.max);
    setZustand("bereit");
    if (d.pruefung) setPruefung(pruefungAus(d.pruefung.status, d.pruefung.gruende));
    else if (hash && zu) setPruefung({ status: "nicht_geprueft", gruende: [] });
    else if (hash) void pruefen(hash);
    else setPruefung(null);
  }, [flight.id, nurLesen, pruefen]);

  useEffect(() => {
    if (!ohneKern) void laden();
    return () => {
      pruefLauf.current++;
    };
  }, [laden, ohneKern]);

  const ungespeichert = zustand === "bereit" && !gesperrt && text !== gespeichert;

  const schliessen = useCallback(() => {
    if (speichert) return;
    if (ungespeichert && !schliessenFragen) {
      setSchliessenFragen(true);
      return;
    }
    onClose();
  }, [onClose, schliessenFragen, speichert, ungespeichert]);

  useEffect(() => {
    const taste = (e: KeyboardEvent) => {
      if (e.key === "Escape") schliessen();
    };
    window.addEventListener("keydown", taste);
    return () => window.removeEventListener("keydown", taste);
  }, [schliessen]);

  async function speichern() {
    if (speichert || gesperrt || text === gespeichert) return;
    const gesendet = text;
    pruefLauf.current++;
    setSpeichert(true);
    setMeldung(null);
    setServerText(null);
    setSchliessenFragen(false);
    let a: ModulAntwort;
    try {
      a = await modulApi("POST", PFAD, {
        koerper: { flight_id: flight.id, text: gesendet, rev: ++rev.current, quelle: quelle.current, anlass: "knopf", basis: basis.current },
      });
    } catch (e) {
      setMeldung({ art: "fehler", schluessel: ipcFehler(e) });
      setSpeichert(false);
      return;
    }
    setSpeichert(false);
    const d = alsStand(a);
    if (a.status === 200) {
      const bestaetigt = textAus(d.text);
      const hash = textAus(d.hash);
      basis.current = hash;
      setText(bestaetigt);
      setGespeichert(bestaetigt);
      setMeldung({ art: "ok", schluessel: "gespeichert" });
      if (hash) void pruefen(hash);
      else setPruefung(null);
    } else if (a.status === 423) {
      // Der ungespeicherte Text bleibt stehen, damit nichts verloren geht.
      setGesperrt(true);
      setPruefung(null);
      setMeldung({ art: "fehler", schluessel: "gesperrt" });
    } else if (a.status === 409 && d.fehler === "geaendert") {
      // Serverstand zeigen, neue Basis setzen: Das nächste Speichern gewinnt.
      const stand = textAus(d.text);
      basis.current = textAus(d.hash);
      setGespeichert(stand);
      setServerText(stand);
      setPruefung(null);
      setMeldung({ art: "fehler", schluessel: "geaendert" });
    } else {
      setMeldung({ art: "fehler", schluessel: antwortFehler(a, "nicht_gespeichert") });
    }
  }

  function aendern(neu: string) {
    pruefLauf.current++;
    setText(neu);
    setPruefung(null);
    setMeldung(null);
    setSchliessenFragen(false);
  }

  const meldungText = (schluessel: string) => t(`bids.crew_story.meldung.${schluessel}`);
  const kopf = pruefung
    ? pruefung.status === "abgelehnt" && pruefung.gruende.length === 0
      ? t("bids.crew_story.pruefung.abgelehnt_ohne")
      : t(`bids.crew_story.pruefung.${pruefung.status}`)
    : "";

  return (
    <div
      className="manual-modal__backdrop"
      data-testid="crew-story-hintergrund"
      onClick={() => {
        // Getippter Text darf nicht an einem Klick neben das Fenster hängen.
        if (!ungespeichert && !speichert) onClose();
      }}
    >
      <div
        className="manual-modal crew-story"
        role="dialog"
        aria-modal="true"
        aria-labelledby="crew-story-titel"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="manual-modal__head">
          <h3 id="crew-story-titel">{t("bids.crew_story.titel")}</h3>
          <div className="manual-modal__sub">
            {displayCallsign(flight.airline?.icao, flight.flight_number, flight.callsign)}
            {" · "}
            {flight.dpt_airport_id} → {flight.arr_airport_id}
          </div>
        </header>

        <div className="manual-modal__body crew-story__body">
          {ohneKern && <p className="crew-story__hinweis">{t("bids.crew_story.neues_aeroacars")}</p>}
          {!ohneKern && zustand === "laedt" && <div className="manual-modal__loading">{t("bids.crew_story.laedt")}</div>}
          {!ohneKern && zustand === "fehler" && (
            <div className="manual-modal__error" role="alert">
              {meldungText(ladeFehler)}
            </div>
          )}
          {!ohneKern && zustand === "bereit" && (
            <>
              <textarea
                className="crew-story__text"
                rows={6}
                value={text}
                maxLength={max}
                readOnly={gesperrt || speichert}
                autoFocus={!gesperrt}
                placeholder={gesperrt ? undefined : t("bids.crew_story.platzhalter")}
                aria-label={t("bids.crew_story.titel")}
                onChange={(e) => aendern(e.target.value)}
              />
              <div className="crew-story__zeile">
                <span className="crew-story__frist">
                  {gesperrt ? t("bids.crew_story.gesperrt_hinweis") : t("bids.crew_story.frist")}
                </span>
                <span className="crew-story__zaehler">
                  {text.length}/{max}
                </span>
              </div>
              {serverText !== null && (
                <div className="crew-story__server" data-testid="crew-story-server">
                  <div className="crew-story__server-titel">{t("bids.crew_story.server_titel")}</div>
                  <div className="crew-story__server-text">{serverText || t("bids.crew_story.server_leer")}</div>
                </div>
              )}
              {pruefung && (
                <div className={`crew-story__pruefung crew-story__pruefung--${pruefung.status}`} role="status">
                  <div className="crew-story__pruefung-kopf">{kopf}</div>
                  {pruefung.status !== "uebernommen" && pruefung.gruende.length > 0 && (
                    <ul>
                      {pruefung.gruende.map((g) => (
                        <li key={g}>{g}</li>
                      ))}
                    </ul>
                  )}
                </div>
              )}
              {meldung && (
                <div
                  className={meldung.art === "ok" ? "crew-story__ok" : "manual-modal__error"}
                  role={meldung.art === "ok" ? "status" : "alert"}
                >
                  {meldungText(meldung.schluessel)}
                </div>
              )}
              {schliessenFragen && (
                <div className="manual-modal__warning" role="alert">
                  <div className="manual-modal__warning-text">{t("bids.crew_story.nicht_gespeichert_schliessen")}</div>
                </div>
              )}
            </>
          )}
        </div>

        <div className="manual-modal__actions">
          <button type="button" className="button" onClick={schliessen} disabled={speichert}>
            {schliessenFragen ? t("bids.crew_story.trotzdem_schliessen") : t("bids.crew_story.schliessen")}
          </button>
          {!ohneKern && zustand === "fehler" && (
            <button type="button" className="button button--primary" onClick={() => void laden()}>
              {t("bids.crew_story.erneut")}
            </button>
          )}
          {!ohneKern && zustand === "bereit" && !gesperrt && (
            <button
              type="button"
              className="button button--primary"
              onClick={() => void speichern()}
              disabled={speichert || text === gespeichert}
            >
              {speichert ? t("bids.crew_story.speichert") : t("bids.crew_story.speichern")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
