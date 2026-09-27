#!/bin/bash
# Startet das AeroACARS-Messwerkzeug (Doppelklick im Finder).
cd "$(dirname "$0")" || exit 1
if ! command -v python3 >/dev/null 2>&1; then
  echo "Python 3 fehlt. macOS bietet gleich an, die „Befehlszeilen-Entwicklerwerkzeuge“ zu installieren — bitte installieren und danach erneut starten."
  xcode-select --install 2>/dev/null
  read -r -p "Enter zum Beenden … "
  exit 1
fi
python3 xplane_messung.py
