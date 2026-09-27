// Vorschau: Landungs-Detailansicht mit Beispiel-Bordbuch (nur Entwicklung).
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";
import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync(resolve(process.cwd(), "package.json"), "utf-8"));
const ERSATZ = resolve(process.cwd(), "src/dev/ipcBordbuchVorschau.ts");

export default defineConfig({
  plugins: [
    react(),
    {
      // Nur das Bordbuch bekommt die Ersatz-IPC; alles andere bleibt echt.
      name: "bordbuch-ipc-ersatz",
      enforce: "pre",
      resolveId(source, importer) {
        if (source === "../../lib/ipc" && importer?.endsWith("bordbuch/BordbuchLandung.tsx")) return ERSATZ;
        if (source === "../lib/ipc" && importer?.endsWith("components/ResumeFlightBanner.tsx")) return ERSATZ;
        return null;
      },
    },
  ],
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  root: resolve(process.cwd(), "src/dev"),
});
