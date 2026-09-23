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
import { Notice } from "./components/ui";
import { VdgsPlatte, type VdgsStand } from "./components/VdgsBand";
import { SprungBanner } from "./components/SprungBanner";
import type { ActiveFlightInfo } from "./types";

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
  // `?theme=dark` erlaubt, eine Vorschau in beiden Themen anzusehen — sonst
  // haengt das Thema am localStorage, und ein frisch gestarteter Browser
  // (z. B. beim Rendern eines Bildes) zeigt immer nur hell.
  const gewuenscht = new URLSearchParams(window.location.search).get("theme");
  if (gewuenscht === "dark" || gewuenscht === "light") {
    applyTheme(gewuenscht);
  }
  if (was === "cdm") {
    return (
      <SkinProvider>
        <div style={{ height: "100vh", padding: 12, background: "var(--bg)" }}>
          <VatsimCdmView />
        </div>
      </SkinProvider>
    );
  }
  if (was === "vdgs") {
    return (
      <SkinProvider>
        <VdgsVorschau />
      </SkinProvider>
    );
  }
  if (was === "sprung") {
    return (
      <SkinProvider>
        <SprungVorschau />
      </SkinProvider>
    );
  }
  if (was === "integritaet") {
    return (
      <SkinProvider>
        <IntegritaetVorschau />
      </SkinProvider>
    );
  }
  return <div style={{ padding: 24, fontFamily: "system-ui" }}>Unbekannte Vorschau: {was}</div>;
}

/** Vorschau des schwebenden Integritaets-Hinweises ueber echtem Inhalt.
 *  Anlass GAF 9655 (23.09.2026): Im hellen Design stand die Meldung
 *  durchsichtig ueber der Buchungsliste, beides zusammen war unlesbar. */
function IntegritaetVorschau() {
  return (
    <div style={{ minHeight: "100vh", padding: 24, background: "var(--bg)" }}>
      <h1 style={{ color: "var(--text)" }}>Gebuchte Flüge</h1>
      <p style={{ color: "var(--text)" }}>
        Auto-Start aktiv, aber gerade nicht möglich — keine gebuchten Bids gefunden.
        Buche zuerst einen Flug auf der Webseite.
      </p>
      <p style={{ color: "var(--text-dim)" }}>
        Dieser Text liegt UNTER dem Hinweis. Er darf nicht durchscheinen.
      </p>
      <Notice
        floating
        role="alert"
        tone="error"
        level="Data-Integrity-Problem entdeckt"
        detail={
          <span>
            Längere Datenlücke im Endanflug · Der Flug wird normal gewertet.
            Bleibt die Aufzeichnung aber lückenhaft, fehlt am Ende womöglich die
            Landung — dann geht der Bericht zur Prüfung. · 2-mal in diesem Flug.
          </span>
        }
      />
      {/* Die übrigen drei Stufen im Fluss, aber mit derselben schwebenden
          Klasse — so sieht man in EINEM Bild, ob alle deckend sind. */}
      <div style={{ marginTop: 180, display: "grid", gap: 12 }}>
        {(["info", "warn", "success"] as const).map((ton) => (
          <div key={ton} style={{ position: "relative", height: 56 }}>
            <p style={{ color: "var(--text)", margin: 0 }}>
              Text unter „{ton}" — darf nicht durchscheinen, darf nicht durchscheinen.
            </p>
            <Notice
              floating
              tone={ton}
              level={ton.toUpperCase()}
              detail={<span>Schwebender Hinweis der Stufe {ton}.</span>}
              style={{ position: "absolute", top: 0, left: 0, transform: "none" }}
            />
          </div>
        ))}
      </div>
    </div>
  );
}

/** Vorschau des Sprung-Banners: Es erscheint, wenn beim Wiederaufnehmen
 *  ein physikalisch unmoeglicher Sprung erkannt wurde — dann gibt die App
 *  NICHT mehr von selbst ab, sondern der Pilot entscheidet (MSC1588). Die
 *  Tasten sind echt; ohne laufenden Flug antwortet das Backend mit einem
 *  Fehler, der im Banner erscheint. */
function SprungVorschau() {
  const flug = {
    pirep_id: "b5om4wrXNAm1G9mB",
    airline_icao: "MSC",
    flight_number: "1588",
    callsign: null,
    dpt_airport: "EDDN",
    arr_airport: "HECA",
    phase: "arrived",
    was_just_resumed: false,
    divert_hint: null,
    unmoeglicher_sprung: true,
    abgabe_sperre: "sprung",
  } as unknown as ActiveFlightInfo;
  // Beide Gründe untereinander: MSC1588 (Sprung) und GAF 9655 (Landung
  // nach Unterbrechung nicht bewertet). Im Cockpit erscheint nur einer.
  const gaf9655 = {
    ...flug,
    pirep_id: "3Vv2QegWN1lye2J0",
    airline_icao: "GAF",
    flight_number: "9655",
    dpt_airport: "LAKU",
    arr_airport: "EDDN",
    unmoeglicher_sprung: false,
    abgabe_sperre: "landung_fehlt",
  } as unknown as ActiveFlightInfo;
  return (
    <div
      style={{
        minHeight: "100vh",
        padding: 24,
        background: "var(--bg)",
        display: "grid",
        gap: 24,
      }}
    >
      {[flug, gaf9655].map((f) => (
        <SprungBanner
          key={f.pirep_id}
          activeFlight={f}
          onFiledSuccess={() => {}}
          onDiscarded={() => {}}
        />
      ))}
    </div>
  );
}

/** Vorschau des VDGS-Bandes: die Zustaende nebeneinander, mit
 *  erfundenen, aber formgleichen Werten aus der viffsys-Antwort. Im
 *  Cockpit erscheint immer nur einer davon — und nur, wenn es einen
 *  CDM-Eintrag gibt. */
function VdgsVorschau() {
  const leer = {
    eobt: "",
    tobt: "",
    tsat: "",
    ctot: "",
    taxi_min: null,
    cdm_sts: "",
    regulierung: "",
    rwy_sid: "",
    aobt: "",
  };
  const jetzt = new Date();
  const inMin = (n: number) => {
    const d = new Date(jetzt.getTime() + n * 60_000);
    return `${String(d.getUTCHours()).padStart(2, "0")}:${String(
      d.getUTCMinutes(),
    ).padStart(2, "0")}`;
  };
  const faelle: Array<[string, VdgsStand | null]> = [
    [
      "Freigegeben — TSAT in Reichweite",
      {
        ...leer,
        callsign: "GSG421",
        departure: "LEBL",
        eobt: inMin(15),
        tobt: inMin(2),
        tsat: inMin(4),
        taxi_min: 9,
        cdm_sts: "COMPLY",
        rwy_sid: "24L/OLOXO3Q",
      },
    ],
    [
      "TOBT gesetzt, noch warten",
      {
        ...leer,
        callsign: "GSG118",
        departure: "EDDM",
        eobt: inMin(25),
        tobt: inMin(21),
        tsat: "",
        taxi_min: 14,
        cdm_sts: "COMPLY",
      },
    ],
    [
      "Anlassfenster verpasst — kein CDM-Platz (wie EDDC)",
      {
        ...leer,
        callsign: "AIB4TK",
        departure: "EDDC",
        eobt: inMin(-8),
        tobt: inMin(-8),
        tsat: "",
        taxi_min: 10,
        cdm_sts: "COMPLY",
      },
    ],
    [
      "Off-Block — Fenster erfüllt",
      {
        ...leer,
        callsign: "AIB4TK",
        departure: "EDDC",
        eobt: inMin(-8),
        tobt: inMin(-8),
        tsat: "",
        taxi_min: 10,
        cdm_sts: "COMPLY",
        aobt: inMin(-3),
      },
    ],
    [
      "Regulierung — CTOT greift",
      {
        ...leer,
        callsign: "GSG7L",
        departure: "LIRF",
        eobt: inMin(40),
        tobt: inMin(38),
        tsat: inMin(38),
        ctot: inMin(55),
        taxi_min: 18,
        cdm_sts: "FLS-NRA",
        regulierung: "LECMCTA",
        rwy_sid: "25/XENOL5A",
      },
    ],
    // Kein CDM-Eintrag: die schmale Zeile mit dem Rufzeichen. Ohne sie
    // war nicht zu sehen, WOMIT gefragt wurde (20.09.2026).
    ["Kein Eintrag — Rufzeichen pruefbar", null],
  ];

  return (
    <div
      style={{
        minHeight: "100vh",
        padding: 24,
        background: "var(--bg)",
        display: "grid",
        gap: 24,
        gridTemplateColumns: "repeat(auto-fit, minmax(340px, 1fr))",
        alignContent: "start",
        gridAutoRows: "min-content",
      }}
    >
      {faelle.map(([titel, stand]) => (
        <div key={titel} style={{ display: "grid", gap: 8, gridAutoRows: "min-content" }}>
          <div
            style={{
              font: "var(--fw-bold) var(--fs-200)/1 var(--font-mono)",
              letterSpacing: "var(--ls-caps)",
              textTransform: "uppercase",
              color: "var(--text-dim)",
            }}
          >
            {titel}
          </div>
          <VdgsPlatte antwort={{ gefragt_als: stand?.callsign ?? "GSG421", stand }} />
        </div>
      ))}
    </div>
  );
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
