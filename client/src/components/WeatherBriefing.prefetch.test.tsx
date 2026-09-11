// Auto-Refresh ab Descent, ohne Button-Klick.
//
// Anlass (Discord, Michael K. 2026-09-11): "Kann mit Beginn des Descent das
// Wetter am Zielflughafen nochmal selbständig aktualisiert werden. Ohne das
// ich [...] den Button drücken muss." — `maybe_spawn_metar_fetch` (Rust)
// holt das Zielwetter serverseitig schon bei Descent/Final, unabhängig
// davon ob diese Karte offen ist (`activeFlight` wird App-weit alle 2 s
// gepollt). Diese Karte hat das nie gezeigt: Sie holte beim Mount immer
// ihr eigenes METAR, statt den vorgeholten Wert zu lesen.
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import type { MetarSnapshotDto } from "../types";

const metarGet = vi.fn();

vi.mock("../lib/ipc", () => ({
  invoke: (cmd: string, args: unknown) => metarGet(cmd, args),
}));

import { WeatherBriefing } from "./WeatherBriefing";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

afterEach(() => {
  cleanup();
  // mockReset statt mockClear: löscht auch ein per `mockResolvedValue`
  // gesetztes Standardverhalten, sonst hängt es unbeabsichtigt in den
  // nächsten Test hinein (testreihenfolge-abhängig, Codex-Befund).
  metarGet.mockReset();
});

beforeEach(() => {
  // Sicherheitsnetz (fünfte Codex-Runde): der Mount-Effekt kann für eine
  // Seite, die im jeweiligen Test nicht im Fokus steht (z. B. Abflug, wenn
  // nur die Ziel-Seite geprüft wird), unauffällig einen eigenen Fetch
  // auslösen. Ohne Standardverhalten liefert `metarGet` dann `undefined`
  // zurück — ein Testartefakt, an dem `wendeAn`/`WxCard` crasht, nicht ein
  // echter Fehler im Code. Tests, die ein bestimmtes Verhalten prüfen,
  // überschreiben das mit ihrem eigenen `mockImplementation`.
  metarGet.mockImplementation((_cmd: string, args: { icao: string }) =>
    Promise.resolve(metar(args.icao, `${args.icao} FALLBACK`, "2000-01-01T00:00:00Z")),
  );
});

function metar(
  icao: string,
  raw: string,
  time: string,
  windDirDeg = 180,
  windSpeedKt = 15,
  gustKt: number | null = 25,
  timeIsEstimated = false,
): MetarSnapshotDto {
  return {
    icao,
    raw,
    time,
    time_is_estimated: timeIsEstimated,
    wind_direction_deg: windDirDeg,
    wind_speed_kt: windSpeedKt,
    gust_kt: gustKt,
    visibility_m: null,
    temperature_c: null,
    dewpoint_c: null,
    qnh_hpa: null,
  };
}

describe("WeatherBriefing — vom Backend vorgeholtes Wetter", () => {
  it("zeigt das vorgeholte Zielwetter sofort, ohne selbst nachzufragen", async () => {
    render(
      <WeatherBriefing
        dptIcao="EDDF"
        arrIcao="EDDM"
        prefetchedDpt={metar("EDDF", "EDDF 111200Z 24010KT CAVOK", "2026-09-11T11:00:00Z", 240, 10, null)}
        prefetchedArr={metar("EDDM", "EDDM 111200Z 18015G25KT 9999 BKN030", "2026-09-11T12:00:00Z")}
      />,
    );
    await waitFor(() =>
      expect(screen.getByText(/180° \/ 15G25 kt/)).toBeTruthy(),
    );
    // Beide Seiten waren vorgehalten — kein eigener metar_get-Aufruf nötig.
    expect(metarGet).not.toHaveBeenCalled();
  });

  it("fragt selbst nach, solange das Backend noch nichts vorgehalten hat", async () => {
    metarGet.mockResolvedValue(
      metar("EDDM", "EDDM 111200Z 18015G25KT 9999 BKN030", "2026-09-11T12:00:00Z"),
    );
    render(
      <WeatherBriefing dptIcao="EDDF" arrIcao="EDDM" prefetchedDpt={null} prefetchedArr={null} />,
    );
    await waitFor(() => expect(metarGet).toHaveBeenCalled());
  });

  it("zieht ein später eintreffendes Descent-METAR live nach", async () => {
    const { rerender } = render(
      <WeatherBriefing
        dptIcao="EDDF"
        arrIcao="EDDM"
        prefetchedDpt={metar("EDDF", "EDDF 111200Z 24010KT CAVOK", "2026-09-11T11:00:00Z")}
        prefetchedArr={null}
      />,
    );
    rerender(
      <WeatherBriefing
        dptIcao="EDDF"
        arrIcao="EDDM"
        prefetchedDpt={metar("EDDF", "EDDF 111200Z 24010KT CAVOK", "2026-09-11T11:00:00Z")}
        prefetchedArr={metar("EDDM", "EDDM 111200Z 18015G25KT 9999 BKN030", "2026-09-11T12:00:00Z")}
      />,
    );
    // Beide Karten (Abflug UND Ziel) tragen jetzt denselben Wind-Wert —
    // vorher nur die Abflugkarte, die Zielkarte lud noch.
    await waitFor(() =>
      expect(screen.getAllByText(/180° \/ 15G25 kt/)).toHaveLength(2),
    );
  });

  it("lässt einen spät auflösenden eigenen Fetch keinen frischeren Prefetch überschreiben", async () => {
    // Codex-Befund 2026-09-11: ohne Generation-Zähler gewinnt, wer zuletzt
    // AUFLÖST, nicht wer zuletzt AUSGELÖST wurde. Hier: der eigene
    // Mount-Fetch für die ZIEL-Seite (weil noch nichts vorgehalten war)
    // hängt absichtlich in der Luft, während currentReakt ein frischerer
    // Prefetch-Wert eintrifft — der Fetch darf ihn beim Auflösen nicht
    // mehr überschreiben.
    //
    // Zweite Codex-Runde: der Mock löst pro `icao` unterschiedlich auf —
    // Abflug (EDDF) sofort, Ziel (EDDM) verzögert. Ein einzelner, für
    // beide Seiten geteilter Resolver hätte den JEWEILS ANDEREN Aufruf
    // nie aufgelöst und `Promise.all` im Mount-Effekt hängen lassen.
    let loeseArr: (v: MetarSnapshotDto) => void = () => {};
    metarGet.mockImplementation((_cmd: string, args: { icao: string }) => {
      if (args.icao === "EDDF") {
        return Promise.resolve(
          metar("EDDF", "EDDF 111200Z 24010KT CAVOK", "2026-09-11T11:00:00Z", 240, 10, null),
        );
      }
      return new Promise<MetarSnapshotDto>((resolve) => {
        loeseArr = resolve;
      });
    });
    const { rerender } = render(
      <WeatherBriefing dptIcao="EDDF" arrIcao="EDDM" prefetchedDpt={null} prefetchedArr={null} />,
    );
    // Abflug ist sofort da — Ziel lädt noch (eigener Fetch hängt).
    await waitFor(() => expect(screen.getByText(/240° \/ 10 kt/)).toBeTruthy());

    // Frischer Prefetch trifft ein, während der eigene Ziel-Fetch noch offen ist.
    rerender(
      <WeatherBriefing
        dptIcao="EDDF"
        arrIcao="EDDM"
        prefetchedDpt={null}
        prefetchedArr={metar("EDDM", "EDDM 111200Z 18015G25KT 9999 BKN030", "2026-09-11T12:00:00Z")}
      />,
    );
    await waitFor(() =>
      expect(screen.getByText(/180° \/ 15G25 kt/)).toBeTruthy(),
    );

    // Der alte Fetch löst jetzt erst auf, mit einem VERALTETEN Wert. In
    // `act` eingebettet, danach über den Refresh-Button (wieder aktiv =
    // kein Aufruf mehr offen) statt einer festen Tick-Zahl abgewartet
    // (Codex-Befund, zweite Runde): eine feste Anzahl `Promise.resolve()`
    // ist nicht garantiert an die volle Mikrotask-Kette gekoppelt und
    // könnte bei künftigen Änderungen wieder falsch-grün werden.
    await act(async () => {
      loeseArr(metar("EDDM", "EDDM 111000Z 09005KT CAVOK", "2026-09-11T11:00:00Z", 90, 5, null));
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Aktualisieren|Refresh/i })).not.toBeDisabled(),
    );

    // Muss weiter den Prefetch-Wert zeigen, nicht den veralteten Fetch.
    expect(screen.getByText(/180° \/ 15G25 kt/)).toBeTruthy();
    expect(screen.queryByText(/090° \/ 5 kt/)).toBeNull();
  });

  it("lässt einen früher eingetroffenen, aber älteren Prefetch keinen echt neueren eigenen Fetch verdrängen", async () => {
    // Codex-Befund, fünfte Runde (unvoreingenommener Review): die
    // GEGENRICHTUNG des obigen Tests war ungeprüft. Vorherige Fassungen
    // entschieden per Generation-Zähler — JEDER Prefetch, unabhängig von
    // seiner tatsächlichen Aktualität, überholte einen laufenden eigenen
    // Fetch. Hier: ein ÄLTERER Prefetch (11:00) trifft zuerst ein, während
    // der eigene Fetch noch läuft und danach mit einem ECHT NEUEREN Wert
    // (12:00) auflöst — der neuere Wert muss gewinnen, nicht der zuerst
    // eingetroffene.
    let loeseArr: (v: MetarSnapshotDto) => void = () => {};
    metarGet.mockImplementation((_cmd: string, args: { icao: string }) => {
      if (args.icao === "EDDF") {
        return Promise.resolve(
          metar("EDDF", "EDDF 111200Z 24010KT CAVOK", "2026-09-11T11:00:00Z", 240, 10, null),
        );
      }
      return new Promise<MetarSnapshotDto>((resolve) => {
        loeseArr = resolve;
      });
    });
    const { rerender } = render(
      <WeatherBriefing dptIcao="EDDF" arrIcao="EDDM" prefetchedDpt={null} prefetchedArr={null} />,
    );
    await waitFor(() => expect(screen.getByText(/240° \/ 10 kt/)).toBeTruthy());

    // Ein ÄLTERER Prefetch (11:00) trifft ein, während der eigene Fetch
    // noch offen ist — er wird übernommen (nichts anderes war bisher da).
    rerender(
      <WeatherBriefing
        dptIcao="EDDF"
        arrIcao="EDDM"
        prefetchedDpt={null}
        prefetchedArr={metar("EDDM", "EDDM 111000Z 09005KT CAVOK", "2026-09-11T11:00:00Z", 90, 5, null)}
      />,
    );
    await waitFor(() => expect(screen.getByText(/090° \/ 5 kt/)).toBeTruthy());

    // Der eigene Fetch löst jetzt mit einem ECHT NEUEREN Wert auf (12:00).
    // Mit Generation-Zählern wäre er hier verworfen worden, weil der
    // Prefetch (11:00) zuerst da war — das ist genau der Codex-Fund.
    await act(async () => {
      loeseArr(metar("EDDM", "EDDM 111200Z 18015G25KT 9999 BKN030", "2026-09-11T12:00:00Z"));
    });
    await waitFor(() => expect(screen.getByText(/180° \/ 15G25 kt/)).toBeTruthy());
    expect(screen.queryByText(/090° \/ 5 kt/)).toBeNull();
  });

  it("übernimmt einen zweiten Prefetch mit gleichem time aber anderem raw", async () => {
    // Codex-Befund, dritte Runde: Dedup lief über `time`+`raw` gemeinsam,
    // aber ungetestet war der Fall, der den Unterschied zu reinem `time`
    // überhaupt erst begründet — zwei echte Backend-Fetches (Descent,
    // dann Final aus `maybe_spawn_metar_fetch`) können denselben METAR-
    // Beobachtungszeitpunkt tragen, wenn die Bodenstation dazwischen
    // nichts Neues gemeldet hat, aber trotzdem inhaltlich verschieden
    // sein (hier: Wind gedreht, `raw` geändert).
    const dpt = metar("EDDF", "EDDF 111200Z 24010KT CAVOK", "2026-09-11T11:00:00Z", 240, 10, null);
    const { rerender } = render(
      <WeatherBriefing
        dptIcao="EDDF"
        arrIcao="EDDM"
        prefetchedDpt={dpt}
        prefetchedArr={metar("EDDM", "EDDM 111200Z 18015G25KT 9999 BKN030", "2026-09-11T12:00:00Z", 180, 15, 25)}
      />,
    );
    await waitFor(() => expect(screen.getByText(/180° \/ 15G25 kt/)).toBeTruthy());

    rerender(
      <WeatherBriefing
        dptIcao="EDDF"
        arrIcao="EDDM"
        prefetchedDpt={dpt}
        prefetchedArr={metar("EDDM", "EDDM 111200Z 09010KT 9999 BKN030", "2026-09-11T12:00:00Z", 90, 10, null)}
      />,
    );
    await waitFor(() => expect(screen.getByText(/090° \/ 10 kt/)).toBeTruthy());
    expect(screen.queryByText(/180° \/ 15G25 kt/)).toBeNull();
    expect(metarGet).not.toHaveBeenCalled();
  });

  it("übernimmt das METAR des neuen Ziels beim Flugwechsel, mit gleichem time aber anderem raw", async () => {
    // Codex-Befund, dritte Runde: ein METAR des NEUEN Ziels mit zufällig
    // demselben `time`-Wert wie das zuletzt gezeigte METAR des ALTEN
    // Ziels dürfte nicht als „schon angewendet" übersprungen werden —
    // beide Werte tragen absichtlich denselben `time`-Wert, `raw` und
    // Windwerte unterscheiden sich (siebte Codex-Runde korrigiert den
    // Kommentar: vorher stand hier fälschlich "demselben time+raw").
    //
    // Sechste Codex-Runde: die vorherige Fassung nutzte für alt UND neu
    // denselben Windwert — die Assertion wäre auch bei einem toten
    // `setArr`-Aufruf grün geblieben (falsch-grün). Jetzt unterschiedliche
    // Windwerte für EDDM (alt) und EDLN (neu), damit die Prüfung wirklich
    // zeigt, dass der NEUE Wert übernommen wurde, nicht nur "irgendwas".
    const gleicheZeit = "2026-09-11T12:00:00Z";
    const dpt = metar("EDDF", "EDDF 111200Z 24010KT CAVOK", "2026-09-11T11:00:00Z", 240, 10, null);
    const { rerender } = render(
      <WeatherBriefing
        dptIcao="EDDF"
        arrIcao="EDDM"
        prefetchedDpt={dpt}
        prefetchedArr={metar("EDDM", "EDDM 111200Z 18015G25KT 9999 BKN030", gleicheZeit, 180, 15, 25)}
      />,
    );
    await waitFor(() => expect(screen.getByText(/180° \/ 15G25 kt/)).toBeTruthy());

    // Neuer Flug, neues Ziel — derselbe `time`, aber ANDERER Wind, damit
    // die Prüfung unten eindeutig ist.
    rerender(
      <WeatherBriefing
        dptIcao="EDDF"
        arrIcao="EDLN"
        prefetchedDpt={dpt}
        prefetchedArr={metar("EDLN", "EDLN 111200Z 27020KT 9999 BKN030", gleicheZeit, 270, 20, null)}
      />,
    );
    await waitFor(() => expect(screen.getByText(/270° \/ 20 kt/)).toBeTruthy());
    expect(screen.queryByText(/180° \/ 15G25 kt/)).toBeNull();
  });

  it("lässt eine spät antwortende ALTE Route die NEUE nicht überschreiben", async () => {
    // Codex-Befund, sechste Runde (unvoreingenommener Review): `wendeAn`
    // übernimmt bei einer ANDEREN ICAO ungeprüft — das ist richtig für
    // „neuer Flug, neues Wetter", aber `fetchOne` selbst prüfte bisher
    // nicht, ob die ICAO, für die der Request gestartet wurde, noch die
    // AKTUELLE Route ist. Hier: eigener Fetch für EDDM (altes Ziel) hängt
    // in der Luft, während der Pilot auf EDLN wechselt (neuer Prefetch
    // kommt sofort). Löst der alte EDDM-Fetch jetzt erst auf, darf er
    // NICHT mehr geschrieben werden — er gehört zu einer Route, die es
    // nicht mehr gibt.
    let loeseAlt: (v: MetarSnapshotDto) => void = () => {};
    metarGet.mockImplementation((_cmd: string, args: { icao: string }) => {
      if (args.icao === "EDDF") {
        return Promise.resolve(
          metar("EDDF", "EDDF 111200Z 24010KT CAVOK", "2026-09-11T11:00:00Z", 240, 10, null),
        );
      }
      return new Promise<MetarSnapshotDto>((resolve) => {
        loeseAlt = resolve;
      });
    });
    const { rerender } = render(
      <WeatherBriefing dptIcao="EDDF" arrIcao="EDDM" prefetchedDpt={null} prefetchedArr={null} />,
    );
    await waitFor(() => expect(screen.getByText(/240° \/ 10 kt/)).toBeTruthy());
    // Ziel-Fetch für EDDM hängt (noch kein Prefetch, `loeseAlt` noch offen).

    // Flugwechsel: neues Ziel EDLN, sofort mit Prefetch versorgt.
    rerender(
      <WeatherBriefing
        dptIcao="EDDF"
        arrIcao="EDLN"
        prefetchedDpt={null}
        prefetchedArr={metar("EDLN", "EDLN 111200Z 27020KT 9999 BKN030", "2026-09-11T12:00:00Z", 270, 20, null)}
      />,
    );
    await waitFor(() => expect(screen.getByText(/270° \/ 20 kt/)).toBeTruthy());

    // Der alte EDDM-Fetch löst jetzt erst auf — für eine Route, die es
    // nicht mehr gibt. Darf NICHT mehr geschrieben werden.
    await act(async () => {
      loeseAlt(metar("EDDM", "EDDM 111200Z 18015G25KT 9999 BKN030", "2026-09-11T12:05:00Z", 180, 15, 25));
    });
    expect(screen.getByText(/270° \/ 20 kt/)).toBeTruthy();
    expect(screen.queryByText(/180° \/ 15G25 kt/)).toBeNull();
  });

  it("zeigt beim Routenwechsel nicht weiter das Wetter des alten Ziels, während das neue noch lädt", async () => {
    // Codex-Befund, siebte Runde: die "Regression vermeiden"-Klausel in
    // `fetchOne` (kein Rückfall auf den Lade-Spinner, solange schon ein
    // Wert da ist) prüfte bisher nur `prev.kind === "ready"`, nicht ob
    // dieser Wert überhaupt zur GERADE angefragten ICAO gehört. Beim
    // Flugwechsel ohne sofort verfügbaren Prefetch fürs neue Ziel hätte
    // die Karte so weiter das METAR des ALTEN Ziels gezeigt — unter dem
    // Label des neuen —, bis der neue Fetch durch ist (oder für immer,
    // schlägt er fehl).
    let loeseNeu: (v: MetarSnapshotDto) => void = () => {};
    metarGet.mockImplementation((_cmd: string, args: { icao: string }) => {
      if (args.icao === "EDDF") {
        return Promise.resolve(
          metar("EDDF", "EDDF 111200Z 24010KT CAVOK", "2026-09-11T11:00:00Z", 240, 10, null),
        );
      }
      return new Promise<MetarSnapshotDto>((resolve) => {
        loeseNeu = resolve;
      });
    });
    const { rerender } = render(
      <WeatherBriefing
        dptIcao="EDDF"
        arrIcao="EDDM"
        prefetchedDpt={null}
        prefetchedArr={metar("EDDM", "EDDM 111200Z 18015G25KT 9999 BKN030", "2026-09-11T12:00:00Z", 180, 15, 25)}
      />,
    );
    await waitFor(() => expect(screen.getByText(/180° \/ 15G25 kt/)).toBeTruthy());

    // Flugwechsel auf EDLN — noch kein Prefetch fürs neue Ziel da, der
    // Mount-Effekt löst einen eigenen (hier hängenden) Fetch aus.
    rerender(
      <WeatherBriefing dptIcao="EDDF" arrIcao="EDLN" prefetchedDpt={null} prefetchedArr={null} />,
    );

    // Solange der neue Fetch noch offen ist, darf das alte EDDM-Wetter
    // NICHT mehr angezeigt werden — das gehört zu einer Route, die es
    // nicht mehr gibt.
    await waitFor(() => expect(screen.queryByText(/180° \/ 15G25 kt/)).toBeNull());

    await act(async () => {
      loeseNeu(metar("EDLN", "EDLN 111200Z 27020KT 9999 BKN030", "2026-09-11T12:10:00Z", 270, 20, null));
    });
    await waitFor(() => expect(screen.getByText(/270° \/ 20 kt/)).toBeTruthy());
  });

  it("lässt einen geschätzten Zeitstempel keinen echten Beobachtungswert verdrängen", async () => {
    // Vorgemerkter Rust-Punkt vom 2026-09-11, jetzt umgesetzt: fehlt
    // NOAAs obsTime, setzt das Backend die AKTUELLE Abrufzeit ein
    // (`time_is_estimated: true`) — praktisch immer "jetzt", also fast
    // immer numerisch "neuer" als jede echte Beobachtungszeit. Ohne
    // Sonderbehandlung würde `wendeAn` einen echten, aber älter
    // datierten Bericht fälschlich verdrängen, nur weil der neue Fetch
    // zufällig auf eine Station mit fehlender obsTime traf.
    const dpt = metar("EDDF", "EDDF 111200Z 24010KT CAVOK", "2026-09-11T11:00:00Z", 240, 10, null);
    const echterWert = metar(
      "EDDM", "EDDM 111200Z 18015G25KT 9999 BKN030", "2026-09-11T12:00:00Z", 180, 15, 25, false,
    );
    const { rerender } = render(
      <WeatherBriefing dptIcao="EDDF" arrIcao="EDDM" prefetchedDpt={dpt} prefetchedArr={echterWert} />,
    );
    await waitFor(() => expect(screen.getByText(/180° \/ 15G25 kt/)).toBeTruthy());

    // Ein geschätzter Wert trifft ein — numerisch neuer (13:00 > 12:00),
    // aber als Schätzung markiert. Darf den echten nicht verdrängen.
    const geschaetzterWert = metar(
      "EDDM", "EDDM 111200Z 09005KT CAVOK", "2026-09-11T13:00:00Z", 90, 5, null, true,
    );
    rerender(
      <WeatherBriefing dptIcao="EDDF" arrIcao="EDDM" prefetchedDpt={dpt} prefetchedArr={geschaetzterWert} />,
    );
    expect(screen.getByText(/180° \/ 15G25 kt/)).toBeTruthy();
    expect(screen.queryByText(/090° \/ 5 kt/)).toBeNull();
  });
});
