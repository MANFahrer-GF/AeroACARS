// Eigene Vite-Konfiguration für die Demo: ein Entry, alles in EINE Datei.
//
// Eine einzelne HTML-Datei, weil die Demo verschickt und angesehen wird,
// nicht ausgeliefert. Ein Ordner mit assets/ daneben kommt beim Weitergeben
// nicht mit und die Seite bliebe leer.
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

export default defineConfig({
  plugins: [react()],
  root: resolve(process.cwd(), "src/dev"),
  build: {
    outDir: resolve(process.cwd(), "demo-dist"),
    emptyOutDir: true,
    rollupOptions: { input: resolve(process.cwd(), "src/dev/demo.html") },
    // Alles inline: keine externen Dateien, keine Chunk-Aufteilung.
    assetsInlineLimit: 100_000_000,
    cssCodeSplit: false,
  },
});
