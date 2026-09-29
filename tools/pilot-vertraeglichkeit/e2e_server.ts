import http from "node:http"; import { mkdtempSync } from "node:fs"; import { join } from "node:path"; import { tmpdir } from "node:os"; import { createHash } from "node:crypto";
import { Db } from "./db.js"; import { buildServer } from "./server.js"; import type { Config } from "./config.js";
const KEY = "e2e-phpvms-schluessel-0001"; const PW = "e2e-pilot-passwort-0123456789ab";
const bekannt = process.env.E2E_BEKANNT === "1";
// „GSG-Webseite": kennt den Schluessel, gehoert Pilot 7 alle PIREPs.
http.createServer((req, res) => {
  const k = req.headers["x-api-key"]; res.setHeader("content-type", "application/json");
  if (req.url === "/api/user") { if (k === KEY) { res.end(JSON.stringify({ data: { id: 7, name: "E2E Pilot", state: 1 } })); } else { res.statusCode = 401; res.end("{}"); } }
  else if (req.url?.startsWith("/api/pireps/") && k === "recorder-admin-key") res.end(JSON.stringify({ data: { id: "x", user_id: 7 } }));
  else { res.statusCode = 404; res.end("{}"); }
}).listen(47832, "127.0.0.1");
const T = mkdtempSync(join(tmpdir(), "e2e-"));
const cfg = { port: 47831, bindHost: "127.0.0.1", dbPath: join(T, "t.db"), brokerUrl: "mqtt://127.0.0.1:1883", brokerUser: "t", brokerPassword: "t", topicFilter: "aeroacars/#", sessionSecret: "x".repeat(32), cookieSecure: false, staleFlightSeconds: 180, positionRetentionDays: 30, phpvmsBaseUrl: "http://127.0.0.1:47832", phpvmsApiKey: "recorder-admin-key", defaultVaPrefix: "test", provisionPublicBrokerUrl: "wss://x/mqtt", pushVapidPublic: "", pushVapidPrivate: "", pushContactEmail: "t@e.invalid", webauthnRpId: "localhost", webauthnOrigin: "http://localhost", webauthnRpName: "T", smtpHost: "", smtpPort: 465, smtpUser: "", smtpPass: "", smtpFrom: "t@e.invalid", autoRebootEnabled: false, autoRebootQuietMs: 1, autoRebootWindow: "01:00-06:00", autoRebootCheckMs: 1 } as Config;
const db = new Db(cfg.dbPath);
db.upsertProvisionedPilot({ va_prefix: "test", pilot_id: "7", username: "pilot_7", password: PW, phpvms_user_id: 7, display_name: "E2E" });
if (bekannt) db.saveProvisionKeyHash(createHash("sha256").update(KEY).digest("hex"), Date.now() + 86_400_000);
const app = await buildServer({ cfg, db });
await app.listen({ port: 47831, host: "127.0.0.1" });
console.log("READY", bekannt ? "bekannt" : "unbekannt");
