/**
 * Besondere Anzeigen des Telemetrie-Monitors, die kein Zeitdiagramm sind:
 * Anflugfenster, Flight Envelope, Fahrwerk, Triebwerke, Tanks, Systeme.
 *
 * Alle als SVG mit CSS-Variablen, damit helles und dunkles Design ohne
 * eigene Farbtabellen stimmen. Fehlt ein Wert, steht „–" — nie eine
 * erfundene Zahl.
 */

import { useTranslation } from "react-i18next";
import { mitVorzeichen, wertMitEinheit, zahl } from "./format";
import type { Telemetrie } from "./useTelemetrie";

type P = { tm: Telemetrie };

function fehlt(v: number | null): v is null {
  return v === null;
}

/* ------------------------------------------------------------ Anflugfenster */

export function Anflugfenster({ tm }: P) {
  const { t, i18n } = useTranslation();
  const loc = tm.wert("loc_ablage");
  const gs = tm.wert("gs_ablage");
  const hatLoc = (tm.wert("loc_empfang") ?? 0) >= 0.5;
  const hatGs = (tm.wert("gs_empfang") ?? 0) >= 0.5;
  // Spur der letzten Sekunden.
  const spur: Array<[number, number]> = [];
  const il = tm.zahlIndex.get("loc_ablage");
  const ig = tm.zahlIndex.get("gs_ablage");
  if (il !== undefined && ig !== undefined) {
    for (const f of tm.frames.slice(-60)) {
      const a = f.z[il];
      const b = f.z[ig];
      if (typeof a === "number" && typeof b === "number") spur.push([a, b]);
    }
  }
  const S = 60; // Pixel je Dot
  const c = (v: number) => Math.max(-2.4, Math.min(2.4, v)) * S;
  return (
    <div className="tele-tafel">
      <svg viewBox="-170 -150 340 300" className="tele-svg" role="img" aria-label={t("telemetrie.tafel.anflug")}>
        <rect x={-2 * S} y={-2 * S} width={4 * S} height={4 * S} className="tele-rahmen" />
        <rect x={-S} y={-S} width={2 * S} height={2 * S} className="tele-rahmen-innen" />
        <line x1={-2.4 * S} x2={2.4 * S} y1={0} y2={0} className="tele-achse" />
        <line y1={-2.4 * S} y2={2.4 * S} x1={0} x2={0} className="tele-achse" />
        {[-2, -1, 1, 2].map((d) => (
          <text key={d} x={d * S} y={2 * S + 16} className="tele-skala" textAnchor="middle">
            {d}
          </text>
        ))}
        <text x={2.4 * S + 4} y={-6} className="tele-skala">LOC</text>
        <text x={6} y={-2.4 * S + 4} className="tele-skala">G/S</text>
        {spur.map(([a, b], i) => (
          <circle key={i} cx={c(a)} cy={c(b)} r={2} className="tele-spur" opacity={(i / spur.length) * 0.5} />
        ))}
        {!fehlt(loc) && !fehlt(gs) && hatLoc && (
          <circle cx={c(loc)} cy={hatGs ? c(gs) : 0} r={6} className="tele-punkt" />
        )}
      </svg>
      <div className="tele-tafel-fuss">
        <span>
          LOC {hatLoc && loc !== null ? mitVorzeichen(loc, 2, i18n.language) : "–"}
        </span>
        <span>
          G/S {hatGs && gs !== null ? mitVorzeichen(gs, 2, i18n.language) : "–"}
        </span>
        {!hatLoc && <span className="tele-hinweis">{t("telemetrie.kein_ils")}</span>}
      </div>
    </div>
  );
}

/* ---------------------------------------------------------- Flight Envelope */

export function Envelope({ tm }: P) {
  const { t, i18n } = useTranslation();
  const ias = tm.wert("ias");
  const g = tm.wert("g");
  const vs1 = tm.wert("vs1") ?? tm.wert("vls");
  const vfe = tm.wert("vfe");
  const vmo = tm.wert("vmo");
  const bisV = Math.max(200, (vmo ?? 0) + 30, (ias ?? 0) + 30);
  const W = 320;
  const H = 180;
  const L = 36;
  const B = 22;
  const x = (v: number) => L + (Math.max(0, Math.min(bisV, v)) / bisV) * (W - L - 8);
  const y = (v: number) => 8 + ((3 - Math.max(-1.5, Math.min(3, v))) / 4.5) * (H - B - 8);
  const spur: Array<[number, number]> = [];
  const ii = tm.zahlIndex.get("ias");
  const ig = tm.zahlIndex.get("g");
  if (ii !== undefined && ig !== undefined) {
    for (const f of tm.frames.slice(-80)) {
      const a = f.z[ii];
      const b = f.z[ig];
      if (typeof a === "number" && typeof b === "number") spur.push([a, b]);
    }
  }
  const grenze = (v: number | null, name: string, klasse: string) =>
    v === null ? null : (
      <g key={name}>
        <line x1={x(v)} x2={x(v)} y1={8} y2={H - B} className={klasse} />
        <text x={x(v)} y={H - 6} textAnchor="middle" className="tele-skala">
          {name}
        </text>
      </g>
    );
  return (
    <div className="tele-tafel">
      <svg viewBox={`0 0 ${W} ${H}`} className="tele-svg" role="img" aria-label={t("telemetrie.tafel.envelope")}>
        {[-1, 0, 1, 2, 2.5].map((v) => (
          <g key={v}>
            <line x1={L} x2={W - 8} y1={y(v)} y2={y(v)} className={v === 2.5 || v === -1 ? "tele-grenze" : "tele-gitter"} />
            <text x={L - 4} y={y(v) + 4} textAnchor="end" className="tele-skala">
              {zahl(v, v % 1 ? 1 : 0, i18n.language)}
            </text>
          </g>
        ))}
        {grenze(vs1, "VS", "tele-warn")}
        {grenze(vfe, "VFE", "tele-warn")}
        {grenze(vmo, "VMO", "tele-grenze")}
        <polyline
          points={spur.map(([a, b]) => `${x(a)},${y(b)}`).join(" ")}
          className="tele-spurlinie"
        />
        {ias !== null && g !== null && <circle cx={x(ias)} cy={y(g)} r={5} className="tele-punkt" />}
      </svg>
      <div className="tele-tafel-fuss">
        <span>{ias === null ? "–" : `${zahl(ias, 0, i18n.language)} kt`}</span>
        <span>{g === null ? "–" : `${zahl(g, 2, i18n.language)} g`}</span>
        {vmo === null && vs1 === null && <span className="tele-hinweis">{t("telemetrie.keine_grenzen")}</span>}
      </div>
    </div>
  );
}

/* ---------------------------------------------------------------- Fahrwerk */

export function Fahrwerk({ tm }: P) {
  const { t, i18n } = useTranslation();
  const raeder: Array<[string, string, number, number]> = [
    ["boden_bug", "einfederung_bug", 60, 26],
    ["boden_links", "einfederung_links", 28, 96],
    ["boden_rechts", "einfederung_rechts", 92, 96],
  ];
  const bl = tm.wert("bremse_links");
  const br = tm.wert("bremse_rechts");
  const bremse = (name: string, v: number | null, yy: number) => (
    <g key={name}>
      <text x={150} y={yy - 6} className="tele-skala">
        {name}
      </text>
      <text x={310} y={yy - 6} textAnchor="end" className="tele-wert">
        {v === null ? "–" : `${zahl(v, 0, i18n.language)} %`}
      </text>
      <rect x={150} y={yy} width={160} height={8} className="tele-balken-grund" />
      {v !== null && <rect x={150} y={yy} width={(160 * Math.max(0, Math.min(100, v))) / 100} height={8} className="tele-balken" />}
    </g>
  );
  return (
    <div className="tele-tafel">
      <svg viewBox="0 0 320 150" className="tele-svg" role="img" aria-label={t("telemetrie.tafel.fahrwerk")}>
        <line x1={60} x2={60} y1={14} y2={128} className="tele-rumpf" />
        <line x1={14} x2={106} y1={80} y2={80} className="tele-rumpf" />
        {raeder.map(([b, e, cx, cy]) => {
          const an = tm.wert(b);
          const ein = tm.wert(e);
          return (
            <g key={b}>
              <rect
                x={cx - 9}
                y={cy - 13}
                width={18}
                height={26}
                rx={4}
                className={an === null ? "tele-rad-unbekannt" : an >= 0.5 ? "tele-rad-an" : "tele-rad"}
              />
              {ein !== null && (
                <text x={cx} y={cy + 26} textAnchor="middle" className="tele-skala">
                  {zahl(ein, 0, i18n.language)} %
                </text>
              )}
            </g>
          );
        })}
        {bremse(t("telemetrie.kanal.bremse_links"), bl, 40)}
        {bremse(t("telemetrie.kanal.bremse_rechts"), br, 76)}
        <text x={150} y={116} className="tele-skala">
          {t("telemetrie.kanal.autobrake")}: {tm.text("autobrake") ?? "–"}
        </text>
        <text x={150} y={134} className="tele-skala">
          {t("telemetrie.kanal.spoiler")}: {wertMitEinheit(tm.kanal("spoiler") ?? FALLBACK, tm.wert("spoiler"), t, i18n.language)}
        </text>
      </svg>
    </div>
  );
}

const FALLBACK = { id: "", gruppe: "flug", einheit: "", stellen: 0, quelle: "sim", art: "zahl" } as const;

/* -------------------------------------------------------------- Triebwerke */

function Bogen({ wert, max, rot, beschriftung }: { wert: number | null; max: number; rot: number; beschriftung: string }) {
  const { i18n } = useTranslation();
  const r = 40;
  const a0 = Math.PI * 0.8;
  const a1 = Math.PI * 2.2;
  const winkel = (v: number) => a0 + (a1 - a0) * Math.max(0, Math.min(1, v / max));
  const punkt = (a: number) => [50 + r * Math.cos(a), 52 + r * Math.sin(a)];
  const bogen = (von: number, bis: number) => {
    const [x1, y1] = punkt(von);
    const [x2, y2] = punkt(bis);
    const gross = bis - von > Math.PI ? 1 : 0;
    return `M ${x1} ${y1} A ${r} ${r} 0 ${gross} 1 ${x2} ${y2}`;
  };
  return (
    <svg viewBox="0 0 100 90" className="tele-bogen" role="img" aria-label={beschriftung}>
      <path d={bogen(a0, a1)} className="tele-bogen-grund" />
      <path d={bogen(winkel(rot), a1)} className="tele-bogen-rot" />
      {wert !== null && <path d={bogen(a0, winkel(wert))} className="tele-bogen-wert" />}
      <text x={50} y={56} textAnchor="middle" className="tele-bogen-zahl">
        {wert === null ? "–" : zahl(wert, 1, i18n.language)}
      </text>
      <text x={50} y={76} textAnchor="middle" className="tele-skala">
        {beschriftung}
      </text>
    </svg>
  );
}

export function Triebwerke({ tm }: P) {
  const { t, i18n } = useTranslation();
  const anzahlGemeldet = tm.wert("triebwerke_anzahl");
  // Ohne Angabe: so viele, wie N1 liefern.
  let anzahl = anzahlGemeldet ?? [1, 2, 3, 4].filter((n) => tm.wert(`n1_${n}`) !== null).length;
  anzahl = Math.max(1, Math.min(4, Math.round(anzahl || 2)));
  const zeile = (label: string, id: string, n: number) => {
    const k = tm.kanal(`${id}_${n}`);
    return (
      <span className="tele-kv-paar" key={id}>
        <span>{label}</span>
        <span>{k ? wertMitEinheit(k, tm.wert(`${id}_${n}`), t, i18n.language) : "–"}</span>
      </span>
    );
  };
  return (
    <div className="tele-triebwerke" style={{ gridTemplateColumns: `repeat(${anzahl}, minmax(0, 1fr))` }}>
      {Array.from({ length: anzahl }, (_, i) => i + 1).map((n) => (
        <div key={n} className="tele-tw">
          <div className="tele-tw-kopf">
            ENG {n}
            {(tm.wert(`laeuft_${n}`) ?? 0) >= 0.5 && <span className="tele-tw-an" aria-label={t("telemetrie.an")} />}
          </div>
          <Bogen wert={tm.wert(`n1_${n}`)} max={110} rot={104} beschriftung="N1 %" />
          <div className="tele-kv">
            {zeile("N2", "n2", n)}
            {zeile("EGT", "egt", n)}
            {zeile("FF", "ff", n)}
            {zeile(t("telemetrie.oeldruck"), "oeldruck", n)}
            {zeile(t("telemetrie.oeltemp"), "oeltemp", n)}
            {zeile(t("telemetrie.schubhebel"), "schubhebel", n)}
          </div>
        </div>
      ))}
    </div>
  );
}

/* -------------------------------------------------------------------- Tanks */

export function Tanks({ tm }: P) {
  const { t, i18n } = useTranslation();
  const ids = ["tank_links", "tank_mitte", "tank_rechts", "tank_4", "xp_tank_1", "xp_tank_2", "xp_tank_3"];
  const tanks = ids
    .map((id) => ({ id, v: tm.wert(id) }))
    .filter((x): x is { id: string; v: number } => x.v !== null);
  // Die Tankgroesse kennt der Simulator nicht zuverlaessig — die Saeule
  // zeigt deshalb den Anteil am Sprit an Bord, nicht „voll/leer".
  const gesamt = Math.max(1, tm.wert("sprit_gesamt") ?? tanks.reduce((a, x) => a + x.v, 0));
  const max = gesamt;
  if (tanks.length === 0) {
    return <p className="tele-hinweis">{t("telemetrie.keine_tanks")}</p>;
  }
  return (
    <div className="tele-tanks">
      {tanks.map(({ id, v }) => (
        <div key={id} className="tele-tank">
          <div className="tele-tank-saeule">
            <div className="tele-tank-inhalt" style={{ height: `${(v / max) * 100}%` }} />
          </div>
          <div className="tele-wert">{zahl(v, 0, i18n.language)} kg</div>
          <div className="tele-skala-html">{t(`telemetrie.kanal.${id}`)}</div>
        </div>
      ))}
      <p className="tele-hinweis tele-tanks-hinweis">{t("telemetrie.tanks_anteil")}</p>
    </div>
  );
}

/* ------------------------------------------------------------------ Systeme */

const SYSTEM_BLOECKE: Array<[string, string[]]> = [
  ["druck", ["kabinenhoehe", "kabine_vs", "differenzdruck"]],
  ["energie", ["batteriespannung", "batterie", "aussenstrom", "apu", "apu_drehzahl", "avionik"]],
  ["hydraulik", ["hydraulik"]],
  ["eis", ["vereisung", "pitot_vereisung", "pitotheizung", "enteisung_tw", "enteisung_fl"]],
  ["warnungen", ["master_caution", "master_warning", "ueberziehwarnung", "ueberdrehzahl", "unter_gs"]],
  ["kabine", ["anschnallzeichen"]],
];

export function Systeme({ tm }: P) {
  const { t, i18n } = useTranslation();
  return (
    <div className="tele-systeme">
      {SYSTEM_BLOECKE.map(([block, ids]) => (
        <div key={block} className="tele-block">
          <h4>{t(`telemetrie.block.${block}`)}</h4>
          <div className="tele-kv">
            {ids.map((id) => {
              const k = tm.kanal(id);
              if (!k) return null;
              const v = tm.wert(id);
              const warn = k.art === "schalter" && block === "warnungen" && (v ?? 0) >= 0.5;
              return (
                <span key={id} className="tele-kv-paar">
                  <span>{t(`telemetrie.kanal.${id}`)}</span>
                  <span className={warn ? "tele-rot" : ""}>{wertMitEinheit(k, v, t, i18n.language)}</span>
                </span>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}
