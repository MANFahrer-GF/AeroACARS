/** CSV-Export des Telemetrie-Puffers (Semikolon, Zeit in UTC). */

import type { Telemetrie } from "./useTelemetrie";

export function csvText(tm: Telemetrie, ids: string[]): string {
  const spalten = ids.filter((id) => tm.zahlIndex.has(id) || tm.textIndex.has(id));
  const kopf = ["zeit_utc", ...spalten.map((id) => {
    const k = tm.kanal(id);
    return k?.einheit ? `${id} [${k.einheit}]` : id;
  })];
  const zeilen = [kopf.join(";")];
  for (const f of tm.frames) {
    const felder = [new Date(f.t).toISOString()];
    for (const id of spalten) {
      const zi = tm.zahlIndex.get(id);
      if (zi !== undefined) {
        const v = f.z[zi];
        felder.push(typeof v === "number" ? String(v) : "");
      } else {
        const ti = tm.textIndex.get(id);
        const s = ti === undefined ? null : f.s[ti];
        felder.push(s ? `"${s.replace(/"/g, '""')}"` : "");
      }
    }
    zeilen.push(felder.join(";"));
  }
  return zeilen.join("\n") + "\n";
}

