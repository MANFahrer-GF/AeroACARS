#!/usr/bin/env bash
# apt nur über Spiegel, deren Name der Runner gerade auflöst (11.11.2026: auf den
# GitHub-Linux-Runnern lösten sich *.ubuntu.com stundenlang nicht auf, Release v3.0.1
# und Paketlauf 43039706668 scheiterten daran). Reihenfolge = Vorrang; der Azure-
# Spiegel bleibt im Normalbetrieb vorn. Ersetzt /etc/apt/apt-mirrors.txt, die
# ubuntu.sources des Runner-Images liest Archiv und Sicherheitsupdates von dort.
set -euo pipefail
kandidaten=(
  http://azure.archive.ubuntu.com/ubuntu/
  http://mirrors.edge.kernel.org/ubuntu/
  https://ftp.halifax.rwth-aachen.de/ubuntu/
  http://archive.ubuntu.com/ubuntu/
)
liste=""; n=0
for u in "${kandidaten[@]}"; do
  host=${u#*://}; host=${host%%/*}
  if getent hosts "$host" >/dev/null; then
    n=$((n+1)); liste+="$u	priority:$n"$'\n'
  else
    echo "::warning::apt-Spiegel $host ist nicht auflösbar, wird übersprungen"
  fi
done
[ "$n" -gt 0 ] || { echo "::error::kein apt-Spiegel auflösbar"; exit 1; }
printf '%s' "$liste" | sudo tee /etc/apt/apt-mirrors.txt
grep -q 'mirror+file:/etc/apt/apt-mirrors.txt' /etc/apt/sources.list.d/ubuntu.sources \
  || { echo "::error::ubuntu.sources liest nicht aus apt-mirrors.txt"; exit 1; }
