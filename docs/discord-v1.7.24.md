# Discord-Entwurf — AeroACARS v1.7.24

> Entwurf. Noch NICHT gepostet. Kanal: vermutlich #ankündigungen.
> Kein Discord-Tool in dieser Sitzung angebunden — bitte selbst posten.

---

**✈️ AeroACARS v1.7.24 ist da — Zielwetter aktualisiert sich jetzt von selbst**

**Was los war**

Michael hatte den Wunsch: mit Beginn des Sinkflugs soll sich das Wetter am Zielflughafen von selbst aktualisieren, ohne dass man auf „Aktualisieren" klicken muss. Und tatsächlich — das Backend hat das Zielwetter längst automatisch beim Sinkflug und kurz vor der Landung nachgeholt. Nur hat die Wetter-Karte im Cockpit das nie gezeigt: sie hat beim Öffnen immer ihr eigenes, unabhängiges METAR geholt und den frisch vorgeholten Wert einfach ignoriert.

**Was wir gemacht haben**

Die Karte übernimmt den automatisch vorgeholten Wert jetzt selbst — kein Klick mehr nötig, die Anzeige zieht live nach, sobald ein frischeres Wetter eintrifft.

**Nebenbei mitgenommen**

- **Rollweg-Grafik:** Schwenkte die Rollspur nach dem Verlassen der Bahn weit aus (z. B. eine große Maschine mit großem Kurvenradius auf eine Rollbahn), brach die Linie bisher kommentarlos am Bildrand ab — sah aus wie „keine Kurve geflogen". Ein Hinweis an der Stelle nennt jetzt den tatsächlichen Ausschlag.
- **Spurweite:** Die Beechcraft Duke (BE60), der GippsAero Airvan (GA8) und die Jakowlew Jak-18T (YK18) fehlten in der Tabelle für die seitliche Bahn-Bewertung — Landungen mit diesen Mustern zeigten „Spurweite nicht hinterlegt" statt einer Note. Alle drei sind jetzt mit Herstellerangaben hinterlegt (danke an Sven für den Hinweis mit der Duke!).

**Update:** startet AeroACARS, das Update wird euch angeboten.
