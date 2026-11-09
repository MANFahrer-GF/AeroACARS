// Vorschau des Crew-Story-Fensters im Browser: echte Buchungsliste, IPC durch eine Attrappe ersetzt
// (src/dev/crewstoryFakeIpc.ts). `?fall=` waehlt die Lage. Start: npx vite --config vite.crewstory.config.mjs
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const hier = dirname(fileURLToPath(import.meta.url));
const attrappe = resolve(hier, "src/dev/crewstoryFakeIpc.ts");

export default defineConfig({
  plugins: [react()],
  root: resolve(hier, "src/dev"),
  server: { port: 5197, strictPort: true, fs: { allow: [resolve(hier, "..")] } },
  optimizeDeps: { entries: ["crewstory.html"] },
  resolve: {
    alias: [{ find: /^(\.\.\/)+lib\/ipc$/, replacement: attrappe }, { find: /^\.\/ipc$/, replacement: attrappe }],
  },
});
