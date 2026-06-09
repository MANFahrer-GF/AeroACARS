import { useEffect, useRef, useState, useCallback } from "react";
import type { Update } from "@tauri-apps/plugin-updater";
import { isTauri } from "../lib/ipc";

// v0.16.0 (#LAN-Remote): die Updater-/Process-Plugins existieren nur im
// Tauri-Build. In einem reinen LAN-Browser (Tablet) gibt es keinen
// Updater — der Tablet-Nutzer kann die Desktop-App eh nicht aktualisieren.
// Deshalb werden `plugin-updater` und `plugin-process` NICHT mehr statisch
// am Modulkopf importiert (das würde im Browser-Bundle beim Eval die
// fehlenden Tauri-Globals anfassen). Stattdessen lazy `import()` nur auf dem
// Tauri-Pfad. `import type { Update }` oben ist rein typseitig und wird vom
// Bundler wegradiert — kein Runtime-Effekt.
async function tauriCheckForUpdate(): Promise<Update | null> {
  if (!isTauri) return null;
  const { check } = await import("@tauri-apps/plugin-updater");
  return (await check()) as Update | null;
}

async function tauriRelaunch(): Promise<void> {
  if (!isTauri) return;
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}

/**
 * v0.5.48 — Zentraler Update-Checker mit Eskalations-Stufen.
 *
 * **Warum ein Hook?** Zwei UI-Komponenten — UpdateButton (Header,
 * immer sichtbar) und UpdateBanner (groß, nur ab 3 Tagen) — zeigen
 * denselben Update-Status. Beide müssen aus EINER Quelle kommen sonst
 * doppelte `check()`-Calls und inkonsistente Stage-Berechnung.
 * Hook bietet diese Quelle. Wird einmal in App.tsx aufgerufen, das
 * Ergebnis via Props an Button + Banner weitergegeben.
 *
 * **Polling-Strategie (Pilot-App-spezifisch):**
 *
 * - **Beim App-Start:** 1× sofort (wie heute)
 * - **Während App läuft:** alle 4 h re-check (langer Cruise =
 *   stunden-lange Sessions, Pilot soll Updates mitbekommen)
 * - **Bei Window-Focus:** re-check wenn der letzte Check > 30 min her
 *   ist (Pilot wechselt vom Sim zurück zur App nach Stunden)
 * - **Niemals:** kein Spam-Polling — GitHub-API hat Rate-Limits, plus
 *   wir wollen den Sim-FPS nicht stören
 *
 * **Eskalations-Stages (für UI):**
 *
 * - `none` — kein Update
 * - `fresh` — Update gerade entdeckt, < 24 h gesehen
 * - `pulse` — > 24 h gesehen, Pilot hat ignoriert → Button glüht
 * - `banner` — > 72 h gesehen → großes Banner darf erscheinen
 *   (Banner-Komponente ist zusätzlich phase-aware: nicht im Flug)
 *
 * **localStorage-Keys:**
 *
 * - `aeroacars.update.first_seen.{version}` — wann das aktuelle Update
 *   zum ersten Mal entdeckt wurde (für Stage-Berechnung)
 * - `aeroacars.update.dismissed_until` — Pilot klickt „Später" → 4 h
 *   Banner-Stille (Button bleibt sichtbar)
 * - `aeroacars.update.last_check_at` — letzter erfolgreicher Check
 *   (für Focus-Re-Check-Throttle)
 */

const FOUR_HOURS_MS = 4 * 60 * 60 * 1000;
const THIRTY_MIN_MS = 30 * 60 * 1000;
const ONE_DAY_MS = 24 * 60 * 60 * 1000;
const THREE_DAYS_MS = 3 * ONE_DAY_MS;
const SNOOZE_DURATION_MS = FOUR_HOURS_MS;

export type UpdateStage = "none" | "fresh" | "pulse" | "banner";

export interface UseUpdateCheckerResult {
  update: Update | null;
  stage: UpdateStage;
  /** True nur während ein Download/Install läuft. */
  installing: boolean;
  /** Lokalisierter Status-String während Download (oder null). */
  progress: string | null;
  /** Pilot klickt „Später" → Banner für 4 h zu, Button bleibt. */
  snoozeBanner: () => void;
  /** True wenn Pilot gerade snoozed hat — Banner bleibt versteckt
   *  bis snooze abläuft. Button ignoriert das. */
  bannerSnoozed: boolean;
  /** Manuell installieren + relaunchen. */
  installAndRelaunch: () => Promise<void>;
}

function readNum(key: string): number | null {
  try {
    const v = localStorage.getItem(key);
    if (!v) return null;
    const n = Number(v);
    return Number.isFinite(n) ? n : null;
  } catch {
    return null;
  }
}

function writeNum(key: string, n: number) {
  try {
    localStorage.setItem(key, String(n));
  } catch {
    // localStorage voll oder disabled — ignorieren
  }
}

function clearKey(key: string) {
  try {
    localStorage.removeItem(key);
  } catch {
    /* noop */
  }
}

function firstSeenKey(version: string): string {
  return `aeroacars.update.first_seen.${version}`;
}

const DISMISS_KEY = "aeroacars.update.dismissed_until";
const LAST_CHECK_KEY = "aeroacars.update.last_check_at";

/** Räumt veraltete first_seen-Einträge aus localStorage auf — wenn
 *  Pilot von v0.5.45 → v0.5.46 → v0.5.47 läuft, sammeln sich sonst
 *  beliebig viele alte Keys. */
function pruneOldVersionKeys(currentVersion: string) {
  try {
    const prefix = "aeroacars.update.first_seen.";
    const toDelete: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key && key.startsWith(prefix) && key !== firstSeenKey(currentVersion)) {
        toDelete.push(key);
      }
    }
    toDelete.forEach((k) => localStorage.removeItem(k));
  } catch {
    /* noop */
  }
}

function computeStage(
  update: Update | null,
  bannerSnoozed: boolean,
  now: number,
): UpdateStage {
  if (!update) return "none";
  const seenAt = readNum(firstSeenKey(update.version));
  // Falls noch nie gesehen (sollte nicht passieren wenn Hook korrekt
  // läuft, aber Defensive), ist es per Definition fresh.
  if (seenAt == null) return "fresh";
  const age = now - seenAt;
  if (age >= THREE_DAYS_MS && !bannerSnoozed) return "banner";
  if (age >= ONE_DAY_MS) return "pulse";
  return "fresh";
}

export function useUpdateChecker(): UseUpdateCheckerResult {
  const [update, setUpdate] = useState<Update | null>(null);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<string | null>(null);
  // tick zwingt Re-Compute der Stage einmal pro Minute. Nötig weil
  // sich die Stage rein durch Zeitablauf (24 h, 72 h) ändert ohne
  // dass `update` sich ändert.
  const [, setTick] = useState(0);
  // Lock damit zwei parallele check()-Calls (z.B. Mount + Focus
  // gleichzeitig nach Sleep) nicht doppeln.
  const checkInFlight = useRef(false);

  const performCheck = useCallback(async () => {
    // Browser-Build (LAN-Tablet): kein Updater — Hook bleibt no-op.
    if (!isTauri) return;
    if (checkInFlight.current) return;
    checkInFlight.current = true;
    try {
      const u = await tauriCheckForUpdate();
      writeNum(LAST_CHECK_KEY, Date.now());
      if (u) {
        setUpdate(u);
        // Erste Sichtung dieser Version → Timestamp setzen.
        // Bei späteren Re-Checks nicht überschreiben (sonst würde der
        // Pulse-/Banner-Timer nie hochzählen).
        const k = firstSeenKey(u.version);
        if (readNum(k) == null) writeNum(k, Date.now());
        pruneOldVersionKeys(u.version);
      } else {
        // Kein Update mehr → eventuelle alte first_seen-Einträge
        // löschen, dann State zurücksetzen.
        setUpdate(null);
        clearKey(DISMISS_KEY);
      }
    } catch {
      // Offline / GitHub down — leise. Nächster Tick versucht's wieder.
    } finally {
      checkInFlight.current = false;
    }
  }, []);

  // Initial-Check beim Mount + Polling alle 4 h.
  useEffect(() => {
    void performCheck();
    const id = setInterval(() => void performCheck(), FOUR_HOURS_MS);
    return () => clearInterval(id);
  }, [performCheck]);

  // Re-Check bei Window-Focus (Pilot kommt vom Sim zurück), aber nur
  // wenn letzter Check > 30 min her — sonst sinnloses Spam-Polling
  // bei jedem Tab-Wechsel.
  useEffect(() => {
    const onFocus = () => {
      const last = readNum(LAST_CHECK_KEY) ?? 0;
      if (Date.now() - last >= THIRTY_MIN_MS) void performCheck();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [performCheck]);

  // Stage-Tick: einmal pro Minute neu rendern damit fresh→pulse→banner
  // ohne neuen check() greifen. Billig — nur ein State-Bump.
  useEffect(() => {
    if (!update) return;
    const id = setInterval(() => setTick((t) => t + 1), 60 * 1000);
    return () => clearInterval(id);
  }, [update]);

  const dismissedUntil = readNum(DISMISS_KEY) ?? 0;
  const now = Date.now();
  const bannerSnoozed = dismissedUntil > now;
  const stage = computeStage(update, bannerSnoozed, now);

  const snoozeBanner = useCallback(() => {
    writeNum(DISMISS_KEY, Date.now() + SNOOZE_DURATION_MS);
    // Tick triggern damit Banner sofort verschwindet.
    setTick((t) => t + 1);
  }, []);

  const installAndRelaunch = useCallback(async () => {
    if (!update || installing) return;
    setInstalling(true);
    setProgress("Lädt Update herunter…");
    try {
      let downloaded = 0;
      let total = 0;
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            total = event.data.contentLength ?? 0;
            setProgress(
              total > 0
                ? `Download: 0 / ${(total / 1_048_576).toFixed(1)} MB`
                : "Download startet…",
            );
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            setProgress(
              total > 0
                ? `Download: ${(downloaded / 1_048_576).toFixed(1)} / ${(total / 1_048_576).toFixed(1)} MB`
                : `Download: ${(downloaded / 1_048_576).toFixed(1)} MB`,
            );
            break;
          case "Finished":
            setProgress("Installiere — App startet gleich neu…");
            break;
        }
      });
      await tauriRelaunch();
    } catch (err) {
      setProgress(`Fehler: ${err}`);
      setInstalling(false);
    }
  }, [update, installing]);

  return {
    update,
    stage,
    installing,
    progress,
    snoozeBanner,
    bannerSnoozed,
    installAndRelaunch,
  };
}
