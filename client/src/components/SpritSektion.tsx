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
import { useTranslation } from "react-i18next";
import type { SpritAuswertung, SpritPhase } from "../lib/sprit";
import { balken, dezimal, hauptzahl, hauptzahlText, kg, pct, phaseTon } from "../lib/sprit";

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
  const l = sprit.leiter;
  if (!l || l.block_kg <= 0) return null;
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
  const parts: Array<[number, string, string]> = [
    [l.taxi_kg, "#6b7688", t("landing.sprit.leiter_taxi")],
    [l.trip_kg, "#38bdf8", t("landing.sprit.leiter_trip")],
    [l.contingency_kg, "#f2b24c", t("landing.sprit.leiter_contingency")],
    // Nur das Extra, das an Bord war — die Untertankung kommt fertig aus
    // der Rechnung (`sprit.rs`, Leiter).
    [Math.max(l.extra_kg - (l.untertankung_kg ?? 0), 0), "#3d5a6c", t("landing.sprit.leiter_extra")],
    [l.sonstiges_kg ?? 0, "#5a5f7a", t("landing.sprit.leiter_sonstiges")],
    [l.uebertankung_kg ?? 0, "#47705a", t("landing.sprit.leiter_uebertankung")],
    [l.alternate_kg, "#566273", t("landing.sprit.leiter_alternate")],
    [l.reserve_kg, "#566273", t("landing.sprit.leiter_reserve")],
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
    .map(([kgWert, fill, label]) => {
      const w = x(kgWert);
      const r = { x: cursor, w, fill, label, kg: kgWert };
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
  return (
    <svg viewBox={`0 0 ${W} 100`} role="img" aria-label={t("landing.sprit.leiter_titel")} style={{ display: "block", maxWidth: "100%", height: "auto", marginTop: "0.6rem" }}>
      <text x="0" y="11" style={{ font: "600 11px system-ui, sans-serif", fill: "var(--text, #e8ecf3)" }}>
        {t("landing.sprit.leiter_titel")} · {kg(l.block_kg)} kg
      </text>
      {rects.map((r) => (
        <rect key={r.label} x={r.x} y={20} width={Math.max(r.w, 0)} height={22} fill={r.fill} stroke="var(--border, #2a3344)" strokeWidth={0.5} />
      ))}
      {rects
        .filter((r) => {
          // 10.5px Monospace ≈ 6.3px je Zeichen — nur beschriften, wenn der
          // Text ins Segment passt und im Bild bleibt.
          const breite = (`${r.label} ${kg(r.kg)}`).length * 6.3;
          return r.w >= breite + 6 && r.x + 3 + breite <= W;
        })
        .map((r) => (
          <text key={`t-${r.label}`} x={r.x + 3} y={56} style={{ font: "10.5px ui-monospace, monospace", fill: "var(--text-muted, #9aa4b2)" }}>
            {r.label} {kg(r.kg)}
          </text>
        ))}
      {/* Die Abhebe-Marke wird IMMER gezeichnet. Bis v1.7.35 verschwand sie
          bei kleinem Abstand zur Landemarke ganz — ohne Hinweis, und damit
          brach die zugesagte Eigenschaft „der Abstand ist der Verbrauch"
          still: Wo nichts steht, liest niemand einen Abstand ab. Bei kurzem
          Flug liegen die Marken eben dicht beieinander; das IST die
          Aussage. */}
      {abhebenX != null && (
        <>
          <line x1={abhebenX} y1={18} x2={abhebenX} y2={44} stroke="var(--text-muted, #9aa4b2)" strokeWidth={1} strokeDasharray="3 2" />
          {/* Unter die Leiter statt daneben: der Abhebe-Tankstand liegt nah
              am Block, die Marke also weit links — auf y=14 kollidierte der
              Text mit dem Leiter-Titel. */}
          <text x={Math.min(abhebenX + 4, W - 120)} y={68} style={{ font: "10px ui-monospace, monospace", fill: "var(--text-muted, #9aa4b2)" }}>
            {sprit.einstieg_in_der_luft
              ? t("landing.sprit.eingestiegen_mit", { kg: kg(abhebenKg) })
              : t("landing.sprit.abgehoben_mit", { kg: kg(abhebenKg) })}
          </text>
        </>
      )}
      {markX != null && (
        <>
          <line x1={markX} y1={16} x2={markX} y2={46} stroke={markFarbe} strokeWidth={2} />
          <text x={Math.min(markX + 5, W - 210)} y={80} style={{ font: "600 11px system-ui, sans-serif", fill: "var(--text, #e8ecf3)" }}>
            ▲ {t("landing.sprit.gelandet_mit", { kg: kg(ldg) })}
          </text>
        </>
      )}
      <text x="0" y="94" style={{ font: "10.5px ui-monospace, monospace", fill: "var(--text-muted, #9aa4b2)" }}>
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
export function SpritSektion({ sprit }: { sprit: SpritAuswertung | null | undefined }) {
  const { t } = useTranslation();
  if (!sprit) return null;
  const r = sprit.reserve;
  const reserveZeile =
    r.status === "intakt"
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
        <b>{t("landing.sprit.title")}</b>
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
          wert={r.status === "nicht_pruefbar" ? null : String(Math.round(r.quote_pct))}
          einheit="%"
          ton={r.status === "intakt" ? "ok" : r.status === "unterschritten" ? "warn" : "neutral"}
          erklaerung={t("landing.sprit.hint_reserve")}
          grundWennLeer={t("landing.sprit.na_reserve")}
        />
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
      </div>
      {sprit.rollen_nach_landung_kg != null && (
        <div style={{ fontSize: "0.82rem", color: "var(--text-muted, #9aa4b2)", marginTop: "0.4rem" }}>
          {t("landing.sprit.rollen_nach_landung", { kg: kg(sprit.rollen_nach_landung_kg) })}
        </div>
      )}
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
