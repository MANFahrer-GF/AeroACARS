/**
 * Namen zu einer Menge von Kennungen nachladen — gedeckelt, abbrechbar und
 * ohne dass ein Abbruch Einträge dauerhaft verschluckt.
 *
 * Warum das eine eigene Datei ist: die Buchhaltung drumherum ist die
 * eigentliche Schwierigkeit, und im Bauch einer 4.000-Zeilen-Komponente lässt
 * sie sich nicht prüfen. Genau dort ist der schwerste Fehler dieser Runde
 * entstanden (QS 18.08.2026) — siehe {@link ladeNamenGedeckelt}.
 *
 * Die drei Regeln, die dabei zusammenspielen müssen:
 *
 *  1. **Deckel.** Höchstens `gleichzeitig` Abrufe zur selben Zeit. Vorher lief
 *     je Kennung sofort einer los: bei einem Piloten mit langer Historie waren
 *     das 89 in zwei Sekunden. Jeder braucht eine eigene Verbindung, und ein
 *     parallel laufender Abruf (die Buchungsliste) lief in den Timeout — der
 *     Pilot sah einen Netzwerkfehler mitten im Reiseflug.
 *
 *  2. **Abbruch gibt frei.** Wird abgebrochen, müssen alle nicht beantworteten
 *     Kennungen wieder freigegeben werden. Sonst gelten sie als „schon
 *     angefragt" und werden nie wieder versucht.
 *
 *  3. **Fehlschläge sind gedeckelt.** Ein Fehlschlag gibt frei (ein Aussetzer
 *     soll sich nicht einbrennen), aber nur bis zu `hoechstversuche`. Ohne
 *     diese Schranke entsteht mit einem periodisch neu laufenden Aufrufer eine
 *     unbegrenzte Wiederholschleife — genau dann, wenn das Netz ohnehin klemmt.
 */

/** Gedächtnis über mehrere Läufe hinweg. Gehört dem Aufrufer (typisch: eine Ref). */
export interface NachladeGedaechtnis {
  /** Kennungen, die bereits angefragt wurden (laufend oder abgeschlossen). */
  angefragt: Set<string>;
  /** Fehlversuche je Kennung. */
  versuche: Map<string, number>;
}

export function leeresGedaechtnis(): NachladeGedaechtnis {
  return { angefragt: new Set(), versuche: new Map() };
}

export interface NachladeOptionen {
  /** Höchstzahl gleichzeitiger Abrufe. */
  gleichzeitig: number;
  /** Nach wie vielen Fehlversuchen eine Kennung endgültig aufgegeben wird. */
  hoechstversuche: number;
}

export const NACHLADE_VORGABE: NachladeOptionen = {
  // Vier ist die übliche Hausnummer eines Browsers je Gegenstelle. Die
  // Gesamtdauer leidet kaum: die Antworten sind winzig, und der Rust-Teil
  // puffert sie prozessweit — beim zweiten Öffnen geht gar keine Anfrage raus.
  gleichzeitig: 4,
  hoechstversuche: 3,
};

/**
 * Arbeitet `kennungen` gedeckelt ab und ruft `melden` für jeden Treffer.
 *
 * Gibt eine Funktion zurück, die den Lauf abbricht **und dabei aufräumt**.
 * Genau dieses Aufräumen war der Fehler: `records` wird alle 5 s neu gesetzt,
 * der aufrufende Effekt lief also alle 5 s neu und räumte die noch laufende
 * Warteschlange ab. Die nicht abgearbeiteten Kennungen standen aber schon als
 * „angefragt" im Gedächtnis — der nächste Lauf übersprang sie damit **für
 * immer**. Auf einer langsamen Leitung wäre gut die Hälfte der Zeilen dauerhaft
 * ohne Ortsnamen geblieben; ausgerechnet auf der Verbindung also, für die der
 * Deckel überhaupt gebaut wurde.
 */
export function ladeNamenGedeckelt<T>(
  kennungen: Iterable<string>,
  gedaechtnis: NachladeGedaechtnis,
  abrufen: (kennung: string) => Promise<T | null>,
  melden: (kennung: string, wert: T) => void,
  optionen: NachladeOptionen = NACHLADE_VORGABE,
): () => void {
  // Ränder, die von aussen leicht hereinkommen (QS-Runde 3):
  //  * Doppelte Kennungen — `filter` sieht die Merkliste, das Eintragen kommt
  //    erst danach; ohne `new Set` gingen Dubletten doppelt raus.
  //  * Leere Zeichenketten — ein Datensatz ohne Flughafen erzeugte sonst einen
  //    Abruf auf "".
  const offen = [...new Set(kennungen)].filter(
    (k) => k !== "" && !gedaechtnis.angefragt.has(k),
  );
  for (const k of offen) gedaechtnis.angefragt.add(k);

  let abgebrochen = false;
  // Was wirklich beantwortet wurde (oder endgültig aufgegeben). Alles andere
  // gibt der Abbruch wieder frei.
  const erledigt = new Set<string>();
  let naechster = 0;

  void (async () => {
    const arbeiter = async () => {
      while (!abgebrochen) {
        const kennung = offen[naechster++];
        if (kennung === undefined) return;
        try {
          const wert = await abrufen(kennung);
          if (abgebrochen) return;
          if (wert !== null && wert !== undefined) melden(kennung, wert);
          // Auch ein leeres Ergebnis zählt als erledigt: der Abruf lief, es
          // gibt schlicht nichts. Sonst fragten wir bis zum Versuchslimit
          // immer wieder an.
          erledigt.add(kennung);
        } catch {
          const bisher = (gedaechtnis.versuche.get(kennung) ?? 0) + 1;
          gedaechtnis.versuche.set(kennung, bisher);
          if (bisher < optionen.hoechstversuche) {
            gedaechtnis.angefragt.delete(kennung); // später nochmal
          } else {
            erledigt.add(kennung); // aufgegeben, nicht wieder freigeben
          }
        }
      }
    };
    await Promise.all(
      // Mindestens ein Arbeiter, sonst passiert bei `gleichzeitig: 0` gar
      // nichts — und die Kennungen blieben als "angefragt" haengen, ohne dass
      // je jemand fragt.
      Array.from(
        { length: Math.min(Math.max(1, optionen.gleichzeitig), offen.length) },
        () => arbeiter(),
      ),
    );
  })();

  return () => {
    abgebrochen = true;
    for (const kennung of offen) {
      if (!erledigt.has(kennung)) gedaechtnis.angefragt.delete(kennung);
    }
  };
}
