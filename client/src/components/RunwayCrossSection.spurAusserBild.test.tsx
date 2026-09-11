// Realer Ausschlag jenseits des sichtbaren Streifens muss benannt werden,
// statt dass die Spur kommentarlos am Bildrand aufhört.
//
// Anlass 2026-09-11 (UAE420, A388, YPPH RWY03): Nach dem Räumen schwenkte
// die Maschine real und plausibel bis 100,3 m aus (27 saubere, aufeinander
// aufbauende Proben) — auf einer Bahn mit ≈23 m sichtbarem Streifen brach
// `amRandAbschneiden` davon fast alles ab, übrig blieb ein Knick statt
// einer erkennbaren Kurve. Die Werte unten sind eine gekürzte, aber
// unverändert monotone Teilmenge der echten Samples dieses Flugs.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { erzeugeProjektion } from "../lib/runwayProjection";
import { RunwayCrossSection } from "./RunwayCrossSection";

// Echte Feldnamen aus `RunwayCrossSection`s `tokens`-Prop, siehe
// `RunwayCrossSection.raeumung.test.tsx` (Codex-Befund dritte Runde: die
// vorherige Fassung nutzte erfundene Feldnamen per `as unknown`-Zwang).
const TOKENS: Parameters<typeof RunwayCrossSection>[0]["tokens"] = {
  tarmac: "#1e293b",
  tarmacBorder: "#475569",
  centerline: "#e2e8f0",
  rollout: "#38bdf8",
  tdPerfect: "#22c55e",
  tdWarn: "#f59e0b",
  tdSevere: "#ef4444",
  rollweg: "#38bdf8",
  rollwegRand: "#0ea5e9",
};

/** YPPH RWY03 wie im Bestand: 3444 m lang, 45,1 m breit, keine versetzte
 *  Schwelle. `padX`/`innerW` wie in `RunwayDiscipline.test.tsx` (eddh23). */
function ypph03() {
  return erzeugeProjektion({ lengthM: 3444, ddsM: 0, padX: 70, innerW: 1060 });
}

// Gekürzte, aber unveränderte Teilmenge der echten UAE420-Samples
// (Aufsetzen bei 691 m, Räumen ab ≈1977 m, dann Ausschwenken auf 100,3 m).
const SPUR_ECHT = [
  { laengs_m: 691.1, quer_m: -7.6 },
  { laengs_m: 1000, quer_m: -3.5 },
  { laengs_m: 1916.2, quer_m: 7.7 },
  { laengs_m: 1949.4, quer_m: 13.3 },
  { laengs_m: 1985.3, quer_m: 28.7 },
  { laengs_m: 2030.0, quer_m: 60.6 },
  { laengs_m: 2077.8, quer_m: 100.3 },
];

function zeichne(samples: typeof SPUR_ECHT) {
  return render(
    <RunwayCrossSection
      projektion={ypph03()}
      runwayWidthM={45.1}
      trackWidthM={14.3}
      samples={samples}
      touchdownM={691.1}
      touchdownOffsetM={-7.6}
      clearanceM={1976.5}
      clearanceSide="right"
      width={1200}
      tokens={TOKENS}
    />,
  );
}

describe("Spur läuft weiter — Hinweis am Bildrand", () => {
  it("nennt den realen Ausschlag, wenn die Spur den sichtbaren Streifen verlässt", () => {
    const { container } = zeichne(SPUR_ECHT);
    const texte = Array.from(container.querySelectorAll("text")).map(
      (t) => t.textContent ?? "",
    );
    const treffer = texte.find((s) => s.includes("100 m"));
    expect(treffer, texte.join(" | ")).toBeTruthy();
  });

  it("bleibt still, wenn die Spur den sichtbaren Streifen nie verlässt", () => {
    // Gegenprobe: dieselbe Spur, aber ohne den weiten Ausschwenker danach —
    // das Flugzeug rollt einfach im Streifen weiter.
    const imStreifen = SPUR_ECHT.map((s, i) =>
      i >= 4 ? { laengs_m: s.laengs_m, quer_m: 10 } : s,
    );
    const { container } = zeichne(imStreifen);
    const texte = Array.from(container.querySelectorAll("text")).map(
      (t) => t.textContent ?? "",
    );
    expect(texte.some((s) => s.includes("Spur läuft weiter"))).toBe(false);
  });

  it("hält den Hinweistext im Zeichenbereich", () => {
    const { container } = zeichne(SPUR_ECHT);
    const svg = container.querySelector("svg")!;
    const [, , vbW, vbH] = svg.getAttribute("viewBox")!.split(" ").map(Number);
    for (const el of Array.from(svg.querySelectorAll("text"))) {
      const x = Number(el.getAttribute("x"));
      const y = Number(el.getAttribute("y"));
      if (Number.isFinite(x)) {
        expect(x).toBeGreaterThanOrEqual(0);
        expect(x).toBeLessThanOrEqual(vbW!);
      }
      if (Number.isFinite(y)) {
        expect(y).toBeGreaterThanOrEqual(0);
        expect(y).toBeLessThanOrEqual(vbH!);
      }
    }
  });
});
