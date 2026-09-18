#!/usr/bin/env node
// Der Korpus-Lauf — die Rechnung gegen die echten Flüge der Datenbank.
//
// # Warum dieses Skript das wichtigste der Prüfliste ist
//
// Alle anderen Tests füttern erfundene Eingaben. Das reicht für Grenzfälle,
// aber nicht für die eine Frage, die vor dem Ausliefern zählt: **Was ändert
// sich am echten Bestand, und ist die Änderung plausibel?**
//
// Zweimal hat genau dieser Lauf Fehler gefunden, die keine Codeprüfung
// gesehen hat:
//
//   · Der Referenzflug des Releases bekam gar keine Phasen — und mit ihm
//     ein Drittel des Bestands, alle vier A380 darunter.
//   · Die Warnfarbe hätte 82 % aller Flüge getroffen.
//
// # Der Wächter gegen den gefährlichsten Rückfall
//
// Seit v1.7.36 wird der Plan-Anteil am gemessenen ORT bestimmt. Geht diese
// Ortsbestimmung kaputt, bleiben die PROZENTWERTE plausibel — sie werden ja
// am selben (falschen) Ort abgelesen. Verraten würde es nur die
// Anflugstrecke: Schnappt der Messpunkt schon bei einer Zwischenabsenkung
// im Reiseflug zu, steht dort 800 statt 182 NM.
//
// Deshalb prüft dieses Skript die STRECKEN, nicht nur die Prozente.
//
// Aufruf:  node scripts/pruef-korpus.mjs
//          node scripts/pruef-korpus.mjs --daten <verzeichnis>

import { existsSync, readFileSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { homedir, tmpdir } from "node:os";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HIER = dirname(fileURLToPath(import.meta.url));

/** Wo die Messdaten liegen. */
const ARG = process.argv.indexOf("--daten");
// Flugspuren von Piloten — deshalb ausserhalb des oeffentlichen Repos.
const DATEN =
  ARG > -1 && process.argv[ARG + 1]
    ? resolve(process.argv[ARG + 1])
    : resolve(homedir(), "Claude", "aeroacars-korpus", "sprit-2026-09-18");
const ERGEBNIS = resolve(tmpdir(), `sprit-korpus-${process.pid}.tsv`);

/**
 * Die Schwellen.
 *
 * Sie stammen aus der Messung vom 18.09.2026 gegen 43 Flüge mit Track und
 * Navlog. Jede hat einen Grund — eine Schwelle ohne Begründung ist eine
 * Zahl, die beim ersten roten Lauf aufgeweicht wird.
 */
const SCHWELLEN = {
  // Vor der Ortsbestimmung bekamen 39 von 43 einen Messpunkt; die vier
  // übrigen waren zwei abgebrochene Flüge und zwei, die den Durchgang nie
  // live zeigten. Mit der Projektion sollten es mehr sein, nie weniger.
  mit_messpunkt_mindestens: 39,
  // DLH 370 ist der Referenzflug des Releases. Seine Zahlen stehen in den
  // Release-Notes; wandern sie, muss der Text mitwandern.
  dlh370_bis_sinkflug: { soll: -3.0, toleranz: 1.5 },
  dlh370_anflug: { soll: 175, toleranz: 20 },
  // Über 400 % ist keine Auswertung mehr, sondern ein Rechenfehler.
  anflug_pct_max: 400,
  // **Der Wächter gegen den Reiseflug-Fehler.** Die geflogene Anflugstrecke
  // darf die geplante übertreffen (Radarführung), aber nicht um ein
  // Vielfaches. Steht dort das Dreifache, hat der Messpunkt zu früh
  // zugeschnappt und die Phase enthält Reiseflug.
  strecke_verhaeltnis_max: 3.0,
  // Die beiden Phasen sind zwei Teile derselben Strecke. Ihre Plan-Anteile
  // müssen den Plan-Trip ergeben, sonst driftet der Schnitt.
  phasensumme_abweichung_pct: 1.0,
};

function lies(name) {
  const p = resolve(DATEN, name);
  if (!existsSync(p)) return null;
  return readFileSync(p, "utf-8");
}

function main() {
  if (!existsSync(DATEN)) {
    console.error(`FEHLER: Die Messdaten fehlen unter ${DATEN}.`);
    console.error("Ohne sie ist dieser Lauf wertlos — kein stilles Grün.");
    console.error("Mit --daten <verzeichnis> auf einen anderen Ort zeigen.");
    process.exit(1);
  }

  // Den AUSGELIEFERTEN Code laufen lassen — `sprit_tick`,
  // `sprit_plan_am_ort`, `sprit_auswertung_aus` — ueber die echten Spuren.
  // Bis zur QS vom 18.09.2026 las dieses Skript stattdessen eine liegende
  // Datei aus einer Python-Nachbildung und war bei jeder Codeaenderung
  // gruen (QS-Befund F4).
  rmSync(ERGEBNIS, { force: true });
  const lauf = spawnSync(
    "cargo",
    ["test", "-p", "aeroacars-app", "--lib", "sprit_korpus", "--", "--ignored", "--nocapture"],
    {
      cwd: resolve(HIER, "..", "client", "src-tauri"),
      env: { ...process.env, SPRIT_KORPUS: DATEN, SPRIT_KORPUS_AUS: ERGEBNIS },
      encoding: "utf-8",
    },
  );
  if (lauf.status !== 0 || !existsSync(ERGEBNIS)) {
    console.error("FEHLER: Der Korpus-Test im Code lief nicht durch.");
    console.error(`${lauf.stdout ?? ""}${lauf.stderr ?? ""}`.split("\n").slice(-25).join("\n"));
    process.exit(1);
  }
  const roh = readFileSync(ERGEBNIS, "utf-8");

  const zeilen = roh
    .trim()
    .split("\n")
    .filter((z) => z && !z.startsWith("#"));
  const kopf = zeilen.shift()?.split("\t") ?? [];
  const spalte = (name) => {
    const i = kopf.indexOf(name);
    if (i < 0) {
      console.error(`FEHLER: Spalte „${name}" fehlt in ergebnis.tsv.`);
      console.error(`Vorhanden: ${kopf.join(", ")}`);
      process.exit(1);
    }
    return i;
  };

  const iId = spalte("flug");
  const iStatus = spalte("status");
  const iBis = spalte("bis_sinkflug_pct");
  const iAn = spalte("anflug_pct");
  const iStrecke = spalte("strecke_anflug_nm");
  const iPlanStrecke = spalte("plan_strecke_anflug_nm");
  const iPlanBis = spalte("plan_bis_kg");
  const iPlanAn = spalte("plan_anflug_kg");
  const iPlanTrip = spalte("plan_trip_kg");

  const fluege = zeilen.map((z) => z.split("\t"));
  if (fluege.length === 0) {
    console.error("FEHLER: ergebnis.tsv enthält keine Flüge.");
    process.exit(1);
  }

  const fehler = [];
  const mitPunkt = fluege.filter((f) => f[iStatus] === "ok");

  // 1. Wie viele Flüge bekommen überhaupt eine Auswertung?
  if (mitPunkt.length < SCHWELLEN.mit_messpunkt_mindestens) {
    fehler.push(
      `Nur ${mitPunkt.length} von ${fluege.length} Flügen haben einen Messpunkt ` +
        `(mindestens ${SCHWELLEN.mit_messpunkt_mindestens} erwartet). ` +
        `Die Ortsbestimmung verliert Flüge, statt welche zu gewinnen.`,
    );
  }

  // 2. Der Referenzflug des Releases.
  const dlh = fluege.find((f) => f[iId] === "1BWKw0L9wa74zWX4");
  if (!dlh) {
    fehler.push("DLH 370 (1BWKw0L9wa74zWX4) fehlt im Korpus.");
  } else if (dlh[iStatus] !== "ok") {
    fehler.push(
      `DLH 370 bekommt keine Phasen (Status „${dlh[iStatus]}"). ` +
        `Genau das war der Befund vom 18.09. — der Flug, mit dem sich das ` +
        `Release erklärt, zeigte in der Oberfläche zweimal einen Strich.`,
    );
  } else {
    for (const [feld, i, s] of [
      ["bis Sinkflug", iBis, SCHWELLEN.dlh370_bis_sinkflug],
      ["Anflug", iAn, SCHWELLEN.dlh370_anflug],
    ]) {
      const ist = Number(dlh[i]);
      if (!Number.isFinite(ist) || Math.abs(ist - s.soll) > s.toleranz) {
        fehler.push(
          `DLH 370 „${feld}": ${ist} % statt ${s.soll} ± ${s.toleranz}. ` +
            `Diese Zahl steht in den Release-Notes — wandert sie, muss der ` +
            `Text mitwandern.`,
        );
      }
    }
  }

  // 3. Unsinnige Prozentwerte.
  for (const f of mitPunkt) {
    const pct = Number(f[iAn]);
    if (Number.isFinite(pct) && Math.abs(pct) > SCHWELLEN.anflug_pct_max) {
      fehler.push(`${f[iId]}: Anflug ${pct} % — über ${SCHWELLEN.anflug_pct_max} %.`);
    }
  }

  // 4. DER WÄCHTER: Strecke gegen Plan-Strecke.
  for (const f of mitPunkt) {
    const ist = Number(f[iStrecke]);
    const plan = Number(f[iPlanStrecke]);
    if (!Number.isFinite(ist) || !Number.isFinite(plan) || plan <= 0) continue;
    const v = ist / plan;
    if (v > SCHWELLEN.strecke_verhaeltnis_max) {
      fehler.push(
        `${f[iId]}: Anflugstrecke ${ist} NM gegen ${plan} NM Plan (${v.toFixed(1)}-fach). ` +
          `Der Messpunkt ist zu früh zugeschnappt — die Phase enthält Reiseflug. ` +
          `Die Prozentwerte sehen dabei plausibel aus; nur diese Zahl verrät es.`,
      );
    }
  }

  // 5. Die beiden Phasen sind zwei Teile derselben Strecke: Ihre
  //    Plan-Anteile muessen den Plan-Trip ergeben. Die Schwelle war bis zur
  //    QS vom 18.09.2026 definiert, aber nie geprueft (Befund F4).
  let summenGeprueft = 0;
  for (const f of mitPunkt) {
    const bis = Number(f[iPlanBis]);
    const an = Number(f[iPlanAn]);
    const trip = Number(f[iPlanTrip]);
    if (![bis, an, trip].every((v) => Number.isFinite(v) && v > 0)) continue;
    summenGeprueft += 1;
    const abw = (Math.abs(bis + an - trip) / trip) * 100;
    if (abw > SCHWELLEN.phasensumme_abweichung_pct) {
      fehler.push(
        `${f[iId]}: Plan-Anteile ${bis} + ${an} = ${bis + an} kg gegen Trip ${trip} kg ` +
          `(${abw.toFixed(1)} %). Der Schnitt zwischen den Phasen driftet.`,
      );
    }
  }
  if (summenGeprueft === 0) {
    fehler.push("Phasensumme: kein einziger Flug mit beiden Plan-Anteilen — nichts geprueft.");
  }

  // Grundgesamtheit — ohne sie wäre ein leeres Ergebnis „alles gut".
  if (mitPunkt.length === 0) {
    console.error("FEHLER: Kein einziger Flug mit Messpunkt. Nichts geprüft.");
    process.exit(1);
  }

  // Echter Median (bei gerader Anzahl der Mittelwert der beiden mittleren) —
  // dieselbe Definition wie `statistics.median`, sonst nennen Skript und
  // Spec verschiedene Zahlen fuer denselben Lauf.
  const median = (arr) => {
    const s = [...arr].sort((a, b) => a - b);
    const m = Math.floor(s.length / 2);
    return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
  };
  const anPct = mitPunkt.map((f) => Number(f[iAn])).filter(Number.isFinite);
  const bisPct = mitPunkt.map((f) => Number(f[iBis])).filter(Number.isFinite);

  console.log(`${fluege.length} Flüge, davon ${mitPunkt.length} mit Messpunkt.`);
  console.log(`  bis Sinkflug: Median ${median(bisPct)?.toFixed(1)} %`);
  console.log(`  Anflug:       Median ${median(anPct)?.toFixed(1)} %`);
  if (dlh && dlh[iStatus] === "ok") {
    console.log(`  DLH 370:      ${dlh[iBis]} % / ${dlh[iAn]} %`);
  }

  if (fehler.length > 0) {
    console.error(`\nFEHLER: ${fehler.length} Befund(e) am echten Bestand:`);
    for (const f of fehler) console.error(`  · ${f}`);
    process.exit(1);
  }

  console.log("KORPUS OK");
}

main();
