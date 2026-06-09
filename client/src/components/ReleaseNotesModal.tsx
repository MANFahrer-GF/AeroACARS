import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ReleaseNotes } from "../types";

interface Props {
  /** Version string WITHOUT leading 'v' (e.g. "0.1.23"). */
  version: string;
  onClose: () => void;
}

/**
 * "What's new in v{X}" modal — fetches the GitHub release body for
 * the given version and renders it as Markdown. Bilingual: if the
 * body has `## 🇩🇪 Deutsch` and `## 🇬🇧 English` section markers we
 * extract just the section matching the current i18n locale; if only
 * one language exists (or no markers at all) we render the body as-is.
 *
 * Auto-fired once per version after an in-app update (App.tsx tracks
 * `lastSeenVersion` in localStorage). Also re-openable any time from
 * the About panel.
 */
export function ReleaseNotesModal({ version, onClose }: Props) {
  const { t, i18n } = useTranslation();
  const [notes, setNotes] = useState<ReleaseNotes | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const r = await invoke<ReleaseNotes>("fetch_release_notes", {
          version,
        });
        if (!cancelled) {
          setNotes(r);
          setError(null);
        }
      } catch (e) {
        if (cancelled) return;
        // Distinguish between "release doesn't exist yet" and
        // "network is borked" — the user can decide whether to
        // open GitHub or just close.
        const msg = String(e);
        if (msg.includes("not_found")) {
          setError(t("release_notes.error_no_release"));
        } else {
          setError(t("release_notes.error_offline"));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [version, t]);

  const langSection = useMemo<string | null>(() => {
    if (!notes) return null;
    return extractLanguageSection(notes.body, i18n.language);
  }, [notes, i18n.language]);

  const headerTitle = notes
    ? t("release_notes.modal_title", { version: notes.tag_name })
    : t("release_notes.modal_title", { version: `v${version}` });

  const publishedDate = notes
    ? new Date(notes.published_at).toLocaleDateString(i18n.language, {
        year: "numeric",
        month: "long",
        day: "numeric",
      })
    : null;

  const openOnGithub = async () => {
    if (!notes?.html_url) return;
    try {
      await invoke("plugin:opener|open_url", { url: notes.html_url });
    } catch {
      // Fallback: window.open works in dev (Tauri webview supports it)
      window.open(notes.html_url, "_blank", "noopener,noreferrer");
    }
  };

  return (
    <div
      className="modal-backdrop"
      role="dialog"
      aria-modal="true"
      onClick={onClose}
    >
      <div
        className="modal modal--wide release-notes-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="release-notes-modal__header">
          <div>
            <h2 className="modal__title">{headerTitle}</h2>
            {publishedDate && (
              <p className="release-notes-modal__published">
                {t("release_notes.published_prefix")} {publishedDate}
              </p>
            )}
          </div>
        </header>

        <div className="release-notes-modal__body">
          {!notes && !error && (
            <p className="modal__loading">{t("release_notes.loading")}</p>
          )}
          {error && <p className="modal__error">{error}</p>}
          {langSection !== null && langSection.trim().length > 0 && (
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                a: ({ href, children, ...rest }) => (
                  <a
                    {...rest}
                    href={href}
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {children}
                  </a>
                ),
              }}
            >
              {langSection}
            </ReactMarkdown>
          )}
        </div>

        <div className="modal__footer release-notes-modal__footer">
          {notes && (
            <button
              type="button"
              className="button"
              onClick={() => void openOnGithub()}
            >
              {t("release_notes.open_on_github")}
            </button>
          )}
          <button
            type="button"
            className="button button--primary"
            onClick={onClose}
          >
            {t("release_notes.dismiss")}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Pull just the section matching `locale` from a bilingual release-notes
 * body. Recognised section markers (case-insensitive headings):
 *
 *   ## 🇩🇪 Deutsch        / ## DE / ## German
 *   ## 🇬🇧 English        / ## EN / ## Englisch
 *
 * If only one language section exists, return it.
 * If no markers at all (legacy releases), return the full body.
 * If the requested locale's section is missing but the OTHER one
 * exists, fall back to that — better to show some notes than none.
 */
export function extractLanguageSection(body: string, locale: string): string {
  const lang = locale.toLowerCase().startsWith("de") ? "de" : "en";

  // Match a heading line that starts with `## ` and contains either the
  // flag emoji, the language name, or the ISO code. Case-insensitive.
  // The capture catches the line so we know where each section starts.
  const headingRegex = /^##\s+(.+)$/gm;
  type Section = { lang: "de" | "en" | null; start: number; end: number };
  const sections: Section[] = [];
  let m: RegExpExecArray | null;
  while ((m = headingRegex.exec(body)) !== null) {
    const headingText = m[1].toLowerCase();
    let secLang: "de" | "en" | null = null;
    if (
      headingText.includes("🇩🇪") ||
      /\bde\b/.test(headingText) ||
      headingText.includes("deutsch") ||
      headingText.includes("german")
    ) {
      secLang = "de";
    } else if (
      headingText.includes("🇬🇧") ||
      headingText.includes("🇺🇸") ||
      /\ben\b/.test(headingText) ||
      headingText.includes("english") ||
      headingText.includes("englisch")
    ) {
      secLang = "en";
    }
    if (secLang) {
      // Close the previous section (if any) at this heading's start.
      if (sections.length > 0 && sections[sections.length - 1].end === -1) {
        sections[sections.length - 1].end = m.index;
      }
      sections.push({ lang: secLang, start: m.index + m[0].length, end: -1 });
    }
  }
  // Close the last open section at end-of-body.
  if (sections.length > 0 && sections[sections.length - 1].end === -1) {
    sections[sections.length - 1].end = body.length;
  }

  if (sections.length === 0) {
    // No language markers at all — legacy / unilingual release.
    return body;
  }

  const wanted = sections.find((s) => s.lang === lang);
  if (wanted) {
    return body.substring(wanted.start, wanted.end).trim();
  }
  // Requested language missing — fall back to the OTHER language so
  // the pilot at least sees something.
  const fallback = sections[0];
  return body.substring(fallback.start, fallback.end).trim();
}
