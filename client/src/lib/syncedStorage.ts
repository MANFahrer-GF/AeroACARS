// Gespiegelter Einstellungs-Speicher (v1.5.6, #lan-bruecke-1zu1).
//
// Problem (Feldbefund Thomas, 11.08.2026): SimBrief-Konto, gelesene
// Nachrichten und die Squawk-Notiz lagen im `localStorage` des jeweiligen
// Browsers. Über die LAN-Brücke ist das Tablet ein ANDERER Browser — es
// sah leere Felder und alle Nachrichten als ungelesen, obwohl in der App
// alles gesetzt/gelesen war.
//
// Lösung: Der Host hält die Wahrheit (`ui_state.json`, siehe
// src-tauri/src/ui_state.rs). Dieses Modul ist die dünne Schicht davor:
//
//   1. `hydrateSyncedStorage()` läuft EINMAL beim App-Start. Es sät zuerst
//      die lokal vorhandenen Werte hoch (Migration bestehender Installs —
//      ohne das verlöre der Pilot beim Update seine SimBrief-ID), holt
//      dann den zusammengeführten Stand und schreibt ihn in `localStorage`.
//   2. Danach lesen die Komponenten weiter synchron aus `localStorage` —
//      deshalb war praktisch keine Umbauarbeit nötig.
//   3. Geschrieben wird über `syncedSet`/`syncedRemove`: lokal sofort
//      (die UI soll nicht auf das Netz warten), Host im Hintergrund.
//
// Bewusst NICHT gespiegelt: reine Ansichtsvorlieben (Kartenhintergrund,
// Track-Up, eingeklappte Navigation, zuletzt offener Reiter). Ein Tablet
// darf anders eingestellt sein als ein 27-Zoll-Monitor.

import { invoke } from "@tauri-apps/api/core";

/** Die Schlüssel, die App und Tablet gemeinsam sehen sollen. */
export const SYNCED_KEYS = [
  "simbrief_username",
  "simbrief_user_id",
  "aeroacars.readNewsIds",
  // `session:` = geteilt, aber NICHT haltbar. Der Transponder-Merker darf
  // den Programmstart nicht überleben (sonst meldet er beim nächsten Flug
  // fälschlich "schon übernommen" — dokumentierter Fix), soll aber auf dem
  // Tablet sichtbar sein. Host hält ihn nur im Speicher, hier liegt er im
  // `sessionStorage` statt im `localStorage`.
  "session:aeroacars.transponder.squawk_memo",
] as const;

export type SyncedKey = (typeof SYNCED_KEYS)[number];

function isSynced(key: string): key is SyncedKey {
  return (SYNCED_KEYS as readonly string[]).includes(key);
}

function isSessionKey(key: string): boolean {
  return key.startsWith("session:");
}

/** Der zum Schlüssel passende Browser-Speicher. */
function backingStore(key: string): Storage {
  return isSessionKey(key) ? sessionStorage : localStorage;
}

/** Speicher kann werfen (Quota, Privatmodus) — nie den Aufrufer reißen. */
function safeLocalSet(key: string, value: string | null): void {
  try {
    const store = backingStore(key);
    if (value === null) store.removeItem(key);
    else store.setItem(key, value);
  } catch {
    /* best effort */
  }
}

/** Lesen aus dem passenden Speicher. */
export function syncedGet(key: string): string | null {
  try {
    return backingStore(key).getItem(key);
  } catch {
    return null;
  }
}

/**
 * Wert setzen: lokal sofort, Host im Hintergrund. Für nicht-gespiegelte
 * Schlüssel identisch zu `localStorage.setItem` (damit Aufrufer diese
 * Funktion bedenkenlos überall verwenden können).
 */
export function syncedSet(key: string, value: string): void {
  safeLocalSet(key, value);
  if (!isSynced(key)) return;
  void invoke("ui_state_set", { key, value }).catch(() => {
    /* Host offline/alt: lokaler Stand bleibt gültig */
  });
}

/** Wert löschen — lokal sofort, Host im Hintergrund. */
export function syncedRemove(key: string): void {
  safeLocalSet(key, null);
  if (!isSynced(key)) return;
  void invoke("ui_state_set", { key, value: null }).catch(() => {});
}

/**
 * Einmal beim App-Start aufrufen. Liefert `true`, wenn der Host-Stand
 * übernommen wurde (die UI kann dann neu lesen).
 */
export async function hydrateSyncedStorage(): Promise<boolean> {
  // Was liegt lokal? Das ist bei einem frisch aktualisierten Desktop-
  // Client der bisherige Stand — er soll nicht verloren gehen.
  const seed: Record<string, string> = {};
  for (const k of SYNCED_KEYS) {
    const v = syncedGet(k);
    if (v !== null && v !== "") seed[k] = v;
  }

  try {
    // `seed` überschreibt am Host NICHTS — es füllt nur Lücken. Damit
    // kann ein spät gestartetes Tablet mit altem Stand die frischeren
    // Werte des PCs nicht plattmachen.
    const merged = (await invoke("ui_state_seed", { values: seed })) as Record<
      string,
      string
    >;
    for (const k of SYNCED_KEYS) {
      const v = merged[k];
      safeLocalSet(k, v === undefined ? null : v);
    }
    return true;
  } catch {
    // Kein Backend erreichbar (z. B. Vitest, alter Host): lokaler Stand
    // bleibt einfach stehen — genau das Verhalten von vorher.
    return false;
  }
}
