import { describe, expect, it } from "vitest";
import i18next from "i18next";
import de from "../../../bundle/texte/de.json";
import en from "../../../bundle/texte/en.json";
import italienisch from "../../../bundle/texte/it.json";
import { aktivitaetAnzeige, type AktivitaetEintrag } from "./aktivitaetText";

// Genau das, was der Kern für DLH 41 liefert (`nachtanken_luft_hinweis_text`,
// Rust-Test `nachtanken_luft_tests.rs`).
const eintrag: AktivitaetEintrag = {
  message: "In der Luft nachgetankt: 20762 kg. Der Verbrauch dieses Fluges rechnet die Menge mit.",
  detail: "Tank davor 1384 kg · Die Flugleitung sieht den Vorgang in der Prüfung.",
  text: {
    zeile: { schluessel: "activity.refuel.line", werte: { kg: 20762 } },
    detail: [
      { schluessel: "activity.refuel.before", werte: { kg: 1384 } },
      { schluessel: "activity.refuel.review", werte: {} },
    ],
  },
};

async function sprache(lng: string, texte: object) {
  const i = i18next.createInstance();
  await i.init({ lng, resources: { [lng]: { translation: texte } }, interpolation: { escapeValue: false } });
  return aktivitaetAnzeige(eintrag, (k, w) => i.t(k, w ?? {}), (k) => i.exists(k));
}

describe("Aktivitätszeile Nachtanken in drei Sprachen", () => {
  it("Deutsch aus dem Paket ist wortgleich mit dem Rückfall des Kerns", async () => {
    expect(await sprache("de", de)).toEqual({ message: eintrag.message, detail: eintrag.detail });
  });

  it("Englisch", async () => {
    expect(await sprache("en", en)).toEqual({
      message: "Refuelled in flight: 20762 kg. This flight's fuel burn includes that amount.",
      detail: "tank before 1384 kg · Flight operations will see this in the review.",
    });
  });

  it("Italienisch", async () => {
    expect(await sprache("it", italienisch)).toEqual({
      message: "Rifornimento in volo: 20762 kg. Il consumo di questo volo tiene conto della quantità.",
      detail: "serbatoio prima 1384 kg · La direzione voli vede l'operazione nella revisione.",
    });
  });

  it("altes Paket ohne die Schlüssel: der deutsche Rückfall, nie ein roher Schlüssel", async () => {
    const { activity: _weg, ...alt } = en as Record<string, unknown>;
    expect(await sprache("en", alt)).toEqual({ message: eintrag.message, detail: eintrag.detail });
    const teilweise = { ...alt, activity: { refuel: { line: "x {{kg}}" } } };
    expect(await sprache("en", teilweise)).toEqual({ message: eintrag.message, detail: eintrag.detail });
  });

  it("Zeile ohne Schlüssel bleibt, wie der Kern sie schreibt", () => {
    const roh = { message: "Auto-Start aktiviert", detail: undefined };
    expect(aktivitaetAnzeige(roh, () => "FALSCH", () => true)).toEqual(roh);
  });
});
