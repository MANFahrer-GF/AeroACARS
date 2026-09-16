import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { wetterAlter } from "../lib/wetterAlter";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";
import type { MetarSnapshotDto } from "../types";

interface MetarFetchState {
  kind: "loading" | "ready" | "error";
  data?: MetarSnapshotDto;
  error?: string;
}

/**
 * Entscheidet, ob ein neu eingetroffener METAR-Wert den aktuell gezeigten
 * ersetzen darf — die einzige Stelle, an der das entschieden wird
 * (Prefetch-Effekte UND eigener Fetch rufen dieselbe Funktion).
 *
 * Fünfte Codex-Runde: frühere Fassungen entschieden das über die
 * REIHENFOLGE des Eintreffens (Generation-Zähler) statt über die
 * tatsächliche Aktualität der Daten — ein spät auflösender, aber neuerer
 * eigener Fetch konnte von einem früher eingetroffenen, aber älteren
 * Prefetch verworfen werden. Hier zählt ausschliesslich die METAR-
 * Beobachtungszeit selbst (`time`), nicht wer zuerst da war:
 *
 *   - anderer Flughafen (Flugwechsel) → immer übernehmen, kein Vergleich
 *     sinnvoll.
 *   - echt älter (`data.time < prev.data.time`) → verwerfen.
 *   - identisch (`time` UND `raw` gleich) → verwerfen (kein Update, kein
 *     unnötiger Re-Render).
 *   - alles andere (neuer, oder gleicher `time` mit anderem `raw` — siehe
 *     Kommentar an den Aufrufstellen) → übernehmen.
 */
function wendeAn(prev: MetarFetchState, data: MetarSnapshotDto): MetarFetchState {
  if (prev.kind === "ready" && prev.data && prev.data.icao === data.icao) {
    // `time_is_estimated` (Rust-Fallback auf die Abrufzeit, wenn NOAAs
    // obsTime fehlt/unparsbar war — vorgemerkte Datenqualitäts-Aufgabe
    // vom 2026-09-11) macht `time` unzuverlässig für den Aktualitäts-
    // vergleich: ein geschätzter Wert ist praktisch immer "jetzt" und
    // würde einen ECHTEN, aber älter datierten Beobachtungswert sonst
    // fälschlich verdrängen — obwohl beide dieselbe (oder eine ältere)
    // Meldung beschreiben könnten. Ein geschätzter Wert gewinnt deshalb
    // nie gegen einen bereits gezeigten echten.
    if (data.time_is_estimated && !prev.data.time_is_estimated) return prev;
    if (data.time < prev.data.time) return prev;
    if (data.time === prev.data.time && data.raw === prev.data.raw) return prev;
  }
  return { kind: "ready", data };
}

interface Props {
  /** Departure airport ICAO. */
  dptIcao: string;
  /** Arrival airport ICAO. */
  arrIcao: string;
  /**
   * Schon vom Backend geholtes Wetter — `maybe_spawn_metar_fetch` (Rust)
   * fragt es selbstständig bei Boarding/Takeoff (Abflug) und bei Descent/
   * Final (Ziel) ab, unabhängig davon, ob diese Karte gerade offen ist.
   * Michael, Discord 2026-09-11: „kann mit Beginn des Descent das Wetter
   * am Zielflughafen nochmal selbständig aktualisiert werden […] ohne
   * dass ich […] den Button drücken muss." — genau das lief serverseitig
   * schon, nur zeigte diese Karte es nie: Sie holte beim Mount immer ihr
   * eigenes, unabhängiges METAR statt den längst vorhandenen Wert zu
   * lesen. `activeFlight` wird App-weit alle 2 s gepollt (App.tsx), auch
   * wenn das Cockpit-Tab gar nicht offen ist — die Werte landen also
   * automatisch hier, sobald die Karte das nächste Mal gerendert wird,
   * und aktualisieren sich live weiter, solange sie offen bleibt.
   */
  prefetchedDpt?: MetarSnapshotDto | null;
  prefetchedArr?: MetarSnapshotDto | null;
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

/** 1 hPa = 0.0295299830714 inHg. US-Piloten lesen QNH in inHg. */
function fmtQnh(hpa: number | null): string {
  if (hpa == null) return "—";
  const inHg = hpa * 0.0295299830714;
  return `${hpa.toFixed(0)} hPa / ${inHg.toFixed(2)} inHg`;
}

/**
 * Ein Wetter-Card pro Airport (Stage E redesign — .wx card statt der
 * früheren kompakten Inline-Zeile). Kopf: Tag (Departure/Arrival) +
 * ICAO + Wetter-Phänomen-Badge + Aufklapp-Button für den METAR-Rohtext.
 * Zeile: Wind / Sicht / Temp+Dew / QNH inline mit Trennern — exakt
 * dieselben Werte/Formatter wie vorher (fmtWind/fmtVisibilityKm/
 * fmtVisibilityFromRaw/fmtQnh/extractWeatherPhenomena unverändert),
 * nur in der neuen Karten-Optik statt der alten .weather-row-Zeile.
 */
function WxCard({
  label,
  state,
}: {
  label: string;
  state: MetarFetchState;
}) {
  const { t } = useTranslation();
  const [showRaw, setShowRaw] = useState(false);
  // Eigener Takt fuer die Altersangabe: Sie darf nicht daran haengen,
  // dass die Karte aus einem anderen Grund neu zeichnet (unabhaengige
  // QS, 16.09.2026 — sonst friert „vor 6 Min." lautlos ein, sobald der
  // uebergeordnete Poll ausfaellt oder die Karte woanders benutzt wird).
  // 30 s reichen: die Anzeige ist minutengenau.
  const [, takt] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => takt((n) => n + 1), 30_000);
    return () => window.clearInterval(id);
  }, []);

  if (state.kind === "loading") {
    return (
      <div className="card">
        <div className="wx__head">
          <span className="wx__tag">{label}</span>
        </div>
        <div className="wx__row">
          <span className="wx__cell">{t("weather.loading")}</span>
        </div>
      </div>
    );
  }
  if (state.kind === "error") {
    return (
      <div className="card">
        <div className="wx__head">
          <span className="wx__tag">{label}</span>
        </div>
        <div className="wx__row">
          <span className="wx__cell">{state.error ?? t("weather.error")}</span>
        </div>
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
  // v1.7.30: Wie alt ist das, was hier steht? Ohne die Angabe sieht ein
  // zwei Stunden altes METAR genauso aus wie ein frisches.
  //
  // `time_is_estimated` heisst: die Station hat keine brauchbare
  // Beobachtungszeit geliefert, und `time` ist der Zeitpunkt UNSERES
  // Abrufs (siehe MetarSnapshotDto). Dann waere jede Altersangabe eine
  // Behauptung ueber die Frische, die die Daten nicht decken — ein zwei
  // Stunden altes METAR stuende als „gerade eben" da. In dem Fall lieber
  // nichts anzeigen (Codex-QS P1, 16.09.2026).
  const alter = m.time_is_estimated ? null : wetterAlter(m.time, new Date());
  return (
    <div className="card">
      <div className="wx__head">
        <span className="wx__tag">{label}</span>
        <span className="wx__icao">{m.icao}</span>
        {wx && (
          <span className="wx__phen" title={`Wetterphänomen: ${wx.label}`}>
            {wx.icon} {wx.label}
          </span>
        )}
        {alter && (
          <span
            className={`wx__alter${alter.veraltet ? " wx__alter--veraltet" : ""}`}
            style={{ marginLeft: "auto" }}
          >
            {t(alter.text.key, alter.text.werte)}
            {alter.veraltet && <> · {t("weather.age_stale")}</>}
          </span>
        )}
        <button
          type="button"
          className="card__action"
          // `.card__action` traegt `margin-left:auto` — mit der
          // Altersanzeige gaebe es zwei Auto-Abstaende, die den Platz
          // teilen und beide auseinanderziehen (Codex-QS P2).
          style={alter ? { marginLeft: 0 } : { marginLeft: "auto" }}
          onClick={() => setShowRaw((v) => !v)}
          aria-expanded={showRaw}
          title="METAR"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
            <path d="m6 9 6 6 6-6" />
          </svg>
        </button>
      </div>
      <div className="wx__row">
        <span className="wx__cell" title="Wind">
          {fmtWind(m.wind_direction_deg, m.wind_speed_kt, m.gust_kt)}
        </span>
        <span className="wx__sep" aria-hidden="true">·</span>
        <span className="wx__cell" title="Sicht / Visibility">
          <b>VIS</b> {visibilityLabel}
        </span>
        <span className="wx__sep" aria-hidden="true">·</span>
        <span className="wx__cell" title="Temperatur / Taupunkt">
          <b>T/DP</b>{" "}
          {m.temperature_c != null ? `${m.temperature_c.toFixed(0)}°` : "—"}
          {" / "}
          {m.dewpoint_c != null ? `${m.dewpoint_c.toFixed(0)}°` : "—"}
        </span>
        <span className="wx__sep" aria-hidden="true">·</span>
        <span className="wx__cell" title="QNH / Druck">
          <b>QNH</b> {fmtQnh(m.qnh_hpa)}
        </span>
      </div>
      {showRaw && m.raw && <pre className="wx__raw">{m.raw}</pre>}
    </div>
  );
}

export function WeatherBriefing({
  dptIcao,
  arrIcao,
  prefetchedDpt,
  prefetchedArr,
}: Props) {
  const { t } = useTranslation();
  const [dpt, setDpt] = useState<MetarFetchState>({ kind: "loading" });
  const [arr, setArr] = useState<MetarFetchState>({ kind: "loading" });

  // Unmount-Wächter (Codex-Befund, zweite Runde): ein Fetch kann noch
  // laufen, wenn der Pilot die Karte wegnavigiert (Tab-Wechsel). Ohne
  // diese Prüfung würde `fetchOne` trotzdem noch `set(...)` auf eine
  // verschwundene Komponente aufrufen — React 19 warnt dafür nicht mehr,
  // aber unnötig ist es trotzdem.
  const gemountet = useRef(true);
  useEffect(() => {
    gemountet.current = true;
    return () => {
      gemountet.current = false;
    };
  }, []);

  // ── Reiner Ladeindikator, ohne Einfluss auf welche Daten gezeigt werden ──
  //
  // Fünfte Codex-Runde: Die vorherigen Fassungen (erst ein Zähler, der
  // `refreshing` direkt trug, dann Generation-Zähler, die entschieden,
  // welcher von zwei parallelen `metar_get`-Aufrufen "gewinnt") hatten
  // beide dieselbe Wurzelursache — sie regelten Ladezustand UND
  // Datenkorrektheit über denselben Mechanismus. Ein spät auflösender,
  // aber inhaltlich NEUERER eigener Fetch konnte von einem älteren, aber
  // zuerst eingetroffenen Prefetch verworfen werden, weil "zuerst da"
  // nicht "aktueller" bedeutet.
  //
  // Jetzt getrennt: `refreshLaeuft` ist NUR ein Zähler laufender Netz-
  // Aufrufe fürs Icon/den Button — er entscheidet nichts über Daten.
  // Welcher Wert angezeigt wird, entscheidet ausschliesslich `wendeAn`
  // unten, anhand der METAR-Beobachtungszeit selbst.
  const [refreshLaeuft, setRefreshLaeuft] = useState(0);
  const refreshing = refreshLaeuft > 0;

  // Aktuelle Route, immer frisch.
  //
  // Sechste Codex-Runde: `fetchOne` prüfte bisher nur die METAR-Zeit
  // (`wendeAn`), nicht mehr, ob die ICAO, für die der Request gestartet
  // wurde, überhaupt noch die aktuelle Route ist. Wechselt der Pilot
  // während ein Fetch für EDDM noch offen ist auf einen neuen Flug nach
  // EDLN, würde `wendeAn` das späte EDDM-Ergebnis anstandslos übernehmen
  // — andere ICAO, also kein Zeitvergleich, direkte Übernahme (siehe
  // `wendeAn` oben: „anderer Flughafen → immer übernehmen"). Das war
  // richtig gedacht für „neuer Flug, neues Wetter", aber falsch für
  // „alter Flug antwortet spät nach". Der Unterschied: nur ein Ergebnis,
  // dessen ICAO noch mit der AKTUELLEN Route übereinstimmt, darf
  // überhaupt geschrieben werden.
  //
  // `useLayoutEffect` statt Zuweisung im Render-Körper (siebte Codex-
  // Runde): React untersagt das Mutieren eines Refs während des Renderns
  // ausdrücklich, ausser zur reinen Initialisierung — bei Concurrent
  // Rendering kann ein begonnener, dann verworfener Render-Versuch mit
  // einer NEUEN Route trotzdem den Ref überschreiben, während weiterhin
  // die ALTE Route committed bleibt. Ein `useLayoutEffect` läuft nur nach
  // einem tatsächlich COMMITTETEN Render, genau synchron mit dem, was der
  // Nutzer sieht.
  const aktuelleDptIcao = useRef(dptIcao);
  const aktuelleArrIcao = useRef(arrIcao);
  useLayoutEffect(() => {
    aktuelleDptIcao.current = dptIcao;
  }, [dptIcao]);
  useLayoutEffect(() => {
    aktuelleArrIcao.current = arrIcao;
  }, [arrIcao]);

  const fetchOne = useCallback(
    async (
      icao: string,
      set: Dispatch<SetStateAction<MetarFetchState>>,
      aktuelleIcao: MutableRefObject<string>,
    ) => {
      setRefreshLaeuft((n) => n + 1);
      try {
        // Regression vermeiden: Ist schon ein Wert für DIESELBE ICAO da,
        // bleibt er stehen, solange der Hintergrund-Refresh läuft — kein
        // Zurückfallen auf einen Lade-Spinner über bereits gezeigten guten
        // Daten. Aber: gehört der gezeigte Wert zu einer ANDEREN ICAO (z.
        // B. Routenwechsel ohne sofortigen Prefetch), ist er hier fehl am
        // Platz — sonst zeigt die Karte des NEUEN Ziels das Wetter des
        // ALTEN weiter, bis der neue Fetch durch ist (oder für immer,
        // schlägt er fehl). Siebte Codex-Runde.
        set((prev) =>
          prev.kind === "ready" && prev.data?.icao === icao ? prev : { kind: "loading" },
        );
        const data = await invoke<MetarSnapshotDto>("metar_get", { icao });
        // Komponente weg, ODER die Route hat sich geändert, während der
        // Request lief — das Ergebnis gehört nicht mehr zur aktuellen Seite.
        // `data.icao !== icao` zusätzlich geprüft (siebte Codex-Runde):
        // die Antwort selbst könnte — falsch konfiguriert, NOAA-Eigenart —
        // eine andere ICAO tragen als angefragt; garantiert war das nie.
        if (!gemountet.current || icao !== aktuelleIcao.current || data.icao !== icao) return;
        set((prev) => wendeAn(prev, data));
      } catch (err: unknown) {
        if (!gemountet.current || icao !== aktuelleIcao.current) return;
        const msg =
          typeof err === "object" && err !== null && "message" in err
            ? String((err as { message: string }).message)
            : String(err);
        set((prev) =>
          prev.kind === "ready" && prev.data?.icao === icao
            ? prev
            : { kind: "error", error: msg },
        );
      } finally {
        setRefreshLaeuft((n) => n - 1);
      }
    },
    [],
  );

  // Leere ICAO nie dauerhaft im Ladezustand belassen (Codex-Befund,
  // fünfte Runde): Ohne ICAO wird nie ein Fetch ausgelöst — der initiale
  // `{ kind: "loading" }`-State würde sonst für immer stehen bleiben,
  // Button dauerhaft deaktiviert, Karte dauerhaft "lädt".
  useEffect(() => {
    if (!dptIcao) setDpt({ kind: "error", error: "no ICAO" });
  }, [dptIcao]);
  useEffect(() => {
    if (!arrIcao) setArr({ kind: "error", error: "no ICAO" });
  }, [arrIcao]);

  // Vom Backend vorgeholtes Wetter übernehmen, sobald es da ist — und
  // weiter, solange die Karte offen bleibt: `prefetchedDpt`/`prefetchedArr`
  // kommen aus `activeFlight` (App-weiter 2-s-Poll, siehe Props-Doku oben),
  // aktualisieren sich also von selbst, wenn `maybe_spawn_metar_fetch`
  // (Rust) bei Descent/Final einen frischeren Wert holt. `icao`-Check
  // verhindert, dass beim Wechsel auf einen neuen Flug für einen Tick noch
  // das METAR des alten Ziels durchrutscht, bevor der Poll nachzieht.
  // Das eigentliche "ist das neu genug"-Urteil faellt in `wendeAn`.
  useEffect(() => {
    if (prefetchedDpt && prefetchedDpt.icao === dptIcao) {
      setDpt((prev) => wendeAn(prev, prefetchedDpt));
    }
  }, [prefetchedDpt, dptIcao]);
  useEffect(() => {
    if (prefetchedArr && prefetchedArr.icao === arrIcao) {
      setArr((prev) => wendeAn(prev, prefetchedArr));
    }
  }, [prefetchedArr, arrIcao]);

  // Initial load + reload whenever the route changes — nur für die Seite,
  // die das Backend beim Mount noch NICHT vorgeholt hat (z. B. ganz am
  // Anfang, bevor Boarding/Takeoff den Abflug-Fetch ausgelöst hat). Sonst
  // wäre das ein zweiter, überflüssiger Netz-Trip für denselben Wert.
  useEffect(() => {
    if (!dptIcao && !arrIcao) return;
    const brauchtDpt = !!dptIcao && !(prefetchedDpt?.icao === dptIcao);
    const brauchtArr = !!arrIcao && !(prefetchedArr?.icao === arrIcao);
    if (!brauchtDpt && !brauchtArr) return;
    if (brauchtDpt) void fetchOne(dptIcao, setDpt, aktuelleDptIcao);
    if (brauchtArr) void fetchOne(arrIcao, setArr, aktuelleArrIcao);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- prefetchedDpt/
    // prefetchedArr bewusst nicht in den Deps: ihr Eintreffen behandelt
    // der Effekt oben, dieser hier soll nur beim Routenwechsel laufen.
    // `wendeAn` entscheidet beim Schreiben ohnehin anhand der METAR-Zeit,
    // welcher der beiden Werte tatsächlich aktueller ist — unabhängig
    // davon, welcher zuerst eintrifft.
  }, [dptIcao, arrIcao, fetchOne]);

  async function handleRefresh() {
    if (refreshing) return;
    await Promise.all([
      dptIcao ? fetchOne(dptIcao, setDpt, aktuelleDptIcao) : Promise.resolve(),
      arrIcao ? fetchOne(arrIcao, setArr, aktuelleArrIcao) : Promise.resolve(),
    ]);
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
      <div className="wx">
        <WxCard label={t("weather.departure")} state={dpt} />
        <WxCard label={t("weather.arrival")} state={arr} />
      </div>
    </section>
  );
}
