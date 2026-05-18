// ⚠ REGRESSION-GUARD — vor Modifikationen an diesem File bitte lesen:
//
//   1. `<RenderedReleaseNotes>` MUSS den `update.body` parsen, NICHT
//      als rohen String in `<p>` schreiben. Der body kommt vom GitHub-
//      Release-Body und ist IMMER Markdown (Headings, Tabellen, Bold).
//      Wenn jemand das durch `<p>{update.body}</p>` ersetzt, sieht der
//      Pilot `### Tracker`, `**bold**`, `| col | col |` als Roh-Text.
//
//   2. Modal-CSS-Constraints (`.update-modal` max-height + flex-column,
//      `.update-modal__notes` overflow-y + flex 1 1 auto) sind nötig
//      damit der Install-Button bei langen Release-Notes nicht unter
//      den Bildschirmrand rutscht. Siehe Warn-Block oberhalb beider
//      CSS-Regeln in App.css.
//
//   3. UpdateButton.test.tsx fängt beide Regressionen — wenn die Tests
//      bei einem Refactor rot werden, bitte VOR dem Release fixen, nicht
//      die Tests anpassen. Siehe docs/release-checklist.md Stufe 3 + 4.
//
//   Background: Discord-Befund Svenny1974 2026-05-18 — v0.9.x Update-
//   Modal kaputt, Pilot konnte nicht updaten. Spec docs/release-notes/
//   v0.10.0.md.

import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { UseUpdateCheckerResult } from "../hooks/useUpdateChecker";

/**
 * Inline „Update verfügbar"-Button im App-Header.
 *
 * v0.5.48: Source-of-Truth für Update-State ist jetzt der zentrale
 * `useUpdateChecker`-Hook (in App.tsx aufgerufen). Diese Komponente
 * konsumiert das Ergebnis. Visuelle Eskalation:
 *
 * - `fresh` (< 24 h gesehen): dezenter Button wie bisher
 * - `pulse` (≥ 24 h ignoriert): Button bekommt sanfte Pulse-Animation
 *   damit der Pilot ihn nicht weiter übersieht
 * - `banner` (≥ 72 h ignoriert): Button glüht zusätzlich + dauerhafte
 *   Pulsation. Parallel macht UpdateBanner das große Banner — Button
 *   bleibt aber sichtbar damit der Pilot direkt installieren kann
 *
 * v0.10.0 Hotfix: Release-Notes werden jetzt leichtgewichtig formatiert
 * statt als rohes Markdown angezeigt. Vorher sah der Pilot bei dem
 * bilingualen v0.9.x-Release-Body Roh-Markdown („### Tracker", Tabellen-
 * Pipes, `**bold**`) UND das Modal sprengte den Viewport, sodass der
 * Install-Button unter dem Fold lag (Discord-Befund Svenny1974
 * 2026-05-18). CSS-Fix in App.css macht das Modal scrollbar; der
 * `RenderedReleaseNotes`-Block hier macht es lesbar — ohne neue
 * NPM-Dependency.
 *
 * Renders nichts wenn Hook `stage === "none"` meldet.
 */
export function UpdateButton({ checker }: { checker: UseUpdateCheckerResult }) {
  const { t } = useTranslation();
  const { update, stage, installing, progress, installAndRelaunch } = checker;
  const [open, setOpen] = useState(false);

  if (!update || stage === "none") return null;

  const cls = [
    "update-button",
    stage === "pulse" ? "update-button--pulse" : "",
    stage === "banner" ? "update-button--escalated" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <>
      <button
        type="button"
        className={cls}
        onClick={() => setOpen(true)}
        title={t("update.button_title", { version: update.version })}
      >
        <span className="update-button__icon" aria-hidden="true">
          ⬇
        </span>
        <span>{t("update.button_label")}</span>
      </button>

      {open && (
        <div
          className="update-modal__backdrop"
          onClick={() => !installing && setOpen(false)}
        >
          <div
            className="update-modal"
            role="dialog"
            aria-labelledby="update-modal-title"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 id="update-modal-title" className="update-modal__title">
              {t("update.modal_title", { version: update.version })}
            </h3>
            {update.body && (
              <div className="update-modal__notes">
                <RenderedReleaseNotes body={update.body} />
              </div>
            )}
            {progress && (
              <p className="update-modal__progress">{progress}</p>
            )}
            <div className="update-modal__actions">
              <button
                type="button"
                className="button button--primary"
                onClick={() => void installAndRelaunch()}
                disabled={installing}
              >
                {installing ? "…" : t("update.install_now")}
              </button>
              <button
                type="button"
                className="button"
                onClick={() => setOpen(false)}
                disabled={installing}
              >
                {t("update.later")}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}

// ─── Lightweight Markdown-Renderer fürs Update-Modal ────────────────
//
// Bewusst KEINE neue NPM-Dependency (react-markdown ist + 90 KB gzipped
// und braucht remark/unified-Plugins für GFM-Tabellen — fürs Update-
// Modal Overkill). Wir parsen ein kleines Markdown-Subset selbst:
//
//   - `## Foo`  → größere Bold-Heading
//   - `### Foo` → kleinere Bold-Heading
//   - `**foo**` → <strong>
//   - `` `foo` `` → <code>
//   - `- foo`   → <ul><li>…
//   - `| a | b |` → entzerrt zu „a · b" als Klartext-Zeile (Tabellen
//     in v0.9.x-Release-Notes hatten Spalten wie „Cargo build" |
//     „2025/205" — als Pipes-Roh-Text war das unleserlich)
//   - `---`     → <hr>
//   - `[text](url)` → text + plain (Modal hat kein Browser-Open-Hook
//     für externe Links; URL wird einfach als Klartext hinten dran
//     gehängt damit Pilot sie wenigstens copy-paste kann)
//
// Alles andere bleibt as-is (Plain-Text-Paragraphs). Wenn die Release-
// Notes auf etwas Komplexeres umschwenken (Bilder, verschachtelte
// Listen), holen wir react-markdown rein — bis dahin reicht das.

interface ParsedLine {
  kind: "h2" | "h3" | "li" | "p" | "hr" | "blank";
  text: string;
}

function parseReleaseNotes(body: string): ParsedLine[] {
  return body
    .replace(/\r\n/g, "\n")
    .split("\n")
    .map((raw): ParsedLine => {
      const line = raw.trimEnd();
      if (line.length === 0) return { kind: "blank", text: "" };
      if (/^---+\s*$/.test(line)) return { kind: "hr", text: "" };
      if (/^##\s+/.test(line)) {
        return { kind: "h2", text: line.replace(/^##\s+/, "") };
      }
      if (/^###\s+/.test(line)) {
        return { kind: "h3", text: line.replace(/^###\s+/, "") };
      }
      if (/^[-*]\s+/.test(line)) {
        return { kind: "li", text: line.replace(/^[-*]\s+/, "") };
      }
      // Tabellen-Zeile (z. B. v0.9.x Cargo-Build-Tabelle): Pipes durch
      // schmale Trenner ersetzen damit der Pilot wenigstens die Spalten-
      // Werte als lesbare Zeile sieht.
      if (line.startsWith("|") && line.endsWith("|") && line.includes("|", 1)) {
        const cells = line
          .slice(1, -1)
          .split("|")
          .map((c) => c.trim())
          .filter((c) => c.length > 0 && !/^[-:\s]+$/.test(c));
        if (cells.length === 0) return { kind: "blank", text: "" };
        return { kind: "p", text: cells.join("  ·  ") };
      }
      return { kind: "p", text: line };
    });
}

/** Wandelt Inline-Markdown (`**bold**`, `` `code` ``, `[txt](url)`) in
 *  React-Nodes um. Bewusst minimal — kein verschachteltes Markdown. */
function renderInline(text: string): ReactNode[] {
  // Pattern matched: **bold**, `code`, [text](url) — in dieser Reihenfolge
  // (längste/spezifischste Pattern zuerst).
  const out: ReactNode[] = [];
  const re = /(\*\*([^*]+)\*\*|`([^`]+)`|\[([^\]]+)\]\(([^)]+)\))/g;
  let lastIdx = 0;
  let m: RegExpExecArray | null;
  let key = 0;
  while ((m = re.exec(text)) !== null) {
    if (m.index > lastIdx) {
      out.push(text.slice(lastIdx, m.index));
    }
    if (m[2] != null) {
      out.push(<strong key={key++}>{m[2]}</strong>);
    } else if (m[3] != null) {
      out.push(<code key={key++}>{m[3]}</code>);
    } else if (m[4] != null) {
      // Link: Text + URL als Klartext (Modal hat keinen Browser-Open-
      // Hook; URL haengen wir an damit Pilot sie zumindest copy-paste
      // kann). Spaeter ggf. via tauri shell.open() echter Link.
      out.push(
        <span key={key++}>
          {m[4]} <code style={{ fontSize: "0.85em" }}>{m[5]}</code>
        </span>,
      );
    }
    lastIdx = m.index + m[0].length;
  }
  if (lastIdx < text.length) out.push(text.slice(lastIdx));
  return out;
}

function RenderedReleaseNotes({ body }: { body: string }) {
  const parsed = parseReleaseNotes(body);
  const out: ReactNode[] = [];
  let currentList: ReactNode[] | null = null;
  let key = 0;

  const flushList = () => {
    if (currentList && currentList.length > 0) {
      out.push(<ul key={`list-${key++}`}>{currentList}</ul>);
    }
    currentList = null;
  };

  for (const line of parsed) {
    if (line.kind !== "li") flushList();
    switch (line.kind) {
      case "h2":
        out.push(
          <div key={key++} className="update-modal__notes-h2">
            {renderInline(line.text)}
          </div>,
        );
        break;
      case "h3":
        out.push(
          <div key={key++} className="update-modal__notes-h3">
            {renderInline(line.text)}
          </div>,
        );
        break;
      case "li":
        if (!currentList) currentList = [];
        currentList.push(<li key={key++}>{renderInline(line.text)}</li>);
        break;
      case "hr":
        out.push(<hr key={key++} />);
        break;
      case "blank":
        // CSS `white-space: pre-line` macht den Zeilenumbruch im
        // umgebenden Container — wir wollen aber keine doppelten Breaks.
        // Statt <br> einfach ein leeres div mit kleinem margin.
        out.push(<div key={key++} style={{ height: "0.5em" }} />);
        break;
      case "p":
      default:
        out.push(<div key={key++}>{renderInline(line.text)}</div>);
        break;
    }
  }
  flushList();
  return <>{out}</>;
}
