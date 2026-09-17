# Discord-Entwurf — AeroACARS v1.7.27

> Entwurf. Noch NICHT gepostet. Kanal: vermutlich #ankündigungen.

---

**✈️ AeroACARS v1.7.27 ist da — sehr weiche Landungen werden wieder erkannt, und ihr seht den Prüfstatus eurer Flüge**

**Was los war**

Sven hat mit dem Fenix A321 in Tallinn eine richtig butterweiche Landung hingelegt. AeroACARS hat sie aber gar nicht als Landung erkannt, und der Flug blieb deshalb in der Prüfung hängen, obwohl alles in Ordnung war.

Der Grund: Beim Aufsetzen prüft AeroACARS unter anderem, ob das Flugzeug wirklich knapp über dem Boden ist. Die Grenze lag fest bei 5 ft. Der Simulator misst die Höhe aber nicht an den Rädern, sondern an einem Punkt im Rumpf. Ein Fenix-Airbus steht so schon mit 9 ft auf dem Boden, ein A350 mit 15 ft. Bei normalen Landungen fiel das nicht auf, bei sehr sanften schon. Seit Juni hat das rund zehn echte Landungen großer MSFS-Flugzeuge getroffen.

**Was wir gemacht haben**

- **Landungserkennung:** AeroACARS misst jetzt beim Rollen vor dem Start, wie hoch euer Flugzeug am Boden steht, und prüft beim Aufsetzen relativ dazu. Kleine Flugzeuge, Hubschrauber und Wasserflugzeuge bleiben wie bisher.
- **Prüfstatus im Client:** Hält die VA einen Flug zur Prüfung zurück, seht ihr das jetzt direkt in AeroACARS, im Landungs-Tab und im Logbuch. Ihr seht, ob der Flug geprüft wird, warum, und ob er inzwischen freigegeben wurde. Der Stand aktualisiert sich von selbst.
- **Automatische Neubewertung:** Kommt die Landung kurz nach dem Einreichen über das Flugprotokoll nach, bewertet der Server den Flug jetzt selbst neu. Svens Flug wäre damit ohne Handarbeit durchgelaufen.

**Nebenbei mitgenommen**

- Wird eine Landung doch einmal nicht erkannt, schreibt AeroACARS die einzelnen Prüfergebnisse ins Protokoll. So finden wir die Ursache künftig schneller.
- Eine Warnung, die nach so einem Fall bei jedem Messtakt ins Protokoll lief, erscheint nur noch einmal.

Danke an Sven für die Meldung und das Protokoll, das hat die Ursache eindeutig gemacht! 🙏

**Update:** Startet AeroACARS, das Update wird euch angeboten.
