"""python3 -I -m unittest discover -s scripts/bundle"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hinweis  # noqa: E402

DE = '{"paket": {"neu_text": "%s"}}'


class Sichtbar(unittest.TestCase):
    def test_oberflaeche_und_texte(self):
        for p in ["client/src/components/MeineAnfragen.tsx", "client/src/components/hilfeSeite.css",
                  "bundle/texte/de.json", "bundle/tabellen/vref.json", "client/index.html"]:
            self.assertTrue(hinweis.sichtbar(p), p)

    def test_interna_nicht(self):
        for p in ["client/src/components/MeineAnfragen.test.tsx", "client/src/dev/chatEntry.tsx",
                  "client/src/lib/__tests__/x.ts", "client/src-tauri/src/lib.rs", ".github/workflows/bundle.yml",
                  "scripts/bundle/notiz.py", "bundle/ourairports/airports.json", "docs/x.md"]:
            self.assertFalse(hinweis.sichtbar(p), p)


class Pruefen(unittest.TestCase):
    def test_meine_anfragen_ohne_text_bricht_ab(self):
        # Genau der Fall vom 30.10.: Oberflaeche geaendert, neu_text stehen geblieben.
        ok, grund = hinweis.pruefen(["client/src/components/MeineAnfragen.tsx", "bundle/texte/de.json"],
                                    "iPad-Chat", "iPad-Chat", ["feat(3.0): Meine Anfragen kurz halten"])
        self.assertFalse(ok)
        self.assertIn("MeineAnfragen.tsx", grund)

    def test_mit_neuem_text_ok(self):
        self.assertTrue(hinweis.pruefen(["client/src/components/MeineAnfragen.tsx"], "iPad-Chat", "Meine Anfragen", [])[0])

    def test_leerer_text_zaehlt_nicht_als_neu(self):
        self.assertFalse(hinweis.pruefen(["client/src/App.tsx"], "iPad-Chat", "", [])[0])

    def test_ausnahme_bewusst(self):
        self.assertTrue(hinweis.pruefen(["client/src/App.tsx"], "a", "a", ["refactor: x\n\n[kein-hinweis]"])[0])

    def test_nur_ci_ok(self):
        # Generation 58: nur bundle.yml geaendert — nichts fuer Piloten.
        self.assertTrue(hinweis.pruefen([".github/workflows/bundle.yml"], "a", "a", [])[0])

    def test_neu_text_lesen(self):
        self.assertEqual(hinweis.neu_text(DE % " Hallo "), "Hallo")
        self.assertEqual(hinweis.neu_text("kaputt"), "")


if __name__ == "__main__":
    unittest.main()
