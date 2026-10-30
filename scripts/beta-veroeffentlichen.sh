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
#       Spaeter beim finalen 3.0: kopiert latest.json des fertigen Releases
#       <tag> (z. B. v3.0.0) in den Beta-Kanal, damit die Betas auf die finale
#       Fassung wechseln (semver: 3.0.0 > 3.0.0-beta.N).
#
# Idempotent: erneuter Aufruf laedt alles mit --clobber neu hoch.
# Nichts davon darf je "latest" werden — 2.x fragt releases/latest ab.
set -euo pipefail

REPO=MANFahrer-GF/AeroACARS
KANAL=aeroacars3-beta-kanal
KANAL_URL="https://github.com/${REPO}/releases/download/${KANAL}/latest.json"

fehler() { echo "FEHLER: $*" >&2; exit 1; }
[[ $# -ge 1 ]] || fehler "Aufruf: $0 <run-id> | --final <tag>"
for w in gh jq curl; do command -v "$w" >/dev/null || fehler "$w fehlt"; done

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Pre-Release-Status hart pruefen: ein v3-Release als "latest" bekaeme jeder
# 2.x-Pilot als Update angeboten.
pruefe_vorab() {
  [[ $(gh release view "$1" -R "$REPO" --json isPrerelease -q .isPrerelease) == "true" ]] \
    || fehler "Release $1 ist KEINE Pre-Release — abgebrochen, bitte von Hand pruefen"
}

# Kanal-Release anlegen (falls noetig) und latest.json ersetzen — immer zuletzt.
kanal_setzen() {
  local json=$1 ziel=$2
  if ! gh release view "$KANAL" -R "$REPO" >/dev/null 2>&1; then
    gh release create "$KANAL" -R "$REPO" --prerelease --latest=false --target "$ziel" \
      --title "AeroACARS 3 – Beta-Update-Kanal" \
      --notes "Nur fuer den Auto-Updater der AeroACARS-3-Betas (latest.json). Bitte nicht loeschen und nicht als Latest markieren."
  fi
  pruefe_vorab "$KANAL"
  gh release upload "$KANAL" "$json" -R "$REPO" --clobber
  local live
  live=$(curl -fsSL "$KANAL_URL" | jq -r .version)
  echo "Kanal $KANAL zeigt jetzt auf Version: $live"
}

if [[ $1 == "--final" ]]; then
  TAG=${2:?Tag fehlt, z. B. v3.0.0}
  [[ $(gh release view "$TAG" -R "$REPO" --json isPrerelease,isDraft -q '.isPrerelease or .isDraft') == "false" ]] \
    || fehler "$TAG ist kein veroeffentlichtes finales Release"
  gh release download "$TAG" -R "$REPO" -p latest.json -D "$TMP"
  VER=$(jq -r .version "$TMP/latest.json")
  [[ "v$VER" == "$TAG" && "$VER" != *-* ]] || fehler "latest.json von $TAG traegt Version $VER"
  kanal_setzen "$TMP/latest.json" "$(gh release view "$TAG" -R "$REPO" --json targetCommitish -q .targetCommitish)"
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
roh .github/workflows/release.yml | grep -qF "'!v3.*-beta*'" \
  || fehler "release.yml an $SHA schliesst v3-Beta-Tags nicht aus — Tag wuerde einen Release-Build ausloesen"

gh run download "$RUN" -R "$REPO" -n AeroACARS-beta-kanal -D "$TMP/kanal" \
  || fehler "Artefakt AeroACARS-beta-kanal fehlt (Lauf ohne beta=true oder aelter als 7 Tage?)"
cd "$TMP/kanal"

P="AeroACARS.3_${VER}"
DATEIEN=("${P}_x64-setup.exe" "${P}_x64-setup.exe.sig" "${P}_aarch64.dmg" "${P}_aarch64.app.tar.gz" "${P}_aarch64.app.tar.gz.sig")
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
kanal_setzen latest.json "$SHA"
echo "Fertig: $TAG veroeffentlicht, Beta-Kanal aktualisiert."
