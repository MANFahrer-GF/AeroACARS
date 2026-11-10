// Bordbuch eines geflogenen Flugs — Reiter im Logbuch (seit 10.11.2026; vorher
// eigener Tab). Checkliste und Flugprofil nebeneinander. Kein Rot, keine Punkte,
// kein Einfluss auf die Bewertung.

import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { bilanz, erledigt, sichtbar, type Eintrag, type LiveAnsicht, type Regel } from "../../lib/bordbuch";
import { BordbuchCheckliste } from "./BordbuchCheckliste";
import { BordbuchProfil } from "./BordbuchProfil";
import "./bordbuch.css";

/** Bordbuch-Punkte als Ereignisse für die Flugkarte — dieselbe Auswahl wie die Marken im Flugprofil. */
export function bordbuchEreignisse(e: Eintrag | null | undefined, t: TFunction) {
  if (!e || e.aus_grund) return undefined;
  const ereignisse = e.punkte
    .filter((p) => sichtbar(p, e.eingeschaltet) && p.zeit && (erledigt(p.status) || p.status === "diesmal_ohne"))
    .map((p) => ({
      t: new Date(p.zeit as string).toISOString(),
      k: "bordbuch",
      schluessel: p.regel,
      txt: t("bordbuch.auf_karte_text", { was: t(`bordbuch.kurz.${p.regel}`) })
        + (p.status === "diesmal_ohne" ? ` · ${t("bordbuch.status.diesmal_ohne")}` : p.stellung ? ` · ${p.stellung}` : ""),
    }));
  if (ereignisse.length === 0) return undefined;
  return {
    zusatzEreignisse: ereignisse,
    zusatzKategorien: { bordbuch: { col: "#86efac", an: true, c: "B", n: t("bordbuch.titel"), how: t("bordbuch.auf_karte_erklaerung") } },
  };
}

interface Props {
  eintrag: Eintrag | null;
  onMarkieren: (regel: Regel, nachAtc: boolean) => Promise<void>;
  /** Fehlt, wenn es zu dem Flug keine Karte gibt */
  onAufKarte?: (regel: Regel) => void;
}

export function BordbuchFlug({ eintrag, onMarkieren, onAufKarte }: Props) {
  const { t, i18n } = useTranslation();
  if (!eintrag) return <div className="bb-leer">{t("bordbuch.kein_eintrag")}</div>;
  if (eintrag.aus_grund) return <AusHinweis grund={eintrag.aus_grund} />;
  const b = bilanz(eintrag.punkte, eintrag.eingeschaltet);
  const alles = b.von > 0 && b.ok === b.von;
  const datum = new Intl.DateTimeFormat(i18n.language, { day: "2-digit", month: "2-digit", year: "2-digit" }).format(new Date(eintrag.erstellt_at));
  return (
    <div className="bb-flug">
      <section className="bb-flug-karte" aria-label={t("bordbuch.variante.checkliste")}>
        <h2>{t("bordbuch.variante.checkliste")}</h2>
        <div className="bb-unter">
          {datum}{` · ${t(`bordbuch.klasse.${eintrag.klasse}`)} · ${eintrag.regelwerk.toUpperCase()}`}{eintrag.nacht_start ? ` · ${t("bordbuch.nachts")}` : ""}
        </div>
        {b.von > 0 && <div className="bb-unter">{t("bordbuch.bilanz", { ok: b.ok, von: b.von })}. {alles ? t("bordbuch.satz_alles") : t("bordbuch.satz_teil")}</div>}
        <BordbuchCheckliste
          punkte={eintrag.punkte}
          eingeschaltet={eintrag.eingeschaltet}
          flugzeug={eintrag.flug.titel ?? eintrag.flug.muster ?? null}
          onMarkieren={onMarkieren}
          onAufKarte={onAufKarte}
          rolltempoGrenzeKt={eintrag.rolltempo_grenze_kt}
        />
      </section>
      <section className="bb-flug-karte" aria-label={t("bordbuch.variante.profil")}>
        <h2>{t("bordbuch.variante.profil")}</h2>
        <div className="bb-unter">{t("bordbuch.profil_erklaerung")}</div>
        <BordbuchProfil
          punkte={eintrag.punkte}
          eingeschaltet={eintrag.eingeschaltet}
          profil={eintrag.profil}
          dep={eintrag.flug.dep}
          arr={eintrag.flug.arr}
          rolltempoGrenzeKt={eintrag.rolltempo_grenze_kt}
          hoehe={300}
        />
      </section>
      <p className="bb-fuss bb-flug-fuss">{t("bordbuch.fuss_privat")}</p>
    </div>
  );
}

/** Das laufende Bordbuch aufgeklappt: gleiche Anordnung, aber noch nichts zu markieren. */
export function BordbuchLiveInhalt({ live }: { live: LiveAnsicht }) {
  const { t } = useTranslation();
  return (
    <div className="bb-flug bb-flug--live">
      <section className="bb-flug-karte" aria-label={t("bordbuch.variante.checkliste")}>
        <h2>{t("bordbuch.variante.checkliste")}</h2>
        <BordbuchCheckliste punkte={live.punkte} eingeschaltet={live.eingeschaltet} flugzeug={null} rolltempoGrenzeKt={live.rolltempo_grenze_kt} />
      </section>
      <section className="bb-flug-karte" aria-label={t("bordbuch.variante.profil")}>
        <h2>{t("bordbuch.variante.profil")}</h2>
        <BordbuchProfil punkte={live.punkte} eingeschaltet={live.eingeschaltet} profil={live.profil} rolltempoGrenzeKt={live.rolltempo_grenze_kt} />
      </section>
    </div>
  );
}

export function AusHinweis({ grund }: { grund: string }) {
  const { t } = useTranslation();
  return <div className="bb-aus-hinweis">{t(`bordbuch.aus.${grund}`, t("bordbuch.aus.ga"))}</div>;
}
