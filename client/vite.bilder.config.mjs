// Vorschau des Bilder-Tabs im Browser: echte Komponenten, IPC durch eine Attrappe ersetzt
// (src/dev/bilderFakeIpc.ts). `?fall=` waehlt die Lage (siehe dort). Start: npx vite --config vite.bilder.config.mjs
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const hier = dirname(fileURLToPath(import.meta.url));
const attrappe = resolve(hier, "src/dev/bilderFakeIpc.ts");

export default defineConfig({
  plugins: [react()],
  root: resolve(hier, "src/dev"),
  server: { port: 5199, strictPort: true, fs: { allow: [resolve(hier, "..")] } },
  resolve: {
    alias: [{ find: /^(\.\.\/)+lib\/ipc$/, replacement: attrappe }, { find: /^\.\/ipc$/, replacement: attrappe }],
  },
});
