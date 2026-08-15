# Pre-Release-Checkliste AeroACARS

**MUSS** vor jedem `git tag vX.Y.Z && git push origin vX.Y.Z` durchgegangen werden. Jede Zeile abhaken oder explizit übersteuern (im Release-PR-Body dokumentieren warum).

Hintergrund: v0.9.0/v0.9.1/v0.9.2 hatten einen **release-blocking Update-Modal-Bug** der erst beim Endkunden (Discord-Befund Svenny1974 2026-05-18) auffiel — Modal sprengte den Viewport, Roh-Markdown wurde nicht gerendert. Hätten wir die Checkliste gehabt + die jetzt existierenden vitest-Guards, wäre der Bug VOR Release rot geworden.

---

## 0. Branch-Hygiene

- [ ] Branch ist auf aktuellem `main` rebased (kein 8-Monate-alter Branch wie der v0.10.0-Feature-Branch der StableApproachBanner-Diff produzierte)
- [ ] `git diff origin/main..HEAD --stat` enthält NUR Files die wirklich zu diesem Release gehören
- [ ] Keine `dev-only`-Artefakte in `git diff --cached` (z. B. ad-hoc Test-Buttons, Sentry-Test-Endpoints)

## 1. Version-Bump synchron an allen drei Stellen

- [ ] `client/package.json` `version`
- [ ] `client/src-tauri/Cargo.toml` `[workspace.package] version`
- [ ] `client/src-tauri/tauri.conf.json` `version`
- [ ] `Cargo.lock` automatisch mit-updated (cargo run / check)

Check: `grep -E '"0\.X\.Y"|version = "0\.X\.Y"' client/package.json client/src-tauri/Cargo.toml client/src-tauri/tauri.conf.json` zeigt **alle drei** mit der neuen Version.

## 2. Bilinguale Release-Notes

- [ ] `docs/release-notes/vX.Y.Z.md` existiert
- [ ] Hat 🇩🇪 Deutsch-Block UND 🇬🇧 English-Block
- [ ] Wenn Wire-Format-Änderungen drin sind: VA-Owner-Hinweis ob `aeroacars-live` mit-deployed werden muss
- [ ] Wenn Migration-Sensible Änderung drin (Score-Algorithmen, DB-Schemas): konkreter Hinweis was passieren sollte
- [ ] **Notes FINAL machen, BEVOR der Tag gesetzt wird.** Die CI liest die
      Datei im getaggten Stand. Was danach committet wird, landet weder im
      Release-Text noch im Update-Fenster der Piloten — und faellt niemandem
      auf, weil beides trotzdem gefuellt aussieht. Nachtraeglich kostet es
      ZWEI Korrekturen (v1.6.4, 15.08.2026):
      `gh release edit vX.Y.Z --notes-file docs/release-notes/vX.Y.Z.md`
      UND das `notes`-Feld in `latest.json` neu hochladen
      (`gh release upload vX.Y.Z latest.json --clobber`) — letzteres ist das,
      was der Pilot im Update-Fenster tatsaechlich liest. Die Signaturen in
      `latest.json` bleiben dabei unberuehrt, aber danach pruefen.
- [ ] Diff gegen die Notes gegenlesen: `git log --reverse --oneline <letzter-Tag>..HEAD`.
      Bei v1.6.4 fehlten so drei pilotensichtbare Punkte (gebuchte Positionen,
      Nahverkehrsbereiche, Klickfenster-Kosmetik) — Thomas hat sie gefunden,
      nicht die Checkliste.

## 3. Tests grün

- [ ] `cargo check` (`client/src-tauri/`) ohne Warnings/Errors
- [ ] **`cargo test --workspace`** — nicht `cargo test`! Ohne `--workspace`
      läuft aus `client/src-tauri` **nur** das Paket `aeroacars-app`; die
      Tests von `landing-scoring` (also der komplette Landungs-Score) laufen
      dann gar nicht mit. Bei der QS zu v1.6.3 blieben dadurch zwei echte
      Regressionen lokal grün, die in der CI rot gewesen wären.
- [ ] `cargo test -p landing-scoring` (deckt `--workspace` mit ab; einzeln
      nur nützlich, wenn man gezielt am Score arbeitet)
- [ ] `cargo test -p aeroacars-app --lib` (Backend-Lib-Tests)
- [ ] `cargo test --doc` (**Doctests!** — v0.19.3 hat `main` rot hinterlassen, weil ein
      eingerückter Beispiel-Text im Modul-Kommentar von `arrival.rs` als Rust-Code
      interpretiert und zu kompilieren versucht wurde. `--lib` fängt das NICHT.)
- [ ] `npm test` im `client/` (Vitest)
- [ ] `npx tsc -b` im `client/` (Strict-Type-Check)

**Speziell für jeden Release verpflichtend:**

- [ ] `UpdateButton.test.tsx` ist grün — verhindert die Wiederholung des Svenny-Bugs (Modal-Struktur + Markdown-Parsing). Wenn rot: NICHT releasen, erst beheben.

## 4. Update-Modal-Smoke-Test (manuell, 60 Sekunden)

Klingt trivial, hätte aber den Svenny-Bug gefangen. Vor JEDEM Release:

- [ ] `npm run dev` starten
- [ ] In DevTools-Console: ein Mock-Update injizieren um den Modal zu erzwingen — z. B. via React-DevTools Component-State auf `UpdateButton` modifizieren, oder den `useUpdateChecker`-Hook lokal mit Stub patchen
- [ ] **Mit dem AKTUELLEN Release-Body öffnen** (= Inhalt aus `docs/release-notes/vX.Y.Z.md`)
- [ ] Sichtprüfung:
  - [ ] Modal bleibt im Viewport (nicht über Bildschirmrand)
  - [ ] „Installieren"-Button sichtbar am unteren Rand
  - [ ] Notes scrollen wenn lang
  - [ ] Markdown ist gerendert (keine `###`, keine `**bold**`-Roh-Strings, keine Tabellen-Pipes als Text)

Alternative falls Mock-Injection zu aufwändig: Manuell auf zweitem PC mit alter Version installieren, neuen Tag rauspushen, abwarten bis Updater anbietet, Modal aufmachen und durchklicken.

## 5. Wire-Compat (wenn Score-/Payload-/DB-Änderungen drin sind)

- [ ] `aeroacars-live` Branch existiert mit Mirror-Implementation
- [ ] aeroacars-live `tsc --noEmit` grün
- [ ] aeroacars-live `landingScoring.test.ts` grün
- [ ] **Reihenfolge:** aeroacars-live ZUERST deployen (VPS `deploy-recorder.sh`), DANN Pilot-Client-Tag pushen. Sonst sehen frisch-updatete Piloten Felder die der Recorder noch nicht durchreicht.

## 6. v0.9.x-Update-Pfad (einmaliger Hotfix)

Solange noch v0.9.x-Clients da draußen sind:

- [ ] Discord-Ankündigung enthält klar dass v0.9.x-User **manuell** das neue Setup.exe von GitHub Releases installieren müssen (das Modal in v0.9.x ist kaputt, kann das Auto-Update nicht auslösen)
- [ ] v0.9.2 (und ggf. v0.9.1, v0.9.0) als `--prerelease` markiert via `gh release edit v0.9.2 --prerelease`, damit der Updater von Bestands-Installationen v0.10.0 als "latest" sieht

Ab v0.10.0+ funktioniert das Auto-Update wieder normal (Modal-Hotfix in v0.10.0).

## 6b. Flow-Widget (MSFS-In-Sim-Panel) mitziehen

Das Flow-Widget wird NICHT über den Updater verteilt, sondern als Zip im
Nextcloud-Tauschordner (`~/Library/CloudStorage/Nextcloud-cloud.kant.ovh-
ThomasKant/Daten Tausch Claude/`). Es veraltet deshalb lautlos, wenn man
es beim Release vergisst (Anweisung Thomas, 13.08.2026: „bitte auch immer
den letzten Stand von Flow mitziehen").

- [ ] Prüfen, ob sich `msfs-panel/flow-widget/` seit dem letzten Paket
      geändert hat:
      `find msfs-panel/flow-widget -newer "<Tauschordner>/aeroacars-flow-widget-v*.zip" -type f`
- [ ] Wenn ja: Version hochzählen, neu zippen, ins Tauschverzeichnis
      legen (`aeroacars-flow-widget-vX.Y-fuer-export-YYYYMMDD.zip`),
      altes Paket dort liegen lassen (Historie).
- [ ] Wenn der Panel-SERVER im Client geändert wurde (client/src-tauri
      panel_server, /panel/*-Routen): prüfen, ob das Widget die Änderung
      braucht — Server und Widget sind ein Wire-Paar wie Client/Recorder.

## 7. Release-Tag + GitHub-Actions

- [ ] PR auf `main` gemerget
- [ ] **Letzter Blick auf die Notes** — nach dem Tag ist die Datei fuer die CI eingefroren (siehe Abschnitt 2).
- [ ] `git tag vX.Y.Z && git push origin vX.Y.Z` (KEIN lokales `npm run tauri build` — GitHub-Actions baut signed Win+Mac, siehe `MEMORY.md` „Release-Automation")

> **Lokal testen: `npm run build:lokal`.**
> Der private Signaturschluessel liegt bewusst nur in der CI. Ein lokales
> `npm run tauri build` laeuft deshalb bis zum Ende durch, endet aber mit
> Fehlercode ("incorrect updater private key password"), obwohl die `.app`
> fertig gebaut ist -- das sieht wie ein gescheiterter Build aus und ist
> keiner. `build:lokal` schaltet ueber `src-tauri/tauri.lokal.conf.json` nur
> die Updater-Dateien ab und endet mit 0.
>
> **Niemals `createUpdaterArtifacts` in der Hauptkonfiguration abschalten.**
> Dann bauten die CI-Releases keine `.sig` und keine `latest.json` mehr, und
> das Auto-Update aller Piloten stuende still, ohne dass es jemand merkt.
> `client/src/lokalerBuild.test.ts` haelt beides fest.

- [ ] GitHub-Release-Body aus `docs/release-notes/vX.Y.Z.md` reinkopieren (das genaue File rendert dann auch im Update-Modal sauber — siehe Stufe 4)
- [ ] Verify in den Release-Assets:
  - [ ] `latest.json` enthält neue Version
  - [ ] `AeroACARS_x64-setup.exe` + Signatures vorhanden

## 8. Post-Release-Verifikation

- [ ] Auf einem Test-Rechner mit der **vorherigen** Version: Update-Modal anbieten lassen, durchklicken, prüfen ob installiert wird
- [ ] Bei Wire-Format-Änderungen: 1 Test-Touchdown live fliegen und im aeroacars-live Dashboard verifizieren

---

## Warum diese Checklist existiert

Discord, 18.05.2026, Pilot Svenny1974:
> Wollte das Update machen, aber außer wie auf dem Bild zusehen passiert nichts. Kann nicht scrollen gar nichts.

**Root-Cause:** `.update-modal` in v0.9.x hatte kein `max-height` + kein `overflow`, plus `update.body` wurde als rohes `<p>` gerendert. Bei langem bilingualen Body sprengte das Modal den Viewport — Install-Button lag unter dem Fold.

**Was hätte den Bug verhindert (= jetzt strukturell im Repo):**
1. Diese Checkliste (Stufe 4 wäre angeschlagen)
2. `client/src/components/UpdateButton.test.tsx` (Stufe 3 wäre rot geworden)
3. WARN-Kommentare in `App.css` `.update-modal*` (Stufe 0 hätte verhindert dass jemand max-height versehentlich rausnimmt)

Diese Checkliste durchgehen ist 5 Minuten. Ein released Bug der Piloten am Updaten hindert kostet Stunden Discord-Support + nochmal-Release. Wir gehen sie durch.
