"""python3 -I -m unittest discover -s scripts/bundle"""
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import freigabeweg as fw  # noqa: E402


def m(**dateien):
    return {"files": [{"path": p.replace("__", "/"), "sha256": h} for p, h in dateien.items()]}


class Freigabeweg(unittest.TestCase):
    def lauf(self, neu, alt):
        with tempfile.TemporaryDirectory() as d:
            pn, pa = os.path.join(d, "neu.json"), os.path.join(d, "alt.json")
            json.dump(neu, open(pn, "w"))
            if alt is not None:
                json.dump(alt, open(pa, "w"))
            return fw.main(["", pn, pa])

    def test_nur_ourairports_geaendert_ohne_freigabe(self):
        self.assertEqual(self.lauf(m(ourairports__runways_csv="b", texte__de="t"), m(ourairports__runways_csv="a", texte__de="t")), 0)

    def test_anderes_geaendert_braucht_freigabe(self):
        self.assertEqual(self.lauf(m(ourairports__runways_csv="b", texte__de="neu"), m(ourairports__runways_csv="a", texte__de="t")), 1)

    def test_datei_dazu_oder_weg_braucht_freigabe(self):
        self.assertEqual(self.lauf(m(ourairports__x="a", modul="w"), m(ourairports__x="a")), 1)
        self.assertEqual(self.lauf(m(ourairports__x="a"), m(ourairports__x="a", modul="w")), 1)

    def test_kein_veroeffentlichter_stand_braucht_freigabe(self):
        self.assertEqual(self.lauf(m(ourairports__x="a"), None), 1)


if __name__ == "__main__":
    unittest.main()
