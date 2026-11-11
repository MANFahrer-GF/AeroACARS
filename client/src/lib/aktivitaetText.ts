// Aktivitätszeilen in der Sprache des Piloten (3.0.1, Anlass: die Zeile
// „In der Luft nachgetankt“ stand nur auf Deutsch da). Der Kern liefert zu
// einer Zeile optional Schlüssel und Werte (`text`); `message` und `detail`
// bleiben der deutsche Rückfall. Übersetzt wird nur, wenn das Paket ALLE
// Schlüssel kennt: Ein neuer Client mit altem Paket darf nie einen rohen
// Schlüssel zeigen.

export interface AktivitaetTeil {
  schluessel: string;
  werte?: Record<string, unknown>;
}

export interface AktivitaetText {
  zeile: AktivitaetTeil;
  detail?: AktivitaetTeil[];
}

export interface AktivitaetEintrag {
  message: string;
  detail?: string;
  text?: AktivitaetText;
}

type Uebersetzer = (schluessel: string, werte?: Record<string, unknown>) => string;

export function aktivitaetAnzeige(
  e: AktivitaetEintrag,
  t: Uebersetzer,
  kennt: (schluessel: string) => boolean,
): { message: string; detail?: string } {
  const teile = e.text ? [e.text.zeile, ...(e.text.detail ?? [])] : [];
  if (!e.text || !teile.every((x) => kennt(x.schluessel))) {
    return { message: e.message, detail: e.detail };
  }
  const [zeile, ...detail] = teile.map((x) => t(x.schluessel, x.werte ?? {}));
  return { message: zeile, detail: detail.length ? detail.join(" · ") : undefined };
}
