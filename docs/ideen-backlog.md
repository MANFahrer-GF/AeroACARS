# AeroACARS — Ideen-Backlog

Gesammelte Funktionswünsche mit Ausarbeitung. Stand 12.08.2026.
Nichts hier ist beschlossen; die offenen Entscheidungen stehen jeweils am Ende.

---

## 0. Beschlossen: was in die nächste Version soll

**Entscheidung Thomas, 12.08.2026:** Der Client-Fix an der Flugbericht-Übernahme
geht NICHT einzeln raus, sondern fährt mit Chat und VATSIM-Karte zusammen.
Ausdrücklich **nicht jetzt** — kein Zeitdruck, kein Zwischenrelease.

Damit hängt an der nächsten Client-Version:

| Teil | Stand |
|---|---|
| Übernahme prüft Strecke + Flugzeug (`pirep_ist_uebernehmbar`) | **fertig, committet, nicht released** (`8978f47`) |
| Pilotenchat | Ausarbeitung unten, Entscheidungen offen |
| VATSIM auf der Client-Karte | Ausarbeitung unten, Entscheidungen offen |

Der Übernahme-Fix ist der einzige davon, der einen echten Fehler behebt:
gleiche Flugnummer auf anderer Strecke oder mit anderem Flugzeug liess den
neuen Flug die Kennung des alten erben. Er liegt fertig da und wartet nur auf
die Begleitung — falls doch früher etwas anderes released wird, sollte er
einfach mitfahren.

---

## 1. Pilotenchat im Client

**Wunsch (Thomas):** „einen Chat in AeroACARS für online Flieger (nur für online) —
und was passiert, wenn man offline ist danach?"

### Was „online" heißt — zwei Lesarten

Der Wunsch lässt beides zu, und die Wahl ändert das Produkt:

| Lesart | Wer darf mitreden | Charakter |
|---|---|---|
| **A · aktive Flieger** | Wer gerade einen Flug im Client laufen hat | Cockpit-Funk unter Kollegen, klein und ruhig |
| **B · Netzwerk-Flieger** | Nur wer auf VATSIM/IVAO unterwegs ist | Sehr kleine Runde, oft niemand da |

**Empfehlung: A.** B klingt exklusiver, ist aber in der Praxis fast immer leer — bei
einer VA unserer Größe sind selten mehrere gleichzeitig auf VATSIM. A hat dieselbe
Wirkung (nur wer fliegt, redet mit) ohne die Runde totzumachen. Wer zusätzlich auf
VATSIM ist, kann ein Abzeichen bekommen — das ist die schönere Umsetzung von „für
Online-Flieger", weil sie niemanden ausschließt.

### Abgrenzung zu Discord — sonst bauen wir es zweimal

Discord bleibt der Ort für alles Dauerhafte: Ankündigungen, Diskussionen, Support,
Bilder. Der Client-Chat kann und soll das nicht ersetzen. Sein Daseinsgrund ist
genau eine Sache: **im Cockpit erreichbar sein, ohne aus dem Vollbild zu wechseln.**
Alt-Tab kostet im Sim spürbar (Ruckler, manchmal Verbindungsabbruch); das ist der
Schmerz, den der Chat nimmt.

Daraus folgt der Zuschnitt: kurze Zurufe („wer ist noch nach Lissabon unterwegs?",
„EDDF hat gerade Gewitter", „bin gleich am Gate"), keine Threads, keine Dateien,
keine Reaktionen.

### Die Offline-Frage — der eigentliche Knackpunkt

Drei Modelle, und die Wahl entscheidet über Aufwand, Recht und Erwartung:

**a) Flüchtig wie Funk.** Wer nicht dabei war, hat es verpasst. Nichts wird
gespeichert.
*Für:* trivial umzusetzen, datenschutzrechtlich unbedenklich, keine Moderation nötig,
keine Erwartung „ich muss nachlesen".
*Gegen:* Wer 20 Minuten im Anflug beschäftigt war, verpasst alles. Fühlt sich
unfertig an.

**b) Kurzes Gedächtnis (Empfehlung).** Beim Verbinden bekommt der Client die letzten
**50 Nachrichten oder 12 Stunden**, je nachdem was kleiner ist. Danach läuft es live.
Älteres wird automatisch gelöscht.
*Für:* Man steigt mit Kontext ein („ah, sie reden über das Gewitter in Frankfurt"),
ohne dass ein Archiv entsteht. Die Löschfrist ist die Datenschutz-Antwort.
*Gegen:* Braucht eine kleine Tabelle auf dem Server und eine Aufräum-Aufgabe.

**c) Voller Verlauf.** Alles bleibt, durchsuchbar.
*Gegen:* Dann bauen wir einen Messenger — mit Moderation, Meldefunktion,
Löschanfragen, Aufbewahrungsfristen. Das ist ein eigenes Produkt und konkurriert
direkt mit Discord. **Klar abraten.**

**Und wenn jemand offline geht?** In Modell b passiert schlicht nichts: Keine
Benachrichtigungen, kein Nachliefern per Mail, kein ungelesen-Zähler über Tage. Der
Chat ist an den Flug gebunden — schließt der Pilot den Client, ist er raus, und beim
nächsten Start sieht er die letzten Stunden. Das ist ehrlich und erzeugt keine
Bringschuld. Wer garantiert erreicht werden will, schreibt in Discord.

### Technik — das meiste steht schon

- **Transport:** Der MQTT-Broker läuft (`aeroacars/{va}/{pilot}/{kanal}`), und der
  Client abonniert bereits einen Rückkanal (`integrity_flag`). Ein Chat-Kanal
  `aeroacars/{va}/chat` fügt sich ohne neue Infrastruktur ein.
- **Berechtigung:** Der Client authentifiziert sich schon per phpVMS-API-Schlüssel.
  Der Recorder muss prüfen: Schreiben darf nur, wer eine **offene Flugsitzung** hat
  (`flight_sessions.ended_at IS NULL`) — damit ist „nur für Flieger" serverseitig
  durchgesetzt und nicht nur in der Oberfläche versteckt.
- **Absender:** Rufzeichen plus Klarname aus `provisioned_pilots`, nicht frei
  wählbar. Kein Platz für Fantasienamen, keine Verwechslung.
- **Speicher (Modell b):** Eine Tabelle `chat_messages`, Aufräumen im bestehenden
  Wartungs-Takt.
- **Oberfläche:** Eigener Reiter oder — schöner — ein einklappbarer Streifen, der
  auch über der Karte liegen kann. Auf dem Tablet über die LAN-Brücke funktioniert
  er automatisch mit (der Rückkanal ist gespiegelt).
- **Aufwand grob:** Server 1 Tag, Client 1–2 Tage, dazu Tests.

### Was vorher geklärt sein muss

1. **Datenschutzseite ergänzen** (Modell b): Was wird gespeichert, wie lange, warum.
   Wir haben die kombinierte Rechtsseite — dort gehört ein Absatz hin.
2. **Moderation:** Mindestens ein Admin-Knopf zum Löschen einzelner Nachrichten und
   zum Stummschalten eines Piloten. Ohne das nicht starten — auch in einer kleinen,
   netten VA.
3. **Abschaltbar:** Ein Schalter in den Einstellungen. Wer im Sim seine Ruhe will,
   soll den Streifen wegbekommen.

### Offene Entscheidungen für Thomas

- Lesart A (alle aktiven Flieger) oder B (nur VATSIM/IVAO)?
- Modell a (flüchtig) oder b (12 Stunden Gedächtnis)?
- Eigener Reiter oder überlagernder Streifen?

---

## 2. VATSIM in der Client-Karte

Verkehr und Lotsen wie auf der GSG-Live-Map. Die Datenquelle ist offen und ohne
Schlüssel nutzbar; die Webapp macht es bereits, der Code ist also vorhanden und
müsste „nur" in den Client wandern.

**Der eigentliche Gewinn sind die Lotsen, nicht die Flugzeuge.** Michels PDC-Problem
am 11.08. (SBBR meldete „nicht registriert") wäre mit einer Lotsenanzeige sofort
erklärt gewesen: Man sieht, ob überhaupt jemand am Platz ist und unter welchem
Rufzeichen. Empfehlung: mit der Lotsen-Ebene anfangen, Verkehr danach.

Zu beachten: Abruf höchstens alle 15 Sekunden (Rücksicht auf die Datenquelle), und
auf schwachen Geräten abschaltbar halten — hunderte Flugzeuge auf der Karte kosten
Rechenzeit, die im Sim fehlt.

---

## 3. Tab-Wechsel über die LAN-Brücke ist träge

**Ursache steht fest:** Fünf Ansichten (Karte, Logbuch, Landung, PDC/CPDLC,
Release-Notes) werden erst beim ersten Öffnen nachgeladen. Am PC ist das ein
Plattenzugriff, auf dem Tablet ein WLAN-Transfer — danach holen die Ansichten ihre
Daten in weiteren Einzelanfragen.

Drei Hebel, aufsteigend nach Aufwand:

1. **Vorladen im Leerlauf** — sobald die App steht, die Ansichten still im
   Hintergrund holen. Größter Effekt, kleinster Eingriff.
2. **Daten zwischenspeichern** — beim Zurückwechseln den letzten Stand sofort zeigen
   und im Hintergrund auffrischen, statt weiß zu bleiben.
3. **Abfragen bündeln** — ein Tab-Wechsel soll nicht fünf einzelne Anfragen
   auslösen.

---

## 4. Menü als Symbolleiste statt fester Seitenleiste

Vorschau gebaut (11.08.). Empfehlung: **Symbolleiste (56 px statt 190 px)**, nicht
das klassische Burger-Menü.

Beim Burger kostet jeder Wechsel zwei Klicks, und im geschlossenen Zustand
verschwinden genau die Dinge, die man im Flug im Blick haben will: die
Verbindungslampen (GSG, Sim, LIVE) und der Nachrichten-Zähler. Die Symbolleiste
spart fast denselben Platz, behält aber alles sichtbar.

Darüber hinaus **mitwachsend**: großer Monitor → volle Leiste wie heute, Tablet →
Symbolleiste, Handy → Burger. Reine Fenstergrößen-Logik, niemand muss etwas
einstellen. Dazu ein Knopf zum manuellen Ein- und Ausklappen, dessen Wahl gemerkt
wird.
