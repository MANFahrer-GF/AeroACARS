// v1.3.0 (#Hoppie-PDC-CPDLC) — top-level "CPDLC needs attention" banner.
// Mirrors DivertBanner.tsx's shape. Visible on ANY tab (rendered in
// App.tsx alongside the other top-level banners), so a pilot on e.g. the
// Cockpit tab still sees an uplink instruction is waiting for a reply.

import { useTranslation } from "react-i18next";

interface Props {
  count: number;
  onOpenTab: () => void;
  onDismiss: () => void;
}

export function CpdlcMessageBanner({ count, onOpenTab, onDismiss }: Props) {
  const { t } = useTranslation();
  if (count === 0) return null;

  return (
    <section className="cpdlc-banner" role="alert" aria-live="polite">
      <span className="cpdlc-banner__icon" aria-hidden="true">
        ✉
      </span>
      <p className="cpdlc-banner__text">{t("cpdlc.banner_text", { count })}</p>
      <button type="button" className="button button--primary" onClick={onOpenTab}>
        {t("cpdlc.banner_open")}
      </button>
      <button
        type="button"
        className="cpdlc-banner__dismiss"
        onClick={onDismiss}
        aria-label={t("cpdlc.banner_dismiss")}
      >
        ✕
      </button>
    </section>
  );
}
