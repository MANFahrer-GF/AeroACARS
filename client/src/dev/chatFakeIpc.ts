// IPC-Attrappe für die Chat-Demo (vite.chat.config.mjs ersetzt ../lib/ipc
// hierdurch): ein voller Verlauf, damit sich das Layout bei vielen
// Nachrichten nachstellen lässt.
type Cb = (e: { payload: unknown }) => void;

export const isTauri = true;
export const formatIpcError = (e: unknown) => String(e);
export async function openExternal(url: string) {
  window.open(url, "_blank");
}

const hoerer = new Map<string, Cb[]>();
export async function listen(event: string, cb: Cb) {
  hoerer.set(event, [...(hoerer.get(event) ?? []), cb]);
  return () => hoerer.set(event, (hoerer.get(event) ?? []).filter((c) => c !== cb));
}

const anzahl = Number(new URLSearchParams(location.search).get("n") ?? 80);
const start = Date.UTC(2026, 9, 30, 16, 0);
const nachrichten = Array.from({ length: anzahl }, (_, i) => ({
  id: i + 1,
  va_prefix: "GSG",
  von_pilot_id: i % 3 === 0 ? "GSG0001" : `GSG01${i % 7}`,
  ts: start + i * 45_000,
  text: i % 5 === 0
    ? `Nachricht ${i + 1}: etwas längerer Zuruf, damit auch Zeilenumbrüche im Verlauf vorkommen und die Liste realistisch wächst.`
    : `Nachricht ${i + 1}`,
  callsign: i % 3 === 0 ? "GSG1234" : `DLH${400 + (i % 7)}`,
  anzeigename: i % 3 === 0 ? "Thomas K." : `Pilot ${i % 7}`,
}));

export async function invoke<T>(cmd: string): Promise<T> {
  if (cmd === "chat_verlauf") return { nachrichten, kein_laufender_flug: false } as T;
  if (cmd === "paket_info") return { generation: 57 } as T;
  if (cmd === "chat_teilnehmer") {
    return {
      teilnehmer: Array.from({ length: 6 }, (_, i) => ({
        pilot_id: `GSG01${i}`, callsign: `DLH${400 + i}`, dep: "EDDF", arr: "LEPA", anzeigename: `Pilot ${i}`,
      })),
    } as T;
  }
  return undefined as T;
}
