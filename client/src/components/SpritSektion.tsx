/**
 * v1.7.35 — Sprit-Auswertung ohne Note: Badge im Kopf, Sektion unten.
 *
 * Beide Komponenten **rendern nur** `record.sprit`, das der Rust-Client
 * einmal beim Aufsetzen gerechnet hat (`landing_scoring::sprit`). Kein
 * Fallback, kein Nachrechnen: fehlt die Auswertung, rendert `SpritBadge`
 * nichts und `SpritSektion` gibt `null` zurück — der Aufrufer zeigt dann
 * den alten Plan/Ist-Balken.
 */
import { useTranslation } from "react-i18next";
import type { SpritAuswertung, SpritPhase } from "../lib/sprit";
import { balken, hauptzahl, kg, pct, phaseTon } from "../lib/sprit";

const TON = {
  ok: "#22c55e",
  warn: "#f2b24c",
  bad: "#ff5c4d",
  neutral: "var(--text-muted)",
} as const;

const PILL = {
  gruen: { color: "#1f6e3a", bg: "#d4eddc", key: "landing.sprit.badge_intakt" },
  gelb: { color: "#8a4500", bg: "#fde2c4", key: "landing.sprit.badge_unter" },
  grau: { color: "#555", bg: "#eee", key: "landing.sprit.badge_np" },
} as const;

/** Badge neben „Forensik v2" im Kopf der Landung. Nie rot, keine Zahl. */
export function SpritBadge({ sprit }: { sprit: SpritAuswertung | null | undefined }) {
  const { t } = useTranslation();
  if (!sprit) return null;
  const p = PILL[sprit.badge] ?? PILL.grau;
  return (
    <div
      data-testid="sprit-badge"
      data-ton={sprit.badge}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: "0.5rem",
        padding: "0.35rem 0.6rem",
        background: "#f7f9fc",
        borderRadius: "0.5rem",
        fontSize: "0.85rem",
        fontWeight: 700,
        color: "#1a2230",
      }}
    >
      {t("landing.sprit.badge_label")}
      <span
        style={{
          fontWeight: 600,
          fontSize: "0.76rem",
          padding: "2px 8px",
          borderRadius: 999,
          color: p.color,
          background: p.bg,
        }}
      >
        {t(p.key)}
      </span>
    </div>
  );
}

function Phase({
  label,
  phase,
  sub,
  art,
}: {
  label: string;
  phase: SpritPhase | null;
  sub: string | null;
  art: "bis_sinkflug" | "anflug";
}) {
  const { t } = useTranslation();
  const ton = phaseTon(phase, art);
  const b = balken(phase);
  return (
    <>
      <span style={{ color: "var(--text-muted)" }}>{label}</span>
      <div
        style={{
          height: 11,
          background: "var(--surface-2)",
          borderRadius: 4,
          overflow: "hidden",
          position: "relative",
        }}
      >
        <i
          style={{
            position: "absolute",
            inset: 0,
            width: `${b.plan}%`,
            background: "rgba(127,127,127,0.25)",
            display: "block",
          }}
        />
        <i
          style={{
            position: "absolute",
            inset: 0,
            width: `${b.ist}%`,
            background: TON[ton],
            display: "block",
          }}
        />
      </div>
      <span
        style={{
          fontWeight: 600,
          fontVariantNumeric: "tabular-nums",
          whiteSpace: "nowrap",
          color: ton === "neutral" ? "var(--text-muted)" : TON[ton],
        }}
      >
        {phase ? hauptzahl(phase, art) : t("landing.sprit.keine_messung")}
      </span>
      <span style={{ gridColumn: "2 / -1", color: "var(--text-muted)", fontSize: "0.76rem", marginTop: "-0.3rem" }}>
        {phase ? t("landing.sprit.phase_werte", { ist: kg(phase.ist_kg), plan: kg(phase.plan_kg) }) : ""}
        {phase && hauptzahl(phase, art) !== pct(phase.abweichung_pct) ? ` · ${pct(phase.abweichung_pct)}` : ""}
        {phase && sub ? ` · ${sub}` : ""}
      </span>
    </>
  );
}

function Leiter({ sprit }: { sprit: SpritAuswertung }) {
  const { t } = useTranslation();
  const l = sprit.leiter;
  if (!l || l.block_kg <= 0) return null;
  const W = 560;
  const x = (v: number) => (v / l.block_kg) * W;
  const parts: Array<[keyof typeof l, string, string]> = [
    ["taxi_kg", "#4a5563", t("landing.sprit.leiter_taxi")],
    ["trip_kg", "#38bdf8", t("landing.sprit.leiter_trip")],
    ["contingency_kg", "#f2b24c", t("landing.sprit.leiter_contingency")],
    // Reihenfolge = Verbrauchsreihenfolge: erst Contingency, dann Extra.
    // Alternate und Reserve stehen rechts, weil sie stehen bleiben sollen —
    // nur so trifft die von rechts gemessene Landemarke denselben Punkt,
    // den `sprit.rs` rechnet (bei DLH370 die Grenze der 3581 kg Rest-Extra).
    ["extra_kg", "#1f2d3a", t("landing.sprit.leiter_extra")],
    ["alternate_kg", "#3b4655", t("landing.sprit.leiter_alternate")],
    ["reserve_kg", "#3b4655", t("landing.sprit.leiter_reserve")],
  ];
  let cursor = 0;
  const rects = parts.map(([k, fill, label]) => {
    const w = x(l[k]);
    const r = { x: cursor, w, fill, label, kg: l[k] };
    cursor += w;
    return r;
  });
  // Zwei Marken, beide Tatsachen: womit abgehoben, womit gelandet.
  //
  // Die Leiter zeigt den PLAN. Wer anders tankt als geplant, bei dem passt
  // ein einzelner Strich nirgends — vorher stand er entweder im falschen
  // Block (gegen die Zeile daneben) oder bei nicht verbrauchter Contingency
  // fest auf deren Kante (gegen sein eigenes Etikett). Mit beiden Marken
  // liest sich die Grafik ohne Widerspruch: der Abstand dazwischen ist der
  // Verbrauch, und die Zeile darunter erklaert, was davon Contingency und
  // Extra getragen haben.
  const ldg = sprit.landing_fuel_kg;
  const imBild = (v: number) => Math.min(Math.max(v, 0), W);
  const markX = ldg != null ? imBild(W - x(ldg)) : null;
  // Der Abhebe-Tankstand kommt aus der Auswertung — er wird NICHT
  // zurückgerechnet. Ein früherer Versuch tat das und lag bei DLH 370 um
  // 3 204 kg daneben: Bleibt der Mehrverbrauch unter der Contingency,
  // steckt er in keinem der übrigen Felder.
  const abhebenKg = sprit.takeoff_fuel_kg;
  const abhebenX = abhebenKg != null ? imBild(W - x(abhebenKg)) : null;
  // Die Marke ist eine Tatsache, kein Urteil: Ton nach Reservestand,
  // nie die Fehlerfarbe.
  const markFarbe = sprit.badge === "gelb" ? TON.warn : "var(--text)";
  return (
    <svg viewBox={`0 0 ${W} 100`} role="img" aria-label={t("landing.sprit.leiter_titel")} style={{ display: "block", maxWidth: "100%", height: "auto", marginTop: "0.6rem" }}>
      <text x="0" y="11" style={{ font: "600 11px system-ui, sans-serif", fill: "var(--text)" }}>
        {t("landing.sprit.leiter_titel")} · {kg(l.block_kg)} kg
      </text>
      {rects.map((r) => (
        <rect key={r.label} x={r.x} y={20} width={Math.max(r.w, 0)} height={22} fill={r.fill} stroke="var(--border)" strokeWidth={0.5} />
      ))}
      {rects
        .filter((r) => {
          // 10.5px Monospace ≈ 6.3px je Zeichen — nur beschriften, wenn der
          // Text ins Segment passt und im Bild bleibt.
          const breite = (`${r.label} ${kg(r.kg)}`).length * 6.3;
          return r.w >= breite + 6 && r.x + 3 + breite <= W;
        })
        .map((r) => (
          <text key={`t-${r.label}`} x={r.x + 3} y={56} style={{ font: "10.5px ui-monospace, monospace", fill: "var(--text-muted)" }}>
            {r.label} {kg(r.kg)}
          </text>
        ))}
      {abhebenX != null && Math.abs(abhebenX - (markX ?? 0)) > 6 && (
        <>
          <line x1={abhebenX} y1={18} x2={abhebenX} y2={44} stroke="var(--text-muted)" strokeWidth={1} strokeDasharray="3 2" />
          {/* Unter die Leiter statt daneben: der Abhebe-Tankstand liegt nah
              am Block, die Marke also weit links — auf y=14 kollidierte der
              Text mit dem Leiter-Titel. */}
          <text x={Math.min(abhebenX + 4, W - 120)} y={68} style={{ font: "10px ui-monospace, monospace", fill: "var(--text-muted)" }}>
            {t("landing.sprit.abgehoben_mit", { kg: kg(abhebenKg) })}
          </text>
        </>
      )}
      {markX != null && (
        <>
          <line x1={markX} y1={16} x2={markX} y2={46} stroke={markFarbe} strokeWidth={2} />
          <text x={Math.min(markX + 5, W - 210)} y={80} style={{ font: "600 11px system-ui, sans-serif", fill: "var(--text)" }}>
            ▲ {t("landing.sprit.gelandet_mit", { kg: kg(ldg) })}
          </text>
        </>
      )}
      <text x="0" y="94" style={{ font: "10.5px ui-monospace, monospace", fill: "var(--text-muted)" }}>
        {sprit.contingency_verbraucht ? t("landing.sprit.contingency_verbraucht") : t("landing.sprit.contingency_unberuehrt")}
        {sprit.alternate_und_reserve_intakt === true ? ` · ${t("landing.sprit.alt_res_intakt")}` : ""}
        {sprit.alternate_und_reserve_intakt === false ? ` · ${t("landing.sprit.alt_res_angegriffen")}` : ""}
      </text>
    </svg>
  );
}

function Zeile({ label, children, ton }: { label: string; children: React.ReactNode; ton?: keyof typeof TON }) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        gap: 12,
        flexWrap: "wrap",
        fontSize: "0.86rem",
        padding: "0.45rem 0",
        borderTop: "1px solid color-mix(in srgb, var(--text) 8%, transparent)",
      }}
    >
      <span style={{ color: "var(--text-muted)" }}>{label}</span>
      <b style={{ fontVariantNumeric: "tabular-nums", fontWeight: 600, color: ton ? TON[ton] : "var(--text)" }}>{children}</b>
    </div>
  );
}

/** Die Sektion „Sprit" im Landungs-Tab — Phasen, Leiter, Reserve, Extra. */
export function SpritSektion({ sprit }: { sprit: SpritAuswertung | null | undefined }) {
  const { t } = useTranslation();
  if (!sprit) return null;
  const r = sprit.reserve;
  const reserveZeile =
    r.status === "intakt"
      ? t("landing.sprit.reserve_zeile_intakt", { ldg: kg(sprit.landing_fuel_kg), res: kg(sprit.reserve_kg), q: Math.round(r.quote_pct) })
      : r.status === "unterschritten"
        ? t("landing.sprit.reserve_zeile_unter", { ldg: kg(sprit.landing_fuel_kg), res: kg(sprit.reserve_kg), q: Math.round(r.quote_pct) })
        : t(`landing.sprit.reserve_grund_${r.grund}`, { defaultValue: t("landing.sprit.badge_np") });
  const reserveTon = r.status === "intakt" ? "ok" : r.status === "unterschritten" ? "warn" : "neutral";
  const anflugSub =
    sprit.zeit_unter_schwelle_min != null && sprit.schwelle_ft != null
      ? t("landing.sprit.zeit_wert", { min: sprit.zeit_unter_schwelle_min.toFixed(1).replace(".", ","), ft: kg(sprit.schwelle_ft) }) +
        (sprit.strecke_anflug_nm != null && sprit.plan_strecke_anflug_nm != null
          ? ` · ${t("landing.sprit.strecke", { ist: Math.round(sprit.strecke_anflug_nm), plan: Math.round(sprit.plan_strecke_anflug_nm) })}`
          : "")
      : null;
  return (
    <div data-testid="sprit-sektion" style={{ marginBottom: "0.9rem" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: "0.6rem" }}>
        <b>{t("landing.sprit.title")}</b>
        <span
          style={{
            fontSize: "0.62rem",
            letterSpacing: ".06em",
            textTransform: "uppercase",
            padding: "2px 7px",
            borderRadius: 999,
            background: "color-mix(in srgb, var(--text) 8%, transparent)",
            color: "var(--text-muted)",
            fontWeight: 600,
          }}
        >
          {t("landing.sprit.keine_note")}
        </span>
        <span
          title={t("landing.sprit.info")}
          aria-label={t("landing.sprit.info")}
          style={{ color: "var(--text-muted)", cursor: "help", fontSize: "0.8rem" }}
        >
          ⓘ
        </span>
      </div>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "112px 1fr auto",
          gap: "0.5rem 0.7rem",
          alignItems: "center",
          fontSize: "0.84rem",
        }}
      >
        <Phase label={t("landing.sprit.bis_sinkflug")} phase={sprit.bis_sinkflug} sub={null} art="bis_sinkflug" />
        <Phase label={t("landing.sprit.anflug")} phase={sprit.anflug} sub={anflugSub} art="anflug" />
      </div>
      <Leiter sprit={sprit} />
      <div style={{ marginTop: "0.6rem" }}>
        <Zeile label={t("landing.sprit.reserve_label")} ton={reserveTon}>
          {reserveZeile}
        </Zeile>
        {sprit.extra_getankt_kg != null && sprit.extra_genutzt_kg != null && sprit.extra_ungenutzt_kg != null && (
          <Zeile label={t("landing.sprit.extra_label")}>
            {t("landing.sprit.extra_zeile", {
              getankt: kg(sprit.extra_getankt_kg),
              genutzt: kg(sprit.extra_genutzt_kg),
              ungenutzt: kg(sprit.extra_ungenutzt_kg),
            })}
          </Zeile>
        )}
      </div>
    </div>
  );
}
