import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { markeMitVorschau } from "./markenVorschau";

function zeiger(el: HTMLElement, art: string, pointerType = "mouse") {
  const ev = new MouseEvent(art);
  Object.defineProperty(ev, "pointerType", { value: pointerType });
  el.dispatchEvent(ev);
}
const tip = () => document.querySelector<HTMLElement>(".marken-vorschau");
const rect = (left: number, top: number, w: number, h: number) => ({ left, top, right: left + w, bottom: top + h, width: w, height: h }) as DOMRect;

function aufbau(adresse: () => Promise<string | null> = async () => "blob:bild") {
  const el = document.createElement("button");
  el.title = "Bild 12:00Z";
  document.body.appendChild(el);
  const huelle = document.createElement("div");
  huelle.getBoundingClientRect = () => rect(0, 0, 800, 600);
  const map = { getContainer: () => huelle, on: vi.fn(), off: vi.fn() };
  const zu = markeMitVorschau(el, map as never, "Bild 12:00Z", () => ({ url: adresse(), zeilen: ["GSG412", "12:00Z"] }));
  return { el, map, zu };
}
const warten = () => vi.advanceTimersByTimeAsync(200);

describe("markeMitVorschau", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); document.body.innerHTML = ""; });

  it("zeigt beim Ueberfahren Bild und Beschriftung und nimmt den Browser-Tooltip weg", async () => {
    const { el } = aufbau();
    expect(el.hasAttribute("title")).toBe(false);
    zeiger(el, "pointerenter");
    await warten();
    expect(tip()!.querySelector("img")!.getAttribute("src")).toBe("blob:bild");
    expect([...tip()!.querySelectorAll(".marken-vorschau__zeile")].map((z) => z.textContent)).toEqual(["GSG412", "12:00Z"]);
  });
  it("Verlassen schliesst sie und meldet die Karte ab", async () => {
    const { el, map } = aufbau();
    zeiger(el, "pointerenter");
    await warten();
    expect(map.on).toHaveBeenCalledWith("move", expect.any(Function));
    zeiger(el, "pointerleave");
    expect(tip()).toBeNull();
    expect(map.off).toHaveBeenCalledWith("move", map.on.mock.calls[0][1]);
  });
  it("am Tablet erscheint nichts", async () => {
    const { el } = aufbau();
    zeiger(el, "pointerenter", "touch");
    await warten();
    expect(tip()).toBeNull();
  });
  it("kurzes Vorbeifahren oeffnet nichts und laedt nichts", async () => {
    const adresse = vi.fn(async () => "blob:bild");
    const { el } = aufbau(adresse);
    zeiger(el, "pointerenter");
    zeiger(el, "pointerleave");
    await warten();
    expect(tip()).toBeNull();
    expect(adresse).not.toHaveBeenCalled();
  });
  it("Klick schliesst sie, die Grossansicht liegt sonst darunter", async () => {
    const { el } = aufbau();
    zeiger(el, "pointerenter");
    await warten();
    el.click();
    expect(tip()).toBeNull();
  });
  it("Rueckgabe raeumt auf, wenn die Marke verschwindet", async () => {
    const { el, zu } = aufbau();
    zeiger(el, "pointerenter");
    await warten();
    zu();
    expect(tip()).toBeNull();
  });
  it("ein spaet geladenes Bild landet nicht in der naechsten Vorschau", async () => {
    let erstes: (u: string) => void = () => undefined;
    const adresse = vi.fn<() => Promise<string | null>>()
      .mockImplementationOnce(() => new Promise((ok) => { erstes = ok; }))
      .mockImplementationOnce(async () => "blob:zwei");
    const { el } = aufbau(adresse);
    zeiger(el, "pointerenter");
    await warten();
    zeiger(el, "pointerleave");
    zeiger(el, "pointerenter");
    await warten();
    erstes("blob:eins");
    await warten();
    expect([...tip()!.querySelectorAll("img")].map((i) => i.getAttribute("src"))).toEqual(["blob:zwei"]);
  });
  it.each([
    ["kein Bild", async () => null],
    ["Fehler beim Laden", async () => { throw new Error("weg"); }],
  ])("%s: nur die Beschriftung bleibt", async (_n, adresse) => {
    const { el } = aufbau(adresse as () => Promise<string | null>);
    zeiger(el, "pointerenter");
    await warten();
    expect(tip()!.querySelector(".marken-vorschau__bild")).toBeNull();
    expect(tip()!.textContent).toBe("GSG41212:00Z");
  });
  it.each([
    ["keine Daten", () => null],
    ["leere Beschriftung", () => ({ url: Promise.resolve(null), zeilen: [] })],
  ])("%s: der Name der Marke bleibt", async (_n, daten) => {
    const el = document.createElement("button");
    document.body.appendChild(el);
    const huelle = document.createElement("div");
    markeMitVorschau(el, { getContainer: () => huelle, on: vi.fn(), off: vi.fn() } as never, "Bild 12:00Z", daten);
    zeiger(el, "pointerenter");
    await warten();
    expect(tip()!.textContent).toBe("Bild 12:00Z");
  });
  it("steht ueber dem Punkt, am oberen Rand darunter", async () => {
    const offset = vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(140);
    const { el } = aufbau();
    el.getBoundingClientRect = () => rect(100, 300, 20, 20);
    zeiger(el, "pointerenter");
    await warten();
    expect(tip()!.style.top).toBe("150px");
    zeiger(el, "pointerleave");
    el.getBoundingClientRect = () => rect(100, 30, 20, 20);
    zeiger(el, "pointerenter");
    await warten();
    expect(tip()!.style.top).toBe("60px");
    offset.mockRestore();
  });
  it("folgt dem Punkt auch bei geaenderter Kartengroesse und meldet sich wieder ab", async () => {
    const { el, map } = aufbau();
    zeiger(el, "pointerenter");
    await warten();
    const anruf = map.on.mock.calls.find((c) => c[0] === "resize")!;
    el.getBoundingClientRect = () => rect(400, 300, 20, 20);
    (anruf[1] as () => void)();
    expect(tip()!.style.left).not.toBe("8px");
    zeiger(el, "pointerleave");
    expect(map.off).toHaveBeenCalledWith("resize", anruf[1]);
  });
  it("wandert der Punkt aus der Karte, verschwindet sie", async () => {
    const { el, map } = aufbau();
    zeiger(el, "pointerenter");
    await warten();
    expect(tip()!.hidden).toBe(false);
    el.getBoundingClientRect = () => rect(900, 300, 20, 20);
    (map.on.mock.calls[0][1] as () => void)();
    expect(tip()!.hidden).toBe(true);
  });
});
