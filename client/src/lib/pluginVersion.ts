// Mindestversion des X-Plane-Plugins für das HUD-Band (ADR-0005) und der
// Vergleich dazu. Gegenstück zu `kann_band` im Rust-Client (plugin2.rs):
// darunter sendet die App kein Band, also muss die Oberfläche sagen, warum.
import type { XPlanePremiumStatus } from "../types";

export const BAND_MIN_PLUGIN = "1.1.0";

function teile(v: string): [number, number, number] | null {
  const m = /^\s*v?(\d+)\.(\d+)(?:\.(\d+))?/.exec(v);
  if (!m) return null;
  return [Number(m[1]), Number(m[2]), Number(m[3] ?? 0)];
}

/** `true`/`false`, oder `null`, wenn eine Version nicht lesbar ist. */
export function versionKleiner(a: string, b: string): boolean | null {
  const x = teile(a);
  const y = teile(b);
  if (!x || !y) return null;
  for (let i = 0; i < 3; i++) {
    if (x[i] !== y[i]) return x[i] < y[i];
  }
  return false;
}

/** Läuft ein Plugin mit Protokoll 2, das für das Band zu alt ist?
 *  Nur bei einer sicher lesbaren, kleineren Version — sonst kein Hinweis. */
export function bandBrauchtUpdate(
  s: Pick<XPlanePremiumStatus, "active" | "protokoll" | "plugin_version" | "veraltet"> | null,
): boolean {
  if (!s || !s.active || s.protokoll !== 2 || s.veraltet) return false;
  if (!s.plugin_version) return false;
  return versionKleiner(s.plugin_version, BAND_MIN_PLUGIN) === true;
}

// Plugin-Version, die zu dieser App gehört (xplane-plugin/CMakeLists.txt,
// Wächter in pluginVersion.test.ts). Das Plugin-Paket lädt die App passend
// zu ihrer eigenen Version herunter.
export const PLUGIN_AKTUELL = "1.1.1";

/** Kann das Band schon, ist aber älter als das Plugin dieser App? Dann
 *  Update anbieten (1.1.1: Band auf zweitem Monitor). Schweigt, wo
 *  `bandBrauchtUpdate` oder „veraltet" schon einen Hinweis zeigen. */
export function pluginUpdateEmpfohlen(
  s: Pick<XPlanePremiumStatus, "active" | "protokoll" | "plugin_version" | "veraltet"> | null,
): boolean {
  if (!s || !s.active || s.protokoll !== 2 || s.veraltet) return false;
  if (!s.plugin_version || bandBrauchtUpdate(s)) return false;
  return versionKleiner(s.plugin_version, PLUGIN_AKTUELL) === true;
}
