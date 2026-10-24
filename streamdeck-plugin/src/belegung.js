// Die Drehregler des StreamDeck + und die Belegung der mitgelieferten Profile.

export const PRAEFIX = "eu.german-sky-group.aeroacars";

/** Aktions-UUID: Elgato erlaubt nur a-z, 0-9, Bindestrich und Punkt. */
export const uuid = (id) => `${PRAEFIX}.${id.replace(/_/g, "-")}`;

export const REGLER = {
  regler_ansage: {
    name: "Regler Ansagen",
    tip: "Drehen: Lautstärke der Ansagen. Drücken: Ansagen stumm und wieder an.",
    icon: "spk",
    ausloeser: { Rotate: "Lautstärke", Push: "Stumm / Ton an" },
  },
  regler_musik: {
    name: "Regler Musik",
    tip: "Drehen: Lautstärke der Boarding-Musik. Kurz drücken: Musik stumm/laut. Halten: Boarding-Musik an/aus.",
    icon: "music",
    ausloeser: { Rotate: "Lautstärke", Push: "Kurz = stumm/laut, halten = an/aus" },
  },
  regler_info: {
    name: "Regler Flugdaten",
    tip: "Drehen blättert durch Höhe, Geschwindigkeit, Wind, ETA, Sprit und Flugzeit. Drücken: AeroACARS nach vorn.",
    icon: "clock",
    ausloeser: { Rotate: "Seite wechseln", Push: "AeroACARS öffnen" },
  },
  regler_wetter: {
    name: "Regler Wetter",
    tip: "Drehen wechselt zwischen Wetter am Start und am Ziel. Drücken: METAR neu laden.",
    icon: "wind",
    ausloeser: { Rotate: "Start / Ziel", Push: "METAR laden" },
  },
};

// Gerätetyp laut Elgato-SDK (DeviceType), Modellkennung wie in Exporten der
// StreamDeck-Software. XL und + sind aus echten Exporten abgelesen; die des
// 15er-Decks (MK.2) ist NICHT belegt und muss am Gerät geprüft werden.
export const PROFILE = [
  {
    datei: "AeroACARS 15",
    name: "AeroACARS",
    geraetetyp: 0,
    modell: "20GBA9901",
    spalten: 5,
    reihen: 3,
  },
  {
    datei: "AeroACARS XL",
    name: "AeroACARS",
    geraetetyp: 2,
    modell: "20GAT9901",
    spalten: 8,
    reihen: 4,
  },
  {
    datei: "AeroACARS Plus",
    name: "AeroACARS",
    geraetetyp: 7,
    modell: "20GBD9901",
    spalten: 4,
    reihen: 2,
    regler: ["regler_ansage", "regler_musik", "regler_info", "regler_wetter"],
  },
];

/** Name des mitgelieferten Profils je Gerätetyp, wie im Manifest (`profiles/…`). */
export const profilFuer = (geraetetyp) => {
  const p = PROFILE.find((x) => x.geraetetyp === geraetetyp);
  return p ? `profiles/${p.datei}` : null;
};
