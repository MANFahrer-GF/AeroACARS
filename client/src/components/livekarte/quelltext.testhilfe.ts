// Fuer die Quelltext-Waechter der Live-Karte: die Komponente samt ihrer Bausteine als ein Text.
import { readdirSync, readFileSync } from "node:fs";

const ORDNER = "src/components/livekarte";

export function karteQuelltext(): string {
  const bausteine = readdirSync(ORDNER)
    .filter((d) => /\.tsx?$/.test(d) && !/\.test\.|testhilfe/.test(d))
    .sort()
    .map((d) => readFileSync(`${ORDNER}/${d}`, "utf8"));
  return [readFileSync("src/components/LiveMapView.tsx", "utf8"), ...bausteine].join("\n");
}
