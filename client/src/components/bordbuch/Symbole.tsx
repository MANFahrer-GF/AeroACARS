// Bordbuch — Symbole als SVG statt Schriftzeichen: die Cockpit-Schrift
// (B612 Mono) kennt z. B. kein „✓", und die Ersatzschrift verrät sich im
// Screenshot nicht (siehe Gedächtnis „Cockpit-Schrift kennt nicht jedes
// Zeichen").

export function Haken({ size = 12 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 12 12" aria-hidden focusable="false">
      <path d="M2.2 6.4 4.8 9 9.8 3.2" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

export function Kreis({ size = 10 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 10 10" aria-hidden focusable="false">
      <circle cx="5" cy="5" r="3.6" fill="none" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  );
}
