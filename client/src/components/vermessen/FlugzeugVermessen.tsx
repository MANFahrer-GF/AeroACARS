// „Flugzeug vermessen" (28.09.2026) — geführte Schaltermessung.
//
// Thomas: „Wichtig ist, dass es gut durch das Programm geführt wird, genau
// erklärt wird, was zu machen ist." Deshalb je Schalter: was, wo, wie er
// heißt, und nach jeder Stellung eine Rückmeldung, ob sich etwas bewegt hat.
// Die Messung selbst läuft im Backend (`vermessung_*`), hier nur Führung.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../../lib/ipc";
import type { SimStatus } from "../../types";
import { SCHRITTE, type SchrittDef } from "./schritte";
import "./vermessen.css";

interface StartAntwort {
  sim: "msfs" | "xplane";
  flugzeug: { titel?: string | null; icao?: string | null; autor?: string | null };
  anzahl_werte: number;
  l_namen: number;
  /** Sitzungsnummer — geht bei jedem Befehl mit. */
  sitzung: number;
}
interface StellungAntwort {
  mitgegangen: number;
  erste: boolean;
}
interface SchrittAntwort {
  kandidaten: number;
  beispiele: Array<{ variable: string; werte: Array<number | null> }>;
}

type Ergebnis = { uebersprungen: boolean; kandidaten: number };

type Phase =
  | { art: "start" }
  | { art: "verbinden" }
  | { art: "ruhe"; laeuft: boolean; rauschen: number | null }
  | {
      art: "schritt";
      nr: number;
      begonnen: boolean;
      stellung: number;
      rueckmeldungen: Array<{ stellung: string; antwort: StellungAntwort | null }>;
      misst: boolean;
      abschluss: SchrittAntwort | null;
    }
  | { art: "fertig"; sendet: boolean }
  | { art: "gesendet"; id: string };

function fehlerText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}

export function FlugzeugVermessen() {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>({ art: "start" });
  const [start, setStart] = useState<StartAntwort | null>(null);
  const [ergebnisse, setErgebnisse] = useState<Record<string, Ergebnis>>({});
  const [fehler, setFehler] = useState<string | null>(null);
  const [sim, setSim] = useState<SimStatus | null>(null);
  // Laufnummer: Abbrechen/Verlassen zählt hoch, eine danach noch
  // eintreffende Antwort ändert die Ansicht nicht mehr.
  const lauf = useRef(0);
  // Ein Befehl läuft — Doppelklicks nicht zweimal senden.
  const belegt = useRef(false);
  // Sitzung im Backend (ab erfolgreichem Start).
  const sitzung = useRef<number | null>(null);
  // Kennung des eigenen Starts — damit trifft ein Abbruch mitten im
  // Verbinden nur diesen Start, nie einen gleichzeitig vom iPad gestarteten.
  const startNr = useRef<number | null>(null);
  const beendenArgs = () => ({
    ...(startNr.current === null ? {} : { start: startNr.current }),
    ...(sitzung.current === null ? {} : { sitzung: sitzung.current }),
  });

  // Simulatorstatus für den Startbildschirm.
  useEffect(() => {
    if (phase.art !== "start") return;
    let lebt = true;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const frage = async () => {
      try {
        const s = await invoke<SimStatus>("sim_status");
        if (lebt) setSim(s);
      } catch {
        if (lebt) setSim(null);
      }
      if (lebt) timer = setTimeout(() => void frage(), 2000);
    };
    void frage();
    return () => {
      lebt = false;
      if (timer) clearTimeout(timer);
    };
  }, [phase.art]);

  // Verlässt der Pilot die Seite, Messung sauber beenden — auch mitten im
  // Verbinden (der Start bricht dann im Backend selbst ab).
  useEffect(
    () => () => {
      lauf.current++;
      // Nie gestartet: nichts zu beenden.
      if (startNr.current !== null) void invoke("vermessung_beenden", beendenArgs()).catch(() => undefined);
    },
    [],
  );

  const beenden = useCallback(async () => {
    lauf.current++;
    belegt.current = false;
    const args = beendenArgs();
    sitzung.current = null;
    startNr.current = null;
    if (args.start !== undefined || args.sitzung !== undefined) {
      await invoke("vermessung_beenden", args).catch(() => undefined);
    }
  }, []);

  /** Einen Befehl ausführen; `null`, wenn inzwischen abgebrochen wurde. */
  const ausfuehren = async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T | null> => {
    const n = lauf.current;
    const mitNr = sitzung.current === null ? args : { ...args, sitzung: sitzung.current };
    const a = await invoke<T>(cmd, mitNr);
    return lauf.current === n ? a : null;
  };
  const fehlerWennAktuell = (n: number, e: unknown) => {
    if (lauf.current === n) setFehler(fehlerText(e));
  };

  const abbrechen = async () => {
    if (!window.confirm(t("vermessen.abbrechen_frage"))) return;
    await beenden();
    setErgebnisse({});
    setFehler(null);
    setPhase({ art: "start" });
  };

  const starten = async () => {
    setFehler(null);
    setPhase({ art: "verbinden" });
    const n = lauf.current;
    startNr.current = Math.floor(Math.random() * 2 ** 50);
    sitzung.current = null;
    try {
      const a = await ausfuehren<StartAntwort>("vermessung_starten", { start: startNr.current });
      if (!a) return;
      sitzung.current = a.sitzung;
      setStart(a);
      setErgebnisse({});
      setPhase({ art: "ruhe", laeuft: false, rauschen: null });
    } catch (e) {
      if (lauf.current !== n) return;
      setFehler(fehlerText(e));
      setPhase({ art: "start" });
    }
  };

  const ruhe = async () => {
    setPhase({ art: "ruhe", laeuft: true, rauschen: null });
    const n = lauf.current;
    try {
      const r = await ausfuehren<{ rauschen: number }>("vermessung_ruhe");
      if (!r) return;
      setPhase({ art: "ruhe", laeuft: false, rauschen: r.rauschen });
    } catch (e) {
      if (lauf.current !== n) return;
      setFehler(fehlerText(e));
      setPhase({ art: "ruhe", laeuft: false, rauschen: null });
    }
  };

  const schrittBeginnen = (nr: number) =>
    setPhase({ art: "schritt", nr, begonnen: false, stellung: 0, rueckmeldungen: [], misst: false, abschluss: null });

  const abschliessen = async (def: SchrittDef, uebersprungen: boolean, rueck: Array<{ stellung: string; antwort: StellungAntwort | null }>, nr: number) => {
    const n = lauf.current;
    try {
      const a = await ausfuehren<SchrittAntwort>("vermessung_schritt_abschliessen", {
        schalter: def.schalter,
        uebersprungen,
      });
      if (!a) return;
      setErgebnisse((e) => ({ ...e, [def.schalter]: { uebersprungen, kandidaten: a.kandidaten } }));
      if (uebersprungen) {
        weiterNach(nr);
      } else {
        setPhase({ art: "schritt", nr, begonnen: true, stellung: def.stellungen.length, rueckmeldungen: rueck, misst: false, abschluss: a });
      }
    } catch (e) {
      fehlerWennAktuell(n, e);
    }
  };

  const weiterNach = (nr: number) => {
    if (nr + 1 < SCHRITTE.length) schrittBeginnen(nr + 1);
    else setPhase({ art: "fertig", sendet: false });
  };

  const stellungMessen = async (vorhanden: boolean) => {
    if (phase.art !== "schritt" || belegt.current) return;
    belegt.current = true;
    try {
      await stellungMessenInnen(vorhanden);
    } finally {
      belegt.current = false;
    }
  };

  const stellungMessenInnen = async (vorhanden: boolean) => {
    if (phase.art !== "schritt") return;
    const n = lauf.current;
    const def = SCHRITTE[phase.nr]!;
    const name = def.stellungen[phase.stellung]!;
    let rueck = phase.rueckmeldungen;
    if (vorhanden) {
      setPhase({ ...phase, misst: true });
      try {
        const a = await ausfuehren<StellungAntwort>("vermessung_stellung", { stellung: t(`vermessen.stellung.${name}`) });
        if (!a) return;
        rueck = [...rueck, { stellung: name, antwort: a }];
      } catch (e) {
        if (lauf.current !== n) return;
        setFehler(fehlerText(e));
        setPhase({ ...phase, misst: false });
        return;
      }
    } else {
      rueck = [...rueck, { stellung: name, antwort: null }];
    }
    const naechste = phase.stellung + 1;
    if (naechste >= def.stellungen.length) {
      setPhase({ ...phase, misst: false, rueckmeldungen: rueck, stellung: naechste });
      await abschliessen(def, false, rueck, phase.nr);
    } else {
      setPhase({ ...phase, misst: false, rueckmeldungen: rueck, stellung: naechste });
    }
  };

  const ueberspringen = async () => {
    if (phase.art !== "schritt" || belegt.current) return;
    belegt.current = true;
    try {
      await abschliessen(SCHRITTE[phase.nr]!, true, [], phase.nr);
    } finally {
      belegt.current = false;
    }
  };

  const letzteStellung = async () => {
    if (phase.art !== "schritt" || belegt.current) return;
    belegt.current = true;
    try {
      const def = SCHRITTE[phase.nr]!;
      await abschliessen(def, false, phase.rueckmeldungen, phase.nr);
    } finally {
      belegt.current = false;
    }
  };

  const schrittNeu = async () => {
    if (phase.art !== "schritt") return;
    await invoke("vermessung_schritt_neu", sitzung.current === null ? undefined : { sitzung: sitzung.current }).catch(
      () => undefined,
    );
    setPhase({ ...phase, stellung: 0, rueckmeldungen: [], abschluss: null, begonnen: true });
  };

  const senden = async () => {
    setFehler(null);
    setPhase({ art: "fertig", sendet: true });
    const n = lauf.current;
    try {
      const a = await ausfuehren<{ id: string }>("vermessung_senden");
      if (!a) return;
      await beenden();
      setPhase({ art: "gesendet", id: a.id });
    } catch (e) {
      if (lauf.current !== n) return;
      setFehler(fehlerText(e));
      setPhase({ art: "fertig", sendet: false });
    }
  };

  const flugzeugName = start ? [start.flugzeug.titel, start.flugzeug.icao].filter(Boolean).join(" · ") : "";

  return (
    <div className="vm">
      {fehler && (
        <div className="vm-fehler" role="alert">
          {t("vermessen.fehler", { fehler })}
        </div>
      )}

      {phase.art === "start" && <StartSeite sim={sim} onStart={() => void starten()} />}

      {phase.art === "verbinden" && (
        <section className="vm-karte vm-mitte">
          <div className="vm-drehen" aria-hidden />
          <h3>{t("vermessen.verbinden_titel")}</h3>
          <p className="vm-dim">{t("vermessen.verbinden_text")}</p>
        </section>
      )}

      {phase.art === "ruhe" && start && (
        <section className="vm-karte">
          <Kopf flugzeug={flugzeugName} text={t("vermessen.verbunden", { anzahl: start.anzahl_werte })} />
          {start.sim === "msfs" && (
            <p className={start.l_namen > 0 ? "vm-hinweis" : "vm-hinweis vm-hinweis--warn"}>
              {start.l_namen > 0 ? t("vermessen.mit_scan", { anzahl: start.l_namen }) : t("vermessen.ohne_scan")}
            </p>
          )}
          <h3>{t("vermessen.ruhe_titel")}</h3>
          <p>{t("vermessen.ruhe_text")}</p>
          {phase.laeuft && <Fortschritt sekunden={8} text={t("vermessen.ruhe_laeuft")} />}
          {phase.rauschen !== null && <p className="vm-ok">✓ {t("vermessen.ruhe_fertig", { anzahl: phase.rauschen })}</p>}
          <div className="vm-knoepfe">
            {phase.rauschen === null ? (
              <button type="button" className="button button--primary" disabled={phase.laeuft} onClick={() => void ruhe()}>
                {t("vermessen.ruhe_start")}
              </button>
            ) : (
              <button type="button" className="button button--primary" onClick={() => schrittBeginnen(0)}>
                {t("vermessen.weiter")}
              </button>
            )}
            <button type="button" className="button vm-leise" onClick={() => void abbrechen()}>
              {t("vermessen.abbrechen")}
            </button>
          </div>
        </section>
      )}

      {phase.art === "schritt" && (
        <SchrittKarte
          phase={phase}
          ergebnisse={ergebnisse}
          onBeginnen={() => setPhase({ ...phase, begonnen: true })}
          onUeberspringen={() => void ueberspringen()}
          onErledigt={() => void stellungMessen(true)}
          onGibtEsNicht={() => void stellungMessen(false)}
          onLetzte={() => void letzteStellung()}
          onNeu={() => void schrittNeu()}
          onWeiter={() => weiterNach(phase.nr)}
          onAbbrechen={() => void abbrechen()}
        />
      )}

      {phase.art === "fertig" && (
        <section className="vm-karte">
          <Kopf flugzeug={flugzeugName} />
          <h3>{t("vermessen.fertig_titel")}</h3>
          <p>{t("vermessen.fertig_text")}</p>
          <ul className="vm-uebersicht">
            {SCHRITTE.map((s) => {
              const e = ergebnisse[s.schalter];
              return (
                <li key={s.schalter}>
                  <span>{t(`vermessen.schritt.${s.schalter}.titel`)}</span>
                  <span className={!e || e.uebersprungen ? "vm-dim" : e.kandidaten > 0 ? "vm-ok" : "vm-warn"}>
                    {!e || e.uebersprungen
                      ? t("vermessen.uebersprungen")
                      : e.kandidaten > 0
                        ? t("vermessen.werte", { anzahl: e.kandidaten })
                        : t("vermessen.keine_werte")}
                  </span>
                </li>
              );
            })}
          </ul>
          <div className="vm-knoepfe">
            <button type="button" className="button button--primary" disabled={phase.sendet} onClick={() => void senden()}>
              {phase.sendet ? t("vermessen.sendet") : t("vermessen.senden")}
            </button>
            <button type="button" className="button vm-leise" disabled={phase.sendet} onClick={() => void abbrechen()}>
              {t("vermessen.abbrechen")}
            </button>
          </div>
        </section>
      )}

      {phase.art === "gesendet" && (
        <section className="vm-karte vm-mitte">
          <div className="vm-haken" aria-hidden>
            ✓
          </div>
          <h3>{t("vermessen.gesendet_titel")}</h3>
          <p>{t("vermessen.gesendet_text")}</p>
          <button type="button" className="button" onClick={() => setPhase({ art: "start" })}>
            {t("vermessen.neue_messung")}
          </button>
        </section>
      )}
    </div>
  );
}

function StartSeite({ sim, onStart }: { sim: SimStatus | null; onStart: () => void }) {
  const { t } = useTranslation();
  const snap = sim?.snapshot ?? null;
  const verbunden = !!snap;
  const amBoden = snap?.on_ground !== false;
  const flugzeug = [snap?.aircraft_title, snap?.aircraft_icao].filter(Boolean).join(" · ");
  return (
    <section className="vm-karte">
      <h3 className="vm-titel">{t("vermessen.titel")}</h3>
      <p>{t("vermessen.einleitung")}</p>
      <p className="vm-dim">{t("vermessen.dauer")}</p>
      <div className="vm-liste">
        <div className="vm-liste-titel">{t("vermessen.vorher_titel")}</div>
        <ol>
          <li>{t("vermessen.vorher_sim")}</li>
          <li>{t("vermessen.vorher_boden")}</li>
          <li>{t("vermessen.vorher_strom")}</li>
          <li>{t("vermessen.vorher_pause")}</li>
        </ol>
      </div>
      <div className={`vm-status ${verbunden && amBoden ? "vm-status--ok" : "vm-status--warn"}`}>
        {!verbunden ? t("vermessen.sim_fehlt") : !amBoden ? t("vermessen.sim_luft") : t("vermessen.sim_ok", { flugzeug: flugzeug || "—" })}
      </div>
      <p className="vm-klein vm-dim">{t("vermessen.was_gesendet")}</p>
      <div className="vm-knoepfe">
        <button type="button" className="button button--primary" disabled={!verbunden || !amBoden} onClick={onStart}>
          {t("vermessen.starten")}
        </button>
      </div>
    </section>
  );
}

function Kopf({ flugzeug, text }: { flugzeug: string; text?: string }) {
  return (
    <div className="vm-kopf">
      <span className="vm-flugzeug">{flugzeug || "—"}</span>
      {text && <span className="vm-dim">{text}</span>}
    </div>
  );
}

function Fortschritt({ sekunden, text }: { sekunden: number; text: string }) {
  const [rest, setRest] = useState(sekunden);
  useEffect(() => {
    const iv = setInterval(() => setRest((r) => Math.max(0, r - 1)), 1000);
    return () => clearInterval(iv);
  }, []);
  return (
    <div className="vm-fortschritt" aria-live="polite">
      <div className="vm-balken">
        <div className="vm-balken-fuellung" style={{ animationDuration: `${sekunden}s` }} />
      </div>
      <span>
        {text}
        {rest > 0 ? `${rest} s` : ""}
      </span>
    </div>
  );
}

function SchrittKarte({
  phase,
  ergebnisse,
  onBeginnen,
  onUeberspringen,
  onErledigt,
  onGibtEsNicht,
  onLetzte,
  onNeu,
  onWeiter,
  onAbbrechen,
}: {
  phase: Extract<Phase, { art: "schritt" }>;
  ergebnisse: Record<string, Ergebnis>;
  onBeginnen: () => void;
  onUeberspringen: () => void;
  onErledigt: () => void;
  onGibtEsNicht: () => void;
  onLetzte: () => void;
  onNeu: () => void;
  onWeiter: () => void;
  onAbbrechen: () => void;
}) {
  const { t, i18n } = useTranslation();
  const def = SCHRITTE[phase.nr]!;
  const k = `vermessen.schritt.${def.schalter}`;
  const hinweis = i18n.exists(`${k}.hinweis`) ? t(`${k}.hinweis`) : null;
  const letzteRueck = phase.rueckmeldungen[phase.rueckmeldungen.length - 1];
  const nichtsBewegt = !!letzteRueck?.antwort && !letzteRueck.antwort.erste && letzteRueck.antwort.mitgegangen === 0;
  const fertig = phase.abschluss !== null;
  const gemessen = phase.rueckmeldungen.filter((r) => r.antwort).length;

  return (
    <section className="vm-karte">
      <div className="vm-schritte" aria-hidden>
        {SCHRITTE.map((s, i) => (
          <span
            key={s.schalter}
            className={`vm-punkt${i === phase.nr ? " vm-punkt--jetzt" : ""}${
              ergebnisse[s.schalter]?.uebersprungen
                ? " vm-punkt--weg"
                : ergebnisse[s.schalter]
                  ? ergebnisse[s.schalter]!.kandidaten > 0
                    ? " vm-punkt--fertig"
                    : " vm-punkt--leer"
                  : ""
            }`}
          />
        ))}
      </div>
      <div className="vm-dim vm-klein">{t("vermessen.schritt_von", { nr: phase.nr + 1, von: SCHRITTE.length })}</div>
      <h3 className="vm-titel">{t(`${k}.titel`)}</h3>

      <div className="vm-info">
        <div>
          <div className="vm-info-titel">{t("vermessen.wo")}</div>
          <div>{t(`${k}.wo`)}</div>
        </div>
        <div>
          <div className="vm-info-titel">{t("vermessen.heisst")}</div>
          <div className="vm-mono">{t(`${k}.namen`)}</div>
        </div>
      </div>
      {hinweis && <p className="vm-hinweis">{hinweis}</p>}

      {!phase.begonnen ? (
        <>
          <p className="vm-frage">{t("vermessen.hat_schalter")}</p>
          <div className="vm-knoepfe">
            <button type="button" className="button button--primary" onClick={onBeginnen}>
              {t("vermessen.ja_starten")}
            </button>
            <button type="button" className="button" onClick={onUeberspringen}>
              {t("vermessen.nein_ueberspringen")}
            </button>
          </div>
        </>
      ) : (
        <>
          <ol className="vm-stellungen">
            {def.stellungen.map((s, i) => {
              const r = phase.rueckmeldungen[i];
              const jetzt = !fertig && i === phase.stellung;
              const leer = !!r?.antwort && !r.antwort.erste && r.antwort.mitgegangen === 0;
              const zustand = r ? (r.antwort ? (leer ? "leer" : "fertig") : "fehlt") : jetzt ? "jetzt" : i < phase.stellung ? "fehlt" : "offen";
              if (fertig && !r) return null;
              return (
                <li key={`${s}-${i}`} className={`vm-stellung vm-stellung--${zustand}`}>
                  <span className="vm-stellung-marke" aria-hidden>
                    {zustand === "fertig" ? "✓" : zustand === "leer" ? "!" : zustand === "fehlt" ? "–" : i + 1}
                  </span>
                  <span className="vm-stellung-name">{t(`vermessen.stellung.${s}`)}</span>
                  <span className="vm-stellung-rueck">
                    {r?.antwort
                      ? r.antwort.erste
                        ? t("vermessen.ausgang")
                        : r.antwort.mitgegangen === 1
                          ? t("vermessen.bewegt_eins")
                          : r.antwort.mitgegangen > 0
                            ? t("vermessen.bewegt", { anzahl: r.antwort.mitgegangen })
                            : t("vermessen.keine_bewegung")
                      : r
                        ? t("vermessen.gibt_es_nicht")
                        : ""}
                  </span>
                </li>
              );
            })}
          </ol>

          {!fertig && (
            <div className="vm-aufgabe">
              <div className="vm-aufgabe-text">
                {t("vermessen.stelle_auf")} <strong>{t(`vermessen.stellung.${def.stellungen[phase.stellung]}`)}</strong>
              </div>
              <div className="vm-knoepfe">
                <button type="button" className="button button--primary" disabled={phase.misst} onClick={onErledigt}>
                  {phase.misst ? t("vermessen.messe") : t("vermessen.erledigt")}
                </button>
                {phase.stellung > 0 && (
                  <button type="button" className="button" disabled={phase.misst} onClick={onGibtEsNicht}>
                    {t("vermessen.gibt_es_nicht")}
                  </button>
                )}
                {def.offenesEnde && gemessen >= 2 && (
                  <button type="button" className="button" disabled={phase.misst} onClick={onLetzte}>
                    {t("vermessen.letzte_raste")}
                  </button>
                )}
              </div>
            </div>
          )}

          {nichtsBewegt && !fertig && (
            <div className="vm-hinweis vm-hinweis--warn">
              {t("vermessen.nichts_bewegt")}{" "}
              <button type="button" className="vm-link" onClick={onNeu}>
                {t("vermessen.neu_beginnen")}
              </button>
            </div>
          )}

          {fertig && phase.abschluss && (
            <div className={phase.abschluss.kandidaten > 0 ? "vm-ergebnis vm-ergebnis--ok" : "vm-ergebnis vm-ergebnis--warn"}>
              {phase.abschluss.kandidaten > 0
                ? t("vermessen.gefunden", { anzahl: phase.abschluss.kandidaten })
                : t("vermessen.nichts_gefunden")}
              {phase.abschluss.beispiele.length > 0 && (
                <ul className="vm-beispiele vm-mono">
                  {phase.abschluss.beispiele.map((b) => (
                    <li key={b.variable}>
                      {b.variable}: {b.werte.map((w) => (w == null ? "—" : String(Math.round(w * 1000) / 1000))).join(" / ")}
                    </li>
                  ))}
                </ul>
              )}
              <div className="vm-knoepfe">
                <button type="button" className="button button--primary" onClick={onWeiter}>
                  {phase.nr + 1 < SCHRITTE.length ? t("vermessen.naechster") : t("vermessen.zur_uebersicht")}
                </button>
                <button type="button" className="button" onClick={onNeu}>
                  {t("vermessen.neu_beginnen")}
                </button>
              </div>
            </div>
          )}
        </>
      )}

      <div className="vm-fuss">
        <button type="button" className="vm-link vm-dim" onClick={onAbbrechen}>
          {t("vermessen.abbrechen")}
        </button>
      </div>
    </section>
  );
}
