// Ein geöffneter Flug im Logbuch (Design 10.11.2026): Kopf mit den Eckdaten,
// darunter die Reiter Karte, Bordbuch und Protokoll.

import { lazy, Suspense, useEffect, useRef, useState, type KeyboardEvent } from "react";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import { bilanz, type Eintrag, type Regel } from "../lib/bordbuch";
import { ohneDatumsgrenzenSprung } from "../lib/datumsgrenze";
import type { FlugkarteSteuerung } from "../flugkarte/flugkarte";
import type { OhneAkte } from "../flugkarte/FlugkarteFlaeche";
import { kartenAnfrage, useKartengrundlage } from "./BasemapContext";
import { BordbuchFlug, bordbuchEreignisse } from "./bordbuch/BordbuchFlug";
import { PruefstatusKasten, PruefstatusMarke, pruefArt, type PirepPruefstatus } from "./PirepPruefstatus";
import { UploadKasten } from "./UploadAusstehend";
import { badge, dur, elapsed, esc, fmtDate, type Detail, type RoutePt } from "./logbuchForm";

// Flugkarte: erst beim Öffnen laden — deck.gl soll den Start nicht bremsen
const FlugkarteFlaeche = lazy(() => import("../flugkarte/FlugkarteFlaeche"));
const FlugkarteFenster = lazy(() => import("../flugkarte/FlugkarteFenster"));

type Reiter = "karte" | "bordbuch" | "protokoll";
const REITER: Reiter[] = ["karte", "bordbuch", "protokoll"];

interface Props {
  detail: Detail;
  pruefstatus: PirepPruefstatus | undefined;
  uploads: Parameters<typeof UploadKasten>[0]["eintraege"];
  onZurueck: () => void;
  onLandungAnsehen?: (pirepId: string) => void;
}

export function LogbuchFlug({ detail, pruefstatus, uploads, onZurueck, onLandungAnsehen }: Props) {
  const { t, i18n } = useTranslation();
  const [reiter, setReiter] = useState<Reiter>("karte");
  // undefined: lädt noch — die Karte wartet darauf, damit die Bordbuch-Punkte gleich drin sind
  const [eintrag, setEintrag] = useState<Eintrag | null | undefined>(undefined);
  const [ohneAkte, setOhneAkte] = useState<OhneAkte | null>(null);
  const [vollbild, setVollbild] = useState(false);
  const [hatLandung, setHatLandung] = useState<boolean | null>(null);
  const karte = useRef<FlugkarteSteuerung | null>(null);
  const wartend = useRef<Regel | null>(null);
  const reiterKnoepfe = useRef<Partial<Record<Reiter, HTMLButtonElement | null>>>({});

  useEffect(() => {
    let aktiv = true;
    setEintrag(undefined);
    invoke<Eintrag | null>("bordbuch_eintrag", { pirepId: detail.id })
      .then((e) => { if (aktiv) setEintrag(e ?? null); })
      .catch((e) => { console.warn("bordbuch_eintrag", e); if (aktiv) setEintrag(null); });
    invoke<unknown>("landing_get", { pirepId: detail.id })
      .then((r) => { if (aktiv) setHatLandung(r != null); })
      .catch(() => { if (aktiv) setHatLandung(false); });
    return () => { aktiv = false; };
  }, [detail.id]);

  const markieren = async (regel: Regel, nachAtc: boolean) => {
    const neu = await invoke<Eintrag | null>("bordbuch_markieren", { pirepId: detail.id, regel, nachAtc });
    if (neu) setEintrag(neu);
  };

  const aufKarte = (regel: Regel) => {
    setReiter("karte");
    if (karte.current) karte.current.zeigeEreignis(regel);
    else wartend.current = regel;
  };

  const wechselMitTaste = (ev: KeyboardEvent<HTMLButtonElement>, r: Reiter) => {
    const schritt = ev.key === "ArrowRight" ? 1 : ev.key === "ArrowLeft" ? -1 : 0;
    if (!schritt) return;
    ev.preventDefault();
    const neu = REITER[(REITER.indexOf(r) + schritt + REITER.length) % REITER.length];
    setReiter(neu);
    reiterKnoepfe.current[neu]?.focus();
  };

  const b = eintrag && !eintrag.aus_grund ? bilanz(eintrag.punkte, eintrag.eingeschaltet) : null;
  const bilanzDa = b != null && b.von > 0;
  const pruefOk = pruefArt(pruefstatus) === "ok";
  const zahl = (n: number | undefined, einheit: string) => (n == null ? "—" : `${n} ${einheit}`);
  const reiterText = (r: Reiter) =>
    r === "karte" ? t("logbook_view.reiter_karte")
    : r === "protokoll" ? t("logbook_view.reiter_protokoll")
    : bilanzDa ? `${t("bordbuch.titel")} ${t("bordbuch.bilanz_kurz", { ok: b.ok, von: b.von })}` : t("bordbuch.titel");

  const fakten: Array<[string, string]> = [
    [t("logbook_view.table_duration"), dur(detail.duration_min)],
    [t("logbook_view.table_distance"), zahl(detail.distance_nm, "nm")],
    [t("logbook_view.table_landing"), zahl(detail.landing_rate_fpm, "fpm")],
    [t("bordbuch.titel"), bilanzDa ? `${b.ok}/${b.von}` : "—"],
  ];

  return (
    <section className="aa-lb-flug">
      <header className="aa-lb-flug-kopf">
        <button type="button" className="aa-lb-btn" onClick={onZurueck}>{t("logbook_view.detail_back")}</button>
        <div className="aa-lb-flug-wer">
          <div><span className="aa-lb-cs">{detail.callsign}</span><span className="aa-lb-route">{detail.dep_icao}<span className="aa-lb-arr">→</span>{detail.arr_icao}</span></div>
          <div className="aa-lb-flug-unter">
            {fmtDate(detail.date, i18n.language)} · {detail.aircraft_icao} · {detail.aircraft_reg}
            <span dangerouslySetInnerHTML={{ __html: badge(detail.status, t) }} />
            <PruefstatusMarke status={pruefstatus} />
          </div>
        </div>
        <div className="aa-lb-det-stats">
          {fakten.map(([k, v]) => <div key={k}><div className="aa-lb-k">{k}</div><div className="aa-lb-v">{v}</div></div>)}
        </div>
        <button
          type="button"
          className="aa-lb-btn aa-lb-btn--rand"
          disabled={!hatLandung || !onLandungAnsehen}
          title={hatLandung === false ? t("logbook_view.keine_landung") : undefined}
          onClick={() => onLandungAnsehen?.(detail.id)}
        >
          {t("logbook_view.landung_ansehen")}
        </button>
      </header>
      {/* „In Ordnung“ braucht keinen Platz über der Karte; alles andere muss sofort auffallen. */}
      {!pruefOk && <PruefstatusKasten status={pruefstatus} />}
      <UploadKasten eintraege={uploads} />
      <div className="aa-lb-flug-reiterzeile">
        <div role="tablist" aria-label={t("logbook_view.reiter_aria")} className="aa-lb-flug-reiter">
          {REITER.map((r) => (
            <button
              key={r}
              ref={(el) => { reiterKnoepfe.current[r] = el; }}
              type="button"
              role="tab"
              id={`aa-lb-reiter-${r}`}
              aria-controls={`aa-lb-feld-${r}`}
              aria-selected={reiter === r}
              tabIndex={reiter === r ? 0 : -1}
              className={reiter === r ? "ist-aktiv" : undefined}
              onClick={() => setReiter(r)}
              onKeyDown={(ev) => wechselMitTaste(ev, r)}
            >
              {reiterText(r)}
            </button>
          ))}
        </div>
        {reiter === "karte" && !ohneAkte && (
          <button type="button" className="aa-lb-btn" onClick={() => { karte.current = null; setVollbild(true); }}>{t("logbook_view.vollbild")}</button>
        )}
      </div>
      <div className="aa-lb-flug-feld">
        {/* Karte bleibt beim Reiterwechsel stehen (unsichtbar, gleiche Größe): Wiedergabe und Ausschnitt gehen nicht verloren. */}
        <div role="tabpanel" id="aa-lb-feld-karte" aria-labelledby="aa-lb-reiter-karte" className={`aa-lb-flug-karte${reiter === "karte" ? "" : " ist-weg"}`}>
          {ohneAkte ? (
            <div className="aa-lb-linie">
              <p className="aa-lb-linie-hinweis" role="status">
                {ohneAkte.art === "fehlt" ? t("logbook_view.keine_aufzeichnung") : ohneAkte.text}
              </p>
              <LinienKarte route={detail.route ?? []} />
            </div>
          ) : (eintrag !== undefined && !vollbild && (
            <Suspense fallback={<div className="flugkarte-hinweis">{t("flugkarte.fenster.laedt")}</div>}>
              <FlugkarteFlaeche
                pirepId={detail.id}
                zusatz={bordbuchEreignisse(eintrag, t)}
                onOhneAkte={setOhneAkte}
                onBereit={(api) => {
                  karte.current = api;
                  if (wartend.current) { api.zeigeEreignis(wartend.current); wartend.current = null; }
                }}
              />
            </Suspense>
          ))}
        </div>
        <div role="tabpanel" id="aa-lb-feld-bordbuch" aria-labelledby="aa-lb-reiter-bordbuch" className={`aa-lb-flug-blatt${reiter === "bordbuch" ? "" : " ist-weg"}`}>
          {eintrag === undefined
            ? <div className="bb-leer">{t("logbook_view.loading")}</div>
            : <BordbuchFlug eintrag={eintrag} onMarkieren={markieren} onAufKarte={ohneAkte ? undefined : aufKarte} />}
        </div>
        <div role="tabpanel" id="aa-lb-feld-protokoll" aria-labelledby="aa-lb-reiter-protokoll" className={`aa-lb-flug-blatt${reiter === "protokoll" ? "" : " ist-weg"}`}>
          {pruefOk && <PruefstatusKasten status={pruefstatus} />}
          <div className="aa-lb-panel">
            <h3>{t("logbook_view.detail_log_title")}</h3>
            <div className="aa-lb-log" dangerouslySetInnerHTML={{
              __html: (detail.log ?? []).map((l) => {
                const phase = l.message.startsWith("Phase:");
                // v0.19-Stage-E: "phase" (ohne Praefix) kollidierte mit der
                // GLOBALEN .phase-Klasse (App.tsx Lade-/Session-Check-Screen)
                // — Grenzflaeche/Hintergrund/Padding/flex-column von dort
                // ueberlagerten die Grid-Zeile hier, sichtbar als kaputte
                // Kaesten im Fluglogbuch. Eigener, praefixierter Modifier.
                return `<div class="aa-lb-logrow ${phase ? "aa-lb-logrow--phase" : ""}"><span class="aa-lb-t">${elapsed(l.t)}</span><span class="aa-lb-m">${phase ? '<span class="aa-lb-dot"></span>' : ""}${esc(l.message)}</span></div>`;
              }).join(""),
            }} />
          </div>
        </div>
      </div>
      {vollbild && (
        <Suspense fallback={null}>
          <FlugkarteFenster pirepId={detail.id} onClose={() => setVollbild(false)} />
        </Suspense>
      )}
    </section>
  );
}

/** Rückfall ohne Flugaufzeichnung: nur die geflogene Linie aus dem PIREP. */
function LinienKarte({ route }: { route: RoutePt[] }) {
  const grundlage = useKartengrundlage();
  // Die Karte wird einmalig gebaut; der Schlüssel kommt erst danach vom
  // Server. Über diesen Verweis liest `kartenAnfrage` bei jeder Kachel
  // den aktuellen Stand — siehe dort.
  const grundlageRef = useRef(grundlage);
  grundlageRef.current = grundlage;
  const feld = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!feld.current) return;
    const punkte = route.filter((p) => typeof p.lat === "number" && typeof p.lon === "number");
    const dark = document.documentElement.dataset.theme === "dark";
    const map = new maplibregl.Map({
      container: feld.current,
      style: dark ? grundlage.dunkel : grundlage.hell,
      center: punkte.length ? [punkte[Math.floor(punkte.length / 2)].lon, punkte[Math.floor(punkte.length / 2)].lat] : [6, 48],
      zoom: 5,
      attributionControl: { compact: true },
      // Der Schlüssel gehört an JEDE Anfrage, nicht nur an die
      // Stil-Adresse — die style.json verweist selbst weiter auf
      // TileJSON, Kacheln, Schriften und Sprites. Siehe `kartenAnfrage`.
      transformRequest: kartenAnfrage(() => grundlageRef.current.schluessel),
    });
    const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#0a84ff";
    map.on("load", () => {
      // Erst filtern, dann zaehlen: fallen kaputte Punkte raus, darf die
      // Karte nicht mit einem leeren Track weitermachen (Cloud-QS 25.09.2026).
      const coords = ohneDatumsgrenzenSprung(punkte.map((p) => [p.lon, p.lat] as [number, number]));
      if (coords.length >= 2) {
        map.addSource("trk", { type: "geojson", data: { type: "Feature", properties: {}, geometry: { type: "LineString", coordinates: coords } } });
        map.addLayer({ id: "trk", type: "line", source: "trk", layout: { "line-cap": "round", "line-join": "round" }, paint: { "line-color": accent, "line-width": 3 } });
        const pin = (c: [number, number], col: string) => { const el = document.createElement("div"); el.style.cssText = `width:12px;height:12px;border-radius:50%;background:${col};border:2px solid #fff;box-shadow:0 0 3px rgba(0,0,0,.5)`; new maplibregl.Marker({ element: el }).setLngLat(c).addTo(map); };
        pin(coords[0], "#30d158");
        pin(coords[coords.length - 1], "#ff453a");
        const b = coords.reduce((acc, c) => acc.extend(c), new maplibregl.LngLatBounds(coords[0], coords[0]));
        map.fitBounds(b, { padding: 50, duration: 0 });
      }
    });
    return () => { map.remove(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [route]);

  return <div className="aa-lb-map" ref={feld} />;
}
