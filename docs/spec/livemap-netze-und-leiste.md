# Live-Karte: zweites Netz (IVAO) und schlankere Leiste

Stand 16.08.2026 · Konzept, noch nicht gebaut · entschieden mit Thomas

## Warum

Die Kartenleiste ist voll. Ein zweites Netz passt nur hinein, wenn vorher
Platz entsteht — und beim Aufräumen fällt auf, dass ein guter Teil der Breite
für Dinge draufgeht, die nichts anzeigen, was man nicht ohnehin sieht.

## Entscheidungen

**Nie beide Netze gleichzeitig.** Zwei Netze übereinander ergeben ein Bild,
in dem niemand mehr erkennt, wer welchen Luftraum betreut. Deshalb kein
zweiter Schalter neben VATSIM, sondern EIN dreigeteilter Umschalter:

    Aus · VATSIM · IVAO

Die Ausschließlichkeit steckt damit in der Bauform. Es braucht keine Regel,
die verhindert, dass beide an sind — es geht schlicht nicht.

**Zweier-Gruppen werden Symbole.** „Ansicht" (Karte/Satellit) und
„Ausrichtung" (Norden/Kurs) sind je EINE Ja/Nein-Entscheidung, kosten aber je
zwei breite Wörter plus Überschrift. Als Symbolpaar ohne Überschrift etwa ein
Drittel so breit. Vertretbar, weil bei einem Zweier-Umschalter immer eine
Hälfte hell ist — der Zustand bleibt eindeutig.

**Ebenen-Schalter: Symbol, Text nur wenn an** (Variante B).
Track, Taxiweg und VA-Verkehr sind UNABHÄNGIGE Ein/Aus-Schalter, anders als
die Paare oben. Bei reinen Symbolen hinge der Zustand allein an der Färbung.
Der aktive Schalter trägt deshalb sein Wort, die inaktiven nur das Symbol.
Preis: die Leiste zuckt beim Umschalten um die Wortbreite. Bewusst in Kauf
genommen — Zustandsklarheit vor Ruhe im Layout.

**Was nichts tut, verschwindet.**
- Der Höhenregler erscheint nur bei aktivem Netz (ohne Netz regelt er nichts).
- „Auf Flug zentrieren" nur bei laufendem Flug, und dann als Fadenkreuz.

**Überschriften raus, Trennstriche rein.** „ANSICHT", „EBENEN" und so weiter
erklären, was ohnehin sichtbar ist. Dünne Trennlinien gruppieren genauso gut.

## Breite

| Gruppe | heute | danach |
|---|---|---|
| Ansicht + Ausrichtung | ~340 px | ~110 px |
| Ebenen (3 Schalter) | ~250 px | ~150 px |
| Netz | 95 px (nur VATSIM) | ~190 px (drei Zustände) |
| Zentrieren | ~150 px | ~34 px |
| **gesamt** | **~835 px** | **~485 px** |

Bei einer Funktion mehr.

## IVAO: was dafür nötig ist — und was nicht

**Keine OAuth-Freigabe.** Der Live-Feed ist offiziell öffentlich:
`https://api.ivao.aero/v2/tracker/whazzup`, ohne Anmeldung, gemessen 482 KB
in 112 ms. Das IVAO-Wiki sagt dazu wörtlich: „IVAO Public APIs don't require
any authentication nor access request."

**Sektorflächen kommen NICHT von IVAO.** Sie entstehen wie bei VATSIM aus den
VATSpy-Grenzen — echte FIR-Grenzen sind real-weltliche Lufträume, keine
Netz-Eigenheit. Die GSG-Webseite macht das seit Langem genau so
(`live_map_scripts.blade.php`: dieselbe Funktion `renderActiveSectors` für
beide Netze). Diese Vorlage ist erprobt und muss nicht erfunden werden.

**Nicht verfolgt:** IVAO hat eigene, feinere Sektoren hinter den
undokumentierten Endpunkten `/v2/ATCPositions/{id}` und `/v2/subcenters/{id}`
(gefunden im Bündel von tracker.ivao.aero, das nach der Rufzeichen-Endung
zwischen beiden wählt). Die verlangen ein Anwendungs-Token über
`client_credentials`. Wäre der Weg zu mehr Genauigkeit — nur wenn sich die
FIR-Näherung als zu grob erweist.

**Der Feed ist reichhaltiger als der von VATSIM:** ATIS-Text bei allen
Lotsen, Entfernung zu Start und Ziel vorgerechnet. Rufzeichen-Aufbau ist
identisch (`LEAL_TWR`, `OXMF_S_CTR`) — der vorhandene Zerleger passt ohne
Umbau.

## Bauweg

1. Recorder: zweiter Zulauf in denselben Trichter. Feed holen, Lotsen
   einlesen, durch dieselbe Zuordnung wie VATSIM schicken, als eigene Ebene
   ausgeben. Kein neuer Rechenkern.
2. Client: Umschalter ersetzt den VATSIM-Knopf; die Abfrage bekommt das
   gewählte Netz mit.
3. Leiste umbauen (Symbole, Variante B, bedingte Einblendung).

## Offen

Nichts mehr. Der letzte Punkt (Symbol für „Taxiweg") ist am 16.08.2026
entschieden — siehe unten.

## Entschieden nach dem Bau (16.08.2026, mit Thomas)

- **Symbol für „Taxiweg": `traffic-cone`.** Vorher stand dort Tabler
  `building-airport` (verifiziert: alle acht Pfade identisch mit dem
  Original). Das Motiv heißt aber „Flughafen", nicht „Rollweg", und lief bei
  15 px zu einem Fleck zusammen. Ein Weg-Motiv verbietet sich weiterhin: der
  nächstbeste Kandidat `arrow-guide` trägt Punkt und Knick wie das
  Track-Symbol direkt daneben — genau die Kollision, vor der dieser Entwurf
  gewarnt hat. `windsock` steht zwar am Flugplatz, meint aber Wind. Der Hut
  meint den Bodenbereich statt des Weges; vier Striche, die auch klein noch
  eine Silhouette haben.

- **„VA-Verkehr" bleibt ausgeschrieben.** Die hier geplante Kürzung auf „VA"
  ist nie in den Code gegangen: alle drei Sprachen tragen seit dem
  Design-System-Umbau (c17ffb6) die Langform — „VA-Verkehr" / „VA Traffic" /
  „Traffico VA". Thomas hat die Langform bestätigt. Erledigt, nicht vergessen.
- **Die Browser-Livemap (live.kant.ovh) bleibt ohne IVAO.** Der Recorder
  liefert `netz=ivao` längst aus, `webapp/src` fragt aber weiter nur VATSIM
  (181 Treffer gegen 0). Das ist so gewollt — IVAO ist eine Client-Funktion,
  die beiden Karten dürfen sich hier unterscheiden.
- **Feinere IVAO-Sektoren bleiben geparkt.** Die token-pflichtigen Endpunkte
  werden erst angefasst, wenn die FIR-Näherung sich als zu grob erweist.
- **Kein Rollwege-Schalter auf der Browser-Karte.** Aufgefallen beim
  Symboltausch: die Webapp legt die OSM-Bodendaten als feste Ebenen an
  (`ground-apron`, `ground-terminal`, sichtbar ab Zoom 12) und kennt kein
  `showTaxi` — auf live lassen sich die Rollwege also nicht abschalten,
  anders als im Client. Thomas braucht den Schalter dort vorerst nicht.
  Bewusste Asymmetrie, keine Lücke; jederzeit nachrüstbar.
