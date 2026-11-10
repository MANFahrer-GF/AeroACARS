// Reiter Protokoll eines Flugs (Design 10.11.2026): PIREP-Felder nach Themen,
// Finanzen und das Fluglogbuch. Alte PIREPs oder ein älterer Modulstand liefern
// Felder und Finanzen nicht; dann bleibt nur, was da ist.
import { useTranslation } from "react-i18next";
import { elapsed, esc, type Detail } from "./logbuchForm";
import { buchungstext, finanzen, gruppieren } from "./protokollDaten";

export function LogbuchProtokoll({ detail }: { detail: Detail }) {
  const { t, i18n } = useTranslation();
  const gruppen = gruppieren(detail.fields);
  const felderZahl = gruppen.reduce((s, g) => s + g.zeilen.length, 0);
  const fin = finanzen(detail.transactions);
  const hatBuchungen = fin.einnahmen.length + fin.kosten.length > 0;
  const beladung = (detail.fares ?? []).filter((f) => f.count > 0);
  const hatFinanzen = hatBuchungen || beladung.length > 0;
  const log = detail.log ?? [];

  if (felderZahl === 0 && !hatFinanzen && log.length === 0) {
    return <div className="aa-lb-panel">{t("logbook_view.protokoll_leer")}</div>;
  }

  const zahl = new Intl.NumberFormat(i18n.language);
  const geldForm = new Intl.NumberFormat(i18n.language, { style: "currency", currency: detail.currency || "EUR" });
  const geld = (cent: number) => geldForm.format(cent / 100);

  return (
    <div className={`aa-lb-prot${felderZahl > 0 && hatFinanzen ? " aa-lb-prot--zwei" : ""}`}>
      {felderZahl > 0 && (
        <section className="aa-lb-panel" aria-label={t("logbook_view.protokoll_felder")}>
          <div className="aa-lb-prot-kopf">
            <h3>{t("logbook_view.protokoll_felder")}</h3>
            <span>{t("logbook_view.protokoll_felder_zahl", { count: felderZahl })}</span>
          </div>
          <div className="aa-lb-prot-spalten">
            {gruppen.map((g) => (
              <div className="aa-lb-prot-gruppe" key={g.gruppe}>
                <div className="aa-lb-prot-gtitel">{t(`logbook_view.protokoll_gruppe_${g.gruppe}`)}</div>
                {g.zeilen.map((z, i) => (
                  <div className="aa-lb-prot-zeile" key={`${z.slug}-${i}`}>
                    <span>{z.name}</span>
                    <span className="aa-lb-prot-wert">{z.value}</span>
                  </div>
                ))}
              </div>
            ))}
          </div>
        </section>
      )}
      {hatFinanzen && (
        <section className="aa-lb-panel" aria-label={t("logbook_view.protokoll_finanzen")}>
          <h3>{t("logbook_view.protokoll_finanzen")}</h3>
          {hatBuchungen && (
            <div className="aa-lb-prot-summen">
              <div>
                <div className="aa-lb-k">{t(fin.ergebnis < 0 ? "logbook_view.protokoll_verlust" : "logbook_view.protokoll_gewinn")}</div>
                <div className={`aa-lb-prot-ergebnis ${fin.ergebnis < 0 ? "ist-minus" : "ist-plus"}`}>{geld(fin.ergebnis)}</div>
              </div>
              <div><div className="aa-lb-k">{t("logbook_view.protokoll_einnahmen")}</div><div className="aa-lb-prot-summe">{geld(fin.summeEin)}</div></div>
              <div><div className="aa-lb-k">{t("logbook_view.protokoll_kosten")}</div><div className="aa-lb-prot-summe">{geld(fin.summeAus)}</div></div>
            </div>
          )}
          {beladung.length > 0 && (
            <div className="aa-lb-prot-beladung">
              <span className="aa-lb-prot-still">{t("logbook_view.protokoll_beladung")}</span>
              {beladung.map((f) => (
                <span key={f.code || f.name}>{f.name} <strong>{zahl.format(f.count)} {f.type === 1 ? (detail.units?.weight || "kg") : "Pax"}</strong></span>
              ))}
            </div>
          )}
          {fin.einnahmen.length > 0 && (
            <>
              <div className="aa-lb-prot-gtitel aa-lb-prot-ein">
                <span>{t("logbook_view.protokoll_einnahmen")}</span>
                <span>{t("logbook_view.protokoll_betrag")}</span>
                <span>{t("logbook_view.protokoll_kosten")}</span>
              </div>
              {fin.einnahmen.map((b, i) => (
                <div className="aa-lb-prot-zeile aa-lb-prot-ein" key={`${b.memo}-${i}`}>
                  <span>{buchungstext(b.memo)}</span>
                  <span className="aa-lb-prot-wert ist-plus">{geld(b.credit_c)}</span>
                  <span className="aa-lb-prot-wert aa-lb-prot-still">{b.debit_c ? geld(b.debit_c) : ""}</span>
                </div>
              ))}
            </>
          )}
          {fin.kosten.length > 0 && (
            <>
              <div className="aa-lb-prot-gtitel">{t("logbook_view.protokoll_kosten")}</div>
              {fin.kosten.map((b, i) => (
                <div className="aa-lb-prot-zeile aa-lb-prot-kosten" key={`${b.memo}-${i}`}>
                  <span>{buchungstext(b.memo)}</span>
                  <span className="aa-lb-prot-wert">{geld(b.debit_c)}</span>
                </div>
              ))}
            </>
          )}
        </section>
      )}
      {log.length > 0 && (
        <section className="aa-lb-panel aa-lb-prot-log" aria-label={t("logbook_view.detail_log_title")}>
          <h3>{t("logbook_view.detail_log_title")}</h3>
          <div className="aa-lb-log" dangerouslySetInnerHTML={{
            __html: log.map((l) => {
              const phase = l.message.startsWith("Phase:");
              // v0.19-Stage-E: "phase" (ohne Praefix) kollidierte mit der
              // GLOBALEN .phase-Klasse (App.tsx Lade-/Session-Check-Screen)
              // — Grenzflaeche/Hintergrund/Padding/flex-column von dort
              // ueberlagerten die Grid-Zeile hier, sichtbar als kaputte
              // Kaesten im Fluglogbuch. Eigener, praefixierter Modifier.
              return `<div class="aa-lb-logrow ${phase ? "aa-lb-logrow--phase" : ""}"><span class="aa-lb-t">${elapsed(l.t)}</span><span class="aa-lb-m">${phase ? '<span class="aa-lb-dot"></span>' : ""}${esc(l.message)}</span></div>`;
            }).join(""),
          }} />
        </section>
      )}
    </div>
  );
}
