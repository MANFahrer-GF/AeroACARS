# AeroACARS — Ideen-Backlog

Gesammelte Funktionswünsche mit Ausarbeitung. Stand 12.08.2026.

**Beim Lesen beachten:** Dieses Dokument ist eine Momentaufnahme und veraltet
schneller als der Code. Vor jeder Aussage über den Stand einer Sache am Code
nachsehen — genau das ist am 12.08. schiefgegangen, als der längst gebaute
Vorlade-Mechanismus hier noch als offen stand.
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

### VDGS — geklärt am 12.08.2026 (gemessen, nicht vermutet)

**Entschieden:** Es geht um `vats.im/vdgs`, das A-CDM-Werkzeug von VATSIM Spain.
Die Andockanzeige am Gate ist NICHT gemeint.

**Was der Aufruf ergibt.** `vats.im/vdgs` leitet auf `cdm.vatsimspain.es/vdgs/`
und von dort auf `auth.vatsim.net/oauth/authorize` mit **`client_id=1560`** und
einer Rückleit-Adresse auf deren Server. Daraus folgt:

- **Wir können den Piloten dort nicht anmelden.** Die OAuth-Anwendung gehört
  VATSIM Spain. Eine eigene VATSIM-Connect-Anwendung gäbe uns nur die Identität
  des Piloten, keinen Zugang zu deren Daten.
- **Keine Schnittstelle.** Die üblichen Pfade (`api.php`, `data.php`, `/api`,
  `get.php`) antworten alle mit 404.

**Und der wichtigste Punkt, bestätigt von Thomas:** Vorbelegen müssen wir gar
nichts. Rufzeichen und EOBT holt sich die Seite aus dem VATSIM-Flugplan, TSAT,
CTOT und ATFCM rechnet deren CDM-Logik. Der Pilot trägt dort selbst nur die TOBT
ein. „Daten übergeben" fällt als Aufgabe damit weg.

**Was bleibt — reine Anzeige, zwei Stufen:**

1. **Fenster mit der Seite.** Ein Knopf in AeroACARS öffnet ein eigenes Fenster,
   der Pilot meldet sich einmal mit VATSIM an, die Sitzung bleibt bestehen. Kein
   Rahmen in unserer Oberfläche (Cloudflare, PHP-Sitzung, fremdes Eigentum),
   sondern ein echtes Fenster. Klein, in etwa einer Stunde gebaut.
2. **Werte ablesen und weiterreichen** — TOBT, TSAT, CTOT, Taxi-Zeit — und dort
   zeigen, wo sie gebraucht werden: Cockpit-Fenster, Tablet, MSFS-Panel im Sim.
   Das ist der eigentliche Gewinn: die Zeiten sehen, ohne aus dem Sim zu
   wechseln.

**Vorbehalte, die man beim Bauen kennen muss:**

- Stufe 2 hängt am HTML der fremden Seite. Ändert es sich, steht bei uns „—".
  Deshalb nur anzeigen und NICHTS davon in Flugberichte schreiben.
- Die Seite pollt selbst („Retrying…"), es gibt also einen internen
  Datenendpunkt. Im eingeloggten Zustand ist in einer Minute sichtbar, ob der
  sich sauber ansprechen lässt — das wäre stabiler als das HTML.
- Anständig wäre eine kurze Mail an VATSIM Spain: nicht um Erlaubnis, sondern
  damit sie Bescheid geben können, wenn sich etwas ändert.

---

## 3. Tab-Wechsel über die LAN-Brücke ist träge — GRÖSSTENTEILS ERLEDIGT (v1.5.7)

**Stand 12.08.2026, am Code geprüft.** Zwei der drei Hebel sind gebaut und
ausgeliefert:

1. **Vorladen im Leerlauf — ERLEDIGT** (`App.tsx`, `prefetchViews`): sobald die
   App steht, werden die fünf nachgeladenen Ansichten nacheinander im
   Hintergrund geholt. Nacheinander, nicht gleichzeitig — auf dem Tablet teilen
   sich Telemetrie und Nachschub dieselbe WLAN-Strecke.
2. **Daten zwischenspeichern — ERLEDIGT** (`lib/ipc.ts`): 20 Sekunden
   Weiterverwendung, harte Obergrenze 120 Sekunden. Bewusst eine kurze,
   ausdrückliche Liste (Flughafen-Stammdaten, Flugzeugdaten, abgeschlossene
   Flugberichte) — die QS hatte die erste, weite Fassung als gefährlich
   entlarvt, u.a. beim Divert-Fenster. Begründung steht im Code.
3. **Abfragen bündeln — offen.** Der kleinste der drei Hebel: ein Tab-Wechsel
   löst weiterhin mehrere Einzelanfragen aus.

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
