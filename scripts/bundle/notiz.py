#!/usr/bin/env python3
"""Begleitnotiz eines Laufzeit-Pakets (AeroACARS 3.0, Reiter „Pakete & Module“).

Thomas 30.10.2026: „Was ist in 53 alles drin? Das muss da auch stehen, was man
da ändert — was die Kernaufgabe von 53 war.“

Schreibt notiz.json NEBEN das Paket (nicht ins signierte Manifest, Clients
lesen sie nie):
  {generation, repo_commit, basis_generation, basis_commit, kernaufgabe,
   commits: [{sha, titel}]}
commits = git log <Commit der zuletzt freigegebenen Generation>..HEAD auf den
Paket-Pfaden. Die freigegebene Generation kommt aus der GitHub-API: der
neueste Lauf dieses Workflows, dessen Job „Für alle freigeben“ erfolgreich
war (Laufnummer = Generation, head_sha = repo_commit). live gibt den aktiven
Stand nur angemeldet heraus, daher nicht von dort.

Aufruf (CI):
  python3 -I notiz.py --generation G --repo-commit SHA --ausgabe paket/notiz.json
      [--kernaufgabe TEXT]  (leer → Überschrift des neuesten Commits)
      [--basis GEN:SHA]     (statt GitHub-API, für Tests und Nachträge)
Umgebung für die API: GITHUB_TOKEN, GITHUB_REPOSITORY, GITHUB_RUN_ID.
"""
import argparse
import json
import os
import subprocess
import sys
import urllib.request

PFADE = ["bundle", "client/src", "client/src-tauri/befehle.json"]
FREIGABE_JOB = "Für alle freigeben"
GRENZE = 200


def api(pfad: str):
    req = urllib.request.Request(
        "https://api.github.com/repos/" + os.environ["GITHUB_REPOSITORY"] + pfad,
        headers={"Authorization": "Bearer " + os.environ.get("GITHUB_TOKEN", ""), "Accept": "application/vnd.github+json"},
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def basis_aus_laeufen(laeufe, jobs_von, eigener_lauf=None):
    """(generation, sha) des neuesten Laufs mit erfolgreichem Freigabe-Job, sonst None."""
    for lauf in sorted(laeufe, key=lambda l: l["run_number"], reverse=True):
        if str(lauf["id"]) == str(eigener_lauf) or lauf.get("conclusion") != "success":
            continue
        if any(j["name"].startswith(FREIGABE_JOB) and j.get("conclusion") == "success" for j in jobs_von(lauf["id"])):
            return lauf["run_number"], lauf["head_sha"]
    return None


def basis_von_github():
    try:
        laeufe = []
        for seite in (1, 2, 3):
            teil = api(f"/actions/workflows/bundle.yml/runs?status=completed&per_page=100&page={seite}")["workflow_runs"]
            laeufe += teil
            if len(teil) < 100:
                break
        return basis_aus_laeufen(laeufe, lambda i: api(f"/actions/runs/{i}/jobs?per_page=100")["jobs"], os.environ.get("GITHUB_RUN_ID"))
    except Exception as e:  # Notiz ist Beiwerk: lieber ohne Basis als ohne Paket.
        print(f"Basis nicht ermittelbar: {e}", file=sys.stderr)
        return None


def git(*args) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def commits_seit(basis_sha, ziel: str):
    """[{sha, titel}] neueste zuerst; basis_sha None oder unbekannt → None."""
    if not basis_sha or subprocess.run(["git", "cat-file", "-e", basis_sha + "^{commit}"], capture_output=True).returncode != 0:
        return None
    zeilen = git("log", f"--max-count={GRENZE}", "--format=%h%x09%s", f"{basis_sha}..{ziel}", "--", *PFADE).splitlines()
    return [{"sha": z.split("\t", 1)[0], "titel": z.split("\t", 1)[1]} for z in zeilen if "\t" in z]


def notiz_bauen(generation, repo_commit, kernaufgabe, basis, commits, letzter_titel):
    return {
        "generation": generation,
        "repo_commit": repo_commit,
        "basis_generation": basis[0] if basis and commits is not None else None,
        "basis_commit": basis[1] if basis and commits is not None else None,
        "kernaufgabe": (kernaufgabe or "").strip() or (commits[0]["titel"] if commits else letzter_titel),
        "commits": commits or [],
    }


def main(argv=None) -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--generation", type=int, required=True)
    p.add_argument("--repo-commit", required=True)
    p.add_argument("--ausgabe", required=True)
    p.add_argument("--kernaufgabe", default="")
    p.add_argument("--basis", default="")
    a = p.parse_args(argv)
    if a.basis:
        g, _, sha = a.basis.partition(":")
        basis = (int(g), sha)
    else:
        basis = basis_von_github()
    commits = commits_seit(basis[1] if basis else None, a.repo_commit)
    letzter = git("log", "-1", "--format=%s", a.repo_commit).strip()
    n = notiz_bauen(a.generation, a.repo_commit, a.kernaufgabe, basis, commits, letzter)
    with open(a.ausgabe, "w", encoding="utf-8") as f:
        json.dump(n, f, ensure_ascii=False, indent=1)
        f.write("\n")
    print(f"notiz.json: Gen. {n['generation']}, Basis {n['basis_generation']}, {len(n['commits'])} Commits, Kernaufgabe: {n['kernaufgabe']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
