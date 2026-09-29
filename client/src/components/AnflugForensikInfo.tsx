// Gleitpfad und Anflugruhe — Lernpaket AP4/AP5 (29.09.2026).
//
// Reine Info-Zeilen unter der Approach-Stability-Card. Die Werte rechnet
// das Backend (`src-tauri/src/anflug_forensik.rs`) EINMAL; hier wird nur
// angezeigt, nichts nachgerechnet und nichts eingefärbt. Keine Note, kein
// Ampelband — im Sinne des Bordbuchs: einordnen, nie tadeln.

import { useTranslation } from "react-i18next";

export interface GleitpfadTor {
  proben: number;
  mittel_abs_dots: number;
  max_dots: number;
  max_abw_ft: number;
}

export interface AnflugGleitpfad {
  quelle: string;
  winkel_deg: number;
  tch_ft: number;
  tch_angenommen?: boolean;
  schwellenhoehe_ft?: number | null;
  versatz_ft?: number;
  grund_ohne_werte?: string | null;
  gesamt?: GleitpfadTor | null;
  tor_1000_500?: GleitpfadTor | null;
  tor_500_200?: GleitpfadTor | null;
}

export interface RuheTor {
  proben: number;
  dauer_s: number;
  pfad_vorzeichenwechsel?: number | null;
  nick_unruhe_deg_s?: number | null;
  roll_unruhe_deg_s?: number | null;
  schub_umkehr_pro_min?: number | null;
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

const P = "landing.anflug_forensik";

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

  const torText = (schluessel: "tor_1000_500" | "tor_500_200", tor: GleitpfadTor) =>
    t(`${P}.gleitpfad_tor`, {
      tor: t(`${P}.${schluessel}`),
      mittel: zahl(tor.mittel_abs_dots, 1),
      max: mitVorzeichen(tor.max_dots, 1),
    });

  const ruheText = (schluessel: "tor_1000_500" | "tor_500_200", tor: RuheTor) => {
    const teile: string[] = [];
    if (tor.pfad_vorzeichenwechsel != null) {
      teile.push(t(`${P}.ruhe_seitenwechsel`, { n: tor.pfad_vorzeichenwechsel }));
    }
    if (tor.nick_unruhe_deg_s != null) {
      teile.push(t(`${P}.ruhe_nick`, { v: zahl(tor.nick_unruhe_deg_s, 1) }));
    }
    if (tor.roll_unruhe_deg_s != null) {
      teile.push(t(`${P}.ruhe_roll`, { v: zahl(tor.roll_unruhe_deg_s, 1) }));
    }
    if (tor.schub_umkehr_pro_min != null) {
      teile.push(t(`${P}.ruhe_schub`, { v: zahl(tor.schub_umkehr_pro_min, 1) }));
    }
    if (teile.length === 0) return null;
    return `${t(`${P}.${schluessel}`)}: ${teile.join(" · ")}`;
  };

  const ruheZeilen = ruhe
    ? ([
        ruhe.tor_1000_500 ? ruheText("tor_1000_500", ruhe.tor_1000_500) : null,
        ruhe.tor_500_200 ? ruheText("tor_500_200", ruhe.tor_500_200) : null,
      ].filter((z): z is string => z != null))
    : [];
  const schubFehlt =
    ruhe != null &&
    [ruhe.tor_1000_500, ruhe.tor_500_200].some((x) => x != null) &&
    [ruhe.tor_1000_500, ruhe.tor_500_200].every(
      (x) => x == null || x.schub_umkehr_pro_min == null,
    );

  const g = gleitpfad;
  return (
    <section
      className="landing-section landing-anflug-forensik"
      data-testid="anflug-forensik"
    >
      <h3 style={{ margin: "0 0 4px", fontSize: "0.95rem" }}>{t(`${P}.titel`)}</h3>
      <div style={{ fontSize: "0.74rem", opacity: 0.6, marginBottom: 8 }}>
        {t(`${P}.ohne_note`)}
      </div>

      {g && (
        <div style={{ fontSize: "0.84rem", lineHeight: 1.5, marginBottom: 8 }}>
          {g.gesamt ? (
            <>
              <div>
                {t(`${P}.gleitpfad_gesamt`, {
                  mittel: zahl(g.gesamt.mittel_abs_dots, 1),
                  max: mitVorzeichen(g.gesamt.max_dots, 1),
                  ft: mitVorzeichen(g.gesamt.max_abw_ft, 0),
                  proben: g.gesamt.proben,
                })}
              </div>
              {g.tor_1000_500 && <div>{torText("tor_1000_500", g.tor_1000_500)}</div>}
              {g.tor_500_200 && <div>{torText("tor_500_200", g.tor_500_200)}</div>}
            </>
          ) : (
            g.grund_ohne_werte && (
              <div style={{ opacity: 0.78 }}>{t(`${P}.grund.${g.grund_ohne_werte}`)}</div>
            )
          )}
          <div style={{ fontSize: "0.74rem", opacity: 0.6 }}>
            {g.quelle === "angenommen_3grad"
              ? t(`${P}.bezug_ohne_navdaten`)
              : t(g.tch_angenommen ? `${P}.bezug_tch_angenommen` : `${P}.bezug`, {
                  quelle: t(`${P}.quelle.${g.quelle}`),
                  winkel: zahl(g.winkel_deg, 1),
                  tch: zahl(g.tch_ft, 0),
                })}
          </div>
        </div>
      )}

      {ruheZeilen.length > 0 && (
        <div style={{ fontSize: "0.84rem", lineHeight: 1.5 }}>
          <div style={{ fontWeight: 600 }}>{t(`${P}.ruhe_titel`)}</div>
          {ruheZeilen.map((z) => (
            <div key={z}>{z}</div>
          ))}
          {schubFehlt && (
            <div style={{ fontSize: "0.74rem", opacity: 0.6 }}>{t(`${P}.ruhe_schub_fehlt`)}</div>
          )}
        </div>
      )}
    </section>
  );
}
