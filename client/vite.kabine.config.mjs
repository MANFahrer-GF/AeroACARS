// Demo der Kabinenansagen: echte Komponenten, IPC durch eine Attrappe ersetzt.
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

const attrappe = resolve(process.cwd(), "src/dev/kabineFakeIpc.ts");

export default defineConfig({
  plugins: [react()],
  root: resolve(process.cwd(), "src/dev"),
  resolve: {
    alias: [{ find: /^(\.\.\/lib\/ipc|\.\/ipc|\.\.\/\.\.\/lib\/ipc)$/, replacement: attrappe }],
  },
});
