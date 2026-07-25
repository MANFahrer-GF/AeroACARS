# Gesicherte Landungen — Entwurf

**Stand:** 25.07.2026 · **Status:** Entwurf, nicht gebaut · **Ziel:** v1.2.7 oder später

## Worum es geht

Die Landungen eines Piloten liegen heute ausschließlich lokal in
`landings.json` im App-Datenverzeichnis. Wer seinen Rechner neu aufsetzt, die
Festplatte verliert oder auf einen neuen PC umzieht, fängt bei null an.

Sie sollen deshalb auf dem Live-Server gesichert werden, sodass ein neu
installierter Client sie zurückholt.

## Die Schutzanforderung

**Kein Pilot darf die Landungen eines anderen sehen. Administratoren dürfen
es.** (Entscheidung Thomas, 25.07.2026.)

Das ist bewusst enger gefasst als die zuerst angedachte Ende-zu-Ende-
Verschlüsselung — und dadurch erheblich einfacher. Ein früherer Entwurf sah
einen Schlüssel vor, den nur der Pilot besitzt (24-Wort-Liste, XChaCha20,
Argon2id). Das hätte drei Krypto-Abhängigkeiten, einen Wiederherstellungsablauf
mit Schlüsseleingabe und vor allem ein hartes Verlustrisiko gebracht:
Wortliste weg = Landungen unwiederbringlich weg. Für Landebewertungen aus einem
Flugsimulator steht das in keinem Verhältnis.

**Verworfen wurde auch**, den Verschlüsselungsschlüssel aus dem phpVMS-
API-Schlüssel abzuleiten. Der steht in `phpvmsusers.api_key` im Klartext — jeder
mit Datenbankzugriff könnte damit jedes Backup öffnen, die Verschlüsselung wäre
also Fassade. Zudem hätte ein neu erzeugter API-Schlüssel alle alten Backups
unlesbar gemacht.

Ohne Verschlüsselung verschwindet dieses Problem: Der API-Schlüssel dient nur
noch der Anmeldung, nicht dem Aufschließen. Wer sich einen neuen erzeugt, weist
weiterhin denselben Piloten aus und kommt an seine Daten.

## Was das nicht leistet

Wer den Server übernimmt, kann die Landungen lesen. Das ist die bewusste
Konsequenz der Entscheidung oben und für diese Datenart vertretbar.

## Serverseite

Ein kleines Modul im bestehenden `aeroacars-live`, drei Endpunkte, Anmeldung
über `requireBearerPilot` — dieselbe Mechanik wie bei Navdaten und Flight-Logs:

```
PUT  /api/backup/landings       Stand hochladen
GET  /api/backup/landings       eigenen letzten Stand holen
GET  /api/backup/landings/log   eigene Stände auflisten (Zeit, Größe)
```

**Zugriffsregel:** Der Pfad enthält **keine** Piloten-Kennung. Der Server nimmt
sie ausschließlich aus dem geprüften Token (`req.navdataPilot`). Damit ist es
strukturell unmöglich, durch Verbiegen eines Parameters an fremde Daten zu
kommen — der häufigste Fehler bei genau dieser Art Endpunkt.

**Ablage:** eine Datei je Pilot unter
`/var/lib/aeroacars-recorder/backups/<va>/<pilot>/`, dazu die letzten **fünf**
Stände. Grund: Ein fehlerhafter Client, der eine leere Liste hochlädt, darf
nicht die einzige Kopie vernichten.

**Grenzen:** 5 MiB je Stand, ein Upload alle fünf Minuten. Eine echte
`landings.json` mit mehreren hundert Landungen liegt weit darunter; die Grenze
fängt Fehlläufe ab.

## Clientseite

Gesichert wird nach dem Schreiben einer neuen Landung, verzögert und
zusammengefasst statt bei jeder Änderung sofort, dazu einmal beim Start, falls
der letzte Versuch scheiterte.

Scheitert der Upload, wird still beim nächsten Anlass erneut versucht — ein
Backup, das den Flugbetrieb stört, ist schlechter als keines.

**Zwei Rechner:** Wer abwechselnd an zwei Maschinen fliegt, verliert bei
„letzter Upload gewinnt" Landungen. Deshalb wird **zusammengeführt statt
ersetzt** — Landungen tragen mit der PIREP-Kennung einen natürlichen Schlüssel,
Dubletten sind eindeutig erkennbar. Vor dem Hochladen den Serverstand holen,
zusammenführen, dann schreiben.

## Wiederherstellung

1. Neue Installation, Anmeldung an phpVMS wie gewohnt
2. Die App findet ein Backup und holt es
3. Zusammenführen mit dem (leeren) lokalen Stand, fertig

Keine Schlüsseleingabe, kein Verlustrisiko, keine Sonderfälle.

## Aufwand

| Teil | Umfang |
|---|---|
| Serverendpunkte + Ablage + Historie | klein |
| Backup-Ablauf im Client (auslösen, wiederholen) | klein |
| Zusammenführung zweier Stände | mittel — der knifflige Teil |
| Oberfläche (Schalter, Stand anzeigen, jetzt sichern) | klein |
| Tests | Zusammenführung, Dubletten, fremder Pilot wird abgewiesen |

Keine neuen Abhängigkeiten. Kein Eingriff in Flugaufzeichnung, Bewertung oder
PIREP-Ablauf — das Backup hängt sich nur an das Schreiben der Landungsdatei.

## Offen

**Auch die Flight-Logs?** Dieser Entwurf deckt nur die Landungen ab. Die
Flight-Logs liegen ohnehin schon auf dem Server; dort ginge es nicht um
Sicherung, sondern darum, ob ein Pilot sie herunterladen darf. Eigenes Thema.

**Rückholen alter Stände?** Die Historie der letzten fünf Stände liegt auf dem
Server. Ob der Pilot einen davon selbst zurückholen kann oder ob das
Administratorensache bleibt, ist noch nicht entschieden.
