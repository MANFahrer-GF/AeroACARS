/**
 * Telemetrie-Monitor (v1.8) — der Tab „Telemetrie" und sein eigenes Fenster.
 *
 * Links alle Werte (durchsuchbar, nach Gruppe), rechts die Tafeln der
 * gewaehlten Ansicht, darunter Ereignisse und Watch-Panel. Reine Anzeige:
 * nichts hier beeinflusst Flug oder Bewertung.
 */

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, isTauri } from "../../lib/ipc";
import { Badge, Button } from "../ui";
import { csvText } from "./csv";
import { mitVorzeichen, wertMitEinheit, wertText } from "./format";
import { Anflugfenster, Envelope, Fahrwerk, Systeme, Tanks, Triebwerke } from "./Tafeln";
import { GRUPPEN, type Kanal } from "./typen";
import { useTelemetrie, type Datenquelle, type Telemetrie } from "./useTelemetrie";
import { ZeitDiagramm, type Band, type Hilfslinie, type Reihe } from "./ZeitDiagramm";
import "./telemetrie.css";

type Ansicht = "anflug" | "landung" | "triebwerke" | "aero" | "systeme";
const ANSICHTEN: Ansicht[] = ["anflug", "landung", "triebwerke", "aero", "systeme"];
const FENSTER = [60, 120, 300];

const SPEICHER_WATCH = "aeroacars.telemetrie.watch";
const SPEICHER_ANSICHT = "aeroacars.telemetrie.ansicht";

function lesen<T>(schluessel: string, standard: T): T {
  try {
    const roh = localStorage.getItem(schluessel);
    return roh ? (JSON.parse(roh) as T) : standard;
  } catch {
    return standard;
  }
}
function schreiben(schluessel: string, wert: unknown) {
  try {
    localStorage.setItem(schluessel, JSON.stringify(wert));
  } catch {
    /* privat/voll — dann eben nicht merken */
  }
}

const S1 = "--tele-s1";
const S2 = "--tele-s2";
const S3 = "--tele-s3";
const S4 = "--tele-s4";
const OK = "--sem-ok";
const WARN = "--sem-warn";
const ROT = "--sem-danger";
const MOTOR_FARBEN = [S1, S2, S3, S4];

interface Props {
  /** Im eigenen Fenster: kein „Eigenes Fenster"-Knopf. */
  imFenster?: boolean;
  /** Nur fuer Vorschau und Tests. */
  quelle?: Datenquelle;
}

export function TelemetrieView({ imFenster = false, quelle }: Props) {
  const { t } = useTranslation();
  const tm = useTelemetrie(quelle);
  const [fenster, setFenster] = useState(120);
  const [ansicht, setAnsichtState] = useState<Ansicht>(() => {
    const a = lesen<string>(SPEICHER_ANSICHT, "anflug");
    return (ANSICHTEN as string[]).includes(a) ? (a as Ansicht) : "anflug";
  });
  const [watch, setWatchState] = useState<string[]>(() => lesen<string[]>(SPEICHER_WATCH, ["vs", "g", "ias"]));
  const [csvMeldung, setCsvMeldung] = useState<string | null>(null);

  const setAnsicht = (a: Ansicht) => {
    setAnsichtState(a);
    schreiben(SPEICHER_ANSICHT, a);
  };
  const umschalten = (id: string) => {
    setWatchState((w) => {
      const neu = w.includes(id) ? w.filter((x) => x !== id) : [...w, id];
      schreiben(SPEICHER_WATCH, neu);
      return neu;
    });
  };

  if (tm.zustand === "fehler") {
    return (
      <div className="tele tele-leer">
        <p>{t("telemetrie.fehler", { fehler: tm.fehler ?? "" })}</p>
      </div>
    );
  }

  const ohneDaten = tm.zustand === "bereit" && tm.frames.length === 0;
  const alterMs = tm.letzter ? Date.now() - tm.letzter.t : Infinity;
  const live = !tm.angehalten && alterMs < 3000;

  const csv = async () => {
    setCsvMeldung(null);
    const ids = watch.length ? watch : (tm.katalog?.zahlen.map((k) => k.id) ?? []);
    const inhalt = csvText(tm, ids);
    try {
      const dateiname = `aeroacars-telemetrie-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-")}.csv`;
      const gespeichert = await invoke<boolean>("telemetrie_csv_speichern", { inhalt, dateiname });
      if (gespeichert) setCsvMeldung(t("telemetrie.csv_gespeichert"));
    } catch (e) {
      setCsvMeldung(t("telemetrie.csv_fehler", { fehler: e instanceof Error ? e.message : String(e) }));
    }
  };

  return (
    <div className="tele">
      <div className="tele-kopf">
        <h2 className="tele-titel">{t("telemetrie.titel")}</h2>
        <Badge tone={live ? "ok" : tm.angehalten ? "warn" : "neutral"}>
          {tm.angehalten ? t("telemetrie.angehalten") : live ? t("telemetrie.live") : t("telemetrie.wartet")}
        </Badge>
        <span className="tele-flugzeug">
          {[tm.text("muster"), tm.text("kennzeichen"), tm.text("phase_acars")].filter(Boolean).join(" · ") ||
            t("telemetrie.kein_flugzeug")}
        </span>
        <span className="tele-luecke" />
        <div className="tele-seg" role="group" aria-label={t("telemetrie.zeitfenster")}>
          {FENSTER.map((s) => (
            <button key={s} type="button" aria-pressed={fenster === s} onClick={() => setFenster(s)}>
              {s / 60} min
            </button>
          ))}
        </div>
        <Button size="sm" onClick={() => tm.setAngehalten(!tm.angehalten)}>
          {tm.angehalten ? t("telemetrie.weiter") : t("telemetrie.anhalten")}
        </Button>
        <Button size="sm" onClick={() => tm.markerSetzen(t("telemetrie.marker"))} disabled={!tm.letzter}>
          {t("telemetrie.marker_setzen")}
        </Button>
        {!imFenster && isTauri && (
          <Button size="sm" onClick={() => void invoke("telemetrie_fenster_oeffnen").catch(() => undefined)}>
            {t("telemetrie.eigenes_fenster")}
          </Button>
        )}
        {/* CSV speichert ueber den Dialog des Sim-PCs — auf dem Tablet
            (LAN-Bruecke) gibt es dafuer keinen Speicherort. */}
        {isTauri && (
          <Button size="sm" onClick={() => void csv()} disabled={!tm.letzter}>
            {t("telemetrie.csv")}
          </Button>
        )}
      </div>
      {csvMeldung && <div className="tele-meldung">{csvMeldung}</div>}

      <div className="tele-ansichten" role="tablist" aria-label={t("telemetrie.ansicht")}>
        {ANSICHTEN.map((a) => (
          <button
            key={a}
            type="button"
            role="tab"
            aria-selected={ansicht === a}
            className={ansicht === a ? "tele-ansicht tele-ansicht--aktiv" : "tele-ansicht"}
            onClick={() => setAnsicht(a)}
          >
            {t(`telemetrie.ansicht_${a}`)}
          </button>
        ))}
      </div>

      <div className="tele-koerper">
        <WerteListe tm={tm} watch={watch} umschalten={umschalten} />
        <div className="tele-arbeit">
          {tm.zustand === "laedt" && <p className="tele-hinweis">{t("telemetrie.laedt")}</p>}
          {ohneDaten && <p className="tele-hinweis">{t("telemetrie.kein_sim")}</p>}
          {tm.zustand === "bereit" && !ohneDaten && (
            <AnsichtInhalt tm={tm} ansicht={ansicht} fensterMs={fenster * 1000} />
          )}
          <Ereignisleiste tm={tm} />
          <WatchPanel tm={tm} watch={watch} umschalten={umschalten} fensterMs={fenster * 1000} />
        </div>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------- Werteliste */

function WerteListe({ tm, watch, umschalten }: { tm: Telemetrie; watch: string[]; umschalten: (id: string) => void }) {
  const { t, i18n } = useTranslation();
  const [suche, setSuche] = useState("");
  const [nurMitWert, setNurMitWert] = useState(true);
  const kanaele = useMemo(() => [...(tm.katalog?.zahlen ?? []), ...(tm.katalog?.texte ?? [])], [tm.katalog]);
  const q = suche.trim().toLowerCase();
  const passt = (k: Kanal) => {
    if (!q) return true;
    return (
      t(`telemetrie.kanal.${k.id}`).toLowerCase().includes(q) ||
      k.id.includes(q) ||
      t(`telemetrie.gruppe.${k.gruppe}`).toLowerCase().includes(q)
    );
  };
  const wertVon = (k: Kanal) => (k.art === "text" ? tm.text(k.id) : tm.wert(k.id));
  const sichtbar = kanaele.filter((k) => passt(k) && (!nurMitWert || wertVon(k) !== null));
  return (
    <aside className="tele-liste">
      <input
        id="tele-suche"
        className="tele-suche"
        type="search"
        placeholder={t("telemetrie.suche")}
        aria-label={t("telemetrie.suche")}
        value={suche}
        onChange={(e) => setSuche(e.target.value)}
      />
      <label className="tele-check">
        <input type="checkbox" checked={nurMitWert} onChange={(e) => setNurMitWert(e.target.checked)} />
        {t("telemetrie.nur_mit_wert")}
      </label>
      <div className="tele-zaehler">
        {t("telemetrie.zaehler", { sichtbar: sichtbar.length, gesamt: kanaele.length })}
      </div>
      <div className="tele-gruppen">
        {GRUPPEN.map((g) => {
          const zeilen = sichtbar.filter((k) => k.gruppe === g);
          if (zeilen.length === 0) return null;
          return (
            <div key={g}>
              <div className="tele-gruppe">{t(`telemetrie.gruppe.${g}`)}</div>
              {zeilen.map((k) => {
                const gewaehlt = watch.includes(k.id);
                return (
                  <button
                    key={k.id}
                    type="button"
                    className={gewaehlt ? "tele-zeile tele-zeile--gewaehlt" : "tele-zeile"}
                    onClick={() => umschalten(k.id)}
                    aria-pressed={gewaehlt}
                    title={t("telemetrie.zeile_hinweis")}
                  >
                    <span className="tele-zeile-name">{t(`telemetrie.kanal.${k.id}`)}</span>
                    <span className="tele-zeile-wert">{wertMitEinheit(k, wertVon(k), t, i18n.language)}</span>
                    <span className={`tele-quelle tele-quelle--${k.quelle}`}>{t(`telemetrie.quelle.${k.quelle}`)}</span>
                  </button>
                );
              })}
            </div>
          );
        })}
      </div>
    </aside>
  );
}

/* ------------------------------------------------------------ Ansichten */

interface DiagrammDef {
  titel: string;
  reihen: Array<{ id: string; farbe: string; gestrichelt?: boolean; flaeche?: boolean }>;
  min?: number;
  max?: number;
  stellen?: number;
  hilfslinien?: Hilfslinie[];
  baender?: Band[];
  /** Kopfwert rechts. */
  kopf?: string;
}

function AnsichtInhalt({ tm, ansicht, fensterMs }: { tm: Telemetrie; ansicht: Ansicht; fensterMs: number }) {
  const { t, i18n } = useTranslation();
  const n = motorAnzahl(tm);
  const motoren = Array.from({ length: n }, (_, i) => i + 1);

  const kopfwert = (id: string) => {
    const k = tm.kanal(id);
    return k ? wertMitEinheit(k, tm.wert(id), t, i18n.language) : "–";
  };
  const diagramm = (d: DiagrammDef, breit = false) => {
    const reihen: Reihe[] = d.reihen
      .filter((r) => tm.zahlIndex.has(r.id))
      .map((r) => ({ ...r, name: t(`telemetrie.kanal.${r.id}`) }));
    const hatDaten = reihen.some((r) => tm.wert(r.id) !== null);
    return (
      <section key={d.titel} className={breit ? "tele-karte tele-karte--breit" : "tele-karte"}>
        <header className="tele-karte-kopf">
          <span className="tele-karte-titel">{t(`telemetrie.diagramm.${d.titel}`)}</span>
          <span className="tele-karte-wert">{d.kopf ?? (reihen[0] ? kopfwert(reihen[0].id) : "")}</span>
        </header>
        {hatDaten ? (
          <>
            <ZeitDiagramm
              frames={tm.frames}
              index={tm.zahlIndex}
              reihen={reihen}
              fensterMs={fensterMs}
              min={d.min}
              max={d.max}
              stellen={d.stellen}
              hilfslinien={d.hilfslinien}
              baender={d.baender}
              version={tm.version}
              marker={tm.marker}
            />
            <div className="tele-legende">
              {reihen.map((r) => (
                <span key={r.id}>
                  <i style={{ background: `var(${r.farbe})` }} />
                  {r.name}
                </span>
              ))}
            </div>
          </>
        ) : (
          <p className="tele-hinweis">{t("telemetrie.keine_werte")}</p>
        )}
      </section>
    );
  };
  const tafel = (titel: string, inhalt: React.ReactNode, breit = false) => (
    <section key={titel} className={breit ? "tele-karte tele-karte--breit" : "tele-karte"}>
      <header className="tele-karte-kopf">
        <span className="tele-karte-titel">{t(`telemetrie.tafel.${titel}`)}</span>
      </header>
      {inhalt}
    </section>
  );

  const hoehe = diagramm({
    titel: "hoehe",
    reihen: [
      { id: "hoehe_agl", farbe: S1, flaeche: true },
      { id: "radarhoehe", farbe: S3, gestrichelt: true },
    ],
    min: 0,
  });
  const fahrt = diagramm({
    titel: "fahrt",
    reihen: [{ id: "ias", farbe: S1 }],
    hilfslinien: [
      { wert: "vapp", farbe: OK },
      { wert: "vls", farbe: WARN },
      { wert: "vref", farbe: OK },
      { wert: "vmo", farbe: ROT },
    ],
  });
  const sinkrate = diagramm({
    titel: "sinkrate",
    reihen: [{ id: "vs", farbe: S1 }],
    baender: [{ von: -6000, bis: -1000, farbe: ROT }],
    kopf: tm.wert("vs") === null ? "–" : `${mitVorzeichen(tm.wert("vs") ?? 0, 0, i18n.language)} fpm`,
  });
  const aoa = diagramm({
    titel: "aoa",
    reihen: [
      { id: "aoa", farbe: S3 },
      { id: "schiebewinkel", farbe: S2 },
    ],
    stellen: 1,
    hilfslinien: [{ wert: "aoa_abriss", farbe: ROT }],
  });
  const g3 = diagramm({
    titel: "g",
    reihen: [
      { id: "g", farbe: S1 },
      { id: "g_laengs", farbe: S2 },
      { id: "g_seitlich", farbe: S3 },
    ],
    stellen: 2,
  });
  const wind = diagramm({
    titel: "wind",
    reihen: [
      { id: "gegenwind", farbe: S1 },
      { id: "seitenwind", farbe: S2 },
    ],
  });
  const lage = diagramm({
    titel: "lage",
    reihen: [
      { id: "pitch", farbe: S1 },
      { id: "bank", farbe: S2 },
    ],
    stellen: 1,
  });
  const raten = diagramm({
    titel: "raten",
    reihen: [
      { id: "rate_rollen", farbe: S1 },
      { id: "rate_nicken", farbe: S2 },
      { id: "rate_gieren", farbe: S3 },
    ],
    stellen: 1,
  });
  const steuerung = diagramm({
    titel: "steuerung",
    reihen: [
      { id: "hoehenruder", farbe: S1 },
      { id: "querruder", farbe: S2 },
      { id: "seitenruder", farbe: S3 },
      { id: "trimmung", farbe: S4, gestrichelt: true },
    ],
    min: -100,
    max: 100,
  });
  const bremsen = diagramm({
    titel: "bremsen",
    reihen: [
      { id: "bremse_links", farbe: S1 },
      { id: "bremse_rechts", farbe: S2 },
      { id: "spoiler", farbe: S3, gestrichelt: true },
      { id: "umkehr_1", farbe: S4 },
    ],
    min: 0,
    max: 100,
  });
  const n1 = diagramm({
    titel: "n1",
    reihen: motoren.map((m) => ({ id: `n1_${m}`, farbe: MOTOR_FARBEN[m - 1] })),
    min: 0,
    max: 110,
  });
  const ff = diagramm({
    titel: "ff",
    reihen: motoren.map((m) => ({ id: `ff_${m}`, farbe: MOTOR_FARBEN[m - 1] })),
    min: 0,
  });
  const egt = diagramm({
    titel: "egt",
    reihen: motoren.map((m) => ({ id: `egt_${m}`, farbe: MOTOR_FARBEN[m - 1] })),
  });
  const sprit = diagramm({ titel: "sprit", reihen: [{ id: "sprit_gesamt", farbe: S1, flaeche: true }] });
  const energie = diagramm({ titel: "energie", reihen: [{ id: "energiehoehe", farbe: S1 }] });
  const kabine = diagramm({
    titel: "kabine",
    reihen: [
      { id: "kabinenhoehe", farbe: S1 },
    ],
  });

  const inhalt: Record<Ansicht, React.ReactNode[]> = {
    anflug: [hoehe, fahrt, sinkrate, tafel("anflug", <Anflugfenster tm={tm} />), aoa, wind],
    landung: [sinkrate, g3, tafel("fahrwerk", <Fahrwerk tm={tm} />), bremsen, lage, fahrt],
    triebwerke: [
      tafel("triebwerke", <Triebwerke tm={tm} />, true),
      n1,
      ff,
      egt,
      tafel("tanks", <Tanks tm={tm} />),
      sprit,
    ],
    aero: [tafel("envelope", <Envelope tm={tm} />), aoa, g3, raten, steuerung, energie],
    systeme: [tafel("systeme", <Systeme tm={tm} />, true), tafel("autopilot", <AutopilotTafel tm={tm} />), kabine],
  };
  return <div className="tele-raster">{inhalt[ansicht]}</div>;
}

function motorAnzahl(tm: Telemetrie): number {
  const gemeldet = tm.wert("triebwerke_anzahl");
  if (gemeldet !== null && gemeldet >= 1) return Math.min(4, Math.round(gemeldet));
  const mitN1 = [1, 2, 3, 4].filter((n) => tm.wert(`n1_${n}`) !== null).length;
  return mitN1 || 2;
}

function AutopilotTafel({ tm }: { tm: Telemetrie }) {
  const { t, i18n } = useTranslation();
  const ids = [
    "ap",
    "athr",
    "fma_schub",
    "fma_lateral",
    "fma_vertikal",
    "soll_fahrt",
    "soll_kurs",
    "soll_hoehe",
    "soll_vs",
    "v1",
    "vr",
    "v2",
    "vapp",
    "vls",
    "schubrast",
  ];
  return (
    <div className="tele-kv">
      {ids.map((id) => {
        const k = tm.kanal(id);
        if (!k) return null;
        const v = k.art === "text" ? tm.text(id) : tm.wert(id);
        return (
          <span key={id} className="tele-kv-paar">
            <span>{t(`telemetrie.kanal.${id}`)}</span>
            <span>{wertMitEinheit(k, v, t, i18n.language)}</span>
          </span>
        );
      })}
    </div>
  );
}

/* ------------------------------------------------------------ Ereignisse */

function uhrzeit(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}:${String(d.getUTCSeconds()).padStart(2, "0")}Z`;
}

function Ereignisleiste({ tm }: { tm: Telemetrie }) {
  const { t } = useTranslation();
  const liste = [
    ...tm.ereignisse.map((e) => ({
      t: e.t,
      text: t(`telemetrie.ereignis.${e.art}`, e.werte ?? {}),
      art: e.wichtig ? "wichtig" : "",
    })),
    ...tm.marker.map((m) => ({ t: m.t, text: m.text, art: "marker" })),
  ]
    .sort((a, b) => b.t - a.t)
    .slice(0, 40);
  return (
    <section className="tele-karte tele-karte--breit">
      <header className="tele-karte-kopf">
        <span className="tele-karte-titel">{t("telemetrie.ereignisse")}</span>
      </header>
      {liste.length === 0 ? (
        <p className="tele-hinweis">{t("telemetrie.keine_ereignisse")}</p>
      ) : (
        <div className="tele-ereignisse">
          {liste.map((e, i) => (
            <span key={`${e.t}-${i}`} className={`tele-ereignis tele-ereignis--${e.art || "normal"}`}>
              <b>{uhrzeit(e.t)}</b>
              {e.text}
            </span>
          ))}
        </div>
      )}
    </section>
  );
}

/* ------------------------------------------------------------ Watch-Panel */

function WatchPanel({
  tm,
  watch,
  umschalten,
  fensterMs,
}: {
  tm: Telemetrie;
  watch: string[];
  umschalten: (id: string) => void;
  fensterMs: number;
}) {
  const { t, i18n } = useTranslation();
  const statistik = useMemo(() => {
    const aus = new Map<string, { min: number; max: number; mittel: number } | null>();
    const letzter = tm.frames.length ? tm.frames[tm.frames.length - 1].t : 0;
    const ab = letzter - fensterMs;
    for (const id of watch) {
      const i = tm.zahlIndex.get(id);
      if (i === undefined) {
        aus.set(id, null);
        continue;
      }
      let min = Infinity;
      let max = -Infinity;
      let summe = 0;
      let n = 0;
      for (let j = tm.frames.length - 1; j >= 0 && tm.frames[j].t >= ab; j--) {
        const v = tm.frames[j].z[i];
        if (typeof v !== "number") continue;
        if (v < min) min = v;
        if (v > max) max = v;
        summe += v;
        n++;
      }
      aus.set(id, n ? { min, max, mittel: summe / n } : null);
    }
    return aus;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tm.version, watch, fensterMs, tm.zahlIndex]);

  return (
    <section className="tele-karte tele-karte--breit">
      <header className="tele-karte-kopf">
        <span className="tele-karte-titel">{t("telemetrie.watch")}</span>
        <span className="tele-karte-wert tele-hinweis">{t("telemetrie.watch_hinweis")}</span>
      </header>
      {watch.length === 0 ? (
        <p className="tele-hinweis">{t("telemetrie.watch_leer")}</p>
      ) : (
        <div className="tele-tabelle-rahmen">
          <table className="tele-tabelle">
            <thead>
              <tr>
                <th>{t("telemetrie.spalte.wert")}</th>
                <th>{t("telemetrie.spalte.quelle")}</th>
                <th>{t("telemetrie.spalte.aktuell")}</th>
                <th>{t("telemetrie.spalte.min")}</th>
                <th>{t("telemetrie.spalte.max")}</th>
                <th>{t("telemetrie.spalte.mittel")}</th>
                <th aria-label={t("telemetrie.entfernen")} />
              </tr>
            </thead>
            <tbody>
              {watch.map((id) => {
                const k = tm.kanal(id);
                if (!k) return null;
                const s = statistik.get(id);
                const v = k.art === "text" ? tm.text(id) : tm.wert(id);
                const z = (x: number | undefined) => (x === undefined ? "–" : wertText(k, x, t, i18n.language));
                return (
                  <tr key={id}>
                    <td>{t(`telemetrie.kanal.${id}`)}</td>
                    <td>
                      <span className={`tele-quelle tele-quelle--${k.quelle}`}>{t(`telemetrie.quelle.${k.quelle}`)}</span>
                    </td>
                    <td>{wertMitEinheit(k, v, t, i18n.language)}</td>
                    <td>{k.art === "zahl" ? z(s?.min) : "–"}</td>
                    <td>{k.art === "zahl" ? z(s?.max) : "–"}</td>
                    <td>{k.art === "zahl" ? z(s?.mittel) : "–"}</td>
                    <td>
                      <button type="button" className="tele-x" onClick={() => umschalten(id)} aria-label={t("telemetrie.entfernen")}>
                        ×
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

/** Eigenes Fenster: nur der Monitor, ohne Menue. */
export function TelemetrieFenster() {
  useEffect(() => {
    document.title = "AeroACARS — Telemetrie";
  }, []);
  return (
    <div className="tele-fenster">
      <TelemetrieView imFenster />
    </div>
  );
}
