// v0.5.27 VFR/Manual-Flight-Mode-Modal.
//
// Pilot picks aircraft + enters manual flight plan when no SimBrief OFP
// is available (= small airfields, VFR flights). Two stages:
//   1. Aircraft-Picker mit Suche + Sim-Default
//   2. Manual-Plan-Form (Block-Fuel, ETA Pflicht; Rest optional)

import { useEffect, useMemo, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";
import type { Bid, ActiveFlightInfo, UiError } from "../types";

// Local helper: konvertiert beliebigen err in UiError-Shape (analog
// zur asUiError-Funktion in BidsList.tsx).
function asUiError(err: unknown): UiError {
  if (err && typeof err === "object" && "code" in err && "message" in err) {
    const e = err as Record<string, unknown>;
    return {
      code: typeof e.code === "string" ? e.code : "unknown",
      message: typeof e.message === "string" ? e.message : String(err),
    };
  }
  return { code: "unknown", message: String(err) };
}

interface AircraftPickerEntry {
  id: number;
  registration: string;
  icao: string;
  name: string;
  airport_id: string;
  state: number;
  display: string;
}

interface ManualFlightPlan {
  aircraft_id: number;
  planned_block_fuel_kg: number;
  planned_flight_time_min: number;
  cruise_level_ft?: number;
  planned_route?: string;
  alt_airport_id?: string;
  planned_zfw_kg?: number;
  planned_burn_kg?: number;
  acknowledge_aircraft_mismatch?: boolean;
}

interface SimContextHint {
  aircraft_icao?: string | null;
  aircraft_registration?: string | null;
  fuel_total_kg?: number | null;
}

interface Props {
  bid: Bid;
  /** Aktueller Sim-Snapshot fuer Aircraft-Default + Block-Fuel-Default. */
  simHint: SimContextHint | null;
  onClose: () => void;
  onFlightStarted: (info: ActiveFlightInfo) => void;
}

type Stage = "aircraft" | "plan" | "submitting";

export function ManualFlightModal({ bid, simHint, onClose, onFlightStarted }: Props) {
  const { t } = useTranslation();
  const [stage, setStage] = useState<Stage>("aircraft");
  const [error, setError] = useState<string | null>(null);
  // v0.5.36: yellow warning banner für aircraft_mismatch_warning, mit
  // "Trotzdem starten"-Button. Separater State von `error` (rote Bar).
  const [warning, setWarning] = useState<string | null>(null);

  // Stage 1 — Aircraft-Picker
  const [aircraftList, setAircraftList] = useState<AircraftPickerEntry[] | null>(null);
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<AircraftPickerEntry | null>(null);
  const [loadingFleet, setLoadingFleet] = useState(true);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        setLoadingFleet(true);
        setError(null);
        const list = await invoke<AircraftPickerEntry[]>("fleet_list_at_airport", {
          icao: bid.flight.dpt_airport_id,
        });
        if (cancelled) return;
        setAircraftList(list);
        // Sim-Default-Auswahl: wenn der Sim ein passendes Aircraft geladen
        // hat, dieses vorauswaehlen damit Pilot nicht raten muss
        if (simHint?.aircraft_registration) {
          const match = list.find(
            (a) => a.registration.trim().toUpperCase() === simHint.aircraft_registration!.trim().toUpperCase(),
          );
          if (match) setSelected(match);
        } else if (simHint?.aircraft_icao) {
          const match = list.find(
            (a) => a.icao.trim().toUpperCase() === simHint.aircraft_icao!.trim().toUpperCase(),
          );
          if (match) setSelected(match);
        }
      } catch (err: unknown) {
        if (cancelled) return;
        const ui = asUiError(err);
        setError(`Konnte Fleet nicht laden: ${ui.message}`);
        setAircraftList([]);
      } finally {
        if (!cancelled) setLoadingFleet(false);
      }
    })();
    return () => { cancelled = true; };
  }, [bid.flight.dpt_airport_id, simHint?.aircraft_registration, simHint?.aircraft_icao]);

  const filtered = useMemo(() => {
    if (!aircraftList) return [];
    if (search.trim().length === 0) return aircraftList;
    const q = search.toLowerCase().trim();
    return aircraftList.filter((a) =>
      a.icao.toLowerCase().includes(q)
      || a.registration.toLowerCase().includes(q)
      || a.name.toLowerCase().includes(q),
    );
  }, [aircraftList, search]);

  // Stage 2 — Manual-Plan-Form
  //
  // v0.11.0 (VFR-Resize-Bug): alle Form-Felder werden in localStorage
  // persistiert, sodass selbst ein React-Remount (z.B. wenn der parent
  // bei Window-Resize kurz das Modal aus dem Tree nimmt, oder wenn der
  // 15-s-Bids-Polling-Tick zu einem Re-Render mit neuen Object-Refs
  // führt) die Eingaben nicht verliert. Pilot kann mitten in der
  // Eingabe das Fenster verkleinern/vergrößern und macht ohne
  // Datenverlust weiter. Storage-Key ist bid-spezifisch, damit zwei
  // verschiedene Bids unabhängige Form-States haben.
  const storageKey = `aeroacars.vfr.${bid.id}`;
  const persisted: Partial<{
    blockFuelKg: string;
    flightTimeMin: string;
    cruiseLevel: string;
    route: string;
    altAirport: string;
    zfwKg: string;
  }> = (() => {
    try {
      const raw = localStorage.getItem(storageKey);
      return raw ? JSON.parse(raw) : {};
    } catch {
      return {};
    }
  })();

  const [blockFuelKg, setBlockFuelKg] = useState<string>(() => {
    // Reihenfolge: persistierter Wert > Sim-Default > leer
    if (persisted.blockFuelKg != null) return persisted.blockFuelKg;
    return simHint?.fuel_total_kg ? Math.round(simHint.fuel_total_kg).toString() : "";
  });
  const [flightTimeMin, setFlightTimeMin] = useState<string>(persisted.flightTimeMin ?? "");
  const [cruiseLevel, setCruiseLevel] = useState<string>(persisted.cruiseLevel ?? "");
  const [route, setRoute] = useState<string>(persisted.route ?? "");
  const [altAirport, setAltAirport] = useState<string>(persisted.altAirport ?? "");
  const [zfwKg, setZfwKg] = useState<string>(persisted.zfwKg ?? "");

  // Persistiere alle Form-Felder bei jeder Änderung in localStorage —
  // synchron, damit auch ein sofortiges Unmount (Resize-Crash) den
  // letzten Stand erfasst.
  useEffect(() => {
    try {
      localStorage.setItem(
        storageKey,
        JSON.stringify({
          blockFuelKg,
          flightTimeMin,
          cruiseLevel,
          route,
          altAirport,
          zfwKg,
        }),
      );
    } catch {
      /* localStorage voll oder disabled → noop */
    }
  }, [storageKey, blockFuelKg, flightTimeMin, cruiseLevel, route, altAirport, zfwKg]);

  // Cleanup: wenn der Pilot den Flug erfolgreich startet ODER explizit
  // schließt, ist der persisted-Stand nicht mehr relevant. Wir geben
  // dem caller via Hook eine clear()-Funktion (siehe unten in onFlight-
  // Started / explicitClose-Pfaden).
  const clearPersistedForm = () => {
    try {
      localStorage.removeItem(storageKey);
    } catch {
      /* noop */
    }
  };

  function proceedToPlan() {
    if (!selected) return;
    setError(null);
    setWarning(null);
    setStage("plan");
  }

  async function submit(acknowledgeAircraftMismatch = false) {
    if (!selected) return;
    const blockFuel = parseFloat(blockFuelKg);
    const ftMin = parseInt(flightTimeMin, 10);
    if (!Number.isFinite(blockFuel) || blockFuel <= 0) {
      setError(t("flight.error.invalid_block_fuel"));
      return;
    }
    if (!Number.isFinite(ftMin) || ftMin <= 0) {
      setError(t("flight.error.invalid_flight_time"));
      return;
    }
    // v0.7.1 Phase 2 F1 (Spec docs/spec/v0.7.1-landing-ux-fairness.md):
    // ZFW ist NICHT mehr Pflicht. Leerlassen → VFR/Manual ohne
    // Loadsheet-Wertung (sub_loadsheet wird skipped). Eingegeben +
    // <= 0 → Eingabefehler-Schutz (gleiche Bedingung wie Backend
    // lib.rs:5829-5837 → Err invalid_zfw_value).
    let zfw: number | undefined;
    const zfwTrimmed = zfwKg.trim();
    if (zfwTrimmed.length > 0) {
      const parsed = parseFloat(zfwTrimmed);
      if (!Number.isFinite(parsed) || parsed <= 0) {
        setError(t("flight.error.invalid_zfw_value"));
        return;
      }
      zfw = parsed;
    }
    const plan: ManualFlightPlan = {
      aircraft_id: selected.id,
      planned_block_fuel_kg: blockFuel,
      planned_flight_time_min: ftMin,
      planned_zfw_kg: zfw,
    };
    const cl = parseInt(cruiseLevel, 10);
    if (Number.isFinite(cl) && cl > 0) plan.cruise_level_ft = cl;
    if (route.trim().length > 0) plan.planned_route = route.trim();
    if (altAirport.trim().length > 0) plan.alt_airport_id = altAirport.trim().toUpperCase();
    if (acknowledgeAircraftMismatch) plan.acknowledge_aircraft_mismatch = true;

    setStage("submitting");
    setError(null);
    setWarning(null);
    try {
      const result = await invoke<ActiveFlightInfo>("flight_start_manual", {
        bidId: bid.id,
        plan,
      });
      // v0.11.0: persistierten Form-State löschen — Flug ist gestartet,
      // alte VFR-Eingaben sind nicht mehr relevant.
      clearPersistedForm();
      onFlightStarted(result);
    } catch (err: unknown) {
      const ui = asUiError(err);
      // v0.5.36: aircraft_mismatch_warning ist KEIN Hard-Block — wir
      // zeigen ein gelbes Warn-Banner mit "Trotzdem starten"-Button.
      if (ui.code === "aircraft_mismatch_warning") {
        setWarning(t("flight.error.aircraft_mismatch_warning"));
        setError(null);
        setStage("plan");
        return;
      }
      // Map known backend error codes to localized messages — analog
      // zu BidsList.tsx-IFR-Pfad. Fallback: rohe Server-Message.
      const knownCodes = [
        "no_sim_snapshot",
        "not_on_ground",
        "not_at_departure",
        "missing_airline",
        "missing_aircraft",
        "flight_already_active",
        "bid_not_found",
        "aircraft_not_available",
        "aircraft_mismatch",
        // v0.5.42: explicit Validation-Codes vom Backend
        "invalid_block_fuel",
        "invalid_flight_time",
        "invalid_zfw",
        // v0.7.1 Phase 2 F1 (P2.4-B): neuer Code wenn ZFW=0/negativ
        // angegeben wird. None bleibt erlaubt → kein Error.
        "invalid_zfw_value",
        "phpvms_error",
      ];
      const msg = knownCodes.includes(ui.code)
        ? t(`flight.error.${ui.code}`)
        : ui.message;
      setError(msg);
      setWarning(null);
      setStage("plan");
    }
  }

  return (
    <div
      className="manual-modal__backdrop"
      onClick={() => {
        // v0.11.0 (VFR-Resize-Bug): Backdrop-Click schließt das Modal
        // NUR wenn der Pilot noch nichts ausgewählt/eingegeben hat
        // (stage "aircraft" + nichts selected). Im plan-stage bleibt
        // das Modal hartnäckig offen — Pilot hat Werte getippt, ein
        // versehentlicher Touch/Klick auf den Rand bei Resize darf das
        // nicht wegnehmen. Zum Schließen gibts den expliziten Cancel-
        // Button unten. Submitting-stage bleibt ohnehin offen.
        if (stage === "submitting") return;
        if (stage === "plan") return;
        if (selected) return;
        onClose();
      }}
    >
      <div
        className="manual-modal"
        role="dialog"
        aria-labelledby="manual-modal-title"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="manual-modal__head">
          <h3 id="manual-modal-title">{t("manual_flight.title")}</h3>
          <div className="manual-modal__sub">
            {bid.flight.flight_number}
            {" · "}
            {bid.flight.dpt_airport_id} → {bid.flight.arr_airport_id}
            {" · "}
            <span style={{ opacity: 0.7 }}>{t("manual_flight.subtitle_no_ofp")}</span>
          </div>
        </header>

        {stage === "aircraft" && (
          <div className="manual-modal__body">
            <div className="manual-modal__section-title">
              {t("manual_flight.step_aircraft")}
            </div>
            {loadingFleet ? (
              <div className="manual-modal__loading">{t("manual_flight.loading_fleet")}</div>
            ) : aircraftList && aircraftList.length === 0 ? (
              <div className="manual-modal__empty">
                {t("manual_flight.empty_fleet")}
              </div>
            ) : (
              <>
                <input
                  type="search"
                  placeholder={t("manual_flight.search_placeholder")}
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  className="manual-modal__search"
                  autoFocus
                />
                <div style={{ fontSize: "0.78rem", color: "var(--text-muted)", marginBottom: 8 }}>
                  {t("manual_flight.list_total", {
                    count: aircraftList?.length ?? 0,
                    airport: bid.flight.dpt_airport_id,
                  })}
                </div>
                <div className="manual-modal__list">
                  {filtered.map((a) => {
                    const stateLabel =
                      a.state === 0 ? null :
                      a.state === 1 ? "🔒 in Use" :
                      a.state === 2 ? "✈ in Flight" :
                      "🔧 Maintenance";
                    const stateColor =
                      a.state === 0 ? undefined :
                      a.state === 1 ? "#fbbf24" :
                      a.state === 2 ? "#67e8f9" :
                      "#f87171";
                    const atDpt = a.airport_id?.toUpperCase() === bid.flight.dpt_airport_id.toUpperCase();
                    return (
                      <button
                        key={a.id}
                        type="button"
                        className={`manual-modal__list-item ${selected?.id === a.id ? "selected" : ""}`}
                        onClick={() => setSelected(a)}
                        title={a.state === 0
                          ? `Verfügbar${atDpt ? ` am ${bid.flight.dpt_airport_id}` : a.airport_id ? ` (steht in ${a.airport_id})` : ""}`
                          : `${stateLabel} — phpVMS lehnt ggf. den Prefile ab.`}
                      >
                        <span className="manual-modal__list-icao">{a.icao || "—"}</span>
                        <span className="manual-modal__list-reg">{a.registration || "—"}</span>
                        {a.airport_id && (
                          <span
                            className="manual-modal__list-name"
                            style={atDpt ? { color: "#86efac", fontWeight: 600 } : undefined}
                          >
                            @{a.airport_id}
                          </span>
                        )}
                        {a.name && a.name !== a.icao && (
                          <span className="manual-modal__list-name">{a.name}</span>
                        )}
                        {stateLabel && (
                          <span
                            className="manual-modal__list-name"
                            style={{ color: stateColor, marginLeft: "auto", fontSize: "0.8em" }}
                          >
                            {stateLabel}
                          </span>
                        )}
                      </button>
                    );
                  })}
                  {filtered.length === 0 && (
                    <div className="manual-modal__empty">
                      {t("manual_flight.no_match", { search })}
                    </div>
                  )}
                </div>
              </>
            )}
            {error && <div className="manual-modal__error">{error}</div>}
            <div className="manual-modal__actions">
              <button
                type="button"
                className="button"
                onClick={() => {
                  // v0.11.0: expliziter Cancel räumt persistierten
                  // Form-State auf — Pilot hat sich gegen den Flug
                  // entschieden, alte Werte sollen nicht beim nächsten
                  // VFR-Start als „Vorschlag" wieder auftauchen.
                  clearPersistedForm();
                  onClose();
                }}
              >
                {t("manual_flight.cancel")}
              </button>
              <button
                type="button"
                className="button button--primary"
                disabled={!selected}
                onClick={proceedToPlan}
              >
                {t("manual_flight.next")}
              </button>
            </div>
          </div>
        )}

        {(stage === "plan" || stage === "submitting") && selected && (
          <div className="manual-modal__body">
            <div className="manual-modal__section-title">
              {t("manual_flight.step_plan")}
            </div>
            <div style={{ marginBottom: 12, padding: "8px 10px", background: "rgba(103,232,249,0.08)", borderLeft: "3px solid #67e8f9", borderRadius: 4, fontSize: "0.85rem" }}>
              <strong>{selected.icao} {selected.registration}</strong>
              {selected.name && selected.name !== selected.icao && <> — {selected.name}</>}
            </div>

            <div className="manual-modal__form">
              <label>
                <span>{t("manual_flight.form.block_fuel")} <strong style={{ color: "#fbbf24" }}>*</strong></span>
                <div className="manual-modal__input-with-unit">
                  <input
                    type="number"
                    min="0"
                    step="1"
                    value={blockFuelKg}
                    onChange={(e) => setBlockFuelKg(e.target.value)}
                    placeholder={t("manual_flight.form.block_fuel_placeholder")}
                    disabled={stage === "submitting"}
                  />
                  <span>kg</span>
                </div>
                <small>{t("manual_flight.form.block_fuel_help")}</small>
              </label>

              <label>
                <span>{t("manual_flight.form.flight_time")} <strong style={{ color: "#fbbf24" }}>*</strong></span>
                <div className="manual-modal__input-with-unit">
                  <input
                    type="number"
                    min="1"
                    step="1"
                    value={flightTimeMin}
                    onChange={(e) => setFlightTimeMin(e.target.value)}
                    placeholder={t("manual_flight.form.flight_time_placeholder")}
                    disabled={stage === "submitting"}
                  />
                  <span>min</span>
                </div>
                <small>{t("manual_flight.form.flight_time_help")}</small>
              </label>

              <label>
                <span>{t("manual_flight.form.cruise_level")} <span style={{ color: "var(--fg-dim)" }}>{t("manual_flight.optional")}</span></span>
                <div className="manual-modal__input-with-unit">
                  <input
                    type="number"
                    min="0"
                    step="500"
                    value={cruiseLevel}
                    onChange={(e) => setCruiseLevel(e.target.value)}
                    placeholder={t("manual_flight.form.cruise_level_placeholder")}
                    disabled={stage === "submitting"}
                  />
                  <span>ft</span>
                </div>
                <small>{t("manual_flight.form.cruise_level_help")}</small>
              </label>

              <label>
                <span>{t("manual_flight.form.route")} <span style={{ color: "var(--fg-dim)" }}>{t("manual_flight.optional")}</span></span>
                <input
                  type="text"
                  value={route}
                  onChange={(e) => setRoute(e.target.value)}
                  placeholder={t("manual_flight.form.route_placeholder")}
                  disabled={stage === "submitting"}
                />
                <small>{t("manual_flight.form.route_help")}</small>
              </label>

              <label>
                <span>{t("manual_flight.form.alternate")} <span style={{ color: "var(--fg-dim)" }}>{t("manual_flight.optional")}</span></span>
                <input
                  type="text"
                  value={altAirport}
                  onChange={(e) => setAltAirport(e.target.value.toUpperCase())}
                  placeholder={t("manual_flight.form.alternate_placeholder")}
                  maxLength={4}
                  disabled={stage === "submitting"}
                />
                <small>{t("manual_flight.form.alternate_help")}</small>
              </label>

              <label>
                {/* v0.7.1 Phase 2 F1: ZFW ist NICHT mehr Pflicht.
                    Leerlassen = VFR/Manual ohne Loadsheet-Wertung
                    (sub_loadsheet skipped). Eingegeben + > 0 = wie bisher. */}
                <span>
                  {t("manual_flight.form.zfw")}{" "}
                  <span style={{ color: "var(--muted, #888)", fontWeight: 400 }}>
                    ({t("manual_flight.form.optional")})
                  </span>
                </span>
                <div className="manual-modal__input-with-unit">
                  <input
                    type="number"
                    min="0"
                    step="10"
                    value={zfwKg}
                    onChange={(e) => setZfwKg(e.target.value)}
                    placeholder={t("manual_flight.form.zfw_placeholder")}
                    disabled={stage === "submitting"}
                  />
                  <span>kg</span>
                </div>
                <small>{t("manual_flight.form.zfw_help_optional")}</small>
              </label>
            </div>

            {error && <div className="manual-modal__error">{error}</div>}
            {warning && (
              <div className="manual-modal__warning">
                <div className="manual-modal__warning-title">
                  {t("manual_flight.warning_title")}
                </div>
                <div className="manual-modal__warning-text">{warning}</div>
              </div>
            )}

            <div className="manual-modal__actions">
              <button
                type="button"
                className="button"
                onClick={() => setStage("aircraft")}
                disabled={stage === "submitting"}
              >
                {t("manual_flight.back")}
              </button>
              {(() => {
                // v0.7.1 Phase 2 F1: Submit deaktivieren wenn Block-Fuel
                // oder Flugzeit ungueltig sind. ZFW ist OPTIONAL — leer
                // ist OK (VFR/Manual ohne Loadsheet-Wertung), ein
                // angegebener Wert <= 0 ist Eingabefehler (Backend wird
                // mit invalid_zfw_value antworten).
                const bf = parseFloat(blockFuelKg);
                const ft = parseInt(flightTimeMin, 10);
                const zfwTrimmed = zfwKg.trim();
                const zfwParsed = zfwTrimmed.length > 0 ? parseFloat(zfwTrimmed) : NaN;
                const zfwInvalid =
                  zfwTrimmed.length > 0 &&
                  (!Number.isFinite(zfwParsed) || zfwParsed <= 0);
                const formInvalid =
                  !Number.isFinite(bf) || bf <= 0 ||
                  !Number.isFinite(ft) || ft <= 0 ||
                  zfwInvalid;
                const isSubmitting = stage === "submitting";
                if (warning) {
                  return (
                    <button
                      type="button"
                      className="button button--primary"
                      onClick={() => void submit(true)}
                      disabled={isSubmitting || formInvalid}
                      style={{ background: "#fbbf24", borderColor: "#fbbf24", color: "#1f1f1f" }}
                    >
                      {isSubmitting ? t("manual_flight.submitting") : t("manual_flight.start_anyway")}
                    </button>
                  );
                }
                return (
                  <button
                    type="button"
                    className="button button--primary"
                    onClick={() => void submit(false)}
                    disabled={isSubmitting || formInvalid}
                  >
                    {isSubmitting ? t("manual_flight.submitting") : t("manual_flight.submit")}
                  </button>
                );
              })()}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
