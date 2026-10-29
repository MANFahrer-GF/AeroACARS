"""Tests fuer den OurAirports-Wochenlauf: python3 -I -m unittest discover scripts/ourairports"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import aktualisieren as oa  # noqa: E402

AP_KOPF = "ident,type,latitude_deg,longitude_deg\n"
RW_KOPF = '"id","airport_ref","airport_ident","length_ft","width_ft","le_ident","le_heading_degT","he_ident"\n'


def ap(zeilen):
    return AP_KOPF + "".join(f"{i},{t},{la},{lo}\n" for i, t, la, lo in zeilen)


def rw(zeilen):
    return RW_KOPF + "".join(f'"1","1","{a}","{l}","150","{le}","{h}","{he}"\n' for a, l, le, h, he in zeilen)


# 1000 Fuellplaetze, damit kleine Aenderungen unter den Anzahl-Schwellen bleiben.
FUELL = [(f"X{n:04d}", "small_airport", 10 + n / 1000, 10) for n in range(1000)]
BASIS_AP = FUELL + [("EDDF", "large_airport", 50.0333, 8.5706)]
BASIS_RW = [(f"X{n:04d}", 3000, "09", 90, "27") for n in range(1000)] + [("EDDF", 13123, "07C", 69.9, "25C")]
GSG = {"EDDF"}


class Pruefen(unittest.TestCase):
    def gruende(self, neu_ap=BASIS_AP, neu_rw=BASIS_RW):
        return oa.pruefen(ap(BASIS_AP), ap(neu_ap), rw(BASIS_RW), rw(neu_rw), GSG)

    def test_unveraendert_kein_halt(self):
        self.assertEqual(self.gruende(), [])

    def test_kleine_aenderung_ausserhalb_gsg_kein_halt(self):
        neu = BASIS_AP + [("NEU1", "heliport", 1, 1)]
        verschoben = [(i, t, la + (0.05 if i == "X0001" else 0), lo) for i, t, la, lo in neu]
        self.assertEqual(self.gruende(neu_ap=verschoben), [])

    def test_gsg_flughafen_entfernt(self):
        self.assertTrue(any("EDDF entfernt" in g for g in self.gruende(neu_ap=FUELL)))

    def test_gsg_flughafen_typ_geaendert(self):
        neu = FUELL + [("EDDF", "medium_airport", 50.0333, 8.5706)]
        self.assertTrue(any("EDDF: Typ" in g for g in self.gruende(neu_ap=neu)))

    def test_gsg_flughafen_verschoben(self):
        neu = FUELL + [("EDDF", "large_airport", 50.0533, 8.5706)]
        self.assertTrue(any("EDDF um >" in g for g in self.gruende(neu_ap=neu)))

    def test_gsg_bahn_laenge_kurs_und_umbenennung(self):
        laenger = BASIS_RW[:-1] + [("EDDF", 15000, "07C", 69.9, "25C")]
        self.assertTrue(any("Laenge" in g for g in self.gruende(neu_rw=laenger)))
        gedreht = BASIS_RW[:-1] + [("EDDF", 13123, "07C", 76.0, "25C")]
        self.assertTrue(any("Kurs" in g for g in self.gruende(neu_rw=gedreht)))
        umbenannt = BASIS_RW[:-1] + [("EDDF", 13123, "08C", 69.9, "26C")]
        self.assertTrue(any("entfernt oder umbenannt" in g for g in self.gruende(neu_rw=umbenannt)))

    def test_kurs_ueber_null_grad_ist_kleine_aenderung(self):
        alt = BASIS_RW[:-1] + [("EDDF", 13123, "36", 359.0, "18")]
        neu = BASIS_RW[:-1] + [("EDDF", 13123, "36", 1.0, "18")]
        self.assertEqual(oa.pruefen(ap(BASIS_AP), ap(BASIS_AP), rw(alt), rw(neu), GSG), [])

    def test_massenloeschung_und_zu_viele_verschiebungen(self):
        self.assertTrue(any("Flughaefen:" in g for g in self.gruende(neu_ap=BASIS_AP[:900])))
        viele = [(i, t, la + 0.05, lo) if i != "EDDF" else (i, t, la, lo) for i, t, la, lo in BASIS_AP[:150]] + BASIS_AP[150:]
        self.assertTrue(any("verschoben (hoechstens" in g for g in self.gruende(neu_ap=viele)))

    def test_kopfzeile_geaendert(self):
        self.assertTrue(any("Kopfzeile" in g for g in oa.pruefen(ap(BASIS_AP), "x\n", rw(BASIS_RW), rw(BASIS_RW), GSG)))

    def test_rohe_kopfzeile_airports(self):
        basis = (ap(BASIS_AP), ap(BASIS_AP), rw(BASIS_RW), rw(BASIS_RW), GSG)
        self.assertEqual(oa.pruefen(*basis, oa.AIRPORTS_KOPF_ROH + "\r"), [])
        neu = oa.AIRPORTS_KOPF_ROH + ',"neue_spalte"'
        self.assertTrue(any("roh" in g for g in oa.pruefen(*basis, neu)))

    def test_jede_anzahl_schwelle(self):
        # Flughaefen: +2 % haelt nicht, darueber Halt (1001 Plaetze).
        plus = [(f"N{n:03d}", "small_airport", 1, 1) for n in range(21)]
        self.assertFalse(any("Flughaefen:" in g for g in self.gruende(neu_ap=BASIS_AP + plus[:20])))
        self.assertTrue(any("Flughaefen:" in g for g in self.gruende(neu_ap=BASIS_AP + plus)))
        # Bahnen: -0,5 % haelt nicht, darueber Halt; +2 % ebenso.
        self.assertFalse(any("Bahnen:" in g for g in self.gruende(neu_rw=BASIS_RW[5:])))
        self.assertTrue(any("Bahnen:" in g for g in self.gruende(neu_rw=BASIS_RW[6:])))
        mehr = [(f"N{n:03d}", 3000, "09", 90, "27") for n in range(21)]
        self.assertFalse(any("Bahnen:" in g for g in self.gruende(neu_rw=BASIS_RW + mehr[:20])))
        self.assertTrue(any("Bahnen:" in g for g in self.gruende(neu_rw=BASIS_RW + mehr)))
        # Verschiebungen: 100 haelt nicht, 101 Halt.
        def schiebe(n):
            return [(i, t, la + 0.05, lo) if k < n else (i, t, la, lo) for k, (i, t, la, lo) in enumerate(BASIS_AP)]
        self.assertFalse(any("verschoben (hoechstens" in g for g in self.gruende(neu_ap=schiebe(100))))
        self.assertTrue(any("verschoben (hoechstens" in g for g in self.gruende(neu_ap=schiebe(101))))


class Lauf(unittest.TestCase):
    def test_stand_wird_nur_bei_neuem_stand_geschrieben(self):
        import tempfile
        with tempfile.TemporaryDirectory() as d:
            ziel, roh = os.path.join(d, "ziel"), os.path.join(d, "roh")
            os.mkdir(ziel)
            os.mkdir(roh)
            zeile = '"1","EDDF","large_airport","Frankfurt","50.0333","8.5706"' + ",x" * 13 + "\n"
            with open(os.path.join(roh, "airports.csv"), "w") as f:
                f.write(oa.AIRPORTS_KOPF_ROH + "\n" + zeile)
            with open(os.path.join(roh, "runways.csv"), "w") as f:
                f.write(rw([("EDDF", 13123, "07C", 69.9, "25C")]))
            with open(os.path.join(ziel, "airports.csv"), "w") as f:
                f.write(oa.airports_umsetzen(oa.AIRPORTS_KOPF_ROH + "\n" + zeile))
            with open(os.path.join(ziel, "runways.csv"), "w") as f:
                f.write(rw([("EDDF", 13123, "07C", 69.9, "25C")]))
            stand = os.path.join(d, "stand.txt")
            args = ["--ziel", ziel, "--roh-ordner", roh, "--commit", "abc123", "--stand", stand]
            self.assertEqual(oa.main(args), 0)
            self.assertFalse(os.path.exists(stand))  # kein Unterschied: nichts geschrieben
            with open(os.path.join(roh, "runways.csv"), "w") as f:
                f.write(rw([("EDDF", 13200, "07C", 69.9, "25C")]))
            self.assertEqual(oa.main(args), 0)
            with open(stand) as f:
                self.assertEqual(f.read(), "abc123\n")


class Umsetzen(unittest.TestCase):
    def test_vier_spalten_werte_unveraendert(self):
        roh = '"id","ident","type","name","latitude_deg","longitude_deg"\n"1","EDDF","large_airport","Frankfurt, Main","50.0333","8.5706"\n'
        self.assertEqual(oa.airports_umsetzen(roh), AP_KOPF + "EDDF,large_airport,50.0333,8.5706\n")

    def test_fehlende_spalte(self):
        with self.assertRaises(ValueError):
            oa.airports_umsetzen('"id","ident"\n"1","EDDF"\n')


if __name__ == "__main__":
    unittest.main()
