import { useCallback, useEffect, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";

interface MetarSnapshot {
  icao: string;
  raw: string;
  time: string;
  wind_direction_deg: number | null;
  wind_speed_kt: number | null;
  gust_kt: number | null;
  visibility_m: number | null;
  temperature_c: number | null;
  dewpoint_c: number | null;
  qnh_hpa: number | null;
}

interface MetarFetchState {
  kind: "loading" | "ready" | "error";
  data?: MetarSnapshot;
  error?: string;
}

interface Props {
  /** Departure airport ICAO. */
  dptIcao: string;
  /** Arrival airport ICAO. */
  arrIcao: string;
}

/** Auto-refresh window for the briefing panel. NOAA observations are
 *  reissued at most ~hourly, so polling more often is wasteful — we hit
 *  it once on mount and let the user click Refresh when they want a
 *  fresh look. */
function fmtVisibilityKm(meters: number | null): string {
  if (meters == null) return "—";
  const km = meters / 1000;
  // v0.3.0: aviation-relevant ≥ 9.5 km wird als "≥ 10 km" angezeigt
  // (war 10.0 — das hat 9.999 km als "10.0 km" gerendert was identisch
  // aussieht aber den CAVOK-Indikator unterdrückt). Aviation-Konvention
  // ist: 9999 m = "10 km oder mehr".
  return km >= 9.5 ? "≥ 10 km" : `${km.toFixed(1)} km`;
}

/**
 * Sichtweite aus dem METAR-Rohtext fischen, wenn der Backend-Parser
 * sie nicht geliefert hat (`visibility_m === null`). Real-life-Fälle
 * 2026-05: "9999" = ≥ 10 km, "CAVOK" = visibility ≥ 10 km + no clouds
 * unter 5000 ft, "10SM" = 10 statute miles, "1500" = 1500 m. Wir
 * parsen nur die häufigsten Tokens.
 */
function fmtVisibilityFromRaw(raw: string | null): string {
  if (!raw) return "—";
  if (/\bCAVOK\b/.test(raw)) return "CAVOK";
  // 4-stelliges m-Format wie "9999" oder "1500" — nach dem WIND-Token.
  const mMatch = raw.match(/\b(\d{4})\b/);
  if (mMatch) {
    const m = Number.parseInt(mMatch[1]!, 10);
    if (m === 9999) return "≥ 10 km";
    return `${(m / 1000).toFixed(1)} km`;
  }
  // US-Format "10SM"
  const sm = raw.match(/\b(\d+)SM\b/);
  if (sm) return `${sm[1]} SM`;
  return "—";
}

/**
 * Wetter-Phänomene aus METAR-WX-Codes ableiten (RA = Regen, SN = Schnee,
 * TS = Gewitter, FG = Nebel, etc.) plus Bewölkungs-Indikator.
 * Liefert ein kompaktes Icon + kurze Beschreibung — z.B. "🌦 -RA",
 * "⛈ TSRA", "☁ OVC".
 *
 * Real-WX-Codes:
 *   - Intensität: `-` leicht, kein Prefix mässig, `+` stark
 *   - Beschreibungen: SH=Schauer, TS=Gewitter, FZ=gefrierend, BL=blowing
 *   - Niederschlag: RA=Regen, SN=Schnee, GR=Hagel, GS=Graupel, DZ=Niesel
 *   - Sicht: FG=Nebel, BR=Dunst, HZ=Diesig
 *
 * Wenn kein WX-Code: höchste Bewölkungsschicht als Indikator.
 */
function extractWeatherPhenomena(raw: string | null): {
  icon: string;
  label: string;
} | null {
  if (!raw) return null;
  // Pattern: optional ± Intensität, optionale Descriptor, dann WX-Code
  const wxRegex =
    /\b([+-]?)(VC|RE)?(MI|PR|BC|DR|BL|SH|TS|FZ)?(DZ|RA|SN|SG|IC|PL|GR|GS|UP|FG|BR|HZ|FU|VA|DU|SA|PY|SQ|PO|FC|SS|DS)\b/;
  const match = raw.match(wxRegex);
  // CAVOK = "Ceiling And Visibility OK" — explizit "schönes Wetter":
  // Sicht ≥ 10 km, keine Wolken < 5000 ft, keine signifikanten WX-
  // Phänomene. Wir behandeln das als eigenes Top-Level-Signal weil
  // im METAR-Text keine Cloud-Layer-Codes folgen.
  if (/\bCAVOK\b/.test(raw)) {
    return { icon: "☀", label: "CAVOK" };
  }
  if (match) {
    const intensity = match[1] || "";
    const descriptor = match[3] || "";
    const phenomenon = match[4] || "";
    const code = `${intensity}${descriptor}${phenomenon}`;

    // Icon-Mapping nach Phänomen + Descriptor
    let icon = "🌫"; // default für FG/BR/HZ
    if (descriptor === "TS") icon = "⛈";
    else if (descriptor === "SH") icon = phenomenon === "SN" ? "🌨" : "🌦";
    else if (phenomenon === "RA" || phenomenon === "DZ") icon = "🌧";
    else if (phenomenon === "SN" || phenomenon === "SG") icon = "❄";
    else if (phenomenon === "GR" || phenomenon === "GS" || phenomenon === "PL")
      icon = "🌨";
    else if (phenomenon === "FG") icon = "🌫";
    else if (phenomenon === "BR" || phenomenon === "HZ") icon = "🌁";
    else if (phenomenon === "TS") icon = "⛈";
    return { icon, label: code };
  }
  // Kein WX-Code → Bewölkung als Indikator
  if (/\bSKC\b|\bCLR\b|\bNCD\b|\bNSC\b/.test(raw)) {
    return { icon: "☀", label: "klar" };
  }
  if (/\bOVC\d/.test(raw)) return { icon: "☁", label: "OVC" };
  if (/\bBKN\d/.test(raw)) return { icon: "🌥", label: "BKN" };
  if (/\bSCT\d/.test(raw)) return { icon: "⛅", label: "SCT" };
  if (/\bFEW\d/.test(raw)) return { icon: "🌤", label: "FEW" };
  return null;
}

function fmtWind(
  direction: number | null,
  speed: number | null,
  gust: number | null,
): string {
  if (speed == null || speed === 0) return "Calm";
  const dir = direction == null ? "VRB" : `${direction.toFixed(0).padStart(3, "0")}°`;
  const gustPart = gust && gust > 0 ? `G${gust.toFixed(0)}` : "";
  return `${dir} / ${speed.toFixed(0)}${gustPart} kt`;
}

/**
 * Kompakte Inline-Zeile pro Airport (v0.3.0):
 *   ABFLUG  EDDW  300°/5 kt  9999  18°/11°  1013 hPa  [▸ METAR]
 *
 * Die wichtigsten Werte (Wind / Sicht / Temp+Dew / QNH) inline,
 * der vollständige METAR-Text aufklappbar. Spart ~80 % Höhe gegenüber
 * der alten 2-Card-mit-Sub-Grid-Variante (die WIND/SICHT/TEMP/QNH
 * sowohl als Sub-Grid als auch implizit im METAR-Text hatte).
 */
function MetarRow({
  label,
  state,
}: {
  label: string;
  state: MetarFetchState;
}) {
  const { t } = useTranslation();
  const [showRaw, setShowRaw] = useState(false);

  if (state.kind === "loading") {
    return (
      <div className="weather-row weather-row--loading">
        <span className="weather-row__label">{label}</span>
        <span className="weather-row__hint">{t("weather.loading")}</span>
      </div>
    );
  }
  if (state.kind === "error") {
    return (
      <div className="weather-row weather-row--error">
        <span className="weather-row__label">{label}</span>
        <span className="weather-row__hint">
          {state.error ?? t("weather.error")}
        </span>
      </div>
    );
  }
  const m = state.data!;
  // Sicht: Backend liefert manchmal null (Parser ignoriert "9999" oder
  // CAVOK). Fallback aus dem Raw-METAR parsen damit der Pilot wenigstens
  // die wichtigsten Sichtwerte ("≥ 10 km" / "CAVOK") sieht.
  const visibilityLabel =
    m.visibility_m != null
      ? fmtVisibilityKm(m.visibility_m)
      : fmtVisibilityFromRaw(m.raw);
  // Wetter-Phänomene: aus METAR-Rawtext extrahieren + Icon-Mapping.
  // Beispiele: 🌦 -SHRA (leichter Regenschauer), ⛈ TSRA (Gewitterregen),
  // 🌫 FG (Nebel), ☁ OVC (bedeckt). Wenn nichts erkannt → kein Element.
  const wx = extractWeatherPhenomena(m.raw);
  return (
    <div className="weather-row">
      <span className="weather-row__label">{label}</span>
      <span className="weather-row__icao">{m.icao}</span>
      <span className="weather-row__cell" title="Wind">
        {fmtWind(m.wind_direction_deg, m.wind_speed_kt, m.gust_kt)}
      </span>
      <span className="weather-row__sep" aria-hidden="true">·</span>
      <span className="weather-row__cell" title="Sicht / Visibility">
        👁 {visibilityLabel}
      </span>
      <span className="weather-row__sep" aria-hidden="true">·</span>
      <span className="weather-row__cell" title="Temperatur / Taupunkt">
        {m.temperature_c != null ? `${m.temperature_c.toFixed(0)}°` : "—"}
        {" / "}
        {m.dewpoint_c != null ? `${m.dewpoint_c.toFixed(0)}°` : "—"}
      </span>
      <span className="weather-row__sep" aria-hidden="true">·</span>
      <span className="weather-row__cell" title="QNH / Druck">
        {m.qnh_hpa != null ? `${m.qnh_hpa.toFixed(0)} hPa` : "—"}
      </span>
      {wx && (
        <span
          className="weather-row__wx"
          title={`Wetterphänomen: ${wx.label}`}
        >
          {wx.icon} {wx.label}
        </span>
      )}
      <button
        type="button"
        className="weather-row__toggle"
        onClick={() => setShowRaw((v) => !v)}
        aria-expanded={showRaw}
      >
        {showRaw ? "▾" : "▸"} METAR
      </button>
      {showRaw && m.raw && (
        <pre className="weather-row__raw">{m.raw}</pre>
      )}
    </div>
  );
}

export function WeatherBriefing({ dptIcao, arrIcao }: Props) {
  const { t } = useTranslation();
  const [dpt, setDpt] = useState<MetarFetchState>({ kind: "loading" });
  const [arr, setArr] = useState<MetarFetchState>({ kind: "loading" });
  const [refreshing, setRefreshing] = useState(false);

  const fetchOne = useCallback(
    async (icao: string, set: (s: MetarFetchState) => void) => {
      set({ kind: "loading" });
      try {
        const data = await invoke<MetarSnapshot>("metar_get", { icao });
        set({ kind: "ready", data });
      } catch (err: unknown) {
        const msg =
          typeof err === "object" && err !== null && "message" in err
            ? String((err as { message: string }).message)
            : String(err);
        set({ kind: "error", error: msg });
      }
    },
    [],
  );

  // Initial load + reload whenever the route changes.
  useEffect(() => {
    if (!dptIcao && !arrIcao) return;
    void (async () => {
      setRefreshing(true);
      await Promise.all([
        dptIcao ? fetchOne(dptIcao, setDpt) : Promise.resolve(),
        arrIcao ? fetchOne(arrIcao, setArr) : Promise.resolve(),
      ]);
      setRefreshing(false);
    })();
  }, [dptIcao, arrIcao, fetchOne]);

  async function handleRefresh() {
    if (refreshing) return;
    setRefreshing(true);
    await Promise.all([
      dptIcao ? fetchOne(dptIcao, setDpt) : Promise.resolve(),
      arrIcao ? fetchOne(arrIcao, setArr) : Promise.resolve(),
    ]);
    setRefreshing(false);
  }

  return (
    <section className="weather-briefing">
      <header className="weather-briefing__header">
        <h2 className="weather-briefing__title">{t("weather.title")}</h2>
        <button
          type="button"
          className="weather-briefing__refresh"
          onClick={() => void handleRefresh()}
          disabled={refreshing}
          aria-label={t("weather.refresh")}
        >
          {refreshing ? "…" : "⟳"} <span>{t("weather.refresh")}</span>
        </button>
      </header>
      <div className="weather-briefing__rows">
        <MetarRow label={t("weather.departure")} state={dpt} />
        <MetarRow label={t("weather.arrival")} state={arr} />
      </div>
    </section>
  );
}
