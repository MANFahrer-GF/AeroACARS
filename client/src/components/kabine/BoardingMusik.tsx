// Boarding-Musik (2.1.1 G, Design von Thomas freigegeben am 19.10.2026):
// alles auf der Kabinen-Seite, nichts in den Einstellungen.
// - MusikRegler: zweite Zeile der Ton-Leiste (eigener Pegel, ab Werk 20 %)
// - MusikKasten: Zustand, Quelle, Probehören, Schalter, „Eigene Musik …“
// - MusikDialog: eigene Musik je Airline hinzufügen, anhören, entfernen
import { useEffect, useMemo, useState } from "react";
import { formatIpcError } from "../../lib/ipc";
import { useConfirm } from "../ConfirmDialog";
import { Modal } from "../ui";
import {
  ALLE_AIRLINES,
  airlinesLesen,
  boardingMusikSetzen,
  laenge,
  musikAblage,
  musikDateiWaehlen,
  musikEntfernen,
  musikHinzufuegen,
  musikLautstaerkeSetzen,
  musikOrdnerOeffnen,
  musikProbe,
  musikProbeFlug,
  musikProbeStopp,
  musikPruefen,
  type AirlineEintrag,
  type MusikAblage,
  type MusikZustand,
} from "../../lib/kabinenansagen";

type T = (k: string, o?: Record<string, unknown>) => string;

/** Kennung + Name für die Anzeige: „Lufthansa (DLH)“ bzw. „Alle Airlines“. */
function airlineName(t: T, icao: string, namen: Map<string, string>): string {
  if (icao === ALLE_AIRLINES) return t("kabine.musik.alle");
  const n = namen.get(icao);
  return n ? `${n} (${icao})` : icao;
}

function NotenSymbol() {
  return (
    <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M9 18V5l12-2v13" />
      <circle cx="6" cy="18" r="3" fill="currentColor" stroke="none" />
      <circle cx="18" cy="16" r="3" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** Zweite Zeile der Ton-Leiste: Noten · „Boarding-Musik“ · Regler · %. */
export function MusikRegler({ lautstaerke, t }: { lautstaerke: number; t: T }) {
  const [wert, setWert] = useState(Math.round(lautstaerke * 100));
  const [zieht, setZieht] = useState(false);
  useEffect(() => {
    if (!zieht) setWert(Math.round(lautstaerke * 100));
  }, [lautstaerke, zieht]);
  const loslassen = () => {
    setZieht(false);
    void musikLautstaerkeSetzen(wert / 100, true);
  };
  return (
    <div className="kabine-ton-zeile" data-testid="kabine-musik-regler">
      <span className="kabine-ton-name">
        <NotenSymbol />
        <span>{t("kabine.musik.regler")}</span>
      </span>
      <input
        type="range"
        min={0}
        max={100}
        step={5}
        value={wert}
        aria-label={t("kabine.musik.lautstaerke")}
        onChange={(ev) => {
          const v = Number(ev.target.value);
          setZieht(true);
          setWert(v);
          void musikLautstaerkeSetzen(v / 100, false);
        }}
        onMouseUp={loslassen}
        onTouchEnd={loslassen}
        onKeyUp={loslassen}
      />
      <span className="kabine-ton-wert">{wert} %</span>
    </div>
  );
}

/** Kasten „Boarding-Musik“ unter der Status-Zeile (Design §2). */
export function MusikKasten({ zustand, t, onNeu }: { zustand: MusikZustand; t: T; onNeu: () => void }) {
  const [dialog, setDialog] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);
  const [namen, setNamen] = useState<Map<string, string>>(new Map());
  useEffect(() => {
    void airlinesLesen()
      .then((l) => setNamen(new Map(l.map((a) => [a.icao, a.name]))))
      .catch(() => undefined);
  }, []);
  const z = zustand;
  const probe = z.stand === "probe";
  const quelle =
    z.quelle === "lied"
      ? [t("kabine.musik.quelle_lied"), z.strecke, "3:00"].filter(Boolean).join(" · ")
      : z.quelle === "eigene" && z.ordner
        ? [t("kabine.musik.quelle_eigene"), airlineName(t, z.ordner, namen), t("kabine.musik.stuecke", { count: z.stuecke })].join(" · ")
        : z.stand === "keine"
          ? t("kabine.musik.quelle_keine")
          : z.stand === "ohne_flug"
            ? t("kabine.musik.quelle_ohne_flug")
            : null;
  const hoeren = () => {
    setFehler(null);
    const p = probe ? musikProbeStopp() : musikProbeFlug();
    void p.then(onNeu).catch((e) => setFehler(formatIpcError(e)));
  };
  return (
    <section className={`kabine-musik${z.an ? "" : " ist-aus"}`} data-testid="kabine-musik">
      <div className="kabine-musik-kopf">
        <div className="kabine-musik-titel">
          <strong>
            {t("kabine.musik.titel")}
            {z.titel ? ` · ${z.titel}` : ""}
          </strong>
          {quelle && <span className="kabine-musik-quelle">{quelle}</span>}
        </div>
        <span className={`kabine-plakette ist-${z.stand}`} data-testid="kabine-musik-stand">
          {t(`kabine.musik.stand.${z.stand}`)}
        </span>
      </div>
      <div className="kabine-musik-knoepfe">
        <button type="button" className="button button--ghost kabine-knopf" onClick={hoeren} disabled={!probe && !(z.quelle && ["bereit", "laeuft", "pausiert", "beendet"].includes(z.stand))}>
          {probe ? t("kabine.musik.stopp") : t("kabine.musik.probehoeren")}
        </button>
        <button type="button" className="button button--ghost kabine-knopf" onClick={() => setDialog(true)}>
          {t("kabine.musik.eigene")}
        </button>
        <label className="kabine-musik-schalter">
          <input
            type="checkbox"
            checked={z.an}
            onChange={(e) => {
              void boardingMusikSetzen(e.target.checked).then(onNeu).catch((err) => setFehler(formatIpcError(err)));
            }}
          />
          <span>{t("kabine.musik.schalter")}</span>
        </label>
      </div>
      <p className="kabine-musik-erklaerung">{t("kabine.musik.erklaerung")}</p>
      {fehler && (
        <p className="cpdlc-panel__error" role="status">
          {fehler}
        </p>
      )}
      <MusikDialog open={dialog} onClose={() => { setDialog(false); onNeu(); }} t={t} namen={namen} />
    </section>
  );
}

/** Dialog „Eigene Boarding-Musik“ (Design §3–§6). */
export function MusikDialog({ open, onClose, t, namen }: { open: boolean; onClose: () => void; t: T; namen: Map<string, string> }) {
  const { confirm, dialog: frage } = useConfirm();
  const [ablage, setAblage] = useState<MusikAblage | null>(null);
  const [airlines, setAirlines] = useState<AirlineEintrag[]>([]);
  const [auf, setAuf] = useState<Set<string>>(new Set());
  const [suche, setSuche] = useState("");
  const [datei, setDatei] = useState<string | null>(null);
  const [dateiLaenge, setDateiLaenge] = useState<number | null>(null);
  const [fuer, setFuer] = useState("");
  const [fuerSuche, setFuerSuche] = useState("");
  const [fehler, setFehler] = useState<string | null>(null);
  const [laeuft, setLaeuft] = useState(false);
  const [probe, setProbe] = useState<string | null>(null);

  const neu = () => void musikAblage().then(setAblage).catch((e) => setFehler(formatIpcError(e)));
  useEffect(() => {
    if (!open) return;
    setFehler(null);
    setDatei(null);
    setDateiLaenge(null);
    setAblage(null);
    setFuer("");
    neu();
    void airlinesLesen().then(setAirlines).catch(() => undefined);
  }, [open]);
  useEffect(() => {
    // Vorgewählt: die Airline des Flugs, ohne Flug „Alle Airlines“; ihre
    // Gruppe ist offen. Erst, wenn die Ablage gelesen ist.
    if (!ablage) return;
    const flug = ablage.flug_airline;
    setFuer((f) => f || flug || ALLE_AIRLINES);
    if (flug) setAuf((a) => new Set(a).add(flug));
  }, [ablage]);

  const meldung = (e: unknown) => {
    const code = (e as { code?: string })?.code;
    return code && ["musik_kaputt", "musik_zu_gross", "musik_format", "musik_ordner"].includes(code)
      ? t(`kabine.musik.dialog.fehler.${code}`, { detail: (e as { message?: string }).message ?? "" })
      : formatIpcError(e);
  };

  // Nur Airlines mit Musik, dazu immer die Airline des Flugs (Design §3).
  const gruppen = useMemo(() => {
    const m = new Map<string, MusikAblage["stuecke"]>();
    const flug = ablage?.flug_airline;
    if (flug) m.set(flug, []);
    for (const s of ablage?.stuecke ?? []) m.set(s.airline, [...(m.get(s.airline) ?? []), s]);
    const reihe = [...m.entries()].sort(([a], [b]) => (a === flug ? -1 : b === flug ? 1 : a === ALLE_AIRLINES ? -1 : b === ALLE_AIRLINES ? 1 : a.localeCompare(b)));
    const q = suche.trim().toLowerCase();
    return q ? reihe.filter(([icao]) => icao.toLowerCase().includes(q) || (namen.get(icao) ?? "").toLowerCase().includes(q)) : reihe;
  }, [ablage, suche, namen]);
  const mitMusik = new Set((ablage?.stuecke ?? []).map((s) => s.airline)).size;

  const vorschlaege = useMemo(() => {
    const q = fuerSuche.trim().toLowerCase();
    if (!q) return [];
    return airlines.filter((a) => a.icao.toLowerCase().includes(q) || a.name.toLowerCase().includes(q)).slice(0, 8);
  }, [fuerSuche, airlines]);

  const waehlen = async () => {
    setFehler(null);
    try {
      const p = await musikDateiWaehlen(t("kabine.musik.dialog.datei_waehlen"));
      if (!p) return;
      setLaeuft(true);
      const l = await musikPruefen(p);
      setDatei(p);
      setDateiLaenge(l);
    } catch (e) {
      setDatei(null);
      setFehler(meldung(e));
    } finally {
      setLaeuft(false);
    }
  };
  const hinzufuegen = async () => {
    if (!datei || !fuer) return;
    setFehler(null);
    setLaeuft(true);
    try {
      await musikHinzufuegen(datei, fuer);
      setDatei(null);
      setDateiLaenge(null);
      setAuf((a) => new Set(a).add(fuer));
      neu();
    } catch (e) {
      setFehler(meldung(e));
    } finally {
      setLaeuft(false);
    }
  };
  const entfernen = async (airline: string, d: string) => {
    if (!(await confirm({ message: t("kabine.musik.dialog.entfernen_frage"), confirmLabel: t("kabine.musik.dialog.entfernen"), destructive: true }))) return;
    void musikEntfernen(airline, d).then(neu).catch((e) => setFehler(meldung(e)));
  };
  const anhoeren = (airline: string, d: string) => {
    const k = `${airline}/${d}`;
    if (probe === k) {
      void musikProbeStopp().finally(() => setProbe(null));
    } else {
      void musikProbe(airline, d).then(() => setProbe(k)).catch((e) => setFehler(meldung(e)));
    }
  };
  const schliessen = () => {
    if (probe) void musikProbeStopp();
    setProbe(null);
    onClose();
  };
  const dateiName = datei ? datei.split(/[\\/]/).pop() : null;
  const flug = ablage?.flug_airline ?? null;

  return (
    <>
      <Modal open={open} onClose={schliessen} size="md" title={t("kabine.musik.dialog.titel")} closeLabel={t("kabine.musik.dialog.schliessen")}>
        <div className="kabine-musik-dialog" data-testid="kabine-musik-dialog">
          <p className="kabine-musik-unter">{t("kabine.musik.dialog.unter")}</p>
          {mitMusik >= 8 && (
            <input type="search" className="kabine-musik-suche" placeholder={t("kabine.musik.dialog.suchen")} value={suche} onChange={(e) => setSuche(e.target.value)} />
          )}
          <div className="kabine-musik-gruppen kabine-liste--rollbar">
            {gruppen.length === 0 && <div className="kabine-leer">{t("kabine.musik.dialog.keine")}</div>}
            {gruppen.map(([icao, stuecke]) => {
              const offen = auf.has(icao);
              return (
                <div key={icao} className="kabine-musik-gruppe">
                  <button
                    type="button"
                    className="kabine-musik-gruppe-kopf"
                    aria-expanded={offen}
                    onClick={() => setAuf((a) => { const n = new Set(a); if (n.has(icao)) n.delete(icao); else n.add(icao); return n; })}
                  >
                    <span>{offen ? "▾" : "▸"} {airlineName(t, icao, namen)}</span>
                    <span className="kabine-musik-zahl">{t("kabine.musik.stuecke", { count: stuecke.length })}</span>
                  </button>
                  {offen &&
                    (stuecke.length === 0 ? (
                      <div className="kabine-musik-leer">{t("kabine.musik.dialog.keine")}</div>
                    ) : (
                      <ul className="kabine-musik-stuecke">
                        {stuecke.map((s) => (
                          <li key={s.datei}>
                            <span className="kabine-musik-datei" title={s.datei}>{s.datei}</span>
                            <span className="kabine-musik-laenge">{laenge(s.dauer_s)}</span>
                            <button type="button" className="button button--ghost kabine-knopf" onClick={() => anhoeren(icao, s.datei)}>
                              {probe === `${icao}/${s.datei}` ? t("kabine.musik.stopp") : t("kabine.musik.probehoeren")}
                            </button>
                            <button type="button" className="button button--ghost kabine-knopf" onClick={() => void entfernen(icao, s.datei)}>
                              {t("kabine.musik.dialog.entfernen")}
                            </button>
                          </li>
                        ))}
                      </ul>
                    ))}
                </div>
              );
            })}
          </div>

          <fieldset className="kabine-musik-neu">
            <legend>{t("kabine.musik.dialog.hinzufuegen_titel")}</legend>
            <div className="kabine-musik-neu-zeile">
              <button type="button" className="button button--ghost kabine-knopf" onClick={() => void waehlen()} disabled={laeuft}>
                {t("kabine.musik.dialog.datei_waehlen")}
              </button>
              <span className="kabine-musik-datei">{dateiName ? `${dateiName} · ${laenge(dateiLaenge)}` : t("kabine.musik.dialog.keine_datei")}</span>
            </div>
            {dateiLaenge !== null && dateiLaenge < 30 && <p className="kabine-musik-hinweis">{t("kabine.musik.dialog.kurz")}</p>}
            <div className="kabine-musik-fuer" role="radiogroup" aria-label={t("kabine.musik.dialog.fuer")}>
              <span>{t("kabine.musik.dialog.fuer")}</span>
              <div className="kabine-musik-fuer-wahl">
                {flug && (
                  <label>
                    <input type="radio" name="fuer" checked={fuer === flug} onChange={() => setFuer(flug)} />
                    {t("kabine.musik.dialog.diese", { name: airlineName(t, flug, namen) })}
                  </label>
                )}
                <label>
                  <input type="radio" name="fuer" checked={fuer === ALLE_AIRLINES} onChange={() => setFuer(ALLE_AIRLINES)} />
                  {t("kabine.musik.alle")}
                </label>
                <label>
                  <input type="radio" name="fuer" checked={fuer !== flug && fuer !== ALLE_AIRLINES} onChange={() => setFuer(fuerSuche.toUpperCase() || "")} />
                  {t("kabine.musik.dialog.andere")}
                </label>
              </div>
              {fuer !== flug && fuer !== ALLE_AIRLINES && (
                <div className="kabine-musik-andere">
                  <input
                    type="search"
                    value={fuerSuche}
                    placeholder={t("kabine.musik.dialog.suchen")}
                    onChange={(e) => {
                      setFuerSuche(e.target.value);
                      const v = e.target.value.trim().toUpperCase();
                      setFuer(/^[A-Z0-9]{2,4}$/.test(v) ? v : "");
                    }}
                  />
                  {vorschlaege.length > 0 && (
                    <ul className="kabine-musik-vorschlaege">
                      {vorschlaege.map((a) => (
                        <li key={a.icao}>
                          <button type="button" onClick={() => { setFuer(a.icao); setFuerSuche(`${a.name} (${a.icao})`); }}>
                            {a.name} ({a.icao})
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              )}
            </div>
            <button type="button" className="button button--primary kabine-knopf" onClick={() => void hinzufuegen()} disabled={!datei || !fuer || laeuft}>
              {t("kabine.musik.dialog.hinzufuegen")}
            </button>
          </fieldset>
          {fehler && (
            <p className="cpdlc-panel__error" role="status">
              {fehler}
            </p>
          )}
          <div className="kabine-musik-fuss">
            <span>{t("kabine.musik.dialog.hinweis_recht")}</span>
            <button type="button" className="button button--ghost kabine-knopf" onClick={() => void musikOrdnerOeffnen().catch((e) => setFehler(meldung(e)))}>
              {t("kabine.musik.dialog.ordner")}
            </button>
          </div>
        </div>
      </Modal>
      {frage}
    </>
  );
}
