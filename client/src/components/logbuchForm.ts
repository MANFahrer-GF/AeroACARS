// Gemeinsame Formate und Typen von Logbuch-Liste und geöffnetem Flug.
import type { TFunction } from "i18next";

export interface Item {
  id: string; date?: string; dep_icao?: string; arr_icao?: string; callsign?: string;
  aircraft_icao?: string; aircraft_reg?: string; status?: string;
  duration_min?: number; distance_nm?: number; landing_rate_fpm?: number;
}
export interface RoutePt {
  lat: number; lon: number;
  /** Zeit seit Flugbeginn in ms. */
  t?: number;
}
export interface PirepFeld { name: string; slug: string; value: string }
export interface PirepFare { code: string; name: string; count: number; type: number }
export interface PirepBuchung { memo: string; credit_c: number; debit_c: number }
export interface Detail extends Item {
  route?: RoutePt[];
  log?: { t: number; level?: string; message: string }[];
  fields?: PirepFeld[];
  fares?: PirepFare[];
  transactions?: PirepBuchung[];
  currency?: string;
  units?: { weight?: string };
}

const pad = (n: number) => String(n).padStart(2, "0");
export const dur = (m?: number) => (m == null ? "—" : m >= 60 ? `${Math.floor(m / 60)}h ${pad(m % 60)}m` : `0h ${pad(m)}m`);
export const elapsed = (ms: number) => { const t = Math.round(ms / 60000); return `${pad(Math.floor(t / 60))}:${pad(t % 60)}`; };
/** v0.19.x FIX: was a hand-rolled German-only month-abbreviation array
 *  (JAN..DEZ), shown regardless of locale. `toLocaleDateString` gives the
 *  same "12 AUG 2026"-style layout while actually respecting `locale`. */
export const fmtDate = (iso: string | undefined, locale: string) => {
  if (!iso) return "—";
  const d = new Date(iso);
  const day = pad(d.getDate());
  const month = d.toLocaleDateString(locale, { month: "short" }).toUpperCase().replace(/\.$/, "");
  return `${day} ${month} ${d.getFullYear()}`;
};
export const esc = (s: unknown) => String(s ?? "").replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]!);
const statusSlug = (s?: string) => (s === "accepted" || s === "pending" || s === "rejected" ? s : "pending");
/** v0.19.x FIX: the badge text used to be the raw English status slug
 *  itself ("accepted"/"pending"/"rejected"), shown verbatim regardless
 *  of locale — the CSS class keeps the English slug (styling hook), only
 *  the visible label is now localized. */
export const badge = (s: string | undefined, t: TFunction) =>
  `<span class="aa-lb-badge aa-lb-b-${statusSlug(s)}">${esc(t(`logbook_view.status_${statusSlug(s)}`))}</span>`;
