# Absicherung gegen Fremd-Traffic — Umstellungsplan

Stand 29.09.2026. Anlass: Andere VAs haben den öffentlichen Client geforkt und
die phpVMS-Domain umgebogen. Ziel: kein Traffic auf live.kant.ovh, der nicht von
der GSG kommt, und eine Lizenz, die Nutzung außerhalb der GSG untersagt.

**Entscheidung Thomas (29.09.):** Das Repo bleibt öffentlich — ein privates Repo
kostet Build-Minuten auf GitHub. Updates und Installationsdateien bleiben deshalb
wie bisher auf GitHub. Keine Verschleierung (bei lesbarem Quellcode sinnlos), kein
GitHub-Token nötig.

## Was vorbereitet ist (nichts davon ist live)

| Repo | Zweig | Inhalt |
|---|---|---|
| aeroacars-live | `feat/absicherung-fremdverkehr` | Grundtor, Schalter `INHALTE_NUR_MIT_ANMELDUNG`, Schatten-Report privat, Navdaten-Limit, `SkinContext` |
| AeroACARS | `feat/absicherung-v1912` | Skin, Kartenstil, VATGlasses, Discord mit Pilot-Token (Rust-Befehl `live_inhalt`); proprietäre Lizenz mit KI-Hinweis in LICENSE/README/Über-Dialog. Aufgesetzt auf v1.9.12. Die Lizenzseite im Windows-Installer (`licenseFile`) ist bewusst NICHT drin — nicht auf Windows testbar (Umlaute, stilles Update); erst nach einem Dev-Build-Test nachziehen. |

## Reihenfolge

### Phase 1 — Server (merken die Piloten nicht)

1. **Recorder** `feat/absicherung-fremdverkehr` mergen und deployen (Rezept
   `aeroacars-live-recorder-deploy`). `INHALTE_NUR_MIT_ANMELDUNG` **nicht**
   setzen. Der Deploy erzeugt die Caddyfile neu (Schatten-Report weg). Webapp
   mit dem neuen `SkinContext` bauen und mit ausrollen. Danach prüfen:
   - `curl -s -o /dev/null -w '%{http_code}' https://live.kant.ovh/api/admin/push/vapid-public-key` → 401
   - `curl -s -o /dev/null -w '%{http_code}' https://live.kant.ovh/shadow-phase-report.json` → 308
   - `curl -s -o /dev/null -w '%{http_code}' https://live.kant.ovh/api/v2-skin` → 200 (Schalter noch aus)
   - Admin-Webapp: Login, Karte, Skin, Push-Einstellungen
   - ein laufender Flug: Live-Karte, Log-Upload nach dem Flug
   Zusätzlich für den Überlastungs-Schutz (Commit 94e8c0c im Recorder-Zweig):
   - Caddy-Vorlage wird beim Deploy neu erzeugt (Zeitgrenzen, Körper max. 210 MB);
     vorher `caddy validate` (mit dem Server-Caddy 2.11.4 schon einmal grün).
   - `curl -s -o /dev/null -w '%{http_code} %{header:connection}\n' -X POST -H 'Content-Type: application/json' -H 'Authorization: Bearer erfunden' --data-binary @<200-KB-Datei> https://live.kant.ovh/api/backup/landings` → `401 close`
   - **Grundsatz (Thomas, 29.09.): Kein GSG-Pilot wird je ausgesperrt oder gedrosselt.**
     Dafür gilt im Recorder:
     - Bekannt gute Schlüssel (30 Tage, dauerhaft in `provision_key_hashes`) sind
       von Adress-Sperre und Budget ausgenommen — auch nach einem Neustart.
     - Die Adress-Sperre gilt nur unbekannten Schlüsseln, erst ab 60 Fehlversuchen/h
       (ein phpVMS-Schlüssel ist nicht zu erraten).
     - Ein angemeldeter Pilot darf 1.200 Anfragen/min (Unbekannte 300).
     - Ein neuer Pilot wird nur von einem Botnetz gebremst (Budget 600/min,
       je Absender 60 unbekannte Schlüssel/h).
     - **Notschalter:** `FLOOD_SCHUTZ=aus` in `/etc/aeroacars-recorder.env`, dann
       `systemctl restart aeroacars-recorder` — nimmt Vorprüfung großer Pakete und
       Anmelde-Budget weg. Bei Verdacht auf einen zu Unrecht gebremsten Piloten
       zuerst das, dann in Ruhe suchen.
     - Prüfen nach dem Ausrollen: eine echte Anmeldung eines Piloten, ein
       Log-Upload nach einem Flug, Live-Karte mit einem laufenden Flug.
2. **CARTO**: Ist ein Schlüssel hinterlegt (`carto_api_key`), war er öffentlich
   abrufbar → bei CARTO neu erzeugen und eintragen.
3. **GlitchTip**: Projekt-Grenze für Ereignisse pro Minute setzen.

### Phase 2 — Client-Version (nur mit Thomas' Freigabe)

4. `feat/absicherung` in die nächste Version nach dem X-Plane-Plugin nehmen.
5. QS über den Umfang (Skill `qs-loop`).
6. Dev-Build prüfen: Windows-Installer zeigt die Lizenzseite (Umlaute korrekt),
   Hintergrund-Update bleibt nicht daran hängen; nach dem Login laden Skin,
   Kartenstil, VATGlasses und Discord; dasselbe auf dem LAN-Tablet.
7. Veröffentlichen.

### Phase 3 — warten, dann zumachen

8. Im Health-Report die Client-Versionen beobachten, bis (fast) alle
   GSG-Piloten auf der neuen Version sind.
9. `INHALTE_NUR_MIT_ANMELDUNG=1` in die Recorder-Umgebung, Neustart. Prüfen:
   `curl -s -o /dev/null -w '%{http_code}' https://live.kant.ovh/api/v2-skin` → 401.
   Nachzügler mit alter Version sehen dann die Standard-Skin und keine
   Sektoren; fliegen und abgeben geht weiter.

## Was die Absicherung nicht leisten kann

- Quellcode und Installationsdateien bleiben öffentlich. Wer sich einen Client
  selbst baut, bekommt die lokalen Funktionen (Aufzeichnung, Landeauswertung,
  PIREP an die eigene phpVMS) — aber nichts von unserem Server. Rechtlich ist
  das ab der neuen Version untersagt (LICENSE), technisch nicht zu verhindern.
- Bereits veröffentlichte Versionen (≤ 1.9.x) und die Forks bleiben unter MIT.
- Wer einen gültigen GSG-Zugang hat, sieht, was sein Client bekommt (auch den
  CARTO-Schlüssel). Das Navdaten-Limit (120 Flughäfen/Stunde) verhindert nur
  das Absaugen in Serie.
- Der Lizenztext ist ein Entwurf ohne Rechtsprüfung.

## Überlastung / DDoS — was wo abgefangen wird

| Ebene | Wer | Stand |
|---|---|---|
| Netz (Bandbreite, SYN-, UDP-, Reflexions-Fluten, ungültige Pakete) | Hetzner, kostenlos | Belegt: https://www.hetzner.com/unternehmen/ddos-schutz/ (Stand 29.09.2026) |
| HTTP-Anfragen, die echt aussehen | Recorder | Bremse je Adresse / je echtem Token; große Pakete nur mit Zugang; Anmelde-Budget für die GSG-Webseite |
| Langsame/hängende Verbindungen, riesige Pakete | Caddy | `read_header 20s`, `idle 2m`, Körper max. 210 MB |
| Viele gleichzeitige Verbindungen einer Adresse (443, `/mqtt`) | **offen** | Kein Programm-Schutz. Kandidaten: Verbindungslimit je Adresse in der Firewall des Servers (nftables — Syntax vorher auf einem Testserver prüfen), oder Cloudflare (Free) vor die Domain. |

Bewusst NICHT gemacht: `max_connections` bei Mosquitto. Eine feste Obergrenze
würde ein Angreifer mit ein paar hundert leeren Verbindungen selbst ausfüllen
und damit alle Piloten aussperren — sie hilft nur mit einem Limit je Adresse.

Grenzen: Die Bremse im Programm kostet selbst Rechenzeit. Eine reine
Anfragenflut ohne Zugangsdaten wird nur mit 401 beantwortet (billig), aber nicht
gezählt, weil das Grundtor vor der Route-Bremse läuft. Was das Programm nicht
schafft, muss am Rand des Servers passieren.
