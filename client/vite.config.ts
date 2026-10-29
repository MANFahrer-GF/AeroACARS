import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const host = process.env.TAURI_DEV_HOST;

// v0.9.0 (#GlitchTip): Client-Version fuer Sentry-release-tag
const pkg = JSON.parse(
  readFileSync(fileURLToPath(new URL("./package.json", import.meta.url)), "utf8"),
);

// https://vite.dev/config/
// v3 (Etappe 5, P-8.20): Das HUD (Panel-Server /hud) laedt Skript und Stil
// des Widgets ueber denselben Lader wie die Oberflaeche — darum liegen sie im
// Frontend-Bau unter hud/. Quelle bleibt das msfs-panel-Paket (eine Quelle
// mit Flow-Widget und nativem Panel).
const HUD_QUELLE = "../msfs-panel/Build/PackageSources/html_ui/InGamePanels/AeroACARSPanel/";
const hudDateien = {
  name: "hud-dateien",
  apply: "build" as const,
  generateBundle(this: { emitFile: (f: { type: "asset"; fileName: string; source: string }) => void }) {
    for (const datei of ["panel.js", "panel.css"]) {
      this.emitFile({
        type: "asset",
        fileName: `hud/${datei}`,
        source: readFileSync(fileURLToPath(new URL(HUD_QUELLE + datei, import.meta.url)), "utf8"),
      });
    }
  },
};

export default defineConfig(async () => ({
  plugins: [react(), hudDateien],

  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },

  // v0.8.3: Chunk-Splitting. Vorher landete alles in einem 824 KB
  // index-*.js — Vite warnte "chunks larger than 500 kB". Tauri laedt
  // den Frontend-Bundle aus dem Filesystem (kein Netz-Latenz-Impact),
  // aber kleinere Chunks helfen dem Browser-Parser + erlauben in
  // Zukunft Lazy-Loading von Tab-Bundles. Splits orientieren sich an
  // Vendor-Familien — selten geaenderte deps bleiben cached.
  build: {
    // Default 500 kB ist fuer Web-Apps mit Netz-Latenz konservativ.
    // Tauri laedt aus dem Filesystem — 700 kB main chunk ist hier
    // OK. Lazy-Loading per Tab (LandingPanel/ACARS-Log etc.) wuerde
    // den main weiter shrinken, ist aber separates Refactor-Ticket
    // (geplant fuer v0.9.x — siehe DevDocs).
    chunkSizeWarningLimit: 700,
    rollupOptions: {
      output: {
        manualChunks: {
          // React + DOM-Rendering — 130-180 kB, sehr stabil
          "vendor-react": ["react", "react-dom"],
          // i18n-Stack (~80 kB) — eigenes Chunk, weil Sprachfiles
          // (locales/*.json) eh schon dynamisch via `react-i18next`
          // geladen werden koennten in v0.9.x.
          "vendor-i18n": [
            "i18next",
            "i18next-browser-languagedetector",
            "react-i18next",
          ],
          // Markdown-Rendering (~200 kB durch unified/remark/rehype)
          // — wird nur im About-Tab + Release-Notes-Anzeige
          // gebraucht, perfekter Code-Splitting-Kandidat.
          "vendor-markdown": ["react-markdown", "remark-gfm"],
        },
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    // Eingebauter Paketstand liegt ausserhalb von client/ (`../bundle`).
    fs: { allow: [".", "../bundle"] },
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
