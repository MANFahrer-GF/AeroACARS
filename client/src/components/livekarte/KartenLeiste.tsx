// Die Schalterleiste der Live-Karte: Ansicht, Ebenen, Online-Netz, Kamera.
//
// Jede Bedienung hat feste Maße (siehe App.css): Ein Schalter ändert nur die Färbung, nie die
// Breite. Symbole für die Zweier-Umschalter, bei denen immer eine Hälfte leuchtet und der Zustand
// deshalb eindeutig bleibt; Symbol plus Wort für unabhängige Ein/Aus-Schalter.
import { useTranslation } from "react-i18next";
import type { Netz } from "../../lib/vatglassesKarte";

/** Tabler Icons (MIT), eingebettet: Die Leiste läuft in einem Anwendungsfenster, ein Netzzugriff
 *  für ein paar Glyphen wäre eine Abhängigkeit ohne Gegenwert. Unicode-Zeichen und selbst
 *  gezeichnete Pfade hat Thomas beide verworfen. Quelle: github.com/tabler/tabler-icons. */
const PFAD: Record<string, string> = {
  karte: "<path d=\"M3 7l6 -3l6 3l6 -3v13l-6 3l-6 -3l-6 3v-13\" /> <path d=\"M9 4v13\" /> <path d=\"M15 7v13\" />",
  satellit: "<path d=\"M3.707 6.293l2.586 -2.586a1 1 0 0 1 1.414 0l5.586 5.586a1 1 0 0 1 0 1.414l-2.586 2.586a1 1 0 0 1 -1.414 0l-5.586 -5.586a1 1 0 0 1 0 -1.414\" /> <path d=\"M6 10l-3 3l3 3l3 -3\" /> <path d=\"M10 6l3 -3l3 3l-3 3\" /> <path d=\"M12 12l1.5 1.5\" /> <path d=\"M14.5 17a2.5 2.5 0 0 0 2.5 -2.5\" /> <path d=\"M15 21a6 6 0 0 0 6 -6\" />",
  flach: "<path d=\"M3 7a2 2 0 0 1 2 -2h14a2 2 0 0 1 2 2v10a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2z\" />",
  kugel: "<path d=\"M3 12a9 9 0 1 0 18 0a9 9 0 0 0 -18 0\" /> <path d=\"M3.6 9h16.8\" /> <path d=\"M3.6 15h16.8\" /> <path d=\"M11.5 3a17 17 0 0 0 0 18\" /> <path d=\"M12.5 3a17 17 0 0 1 0 18\" />",
  norden: "<path d=\"M12 5l0 14\" /> <path d=\"M18 11l-6 -6\" /> <path d=\"M6 11l6 -6\" />",
  kurs: "<path d=\"M16 10h4a2 2 0 0 1 0 4h-4l-4 7h-3l2 -7h-4l-2 2h-3l2 -4l-2 -4h3l2 2h4l-2 -7h3l4 7\" />",
  zentrieren: "<path d=\"M4 8v-2a2 2 0 0 1 2 -2h2\" /> <path d=\"M4 16v2a2 2 0 0 0 2 2h2\" /> <path d=\"M16 4h2a2 2 0 0 1 2 2v2\" /> <path d=\"M16 20h2a2 2 0 0 0 2 -2v-2\" /> <path d=\"M9 12l6 0\" /> <path d=\"M12 9l0 6\" />",
  track: "<path d=\"M3 19a2 2 0 1 0 4 0a2 2 0 0 0 -4 0\" /> <path d=\"M19 7a2 2 0 1 0 0 -4a2 2 0 0 0 0 4\" /> <path d=\"M11 19h5.5a3.5 3.5 0 0 0 0 -7h-8a3.5 3.5 0 0 1 0 -7h4.5\" />",
  // Rollweg: "traffic-cone". Der Hut meint den Bodenbereich und bleibt auch klein eine Silhouette.
  taxi: "<path d=\"M4 20l16 0\" /> <path d=\"M9.4 10l5.2 0\" /> <path d=\"M7.8 15l8.4 0\" /> <path d=\"M6 20l5 -15h2l5 15\" />",
  bilder: "<path d=\"M4 8h3l2-3h6l2 3h3v11H4z\" /> <circle cx=\"12\" cy=\"13\" r=\"3.5\" />",
  va: "<path d=\"M16 10h4a2 2 0 0 1 0 4h-4l-4 7h-3l2 -7h-4l-2 2h-3l2 -4l-2 -4h3l2 2h4l-2 -7h3l4 7\" />",
};

function Sym({ pfad }: { pfad: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width="15"
      height="15"
      aria-hidden="true"
      focusable="false"
      style={{ display: "block" }}
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      dangerouslySetInnerHTML={{ __html: pfad }}
    />
  );
}

/** Zweier-Umschalter mit Symbolen. */
function Umschalter({ a, b, istA, setzeA }: {
  a: { pfad: string; name: string };
  b: { pfad: string; name: string };
  istA: boolean;
  setzeA: (wert: boolean) => void;
}) {
  const knopf = (teil: { pfad: string; name: string }, aktiv: boolean, wert: boolean) => (
    <button
      type="button"
      className={`aa-livemap-seg__btn ${aktiv ? "aa-livemap-seg__btn--active" : ""}`}
      aria-pressed={aktiv}
      aria-label={teil.name}
      title={teil.name}
      onClick={() => setzeA(wert)}
    >
      <Sym pfad={teil.pfad} />
    </button>
  );
  return (
    <div className="aa-livemap-seg">
      {knopf(a, istA, true)}
      {knopf(b, !istA, false)}
    </div>
  );
}

/** Ein unabhängiger Ein/Aus-Schalter: Das Wort steht in beiden Zuständen, sonst springt die Breite. */
function Schalter({ pfad, name, an, umschalten, hinweis }: {
  pfad: string;
  name: string;
  an: boolean;
  umschalten: () => void;
  hinweis?: string;
}) {
  return (
    <button
      type="button"
      className={`aa-livemap-toggle ${an ? "aa-livemap-toggle--active" : ""}`}
      aria-pressed={an}
      aria-label={name}
      title={hinweis ?? name}
      onClick={umschalten}
    >
      <Sym pfad={pfad} />
      <span className="aa-livemap-toggle__wort">{name}</span>
    </button>
  );
}

export interface KartenLeisteAngaben {
  satellit: boolean;
  setzeSatellit: (an: boolean) => void;
  kugel: boolean;
  setzeKugel: (an: boolean) => void;
  trackUp: boolean;
  setzeTrackUp: (an: boolean) => void;
  showTrack: boolean;
  umschaltenTrack: () => void;
  showTaxi: boolean;
  umschaltenTaxi: () => void;
  /** Plätze mit geladener Taxi-Karte, für den Tooltip. */
  taxiPlaetze: string[];
  bilderMarken: number;
  bilderAn: boolean;
  umschaltenBilder: () => void;
  showVa: boolean;
  umschaltenVa: () => void;
  netz: Netz;
  setzeNetz: (netz: Netz) => void;
  flAuto: boolean;
  umschaltenFlAuto: () => void;
  flWert: number;
  setzeFlWert: (fl: number) => void;
  /** Ohne Sim-Verbindung gilt immer der Regler, egal was „eigene Höhe“ sagt. */
  flRegelbar: boolean;
  flAnzeige: string;
  /** Gibt es einen laufenden Flug, kommen die Kamera-Knöpfe dazu. */
  hatFlug: boolean;
  hatPosition: boolean;
  folgen: boolean;
  umschaltenFolgen: () => void;
  zentrieren: () => void;
}

export function KartenLeiste(p: KartenLeisteAngaben) {
  const { t } = useTranslation();
  return (
    <div className="aa-livemap-controls aa-livemap-overlay">
      <div className="aa-livemap-controls__group">
        <Umschalter
          a={{ pfad: PFAD.karte, name: t("livemap.view_map") }}
          b={{ pfad: PFAD.satellit, name: t("livemap.view_sat") }}
          istA={!p.satellit}
          setzeA={(karte) => p.setzeSatellit(!karte)}
        />
        <Umschalter
          a={{ pfad: PFAD.flach, name: t("bilder.karte.flach") }}
          b={{ pfad: PFAD.kugel, name: t("bilder.karte.kugel_lang") }}
          istA={!p.kugel}
          setzeA={(flach) => p.setzeKugel(!flach)}
        />
        <Umschalter
          a={{ pfad: PFAD.norden, name: t("livemap.orient_north") }}
          b={{ pfad: PFAD.kurs, name: t("livemap.orient_track") }}
          istA={!p.trackUp}
          setzeA={(norden) => p.setzeTrackUp(!norden)}
        />
      </div>

      <div className="aa-livemap-controls__group">
        <Schalter pfad={PFAD.track} name={t("livemap.layer_track")} an={p.showTrack} umschalten={p.umschaltenTrack} />
        <Schalter
          pfad={PFAD.taxi}
          name={t("livemap.layer_taxi")}
          an={p.showTaxi}
          umschalten={p.umschaltenTaxi}
          hinweis={p.taxiPlaetze.length > 0 ? t("livemap.taxi_loaded", { airports: p.taxiPlaetze.join(" · ") }) : undefined}
        />
        {p.bilderMarken > 0 && (
          <Schalter pfad={PFAD.bilder} name={t("livemap.layer_bilder")} an={p.bilderAn} umschalten={p.umschaltenBilder} />
        )}
        <Schalter pfad={PFAD.va} name={t("livemap.layer_va")} an={p.showVa} umschalten={p.umschaltenVa} />
      </div>

      <div className="aa-livemap-controls__group">
        {/* Ein Umschalter statt zweier Schalter: So KANN nicht beides zugleich an sein. Zwei Netze
            übereinander ergeben ein Bild, in dem niemand mehr erkennt, wer welchen Luftraum betreut. */}
        <span className="aa-livemap-seg" role="group" aria-label={t("livemap.netz", "Netz")}>
          {(["aus", "vatsim", "ivao"] as const).map((wert) => (
            <button
              key={wert}
              type="button"
              className={`aa-livemap-seg__btn ${p.netz === wert ? "aa-livemap-seg__btn--active" : ""}`}
              aria-pressed={p.netz === wert}
              aria-label={wert === "aus" ? t("livemap.netz_aus", "Aus") : wert === "vatsim" ? "VATSIM" : "IVAO"}
              onClick={() => p.setzeNetz(wert)}
              title={
                wert === "aus"
                  ? t("livemap.netz_aus_hint", "Kein Online-Netz anzeigen")
                  : wert === "vatsim"
                    ? t("livemap.layer_vatsim_hint", "Sektoren nach Höhe (VATGlasses, CC BY-NC-SA), Lotsen und fremde Piloten aus dem VATSIM-Netz")
                    : t("livemap.netz_ivao_hint", "Lotsen, ATIS und fremde Piloten aus dem IVAO-Netz. Flächen aus den FIR-Grenzen — IVAO gibt keine eigenen Sektoren heraus.")
              }
            >
              {wert === "aus" ? t("livemap.netz_aus", "Aus") : wert === "vatsim" ? "VATSIM" : "IVAO"}
            </button>
          ))}
        </span>
        {/* Nur bei VATSIM: IVAO liefert keine Höhenbänder, ein Regler ohne Wirkung wäre eine Lüge.
            Der Regler ist immer sichtbar; „eigene Höhe“ schaltet ihn nur still, solange der Flug
            die Höhe liefert (ein Bedienelement, das man suchen muss, ist falsch gebaut). */}
        {p.netz === "vatsim" && (
          <span className="aa-livemap-flwahl">
            <button
              type="button"
              className={`aa-livemap-toggle ${p.flAuto ? "aa-livemap-toggle--active" : ""}`}
              aria-pressed={p.flAuto}
              aria-label={t("livemap.fl_auto", "eigene Höhe")}
              onClick={p.umschaltenFlAuto}
              title={t("livemap.fl_auto_hint", "Sektoren auf der eigenen Flughöhe zeigen")}
            >
              {t("livemap.fl_auto", "eigene Höhe")}
            </button>
            <input
              type="range"
              min={0}
              max={450}
              step={10}
              value={p.flWert}
              disabled={!p.flRegelbar}
              onChange={(e) => p.setzeFlWert(Number(e.target.value))}
              aria-label={t("livemap.fl_regler", "Flugfläche für die Sektoransicht")}
              title={t("livemap.fl_regler", "Flugfläche für die Sektoransicht")}
            />
            <span className="aa-livemap-flwahl__wert">{p.flAnzeige}</span>
          </span>
        )}
      </div>

      {p.hatFlug && (
        <div className="aa-livemap-controls__group">
          {/* Nur bei laufendem Flug: Ohne einen hat Folgen keine Wirkung. */}
          {p.hatPosition && (
            <button
              type="button"
              className="aa-livemap-seg__solo"
              aria-label={t("livemap.center_on_flight")}
              title={t("livemap.center_on_flight")}
              onClick={p.zentrieren}
            >
              <Sym pfad={PFAD.zentrieren} />
            </button>
          )}
          <button
            type="button"
            className={`aa-livemap-toggle ${p.folgen ? "aa-livemap-toggle--active" : ""}`}
            aria-pressed={p.folgen}
            aria-label={t("livemap.follow")}
            title={t("livemap.follow")}
            onClick={p.umschaltenFolgen}
          >
            {t("livemap.follow")}
          </button>
        </div>
      )}
    </div>
  );
}
