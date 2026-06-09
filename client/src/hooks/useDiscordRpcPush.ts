// v0.9.0 (#Discord-RPC) — Hook der bei jeder Aenderung von ActiveFlightInfo /
// SimSnapshot eine neue Presence ans Rust-Backend schiebt.
//
// Spec: docs/spec/v0.9.0-discord-rich-presence.md (Trigger-Tabelle + LE8)
//
// Design:
//   - Wenn kein Flug aktiv → discord_rpc_clear_flight()
//   - Wenn Flug aktiv → discord_rpc_push_state() mit aktueller Phase + Alt
//   - **v0.9.1 F7 (Spec LE8):** waehrend aktivem Flug zusaetzlich
//     `set_sim_lost(true)` rufen wenn simStatus.state != "connected"
//     (= Sim-Adapter ist disconnected / connecting / re-tryt). Damit haengt
//     der naechste Presence-Push das "⚠ Sim getrennt"-Suffix dran. Wieder
//     auf `false` sobald SimConnect/X-Plane wieder lebt.
//   - Backend dedupliziert intern (set_flight + set_sim_lost pruefen Aenderung)
//   - Wenn der Discord-Toggle aus ist, no-ops das Backend ohnehin, also
//     ist's billig diesen Hook auch bei "RPC off" laufen zu lassen.

import { useEffect, useRef } from "react";
import { invoke } from "../lib/ipc";
import type { ActiveFlightInfo, SimStatus } from "../types";

interface Args {
  activeFlight: ActiveFlightInfo | null;
  simStatus: SimStatus | null;
  /** Optional: phpVMS-Profil-URL fuer den "Open Profile"-Button. */
  profileUrl?: string | null;
}

export function useDiscordRpcPush({ activeFlight, simStatus, profileUrl }: Args) {
  // Letzter gepushter Snapshot — verhindert Spam bei jedem Render.
  // Wir vergleichen serialisierte JSON-Strings (klein, billig).
  const lastPushedRef = useRef<string>("");

  useEffect(() => {
    if (!activeFlight) {
      // Sauberes Clear nur einmal (wenn vorher was anderes drin war).
      if (lastPushedRef.current !== "CLEARED") {
        lastPushedRef.current = "CLEARED";
        // v0.12.2: log instead of silently swallowing — a failing
        // Discord push must be diagnosable, not invisible.
        void invoke("discord_rpc_clear_flight").catch((e) =>
          console.warn("[discord-rpc] clear_flight failed", e),
        );
      }
      return;
    }

    const callsign = `${activeFlight.airline_icao}${activeFlight.flight_number}`;
    const altitudeFt = simStatus?.snapshot?.altitude_msl_ft
      ? Math.round(simStatus.snapshot.altitude_msl_ft)
      : null;
    // started_at ISO → unix seconds
    const startUnix = Math.floor(new Date(activeFlight.started_at).getTime() / 1000);

    // Aircraft-ICAO: ActiveFlightInfo hat nur planned_registration (=Tail).
    // Phase-Tag-Allowlist erlaubt aircraft, also nehmen wir den Tail wenn nichts
    // anderes da ist — Discord-Anzeige ist "Aircraft" Fallback (siehe Spec).
    const aircraft = activeFlight.planned_registration || "";

    const payload = {
      callsign,
      dep_icao: activeFlight.dpt_airport,
      arr_icao: activeFlight.arr_airport,
      aircraft,
      altitude_ft: altitudeFt,
      phase: activeFlight.phase.toUpperCase(), // Tauri-Command parsed kanonisch
      sim: simStatus?.kind ?? "",
      start_unix: startUnix,
      profile_url: profileUrl ?? null,
    };

    const key = JSON.stringify(payload);
    if (key === lastPushedRef.current) return; // nichts geaendert

    lastPushedRef.current = key;
    // v0.12.2: surface push failures (command rejected, args invalid,
    // Discord pipe down) instead of swallowing them.
    void invoke("discord_rpc_push_state", { args: payload }).catch((e) =>
      console.warn("[discord-rpc] push_state failed", e),
    );
  }, [activeFlight, simStatus, profileUrl]);

  // v0.9.1 F7 — LE8 "⚠ Sim getrennt"-Suffix verdrahten. Reagiert auf
  // simStatus.state-Aenderungen unabhaengig vom push_state-Pfad damit der
  // Suffix-Status auch dann gepush wird wenn sich sonst nichts veraendert
  // hat (z.B. waehrend Cruise: Pilot fliegt brav weiter, MSFS kurz weg,
  // dann zurueck → Presence muss "⚠"-Suffix kurzzeitig zeigen und wieder
  // entfernen, ohne dass altitude/phase sich aendern).
  //
  // Backend (Manager::set_sim_lost) dedupliziert intern — billig.
  // Nur waehrend aktivem Flug: kein Flug = kein Suffix sinnvoll.
  useEffect(() => {
    if (!activeFlight) return;
    const lost = simStatus ? simStatus.state !== "connected" : true;
    void invoke("discord_rpc_set_sim_lost", { lost }).catch((e) =>
      console.warn("[discord-rpc] set_sim_lost failed", e),
    );
  }, [activeFlight, simStatus?.state]);
}
