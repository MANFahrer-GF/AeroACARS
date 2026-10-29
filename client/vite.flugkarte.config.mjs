// v3 (Etappe 5, P-8.21): Die Flugkarte als eigenständige Seite im Paket
// (dist/flugkarte/ → ui/flugkarte/). Relative Adressen, damit sie überall
// läuft: im Paketordner im Browser, unter aeroui:// und auf live unter
// /api/bundle/ui/flugkarte/. Die Texte `flugkarte.*` kommen aus bundle/texte
// (nur dieser Abschnitt, nicht die ganzen Sprachdateien).
import { defineConfig } from "vite";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const TEXTE = "virtual:flugkarte-texte";

export default defineConfig({
  root: resolve(process.cwd(), "src/flugkarte/seite"),
  base: "./",
  plugins: [
    {
      name: "flugkarte-texte",
      resolveId: (id) => (id === TEXTE ? "\0" + TEXTE : null),
      load(id) {
        if (id !== "\0" + TEXTE) return null;
        const texte = {};
        for (const s of ["de", "en", "it"]) {
          texte[s] = JSON.parse(readFileSync(resolve(process.cwd(), `../bundle/texte/${s}.json`), "utf8")).flugkarte;
        }
        return `export default ${JSON.stringify(texte)};`;
      },
    },
  ],
  build: {
    outDir: resolve(process.cwd(), "dist/flugkarte"),
    emptyOutDir: true,
    chunkSizeWarningLimit: 2500,
  },
});
