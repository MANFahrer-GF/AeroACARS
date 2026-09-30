# macOS: Signatur + Notarisierung (Apple Developer ID)

Stand 30.09.2026. Ab v1.9.14 kann der Release-Workflow die Mac-App und das
X-Plane-Plugin (`mac.xpl`) mit einer **Developer ID** signieren und bei Apple
**notarisieren** lassen. Dann öffnet macOS beides ohne Warnung
(„nicht verifizierter Entwickler“ / „beschädigt“).

Ohne eingetragene Secrets läuft alles wie bisher unsigniert — ein Release
scheitert nicht daran.

## Einmalig einrichten (Thomas)

Voraussetzung: **bezahltes** Apple Developer Program (developer.apple.com →
Account zeigt „Apple Developer Program“ mit Team-ID).

### 1. Zertifikat „Developer ID Application“

1. Auf dem Mac: **Schlüsselbundverwaltung** öffnen → Menü
   *Schlüsselbundverwaltung → Zertifikatsassistent → Zertifikat einer
   Zertifizierungsinstanz anfordern…* → E-Mail eintragen, *Auf der Festplatte
   sichern* → Datei `CertificateSigningRequest.certSigningRequest` speichern.
2. developer.apple.com → *Certificates, Identifiers & Profiles* →
   *Certificates* → **+** → **Developer ID Application** (NICHT „Apple
   Development“, NICHT „Mac App Distribution“) → Profiltyp **G2 Sub-CA** →
   die Anforderungsdatei hochladen → Zertifikat `developerID_application.cer`
   herunterladen und doppelklicken (landet im Schlüsselbund „Anmeldung“).
3. Schlüsselbundverwaltung → *Meine Zertifikate* → Eintrag
   **„Developer ID Application: … (TEAMID)“** aufklappen (Schlüssel muss
   darunter hängen) → Rechtsklick auf das Zertifikat → *Exportieren…* →
   Format **.p12** → ein neues Passwort vergeben (merken).
4. Den genauen Namen notieren, z. B.
   `Developer ID Application: Thomas Kant (AB12CD34EF)` — steht genau so im
   Schlüsselbund.

### 2. API-Schlüssel für die Notarisierung

1. appstoreconnect.apple.com → *Benutzer und Zugriff* → Reiter
   **Integrationen** → *App Store Connect API* → *Teamschlüssel* → **+**.
2. Name z. B. „AeroACARS Notarisierung“, Zugriff **Developer** → erzeugen.
3. **Schlüssel herunterladen** (`AuthKey_XXXXXXXXXX.p8` — geht nur EINMAL).
4. Notieren: **Schlüssel-ID** (10 Zeichen, Spalte „Schlüssel-ID“) und oben
   die **Aussteller-ID / Issuer ID** (UUID).

### 3. Secrets im GitHub-Repo eintragen

github.com/MANFahrer-GF/AeroACARS → *Settings* → *Secrets and variables* →
*Actions* → **New repository secret**, sechsmal:

| Name | Inhalt |
|---|---|
| `APPLE_CERTIFICATE` | die .p12 als Base64: im Terminal `base64 -i Zertifikat.p12 \| pbcopy`, dann einfügen |
| `APPLE_CERTIFICATE_PASSWORD` | Passwort aus Schritt 1.3 |
| `APPLE_SIGNING_IDENTITY` | Name aus Schritt 1.4, beginnt mit `Developer ID Application:` |
| `APPLE_API_ISSUER` | Issuer ID (UUID) aus Schritt 2.4 |
| `APPLE_API_KEY` | Schlüssel-ID (10 Zeichen) aus Schritt 2.4 |
| `APPLE_API_KEY_P8` | kompletter Inhalt der `.p8`-Datei: `pbcopy < AuthKey_XXXXXXXXXX.p8`, dann einfügen |

Danach die heruntergeladenen Dateien (.p12, .p8, .certSigningRequest) sicher
ablegen oder löschen — sie gehören nicht in den Tauschordner.

## Prüfen ohne Release

Actions → **Dev Build (unsigned, manual)** → *Run workflow* → Branch wählen,
Haken **apple_signieren** setzen. Der Schritt „Apple-Signatur pruefen“ muss
grün sein; er verlangt von Gatekeeper `source=Notarized Developer ID`.
Die DMG aus dem Lauf öffnet sich danach auf jedem Mac ohne Warnung.

## Wie es gebaut ist

- `.github/scripts/apple-signatur-vorbereiten.sh` — prüft die Secrets und
  reicht sie an Tauri weiter (Tauri signiert, notarisiert und heftet an).
- `.github/scripts/apple-app-pruefen.sh` — nach dem Bau: codesign, Hardened
  Runtime, Staple, Gatekeeper. Eingerichtet, aber nicht wirksam → Lauf rot.
- `.github/scripts/apple-plugin-signieren.sh` — `mac.xpl`: Developer-ID-
  Signatur + Notarisierung, sonst ad-hoc wie bisher. Eine lose Bibliothek
  kann nicht angeheftet werden; macOS prüft die Notarisierung beim ersten
  Laden online.
- Teilweise eingetragene Secrets → Warnung im Lauf, Build bleibt unsigniert.

Das Zertifikat läuft 5 Jahre; der API-Schlüssel, bis er widerrufen wird.
