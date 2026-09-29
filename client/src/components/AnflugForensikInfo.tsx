// Gleitpfad und Anflugruhe — Lernpaket AP4/AP5 (29.09.2026).
//
// Reine Info-Zeilen unter der Approach-Stability-Card. Die Werte rechnet
// das Backend (`src-tauri/src/anflug_forensik.rs`) EINMAL; hier wird nur
// angezeigt, nichts nachgerechnet und nichts eingefärbt. Keine Note, kein
// Ampelband — im Sinne des Bordbuchs: einordnen, nie tadeln.
//
// ⚠ Diese Datei läuft in ZWEI Welten: im Pilot-Client (LandingPanel, Daten
// aus dem LandingRecord) und in der Webapp auf live.kant.ovh (Daten aus dem
// PIREP-Payload, Felder `anflug_gleitpfad` / `anflug_ruhe`). Kanonisch ist
// der Client, die Webapp bekommt eine Kopie über `scripts/anzeige-sync.mjs`.
// Deshalb: keine Client-Importe, nur React + react-i18next. Der Abgleich
// liest die Schlüssel aus dem Quelltext; er erkennt zwei Formen, und nur
// diese stehen hier:
//   * Literale: `t("landing.anflug_forensik.titel")`
//   * ein literaler Vorspann mit EINER Einsetzung am Ende:
//     `t(\`landing.anflug_forensik.quelle.${…}\`)` (ebenso `grund.`,
//     `schub_grund.`) — abgeglichen wird dann jeder Eintrag unter dem
//     Vorspann. Eine Variable im Vorspann selbst sähe er nicht.

import { useTranslation } from "react-i18next";

export interface GleitpfadTor {
  proben: number;
  mittel_abs_dots: number;
  max_dots: number;
  max_abw_ft: number;
  oberste_hoehe_ft?: number | null;
}

export interface AnflugGleitpfad {
  quelle: string;
  winkel_deg?: number | null;
  tch_ft?: number | null;
  tch_angenommen?: boolean;
  grad_je_dot?: number | null;
  hoehenbezug?: string | null;
  schwellenhoehe_navigraph_ft?: number | null;
  sim_boden_ft?: number | null;
  versatz_ft?: number | null;
  grund_ohne_werte?: string | null;
  gesamt?: GleitpfadTor | null;
  tor_1000_500?: GleitpfadTor | null;
  tor_500_200?: GleitpfadTor | null;
}

export interface RuheTor {
  proben: number;
  dauer_s: number;
  oberste_hoehe_ft?: number | null;
  pfad_vorzeichenwechsel?: number | null;
  nick_unruhe_deg_s?: number | null;
  roll_unruhe_deg_s?: number | null;
  schub_umkehr_pro_min?: number | null;
  schub_grund?: string | null;
}

export interface AnflugRuhe {
  hoehenbezug: string;
  tor_1000_500?: RuheTor | null;
  tor_500_200?: RuheTor | null;
}

interface Props {
  gleitpfad?: AnflugGleitpfad | null;
  ruhe?: AnflugRuhe | null;
}

type TorName = "tor_1000_500" | "tor_500_200";

/** Obere Grenze je Band — liegt die oberste Probe deutlich darunter, war
 *  der Puffer zu kurz oder der Anflug begann tiefer; das wird genannt. */
const TOR_OBEN_FT: Record<TorName | "gesamt", number> = {
  gesamt: 1000,
  tor_1000_500: 1000,
  tor_500_200: 500,
};
/** Proben liegen ~1 s auseinander (≈ 10–15 ft); 50 ft Luft. */
const ERFASST_TOLERANZ_FT = 50;

export function AnflugForensikInfo({ gleitpfad, ruhe }: Props) {
  const { t, i18n } = useTranslation();
  if (!gleitpfad && !ruhe) return null;

  const zahl = (v: number, stellen: number) =>
    new Intl.NumberFormat(i18n.language, {
      minimumFractionDigits: stellen,
      maximumFractionDigits: stellen,
    }).format(v);
  // Vorzeichen immer zeigen: „+" heißt über dem Pfad, „−" darunter.
  const mitVorzeichen = (v: number, stellen: number) => {
    const text = zahl(Math.abs(v), stellen);
    if (Math.abs(v) < 0.5 * 10 ** -stellen) return text;
    return (v > 0 ? "+" : "−") + text;
  };
  const torName = (tor: TorName) =>
    tor === "tor_1000_500"
      ? t("landing.anflug_forensik.tor_1000_500")
      : t("landing.anflug_forensik.tor_500_200");
  const erfasstAb = (band: TorName | "gesamt", oberste?: number | null) =>
    oberste != null && oberste < TOR_OBEN_FT[band] - ERFASST_TOLERANZ_FT
      ? " " + t("landing.anflug_forensik.erfasst_ab", { h: zahl(oberste, 0) })
      : "";

  const torText = (name: TorName, tor: GleitpfadTor) =>
    t("landing.anflug_forensik.gleitpfad_tor", {
      tor: torName(name),
      mittel: zahl(tor.mittel_abs_dots, 1),
      max: mitVorzeichen(tor.max_dots, 1),
    }) + erfasstAb(name, tor.oberste_hoehe_ft);

  const ruheText = (name: TorName, tor: RuheTor) => {
    const teile: string[] = [];
    if (tor.pfad_vorzeichenwechsel != null) {
      teile.push(
        t("landing.anflug_forensik.ruhe_seitenwechsel", { n: tor.pfad_vorzeichenwechsel }),
      );
    }
    if (tor.nick_unruhe_deg_s != null) {
      teile.push(t("landing.anflug_forensik.ruhe_nick", { v: zahl(tor.nick_unruhe_deg_s, 1) }));
    }
    if (tor.roll_unruhe_deg_s != null) {
      teile.push(t("landing.anflug_forensik.ruhe_roll", { v: zahl(tor.roll_unruhe_deg_s, 1) }));
    }
    if (tor.schub_umkehr_pro_min != null) {
      teile.push(
        t("landing.anflug_forensik.ruhe_schub", { v: zahl(tor.schub_umkehr_pro_min, 1) }),
      );
    } else if (tor.schub_grund) {
      teile.push(t(`landing.anflug_forensik.schub_grund.${tor.schub_grund}`));
    }
    if (teile.length === 0) return null;
    return `${torName(name)}: ${teile.join(" · ")}${erfasstAb(name, tor.oberste_hoehe_ft)}`;
  };

  const ruheZeilen = ruhe
    ? [
        ruhe.tor_1000_500 ? ruheText("tor_1000_500", ruhe.tor_1000_500) : null,
        ruhe.tor_500_200 ? ruheText("tor_500_200", ruhe.tor_500_200) : null,
      ].filter((z): z is string => z != null)
    : [];

  const g = gleitpfad;
  // Ohne Bahn speichert das Backend keinen Winkel und keine TCH — dann
  // gibt es auch keinen Bezug zu nennen.
  const hatBezug = g != null && g.winkel_deg != null && g.tch_ft != null;
  const bezugWerte = hatBezug
    ? {
        quelle: t(`landing.anflug_forensik.quelle.${g.quelle}`),
        winkel: zahl(g.winkel_deg as number, 1),
        tch: zahl(g.tch_ft as number, 0),
      }
    : null;
  const dot = g?.grad_je_dot ?? 0.35;
  return (
    <section className="landing-section landing-anflug-forensik" data-testid="anflug-forensik">
      <h3 style={{ margin: "0 0 4px", fontSize: "0.95rem" }}>
        {t("landing.anflug_forensik.titel")}
      </h3>
      <div style={{ fontSize: "0.74rem", opacity: 0.6, marginBottom: 8 }}>
        {t("landing.anflug_forensik.ohne_note", { dot: zahl(dot, 2) })}
      </div>

      {g && (
        <div style={{ fontSize: "0.84rem", lineHeight: 1.5, marginBottom: 8 }}>
          {g.gesamt ? (
            <>
              <div>
                {t("landing.anflug_forensik.gleitpfad_gesamt", {
                  mittel: zahl(g.gesamt.mittel_abs_dots, 1),
                  max: mitVorzeichen(g.gesamt.max_dots, 1),
                  ft: mitVorzeichen(g.gesamt.max_abw_ft, 0),
                  proben: g.gesamt.proben,
                }) + erfasstAb("gesamt", g.gesamt.oberste_hoehe_ft)}
              </div>
              {g.tor_1000_500 && <div>{torText("tor_1000_500", g.tor_1000_500)}</div>}
              {g.tor_500_200 && <div>{torText("tor_500_200", g.tor_500_200)}</div>}
            </>
          ) : (
            g.grund_ohne_werte && (
              <div style={{ opacity: 0.78 }}>
                {t(`landing.anflug_forensik.grund.${g.grund_ohne_werte}`)}
              </div>
            )
          )}
          <div style={{ fontSize: "0.74rem", opacity: 0.6 }}>
            {bezugWerte == null
              ? t("landing.anflug_forensik.bezug_ohne_navdaten")
              : g.tch_angenommen
                ? t("landing.anflug_forensik.bezug_tch_angenommen", bezugWerte)
                : t("landing.anflug_forensik.bezug", bezugWerte)}
          </div>
          {g.hoehenbezug === "sim_boden" && g.sim_boden_ft != null && (
            <div style={{ fontSize: "0.74rem", opacity: 0.6 }}>
              {g.schwellenhoehe_navigraph_ft != null
                ? t("landing.anflug_forensik.hinweis_sim_boden", {
                    sim: zahl(g.sim_boden_ft, 0),
                    nav: zahl(g.schwellenhoehe_navigraph_ft, 0),
                  })
                : t("landing.anflug_forensik.hinweis_sim_boden_ohne_navdaten", {
                    sim: zahl(g.sim_boden_ft, 0),
                  })}
            </div>
          )}
        </div>
      )}

      {ruheZeilen.length > 0 && (
        <div style={{ fontSize: "0.84rem", lineHeight: 1.5 }}>
          <div style={{ fontWeight: 600 }}>{t("landing.anflug_forensik.ruhe_titel")}</div>
          {ruheZeilen.map((z) => (
            <div key={z}>{z}</div>
          ))}
        </div>
      )}
    </section>
  );
}
