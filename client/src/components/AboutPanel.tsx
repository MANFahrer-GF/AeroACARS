import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";
import type { AppInfo } from "../types";

/**
 * About / Credits tab. Quiet, dezent, but acknowledges every project /
 * dataset / piece of reverse-engineering AeroACARS stands on. Each
 * line is a real reference — `OurAirports`, `BeatMyLanding`, `GEES`,
 * `vmsACARS`, `LandingToast` — these were studied in detail to get
 * the touchdown analyzer right.
 *
 * Renders the Gifhorn credit prominently but not loudly. Pilot opens
 * this tab when they want to know "what is this thing made of"; it
 * isn't shoved in their face on every other screen.
 */
interface Props {
  /** Open the in-app release-notes modal for the given version. App
   *  passes a setter that mounts `<ReleaseNotesModal version={...}>`. */
  onShowReleaseNotes: (version: string) => void;
}

export function AboutPanel({ onShowReleaseNotes }: Props) {
  const { t } = useTranslation();
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const ai = await invoke<AppInfo>("app_info");
        if (!cancelled) setInfo(ai);
      } catch {
        // app_info should never fail; if it does, just leave the
        // hero strip blank rather than showing a confusing error.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Womit die Karte zeichnet — Grafikkarte oder Software.
  //
  // Grund: das Aufbauen der VATSIM-Flaechen dauert im Fenster 20-25 s,
  // waehrend die Datenkette gemessen 471 ms braucht (Feed 254, Sektoren 213).
  // Es ist also das Zeichnen. Faellt die WebView auf Software-Rendering
  // zurueck (SwiftShader/llvmpipe), erklaert das die Groessenordnung — und
  // ohne diese Angabe laesst es sich nicht von aussen feststellen.
  const [renderer, setRenderer] = useState<string | null>(null);
  useEffect(() => {
    try {
      const c = document.createElement("canvas");
      const gl = (c.getContext("webgl2") ?? c.getContext("webgl")) as
        | WebGLRenderingContext
        | null;
      if (!gl) {
        setRenderer("kein WebGL");

        return;
      }
      const dbg = gl.getExtension("WEBGL_debug_renderer_info");
      setRenderer(
        dbg
          ? String(gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL))
          : String(gl.getParameter(gl.RENDERER)),
      );
    } catch {
      setRenderer("nicht ermittelbar");
    }
  }, []);

  return (
    <section className="about">
      <header className="about__hero">
        <h2 className="about__title">AeroACARS</h2>
        <p className="about__tagline">{t("about.tagline")}</p>
        {info && (
          <p className="about__version">
            v{info.version}
            {info.commit ? <> · <code>{info.commit.slice(0, 7)}</code></> : null}
          </p>
        )}
        {renderer && (
          <p className="about__version" style={{ opacity: 0.75 }}>
            Grafik: <code>{renderer}</code>
          </p>
        )}
        {info && <p className="about__credit">{info.credit}</p>}
        {info && (
          <div className="about__hero-actions">
            <button
              type="button"
              className="button button--ghost"
              onClick={() => onShowReleaseNotes(info.version)}
            >
              {t("release_notes.about_button")}
            </button>
            <a
              className="button button--ghost"
              href="https://github.com/MANFahrer-GF/AeroACARS/releases"
              target="_blank"
              rel="noreferrer"
            >
              {t("release_notes.about_all_releases")}
            </a>
          </div>
        )}
      </header>

      <div className="about__section">
        <h3>{t("about.purpose_title")}</h3>
        <p>{t("about.purpose_body")}</p>
      </div>

      <div className="about__section">
        <h3>{t("about.acknowledgements_title")}</h3>
        <p className="about__hint">{t("about.acknowledgements_intro")}</p>
        <ul className="about__list">
          <li>
            <strong>OpenStreetMap</strong> — Taxiway and stand geometry
            for the ground charts, © OpenStreetMap contributors,
            available under the{" "}
            <a
              href="https://opendatacommons.org/licenses/odbl/"
              target="_blank"
              rel="noreferrer"
            >
              Open Database License
            </a>
            .
          </li>
          <li>
            <strong>X-Plane Airport Data (apt.dat)</strong> — Taxiway
            designators for airports where OpenStreetMap carries none.
            Copyright © Robin A. Peel, released under the{" "}
            <a
              href="https://www.gnu.org/licenses/old-licenses/gpl-2.0.html"
              target="_blank"
              rel="noreferrer"
            >
              GNU General Public License
            </a>
            , obtained via the{" "}
            <a
              href="https://gateway.x-plane.com/"
              target="_blank"
              rel="noreferrer"
            >
              X-Plane Scenery Gateway
            </a>
            . Only the designators are used; the geometry stays
            OpenStreetMap's.
          </li>
          <li>
            <strong>OurAirports</strong> — Public-domain runway dataset
            powering the centerline/threshold correlation.{" "}
            <a
              href="https://ourairports.com/data/"
              target="_blank"
              rel="noreferrer"
            >
              ourairports.com/data
            </a>
          </li>
          <li>
            <strong>BeatMyLanding</strong> — Reference for touchdown-
            window timings (500 ms / 1500 ms) and bounce-detection
            calibration via AGL edges.
          </li>
          <li>
            <strong>GEES</strong> — Open-source landing-rate logger;
            confirmed our V/S sign convention and native sideslip
            via VEL_BODY_X/Z.{" "}
            <a
              href="https://github.com/scelts/gees"
              target="_blank"
              rel="noreferrer"
            >
              github.com/scelts/gees
            </a>
          </li>
          <li>
            <strong>LandingToast</strong> — Validated the live-VS-at-
            on-ground-edge approach (no PLANE TOUCHDOWN NORMAL VELOCITY
            needed).
          </li>
          <li>
            <strong>Tauri 2 + React + Rust</strong> — App framework.
          </li>
          <li>
            <strong>Microsoft Flight Simulator SDK</strong> — SimConnect
            client API.
          </li>
          <li>
            <strong>Laminar Research X-Plane SDK</strong> — UDP RREF
            DataRef protocol documentation.
          </li>
        </ul>
      </div>

      <div className="about__section">
        <h3>{t("about.thresholds_title")}</h3>
        <p className="about__hint">{t("about.thresholds_intro")}</p>
        <ul className="about__list">
          <li>Boeing 737 FCOM — Hard-Landing inspection trigger</li>
          <li>Airbus A320 FCOM — TD sink rate / inspection criteria</li>
          <li>Lufthansa FOQA — Public category bands</li>
        </ul>
      </div>

      <footer className="about__footer">
        <p>© {new Date().getFullYear()} AeroACARS Project · MIT License</p>
        <p>{info?.credit}</p>
      </footer>
    </section>
  );
}
