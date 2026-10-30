#!/usr/bin/env bash
# AeroACARS 3 — Beta veroeffentlichen (nur nach Freigabe ausfuehren).
#
#   scripts/beta-veroeffentlichen.sh <run-id>
#       Laedt das Artefakt AeroACARS-beta-kanal eines dev-build-Laufs
#       (Eingang beta=true), haengt Installer + Update-Dateien an die
#       Pre-Release v<version> (legt sie an, falls sie fehlt) und ersetzt
#       danach latest.json am Kanal-Release aeroacars3-beta-kanal. Ab da
#       bieten alle Beta-Installationen das Update an.
#
#   scripts/beta-veroeffentlichen.sh --final <tag>
#       Finales 3.x (z. B. v3.0.0, von release.yml als Entwurf/Pre-Release
#       gebaut): veroeffentlicht es als normales Release OHNE "latest" und
#       setzt dessen latest.json in den 3.x-Kanal aeroacars3-kanal und in den
#       Beta-Kanal (semver: 3.0.0 > 3.0.0-beta.N, die Betas wechseln mit).
#
# Idempotent: erneuter Aufruf laedt alles mit --clobber neu hoch.
# Nichts von 3.x darf je GitHub-"latest" werden — 2.x-Installationen fragen
# releases/latest/download/latest.json ab und bekaemen es sonst angeboten.
# 3.x fragt nur seine Kanaele (tauri.conf.json / tauri.beta.conf.json).
set -euo pipefail

REPO=MANFahrer-GF/AeroACARS
KANAL=aeroacars3-beta-kanal
KANAL_FINAL=aeroacars3-kanal

fehler() { echo "FEHLER: $*" >&2; exit 1; }
[[ $# -ge 1 ]] || fehler "Aufruf: $0 <run-id> | --final <tag>"
for w in gh jq curl; do command -v "$w" >/dev/null || fehler "$w fehlt"; done

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Pre-Release-Status hart pruefen: ein v3-Release als "latest" bekaeme jeder
# 2.x-Pilot als Update angeboten.
pruefe_vorab() {
  # Auch kein Entwurf: dessen Dateien waeren fuer die Clients nicht abrufbar.
  [[ $(gh release view "$1" -R "$REPO" --json isPrerelease,isDraft -q '.isPrerelease and (.isDraft | not)') == "true" ]] \
    || fehler "Release $1 ist keine veroeffentlichte Pre-Release (Entwurf oder Latest-Kandidat) — abgebrochen, bitte von Hand pruefen"
}

# Kanal-Release anlegen (falls noetig) und latest.json ersetzen — immer zuletzt.
# Kanaele sind Pre-Releases: die werden nie GitHub-"latest".
kanal_setzen() {
  local kanal=$1 json=$2 ziel=$3 titel=$4
  if ! gh release view "$kanal" -R "$REPO" >/dev/null 2>&1; then
    gh release create "$kanal" -R "$REPO" --prerelease --latest=false --target "$ziel" \
      --title "$titel" \
      --notes "Nur fuer den Auto-Updater von AeroACARS 3 (latest.json). Bitte nicht loeschen und nicht als Latest markieren."
  fi
  pruefe_vorab "$kanal"
  gh release upload "$kanal" "$json" -R "$REPO" --clobber
  # Kontrolle nur als Hinweis: der Upload ist durch, die Auslieferung kann
  # kurz verzoegert noch den alten Stand liefern.
  local live
  live=$(curl -fsSL "https://github.com/${REPO}/releases/download/${kanal}/latest.json" 2>/dev/null | jq -r .version 2>/dev/null) || live="(noch nicht abrufbar)"
  echo "Kanal $kanal liefert gerade Version: $live (erwartet: $(jq -r .version "$json"))"
}

# 2.x-Schutz: GitHub-"latest" muss ein 2.x-Release bleiben.
pruefe_latest_2x() {
  local latest
  latest=$(gh api "repos/${REPO}/releases/latest" -q .tag_name)
  [[ "$latest" != v3.* ]] \
    || fehler "GitHub-latest ist $latest — 2.x-Installationen bekaemen 3.x angeboten! Sofort zurueckstellen: gh release edit <letztes v2-Release> -R $REPO --latest"
  echo "GitHub-latest bleibt $latest (2.x)."
}

if [[ $1 == "--final" ]]; then
  TAG=${2:?Tag fehlt, z. B. v3.0.0}
  [[ "$TAG" =~ ^v3\.[0-9]+\.[0-9]+$ ]] || fehler "$TAG ist kein finales 3.x-Tag (vX.Y.Z mit X=3)"
  gh release view "$TAG" -R "$REPO" >/dev/null 2>&1 || fehler "Release $TAG fehlt — erst release.yml (Tag-Push) bauen lassen"
  gh release download "$TAG" -R "$REPO" -p latest.json -D "$TMP"
  VER=$(jq -r .version "$TMP/latest.json")
  [[ "v$VER" == "$TAG" ]] || fehler "latest.json von $TAG traegt Version $VER"
  while read -r url; do
    [[ "$url" == "https://github.com/${REPO}/releases/download/${TAG}/"* ]] || fehler "fremde URL in latest.json: $url"
  done < <(jq -r '.platforms[].url' "$TMP/latest.json")
  pruefe_latest_2x
  # Veroeffentlichen als normales Release, ausdruecklich nicht "latest".
  gh release edit "$TAG" -R "$REPO" --draft=false --prerelease=false --latest=false
  [[ $(gh release view "$TAG" -R "$REPO" --json isPrerelease,isDraft -q '.isPrerelease or .isDraft') == "false" ]] \
    || fehler "$TAG ist nach dem Veroeffentlichen noch Entwurf/Pre-Release"
  pruefe_latest_2x
  ZIEL=$(gh release view "$TAG" -R "$REPO" --json targetCommitish -q .targetCommitish)
  kanal_setzen "$KANAL_FINAL" "$TMP/latest.json" "$ZIEL" "AeroACARS 3 – Update-Kanal"
  kanal_setzen "$KANAL" "$TMP/latest.json" "$ZIEL" "AeroACARS 3 – Beta-Update-Kanal"
  pruefe_latest_2x
  echo "Fertig: $TAG veroeffentlicht (nicht latest), 3.x- und Beta-Kanal zeigen auf $VER."
  exit 0
fi

RUN=$1
INFO=$(gh run view "$RUN" -R "$REPO" --json headSha,conclusion,workflowName)
[[ $(jq -r .conclusion <<<"$INFO") == "success" ]] || fehler "Lauf $RUN ist nicht erfolgreich"
[[ $(jq -r .workflowName <<<"$INFO") == Dev\ Build* ]] || fehler "Lauf $RUN ist kein Dev-Build"
SHA=$(jq -r .headSha <<<"$INFO")

roh() { gh api -H "Accept: application/vnd.github.raw" "repos/${REPO}/contents/$1?ref=${SHA}"; }
VER=$(roh client/src-tauri/tauri.conf.json | jq -r .version)
[[ "$VER" == *-* ]] || fehler "Version $VER ist keine Vorab-Version — finale Releases laufen ueber release.yml"
TAG="v$VER"
# Der neue Tag loest release.yml aus — nur sicher, wenn dessen Fassung am
# Commit die v3-Betas ausschliesst.
RELEASE_YML=$(roh .github/workflows/release.yml)
grep -qE "^[[:space:]]*-[[:space:]]*'!v3\.\*-beta\*'" <<<"$RELEASE_YML" \
  || fehler "release.yml an $SHA schliesst v3-Beta-Tags nicht aus — Tag wuerde einen Release-Build ausloesen"

gh run download "$RUN" -R "$REPO" -n AeroACARS-beta-kanal -D "$TMP/kanal" \
  || fehler "Artefakt AeroACARS-beta-kanal fehlt (Lauf ohne beta=true oder aelter als 7 Tage?)"
cd "$TMP/kanal"

P="AeroACARS.3_${VER}"
# Das Plugin-Paket laedt die App von releases/download/v<ver>/ (Pruefsumme
# eingebaut, beta-kanal-bauen.sh hat sie gegen das Paket geprueft).
DATEIEN=("${P}_x64-setup.exe" "${P}_x64-setup.exe.sig" "${P}_aarch64.dmg" "${P}_aarch64.app.tar.gz" "${P}_aarch64.app.tar.gz.sig" "AeroACARS-XPlane-Plugin-${TAG}.zip")
for f in "${DATEIEN[@]}" latest.json; do [[ -s "$f" ]] || fehler "$f fehlt im Artefakt"; done
[[ $(jq -r .version latest.json) == "$VER" ]] || fehler "latest.json traegt $(jq -r .version latest.json), tauri.conf.json $VER"
# Jede URL zeigt auf eine Datei, die hier hochgeladen wird, und jede Signatur
# ist genau die der Datei.
while IFS=$'\t' read -r url sig; do
  name=${url##*/}
  [[ "$url" == "https://github.com/${REPO}/releases/download/${TAG}/${name}" ]] || fehler "fremde URL in latest.json: $url"
  [[ -s "$name" ]] || fehler "latest.json zeigt auf $name, das fehlt"
  [[ "$sig" == "$(tr -d '\r\n ' < "$name.sig")" ]] || fehler "Signatur zu $name passt nicht"
done < <(jq -r '.platforms[] | [.url, .signature] | @tsv' latest.json)

echo "Veroeffentliche $TAG aus Lauf $RUN (Commit $SHA) …"
if ! gh release view "$TAG" -R "$REPO" >/dev/null 2>&1; then
  jq -r .notes latest.json > notes.md
  gh release create "$TAG" -R "$REPO" --prerelease --latest=false --target "$SHA" \
    --title "AeroACARS 3 – ${VER}" --notes-file notes.md
fi
pruefe_vorab "$TAG"
gh release upload "$TAG" "${DATEIEN[@]}" -R "$REPO" --clobber
pruefe_vorab "$TAG"

# Erst wenn alle Dateien liegen, den Kanal umstellen.
kanal_setzen "$KANAL" latest.json "$SHA" "AeroACARS 3 – Beta-Update-Kanal"
pruefe_latest_2x
echo "Fertig: $TAG veroeffentlicht, Beta-Kanal aktualisiert."
