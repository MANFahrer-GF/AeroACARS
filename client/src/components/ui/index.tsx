/**
 * AeroACARS — UI-Primitive (Redesign Stufe C).
 *
 * Bewusst rein darstellend: kein i18n, kein Datenzugriff, keine Geschaeftslogik.
 * Texte kommen immer von aussen, damit die Uebersetzung dort bleibt, wo sie
 * heute schon liegt, und beim Umbau nichts verloren geht.
 */

import {
  useCallback,
  useEffect,
  useId,
  useRef,
  type ReactNode,
  type SelectHTMLAttributes,
  type InputHTMLAttributes,
  type TextareaHTMLAttributes,
} from "react";

/* ------------------------------------------------------------------ Button */

export type ButtonVariant = "default" | "primary" | "danger" | "quiet";

export function Button({
  variant = "default",
  size = "md",
  iconLeft,
  children,
  className = "",
  ...rest
}: {
  variant?: ButtonVariant;
  size?: "sm" | "md";
  iconLeft?: ReactNode;
  children?: ReactNode;
} & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  const cls = [
    "ui-btn",
    variant !== "default" ? `ui-btn--${variant}` : "",
    size === "sm" ? "ui-btn--sm" : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <button type="button" className={cls} {...rest}>
      {iconLeft}
      {children}
    </button>
  );
}

/* ------------------------------------------------------------------- Badge */

export type Tone = "neutral" | "ok" | "warn" | "danger" | "info";

export function Badge({
  tone = "neutral",
  children,
  className = "",
  ...rest
}: { tone?: Tone; children?: ReactNode } & React.HTMLAttributes<HTMLSpanElement>) {
  return (
    <span className={`ui-badge ui-badge--${tone} ${className}`} {...rest}>
      {children}
    </span>
  );
}

/* -------------------------------------------------------------------- Card */

export function Card({
  title,
  meta,
  actions,
  flush,
  raised,
  children,
  className = "",
  ...rest
}: {
  title?: ReactNode;
  meta?: ReactNode;
  actions?: ReactNode;
  flush?: boolean;
  raised?: boolean;
  children?: ReactNode;
} & React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={`ui-card ${raised ? "ui-card--raised" : ""} ${className}`} {...rest}>
      {(title || meta || actions) && (
        <div className="ui-card__head">
          {title && <span className="ui-card__title">{title}</span>}
          {meta && <span className="ui-card__meta">{meta}</span>}
          {actions}
        </div>
      )}
      <div className={`ui-card__body ${flush ? "ui-card__body--flush" : ""}`}>
        {children}
      </div>
    </div>
  );
}

/* -------------------------------------------------------------------- Stat */

/**
 * Ein Messwert. `emphasis` steuert die Wahrnehmungsebene:
 *   lead   — im Vorbeischauen erfassbar (Hoehe, Geschwindigkeit)
 *   normal — bei Bedarf gesucht
 *   row    — Zeile in einer Liste
 */
export function Stat({
  label,
  value,
  unit,
  sub,
  emphasis = "normal",
  tone,
  className = "",
  ...rest
}: {
  label: ReactNode;
  value: ReactNode;
  unit?: ReactNode;
  sub?: ReactNode;
  emphasis?: "lead" | "normal" | "row";
  tone?: "ok" | "warn" | "danger";
} & React.HTMLAttributes<HTMLDivElement>) {
  const cls = [
    "ui-stat",
    `ui-stat--${emphasis}`,
    tone ? `ui-stat--${tone}` : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <div className={cls} {...rest}>
      <div className="ui-stat__label">{label}</div>
      <div className="ui-stat__value">
        {value}
        {unit && <span className="ui-stat__unit">{unit}</span>}
      </div>
      {sub && <div className="ui-stat__sub">{sub}</div>}
    </div>
  );
}

/* ------------------------------------------------------------------ Notice */

export type NoticeTone = "info" | "warn" | "error" | "success";

/**
 * Ersetzt die elf einzeln gebauten Banner. `floating` blendet den Hinweis
 * mittig oben ueber der App ein — das brauchte bisher der IntegrityBanner,
 * der dafuer Tailwind-Klassen benutzte, die es im Projekt gar nicht gibt.
 */
export function Notice({
  tone = "info",
  level,
  title,
  detail,
  actions,
  floating,
  role = "status",
  className = "",
  ...rest
}: {
  tone?: NoticeTone;
  level?: ReactNode;
  title?: ReactNode;
  detail?: ReactNode;
  actions?: ReactNode;
  floating?: boolean;
  role?: "status" | "alert";
} & Omit<React.HTMLAttributes<HTMLDivElement>, "title" | "role">) {
  const cls = [
    "ui-notice",
    `ui-notice--${tone}`,
    floating ? "ui-notice--floating" : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <div className={cls} role={role} {...rest}>
      {level && <span className="ui-notice__level">{level}</span>}
      <span className="ui-notice__text">
        {title && <span className="ui-notice__title">{title}</span>}
        {title && detail ? " " : null}
        {detail && <span className="ui-notice__detail">{detail}</span>}
      </span>
      {actions && <span className="ui-notice__actions">{actions}</span>}
    </div>
  );
}

/* ------------------------------------------------------------------- Modal */

/**
 * Eine Umsetzung statt bisher acht — mit EINEM Schleier, EINEM z-index,
 * EINEM Radius. Enthaelt verbindlich: ESC schliesst, Fokus-Falle, Fokus
 * kehrt beim Schliessen zum ausloesenden Element zurueck, korrekte ARIA.
 */
export function Modal({
  open,
  onClose,
  title,
  size = "md",
  footer,
  closeLabel = "Schließen",
  children,
}: {
  open: boolean;
  onClose: () => void;
  title?: ReactNode;
  size?: "sm" | "md" | "lg";
  footer?: ReactNode;
  closeLabel?: string;
  children?: ReactNode;
}) {
  const boxRef = useRef<HTMLDivElement | null>(null);
  const restoreRef = useRef<HTMLElement | null>(null);
  const titleId = useId();

  const focusables = useCallback(() => {
    const root = boxRef.current;
    if (!root) return [] as HTMLElement[];
    return Array.from(
      root.querySelectorAll<HTMLElement>(
        'a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
      ),
    ).filter((el) => el.offsetParent !== null || el === document.activeElement);
  }, []);

  useEffect(() => {
    if (!open) return;
    restoreRef.current = document.activeElement as HTMLElement | null;
    // Erstfokus in den Dialog, sonst haengt der Fokus hinter dem Schleier.
    const first = focusables()[0] ?? boxRef.current;
    first?.focus();

    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key !== "Tab") return;
      const items = focusables();
      if (items.length === 0) return;
      const firstEl = items[0];
      const lastEl = items[items.length - 1];
      const active = document.activeElement;
      if (e.shiftKey && (active === firstEl || !boxRef.current?.contains(active))) {
        e.preventDefault();
        lastEl.focus();
      } else if (!e.shiftKey && active === lastEl) {
        e.preventDefault();
        firstEl.focus();
      }
    }
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      restoreRef.current?.focus?.();
    };
  }, [open, onClose, focusables]);

  if (!open) return null;

  return (
    <div
      className="ui-modal__overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className={`ui-modal ui-modal--${size}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={title ? titleId : undefined}
        ref={boxRef}
        tabIndex={-1}
      >
        {title && (
          <div className="ui-modal__head">
            <span className="ui-modal__title" id={titleId}>
              {title}
            </span>
            <Button
              variant="quiet"
              size="sm"
              className="ui-modal__close"
              onClick={onClose}
              aria-label={closeLabel}
            >
              ✕
            </Button>
          </div>
        )}
        <div className="ui-modal__body">{children}</div>
        {footer && <div className="ui-modal__foot">{footer}</div>}
      </div>
    </div>
  );
}

/* -------------------------------------------------------------------- Tabs */

export interface TabItem {
  id: string;
  label: ReactNode;
  badge?: ReactNode;
  title?: string;
}

/**
 * Vollstaendiges ARIA-Muster inklusive Pfeiltasten, Home/End und
 * `aria-controls`. Bisher gab es 16x role="tab", aber 0x role="tabpanel".
 * Die Leiste scrollt horizontal statt zu ueberlaufen.
 */
export function Tabs({
  items,
  value,
  onChange,
  variant = "underline",
  label,
  panelId,
}: {
  items: TabItem[];
  value: string;
  onChange: (id: string) => void;
  variant?: "underline" | "pill";
  label?: string;
  panelId?: (id: string) => string;
}) {
  const refs = useRef<Record<string, HTMLButtonElement | null>>({});

  function onKeyDown(e: React.KeyboardEvent) {
    const idx = items.findIndex((i) => i.id === value);
    if (idx < 0) return;
    let next = -1;
    if (e.key === "ArrowRight") next = (idx + 1) % items.length;
    else if (e.key === "ArrowLeft") next = (idx - 1 + items.length) % items.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = items.length - 1;
    if (next < 0) return;
    e.preventDefault();
    const id = items[next].id;
    onChange(id);
    refs.current[id]?.focus();
  }

  return (
    <div
      className={`ui-tabs ui-tabs--${variant}`}
      role="tablist"
      aria-label={label}
      onKeyDown={onKeyDown}
    >
      {items.map((item) => {
        const selected = item.id === value;
        return (
          <button
            key={item.id}
            ref={(el) => {
              refs.current[item.id] = el;
            }}
            type="button"
            role="tab"
            id={`tab-${item.id}`}
            aria-selected={selected}
            aria-controls={panelId ? panelId(item.id) : undefined}
            tabIndex={selected ? 0 : -1}
            title={item.title}
            className="ui-tab"
            onClick={() => onChange(item.id)}
          >
            {item.label}
            {item.badge != null && <span className="ui-tab__badge">{item.badge}</span>}
          </button>
        );
      })}
    </div>
  );
}

/** Gegenstueck zu `Tabs` — liefert das fehlende role="tabpanel". */
export function TabPanel({
  id,
  tabId,
  active,
  children,
}: {
  id: string;
  tabId: string;
  active: boolean;
  children?: ReactNode;
}) {
  if (!active) return null;
  return (
    <div role="tabpanel" id={id} aria-labelledby={`tab-${tabId}`} tabIndex={0}>
      {children}
    </div>
  );
}

/* ------------------------------------------------------------------- Field */

type ControlProps =
  | ({ as?: "input" } & InputHTMLAttributes<HTMLInputElement>)
  | ({ as: "select" } & SelectHTMLAttributes<HTMLSelectElement>)
  | ({ as: "textarea" } & TextareaHTMLAttributes<HTMLTextAreaElement>);

/**
 * Beschriftetes Eingabefeld. Grund: acht Felder trugen bisher nur einen
 * Platzhalter und gar kein Label — darunter das Hoppie-Logon-Passwortfeld.
 */
export function Field({
  label,
  hint,
  error,
  required,
  children,
  ...control
}: {
  label: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  required?: boolean;
  children?: ReactNode;
} & ControlProps) {
  const id = useId();
  const hintId = `${id}-hint`;
  const errId = `${id}-err`;
  const { as = "input", className = "", ...rest } = control as {
    as?: "input" | "select" | "textarea";
    className?: string;
  } & Record<string, unknown>;

  const shared = {
    id,
    className: `ui-field__control ${className}`,
    "aria-describedby": [hint ? hintId : null, error ? errId : null]
      .filter(Boolean)
      .join(" ") || undefined,
    "aria-invalid": error ? true : undefined,
    "aria-required": required || undefined,
    ...rest,
  };

  return (
    <div className={`ui-field ${error ? "ui-field--invalid" : ""}`}>
      <label className="ui-field__label" htmlFor={id}>
        {label}
        {required && <span className="ui-field__req">*</span>}
      </label>
      {children ??
        (as === "select" ? (
          <select {...(shared as SelectHTMLAttributes<HTMLSelectElement>)} />
        ) : as === "textarea" ? (
          <textarea {...(shared as TextareaHTMLAttributes<HTMLTextAreaElement>)} />
        ) : (
          <input {...(shared as InputHTMLAttributes<HTMLInputElement>)} />
        ))}
      {hint && (
        <span className="ui-field__hint" id={hintId}>
          {hint}
        </span>
      )}
      {error && (
        <span className="ui-field__error" id={errId}>
          {error}
        </span>
      )}
    </div>
  );
}
