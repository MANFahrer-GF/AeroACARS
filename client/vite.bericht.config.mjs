// Druckvorschau des Landungs-Berichts in EINE HTML-Datei.
//
// Getrennt von `vite.demo.config.mjs`, weil es ein anderer Einstieg mit
// anderem Zweck ist: Die Demo zeigt die Bahn-Anzeige am Bildschirm, diese
// Seite den Bericht auf Papier.
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";
import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync(resolve(process.cwd(), "package.json"), "utf-8"));

export default defineConfig({
  plugins: [react()],
  // Ohne diese Zeile fällt die Fußzeile auf ihren Ersatzwert zurück und
  // der Beispiel-Bericht behauptet „AeroACARS v0.0.0". Die Hauptkonfig
  // setzt sie ebenfalls (vite.config.ts).
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  root: resolve(process.cwd(), "src/dev"),
  build: {
    outDir: resolve(process.cwd(), "bericht-dist"),
    emptyOutDir: true,
    rollupOptions: { input: resolve(process.cwd(), "src/dev/bericht.html") },
    assetsInlineLimit: 100_000_000,
    cssCodeSplit: false,
  },
});
