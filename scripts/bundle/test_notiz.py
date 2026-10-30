"""python3 -I -m unittest discover -s scripts/bundle"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import notiz  # noqa: E402

FREIGABE = "Für alle freigeben (Freigabe Thomas bzw. P-9.43)"


def lauf(nr, sha, conclusion="success"):
    return {"id": 1000 + nr, "run_number": nr, "head_sha": sha, "conclusion": conclusion}


class Basis(unittest.TestCase):
    # Lage 30.10.: 47 freigegeben, 48–52 abgebrochen (Freigabe nie erteilt).
    LAEUFE = [lauf(47, "2290e620"), lauf(48, "eb022824", "cancelled"), lauf(52, "a5745692", "cancelled"), lauf(46, "767c5aee", "cancelled")]

    def jobs(self, freigegeben):
        return lambda i: [{"name": "Bauen, testen, signieren", "conclusion": "success"},
                          {"name": FREIGABE, "conclusion": "success" if i in freigegeben else "cancelled"}]

    def test_neuester_freigegebener_lauf(self):
        self.assertEqual(notiz.basis_aus_laeufen(self.LAEUFE, self.jobs({1047})), (47, "2290e620"))

    def test_erfolgreicher_lauf_ohne_freigabe_zaehlt_nicht(self):
        # Gegenprobe: Lauf-Erfolg allein reicht nicht, der Freigabe-Job muss erfolgreich sein.
        self.assertIsNone(notiz.basis_aus_laeufen(self.LAEUFE, self.jobs(set())))

    def test_eigener_lauf_wird_uebergangen(self):
        self.assertIsNone(notiz.basis_aus_laeufen([lauf(53, "713b516a")], self.jobs({1053}), eigener_lauf=1053))


class Bauen(unittest.TestCase):
    C = [{"sha": "713b516a", "titel": "feat(ueber): aktive Paket-Generation"}, {"sha": "a5745692", "titel": "fix: x"}]

    def test_kernaufgabe_aus_eingabe(self):
        n = notiz.notiz_bauen(53, "713b", "  Hilfe-Formular  ", (47, "2290"), self.C, "letzter")
        self.assertEqual((n["kernaufgabe"], n["basis_generation"], len(n["commits"])), ("Hilfe-Formular", 47, 2))

    def test_kernaufgabe_sonst_gebauter_commit(self):
        # Gegenprobe: nicht der neueste Commit der Liste (kann aelter sein), sondern der gebaute.
        self.assertEqual(notiz.notiz_bauen(53, "713b", "", (47, "2290"), self.C, "letzter")["kernaufgabe"], "letzter")

    def test_pfade_wie_ausloeser(self):
        yml = open(os.path.join(os.path.dirname(__file__), "..", "..", ".github", "workflows", "bundle.yml"), encoding="utf-8").read()
        block = yml.split("paths:", 1)[1].split("workflow_dispatch", 1)[0]
        ausloeser = [z.strip().strip("-").strip().strip("'").rstrip("*").rstrip("/") for z in block.splitlines() if z.strip().startswith("- ")]
        for p in ausloeser:
            self.assertTrue(any(p == q or p.startswith(q + "/") or (p.endswith("panel.") and p.startswith(q)) for q in notiz.PFADE), p)

    def test_ohne_basis_keine_commits_und_ehrlich_null(self):
        n = notiz.notiz_bauen(53, "713b", "", None, None, "letzter")
        self.assertEqual((n["basis_generation"], n["basis_commit"], n["commits"], n["kernaufgabe"]), (None, None, [], "letzter"))


if __name__ == "__main__":
    unittest.main()
