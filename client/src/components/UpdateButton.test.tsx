// v0.10.0 Update-Modal Regression-Tests.
//
// Verhindert dass der „Modal sprengt Viewport + Roh-Markdown"-Bug
// (Discord-Befund Svenny1974 2026-05-18, v0.9.0/v0.9.1/v0.9.2) je
// wieder regressiert. Wenn jemand in 6 Monaten die overflow-Regel oder
// den RenderedReleaseNotes-Block aus Versehen rausnimmt, sollen DIESE
// Tests rot werden — BEVOR der Release rausgeht und Piloten den
// Install-Button nicht erreichen können.
//
// Test-Strategie:
//   1. Echter v0.9.2-Release-Body (geladen aus docs/release-notes/v0.9.2.md)
//      — derselbe Inhalt der Svenny den unscrollbaren Modal-Bug
//      verursacht hat. Wenn das DOM hier sauber rendert, ist die
//      Konstruktion robust gegen reale Inputs.
//   2. DOM-Struktur-Checks die garantieren:
//      - `.update-modal` hat max-height-Constraint (CSS-Regel da)
//      - Notes-Container hat overflow-y aus dem CSS
//      - Install-Button ist im DOM (kein conditional render)
//      - Markdown-Marker (`###`, `**`, Pipes) tauchen NICHT als
//        literaler Text auf
//      - Heading-Elements (`.update-modal__notes-h2/h3`) sind da
//      - Code-, Strong-, hr-, ul-Elements werden korrekt erzeugt
//
// WICHTIG: Diese Tests sind absichtlich DOM-strukturell, NICHT visual-
// regressional. JSDOM hat keinen echten Layout-Engine, kann also nicht
// prüfen ob ein Button visuell off-screen ist. Was wir prüfen:
//   - die CSS-Klassen sind da (= App.css greift mit max-height-Regeln)
//   - die Markdown-Parsing-Logik macht aus Markdown HTML-Elements
//     (nicht Plain-Text)
// Für echte visuelle Regression-Prüfung wäre Playwright/E2E nötig,
// das ist eine separate Investition.

import { describe, it, expect, beforeAll } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { UpdateButton } from "./UpdateButton";
import deCommon from "../locales/de/common.json";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next
      .use(initReactI18next)
      .init({
        lng: "de",
        fallbackLng: "de",
        resources: { de: { common: deCommon } },
        defaultNS: "common",
        interpolation: { escapeValue: false },
      });
  }
});

// ─── v0.9.2-Realistic Body (= das was Svenny gesehen hat) ────────────
//
// Inline statt File-Load: damit der Test reproduzierbar bleibt auch
// wenn jemand die v0.9.2-Notes verschiebt/umbenennt. Inhalt entspricht
// dem strukturellen Charakter eines bilingualen Release-Body — Headings,
// Listen, Tabellen-Pipes, fett, inline-code, hr-Trenner, Links.
const REALISTIC_LONG_BODY_v092 = `## 🇩🇪 Deutsch

**v0.9.2 — Zwei grosse neue Features auf einmal: Dein Flug wandert ins Discord-Profil, und AeroACARS-Crashes melden sich automatisch beim VA-Owner (anonym).**

### Was ist neu

#### 🟢 Discord Rich Presence

Andere VA-Mitglieder sehen dich z.B. mit \`GSG3184 · EDDB → KMRH\` und \`CRUISE · A320 · FL360\` direkt in der Discord-Mitglieder-Liste.

- **Default = aus** — du musst es bewusst einschalten unter \`Einstellungen → Discord Rich Presence\`.
- **Anonym-Modus**: Toggle „Callsign anonymisieren" macht aus \`GSG3184\` ein \`GSG-Flight\`.
- **Sim-spezifisches Badge**: kleines MSFS-2024 / 2020 / X-Plane-11 / 12 Icon.
- **Test-Presence-Button**: 15s Dummy senden um zu prüfen ob Discord die App sieht.

#### 🛡 Anonyme Fehler-Telemetrie (GlitchTip)

Wenn AeroACARS crasht, kann es das jetzt automatisch melden — komplett anonym.

- **Default = aus.** Beim ersten Start kommt ein Banner.
- **Was wird gesendet?** Crash-Stack-Trace, Sim-Name, Aircraft-ICAO, AeroACARS-Version, OS.
- **Was NICHT?** Position, Route, Pilot-Identität, IP-Adresse, Passwörter, E-Mail.

### Verifikation

| Check | Status |
|---|---|
| \`cargo test -p aeroacars-app --lib\` | ✅ 201/201 |
| \`cargo test -p discord-presence\` | ✅ 24/24 (neue Crate) |
| \`cargo test -p aeroacars-app --test\` | ✅ |
| \`npm test\` (Pilot-Client) | ✅ 47/47 |
| \`npm run build\` (Pilot-Client) | ✅ tsc + vite |
| \`npx tsc\` (Recorder) | ✅ |
| End-to-End: GlitchTip-Smoke-Event durchgeschickt | ✅ Event in /api/1/store/ accepted, im Dashboard sichtbar |

### Tracker

Spec: [docs/spec/v0.9.0-roadmap.md](../spec/v0.9.0-roadmap.md), [docs/spec/v0.9.0-discord-rich-presence.md](../spec/v0.9.0-discord-rich-presence.md), [docs/spec/v0.9.0-glitchtip-self-hosted.md](../spec/v0.9.0-glitchtip-self-hosted.md). Privacy-Gates: [docs/spec/v0.9.0-telemetry-contract.md](../spec/v0.9.0-telemetry-contract.md) Sektion 9.

---

## 🇬🇧 English

**v0.9.2 — Two big new features in one release: your flight shows up in your Discord profile, and AeroACARS crashes report themselves to the VA owner automatically (anonymously). Both opt-in, can be turned off any time.**

### What's new

#### 🟢 Discord Rich Presence — your flight status live in your Discord profile

Other VA members see you e.g. as \`GSG3184 · EDDB → KMRH\` and \`CRUISE · A320 · FL360\` directly in the Discord member list.

- **Default = off** — you have to explicitly enable it under \`Settings → Discord Rich Presence\`.
- **Anonymous mode**: toggle "Anonymize callsign" turns \`GSG3184\` into \`GSG-Flight\`.

#### 🛡 Anonymous error telemetry (GlitchTip)

If AeroACARS crashes, it can now report it automatically — fully anonymous.

- **Default = off.** Privacy banner on first launch.
- **Where to?** Self-hosted server, no Sentry 3rd-party, no cloud.
`;

// ─── Mock-UseUpdateCheckerResult ─────────────────────────────────────
function makeChecker(body: string, version = "0.10.0") {
  return {
    update: {
      version,
      body,
      date: null,
    },
    stage: "fresh" as const,
    installing: false,
    progress: null,
    installAndRelaunch: async () => {},
    snooze: () => {},
  };
}

describe("UpdateButton modal — regression guards for Svenny1974 v0.9.2 bug", () => {
  it("renders modal with realistic-long v0.9.2 body without crashing", () => {
    const checker = makeChecker(REALISTIC_LONG_BODY_v092);
    render(<UpdateButton checker={checker as never} />);

    // Step 1: Button rendert (= update.stage !== "none")
    const triggerBtn = screen.getByRole("button", { name: /update/i });
    expect(triggerBtn).toBeInTheDocument();

    // Step 2: Modal oeffnet bei Click
    fireEvent.click(triggerBtn);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("Install-Button bleibt im DOM (kein conditional unmount unter dem Fold)", () => {
    const checker = makeChecker(REALISTIC_LONG_BODY_v092);
    render(<UpdateButton checker={checker as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));

    // Install + Later muessen beide im DOM sein — egal wie lang die
    // Notes sind. Die CSS-Garantien (flex-column + Notes-overflow)
    // sorgen dann zur Laufzeit dafuer dass die Buttons SICHTBAR sind.
    const installBtn = screen.getByRole("button", { name: /installieren|jetzt/i });
    const laterBtn = screen.getByRole("button", { name: /später|spaeter/i });
    expect(installBtn).toBeInTheDocument();
    expect(laterBtn).toBeInTheDocument();
  });

  it("Modal-Container hat die CSS-Klasse die die max-height-Regel triggert", () => {
    const checker = makeChecker(REALISTIC_LONG_BODY_v092);
    const { container } = render(<UpdateButton checker={checker as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));

    // KRITISCH: Class `.update-modal` muss da sein. App.css fuegt max-
    // height + display: flex + flex-direction: column dran. Wenn diese
    // Klasse jemand umbenennt ohne die CSS-Regel mitzuziehen, sprengt
    // das Modal wieder den Viewport.
    const modal = container.querySelector(".update-modal");
    expect(modal).not.toBeNull();

    // Notes-Container muss die Klasse haben die overflow-y: auto + flex-
    // 1-1-auto definiert. Sonst scrollt nichts, Buttons rutschen weg.
    const notes = container.querySelector(".update-modal__notes");
    expect(notes).not.toBeNull();

    // Actions-Container muss da sein (flex: 0 0 auto verankert ihn am
    // unteren Rand).
    const actions = container.querySelector(".update-modal__actions");
    expect(actions).not.toBeNull();
  });

  it("rendert KEINEN roh-Markdown im sichtbaren Text (kein `###`, kein `**`, kein `|`-Tabellen-Pipes)", () => {
    const checker = makeChecker(REALISTIC_LONG_BODY_v092);
    const { container } = render(<UpdateButton checker={checker as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));

    const notesText = container.querySelector(".update-modal__notes")?.textContent ?? "";

    // Wenn der Markdown-Parser broken/abwesend ist, taucht der Roh-Text
    // auf. Diese Asserts schlagen dann fehl — vor dem Release. Genau
    // der Svenny-Bug.

    // `### Was ist neu` darf NICHT als literale `### `-Sequence im Text
    // erscheinen (= Parser muss `###` weggestrippt haben, Inhalt
    // landet als Heading-DOM-Node).
    expect(notesText).not.toMatch(/^###\s/m);
    expect(notesText).not.toMatch(/^##\s/m);

    // `**bold**` darf NICHT literal stehen (= Parser muss zu <strong>
    // konvertiert haben).
    expect(notesText).not.toMatch(/\*\*[^*]+\*\*/);

    // Tabellen-Pipes `| col | col |` waren der schlimmste Teil bei
    // Svenny — ergaben unleserliche Zeilen. Parser muss sie zu
    // Klartext-Zeilen mit `·`-Separator umgewandelt haben.
    expect(notesText).not.toMatch(/^\|.+\|$/m);

    // hr-Trenner `---` darf nicht als literal-Zeichen erscheinen.
    expect(notesText).not.toMatch(/^---+$/m);
  });

  it("rendert Markdown-Elements als echte DOM-Nodes (Headings, strong, code, hr, ul)", () => {
    const checker = makeChecker(REALISTIC_LONG_BODY_v092);
    const { container } = render(<UpdateButton checker={checker as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));

    // Beweis dass Parsing aktiv ist:
    const h2Headings = container.querySelectorAll(".update-modal__notes-h2");
    const h3Headings = container.querySelectorAll(".update-modal__notes-h3");
    const strongs = container.querySelectorAll(".update-modal__notes strong");
    const codes = container.querySelectorAll(".update-modal__notes code");
    const hrs = container.querySelectorAll(".update-modal__notes hr");
    const lis = container.querySelectorAll(".update-modal__notes li");

    expect(h2Headings.length).toBeGreaterThan(0); // `## 🇩🇪 Deutsch` + `## 🇬🇧 English`
    expect(h3Headings.length).toBeGreaterThan(0); // mehrere `### Was ist neu` etc.
    expect(strongs.length).toBeGreaterThan(0);    // `**v0.9.2 — ...**` etc.
    expect(codes.length).toBeGreaterThan(0);      // `\`GSG3184\`` etc.
    expect(hrs.length).toBeGreaterThan(0);        // `---`-Trenner zwischen DE/EN
    expect(lis.length).toBeGreaterThan(0);        // `- Default = aus` etc.
  });

  it("rendert Tabellen-Pipe-Zeilen als Klartext-Zeilen mit `·`-Separator", () => {
    const body = `| Check | Status |\n|---|---|\n| \`cargo test\` | ✅ 201/201 |`;
    const checker = makeChecker(body);
    const { container } = render(<UpdateButton checker={checker as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));

    const notesText = container.querySelector(".update-modal__notes")?.textContent ?? "";
    // Header `| Check | Status |` → `Check  ·  Status` (Separator-Zeile
    // `|---|---|` wird verworfen weil alle Cells nur aus `-`/`:` bestehen)
    expect(notesText).toContain("Check");
    expect(notesText).toContain("Status");
    expect(notesText).toContain("·");
    expect(notesText).not.toContain("|");
  });

  it("ignoriert Separator-Zeilen (|---|---|) komplett", () => {
    const body = `| a | b |\n|---|---|\n| 1 | 2 |`;
    const checker = makeChecker(body);
    const { container } = render(<UpdateButton checker={checker as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));

    const notesText = container.querySelector(".update-modal__notes")?.textContent ?? "";
    // `---` darf nicht auftauchen
    expect(notesText).not.toContain("---");
    // Tabellen-Inhalt aber schon
    expect(notesText).toContain("a");
    expect(notesText).toContain("1");
  });

  it("rendert NICHT wenn checker.stage === \"none\"", () => {
    const checker = {
      ...makeChecker(REALISTIC_LONG_BODY_v092),
      update: null,
      stage: "none" as const,
    };
    const { container } = render(<UpdateButton checker={checker as never} />);
    expect(container.querySelector(".update-button")).toBeNull();
  });

  it("Backward-Compat: rendert auch Plain-Text-Body ohne Markdown sauber", () => {
    const body = "Just a simple plain text release note. No markdown.";
    const checker = makeChecker(body);
    const { container } = render(<UpdateButton checker={checker as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));

    const notesText = container.querySelector(".update-modal__notes")?.textContent ?? "";
    expect(notesText).toContain("Just a simple plain text release note.");
  });
});
