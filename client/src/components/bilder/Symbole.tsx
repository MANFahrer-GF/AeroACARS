// Symbole des Bilder-Tabs (Inline-SVG wie in der Seitenleiste, currentColor).
import type { ReactNode } from "react";

const S = (kinder: ReactNode, breite = 1.8) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={breite} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {kinder}
  </svg>
);

export const Symbol = {
  kamera: S(<><path d="M4 8h3l2-3h6l2 3h3v11H4z" /><circle cx="12" cy="13" r="3.5" /></>),
  senden: S(<path d="M4 12l16-7-5 15-3-6z" />, 2),
  ordner: S(<path d="M3 6h7l2 2h9v11H3z" />),
  papierkorb: S(<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" />),
  haken: S(<path d="M5 12l5 5 9-10" />, 3),
  teil: S(<path d="M6 12h12" />, 3),
  dreh: S(<path d="M12 4a8 8 0 1 1-8 8" />, 2.4),
  ausruf: S(<path d="M12 6v7M12 17v1" />, 2.6),
  suche: S(<><circle cx="11" cy="11" r="6" /><path d="M20 20l-4.5-4.5" /></>, 2),
  zu: S(<path d="M6 6l12 12M18 6L6 18" />),
  auge: S(<><path d="M2 12s4-6 10-6 10 6 10 6-4 6-10 6S2 12 2 12z" /><circle cx="12" cy="12" r="2.5" /></>, 2),
  hochladen: S(<path d="M4 16v3h16v-3M12 4v11M8 8l4-4 4 4" />, 1.6),
  plus: S(<path d="M12 5v14M5 12h14" />, 2),
  regler: S(<path d="M4 7h10M18 7h2M4 17h4M12 17h8M14 4v6M8 14v6" />),
  herunter: S(<path d="M4 16v3h16v-3M12 4v11M8 11l4 4 4-4" />),
  stern: S(<path d="M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z" />),
};
