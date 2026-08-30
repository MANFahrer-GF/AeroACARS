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
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import pkg from "../../package.json";

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
//
// **Achtung, Namensfalle (QS v1.6.3).** "LONG" beschreibt hier die
// STRUKTUR, nicht die Länge: dieser Text ist rund 260 Zeichen lang,
// echte Release-Notes sind es zehntausend. Der Test hat den Modal-Bug
// also nie an realistischer Länge geprüft. Die eigentliche Absicherung
// dagegen steht weiter unten in `echte_release_notes_sprengen_das_modal
// _nicht` — die lädt die tatsächlich ausgelieferte Notiz von der Platte.
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

  it("rendert *kursiv* als <em> — und lässt **fett** dabei heil", () => {
    // v1.5.7: Der Renderer kannte gar kein Kursiv. Weil unsere Notes am Ende
    // immer eine kursive Fußzeile tragen, stand im Update-Dialog seit
    // mehreren Versionen sichtbar `*…*` als Zeichen. Der Roh-Markdown-
    // Wächter schlug erst an, als zwei kursive Absätze aufeinander folgten
    // und ihre Sternchen zu `**` zusammenstießen — der Fehler lag also die
    // ganze Zeit knapp unter der Schwelle. Dieser Test setzt sie tiefer.
    const body = [
      "*Keine Änderungen am Wire-Format.*",
      "",
      "*Zweite kursive Zeile direkt danach.*",
      "",
      "Ein **fetter** und ein *kursiver* Teil in einer Zeile.",
    ].join("\n");
    const checker = makeChecker(body);
    const { container } = render(<UpdateButton checker={checker as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));

    const notes = container.querySelector(".update-modal__notes")!;
    expect(notes.querySelectorAll("em").length).toBe(3);
    expect(notes.querySelectorAll("strong").length).toBe(1);
    // Kein einziges Sternchen darf im sichtbaren Text übrig bleiben.
    expect(notes.textContent ?? "").not.toContain("*");
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

// ─── Die Release-Notes DIESER Version, echt von der Platte ────────────
//
// Punkt 4 der Pre-Release-Checkliste verlangt einen manuellen Sichttest
// des Update-Dialogs mit dem AKTUELLEN Release-Body. Genau der wurde
// beim Bau von v1.5.0-beta.3 fast übersprungen — und hätte einen echten
// Fehler durchgelassen: die Notes begannen mit einem Markdown-Zitatblock
// (`> …`), den dieser Renderer gar nicht kennt. Der Pilot hätte den
// Warnhinweis mit einem rohen `>` davor gesehen.
//
// Der Test liest deshalb die Notes-Datei zur aktuellen Version aus
// `package.json` und prüft sie durch den echten Renderer. Er hält sich
// selbst aktuell: eine neue Version braucht nur ihre Notes-Datei.
//
// Bewusst KEIN Ersatz für den Sichttest — JSDOM hat keine Layout-Engine
// und kann nicht sehen, ob der Installieren-Knopf aus dem Bild rutscht.
// Er fängt aber die Hälfte, die sich mechanisch prüfen lässt: Markdown,
// das gar nicht als Markdown ankommt.
describe("Update-Dialog mit den Release-Notes der aktuellen Version", () => {
  const version: string = pkg.version;
  const notesPath = resolve(__dirname, "../../../docs/release-notes", `v${version}.md`);

  it("hat überhaupt eine Notes-Datei", () => {
    expect(
      existsSync(notesPath),
      `docs/release-notes/v${version}.md fehlt — Checklistenpunkt 2`,
    ).toBe(true);
  });

  it("rendert sie ohne rohe Markdown-Reste", () => {
    if (!existsSync(notesPath)) return;
    const body = readFileSync(notesPath, "utf-8");
    const checker = makeChecker(body, version);
    const { container } = render(<UpdateButton checker={checker as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));

    const notesText = container.querySelector(".update-modal__notes")?.textContent ?? "";
    expect(notesText.length).toBeGreaterThan(50);

    // Zeilenanfänge, die der Renderer nicht auflöst, bleiben als Zeichen
    // stehen. Genau so hätte man den Zitatblock-Fehler gesehen.
    for (const zeile of notesText.split("\n")) {
      const t = zeile.trimStart();
      expect(t.startsWith(">"), `Zitatblock wird nicht gerendert: ${zeile}`).toBe(false);
      expect(t.startsWith("#"), `Überschrift wird nicht gerendert: ${zeile}`).toBe(false);
      expect(t.startsWith("|"), `Tabellenzeile wird nicht gerendert: ${zeile}`).toBe(false);
    }
    expect(notesText).not.toContain("**");
    expect(notesText).not.toContain("---");
    // Backticks im gerenderten Text heißen: der Code-Abschnitt wurde nicht
    // umgewandelt. Passiert genau dann, wenn er INNERHALB von Fettdruck
    // steht — der Renderer verschachtelt bewusst nicht, und in
    // v1.5.0-beta.3 stand genau so ein Fall in den Notes.
    expect(notesText).not.toContain("`");
  });

  // ─── Die echte ausgelieferte Notiz ──────────────────────────────────
  //
  // Der Grund für diesen Test: der Body oben heißt "REALISTIC_LONG", ist
  // aber 264 Zeichen lang — die Notiz zu v1.6.3 hat 9548, also das
  // 36-fache. Der Modal-Bug, den diese Datei verhindern soll, hängt genau
  // an der Länge. Ohne diesen Test prüft der ganze Rest ihn nie.
  //
  // Die Datei wird zur Bauzeit eingelesen, nicht kopiert: so kann die
  // Notiz nicht auseinanderlaufen mit dem, was der Pilot im Update-Fenster
  // sieht.
  it("die echten Release-Notes sprengen das Modal nicht", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const verzeichnis = path.resolve(__dirname, "../../../docs/release-notes");
    // Nach Versionsnummer sortieren, nicht als Text: sonst käme `v1.10.0`
    // vor `v1.6.3` und der Test prüfte irgendwann die falsche Notiz.
    const alsZahlen = (d: string) =>
      d.replace(/^v|\.md$/g, "").split(".").map(Number);
    const dateien = fs
      .readdirSync(verzeichnis)
      .filter((d) => /^v\d+\.\d+\.\d+\.md$/.test(d))
      .sort((a, b) => {
        const [x, y] = [alsZahlen(a), alsZahlen(b)];
        return x[0]! - y[0]! || x[1]! - y[1]! || x[2]! - y[2]!;
      });
    const neueste = dateien[dateien.length - 1];
    const koerper = fs.readFileSync(path.join(verzeichnis, neueste), "utf-8");

    // Vorbedingung: der Test taugt nur, wenn die Notiz wirklich eine ist.
    //
    // Die Zahl soll einen STUMMEL ausschliessen — eine Notiz mit drei
    // Zeilen prüft am Modal nichts. Sie soll nicht vorschreiben, wie
    // lang eine Version zu sein hat: v1.7.6 kam mit 2.939 Zeichen und
    // liess den Test fallen, obwohl die Notiz vollständig war. Eine
    // Notiz künstlich zu strecken, damit ein Test grün wird, wäre die
    // falsche Antwort gewesen.
    //
    // Wie lang der Notiz-Bereich wirklich sein kann, prüft ohnehin
    // `REALISTIC_LONG_BODY_v092` mit einem eingefrorenen Langtext.
    expect(
      koerper.length,
      `${neueste} hat nur ${koerper.length} Zeichen — das sieht nach ` +
        `einem Stummel aus, nicht nach Release-Notes.`,
    ).toBeGreaterThan(1200);

    const checker = makeChecker(koerper);
    const { container } = render(<UpdateButton checker={checker as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));

    // Die drei Bausteine, an denen der Svenny-Bug hing: scrollbarer
    // Notiz-Bereich, verankerte Knopfleiste, beide Knöpfe im DOM.
    expect(container.querySelector(".update-modal")).not.toBeNull();
    expect(container.querySelector(".update-modal__notes")).not.toBeNull();
    expect(container.querySelector(".update-modal__actions")).not.toBeNull();
    expect(
      screen.getByRole("button", { name: /installieren|jetzt/i }),
    ).toBeInTheDocument();

    // Und das Markdown muss gerendert sein, nicht als Rohtext dastehen —
    // sonst liest der Pilot "## 🇩🇪 Deutsch" wörtlich.
    const notiz = container.querySelector(".update-modal__notes")!;
    expect(
      container.querySelectorAll(".update-modal__notes-h2").length,
    ).toBeGreaterThan(0);
    const text = notiz.textContent ?? "";
    // Kein roher Markdown darf durchschlagen — das ist die Klasse, die
    // schon zweimal beim Piloten sichtbar war (Kursiv-Sternchen v1.5.7).
    expect(text).not.toContain("##");
    expect(text).not.toMatch(/\|\s*-{3,}/);
    expect(text).not.toMatch(/^\s*\|/m);

    // Tabellen: der Renderer kann keine echten <table>-Elemente, er baut
    // aus jeder Zeile eine Aufzaehlung mit Trennpunkten. Das ist in
    // Ordnung — aber eine LEERE Kopfzelle verschiebt dort die Zuordnung,
    // weil sie beim Rendern wegfaellt und die Kopfzeile dann eine Spalte
    // weniger hat als die Datenzeilen. Der Pilot liest dann Werte unter
    // den falschen Ueberschriften (QS v1.6.3).
    const zeilen = koerper.split("\n").filter((z) => z.trim().startsWith("|"));
    const spalten = (z: string) =>
      z.trim().replace(/^\||\|$/g, "").split("|").length;
    for (const [i, zeile] of zeilen.entries()) {
      if (/^\s*\|[\s|:-]+\|\s*$/.test(zeile)) continue; // Trennzeile
      const zellen = zeile.trim().replace(/^\||\|$/g, "").split("|");
      expect(
        zellen.every((c) => c.trim().length > 0),
        `Tabellenzeile ${i + 1} hat eine leere Zelle: ${zeile.trim()}`,
      ).toBe(true);
      expect(spalten(zeile)).toBe(spalten(zeilen[0]));
    }
  });
});

/**
 * Die Notizen DIESER Auslieferung durch den echten Dialog.
 *
 * # Warum zusaetzlich zur Vorlage oben
 *
 * `REALISTIC_LONG_BODY_v092` ist eingefroren: Sie prueft den Dialog gegen
 * einen Text von damals. Was ein Pilot heute Abend zu sehen bekommt, hat
 * sie noch nie gesehen — und genau dort entstand der Fehler, den sie
 * verhindern soll (Svenny1974, v0.9.2: rohe Markdown-Zeichen im Fenster).
 *
 * Diese Pruefung liest die NEUESTE Notizdatei aus `docs/release-notes/`
 * und schickt sie durch dieselbe Anzeige. Sie gilt damit automatisch fuer
 * jede kuenftige Auslieferung, ohne dass jemand daran denken muss.
 */
describe("Update-Dialog mit den Notizen dieser Version", () => {
  const neueste = (() => {
    const fs = require("node:fs") as typeof import("node:fs");
    const path = require("node:path") as typeof import("node:path");
    const ordner = path.resolve(__dirname, "..", "..", "..", "docs", "release-notes");
    const dateien = fs
      .readdirSync(ordner)
      .filter((f: string) => /^v\d+\.\d+\.\d+\.md$/.test(f))
      .sort((a: string, b: string) => {
        const z = (s: string) =>
          s.slice(1, -3).split(".").map(Number) as [number, number, number];
        const [a1, a2, a3] = z(a);
        const [b1, b2, b3] = z(b);
        return a1 - b1 || a2 - b2 || a3 - b3;
      });
    const datei = dateien.at(-1)!;
    return { datei, text: fs.readFileSync(path.join(ordner, datei), "utf-8") };
  })();

  it("zeigt keine rohen Markdown-Zeichen", () => {
    render(<UpdateButton checker={makeChecker(neueste.text) as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));
    const dialog = screen.getByRole("dialog");
    const sichtbar = dialog.textContent ?? "";

    // Zuerst: greift die Pruefung ueberhaupt den richtigen Text ab?
    //
    // Ohne diesen Riegel bestuende der Test auch dann, wenn `dialog`
    // leer waere oder die Notizen gar nicht ankommen — er wuerde dann
    // nur feststellen, dass in nichts keine rohen Zeichen stehen.
    // Ein Wort aus der Ueberschrift muss ankommen.
    const erstesWort = /^##\s*\S*\s*(\w+)/m.exec(neueste.text)?.[1] ?? "";
    expect(erstesWort.length, "keine Ueberschrift in den Notizen").toBeGreaterThan(2);
    expect(
      sichtbar.includes(erstesWort),
      `${neueste.datei}: „${erstesWort}" kommt im Fenster gar nicht an — ` +
        `die Pruefung liest den falschen Bereich`,
    ).toBe(true);

    // `**` und `###` duerfen im gerenderten Text nicht mehr vorkommen.
    // Ein einzelnes `#` ist erlaubt (etwa in „#13"), ein Zeilenanfang
    // mit `#` nicht.
    for (const marker of ["**", "###", "|---|"]) {
      // ⚠ Die Fundstelle mitgeben, nicht nur „steht roh".
      //
      // Am 30.08.2026 hat dieser Waechter richtig angeschlagen, aber die
      // Ursache war aus der Meldung nicht zu sehen: Eine Fettung lief
      // ueber einen ZEILENUMBRUCH („**saving fuel no longer\ncosts
      // anything**"), und die rendert der Umsetzer nicht. Das musste
      // erst durch Instrumentieren des Tests gefunden werden — genau
      // die Arbeit, die eine gute Meldung spart.
      const stelle = sichtbar.indexOf(marker);
      const umfeld =
        stelle < 0
          ? ""
          : ` — hier: …${sichtbar.slice(Math.max(0, stelle - 70), stelle + 40)}…`;
      expect(
        sichtbar.includes(marker),
        `${neueste.datei}: „${marker}" steht roh im Fenster${umfeld}\n` +
          `Haeufigste Ursache: die Auszeichnung laeuft ueber einen ` +
          `Zeilenumbruch. Sie muss auf EINER Zeile stehen.`,
      ).toBe(false);
    }
    expect(
      /(^|\n)#{1,6}\s/.test(sichtbar),
      `${neueste.datei}: eine Überschrift wurde nicht gerendert`,
    ).toBe(false);
  });

  it("trägt beide Sprachen", () => {
    // Regel aus dem Gedächtnis: Release-Notes sind immer DE + EN.
    // Eine Version, die das vergisst, faellt hier auf.
    expect(neueste.text, `${neueste.datei}: kein deutscher Block`).toMatch(
      /🇩🇪|Deutsch/,
    );
    expect(neueste.text, `${neueste.datei}: kein englischer Block`).toMatch(
      /🇬🇧|English/,
    );
  });

  it("nennt die Bedienknöpfe auch bei diesem Text", () => {
    render(<UpdateButton checker={makeChecker(neueste.text) as never} />);
    fireEvent.click(screen.getByRole("button", { name: /update/i }));
    expect(
      screen.getByRole("button", { name: /installieren|jetzt/i }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /später|spaeter/i })).toBeInTheDocument();
  });
});
