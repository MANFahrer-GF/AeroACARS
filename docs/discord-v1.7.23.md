# Discord-Entwurf — AeroACARS v1.7.23

> Entwurf. Noch NICHT gepostet. Kanal: vermutlich #ankündigungen.
> Kein Discord-Tool in dieser Sitzung angebunden — bitte selbst posten.

---

**✈️ AeroACARS v1.7.23 ist da — sanfte Landungen werden jetzt erkannt**

**Was los war**

Ein ruhiger, sehr weicher Touchdown konnte bisher als „kein Touchdown erkannt" enden — obwohl im Hintergrund längst ein voller Score berechnet war. Der PIREP blieb dann im Prüfprozess hängen, und ihr musstet euch fragen, was da schiefgelaufen ist. Der Grund: die Erkennung des Aufsetz-Moments verlangte eine physikalische Abstimmung aus mehreren Messwerten — bei einem wirklich sanften, aber echten Aufsetzer (z. B. G-Kraft knapp unter der Schwelle) reichte die Anzahl der „Stimmen" nicht.

**Was wir gemacht haben**

Die Erkennung berücksichtigt jetzt zusätzlich eine eindeutige, durchgehende Bodenberührung als gleichwertigen Nachweis für einen echten Touchdown. Bounces, kurze Streifschüsse und Telemetrielücken bleiben davon unberührt — die fallen weiterhin nicht durch.

**Nebenbei mitgenommen**

Meldet der Simulator für euer Flugzeug nur die generische Baureihe ohne Variante (z. B. „A300" statt „A306"), fehlten bisher Spurweite und Spannweite in der Bahndisziplin-Bewertung — die seitliche Randabstands-Prüfung wurde übersprungen, obwohl das Muster eigentlich bekannt ist. Betrifft z. B. einige A300-600F-Addons. Behoben.

Und intern: die Entscheidung, wann eine Landung als „final" gilt und der Score verschickt wird, lief über zwei unabhängige Mechanismen, die sich gegenseitig ins Gehege kommen konnten (vor allem bei einem Touch-and-Go). Jetzt gibt es dafür nur noch eine einzige, verlässliche Stelle.

**Update:** startet AeroACARS, das Update wird euch angeboten.
