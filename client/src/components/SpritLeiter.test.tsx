/**
 * v1.7.36 — die Sprit-Leiter hält, was sie zeigt.
 *
 * Zwei Zusagen stehen im Code der Leiter, und beide waren bis v1.7.35
 * gebrochen, ohne dass ein Test es merkte:
 *
 *  1. „Der Abstand zwischen den Marken IST der Verbrauch." Die Abhebe-Marke
 *     verschwand bei kleinem Abstand ganz — wo nichts steht, liest niemand
 *     einen Abstand ab.
 *  2. Die Landemarke trifft denselben Punkt, den die Zeile darunter nennt.
 *     Seit v1.7.36 rechnen beide gegen den Tank beim ANLASSEN (`sprit.rs`):
 *     Bei DLH 370 war das genau der Block, 41 644 kg. Gerollt wurden 725 kg
 *     statt geplanter 998 — die Ersparnis erscheint nicht mehr als „273 kg
 *     Übertankung", und Grafik und Zeile sagen beide 3 581 kg übriges Extra
 *     (QS-Vorschlag V-a, 18.09.2026).
 */
import { describe, expect, it } from "vitest";
import { fireEvent, render } from "@testing-library/react";
import { SpritSektion } from "./SpritSektion";
import type { SpritAuswertung } from "../lib/sprit";

function dlh370(ueber: Partial<SpritAuswertung> = {}): SpritAuswertung {
  return {
    fassung: 3,
    bis_sinkflug: { ist_kg: 18688, plan_kg: 19353, abweichung_pct: -3.4, als_kg: false },
    anflug: { ist_kg: 5461, plan_kg: 1865, abweichung_pct: 192.8, als_kg: true },
    zeit_unter_schwelle_min: 15.8,
    schwelle_ft: 9487,
    strecke_anflug_nm: 182,
    plan_strecke_anflug_nm: 139,
    reserve: { status: "intakt", quote_pct: 341.1 },
    reserve_kg: 4916,
    takeoff_fuel_kg: 40919,
    landing_fuel_kg: 16770,
    extra_getankt_kg: 5178,
    extra_genutzt_kg: 1597,
    extra_ungenutzt_kg: 3581,
    contingency_verbraucht: true,
    alternate_und_reserve_intakt: true,
    leiter: {
      taxi_kg: 998,
      trip_kg: 21218,
      contingency_kg: 1061,
      alternate_kg: 8273,
      reserve_kg: 4916,
      extra_kg: 5178,
      block_kg: 41644,
      sonstiges_kg: 0,
      // Aus der Rechnung: 41 644 beim Anlassen − 41 644 Block.
      uebertankung_kg: 0,
    },
    rollen_vor_start: null,
    rollen_nach_landung_kg: null,
    badge: "gruen",
    ...ueber,
  } as SpritAuswertung;
}

/** Die x-Position einer gestrichelten Linie (Abhebe-Marke). */
function abhebeMarke(c: HTMLElement): number | null {
  const l = c.querySelector('line[stroke-dasharray]');
  return l ? Number(l.getAttribute("x1")) : null;
}

/** Die x-Position der Landemarke (durchgezogene Linie). */
function landeMarke(c: HTMLElement): number | null {
  const l = Array.from(c.querySelectorAll("line")).find(
    (e) => !e.getAttribute("stroke-dasharray"),
  );
  return l ? Number(l.getAttribute("x1")) : null;
}

describe("Sprit-Leiter", () => {
  it("die Landemarke trifft, was die Zeile darunter sagt", () => {
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    const W = 560;
    // Die Skala ist der Tank beim Anlassen: Block, keine Übertankung.
    const skala = 41644;
    const mark = landeMarke(container);
    expect(mark).not.toBeNull();

    // Von rechts gemessen steht die Marke bei 16 770 kg Landesprit.
    const kgAnDerMarke = (1 - mark! / W) * skala;
    expect(kgAnDerMarke).toBeCloseTo(16770, 0);

    // Und das Extra, das dabei übrig bleibt, ist genau die Zahl aus der
    // Auswertung.
    const untenImExtra = 4916 + 8273; // Reserve + Alternate
    expect(kgAnDerMarke - untenImExtra).toBeCloseTo(dlh370().extra_ungenutzt_kg!, 0);
  });

  it("der Abstand zwischen den Marken IST der Verbrauch", () => {
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    const W = 560;
    const skala = 41644;
    const ab = abhebeMarke(container);
    const mark = landeMarke(container);
    expect(ab).not.toBeNull();
    expect(mark).not.toBeNull();
    const verbrauchtLautGrafik = ((mark! - ab!) / W) * skala;
    expect(verbrauchtLautGrafik).toBeCloseTo(40919 - 16770, 0);
  });

  it("zeigt die Abhebe-Marke auch bei kurzem Flug", () => {
    // Abheben und Landen fast gleich — bis v1.7.35 verschwand die Marke.
    const kurz = dlh370({ landing_fuel_kg: 40800 });
    const { container } = render(<SpritSektion sprit={kurz} />);
    expect(abhebeMarke(container)).not.toBeNull();
  });

  it("hält die Abhebe-Marke im Bild, wenn über Plan getankt wurde", () => {
    // 43 919 kg abgehoben bei 41 644 kg Plan-Block: Ohne die mitwachsende
    // Skala wurde die Marke auf x=0 geklemmt, während ihr Etikett den
    // echten Wert nannte.
    const basis = dlh370();
    const tanker = dlh370({
      takeoff_fuel_kg: 43919,
      // Die Rechnung liefert die Übertankung mit: 44 644 beim Anlassen
      // − 41 644 Block.
      leiter: { ...basis.leiter!, uebertankung_kg: 3000 },
    });
    const { container } = render(<SpritSektion sprit={tanker} />);
    const ab = abhebeMarke(container);
    expect(ab).not.toBeNull();
    expect(ab!).toBeGreaterThan(0);
    const W = 560;
    const skala = 41644 + 3000;
    expect((1 - ab! / W) * skala).toBeCloseTo(43919, 0);
  });

  it("läuft nicht über die Breite, wenn die Posten den Block übersteigen", () => {
    // Ein OFP, dessen sechs Posten mehr ergeben als der Block (dann ist
    // „Zusatzsprit" 0): Bis v1.7.36-Entwurf liefen die Balken über den
    // Rand hinaus (QS-Vorschlag V-a, 18.09.2026).
    const basis = dlh370();
    const knapp = dlh370({ leiter: { ...basis.leiter!, block_kg: 40000 } });
    const { container } = render(<SpritSektion sprit={knapp} />);
    const rects = [...container.querySelectorAll("svg rect")];
    const rechts = Math.max(
      ...rects.map((r) => Number(r.getAttribute("x")) + Number(r.getAttribute("width"))),
    );
    expect(rechts).toBeLessThanOrEqual(560 + 0.01);
    expect(rechts).toBeCloseTo(560, 1);
  });

  it("trifft auch bei Posten über dem Block die Zeile „Extra ungenutzt“", () => {
    // Werte, wie `sprit.rs` sie für diesen Fall liefert: Block 40 000, die
    // sechs Posten 41 644, Tank beim Anlassen 41 644 — Über- und
    // Untertankung zählen gegen den Plan-Stapel (hier die Posten), also
    // keine von beiden. Skala = Posten = 41 644; geplant
    // gelandet mit 41 644 − 998 − 21 218 = 19 428; 16 770 gelandet → 2 658
    // mehr, Contingency 1 061, Extra genutzt 1 597, ungenutzt 3 581.
    const basis = dlh370();
    const knapp = dlh370({
      leiter: { ...basis.leiter!, block_kg: 40000, uebertankung_kg: 0 },
      extra_genutzt_kg: 1597,
      extra_ungenutzt_kg: 3581,
    });
    const { container } = render(<SpritSektion sprit={knapp} />);
    const W = 560;
    const skala = 41644;
    const kgAnDerMarke = (1 - landeMarke(container)! / W) * skala;
    expect(kgAnDerMarke - (4916 + 8273)).toBeCloseTo(3581, 0);
  });

  it("zeichnet bei Untertankung nur das Extra, das an Bord war", () => {
    // 344 kg unter Block angelassen: `sprit.rs` zieht sie vom Extra ab.
    const basis = dlh370();
    const unter = dlh370({
      leiter: { ...basis.leiter!, uebertankung_kg: 0, untertankung_kg: 344 },
      extra_getankt_kg: 4834,
      extra_genutzt_kg: 1253,
      extra_ungenutzt_kg: 3581,
    });
    const { container } = render(<SpritSektion sprit={unter} />);
    const W = 560;
    const skala = 41644 - 344;
    const breiten = [...container.querySelectorAll("svg rect")].map((r) => Number(r.getAttribute("width")));
    // Taxi, Trip, Contingency, Extra, Alternate, Reserve.
    expect(breiten[3]).toBeCloseTo(((5178 - 344) / skala) * W, 1);
    const kgAnDerMarke = (1 - landeMarke(container)! / W) * skala;
    expect(kgAnDerMarke - (4916 + 8273)).toBeCloseTo(3581, 0);
  });

  it("beschriftet den Tankstand beim Einstieg in der Luft nicht als Abheben", () => {
    const { container } = render(<SpritSektion sprit={dlh370({ einstieg_in_der_luft: true })} />);
    const text = container.textContent ?? "";
    expect(text).not.toContain("abgehoben mit");
    expect(text).toContain("in der Luft");
    // Gegenprobe: ohne Kennzeichen bleibt „abgehoben mit".
    const { container: normal } = render(<SpritSektion sprit={dlh370()} />);
    expect(normal.textContent).toContain("abgehoben mit");
  });

  it("erklärt JEDEN gezeichneten Block — auch Contingency und Extra", () => {
    // BIT348 (18.09.2026): Namen standen nur IM Balken und nur, wenn sie
    // hineinpassten — Contingency und Extra blieben unbeschriftet.
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    const bloecke = container.querySelectorAll("svg rect").length;
    const legende = container.querySelector('[data-testid="sprit-leiter-legende"]')!;
    const eintraege = [...legende.children];
    expect(eintraege.length).toBe(bloecke);
    const text = legende.textContent ?? "";
    for (const name of ["Taxi", "Trip", "Contingency", "Extra", "Alternate", "Reserve"]) {
      expect(text, `${name} fehlt in der Legende`).toContain(name);
    }
    // Und jeder Eintrag erklärt sich beim Darüberfahren.
    for (const e of eintraege) {
      fireEvent.mouseEnter(e);
      const h = container.querySelector('[data-testid="sprit-leiter-hinweis"]')?.textContent ?? "";
      expect(h.length, `${e.textContent} ohne Erklärung`).toBeGreaterThan(30);
      fireEvent.mouseLeave(e);
    }
  });

  it("legt keine Schrift ins gedehnte SVG — sie würde mitwachsen", () => {
    // Die Live-Übersicht zog die Leiter auf fast 2 000 px, die Beschriftung
    // wuchs auf das Dreieinhalbfache. Lesbares ist jetzt HTML.
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    const svg = container.querySelector("svg")!;
    expect(svg.querySelectorAll("text").length).toBe(0);
    expect(svg.getAttribute("preserveAspectRatio")).toBe("none");
    // Gegenprobe: die Marken-Texte gibt es trotzdem — als HTML.
    expect(container.textContent).toContain("gelandet mit");
    expect(container.textContent).toContain("abgehoben mit");
  });

  it("lässt den Titel weg, wenn die Karte ihn schon trägt", () => {
    const { container: mit } = render(<SpritSektion sprit={dlh370()} />);
    const { container: ohne } = render(<SpritSektion sprit={dlh370()} ohneTitel />);
    const kopf = (c: HTMLElement) => c.querySelector('[data-testid="sprit-sektion"] > div')!.textContent ?? "";
    expect(kopf(mit)).toMatch(/^Sprit/);
    expect(kopf(ohne)).not.toMatch(/^Sprit/);
    // „Keine Note" bleibt in beiden Fällen stehen.
    expect(kopf(ohne)).toMatch(/keine Note/i);
  });

  it("zeigt Rollen nach der Landung als volle Zeile — im Maßstab von Rollen vor Start", () => {
    // BIT348: Es stand als kleine Textzeile unter den Balken und ging unter.
    const mitRollen = dlh370({
      rollen_vor_start: { ist_kg: 725, plan_kg: 998, abweichung_pct: -27.4, als_kg: false },
      rollen_nach_landung_kg: 649,
    });
    const { container } = render(<SpritSektion sprit={mitRollen} />);
    const balken = container.querySelector('[data-testid="sprit-rollen-nach-balken"]') as HTMLElement;
    expect(balken, "keine eigene Zeile mit Balken").not.toBeNull();
    // Dieselbe Zeilenform wie die Phasen: Beschriftung, Balken, Wert.
    const zeile = balken.parentElement!;
    expect(zeile.textContent).toContain("Rollen nach Landung");
    expect(zeile.textContent).toContain("649 kg");
    expect(zeile.textContent).toContain("ohne Plan");
    // Maßstab: 649 von max(998 Plan, 725 Ist) = 65 %.
    const fuellung = balken.querySelector("i") as HTMLElement;
    expect(parseFloat(fuellung.style.width)).toBeCloseTo((649 / 998) * 100, 1);
    // Gegenprobe: ohne Wert keine Zeile.
    const { container: ohne } = render(<SpritSektion sprit={dlh370({ rollen_nach_landung_kg: null })} />);
    expect(ohne.querySelector('[data-testid="sprit-rollen-nach-balken"]')).toBeNull();
  });

  it("zeigt die Final Reserve als Abstand in kg — Kachel und Zeile", () => {
    // BIT348: „Reserve 732 kg (338 %)" las sich, als hätte die Reserve 338 %.
    const ueber = dlh370({ landing_fuel_kg: 2476, reserve_kg: 732, reserve_abstand_kg: 1744, reserve: { status: "intakt", quote_pct: 338.3 } });
    const t = render(<SpritSektion sprit={ueber} />).container.textContent ?? "";
    expect(t).toContain("+1\u202f744kg");
    expect(t).toContain("2\u202f476 kg gelandet = 732 kg Final Reserve + 1\u202f744 kg darüber");
    expect(t).not.toMatch(/338\s*%/);
    // Darunter: gelb, mit echtem Minus.
    const unter = dlh370({ landing_fuel_kg: 612, reserve_kg: 732, reserve_abstand_kg: -120, badge: "gelb", reserve: { status: "unterschritten", quote_pct: 83.6 } });
    const u = render(<SpritSektion sprit={unter} />).container.textContent ?? "";
    expect(u).toContain("−120kg");
    expect(u).toContain("612 kg gelandet = 732 kg Final Reserve − 120 kg");
  });

  it("erklärt beim Darüberfahren, was der Block IN DIESEM FLUG war", () => {
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    const hinweis = () => container.querySelector('[data-testid="sprit-leiter-hinweis"]')?.textContent ?? null;
    expect(hinweis(), "ohne Maus keine Erklärung").toBeNull();

    // Trip: die beiden Phasen dieses Fluges, mit Plan.
    fireEvent.mouseEnter(container.querySelector('svg rect[data-block="Trip"]')!);
    expect(hinweis()).toContain("In diesem Flug");
    expect(hinweis()).toContain("bis Sinkflug 18\u202f688 kg (Plan 19\u202f353 kg)");
    expect(hinweis()).toContain("Sinkflug + Anflug 5\u202f461 kg (Plan 1\u202f865 kg)");

    // Contingency: in dieser Vorlage verbraucht.
    fireEvent.mouseEnter(container.querySelector('svg rect[data-block="Contingency"]')!);
    expect(hinweis()).toContain("aufgebraucht — alle 1\u202f061 kg genutzt");

    // Extra: an Bord, genutzt, übrig — dieselben Zahlen wie die Zeile.
    fireEvent.mouseEnter(container.querySelector('svg rect[data-block="Extra"]')!);
    expect(hinweis()).toContain("5\u202f178 kg an Bord · 1\u202f597 kg genutzt · 3\u202f581 kg übrig");

    // Final Reserve: der Abstand.
    fireEvent.mouseEnter(container.querySelector('svg rect[data-block="Final Reserve"]')!);
    expect(hinweis()).toContain("11\u202f854 kg darüber gelandet");

    // Maus weg: Erklärung weg.
    fireEvent.mouseLeave(container.querySelector('svg rect[data-block="Final Reserve"]')!);
    expect(hinweis()).toBeNull();

    // Über die Legende genauso — auch per Tastatur.
    const legende = container.querySelector('[data-testid="sprit-leiter-legende"] [data-block="Alternate"]')!;
    fireEvent.focus(legende);
    expect(hinweis()).toContain("noch vollständig an Bord");
  });

  it("erklärt ⓘ und Kacheln sofort — bei Maus, Tastatur und Tippen", () => {
    // Live-Seite, 18.09.2026: „keine Note ⓘ" hatte scheinbar keine Funktion —
    // der Browser-`title` erscheint spät und auf Touch-Geräten nie.
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    const erklaerung = () => container.querySelector('[data-testid="sprit-erklaerung"]');
    expect(container.querySelectorAll("[title]").length, "noch ein Browser-title übrig").toBe(0);
    const bedienbar = [...container.querySelectorAll('[tabindex="0"]')];
    const info = bedienbar.find((e) => e.textContent?.trim() === "ⓘ")!;
    fireEvent.click(info);
    expect(erklaerung()?.textContent?.length ?? 0).toBeGreaterThan(40);
    expect(info.getAttribute("aria-describedby"), "Bildschirmleser bekommt die Erklärung nicht").toBe(erklaerung()!.id);
    fireEvent.blur(info);
    expect(erklaerung()).toBeNull();
    const kachel = bedienbar.find((e) => /Final Reserve/i.test(e.textContent ?? "") && /kg/.test(e.textContent ?? ""))!;
    fireEvent.mouseEnter(kachel);
    expect(erklaerung()?.textContent).toMatch(/Final Reserve/);
    fireEvent.mouseLeave(kachel);
    expect(erklaerung()).toBeNull();
  });

  it("bleibt auf dem Handy (375 px) ganz im Bild — ⓘ und Leiter", () => {
    // QS v1.7.37, N1: Die Erklärung am ⓘ ragte rund 100 px über den
    // Kartenrand, und die Karte schnitt sie ab.
    const breiteVorher = window.innerWidth;
    const rectVorher = Element.prototype.getBoundingClientRect;
    Object.defineProperty(window, "innerWidth", { value: 375, configurable: true });
    // Jedes Element liegt bei x=150 (ⓘ), der Balken spannt 16…359.
    Element.prototype.getBoundingClientRect = function () {
      const balken = (this as HTMLElement).querySelector?.("svg") != null;
      const links = balken ? 16 : 150;
      const breite = balken ? 343 : 14;
      return { left: links, right: links + breite, width: breite, top: 0, bottom: 10, height: 10, x: links, y: 0, toJSON() {} } as DOMRect;
    };
    try {
      const { container } = render(<SpritSektion sprit={dlh370()} />);
      const imBild = (anker: number, el: HTMLElement) => {
        const links = anker + parseFloat(el.style.left || "0");
        const breite = parseFloat(el.style.maxWidth) || parseFloat(el.style.width);
        expect(links, "ragt links hinaus").toBeGreaterThanOrEqual(16);
        expect(links + breite, "ragt rechts hinaus").toBeLessThanOrEqual(375 - 16);
      };
      const info = [...container.querySelectorAll('[tabindex="0"]')].find((e) => e.textContent?.trim() === "ⓘ")!;
      fireEvent.click(info);
      imBild(150, container.querySelector('[data-testid="sprit-erklaerung"]') as HTMLElement);
      // Leiter: der Block ganz rechts (Final Reserve).
      fireEvent.mouseEnter(container.querySelector('svg rect[data-block="Final Reserve"]')!);
      const h = container.querySelector('[data-testid="sprit-leiter-hinweis"]') as HTMLElement;
      const links = 16 + parseFloat(h.style.left);
      expect(links).toBeGreaterThanOrEqual(16);
      expect(links + parseFloat(h.style.maxWidth)).toBeLessThanOrEqual(375 - 16);
    } finally {
      Object.defineProperty(window, "innerWidth", { value: breiteVorher, configurable: true });
      Element.prototype.getBoundingClientRect = rectVorher;
    }
  });

  it("schließt mit Escape und beim Tippen daneben", () => {
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    const erklaerung = () => container.querySelector('[data-testid="sprit-erklaerung"]');
    const info = [...container.querySelectorAll('[tabindex="0"]')].find((e) => e.textContent?.trim() === "ⓘ")!;
    fireEvent.click(info);
    expect(erklaerung()).not.toBeNull();
    fireEvent.keyDown(info, { key: "Escape" });
    expect(erklaerung(), "Escape schließt nicht").toBeNull();
    // iOS: Tippen daneben sendet weder Blur noch mouseleave.
    fireEvent.click(info);
    expect(erklaerung()).not.toBeNull();
    fireEvent.pointerDown(document.body);
    expect(erklaerung(), "Tippen daneben schließt nicht").toBeNull();
    // Gegenprobe: Tippen AUF das ⓘ lässt ihn offen.
    fireEvent.click(info);
    fireEvent.pointerDown(info);
    expect(erklaerung()).not.toBeNull();
  });

  it("schließt den Leiter-Hinweis bei Tippen daneben, Escape und Drehen", () => {
    // QS v1.7.37: B1 (iOS sendet beim Tippen daneben weder mouseleave noch
    // Blur), B2 (Escape an der Legende ungetestet), B3 (Handy gedreht).
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    const hinweis = () => container.querySelector('[data-testid="sprit-leiter-hinweis"]');
    const trip = container.querySelector('svg rect[data-block="Trip"]')!;
    fireEvent.mouseEnter(trip);
    expect(hinweis()).not.toBeNull();
    fireEvent.pointerDown(document.body);
    expect(hinweis(), "Tippen daneben schließt nicht").toBeNull();
    // Gegenprobe: Tippen auf den Balken selbst lässt ihn offen.
    fireEvent.mouseEnter(trip);
    fireEvent.pointerDown(trip);
    expect(hinweis()).not.toBeNull();
    // Escape an der Legende.
    const eintrag = container.querySelector('[data-testid="sprit-leiter-legende"] [data-block="Extra"]')!;
    fireEvent.focus(eintrag);
    expect(hinweis()?.textContent).toContain("Extra");
    fireEvent.keyDown(eintrag, { key: "Escape" });
    expect(hinweis(), "Escape an der Legende schließt nicht").toBeNull();
    // Drehen des Handys: neue Fensterbreite.
    fireEvent.focus(eintrag);
    fireEvent(window, new Event("resize"));
    expect(hinweis(), "neue Fensterbreite schließt nicht").toBeNull();
  });

  it("Flug #1417: Contingency angebrochen, Kachel nennt die Höhe, kein Extra", () => {
    // Thomas, 19.09.2026: „Contingency unberührt — stimmt das? Gelb wurde doch
    // genutzt." 5 637 Skala − 227 Taxi − 2 115 Trip = 3 295 geplant gelandet,
    // tatsächlich 3 209 → 86 von 238 kg genutzt. Der Datensatz stammt aus
    // v1.7.37 und trägt das neue Feld NICHT — die Anzeige muss es selbst
    // richtig ableiten.
    const f1417 = dlh370({
      bis_sinkflug: { ist_kg: 1613, plan_kg: 1736, abweichung_pct: -7.1, als_kg: false },
      anflug: { ist_kg: 622, plan_kg: 379, abweichung_pct: 64.1, als_kg: true },
      zeit_unter_schwelle_min: 14.3,
      schwelle_ft: 8362,
      reserve: { status: "intakt", quote_pct: 284 },
      reserve_kg: 1130,
      takeoff_fuel_kg: 5444,
      landing_fuel_kg: 3209,
      extra_getankt_kg: 0,
      extra_genutzt_kg: 0,
      extra_ungenutzt_kg: 0,
      contingency_verbraucht: false,
      leiter: { taxi_kg: 227, trip_kg: 2115, contingency_kg: 238, alternate_kg: 1927, reserve_kg: 1130, extra_kg: 0, block_kg: 5637, sonstiges_kg: 0, uebertankung_kg: 0 },
    });
    const { container } = render(<SpritSektion sprit={f1417} />);
    const t = container.textContent ?? "";
    expect(t).toContain("Contingency angebrochen — 86 von 238 kg genutzt");
    expect(t).not.toContain("unberührt");
    // Die Kachel erklärt sich selbst, ganze Minuten.
    expect(t).toMatch(/Unter 8\u202f362 ft\s*14\s*min/);
    expect(t).not.toContain("14,3");
    // Kein Extra geplant: keine Nullen-Zeile.
    expect(t).toContain("kein Extra getankt");
    expect(t).not.toContain("0 kg getankt");
    // Und der Hinweis am gelben Block sagt dasselbe.
    fireEvent.mouseEnter(container.querySelector('svg rect[data-block="Contingency"]')!);
    expect(container.querySelector('[data-testid="sprit-leiter-hinweis"]')?.textContent).toContain("angebrochen — 86 von 238 kg genutzt");
  });
});
