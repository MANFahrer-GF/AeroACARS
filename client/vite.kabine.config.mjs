// Demo der Kabinenansagen: echte Komponenten, IPC durch eine Attrappe ersetzt.
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Relativ zu dieser Datei, nicht zum Startordner (launch.json startet im Repo-Wurzelordner).
const hier = dirname(fileURLToPath(import.meta.url));

const attrappe = resolve(hier, "src/dev/kabineFakeIpc.ts");

export default defineConfig({
  plugins: [react()],
  root: resolve(hier, "src/dev"),
  resolve: {
    alias: [{ find: /^(\.\.\/lib\/ipc|\.\/ipc|\.\.\/\.\.\/lib\/ipc)$/, replacement: attrappe }],
  },
});
