# Gesicherte Landungen — Entwurf

**Stand:** 25.07.2026 · **Status:** Entwurf, nicht gebaut · **Für:** v1.2.7 oder später

## Worum es geht

Die Landungen eines Piloten liegen heute ausschließlich lokal in
`landings.json` im App-Datenverzeichnis. Wer seinen Rechner neu aufsetzt, die
Festplatte verliert oder den Simulator auf einen neuen PC umzieht, fängt bei
null an — die gesamte Historie ist weg.

Sie sollen deshalb auf dem Live-VPS gesichert werden, mit einer harten Zusage:
**Weder der Serverbetreiber noch die Fluggesellschaft können hineinsehen.** Das
muss technisch erzwungen sein, nicht organisatorisch versprochen.

## Der Zielkonflikt, an dem alles hängt

Diese beiden Anforderungen ziehen gegeneinander:

1. Nur der Pilot kann entschlüsseln → der Schlüssel darf nirgends sonst liegen
2. Nach einem Rechner-Neuaufsetzen soll der Pilot wieder drankommen → der
   Schlüssel darf **nicht nur** auf dem Rechner liegen

Ein Schlüssel, der nur auf dem verlorenen Rechner lag, macht das Backup wertlos.
Ein Schlüssel, den der Server kennt, bricht die Zusage. Der Schlüssel muss also
an etwas hängen, das der Pilot **unabhängig vom Gerät** besitzt.

Erschwerend: Unser `secrets`-Crate legt Geheimnisse in einer JSON-Datei im
App-Verzeichnis ab (der Systemschlüsselbund wurde in v0.5.15 bewusst
verlassen). Ein dort abgelegter Schlüssel ist beim Neuaufsetzen ebenfalls weg.
Er taugt als Bequemlichkeitsspeicher, nicht als alleinige Quelle.

## Schlüsselverwaltung — die eigentliche Entscheidung

### Verworfen: Ableitung aus dem phpVMS-Passwort

Naheliegend, aber schlecht. Der Server kennt den Passwort-Hash und könnte bei
schwachen Passwörtern offline raten. Schlimmer: Ein Passwortwechsel würde
sämtliche Backups unlesbar machen — der Pilot verlöre seine Historie durch eine
Handlung, die damit nichts zu tun hat.

### Verworfen: Schlüssel nur im App-Datenverzeichnis

Löst den Wiederherstellungsfall nicht, siehe oben.

### Empfohlen: Wiederherstellungscode, vom Client erzeugt

Beim Einschalten der Funktion erzeugt der Client einen zufälligen
256-Bit-Schlüssel und zeigt ihn als **Wortliste** (BIP-39-Stil, 24 Wörter) an.
Der Pilot sichert diese Liste außerhalb des Rechners — Passwortmanager,
ausgedruckt, egal. Der Schlüssel selbst landet zusätzlich im lokalen
Geheimnisspeicher, damit der Alltag ohne Eingabe läuft.

Vorteile: volle Schlüsselstärke (kein schwaches Passwort), unabhängig vom
phpVMS-Konto, klar kommunizierbar („diese Wörter sind deine Landungen").

Nachteil, der klar gesagt werden muss: **Wortliste weg = Daten weg.** Das ist
kein Fehler, sondern der Preis dafür, dass niemand sonst hineinsehen kann.

### Optional als Komfort: zusätzliche Passphrase

Wer keine Wortliste verwahren mag, kann stattdessen eine Passphrase wählen, aus
der der Schlüssel per **Argon2id** abgeleitet wird (Parameter bewusst teuer:
mindestens 64 MiB Speicher, 3 Durchgänge). Schwächer als ein Zufallsschlüssel,
weil offline angreifbar, aber besser als kein Backup. Als Zweitweg anbieten,
nicht als Voreinstellung.

## Kryptografie

| | |
|---|---|
| Verfahren | **XChaCha20-Poly1305** (authentifiziert, AEAD) |
| Schlüssellänge | 256 Bit |
| Nonce | 192 Bit, **pro Upload neu zufällig** |
| Ableitung (Passphrase-Variante) | Argon2id, ≥64 MiB, 3 Durchgänge, zufälliger Salt |
| Bibliotheken | `chacha20poly1305`, `argon2`, `rand` — heute nicht eingebunden |

XChaCha20 statt AES-GCM wegen des langen Nonce: Bei zufälligen Nonces und vielen
Uploads ist die Kollisionsgefahr vernachlässigbar, während AES-GCM mit 96-Bit-
Nonce sorgfältige Zählerführung verlangt. Authentifiziert heißt: Ein
manipuliertes Backup fällt beim Entschlüsseln auf, statt Unsinn zu liefern.

**Verschlüsselt wird clientseitig, bevor irgendetwas das Gerät verlässt.** Der
Server bekommt einen Byteblock, den er nicht deuten kann.

## Was der Server trotzdem sieht

Ehrlichkeit an dieser Stelle ist wichtiger als eine schöne Zusage:

**Sichtbar:** welcher Pilot, wann hochgeladen, wie groß, wie oft.

**Nicht sichtbar:** Landungen, Bewertungen, Flughäfen, Zeitpunkte, Flugzeuge —
also alles, worum es inhaltlich geht.

Die Größe verrät grob die Anzahl der Landungen. Wer das nicht will, füllt auf
feste Stufen auf (z. B. Vielfache von 64 KiB). Kostet etwas Speicher, ist aber
billig. **Empfehlung: einbauen**, dann ist auch die letzte Ableitung zu.

## Serverseite

Ein neues Modul im `aeroacars-live`-Bestand, zwei Endpunkte, Anmeldung über den
vorhandenen phpVMS-API-Schlüssel — dieselbe Mechanik wie beim Flight-Log-Upload:

```
PUT  /api/backup/landings     Blob hochladen (ersetzt den Stand)
GET  /api/backup/landings     letzten Blob holen
GET  /api/backup/landings/log Liste der Stände (Zeit, Größe) ohne Inhalt
```

**Speicherung:** eine Datei je Pilot unter `/var/lib/aeroacars-recorder/backups/<va>/<pilot>/`,
plus die letzten **fünf** Stände als Historie. Grund: Ein fehlerhafter Client,
der eine leere Liste hochlädt, darf nicht die einzige Kopie vernichten.

**Grenzen:** 5 MiB je Blob, ein Upload je 5 Minuten. Eine `landings.json` mit
mehreren hundert Landungen liegt weit darunter; die Grenze fängt Fehlläufe ab.

## Clientseite

**Wann gesichert wird:** nach dem Schreiben einer neuen Landung, verzögert um
einige Minuten und zusammengefasst — nicht bei jeder Änderung sofort. Dazu
einmal beim Start, falls der letzte Versuch scheiterte.

**Bei Netzproblemen:** still scheitern und beim nächsten Anlass erneut
versuchen. Ein Backup, das den Flugbetrieb stört, ist schlechter als keines.

**Zwei Rechner:** Der einfache Weg (letzter Upload gewinnt) verliert Landungen,
wenn jemand abwechselnd an zwei Rechnern fliegt. Deshalb beim Wiederherstellen
**zusammenführen statt ersetzen** — Landungen haben mit der PIREP-Kennung einen
natürlichen Schlüssel, Dubletten sind eindeutig erkennbar. Vor dem Hochladen den
Serverstand holen, zusammenführen, dann schreiben.

## Wiederherstellung

1. Neue Installation, Anmeldung an phpVMS wie gewohnt
2. Die App findet ein Backup und bietet es an
3. Pilot gibt seine Wortliste ein (oder die Passphrase)
4. Entschlüsseln, zusammenführen, fertig

Schlägt die Entschlüsselung fehl, ist die Wortliste falsch — **nicht** das
Backup kaputt. Diese Unterscheidung muss die Meldung treffen, sonst sucht der
Pilot an der falschen Stelle.

## Grenzen — was das nicht leistet

**Ein kompromittierter Rechner sieht alles.** Ende-zu-Ende schützt Übertragung
und Server, nicht das Gerät des Piloten.

**Verlorene Wortliste = verlorene Daten.** Es gibt keine Hintertür, das ist der
Punkt der Übung. Muss beim Einschalten unübersehbar dastehen, nicht im
Kleingedruckten.

**Kein Schutz vor Löschung durch den Serverbetreiber.** Verschlüsselung
verhindert Lesen, nicht Wegwerfen. Wer das abdecken will, braucht eine zweite
Kopie woanders.

**Wer den Server übernimmt, kann Chiffrate mitnehmen** und offline gegen
Passphrasen rechnen. Bei zufälligen 256-Bit-Schlüsseln aussichtslos; bei der
Passphrase-Variante der Grund für die teuren Argon2id-Parameter.

## Aufwand

| Teil | Umfang |
|---|---|
| Krypto-Baustein im Client (Schlüssel, Wortliste, Ver-/Entschlüsseln) | überschaubar, gut testbar |
| Backup-Ablauf (auslösen, zusammenführen, wiederholen) | mittel — die Zusammenführung ist der knifflige Teil |
| Bedienoberfläche (Einschalten, Wortliste zeigen, Wiederherstellen) | mittel, braucht sorgfältige Texte |
| Serverendpunkte + Ablage | klein |
| Tests | Krypto-Rundlauf, Zusammenführung, Wiederherstellung, falsche Wortliste |

Drei neue Abhängigkeiten im Client. Kein Eingriff in Flugaufzeichnung, Bewertung
oder PIREP-Ablauf — das Backup hängt sich nur an das Schreiben der Landungsdatei.

## Was ich von Dir brauche

**Wortliste oder Passphrase als Standardweg?** Ich empfehle die Wortliste, weil
sie nicht erraten werden kann. Sie verlangt vom Piloten aber, sie wirklich zu
sichern.

**Auffüllen auf feste Größen?** Kostet Speicher, verschließt die letzte
Ableitung über die Anzahl der Landungen. Ich würde es tun.

**Verpflichtend oder freiwillig?** Ich rate zu freiwillig mit deutlichem
Hinweis. Wer die Wortliste nicht sichern will, soll nicht zu einem Backup
gezwungen werden, das er nie zurückholen kann.

**Auch die Flight-Logs sichern?** Dieser Entwurf deckt nur die Landungen ab. Die
Flight-Logs liegen ohnehin schon auf dem Server, allerdings unverschlüsselt und
für den Betreiber lesbar. Falls das auch zugehen soll, ist es ein eigener
Umbau — sag Bescheid, dann arbeite ich den getrennt aus.
