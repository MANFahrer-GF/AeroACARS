import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./i18n";
import "./App.css";
import { applyTheme, getInitialTheme } from "./theme";
import { SkinProvider } from "./components/SkinContext";
import { BasemapProvider } from "./components/BasemapContext";
import { initSentry, getConsent, Sentry } from "./lib/sentry";
import {
  invoke,
  isTauri,
  hasRemoteToken,
  consumePinFromUrl,
  onReauthNeeded,
} from "./lib/ipc";
import { RemotePinGate } from "./components/RemotePinGate";
import { VatsimCdmView } from "./components/VatsimCdmView";

// v0.9.0 (#GlitchTip): Sentry-Init MUSS frueh laufen, sonst gehen
// Bootstrap-Fehler im weissen Bildschirm unter. Init macht KEIN
// Network-Call wenn DSN nicht in Build-Time-Env gesetzt war.
initSentry(typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : "0.0.0");

// Initial-Consent-Sync zum Rust-Backend (Default = false). Damit folgt
// der Backend-Atomic dem gespeicherten Pilot-Choice ohne dass der User
// erst zu Settings muss. Wenn der Command nicht existiert (z.B. alter
// Backend-Build), ignorieren wir den Fehler — Default bleibt aus.
//
// v0.16.0 (#LAN-Remote): im reinen Browser-Build (Tablet) wuerde dieser
// Call noch ohne Token feuern und sofort 401en → die UI in eine Re-Auth-
// Schleife zwingen, bevor das PIN-Gate ueberhaupt steht. Deshalb nur im
// Tauri-Build (dort ist die native IPC immer berechtigt).
if (isTauri) {
  void invoke("error_reporting_set_consent", { enabled: getConsent() }).catch(
    () => undefined,
  );
}

applyTheme(getInitialTheme());

// Feld-Report (2026-08-05): Rechtsklick zeigte das native macOS/WebView-
// Kontextmenu (Zurueck/Neu laden/Element untersuchen) in grauem System-Chrome
// — passt zu keinem Pixel des sonst komplett eigenen Dark-Themes. Es gibt
// keine App-eigene Kontextmenue-Funktion, die dieses Menue ersetzen wuerde
// (kein Copy/Paste-Bedarf, den ein Pilot hier haette), also wird es im
// Desktop-Build schlicht unterdrueckt statt neu gebaut. NICHT im
// LAN-Remote-Browser-Build (Tablet) — dort ist es ein echter Browser-Tab,
// wo das native Menu (z. B. Reload) sinnvoll bleibt.
if (isTauri) {
  document.addEventListener("contextmenu", (e) => e.preventDefault());
}

/**
 * v0.16.0 (#LAN-Remote) — Browser-Bootstrap-Gate.
 *
 * Im Tauri-Desktop-Build rendert sofort die echte App (es gibt keinen
 * Token-Begriff — die native IPC ist immer berechtigt).
 *
 * Im LAN-Browser-Build (Tablet) muss erst ein gueltiger Bearer-Token
 * vorhanden sein, bevor irgendein `invoke()` durchgeht. Ablauf:
 *   1. QR-Deep-Link `?pin=NNNNNN` konsumieren (auto-Auth + URL strippen),
 *   2. wenn danach ein Token existiert → echte App,
 *   3. sonst → Vollbild-PIN-Gate, das nach Erfolg `onAuthenticated` ruft.
 * Ein spaeteres 401 ruft `clearRemoteToken()` in der ipc-Schicht, das ueber
 * `onReauthNeeded` hier zurueck in den Locked-State flippt — ohne Reload.
 */
/**
 * Vorschau einzelner Ansichten waehrend der Entwicklung.
 *
 * Im Browser laeuft der Client als Fernbedienung und ist deshalb hinter der
 * PIN vom Sim-PC gesperrt — richtig fuer das Tablet, im Weg, wenn man nur
 * eine Ansicht anschauen will. Dieser Weg existiert AUSSCHLIESSLICH im
 * Entwicklungsmodus (`import.meta.env.DEV`); im gebauten Programm ist der
 * Zweig gar nicht erst enthalten.
 *
 * Aufruf: http://localhost:1420/?vorschau=cdm
 */
function Vorschau({ was }: { was: string }) {
  if (was === "cdm") {
    return (
      <SkinProvider>
        <div style={{ height: "100vh", padding: 12, background: "var(--bg)" }}>
          <VatsimCdmView />
        </div>
      </SkinProvider>
    );
  }
  return <div style={{ padding: 24, fontFamily: "system-ui" }}>Unbekannte Vorschau: {was}</div>;
}

function Root() {
  if (import.meta.env.DEV) {
    const was = new URLSearchParams(window.location.search).get("vorschau");
    if (was) return <Vorschau was={was} />;
  }

  // Tauri: nie gesperrt. Browser: gesperrt bis ein Token da ist.
  const [unlocked, setUnlocked] = useState(() => isTauri || hasRemoteToken());
  // Wir warten im Browser einen Tick auf den QR-`?pin=`-Flow, damit das
  // Gate nicht kurz aufblitzt, bevor der Deep-Link eingeloest ist.
  const [bootstrapping, setBootstrapping] = useState(
    () => !isTauri && !hasRemoteToken(),
  );

  useEffect(() => {
    if (isTauri) return;
    let cancelled = false;
    // 401-Handler: zurueck ins Gate.
    const off = onReauthNeeded(() => {
      if (!cancelled) setUnlocked(false);
    });
    // QR-Auto-Auth (no-op ohne `?pin=`).
    if (!hasRemoteToken()) {
      void consumePinFromUrl().finally(() => {
        if (cancelled) return;
        if (hasRemoteToken()) setUnlocked(true);
        setBootstrapping(false);
      });
    } else {
      setBootstrapping(false);
    }
    return () => {
      cancelled = true;
      off();
    };
  }, []);

  if (!isTauri && !unlocked) {
    // Waehrend des QR-Bootstraps nichts rendern (verhindert Gate-Flash).
    if (bootstrapping) return null;
    return <RemotePinGate onAuthenticated={() => setUnlocked(true)} />;
  }

  return (
    <SkinProvider>
      <BasemapProvider>
        <App />
      </BasemapProvider>
    </SkinProvider>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Sentry.ErrorBoundary
      fallback={({ resetError }) => (
        <div style={{ padding: 24, fontFamily: "system-ui, sans-serif" }}>
          <h2 style={{ marginTop: 0 }}>Etwas ist schiefgegangen</h2>
          <p>
            AeroACARS hat einen unerwarteten Fehler abgefangen. Wenn die
            anonyme Fehler-Telemetrie aktiv ist, wurde der Fehler bereits
            gemeldet — anderenfalls passiert nichts.
          </p>
          <button onClick={resetError}>Erneut versuchen</button>
        </div>
      )}
    >
      <Root />
    </Sentry.ErrorBoundary>
  </React.StrictMode>,
);
