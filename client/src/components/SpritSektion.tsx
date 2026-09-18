/**
 * v1.7.35 — Sprit-Auswertung ohne Note: Badge im Kopf, Sektion unten.
 *
 * Beide Komponenten **rendern nur** `record.sprit`, das der Rust-Client
 * einmal beim Aufsetzen gerechnet hat (`landing_scoring::sprit`). Kein
 * Fallback, kein Nachrechnen: fehlt die Auswertung, rendert `SpritBadge`
 * nichts und `SpritSektion` gibt `null` zurück — der Aufrufer zeigt dann
 * den alten Plan/Ist-Balken.
 */
import i18n from "i18next";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { SpritAuswertung, SpritPhase } from "../lib/sprit";
import { balken, dezimal, hauptzahl, hauptzahlText, kg, pct, phaseTon, reserveAbstand } from "../lib/sprit";

const TON = {
  ok: "#22c55e",
  warn: "#f2b24c",
  bad: "#ff5c4d",
  neutral: "var(--text-muted, #9aa4b2)",
} as const;

const PILL = {
  gruen: { color: "#1f6e3a", bg: "#d4eddc" },
  gelb: { color: "#8a4500", bg: "#fde2c4" },
  grau: { color: "#555", bg: "#eee" },
} as const;

/**
 * Der Badge-Text je Zustand.
 *
 * Bewusst drei literale `t()`-Aufrufe statt eines Schlüssels aus der
 * Tabelle: Der Beschriftungs-Wächter in `scripts/anzeige-sync.mjs` liest
 * den Quelltext und erkennt nur feste Aufrufe. Stand der Schlüssel in
 * `PILL`, waren diese drei Texte ungeprüft — und ein fehlender Schlüssel
 * hätte in der Live-Übersicht still den deutschen Vorgabetext gezeigt.
 */
export function spritBadgeText(badge: SpritAuswertung["badge"], uebersetzer?: (k: string) => string): string {
  // Ohne uebergebenen Uebersetzer den globalen nehmen — die Live-Uebersicht
  // ruft die Funktion ausserhalb einer Komponente, wo `useTranslation`
  // nicht geht.
  //
  // Die Funktion MUSS `t` heißen: Der Beschriftungs-Wächter erkennt nur
  // literale `t("…")`-Aufrufe. Unter einem anderen Namen waren diese drei
  // Texte für ihn unsichtbar — genau das hat `pruef-a0-waechter.mjs` bei
  // der ersten Fassung gemeldet.
  const t = uebersetzer ?? ((k: string) => i18n.t(k));
  if (badge === "gruen") return t("landing.sprit.badge_intakt");
  if (badge === "gelb") return t("landing.sprit.badge_unter");
  return t("landing.sprit.badge_np");
}

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
        {spritBadgeText(sprit.badge, t)}
      </span>
    </div>
  );
}

/**
 * Rollen nach der Landung — dieselbe Zeile wie die Phasen, aber ohne Plan.
 *
 * Der Balken steht im SELBEN Kilogramm-Massstab wie „Rollen vor Start",
 * damit sich beide Rollphasen auf einen Blick vergleichen lassen. Gibt es
 * die Zeile davor nicht, fuellt der Wert den Balken.
 */
function RollenNachLandung({ kgNach, vorStart }: { kgNach: number; vorStart: SpritPhase | null | undefined }) {
  const { t } = useTranslation();
  const massstab = vorStart ? Math.max(vorStart.plan_kg, vorStart.ist_kg, kgNach, 1) : Math.max(kgNach, 1);
  const breite = (kgNach / massstab) * 100;
  const leise = "var(--text-muted, #9aa4b2)";
  return (
    <>
      <span style={{ color: leise }} title={t("landing.sprit.hint_rollen_nach")}>
        {t("landing.sprit.report_rollen_nach")}
      </span>
      <div
        data-testid="sprit-rollen-nach-balken"
        style={{ height: 11, background: "var(--surface-2, #1b2432)", borderRadius: 4, overflow: "hidden", position: "relative" }}
      >
        <i style={{ position: "absolute", inset: 0, width: `${breite}%`, background: "#8b95a7", display: "block" }} />
      </div>
      <span style={{ fontWeight: 600, fontVariantNumeric: "tabular-nums", whiteSpace: "nowrap", color: "var(--text, #e8ecf3)" }}>
        {kg(kgNach)} kg
      </span>
      <span style={{ gridColumn: "2 / -1", color: leise, fontSize: "0.76rem", marginTop: "-0.3rem" }}>
        {t("landing.sprit.rollen_nach_ohne_plan")}
      </span>
    </>
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
      <span style={{ color: "var(--text-muted, #9aa4b2)" }}>{label}</span>
      <div
        style={{
          height: 11,
          background: "var(--surface-2, #1b2432)",
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
          color: ton === "neutral" ? "var(--text-muted, #9aa4b2)" : TON[ton],
        }}
      >
        {phase ? hauptzahlText(phase, art) : t("landing.sprit.keine_messung")}
      </span>
      <span style={{ gridColumn: "2 / -1", color: "var(--text-muted, #9aa4b2)", fontSize: "0.76rem", marginTop: "-0.3rem" }}>
        {phase ? t("landing.sprit.phase_werte", { ist: kg(phase.ist_kg), plan: kg(phase.plan_kg) }) : ""}
        {phase && hauptzahlText(phase, art) !== pct(phase.abweichung_pct) ? ` · ${pct(phase.abweichung_pct)}` : ""}
        {phase && sub ? ` · ${sub}` : ""}
      </span>
    </>
  );
}

function Leiter({ sprit }: { sprit: SpritAuswertung }) {
  const { t } = useTranslation();
  // Welcher Block gerade unter der Maus liegt (Balken oder Legende).
  const [aktiv, setAktiv] = useState<string | null>(null);
  const l = sprit.leiter;
  if (!l || l.block_kg <= 0) return null;
  // **Was der Block in DIESEM Flug war** — fuer die Erklaerung beim
  // Darueberfahren (Thomas, 18.09.2026: „passend zum Flug"). Nur Werte aus
  // der fertigen Auswertung, nichts nachgerechnet. `null` = nur die
  // allgemeine Erklaerung.
  const bis = sprit.bis_sinkflug;
  const an = sprit.anflug;
  const unter = l.untertankung_kg ?? 0;
  const abstand = reserveAbstand(sprit);
  const imFlug = {
    taxi: sprit.rollen_vor_start
      ? t("landing.sprit.imflug_taxi", { ist: kg(sprit.rollen_vor_start.ist_kg), plan: kg(sprit.rollen_vor_start.plan_kg) })
      : null,
    trip:
      bis && an
        ? t("landing.sprit.imflug_trip", { bis: kg(bis.ist_kg), bisPlan: kg(bis.plan_kg), an: kg(an.ist_kg), anPlan: kg(an.plan_kg) })
        : null,
    contingency:
      sprit.contingency_verbraucht === true
        ? t("landing.sprit.imflug_cont_verbraucht")
        : sprit.contingency_verbraucht === false
          ? t("landing.sprit.imflug_cont_unberuehrt")
          : null,
    extra:
      sprit.extra_getankt_kg != null && sprit.extra_genutzt_kg != null && sprit.extra_ungenutzt_kg != null
        ? t("landing.sprit.imflug_extra", { g: kg(sprit.extra_getankt_kg), n: kg(sprit.extra_genutzt_kg), u: kg(sprit.extra_ungenutzt_kg) }) +
          (unter > 0 ? ` · ${t("landing.sprit.imflug_untertankung", { kg: kg(unter) })}` : "")
        : null,
    sonstiges: null,
    uebertankung: (l.uebertankung_kg ?? 0) > 0 ? t("landing.sprit.imflug_uebertankung", { kg: kg(l.uebertankung_kg ?? 0) }) : null,
    alternate:
      sprit.alternate_und_reserve_intakt === true
        ? t("landing.sprit.imflug_alt_intakt")
        : sprit.alternate_und_reserve_intakt === false
          ? t("landing.sprit.imflug_alt_angegriffen")
          : null,
    reserve:
      abstand == null
        ? null
        : abstand >= 0
          ? t("landing.sprit.imflug_res_ueber", { ab: kg(abstand) })
          : t("landing.sprit.imflug_res_unter", { ab: kg(Math.abs(abstand)) }),
  };
  const W = 560;
  // Zusatzsprit (ETOPS, Minimum) und Übertankung kommen FERTIG aus der
  // Rechnung (`sprit.rs`, Leiter). Bis v1.7.36 rechnete diese Anzeige die
  // Übertankung selbst aus — entgegen der Zusage „nur gerendert" — und
  // Zusatzsprit fehlte ganz, sodass die Marken um genau diesen Betrag
  // danebenlagen.
  //
  // Reihenfolge = Verbrauchsreihenfolge der Rechnung: Contingency, dann
  // Extra. Zusatzsprit und Übertankung liegen darüber und bleiben — wie
  // Alternate und Reserve — stehen. Nur so trifft die Landemarke denselben
  // Punkt, den die Zeile „Extra ungenutzt" nennt.
  // [kg, Farbe, Name, Erklaerung] — die Erklaerung steht als Hinweis an
  // Balken und Legende. Bis v1.7.36 standen Namen nur IM Balken, und nur,
  // wenn sie hineinpassten: Contingency und Extra blieben meist namenlos.
  const parts: Array<[number, string, string, string, string | null]> = [
    [l.taxi_kg, "#6b7688", t("landing.sprit.leiter_taxi"), t("landing.sprit.leiter_hint_taxi"), imFlug.taxi],
    [l.trip_kg, "#38bdf8", t("landing.sprit.leiter_trip"), t("landing.sprit.leiter_hint_trip"), imFlug.trip],
    [l.contingency_kg, "#f2b24c", t("landing.sprit.leiter_contingency"), t("landing.sprit.leiter_hint_contingency"), imFlug.contingency],
    // Nur das Extra, das an Bord war — die Untertankung kommt fertig aus
    // der Rechnung (`sprit.rs`, Leiter).
    [Math.max(l.extra_kg - (l.untertankung_kg ?? 0), 0), "#3d7a94", t("landing.sprit.leiter_extra"), t("landing.sprit.leiter_hint_extra"), imFlug.extra],
    [l.sonstiges_kg ?? 0, "#7a6fa8", t("landing.sprit.leiter_sonstiges"), t("landing.sprit.leiter_hint_sonstiges"), imFlug.sonstiges],
    [l.uebertankung_kg ?? 0, "#4f9a72", t("landing.sprit.leiter_uebertankung"), t("landing.sprit.leiter_hint_uebertankung"), imFlug.uebertankung],
    [l.alternate_kg, "#5d6b80", t("landing.sprit.leiter_alternate"), t("landing.sprit.leiter_hint_alternate"), imFlug.alternate],
    [l.reserve_kg, "#475163", t("landing.sprit.leiter_reserve"), t("landing.sprit.leiter_hint_reserve"), imFlug.reserve],
  ];
  // Die Skala ist die Summe der gezeichneten Posten — normal genau Block
  // plus Übertankung. Plant ein OFP aber mehr in die Posten als in den
  // Block (dann ist „Zusatzsprit" 0), wären die Balken sonst über die
  // Breite hinausgelaufen (QS-Vorschlag V-a, 18.09.2026).
  const summe = parts.reduce((acc, [kgWert]) => acc + Math.max(kgWert, 0), 0);
  const skala = Math.max(l.block_kg + (l.uebertankung_kg ?? 0) - (l.untertankung_kg ?? 0), summe);
  const x = (v: number) => (v / skala) * W;
  let cursor = 0;
  const rects = parts
    .filter(([kgWert]) => kgWert > 0)
    .map(([kgWert, fill, label, hinweis, imFlugText]) => {
      const w = x(kgWert);
      const r = { x: cursor, w, fill, label, hinweis, imFlug: imFlugText, kg: kgWert };
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
  const markFarbe = sprit.badge === "gelb" ? TON.warn : "var(--text, #e8ecf3)";
  const leise = "var(--text-muted, #9aa4b2)";
  // **Nur die Balken wachsen mit der Breite, die Schrift nicht.**
  //
  // Bis v1.7.36 war die ganze Leiter EIN SVG mit fester Innenbreite (560),
  // Beschriftungen eingeschlossen. Die Live-Uebersicht zog es auf fast
  // 2 000 px — und damit jede Beschriftung auf das Dreieinhalbfache, in
  // einer Monospace-Schrift, die sonst nirgends in der Sektion steht
  // (Thomas, BIT348, 18.09.2026). Jetzt: Balken und Marken im SVG, das
  // sich ohne Seitenverhaeltnis dehnt; alles Lesbare ist HTML in der
  // Schrift der Sektion.
  return (
    <div style={{ marginTop: "0.8rem" }}>
      <div style={{ fontSize: "0.86rem", fontWeight: 600, color: "var(--text, #e8ecf3)", marginBottom: 6 }}>
        {t("landing.sprit.leiter_titel")} · {kg(l.block_kg)} kg
      </div>
      <div style={{ position: "relative" }}>
      <svg
        viewBox={`0 0 ${W} 30`}
        preserveAspectRatio="none"
        width="100%"
        height={30}
        role="img"
        aria-label={t("landing.sprit.leiter_titel")}
        style={{ display: "block" }}
      >
        {rects.map((r) => (
          <rect
            key={r.label}
            data-block={r.label}
            aria-label={`${r.label} ${kg(r.kg)} kg — ${r.hinweis}${r.imFlug ? ` ${r.imFlug}` : ""}`}
            x={r.x}
            y={4}
            width={Math.max(r.w, 0)}
            height={22}
            fill={r.fill}
            stroke="var(--border, #2a3344)"
            strokeWidth={1}
            vectorEffect="non-scaling-stroke"
            opacity={aktiv != null && aktiv !== r.label ? 0.45 : 1}
            style={{ cursor: "help", transition: "opacity 120ms" }}
            onMouseEnter={() => setAktiv(r.label)}
            onMouseLeave={() => setAktiv(null)}
          />
        ))}
        {/* Die Abhebe-Marke wird IMMER gezeichnet. Bis v1.7.35 verschwand
            sie bei kleinem Abstand zur Landemarke ganz — ohne Hinweis, und
            damit brach die zugesagte Eigenschaft „der Abstand ist der
            Verbrauch" still. */}
        {abhebenX != null && (
          <line x1={abhebenX} y1={0} x2={abhebenX} y2={30} stroke={leise} strokeWidth={1.5} strokeDasharray="3 2" vectorEffect="non-scaling-stroke" />
        )}
        {markX != null && (
          <line x1={markX} y1={0} x2={markX} y2={30} stroke={markFarbe} strokeWidth={2} vectorEffect="non-scaling-stroke" pointerEvents="none" />
        )}
      </svg>
      {(() => {
        // Die Erklaerung zum Block unter der Maus: was er ist, und was er
        // in diesem Flug war. Sofort sichtbar, nicht erst nach der
        // Verzoegerung eines Browser-Hinweises.
        const r = rects.find((q) => q.label === aktiv);
        if (!r) return null;
        const mitte = ((r.x + r.w / 2) / W) * 100;
        return (
          <div
            role="tooltip"
            data-testid="sprit-leiter-hinweis"
            style={{
              position: "absolute",
              bottom: "calc(100% + 8px)",
              left: `${Math.min(Math.max(mitte, 14), 86)}%`,
              transform: "translateX(-50%)",
              width: "max-content",
              maxWidth: 340,
              padding: "8px 10px",
              borderRadius: 8,
              // DECKEND und mit festen Farben: `--surface-2` ist in der
              // Live-Webapp halbtransparent — der Text darunter schien durch.
              // Feste Schriftfarben, weil der Hinweis in beiden Themes dunkel
              // ist; mit `--text` stuende im hellen Theme dunkel auf dunkel.
              background: "#151c27",
              border: "1px solid #2f3a4d",
              boxShadow: "0 8px 24px rgba(0,0,0,0.45)",
              fontSize: "0.8rem",
              lineHeight: 1.4,
              color: "#b4bdca",
              pointerEvents: "none",
              zIndex: 5,
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 6, color: "#eef2f7", fontWeight: 600 }}>
              <span aria-hidden style={{ width: 10, height: 10, borderRadius: 2, background: r.fill, flex: "none" }} />
              {r.label} · {kg(r.kg)} kg
            </div>
            <div style={{ marginTop: 3 }}>{r.hinweis}</div>
            {r.imFlug && (
              <div style={{ marginTop: 5, color: "#eef2f7", fontWeight: 600 }}>
                {t("landing.sprit.imflug_titel")}: {r.imFlug}
              </div>
            )}
          </div>
        );
      })()}
      </div>
      {/* Legende: jeder Block mit Name, Menge und Erklaerung — und die
          beiden Marken. Erklaert wird am Hinweis (Maus darueber), damit die
          Zeile ruhig bleibt. */}
      <div
        data-testid="sprit-leiter-legende"
        style={{ display: "flex", flexWrap: "wrap", gap: "6px 16px", marginTop: 8, fontSize: "0.8rem", color: leise }}
      >
        {rects.map((r) => (
          <span
            key={`l-${r.label}`}
            data-block={r.label}
            tabIndex={0}
            onMouseEnter={() => setAktiv(r.label)}
            onMouseLeave={() => setAktiv(null)}
            onFocus={() => setAktiv(r.label)}
            onBlur={() => setAktiv(null)}
            style={{ display: "inline-flex", alignItems: "center", gap: 6, cursor: "help" }}
          >
            <span aria-hidden style={{ width: 10, height: 10, borderRadius: 2, background: r.fill, flex: "none" }} />
            {r.label}
            <b style={{ color: "var(--text, #e8ecf3)", fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>{kg(r.kg)} kg</b>
          </span>
        ))}
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: "6px 16px", marginTop: 6, fontSize: "0.8rem", color: leise }}>
        {abhebenX != null && (
          <span title={t("landing.sprit.leiter_hint_abgehoben")} style={{ display: "inline-flex", alignItems: "center", gap: 6, cursor: "help" }}>
            <span aria-hidden style={{ width: 0, height: 12, borderLeft: `2px dashed ${leise}`, flex: "none" }} />
            {sprit.einstieg_in_der_luft
              ? t("landing.sprit.eingestiegen_mit", { kg: kg(abhebenKg) })
              : t("landing.sprit.abgehoben_mit", { kg: kg(abhebenKg) })}
          </span>
        )}
        {markX != null && (
          <span title={t("landing.sprit.leiter_hint_gelandet")} style={{ display: "inline-flex", alignItems: "center", gap: 6, cursor: "help", color: "var(--text, #e8ecf3)" }}>
            <span aria-hidden style={{ width: 0, height: 12, borderLeft: `2px solid ${markFarbe}`, flex: "none" }} />
            {t("landing.sprit.gelandet_mit", { kg: kg(ldg) })}
          </span>
        )}
        <span>
          {sprit.contingency_verbraucht ? t("landing.sprit.contingency_verbraucht") : t("landing.sprit.contingency_unberuehrt")}
          {sprit.alternate_und_reserve_intakt === true ? ` · ${t("landing.sprit.alt_res_intakt")}` : ""}
          {sprit.alternate_und_reserve_intakt === false ? ` · ${t("landing.sprit.alt_res_angegriffen")}` : ""}
        </span>
      </div>
    </div>
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
        borderTop: "1px solid color-mix(in srgb, var(--text, #e8ecf3) 8%, transparent)",
      }}
    >
      <span style={{ color: "var(--text-muted, #9aa4b2)" }}>{label}</span>
      <b style={{ fontVariantNumeric: "tabular-nums", fontWeight: 600, color: ton ? TON[ton] : "var(--text, #e8ecf3)" }}>{children}</b>
    </div>
  );
}

/**
 * Eine Kennzahl mit Erklaerung — und mit Begruendung, wenn sie fehlt.
 *
 * Beides gab es bis v1.7.35 nur in der Live-Uebersicht: Dort erklaerte ein
 * Hinweis je Kachel, was die Zahl bedeutet, und statt eines stummen
 * Strichs stand dort, WARUM nichts zu sehen ist („kein Vergleichspunkt
 * gemessen"). Der Client hatte einen einzigen Tooltip fuer die ganze
 * Sektion und liess fehlende Werte kommentarlos weg.
 *
 * Beim Zusammenlegen wandert die reichere Fassung in die gemeinsame —
 * nichts geht verloren (`feedback-never-drop-features-during-redesign`).
 */
function Kennzahl({
  label,
  wert,
  einheit,
  erklaerung,
  grundWennLeer,
  ton = "neutral",
}: {
  label: string;
  wert: string | null;
  einheit?: string;
  erklaerung: string;
  grundWennLeer: string;
  ton?: "ok" | "warn" | "neutral";
}) {
  const leer = wert == null || wert === "";
  return (
    <div
      title={leer ? grundWennLeer : erklaerung}
      style={{
        border: "1px solid var(--border, #2a3344)",
        borderRadius: 8,
        padding: "0.45rem 0.6rem",
        background: "color-mix(in srgb, var(--text, #e8ecf3) 3%, transparent)",
      }}
    >
      <div
        style={{
          fontSize: "0.68rem",
          color: "var(--text-muted, #9aa4b2)",
          textTransform: "uppercase",
          letterSpacing: ".04em",
          display: "flex",
          alignItems: "center",
          gap: 4,
        }}
      >
        {label}
        {/* Das Fragezeichen erscheint NUR bei fehlendem Wert — so sieht man,
            dass es einen Grund gibt, statt einen stummen Strich zu lesen. */}
        {leer && <span aria-hidden="true">ⓘ</span>}
      </div>
      <div
        style={{
          fontSize: "1.05rem",
          fontWeight: 600,
          fontVariantNumeric: "tabular-nums",
          color: leer
            ? "var(--text-muted, #9aa4b2)"
            : ton === "ok"
              ? TON.ok
              : ton === "warn"
                ? TON.warn
                : "var(--text, #e8ecf3)",
        }}
      >
        {leer ? "—" : wert}
        {!leer && einheit && (
          <span style={{ fontSize: "0.72rem", fontWeight: 400, marginLeft: 3, color: "var(--text-muted, #9aa4b2)" }}>
            {einheit}
          </span>
        )}
      </div>
    </div>
  );
}

/** Die Sektion „Sprit" im Landungs-Tab — Phasen, Leiter, Reserve, Extra. */
export function SpritSektion({ sprit, ohneTitel = false }: { sprit: SpritAuswertung | null | undefined; ohneTitel?: boolean }) {
  const { t } = useTranslation();
  if (!sprit) return null;
  const r = sprit.reserve;
  // v1.7.37: Abstand zur Final Reserve in kg statt der Quote — „Reserve
  // 732 kg (338 %)" las sich, als haette die Reserve 338 % (Thomas, BIT348).
  // Die Quote bleibt nur fuer aeltere Datensaetze ohne das Feld.
  const abstand = reserveAbstand(sprit);
  const reserveZeile =
    r.status === "intakt" && abstand != null
      ? t("landing.sprit.reserve_zeile_ueber", { ldg: kg(sprit.landing_fuel_kg), ab: kg(Math.abs(abstand)), res: kg(sprit.reserve_kg) })
      : r.status === "unterschritten" && abstand != null
        ? t("landing.sprit.reserve_zeile_darunter", { ldg: kg(sprit.landing_fuel_kg), ab: kg(Math.abs(abstand)), res: kg(sprit.reserve_kg) })
        : r.status === "intakt"
      ? t("landing.sprit.reserve_zeile_intakt", { ldg: kg(sprit.landing_fuel_kg), res: kg(sprit.reserve_kg), q: Math.round(r.quote_pct) })
      : r.status === "unterschritten"
        ? t("landing.sprit.reserve_zeile_unter", { ldg: kg(sprit.landing_fuel_kg), res: kg(sprit.reserve_kg), q: Math.round(r.quote_pct) })
        : // Punkt statt Unterstrich, damit `vorspaenneAusQuelltext` den
          // Vorspann sieht. Und OHNE `defaultValue`: Der verschluckte genau
          // den Fehlerfall — fehlte der Schlüssel, stand dort „nicht
          // prüfbar" statt des Grundes, und nichts fiel auf.
          t(`landing.sprit.reserve_grund.${r.grund}`);
  const reserveTon = r.status === "intakt" ? "ok" : r.status === "unterschritten" ? "warn" : "neutral";
  const anflugSub =
    sprit.zeit_unter_schwelle_min != null && sprit.schwelle_ft != null
      ? t("landing.sprit.zeit_wert", { min: dezimal(sprit.zeit_unter_schwelle_min, 1), ft: kg(sprit.schwelle_ft) }) +
        (sprit.strecke_anflug_nm != null && sprit.plan_strecke_anflug_nm != null
          ? ` · ${t("landing.sprit.strecke", { ist: Math.round(sprit.strecke_anflug_nm), plan: Math.round(sprit.plan_strecke_anflug_nm) })}`
          : "")
      : null;
  return (
    <div data-testid="sprit-sektion" style={{ marginBottom: "0.9rem" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: "0.6rem" }}>
        {/* Traegt die umgebende Karte den Titel schon (Live-Uebersicht:
            „⛽ Sprit"), stuende er sonst zweimal untereinander. */}
        {!ohneTitel && <b>{t("landing.sprit.title")}</b>}
        <span
          style={{
            fontSize: "0.62rem",
            letterSpacing: ".06em",
            textTransform: "uppercase",
            padding: "2px 7px",
            borderRadius: 999,
            background: "color-mix(in srgb, var(--text, #e8ecf3) 8%, transparent)",
            color: "var(--text-muted, #9aa4b2)",
            fontWeight: 600,
          }}
        >
          {t("landing.sprit.keine_note")}
        </span>
        <span
          title={t("landing.sprit.info")}
          aria-label={t("landing.sprit.info")}
          style={{ color: "var(--text-muted, #9aa4b2)", cursor: "help", fontSize: "0.8rem" }}
        >
          ⓘ
        </span>
      </div>
      {/* Die Kennzahlenreihe — vier Zahlen auf einen Blick, jede mit
          Erklaerung. Sie kam aus der Live-Uebersicht; der Client hatte sie
          nicht. Darunter stehen wie bisher die Balken und die Leiter. */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(118px, 1fr))",
          gap: "0.45rem",
          marginBottom: "0.7rem",
        }}
      >
        <Kennzahl
          label={t("landing.sprit.bis_sinkflug")}
          wert={sprit.bis_sinkflug ? hauptzahl(sprit.bis_sinkflug, "bis_sinkflug").wert : null}
          einheit={sprit.bis_sinkflug ? hauptzahl(sprit.bis_sinkflug, "bis_sinkflug").einheit : undefined}
          ton={phaseTon(sprit.bis_sinkflug, "bis_sinkflug")}
          erklaerung={t("landing.sprit.hint_bis_sinkflug")}
          grundWennLeer={t("landing.sprit.na_kein_vergleichspunkt")}
        />
        <Kennzahl
          label={t("landing.sprit.anflug")}
          wert={sprit.anflug ? hauptzahl(sprit.anflug, "anflug").wert : null}
          einheit={sprit.anflug ? hauptzahl(sprit.anflug, "anflug").einheit : undefined}
          ton={phaseTon(sprit.anflug, "anflug")}
          erklaerung={t("landing.sprit.hint_anflug")}
          grundWennLeer={t("landing.sprit.na_kein_vergleichspunkt")}
        />
        <Kennzahl
          label={t("landing.sprit.unter_schwelle")}
          wert={sprit.zeit_unter_schwelle_min != null ? dezimal(sprit.zeit_unter_schwelle_min, 1) : null}
          einheit="min"
          erklaerung={
            sprit.schwelle_ft != null
              ? t("landing.sprit.hint_schwelle", { ft: kg(sprit.schwelle_ft) })
              : t("landing.sprit.hint_schwelle_ohne")
          }
          grundWennLeer={t("landing.sprit.na_keine_messung")}
        />
        <Kennzahl
          label={t("landing.sprit.reserve_label")}
          wert={
            r.status === "nicht_pruefbar"
              ? null
              : abstand != null
                ? (abstand > 0 ? "+" : abstand < 0 ? "−" : "") + kg(Math.abs(abstand))
                : String(Math.round(r.quote_pct))
          }
          einheit={abstand != null ? "kg" : "%"}
          ton={r.status === "intakt" ? "ok" : r.status === "unterschritten" ? "warn" : "neutral"}
          erklaerung={t("landing.sprit.hint_reserve")}
          grundWennLeer={t("landing.sprit.na_reserve")}
        />
      </div>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(112px, max-content) 1fr auto",
          gap: "0.5rem 0.7rem",
          alignItems: "center",
          fontSize: "0.84rem",
        }}
      >
        <Phase label={t("landing.sprit.bis_sinkflug")} phase={sprit.bis_sinkflug} sub={null} art="bis_sinkflug" />
        <Phase label={t("landing.sprit.anflug")} phase={sprit.anflug} sub={anflugSub} art="anflug" />
        {/* v1.7.36: Rollen vor dem Start und nach der Landung. Erst damit
            ist der Flug lueckenlos — vorher fehlten bei einem A380
            vierstellige Kilogramm. Nach der Landung OHNE Plan, weil
            SimBrief nur den Weg zum Start plant. */}
        {sprit.rollen_vor_start && (
          <Phase
            label={t("landing.sprit.rollen_vor_start")}
            phase={sprit.rollen_vor_start}
            sub={null}
            art="bis_sinkflug"
          />
        )}
        {/* Rollen nach der Landung: eine volle Zeile wie „Rollen vor
            Start" — bis v1.7.37 stand es als kleine Textzeile darunter und
            ging unter (Thomas, BIT348). Ohne Plan-Balken: SimBrief plant
            nur das Rollen zum Start. */}
        {sprit.rollen_nach_landung_kg != null && (
          <RollenNachLandung kgNach={sprit.rollen_nach_landung_kg} vorStart={sprit.rollen_vor_start} />
        )}
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
