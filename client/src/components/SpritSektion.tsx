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
import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SpritAuswertung, SpritPhase } from "../lib/sprit";
import { balken, hauptzahl, hauptzahlText, kg, pct, phaseTon, reserveAbstand, contingencyGenutzt, minuten } from "../lib/sprit";
import { SpritWegpunkte } from "./SpritWegpunkte";

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
 * Der Rahmen jeder Erklaerung — deckend, feste Farben (in beiden Themes
 * dunkel; mit `--text` stuende im hellen Theme dunkel auf dunkel), und
 * `--surface-2` ist in der Live-Webapp halbtransparent.
 */
const HINWEIS_STIL: React.CSSProperties = {
  position: "absolute",
  padding: "8px 10px",
  borderRadius: 8,
  background: "#151c27",
  border: "1px solid #2f3a4d",
  boxShadow: "0 8px 24px rgba(0,0,0,0.45)",
  fontSize: "0.8rem",
  fontWeight: 400,
  lineHeight: 1.4,
  letterSpacing: "normal",
  textTransform: "none",
  whiteSpace: "normal",
  color: "#b4bdca",
  pointerEvents: "none",
  zIndex: 10,
};

/** Rand zum Bildschirm, den ein Hinweis nie unterschreitet (px). */
const HINWEIS_RAND = 16;

/**
 * Wo ein Hinweis stehen muss, damit er ganz im Bild bleibt — auf dem Handy
 * wie am Monitor.
 *
 * `ankerLinks` ist die linke Kante des Elements im Fenster, `mitte` die
 * gewuenschte Mitte (oder `null`: am Element links anliegend). Zurueck kommt
 * die Breite und der Versatz RELATIV zum Element. Ohne das ragte die
 * Erklaerung am ⓘ auf einem 375-px-Handy rund 100 px ueber den Kartenrand,
 * und die Karte schnitt sie ab (QS v1.7.37, N1).
 */
function hinweisLage(
  ankerLinks: number,
  gewuenscht: number,
  mitte: number | null,
  bereich: { links: number; rechts: number } = fensterBereich(),
): { breite: number; versatz: number } {
  let innenLinks = bereich.links + HINWEIS_RAND;
  let innenRechts = bereich.rechts - HINWEIS_RAND;
  // Sehr schmaler Bereich: lieber ohne Randabstand und schmaler als
  // abgeschnitten — eine Mindestbreite ragte sonst links hinaus (QS/Codex
  // 19.09.2026).
  if (innenRechts - innenLinks < 160) {
    innenLinks = bereich.links;
    innenRechts = bereich.rechts;
  }
  const breite = Math.max(1, Math.min(gewuenscht, innenRechts - innenLinks));
  const links = mitte == null ? ankerLinks : mitte - breite / 2;
  const geklemmt = Math.min(Math.max(links, innenLinks), innenRechts - breite);
  return { breite, versatz: geklemmt - ankerLinks };
}

function fensterBereich(): { links: number; rechts: number } {
  return { links: 0, rechts: typeof window !== "undefined" ? window.innerWidth : 1024 };
}

/**
 * Der waagerechte Bereich, in dem ein Hinweis am Element `el` sichtbar ist:
 * das Fenster, geschnitten mit jedem Vorfahren, der Ueberstand abschneidet.
 * Nur am Fenster ausgerichtet, ragte der Hinweis am Rand der Sprit-Karte
 * hinaus und wurde abgeschnitten — die Karte ist schmaler als das Fenster
 * (Seitenleiste im Client, Rand in der Live-Webapp; Thomas, 19.09.2026).
 */
const SCHNEIDET_AB = new Set(["hidden", "clip", "auto", "scroll"]);

function sichtbarerBereich(el: Element | null): { links: number; rechts: number } {
  const bereich = fensterBereich();
  for (let p = el?.parentElement ?? null; p; p = p.parentElement) {
    const stil = getComputedStyle(p);
    if (!SCHNEIDET_AB.has(stil.overflowX) && !SCHNEIDET_AB.has(stil.overflow)) continue;
    const r = p.getBoundingClientRect();
    if (r.width <= 0) continue;
    bereich.links = Math.max(bereich.links, r.left);
    bereich.rechts = Math.min(bereich.rechts, r.right);
  }
  return bereich;
}

/**
 * Eine Erklaerung, die SOFORT erscheint — bei Maus, Tastatur und Tippen.
 *
 * Bis v1.7.37 standen die Erklaerungen der Sektion im `title`-Attribut. Der
 * Browser zeigt das erst nach rund einer Sekunde Stillhalten, auf Touch-
 * Geraeten nie, und nichts deutet darauf hin: Das „ⓘ" neben „keine Note"
 * sah aus, als haette es keine Funktion (Thomas, Live-Uebersicht,
 * 18.09.2026).
 *
 * `breit`: die Erklaerung ist so breit wie das Element (Kacheln) — dann
 * ragt sie am rechten Kartenrand nicht hinaus.
 */
function Erklaerung({
  text,
  children,
  als = "span",
  breit = false,
  style,
}: {
  text: string;
  children: React.ReactNode;
  als?: "span" | "div";
  breit?: boolean;
  style?: React.CSSProperties;
}) {
  const [offen, setOffen] = useState(false);
  const [lage, setLage] = useState<{ breite: number; versatz: number } | null>(null);
  const id = useId();
  const ref = useRef<HTMLElement | null>(null);
  const oeffnen = () => {
    const r = ref.current?.getBoundingClientRect();
    setLage(r && !breit ? hinweisLage(r.left, 320, null, sichtbarerBereich(ref.current)) : null);
    setOffen(true);
  };
  // Tippen daneben schliesst — iOS Safari sendet dabei weder Blur noch
  // mouseleave, und der Hinweis bliebe sonst offen stehen.
  useEffect(() => {
    if (!offen) return;
    const weg = (e: Event) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOffen(false);
    };
    // Die Lage gilt fuer die Fensterbreite beim Oeffnen. Aendert sie sich
    // (Handy gedreht), schliesst der Hinweis, statt an der alten Stelle zu
    // stehen (QS v1.7.37, B3).
    const zu = () => setOffen(false);
    document.addEventListener("pointerdown", weg);
    window.addEventListener("resize", zu);
    // Scrollt ein Container (auch waagerecht), stimmt der beim Oeffnen
    // gemessene sichtbare Bereich nicht mehr — dann schliessen (QS/Codex
    // 19.09.2026). `capture`, weil scroll nicht hochblubbert.
    document.addEventListener("scroll", zu, true);
    return () => {
      document.removeEventListener("pointerdown", weg);
      window.removeEventListener("resize", zu);
      document.removeEventListener("scroll", zu, true);
    };
  }, [offen]);
  const Tag = als;
  return (
    <Tag
      ref={ref as never}
      tabIndex={0}
      // Bildschirmleser lesen die Erklaerung vor, sobald sie offen ist.
      aria-describedby={offen ? id : undefined}
      onMouseEnter={oeffnen}
      onMouseLeave={() => setOffen(false)}
      onFocus={oeffnen}
      onBlur={() => setOffen(false)}
      // Tippen auf Touch-Geraeten: oeffnen (nicht umschalten — sonst
      // schlaegt der Klick nach dem emulierten mouseenter sofort wieder zu).
      onClick={oeffnen}
      // Escape schliesst (WCAG 1.4.13: Hinweise muessen wegzubekommen sein).
      onKeyDown={(e) => {
        if (e.key === "Escape") setOffen(false);
      }}
      // KEIN `outline: none` — der Fokusring der Tastatur muss sichtbar
      // bleiben (QS v1.7.37, N2).
      style={{ position: "relative", cursor: "help", ...style }}
    >
      {children}
      {offen && (
        <span
          id={id}
          role="tooltip"
          data-testid="sprit-erklaerung"
          style={{
            ...HINWEIS_STIL,
            top: "calc(100% + 6px)",
            ...(breit
              ? { left: 0, right: 0 }
              : { left: lage?.versatz ?? 0, width: "max-content", maxWidth: lage?.breite ?? 320 }),
          }}
        >
          {text}
        </span>
      )}
    </Tag>
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
      <Erklaerung text={t("landing.sprit.hint_rollen_nach")} style={{ color: leise }}>
        {t("landing.sprit.report_rollen_nach")}
      </Erklaerung>
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
  const hinweisId = useId();
  // Linke Kante und Breite des Balkens im Fenster — beim Oeffnen gemessen,
  // damit der Hinweis auf dem Handy nicht aus der Karte ragt.
  const balkenRef = useRef<HTMLDivElement | null>(null);
  const [balken, setBalken] = useState<{ links: number; breite: number; bereich: { links: number; rechts: number } } | null>(null);
  const zeige = (label: string) => {
    const r = balkenRef.current?.getBoundingClientRect();
    setBalken(r ? { links: r.left, breite: r.width, bereich: sichtbarerBereich(balkenRef.current) } : null);
    setAktiv(label);
  };
  // Wie bei `Erklaerung`: Tippen neben Balken und Legende schliesst — iOS
  // Safari sendet dabei weder mouseleave noch Blur (QS v1.7.37, B1) —, und
  // eine neue Fensterbreite auch (B3).
  const leiterRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (aktiv == null) return;
    const weg = (e: Event) => {
      if (leiterRef.current && !leiterRef.current.contains(e.target as Node)) setAktiv(null);
    };
    const zu = () => setAktiv(null);
    document.addEventListener("pointerdown", weg);
    window.addEventListener("resize", zu);
    // Scrollt ein Container (auch waagerecht), stimmt der beim Oeffnen
    // gemessene sichtbare Bereich nicht mehr — dann schliessen (QS/Codex
    // 19.09.2026). `capture`, weil scroll nicht hochblubbert.
    document.addEventListener("scroll", zu, true);
    return () => {
      document.removeEventListener("pointerdown", weg);
      window.removeEventListener("resize", zu);
      document.removeEventListener("scroll", zu, true);
    };
  }, [aktiv]);
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
  const contGenutzt = contingencyGenutzt(sprit);
  const imFlug = {
    taxi: sprit.rollen_vor_start
      ? t("landing.sprit.imflug_taxi", { ist: kg(sprit.rollen_vor_start.ist_kg), plan: kg(sprit.rollen_vor_start.plan_kg) })
      : null,
    trip:
      bis && an
        ? t("landing.sprit.imflug_trip", { bis: kg(bis.ist_kg), bisPlan: kg(bis.plan_kg), an: kg(an.ist_kg), anPlan: kg(an.plan_kg) })
        : null,
    contingency:
      contGenutzt == null
        ? null
        : contGenutzt <= 0
          ? t("landing.sprit.imflug_cont_unberuehrt")
          : contGenutzt >= l.contingency_kg
            ? t("landing.sprit.imflug_cont_ganz", { c: kg(l.contingency_kg) })
            : t("landing.sprit.imflug_cont_teil", { n: kg(contGenutzt), c: kg(l.contingency_kg) }),
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
    <div ref={leiterRef} style={{ marginTop: "0.8rem" }}>
      <div style={{ fontSize: "0.86rem", fontWeight: 600, color: "var(--text, #e8ecf3)", marginBottom: 6 }}>
        {t("landing.sprit.leiter_titel")} · {kg(l.block_kg)} kg
      </div>
      <div ref={balkenRef} style={{ position: "relative" }}>
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
            onMouseEnter={() => zeige(r.label)}
            onMouseLeave={() => setAktiv(null)}
          />
        ))}
        {/* Die Abhebe-Marke wird IMMER gezeichnet. Bis v1.7.35 verschwand
            sie bei kleinem Abstand zur Landemarke ganz — ohne Hinweis, und
            damit brach die zugesagte Eigenschaft „der Abstand ist der
            Verbrauch" still. */}
        {abhebenX != null && (
          <line x1={abhebenX} y1={0} x2={abhebenX} y2={30} stroke={leise} strokeWidth={1.5} strokeDasharray="3 2" vectorEffect="non-scaling-stroke" pointerEvents="none" />
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
        // Mitte des Blocks im Fenster; der Hinweis wird dort zentriert und
        // so geklemmt, dass er ganz im Bild bleibt.
        const mitteImFenster = balken ? balken.links + ((r.x + r.w / 2) / W) * balken.breite : null;
        const lage = balken && mitteImFenster != null ? hinweisLage(balken.links, 340, mitteImFenster, balken.bereich) : null;
        return (
          <div
            id={hinweisId}
            role="tooltip"
            data-testid="sprit-leiter-hinweis"
            style={{
              ...HINWEIS_STIL,
              bottom: "calc(100% + 8px)",
              left: lage ? lage.versatz : 0,
              width: "max-content",
              maxWidth: lage ? lage.breite : 340,
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
            aria-describedby={aktiv === r.label ? hinweisId : undefined}
            onMouseEnter={() => zeige(r.label)}
            onMouseLeave={() => setAktiv(null)}
            onFocus={() => zeige(r.label)}
            onBlur={() => setAktiv(null)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setAktiv(null);
            }}
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
          <Erklaerung text={t("landing.sprit.leiter_hint_abgehoben")} style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <span aria-hidden style={{ width: 0, height: 12, borderLeft: `2px dashed ${leise}`, flex: "none" }} />
            {sprit.einstieg_in_der_luft
              ? t("landing.sprit.eingestiegen_mit", { kg: kg(abhebenKg) })
              : t("landing.sprit.abgehoben_mit", { kg: kg(abhebenKg) })}
          </Erklaerung>
        )}
        {markX != null && (
          <Erklaerung text={t("landing.sprit.leiter_hint_gelandet")} style={{ display: "inline-flex", alignItems: "center", gap: 6, color: "var(--text, #e8ecf3)" }}>
            <span aria-hidden style={{ width: 0, height: 12, borderLeft: `2px solid ${markFarbe}`, flex: "none" }} />
            {t("landing.sprit.gelandet_mit", { kg: kg(ldg) })}
          </Erklaerung>
        )}
        <span>
          {/* Ist nichts bekannt (Tank unplausibel, kein Landesprit), sagt die
              Zeile NICHTS zur Contingency — bis v1.7.38 stand dann
              „unberührt" da (QS v1.7.38, F1). */}
          {[
            contGenutzt == null
              ? null
              : l.contingency_kg <= 0
                ? t("landing.sprit.contingency_keine")
                : contGenutzt <= 0
                  ? t("landing.sprit.contingency_unberuehrt")
                  : contGenutzt >= l.contingency_kg
                    ? t("landing.sprit.contingency_ganz", { c: kg(l.contingency_kg) })
                    : t("landing.sprit.contingency_teil", { n: kg(contGenutzt), c: kg(l.contingency_kg) }),
            sprit.alternate_und_reserve_intakt === true
              ? t("landing.sprit.alt_res_intakt")
              : sprit.alternate_und_reserve_intakt === false
                ? t("landing.sprit.alt_res_angegriffen")
                : null,
          ]
            .filter(Boolean)
            .join(" · ")}
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
    // Die Erklaerung erscheint sofort und auch beim Tippen — siehe
    // `Erklaerung`. Fehlt der Wert, steht dort der GRUND.
    <Erklaerung
      als="div"
      breit
      text={leer ? grundWennLeer : erklaerung}
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
    </Erklaerung>
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
      ? t("landing.sprit.zeit_wert", { min: minuten(sprit.zeit_unter_schwelle_min), ft: kg(sprit.schwelle_ft) }) +
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
        <Erklaerung text={t("landing.sprit.info")} style={{ color: "var(--text-muted, #9aa4b2)", fontSize: "0.8rem" }}>
          ⓘ
        </Erklaerung>
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
          // v1.7.38: Die Kachel nennt die Hoehe selbst („Unter 8 362 ft")
          // statt „Unter Schwelle" — das erklaerte sich nicht (Thomas).
          label={sprit.schwelle_ft != null ? t("landing.sprit.unter_ft", { ft: kg(sprit.schwelle_ft) }) : t("landing.sprit.unter_schwelle")}
          wert={sprit.zeit_unter_schwelle_min != null ? minuten(sprit.zeit_unter_schwelle_min) : null}
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
      {/* v1.7.40: Sprit je Wegpunkt — dieselbe Tabelle wie im Cockpit,
          mit der Auswertung eingefroren. Fehlt bei Altbestand. */}
      {sprit.wegpunkte && sprit.wegpunkte.length >= 2 && (
        <div style={{ marginTop: "0.8rem" }}>
          <SpritWegpunkte zeilen={sprit.wegpunkte} />
        </div>
      )}
      <div style={{ marginTop: "0.6rem" }}>
        <Zeile label={t("landing.sprit.reserve_label")} ton={reserveTon}>
          {reserveZeile}
        </Zeile>
        {sprit.extra_getankt_kg != null && sprit.extra_genutzt_kg != null && sprit.extra_ungenutzt_kg != null && (
          <Zeile label={t("landing.sprit.extra_label")}>
            {sprit.extra_getankt_kg <= 0
              ? t("landing.sprit.extra_keins")
              : t("landing.sprit.extra_zeile", {
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
