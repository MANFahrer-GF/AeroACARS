// Die Drehregler des StreamDeck + und die Belegung der mitgelieferten Profile.

export const PRAEFIX = "eu.german-sky-group.aeroacars";

/** Aktions-UUID: Elgato erlaubt nur a-z, 0-9, Bindestrich und Punkt. */
export const uuid = (id) => `${PRAEFIX}.${id.replace(/_/g, "-")}`;

export const REGLER = {
  regler_ansage: {
    name: "Regler Ansagen",
    tip: "Drehen: Lautstärke der Ansagen. Halten: Kabine stumm. Kurz drücken: Ton wieder an.",
    icon: "spk",
    ausloeser: { Rotate: "Lautstärke", Push: "Halten = stumm, kurz = Ton an" },
  },
  regler_musik: {
    name: "Regler Musik",
    tip: "Drehen: Lautstärke der Boarding-Musik. Kurz drücken: Musik stumm/laut. Halten: Boarding-Musik an/aus.",
    icon: "music",
    ausloeser: { Rotate: "Lautstärke", Push: "Kurz = stumm/laut, halten = an/aus" },
  },
  regler_info: {
    name: "Regler Flugdaten",
    tip: "Drehen blättert durch Blockzeit, Flugzeit, Sprit, Gewicht, Geschwindigkeit und Strecke.",
    icon: "clock",
    ausloeser: { Rotate: "Seite wechseln" },
  },
  regler_wetter: {
    name: "Regler Wetter",
    tip: "Drehen wechselt zwischen Wetter am Start, Wetter am Ziel und Landebahn.",
    icon: "wind",
    ausloeser: { Rotate: "Seite wechseln" },
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
    tasten: [
      ["phase", "flug", "block", "sprit", "wetter_ziel"],
      ["kabine_stumm", "ansage_leiser", "ansage_pegel", "ansage_lauter", "boarding_musik"],
      ["musik_leiser", "musik_pegel", "musik_lauter", "landung", "hinweis"],
    ],
  },
  {
    datei: "AeroACARS XL",
    name: "AeroACARS",
    geraetetyp: 2,
    modell: "20GAT9901",
    tasten: [
      ["phase", "flug", "gates", "block", "luft", "sprit", "gewicht", "hinweis"],
      ["wetter_start", "wetter_ziel", "bahn", "ausweich", "landung", "durchstarten", "verbindung", "hoppie"],
      ["kabine_stumm", "ansage_leiser", "ansage_pegel", "ansage_lauter", "boarding_musik", "musik_leiser", "musik_pegel", "musik_lauter"],
      ["utc", "tempo", "strecke", "ldw", "ansage", "bewertung", "cdm", null],
    ],
  },
  {
    datei: "AeroACARS Plus",
    name: "AeroACARS",
    geraetetyp: 7,
    modell: "20GBD9901",
    tasten: [
      ["phase", "flug", "sprit", "hinweis"],
      ["wetter_ziel", "landung", "hoppie", "verbindung"],
    ],
    regler: ["regler_ansage", "regler_musik", "regler_info", "regler_wetter"],
  },
];
