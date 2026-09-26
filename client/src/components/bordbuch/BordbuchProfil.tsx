// Bordbuch — Variante B: Haken am Flugprofil. Die Punkte sitzen dort im
// Höhenprofil, wo sie passiert sind; ein Tipp zeigt den Moment mit Uhrzeit.

import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { erledigt, sichtbar, zulu, type Punkt, type Regel, type Schalter } from "../../lib/bordbuch";

interface Props {
  punkte: Punkt[];
  eingeschaltet: Schalter[];
  profil: Array<[number, number]>;
  dep?: string | null;
  arr?: string | null;
}

const B = 640;
const H = 170;
const RAND = { l: 8, r: 8, o: 22, u: 26 };

export function BordbuchProfil({ punkte, eingeschaltet, profil, dep, arr }: Props) {
  const { t } = useTranslation();
  const [gewaehlt, setGewaehlt] = useState<Regel | null>(null);

  const marken = useMemo(
    () =>
      punkte.filter(
        (p) => sichtbar(p, eingeschaltet) && p.zeit && (erledigt(p.status) || p.status === "diesmal_ohne"),
      ),
    [punkte, eingeschaltet],
  );

  const bereich = useMemo(() => {
    const zeiten = [
      ...profil.map(([ts]) => ts * 1000),
      ...marken.map((p) => Date.parse(p.zeit as string)),
    ].filter((v) => Number.isFinite(v));
    const hoehen = [...profil.map(([, h]) => h), ...marken.map((p) => p.hoehe_ft ?? 0)];
    if (zeiten.length < 2) return null;
    const t0 = Math.min(...zeiten);
    const t1 = Math.max(...zeiten);
    const hMax = Math.max(1000, ...hoehen);
    return { t0, t1: t1 > t0 ? t1 : t0 + 1, hMax };
  }, [profil, marken]);

  if (!bereich) {
    return <div className="bb-leer">{t("bordbuch.profil_leer")}</div>;
  }
  const x = (ms: number) => RAND.l + ((ms - bereich.t0) / (bereich.t1 - bereich.t0)) * (B - RAND.l - RAND.r);
  const y = (ft: number) => H - RAND.u - (Math.max(0, ft) / bereich.hMax) * (H - RAND.o - RAND.u);

  const linie = profil.map(([ts, h], i) => `${i === 0 ? "M" : "L"}${x(ts * 1000).toFixed(1)},${y(h).toFixed(1)}`).join(" ");
  const flaeche = profil.length > 1
    ? `${linie} L${x(profil[profil.length - 1][0] * 1000).toFixed(1)},${H - RAND.u} L${x(profil[0][0] * 1000).toFixed(1)},${H - RAND.u} Z`
    : "";
  const reiseFt = Math.max(...profil.map(([, h]) => h), 0);

  // Punkte, die im Bild übereinander lägen (z. B. sechs beim Startlauf),
  // zu einer Marke mit Zahl zusammenfassen.
  type Gruppe = { id: string; x: number; y: number; punkte: Punkt[] };
  const gruppen: Gruppe[] = [];
  for (const p of [...marken].sort((a, b) => Date.parse(a.zeit as string) - Date.parse(b.zeit as string))) {
    const px = x(Date.parse(p.zeit as string));
    const py = y(p.hoehe_ft ?? 0);
    const g = gruppen.find((g) => Math.abs(g.x - px) < 16 && Math.abs(g.y - py) < 16);
    if (g) g.punkte.push(p);
    else gruppen.push({ id: p.regel, x: px, y: py, punkte: [p] });
  }
  const aktiv = gruppen.find((g) => g.id === gewaehlt) ?? null;
  // Höhe nur zeigen, wenn der Punkt deutlich über dem Boden lag — am Boden
  // wäre es nur die Flughafenhöhe.
  const boden = Math.min(...profil.map(([, h]) => h), ...marken.map((p) => p.hoehe_ft ?? 0));
  const hoeheText = (ft: number | null) => (ft != null && ft > boden + 500 ? ` · ${ft.toLocaleString()} ft` : "");
  return (
    <div className="bb-profil">
      <svg viewBox={`0 0 ${B} ${H}`} className="bb-profil-svg" role="img" aria-label={t("bordbuch.variante.profil")}>
        {flaeche && <path d={flaeche} className="bb-profil-flaeche" />}
        {linie && <path d={linie} className="bb-profil-linie" />}
        <line x1={RAND.l} x2={B - RAND.r} y1={H - RAND.u} y2={H - RAND.u} className="bb-profil-boden" />
        {dep && <text x={RAND.l} y={H - 8} className="bb-profil-text">{dep}</text>}
        {arr && <text x={B - RAND.r} y={H - 8} textAnchor="end" className="bb-profil-text">{arr}</text>}
        {reiseFt >= 1000 && (
          <text x={B / 2} y={y(reiseFt) - 6} textAnchor="middle" className="bb-profil-text">
            {reiseFt >= 18000 ? `FL${Math.round(reiseFt / 100)}` : `${Math.round(reiseFt / 100) * 100} ft`}
          </text>
        )}
        {gruppen.map((g) => {
          const alleOk = g.punkte.every((p) => erledigt(p.status));
          const atc = g.punkte.every((p) => p.status === "nach_atc");
          const n = g.punkte.length;
          const name = g.punkte.map((p) => t(`bordbuch.kurz.${p.regel}`)).join(", ");
          const umschalten = () => setGewaehlt(gewaehlt === g.id ? null : (g.id as Regel));
          return (
            // Position aussen, Knopf innen: die globale `:active`-Regel
            // (`transform: translateY(1px)`) ersetzt sonst das
            // SVG-`transform` — die Marke sprang beim Antippen nach 0,0.
            <g key={g.id} transform={`translate(${g.x.toFixed(1)},${g.y.toFixed(1)})`}>
            <g
              className={`bb-profil-marke${alleOk ? " ist-ok" : " ist-ohne"}${atc ? " ist-atc" : ""}${gewaehlt === g.id ? " ist-gewaehlt" : ""}`}
              onClick={umschalten}
              role="button"
              tabIndex={0}
              aria-label={name}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") umschalten();
              }}
            >
              <circle r={n > 1 ? 9 : 8} className="bb-profil-punkt" />
              {n > 1 ? (
                <text className="bb-profil-zahl" textAnchor="middle" dy="3.2">{n}</text>
              ) : (
                alleOk && <path d="M-3.4,0.2 -1,2.6 3.6,-2.6" className="bb-profil-haken" />
              )}
            </g>
            </g>
          );
        })}
      </svg>
      <div className="bb-profil-legende">
        <span className="bb-leg bb-leg--ok">{t("bordbuch.status.erledigt")}</span>
        <span className="bb-leg bb-leg--atc">{t("bordbuch.status.nach_atc")}</span>
        <span className="bb-leg bb-leg--ohne">{t("bordbuch.status.diesmal_ohne")}</span>
      </div>
      <div className="bb-profil-moment" aria-live="polite">
        {aktiv ? (
          <ul className="bb-profil-liste">
            {aktiv.punkte.map((p) => (
              <li key={p.regel}>
                <strong>{zulu(p.zeit)}</strong> {t(`bordbuch.kurz.${p.regel}`)}
                {p.stellung ? ` · ${p.stellung}` : ""}
                {" · "}
                {t(`bordbuch.status.${p.status}`)}
                {hoeheText(p.hoehe_ft)}
              </li>
            ))}
          </ul>
        ) : (
          t("bordbuch.profil_tippen")
        )}
      </div>
    </div>
  );
}
