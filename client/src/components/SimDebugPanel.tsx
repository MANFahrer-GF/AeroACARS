import { useTranslation } from "react-i18next";
import type { SimSnapshot, SimStatus } from "../types";
import { SimInspector } from "./SimInspector";
import { SimDiffSnapshot } from "./SimDiffSnapshot";
import { XPlaneInspector } from "./XPlaneInspector";

/**
 * Display-only sim telemetry panel for the Settings tab's debug
 * section. State is fed in via props (polled centrally by
 * `useSimSession` at the App level) so it stays in sync with whatever
 * the cockpit / briefing tabs see, without a duplicate poll loop.
 */
interface Props {
  status: SimStatus | null;
}

function fmtNumber(value: number, fractionDigits: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: fractionDigits,
    maximumFractionDigits: fractionDigits,
  }).format(value);
}

function fmtCoord(deg: number): string {
  const sign = deg >= 0 ? "+" : "−";
  return `${sign}${Math.abs(deg).toFixed(6)}°`;
}

function fmtFt(ft: number, locale: string): string {
  return `${new Intl.NumberFormat(locale).format(Math.round(ft))} ft`;
}

function fmtKt(kt: number, locale: string): string {
  return `${new Intl.NumberFormat(locale).format(Math.round(kt))} kt`;
}

function fmtFpm(fpm: number, locale: string): string {
  const rounded = Math.round(fpm);
  const sign = rounded > 0 ? "+" : "";
  return `${sign}${new Intl.NumberFormat(locale).format(rounded)} fpm`;
}

function fmtHeading(deg: number): string {
  const norm = ((deg % 360) + 360) % 360;
  return `${Math.round(norm).toString().padStart(3, "0")}°`;
}

function fmtKg(kg: number, locale: string): string {
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(
    kg,
  )} kg`;
}

function fmtKgLb(kg: number, locale: string): string {
  // Show kg + raw lb so we can spot conversion / unit issues at a glance.
  const lb = kg / 0.45359237;
  return `${fmtKg(kg, locale)} (${new Intl.NumberFormat(locale, {
    maximumFractionDigits: 0,
  }).format(lb)} lb)`;
}

export function SimDebugPanel({ status }: Props) {
  const { t, i18n } = useTranslation();
  if (!status) {
    return <p className="sim-panel__hint">{t("sim.loading")}</p>;
  }
  const { state, kind, snapshot, last_error } = status;
  const stateLabel = t(`sim.state.${state}`);
  const isOff = kind === "off";
  const kindLabel = t(`sim.kinds.${kind}`);

  return (
    <section className={`sim-panel sim-panel--${state}`}>
      <header className="sim-panel__header">
        <div className="sim-panel__header-left">
          <h2>{t("sim.title")}</h2>
          <span className="sim-panel__kind">{kindLabel}</span>
        </div>
        <span
          className={`sim-panel__state sim-panel__state--${state}`}
          aria-live="polite"
        >
          <span className="sim-panel__dot" /> {stateLabel}
        </span>
      </header>

      {isOff && <p className="sim-panel__hint">{t("sim.off_hint")}</p>}
      {last_error && state !== "connected" && !isOff && (
        <p className="sim-panel__error" role="alert">
          {t("sim.last_error_prefix")}: <code>{last_error}</code>
        </p>
      )}
      {snapshot && state === "connected" && (
        <>
          <SnapshotGrid snap={snapshot} locale={i18n.language} />
          <MassFuelGrid snap={snapshot} locale={i18n.language} />
          <SwitchStatePanel snap={snapshot} />
          <TouchdownGrid snap={snapshot} locale={i18n.language} />
          {/* Sim-specific inspector. MSFS uses an add-watch list of
              SimVars/LVars; X-Plane uses a fixed-at-compile-time
              catalog of subscribed DataRefs (see XPlaneInspector). */}
          {kind && kind.startsWith("xplane") ? (
            <XPlaneInspector />
          ) : (
            <SimInspector />
          )}
          <SimDiffSnapshot snapshot={snapshot} />
        </>
      )}
    </section>
  );
}

/**
 * Switch & light state, rendered as colored pills grouped by
 * subsystem. Lets a pilot flip a switch in the cockpit and immediately
 * see whether the change reaches our pipeline — useful both for
 * end-user debugging ("why isn't my landing light registering?") and
 * for development work on aircraft profiles, since you can verify at
 * a glance which SimVar/LVar mapping is working.
 *
 * `null` values are rendered with a muted dash to distinguish "we
 * don't read this for the active aircraft" from "we read it and it's
 * off".
 */
function SwitchStatePanel({ snap }: { snap: SimSnapshot }) {
  const { t } = useTranslation();
  return (
    <>
      <h3 className="sim-panel__section">{t("sim.sections.switches")}</h3>
      <div className="switch-grid">
        <SwitchGroup label={t("sim.groups.lights")}>
          <Pill label={t("sim.fields.light_landing")} value={snap.light_landing} />
          <Pill label={t("sim.fields.light_taxi")} value={snap.light_taxi} />
          <Pill label={t("sim.fields.light_beacon")} value={snap.light_beacon} />
          <Pill label={t("sim.fields.light_strobe")} value={snap.light_strobe} />
          <Pill label={t("sim.fields.light_nav")} value={snap.light_nav} />
          <Pill label={t("sim.fields.light_logo")} value={snap.light_logo} />
        </SwitchGroup>
        <SwitchGroup label={t("sim.groups.autopilot")}>
          <Pill label={t("sim.fields.ap_master")} value={snap.autopilot_master} />
          <Pill label={t("sim.fields.ap_heading")} value={snap.autopilot_heading} />
          <Pill label={t("sim.fields.ap_altitude")} value={snap.autopilot_altitude} />
          <Pill label={t("sim.fields.ap_nav")} value={snap.autopilot_nav} />
          <Pill label={t("sim.fields.ap_approach")} value={snap.autopilot_approach} />
        </SwitchGroup>
        <SwitchGroup label={t("sim.groups.surfaces")}>
          <ValueBadge
            label={t("sim.fields.flaps")}
            value={fmtPercentSurface(snap.flaps_position)}
          />
          <ValueBadge
            label={t("sim.fields.gear")}
            value={fmtGear(snap.gear_position)}
          />
          <ValueBadge
            label={t("sim.fields.spoilers")}
            value={fmtPercentSurface(snap.spoilers_handle_position)}
          />
          <Pill label={t("sim.fields.spoilers_armed")} value={snap.spoilers_armed} />
        </SwitchGroup>
        <SwitchGroup label={t("sim.groups.systems")}>
          <Pill label={t("sim.fields.battery_master")} value={snap.battery_master} />
          <Pill label={t("sim.fields.avionics_master")} value={snap.avionics_master} />
          <Pill label={t("sim.fields.pitot_heat")} value={snap.pitot_heat} />
          <Pill label={t("sim.fields.eng_anti_ice")} value={snap.engine_anti_ice} />
          <Pill label={t("sim.fields.wing_anti_ice")} value={snap.wing_anti_ice} />
          <ApuPill apuSwitch={snap.apu_switch} apuRpm={snap.apu_pct_rpm} />
        </SwitchGroup>
        <SwitchGroup label={t("sim.groups.aircraft")}>
          <Pill label={t("sim.fields.parking_brake")} value={snap.parking_brake} />
          <Pill label={t("sim.fields.stall_warning")} value={snap.stall_warning} />
          <Pill label={t("sim.fields.overspeed_warning")} value={snap.overspeed_warning} />
        </SwitchGroup>
        <SwitchGroup label={t("sim.groups.engines")}>
          <span className="switch-grid__count">
            {snap.engines_running} / 4
          </span>
        </SwitchGroup>
      </div>
    </>
  );
}

/**
 * Position indicator (flaps / gear / spoilers). Renders as a small
 * badge with a numeric value rather than a bool pill — these are
 * continuous values where "off" / "on" don't capture the meaning.
 */
function ValueBadge({
  label,
  value,
}: {
  label: string;
  value: string | null;
}) {
  return (
    <span
      className={`value-badge ${
        value === null ? "value-badge--unknown" : ""
      }`}
    >
      <span className="value-badge__label">{label}</span>
      <span className="value-badge__value">{value ?? "—"}</span>
    </span>
  );
}

/**
 * APU status pill: APU is special because the switch can be on
 * while the unit is still spooling up. Showing both gives a clearer
 * picture than a plain bool ("APU starting" vs "APU running").
 */
function ApuPill({
  apuSwitch,
  apuRpm,
}: {
  apuSwitch: boolean | null;
  apuRpm: number | null;
}) {
  const { t } = useTranslation();
  if (apuSwitch === null) {
    return (
      <span className="switch-pill switch-pill--unknown">
        <span className="switch-pill__dot" /> {t("sim.fields.apu")}
      </span>
    );
  }
  if (!apuSwitch) {
    return (
      <span className="switch-pill switch-pill--off">
        <span className="switch-pill__dot" /> {t("sim.fields.apu")}
      </span>
    );
  }
  // Switch is on. RPM tells us whether it's running or just starting.
  const rpm = apuRpm ?? 0;
  const running = rpm >= 95;
  const trailing = running ? "" : ` ${Math.round(rpm)}%`;
  return (
    <span
      className={`switch-pill switch-pill--${running ? "on" : "warn"}`}
    >
      <span className="switch-pill__dot" /> {t("sim.fields.apu")}{trailing}
    </span>
  );
}

/** 0..1 → "12%" (two-digit, rounded). null → null. */
function fmtPercentSurface(v: number | null | undefined): string | null {
  if (v === null || v === undefined) return null;
  return `${Math.round(v * 100)}%`;
}

/** Gear position: 0 = up, 1 = down, anything in between = transit. */
function fmtGear(v: number | null | undefined): string | null {
  if (v === null || v === undefined) return null;
  if (v <= 0.02) return "UP";
  if (v >= 0.98) return "DOWN";
  return `${Math.round(v * 100)}%`;
}

/**
 * One switch state. Three visual states:
 *  - on:  filled colored pill
 *  - off: hollow outlined pill
 *  - null/unknown: muted dash (we don't have a wiring for this on
 *                  the current aircraft profile)
 */
function Pill({
  label,
  value,
}: {
  label: string;
  value: boolean | null | undefined;
}) {
  if (value === null || value === undefined) {
    return (
      <span className="switch-pill switch-pill--unknown">
        <span className="switch-pill__dot" /> {label}
      </span>
    );
  }
  return (
    <span className={`switch-pill switch-pill--${value ? "on" : "off"}`}>
      <span className="switch-pill__dot" /> {label}
    </span>
  );
}

function SwitchGroup({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="switch-group">
      <h4 className="switch-group__label">{label}</h4>
      <div className="switch-group__pills">{children}</div>
    </div>
  );
}

/**
 * Live Mass & Fuel readout. Surfaces the values that get written into
 * the PIREP custom fields so the pilot can see *before* filing whether
 * the aircraft addon actually wires them. If you see "—" here, the
 * field will be dropped from the PIREP rather than written as "0 kg".
 */
function MassFuelGrid({ snap, locale }: { snap: SimSnapshot; locale: string }) {
  const { t } = useTranslation();
  return (
    <>
      <h3 className="sim-panel__section">{t("sim.sections.mass_fuel")}</h3>
      <dl className="sim-panel__grid">
        <Row label={t("sim.fields.fuel_total")}>
          {snap.fuel_total_kg > 0
            ? fmtKgLb(snap.fuel_total_kg, locale)
            : <em className="sim-panel__muted">{t("sim.fields.not_wired")}</em>}
        </Row>
        <Row label={t("sim.fields.total_weight")}>
          {snap.total_weight_kg !== null && snap.total_weight_kg > 0
            ? fmtKgLb(snap.total_weight_kg, locale)
            : <em className="sim-panel__muted">{t("sim.fields.not_wired")}</em>}
        </Row>
        <Row label={t("sim.fields.zfw")}>
          {snap.zfw_kg !== null && snap.zfw_kg > 0
            ? fmtKgLb(snap.zfw_kg, locale)
            : <em className="sim-panel__muted">{t("sim.fields.not_wired")}</em>}
        </Row>
      </dl>
    </>
  );
}

/**
 * Last-touchdown grid. These values are populated by the simulation
 * itself the moment the gear hits the ground and stay frozen until the
 * next takeoff. If "—" is shown, no touchdown has been recorded yet
 * this session.
 */
function TouchdownGrid({ snap, locale }: { snap: SimSnapshot; locale: string }) {
  const { t } = useTranslation();
  const hasData = snap.touchdown_vs_fpm !== null;
  if (!hasData) {
    return (
      <>
        <h3 className="sim-panel__section">{t("sim.sections.touchdown")}</h3>
        <p className="sim-panel__hint">{t("sim.fields.no_touchdown_yet")}</p>
      </>
    );
  }
  return (
    <>
      <h3 className="sim-panel__section">{t("sim.sections.touchdown")}</h3>
      <dl className="sim-panel__grid">
        <Row label={t("sim.fields.touchdown_vs")}>
          {fmtFpm(snap.touchdown_vs_fpm ?? 0, locale)}
        </Row>
        <Row label={t("sim.fields.touchdown_pitch")}>
          {fmtNumber(snap.touchdown_pitch_deg ?? 0, 1, locale)}°
        </Row>
        <Row label={t("sim.fields.touchdown_bank")}>
          {fmtNumber(snap.touchdown_bank_deg ?? 0, 1, locale)}°
        </Row>
        <Row label={t("sim.fields.touchdown_heading")}>
          {fmtHeading(snap.touchdown_heading_mag_deg ?? 0)}
        </Row>
        {snap.touchdown_lat !== null && snap.touchdown_lon !== null && (
          <Row label={t("sim.fields.position")}>
            {fmtCoord(snap.touchdown_lat)} · {fmtCoord(snap.touchdown_lon)}
          </Row>
        )}
      </dl>
    </>
  );
}

function SnapshotGrid({ snap, locale }: { snap: SimSnapshot; locale: string }) {
  const { t } = useTranslation();
  return (
    <dl className="sim-panel__grid">
      <Row label={t("sim.fields.position")}>
        {fmtCoord(snap.lat)} · {fmtCoord(snap.lon)}
      </Row>
      {snap.altitude_indicated_ft != null && (
        <Row label={t("sim.fields.altitude_indicated")}>
          {fmtFt(snap.altitude_indicated_ft, locale)}
        </Row>
      )}
      <Row label={t("sim.fields.altitude_msl")}>
        {fmtFt(snap.altitude_msl_ft, locale)}
      </Row>
      <Row label={t("sim.fields.altitude_agl")}>
        {fmtFt(snap.altitude_agl_ft, locale)}
      </Row>
      <Row label={t("sim.fields.heading")}>
        {fmtHeading(snap.heading_deg_magnetic)} ({t("sim.fields.heading_mag")}) ·{" "}
        {fmtHeading(snap.heading_deg_true)} ({t("sim.fields.heading_true")})
      </Row>
      <Row label={t("sim.fields.groundspeed")}>
        {fmtKt(snap.groundspeed_kt, locale)}
      </Row>
      <Row label={t("sim.fields.airspeed")}>
        IAS {fmtKt(snap.indicated_airspeed_kt, locale)} · TAS{" "}
        {fmtKt(snap.true_airspeed_kt, locale)}
      </Row>
      <Row label={t("sim.fields.vertical_speed")}>
        {fmtFpm(snap.vertical_speed_fpm, locale)}
      </Row>
      <Row label={t("sim.fields.attitude")}>
        {t("sim.fields.pitch")} {fmtNumber(snap.pitch_deg, 1, locale)}° ·{" "}
        {t("sim.fields.bank")} {fmtNumber(snap.bank_deg, 1, locale)}°
      </Row>
      <Row label={t("sim.fields.g_force")}>
        {fmtNumber(snap.g_force, 2, locale)} G
      </Row>
      <Row label={t("sim.fields.on_ground")}>
        {snap.on_ground ? t("sim.yes") : t("sim.no")}
      </Row>
    </dl>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </>
  );
}
