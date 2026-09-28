# Plan: „Flugzeug vermessen" im AeroACARS-Client

Stand 28.09.2026 · Auftrag Thomas: statt eines Extra-Werkzeugs für Michel die geführte
Schaltermessung in den Client einbauen, für MSFS und X-Plane.

## Warum

Die Profile brauchen zwei Hälften:

| | Aircraft-Scan (live seit 05.07.) | Live-Messung (fehlt) |
|---|---|---|
| Frage | Welche Variablen gibt es? | Welche geht mit dem Schalter mit, mit welchen Werten? |
| Quelle | Dateien des Add-ons | laufender Simulator, Pilot schaltet |
| Grenze | Marketplace-verschlüsselt (iniBuilds A380 2024) unlesbar | keine — misst, was der Sim liefert |

Bisher von Hand: `tools/schalterpruefung` (MSFS, Rust, Zweig `tools/schalterpruefung`)
und `tools/xplane-messung` (X-Plane, Python, Zweig `fix/a380-autobrake-rto`). Beide
Ansätze sind erprobt und werden die Grundlage.

## Ablauf für den Piloten

1. Einstellungen → Technik → **Flugzeug vermessen** (nur am Boden, Sim verbunden).
2. Hinweis: wozu, Dauer ~5 min, was gesendet wird (nur Flugzeugwerte).
3. **Ruhemessung** 10 s: nichts anfassen → Werte, die sich von selbst ändern, fallen raus.
4. **Geführte Schritte**, je Schalter die Stellungen nacheinander, Knopf „Erledigt"
   (statt Enter), „Gibt es nicht" je Stellung und je Schalter:
   Beacon · Strobes (OFF/AUTO/ON) · NAV (OFF/1/2, Airbus NAV & LOGO) ·
   Landelicht (OFF/ON/RETRACT) · Taxi (OFF/TAXI/T.O.) · Anschnallzeichen
   (OFF/AUTO/ON) · Transponder (STBY/AUTO/ON/ALT/TA/TA-RA) · Klappen (alle Rasten) ·
   Spoiler ARMED · Autobrake (alle Stufen, RTO) · APU · Parkbremse.
   Live-Anzeige: „12 Werte sind mitgegangen" als Rückmeldung, dass es wirkt.
5. **Senden** → live.kant.ovh, Einsende-Strecke des Aircraft-Scans (`/api/ascan`,
   `source = "messung"`, X-API-Key wie beim Client-Scan). Pilot sieht die Einsendung
   unter „Meine Einreichungen", kann sie löschen (DSGVO-Regel wie Scan: Bericht bleibt
   anonymisiert).

## Technik

- **X-Plane:** Web-API v2 WebSocket, alle Zahlen-Datarefs abonnieren, **Pakete ≤ 500**
  (größere Abo-Nachrichten beantwortet X-Plane 12.4 nicht — gemessen 27.09.). Arrays
  elementweise bis 48. Wert-Lesbarkeit wie im Existenz-Fix.
- **MSFS:** aus `tools/schalterpruefung` übernehmen — LVars direkt per SimConnect
  (Liste + Werte), B:-Input-Events (Enumerate + Subscribe), dazu die Standard-SimVars
  der Schalter. Nur Windows.
- **Auswertung im Client:** je Schritt die Werte, die sich über die Stellungen ändern
  und nicht „unruhig" sind; Add-on-Werte vor Standardwerten. Bericht = JSON
  (Flugzeugkennung, Sim, Version, Schritte mit Kandidaten und Werten je Stellung).
- **Server:** `ascan_submissions` bekommt `source = "messung"` + `report_json`;
  Admin-Tab „Aircraft-Scan" zeigt Messungen neben Scans desselben Flugzeugs
  (Zuordnung über ICAO + Titel/Pfad). Mail an `ascan_mail_to` wie beim Scan.
- **Profilbau bleibt bei uns:** aus Messung (+ Scan) das Profil ableiten, testen,
  mit dem nächsten Release ausliefern. Kein automatisches Übernehmen.

## Umfang und Reihenfolge

1. Server: Einsende-Typ „messung" + Admin-Anzeige (klein, ohne Release-Gate).
2. Client X-Plane-Messung (Mac + Windows testbar, Demo auf dem Mac).
3. Client MSFS-Messung (Windows, aus schalterpruefung).
4. Oberfläche (Assistent) + i18n de/en/it + Tests + Vorschau.
5. QS (Codex/Claude-Prüfer), Release mit Thomas' Go.

## Nicht Teil davon

- Automatische Profilerzeugung.
- Messung im Flug (nur am Boden, damit nichts den Flug stört).

## Teil 2 (geplant, nicht in diesem Update): Autopilot im Reiseflug

Anlass 28.09.2026: Die LiveMap zeigt AP/HDG/ALT/NAV/APP aus dem Standard-
SimVar `AUTOPILOT MASTER` & Co. Bei den meisten Airbus-Add-ons ist der tot.
Flüge der letzten 45 Tage, Anteil „AP oberhalb FL200 nie an":
A320 52/62 · A321 44/44 · A319 35/35 · A388 20/21 · A359 16/17 · B77L 10/16 ·
B738 11/23 — dagegen A21N, BCS3, E55P, MD11 je 0.

Am Boden nicht messbar: Thomas hat am 28.09. im iniBuilds-A380 am Gate AP1
gedrückt — rastet nicht ein. Also eigener Teil, nur in der Luft:

- erscheint, wenn das Flugzeug in der Luft ist (statt „bitte am Boden messen"),
  Hinweis: nur im ruhigen Reiseflug, nicht in kritischer Phase bei ATC
- Schritte: AP an/aus/an (endet AN) · lateral NAV/HDG/NAV · vertikal ALT/V/S/ALT
  · APP/LOC optional im Anflug (überspringbar)
- Ruhemessung in der Luft filtert Position, Sprit usw.; zusätzlich Kandidaten
  nur, wenn gleiche Stellungen gleiche Werte liefern (an/aus/an → a/b/a)
- Kandidaten aus dem A380-AAO-Profil: `L:INI_ap1_on`, `L:INI_ap2_on`,
  `L:INI_ATHR_LIGHT`, `L:INI_MCU_LOC_LIGHT`, `L:INI_FCU_HDG_DASHED`
- danach LiveMap-/Log-Felder je Profil aus den Messungen verdrahten
