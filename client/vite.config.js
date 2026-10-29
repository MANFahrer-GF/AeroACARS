var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
    return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (g && (g = 0, op[0] && (_ = 0)), _) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
};
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
var host = process.env.TAURI_DEV_HOST;
// v0.9.0 (#GlitchTip): Client-Version fuer Sentry-release-tag
var pkg = JSON.parse(readFileSync(fileURLToPath(new URL("./package.json", import.meta.url)), "utf8"));
// https://vite.dev/config/
// v3 (Etappe 5, P-8.20): Das HUD (Panel-Server /hud) laedt Skript und Stil
// des Widgets ueber denselben Lader wie die Oberflaeche — darum liegen sie im
// Frontend-Bau unter hud/. Quelle bleibt das msfs-panel-Paket (eine Quelle
// mit Flow-Widget und nativem Panel).
var HUD_QUELLE = "../msfs-panel/Build/PackageSources/html_ui/InGamePanels/AeroACARSPanel/";
var hudDateien = {
    name: "hud-dateien",
    apply: "build",
    generateBundle: function () {
        for (var _i = 0, _a = ["panel.js", "panel.css"]; _i < _a.length; _i++) {
            var datei = _a[_i];
            this.emitFile({
                type: "asset",
                fileName: "hud/".concat(datei),
                source: readFileSync(fileURLToPath(new URL(HUD_QUELLE + datei, import.meta.url)), "utf8"),
            });
        }
    },
};
export default defineConfig(function () { return __awaiter(void 0, void 0, void 0, function () {
    return __generator(this, function (_a) {
        return [2 /*return*/, ({
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
                            host: host,
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
            })];
    });
}); });
