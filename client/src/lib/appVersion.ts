// Die Version des laufenden Clients (19.11.2026).
//
// Die Oberfläche kommt seit v3 aus dem Laufzeit-Paket; die Version gehört
// aber dem Client. Bis Generation 104 stand sie als `__APP_VERSION__` fest im
// Paket: Ein Client 3.1.3 mit Generation 104 meldete sich als 3.1.2 und
// druckte 3.1.2 unter den Landebericht, und jeder Versions-Commit baute eine
// neue Oberfläche (eine Generation ohne neuen Inhalt). Jetzt fragt die
// Oberfläche den Client über `app_info` — in Tauri wie über die LAN-Brücke,
// das kann jeder 3.x-Client, ohne neuen Client.
import { useEffect, useState } from "react";
import { invoke } from "./ipc";

/** Die Version des Clients; ohne Antwort (Vorschau, LAN vor der Anmeldung) "0.0.0". */
export async function appVersion(): Promise<string> {
  try {
    const info = await invoke<{ version?: unknown } | null>("app_info");
    const v = info?.version;
    return typeof v === "string" && v.trim() !== "" ? v : "0.0.0";
  } catch {
    return "0.0.0";
  }
}

/** Für Komponenten: leer, bis die Antwort des Clients da ist. */
export function useAppVersion(): string {
  const [version, setVersion] = useState("");
  useEffect(() => {
    let aktiv = true;
    void appVersion().then((v) => {
      if (aktiv) setVersion(v);
    });
    return () => {
      aktiv = false;
    };
  }, []);
  return version;
}
