/*
 * AeroACARS HUD — v3.9, 11.08.2026: Zeitzelle rotiert selbst (ETE/ETA/FLT alle 8 s) — der Klick kam in Flow nie an.
 *
 * Spricht mit AeroACARS' fest eingebautem Panel-Server auf Port 47847:
 *   GET /panel/status    Flugstatus, im Sekundentakt abgefragt
 *   GET /panel/debrief   Landeprotokoll, einmal pro Landung
 *   GET /panel/activity  juengster ACARS-Log-Eintrag
 *
 * ══════════════════════════════════════════════════════════════════════
 * WARUM v3: der Abgleich mit den Referenzprojekten
 * ══════════════════════════════════════════════════════════════════════
 *
 * Alle Fassungen bis v2.2 bauten ihren eigenen Unterbau: setInterval-
 * Schleifen, globale document-Zugriffe, eigener Sichtbarkeits-Toggle.
 * Der Abgleich mit der offiziellen Flow-Doku (parallel42.com/blogs/
 * flow-documentation) zeigt: Flow HAT einen dokumentierten Lebenszyklus,
 * und die Referenz-Widgets sind genau deshalb stabil, weil sie ihn
 * benutzen:
 *
 *   run()            laeuft bei jedem Klick auf die Wheel-Kachel
 *   html_created(el) liefert das EIGENE Wurzelelement des Widgets
 *   loop_1hz(cb)     Flow-verwalteter Sekundentakt
 *   exit(cb)         laeuft VOR jedem Neuladen/Loeschen des Skripts —
 *                    die Doku sagt woertlich: "be diligent and clear
 *                    your loops, intervals and timeouts"
 *
 * Unser Zombie-Desaster (mehrere Alt-Kopien liefen nach jedem Neuladen
 * weiter, schrieben durcheinander in die Anzeige und verstopften die
 * Verbindungsplaetze des Simulators) war die direkte Folge davon, an
 * dieser API vorbeizuarbeiten: Flow kann fremde setInterval nicht
 * aufraeumen, und wir haben exit() nie registriert.
 *
 * v3 nutzt den offiziellen Lebenszyklus — und behaelt drei Schutzschichten
 * fuer die Uebergangszeit, denn im Sim koennen noch Alt-Kopien OHNE
 * exit() leben, bis er einmal neu gestartet wurde:
 *
 *   1. Abschaltgriff: eine v2/v3-Vorgaengerin wird beim Start stillgelegt.
 *   2. Buehnenentzug: der Inhalt entsteht hier im Skript unter
 *      aa2-Kennungen; Fassungen bis v1.5 finden ihre Kennungen (#dot,
 *      .hud …) nicht mehr und malen ins Leere.
 *   3. ICH.tot: eine stillgelegte Kopie zeichnet und fragt nie wieder.
 *
 * ══════════════════════════════════════════════════════════════════════
 * FELD-BEFUNDE, die im Verhalten stecken (nicht zuruecksetzen)
 * ══════════════════════════════════════════════════════════════════════
 *
 * B2  Anfragen HAENGEN in Coherent GT, sie scheitern nicht (belegt:
 *     "Zeitueberschreitung nach 3 s" im HUD, waehrend PowerShell dieselbe
 *     Route in 43 ms beantwortet bekam) → eigene Zeitschranke.
 * B3  Der Verbindungsvorrat ist klein und wird pro Ziel-Adresse gefuehrt
 *     → hoechstens EINE Anfrage gleichzeitig (alle Routen zusammen), und
 *     Adresswechsel 127.0.0.1 ↔ [::1] NUR nach Zeitueberschreitungen —
 *     ein sofortiger Fehler heisst "da lauscht nichts" (beta.3 kennt ::1
 *     nicht), dann zurueck zur Hauptadresse.
 * B4  Der Renderer hat keine typografischen Sonderzeichen (ein `·` kam
 *     als Kaestchen heraus) → eigener Text nur ASCII, Server-Texte durch
 *     nurAscii().
 * B5  Kein `gap` auf Flex-Containern — siehe code.css.
 *
 * Zeitangaben: alles vom Server ist UTC. Am Ticker steht das ALTER
 * ("vor 3 s") — die Frage dort ist "ist das gerade passiert", nicht "um
 * wie viel Uhr". Ein Z steht nur an echten Flugzeiten (Block an).
 */
(function () {
  'use strict';

  // ═══════════════════════════════════════════════════════════════════
  // 0. Vorgaengerinnen stilllegen + eigener Lebenszyklus-Anker
  // ═══════════════════════════════════════════════════════════════════

  var G = (typeof globalThis !== 'undefined') ? globalThis
        : (typeof window !== 'undefined') ? window
        : this;

  /* Der Ablageplatz traegt die UMGEBUNG im Namen (v3.3). Grund: dieselbe
     Datei laeuft jetzt in Flow Pro UND im nativen MSFS-Panel, und der
     Pilot soll frei waehlen koennen — auch beide gleichzeitig offen.
     Nach allem, was wir wissen, bekommt jede Coherent-GT-Ansicht ihre
     eigenen Globals, die beiden koennten sich also ohnehin nicht sehen.
     Verlassen wollen wir uns darauf nicht: teilten sie sich wider
     Erwarten einen Kontext, wuerde die zweite Kopie die erste als
     "Vorgaengerin" stilllegen — der Pilot saehe ein totes Fenster. Mit
     getrennten Schluesseln raeumt jede Umgebung nur ihre EIGENEN
     Alt-Kopien ab, was der ganze Zweck der Uebung ist. */
  var SCHLUESSEL = '__aeroacarsHudV2_' +
    ((typeof run === 'function') ? 'flow' : 'nativ');

  try {
    if (G && G[SCHLUESSEL] && typeof G[SCHLUESSEL].stop === 'function') {
      G[SCHLUESSEL].stop();
    }
  } catch (e) { /* keine Vorgaengerin, oder kein Zugriff — beides gut */ }

  var ICH = { tot: false, wecker: [] };

  function raeumeAuf() {
    ICH.tot = true;
    for (var i = 0; i < ICH.wecker.length; i++) {
      clearTimeout(ICH.wecker[i]);
      clearInterval(ICH.wecker[i]);
    }
    ICH.wecker.length = 0;
  }

  try {
    G[SCHLUESSEL] = { stop: raeumeAuf };
  } catch (e) { /* kein globaler Ablageplatz: dann greift wenigstens ICH.tot */ }

  /* Der OFFIZIELLE Aufraeum-Haken. Flow ruft ihn vor jedem Neuladen und
     Loeschen — damit ist die Zombie-Quelle an der Wurzel zu, nicht nur
     ueberdeckt. typeof-Pruefung, damit dieselbe Datei auch in der
     Browser-Vorschau ohne Flow laeuft. */
  if (typeof exit === 'function') {
    exit(function () { raeumeAuf(); });
  }

  // ═══════════════════════════════════════════════════════════════════
  // 1. Konfiguration
  // ═══════════════════════════════════════════════════════════════════

  var PORT = 47847;
  var TIMEOUT_MS = 3000;        // B2
  var FEHLER_PAUSE_MS = 5000;   // langsamer nachfragen, solange es klemmt
  var HOSTWECHSEL_NACH = 3;     // B3: Zeitueberschreitungen bis zum Wechsel
  var AKTIVITAET_JEDEN = 5;     // Ticker nach jedem 5. erfolgreichen Status

  /* Zwei gleichwertige Adressen desselben Servers (AeroACARS bedient beide
     ab v1.5.0-beta.4; aeltere nur die erste — dafuer sorgt die Rueckkehr
     in fehlschlag()). */
  var HOSTS = ['127.0.0.1', '[::1]'];
  var hostIndex = 0;
  function basis() { return 'http://' + HOSTS[hostIndex] + ':' + PORT; }

  var APPROACH = ['approach', 'final'];
  var NACH_TOUCHDOWN = ['landing', 'taxi_in', 'blocks_on', 'arrived', 'pirep_submitted'];

  var PHASEN = {
    preflight: 'Vorbereitung', boarding: 'Boarding', pushback: 'Pushback',
    taxi_out: 'Rollen', takeoff_roll: 'Startlauf', takeoff: 'Start',
    climb: 'Steigflug', cruise: 'Reiseflug', holding: 'Warteschleife',
    descent: 'Sinkflug', approach: 'Anflug', final: 'Endanflug',
    landing: 'Landung', taxi_in: 'Rollen zum Stand', blocks_on: 'Am Stand',
    arrived: 'Angekommen', pirep_submitted: 'PIREP eingereicht',
  };

  /* Echte Wire-Werte aus aggregate_score_label(), kleingeschrieben. */
  var BAND = { smooth: '', acceptable: '', firm: 'aa2-w', hard: 'aa2-b', severe: 'aa2-b' };


  // ═══════════════════════════════════════════════════════════════════
  // 2. Zustand
  // ═══════════════════════════════════════════════════════════════════

  /* Kein gespeicherter `modus`: der wird bei jeder Anzeige neu abgeleitet.
     Ein gespeicherter Wert war schon einmal die Ursache dafuer, dass der
     Streifen nach einem Abriss weiter Anflugdaten zeigte. */
  var tickerZaehler = 0;

  var Z = {
    verbunden: false,
    status: null,
    debrief: null,
    debriefFuer: null,
    aktivitaet: null,
    aktivitaetFehlt: false,
    fehlerText: null,
    fehlerSeit: null,
    fehlerInFolge: 0,
    timeoutsInFolge: 0,
  };

  // ═══════════════════════════════════════════════════════════════════
  // 3. Aufbau der Anzeige (Buehnenentzug: eigene aa2-Kennungen)
  // ═══════════════════════════════════════════════════════════════════

  var K = {};          // Knoten, nach Namen
  var wurzelEl = null; // .aa2-root — kommt bevorzugt aus html_created

  function neu(tag, klasse, text) {
    var n = document.createElement(tag);
    if (klasse) n.className = klasse;
    if (text != null) n.textContent = text;
    return n;
  }

  function findeWurzel(el) {
    if (el) {
      if (el.classList && el.classList.contains('aa2-root')) return el;
      if (el.querySelector) {
        var r = el.querySelector('.aa2-root');
        if (r) return r;
      }
    }
    /* Flow-Kontext: der .aa2-root-Wrapper. Nativer MSFS-Kontext: den
       gibt es nicht, dort ist der Streifen selbst die Wurzel. */
    return document.querySelector('.aa2-root') || document.querySelector('.aa2-strip');
  }

  function baueAnzeige() {
    if (K.data) return true; // schon gebaut
    if (!wurzelEl) return false;
    var streifen = (wurzelEl.classList && wurzelEl.classList.contains('aa2-strip'))
      ? wurzelEl
      : wurzelEl.querySelector('.aa2-strip');
    if (!streifen) return false;

    /* Alles Vorherige raus — auch Reste einer Alt-Kopie. Danach existiert
       kein Element mehr, das eine solche kennt. */
    while (streifen.firstChild) streifen.removeChild(streifen.firstChild);

    K.streifen = streifen;

    K.ticker = neu('div', 'aa2-ticker');
    K.age = neu('span', 'aa2-age aa2-mono');
    K.msg = neu('span', 'aa2-msg');
    K.ticker.appendChild(K.age);
    K.ticker.appendChild(K.msg);

    K.rule = neu('div', 'aa2-rule');

    K.data = neu('div', 'aa2-data');
    K.dot = neu('span', 'aa2-dot');
    K.ident = neu('span', 'aa2-ident', 'AeroACARS');
    K.data.appendChild(K.dot);
    K.data.appendChild(K.ident);

    /* Feste Zonenfolge: Kennung | Lage | Messwerte | Anhang. Die
       Trennstriche verwaltet raeumeTrenner() selbst. */
    K.data.appendChild(neu('span', 'aa2-sep'));

    K.state = neu('span', 'aa2-state');
    K.spin = neu('span', 'aa2-spin');
    K.score = neu('span', 'aa2-score');
    K.scoreVal = neu('span', 'aa2-val aa2-xl aa2-mono');
    K.scoreBand = neu('span', 'aa2-band');
    K.score.appendChild(K.scoreVal);
    K.score.appendChild(K.scoreBand);
    K.data.appendChild(K.state);
    K.data.appendChild(K.spin);
    K.data.appendChild(K.score);

    K.data.appendChild(neu('span', 'aa2-sep'));

    K.zellen = [];
    for (var i = 0; i < 4; i++) {
      var z = neu('span', 'aa2-cell');
      var l = neu('span', 'aa2-lbl');
      var v = neu('span', 'aa2-val aa2-mono');
      z.appendChild(l);
      z.appendChild(v);
      K.data.appendChild(z);
      K.zellen.push({ wurzel: z, lbl: l, val: v });
    }

    /* v3.6 (F3): die erste und einzige Klick-Flaeche des Widgets. Eine
       Delegation auf der Datenzeile statt Handler pro Zelle: die Zellen
       werden bei jedem Takt neu beschriftet, der Handler ueberlebt das.
       In Flow liegt die Flaeche im Drag-Bereich — ein Klick ohne
       Bewegung kommt als click durch, ein Ziehen bleibt Ziehen. */
    /* BONUS-Interaktion: der Klick schaltet die Zeitzelle sofort einen
       Rotationsschritt weiter — noetig ist er NIE, die Zelle rotiert
       von selbst (Flow frisst Klicks, Livetest 11.08.2026). */
    try { K.data.addEventListener('click', klickAufDaten); } catch (err) {}
    function klickAufDaten(e) {
      try {
        var el = e.target;
        while (el && el !== K.data) {
          if (el.getAttribute && el.getAttribute('data-aa2-klick') === 'zeit') {
            zeitKlickOffset++;
            zeichne();
            return;
          }
          el = el.parentElement;
        }
      } catch (err) {}
    }

    K.wind = neu('span', 'aa2-wind');
    K.wind.appendChild(neu('span', 'aa2-lbl', 'Wind'));
    var svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('class', 'aa2-arrow');
    var pfad = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    pfad.setAttribute('d', 'M12 2 L18.5 21 L12 16.6 L5.5 21 Z');
    svg.appendChild(pfad);
    K.windPfeil = svg;
    K.windGesamt = neu('span', 'aa2-val aa2-mono');
    K.windQuer = neu('span', 'aa2-val aa2-mono');
    K.wind.appendChild(svg);
    K.wind.appendChild(K.windGesamt);
    K.wind.appendChild(neu('span', 'aa2-lbl', 'quer'));
    K.wind.appendChild(K.windQuer);
    K.data.appendChild(K.wind);

    K.data.appendChild(neu('span', 'aa2-sep'));

    K.tail = neu('span', 'aa2-tail aa2-mono');
    K.data.appendChild(K.tail);

    streifen.appendChild(K.ticker);
    streifen.appendChild(K.rule);
    streifen.appendChild(K.data);
    return true;
  }

  function zeige(knoten, sichtbar) {
    if (knoten) knoten.style.display = sichtbar ? '' : 'none';
  }
  function istSichtbar(knoten) {
    return !!knoten && knoten.style.display !== 'none' && knoten.offsetWidth > 0;
  }

  /* Trennstriche nur zwischen zwei tatsaechlich sichtbaren Zonen — nie am
     Anfang, nie am Ende, nie doppelt. Einmal am Ende jeder Anzeige statt
     von Hand in jedem der siebzehn Zustaende. */
  function raeumeTrenner() {
    var kinder = K.data.childNodes;
    var inhaltGesehen = false;
    var offenerTrenner = null;
    for (var i = 0; i < kinder.length; i++) {
      var n = kinder[i];
      if (n.className && String(n.className).indexOf('aa2-sep') !== -1) {
        zeige(n, false);
        offenerTrenner = inhaltGesehen ? n : null;
        continue;
      }
      if (istSichtbar(n)) {
        if (offenerTrenner) { zeige(offenerTrenner, true); offenerTrenner = null; }
        inhaltGesehen = true;
      }
    }
  }

  /* Eine Zelle ohne Wert wird als GANZES ausgeblendet — nie eine
     Beschriftung ohne Zahl. */
  function setzeZellen(liste) {
    /* null-Eintraege rausfiltern, damit optionale Zellen (WX) keine
       Luecke lassen, sondern die folgenden aufruecken. */
    var dicht = [];
    for (var f = 0; f < liste.length; f++) if (liste[f]) dicht.push(liste[f]);
    liste = dicht;
    for (var i = 0; i < K.zellen.length; i++) {
      var z = K.zellen[i];
      var e = liste[i];
      if (!e || e[1] == null) { zeige(z.wurzel, false); continue; }
      z.lbl.textContent = e[0];
      z.val.textContent = e[1];
      z.val.className = 'aa2-val aa2-mono' + (e[2] ? ' ' + e[2] : '');
      /* v3.6 (F3): viertes Element = Klick-Kennung. Zellen sind sonst
         reine Anzeige; nur wer eine Kennung traegt, ist anfassbar (CSS
         zeigt den Zeiger, nativ oeffnet sie die pointer-events-Sperre). */
      try {
        if (e[3]) z.wurzel.setAttribute('data-aa2-klick', e[3]);
        else z.wurzel.removeAttribute('data-aa2-klick');
      } catch (err) {}
      zeige(z.wurzel, true);
    }
  }

  // ═══════════════════════════════════════════════════════════════════
  // 4. Uebertragung (B2 + B3)
  // ═══════════════════════════════════════════════════════════════════

  /* v3.9 (F3, zweiter Anlauf nach Livetest): Die Zeitzelle ROTIERT von
     selbst. Der Klick-Umschalter aus v3.6 kam im Flow-Widget nie an —
     Flows Drag-Erkennung frisst den Klick (Thomas' Livetest 11.08.2026:
     "geht nicht"), und eine Bedienung, die nur im Browser-HUD
     funktioniert, ist keine. Also wechselt die Zelle jetzt alle 8 s
     selbst: ETE (Restflugzeit) -> ETA (Ankunftszeit, jetzt + ETE) ->
     FLT (geflogene Zeit). Der Klick bleibt als Bonus erhalten, wo er
     durchkommt (Browser-HUD): er schaltet sofort einen Schritt weiter. */
  var ZEIT_ROTATION_MS = 8000;
  var zeitKlickOffset = 0;

  var anfrageLaeuft = false;   // gilt fuer ALLE Routen zusammen
  var naechsterVersuch = 0;

  /* fetch mit Zeitschranke. Die haengende Anfrage laeuft im Hintergrund
     weiter (abbrechen geht in dieser Umgebung nicht), aber das Widget
     bleibt handlungsfaehig und kann benennen, was los ist. */
  function hole(pfad) {
    return new Promise(function (ok, fehler) {
      var fertig = false;
      var wecker = setTimeout(function () {
        if (fertig) return;
        fertig = true;
        var e = new Error('Zeitueberschreitung nach ' + (TIMEOUT_MS / 1000) + ' s');
        e.istZeitueberschreitung = true;
        fehler(e);
      }, TIMEOUT_MS);
      ICH.wecker.push(wecker);
      if (ICH.wecker.length > 50) ICH.wecker.splice(0, 25);
      var p;
      try { p = fetch(basis() + pfad); }
      catch (e2) { clearTimeout(wecker); fertig = true; fehler(e2); return; }
      p.then(function (res) {
        if (fertig) return;
        fertig = true; clearTimeout(wecker); ok(res);
      }, function (e3) {
        if (fertig) return;
        fertig = true; clearTimeout(wecker); fehler(e3);
      });
    });
  }

  function erfolg() {
    anfrageLaeuft = false;
    naechsterVersuch = 0;
    Z.verbunden = true;
    Z.fehlerText = null;
    Z.fehlerSeit = null;
    Z.fehlerInFolge = 0;
    Z.timeoutsInFolge = 0;
  }

  function fehlschlag(err) {
    anfrageLaeuft = false;
    naechsterVersuch = Date.now() + FEHLER_PAUSE_MS;
    Z.verbunden = false;
    Z.fehlerText = (err && (err.message || err.name)) || 'unbekannt';
    Z.fehlerSeit = Z.fehlerSeit || Date.now();
    Z.fehlerInFolge++;
    /* Adresswechsel NUR nach Zeitueberschreitungen: ein Wechsel hilft
       gegen genau ein Problem — einen Vorrat voller toter Steckplaetze,
       und der zeigt sich als HAENGEN. Ein sofortiger Fehler heisst "da
       lauscht nichts" (z. B. [::1] auf beta.3) → zurueck zur
       Hauptadresse, die wenigstens existiert. Ohne diese Rueckkehr gaebe
       es eine Sackgasse: der Sofort-Fehler auf der Ausweichadresse setzt
       den Timeout-Zaehler zurueck, und es kaeme nie wieder ein Wechsel. */
    if (err && err.istZeitueberschreitung) {
      Z.timeoutsInFolge++;
      if (Z.timeoutsInFolge % HOSTWECHSEL_NACH === 0) {
        hostIndex = (hostIndex + 1) % HOSTS.length;
      }
    } else {
      Z.timeoutsInFolge = 0;
      if (hostIndex !== 0) hostIndex = 0;
    }
    zeichne();
  }

  function statusTakt() {
    if (anfrageLaeuft || Date.now() < naechsterVersuch) return;
    anfrageLaeuft = true;
    hole('/panel/status')
      .then(function (res) {
        if (!res.ok) throw new Error('HTTP ' + res.status);
        return res.json();
      })
      .then(function (payload) {
        erfolg();
        neuerStatus(payload);
        /* Ticker-Abfrage HIER anstossen, nicht im Takt.
           DER Ticker-Bug (Feld, 09.08.2026): in `haupttakt()` stand
           `statusTakt(); if (...) aktivitaetTakt();` — und `statusTakt`
           setzt `anfrageLaeuft = true` SYNCHRON, freigegeben wird es erst
           in diesem `.then` hier, also im naechsten Mikrotask. Damit sah
           `aktivitaetTakt()` im selben Takt IMMER eine laufende Anfrage
           und stieg sofort wieder aus. Nicht manchmal — jedes Mal.
           Deshalb blieb die Zeile leer, obwohl `curl` auf dieselbe Route
           sauberes JSON lieferte, und deshalb aenderte auch
           AKTIVITAET_JEDEN = 1 nichts: derselbe Takt, dasselbe Problem.
           Sein Negativtest war der Beweis, nicht der Gegenbeweis.
           An dieser Stelle ist die Sperre frei (erfolg() hat sie eben
           geloest), und die Regel "hoechstens eine Anfrage gleichzeitig"
           bleibt trotzdem gewahrt. */
        tickerZaehler++;
        if (tickerZaehler % AKTIVITAET_JEDEN === 0) aktivitaetTakt();
      })
      .catch(fehlschlag);
  }

  function aktivitaetTakt() {
    if (!Z.verbunden || Z.aktivitaetFehlt) return;
    if (anfrageLaeuft || Date.now() < naechsterVersuch) return;
    anfrageLaeuft = true;
    hole('/panel/activity?limit=1')
      .then(function (res) {
        anfrageLaeuft = false;
        /* 404: diese AeroACARS-Version kennt die Route nicht — kein
           voruebergehender Fehler. Zeile leeren statt einen Eintrag
           altern zu lassen, und nicht weiter abfragen. */
        if (res.status === 404) { Z.aktivitaetFehlt = true; Z.aktivitaet = null; zeichneTicker(); return null; }
        if (!res.ok) return null;
        return res.json();
      })
      .then(function (eintraege) {
        if (eintraege == null) return;
        Z.aktivitaet = (eintraege && eintraege.length) ? eintraege[0] : null;
        zeichneTicker();
      })
      .catch(function () { anfrageLaeuft = false; });
  }

  function holeDebrief(pirepId) {
    if (anfrageLaeuft) return;
    anfrageLaeuft = true;
    hole('/panel/debrief')
      .then(function (res) {
        anfrageLaeuft = false;
        if (!res.ok) return null;
        return res.json();
      })
      .then(function (rec) {
        if (rec == null) return;
        Z.debrief = rec;
        Z.debriefFuer = pirepId;
        zeichne();
      })
      .catch(function () { anfrageLaeuft = false; });
  }

  // ═══════════════════════════════════════════════════════════════════
  // 5. Zustandsableitung
  // ═══════════════════════════════════════════════════════════════════

  function neuerStatus(payload) {
    Z.status = payload || null;
    if (!payload) { Z.debrief = null; Z.debriefFuer = null; }
    /* Das Landeprotokoll GENAU EINMAL, und erst auf der Kante zu
       'ergebnis' — vorher sind die Zahlen bekannt falsch (Feld: Rohwert
       -770 fpm, bewerteter Wert derselben Landung -455). */
    if (lage() === 'ergebnis' && payload && Z.debriefFuer !== payload.pirep_id) {
      holeDebrief(payload.pirep_id);
    }
    zeichne();
  }

  function lage() {
    if (!Z.verbunden) return 'getrennt';
    var s = Z.status;
    if (!s) return 'bereit';
    /* AeroACARS hat den Flug pausiert (Sim weg). Eigener Zustand, weil
       sonst weiter gruen eine Phase daestuende, waehrend die App daneben
       "Sim getrennt - Flug pausiert" meldet: die Werte im Streifen sind
       dann gespeicherte Staende, keine Messung. Feldbefund 09.08.2026. */
    if (s.paused_since) return 'pausiert';
    var ph = s.phase;
    if (NACH_TOUCHDOWN.indexOf(ph) !== -1) {
      if (!s.landing_score_finalized) return 'auswertung';
      if (ph === 'pirep_submitted') return 'eingereicht';
      if (ph === 'blocks_on' || ph === 'arrived') return 'amStand';
      if (ph === 'taxi_in') return 'rollen';
      return 'ergebnis';
    }
    if (APPROACH.indexOf(ph) !== -1) return 'anflug';
    return 'unterwegs';
  }

  // ═══════════════════════════════════════════════════════════════════
  // 6. Formatierung (B4 — nur ASCII)
  // ═══════════════════════════════════════════════════════════════════

  /* Reihenfolge ist wichtig: erst die Zeichen, fuer die es eine sinnvolle
     Entsprechung gibt, dann als LETZTES der Auffangrechen.

     Der Auffangrechen kam am 09.08.2026 dazu. Die Windows-Sitzung fand
     im Sim ein Kaestchen an der Stelle von `▶` (aus dem Fortsetzen-
     Hinweis in lib.rs) — ein Zeichen, das in dieser Liste schlicht
     fehlte. Das Muster war klar: jede Aufzaehlung einzelner Zeichen ist
     unvollstaendig, sobald irgendwo im Client ein neues auftaucht. Ein
     Kaestchen im Sim ist schlimmer als ein fehlendes Zeichen, also
     faellt jetzt alles durch, was der Font nicht sicher hat.

     Umlaute stehen BEWUSST vor dem Rechen: sonst wuerde aus
     "Verzoegerung" ein "Verzgerung". */
  var ERSATZ = [
    [/→/g, '>'], [/·/g, '-'], [/✓/g, 'OK'], [/°/g, ''],
    [/[–—]/g, '-'], [/[„“”]/g, '"'],
    [/[▶►➤]/g, '>'], [/[◀◄]/g, '<'], [/[•●]/g, '-'], [/[×✕✖]/g, 'x'],
    [/[⚠]/g, '!'], [/[…]/g, '...'], [/[‚‘’]/g, "'"],
    [/ä/g, 'ae'], [/ö/g, 'oe'], [/ü/g, 'ue'],
    [/Ä/g, 'Ae'], [/Ö/g, 'Oe'], [/Ü/g, 'Ue'], [/ß/g, 'ss'],
    [/[^\x20-\x7E]/g, ''],   // Auffangrechen — MUSS letzter Eintrag bleiben
  ];
  /* Log-Zeilen selbst kuerzen, statt sich auf `text-overflow: ellipsis`
     zu verlassen. Zwei Gruende, beide aus dem Feld:

     1. Die CSS-Ellipse zeichnet U+2026 — ein typografisches Zeichen.
        Genau diese Klasse von Zeichen erschien im Sim als Kaestchen,
        das war der ganze Grund fuer nurAscii(). Drei gewoehnliche
        Punkte koennen das nicht passieren.
     2. Ein Zeichenlimit wirkt unabhaengig von der Schrift. Der Sim
        rendert rund ein Viertel breiter als der Browser — eine
        pixelbasierte Kuerzung faellt in beiden Umgebungen verschieden
        aus, eine zeichenbasierte nicht.

     Anlass war die Sim-Pause-Meldung, die Vorher- UND Nachher-
     Koordinaten mitfuehrt und dadurch weit ueber jede Fensterbreite
     hinauslief (Foto aus dem Sim, 09.08.2026). Die CSS-Ellipse bleibt
     als zweites Netz stehen, greift aber im Normalfall nicht mehr. */
  var TICKER_MAX = 100;

  function kuerze(t) {
    var s = String(t == null ? '' : t);
    if (s.length <= TICKER_MAX) return s;
    /* An der letzten Wortgrenze trennen, damit nicht mitten im Wort
       abgeschnitten wird — aber nur, wenn dabei nicht zu viel verloren
       geht. */
    var schnitt = s.lastIndexOf(' ', TICKER_MAX - 3);
    if (schnitt < TICKER_MAX - 25) schnitt = TICKER_MAX - 3;
    return s.slice(0, schnitt) + '...';
  }

  function nurAscii(t) {
    var s = String(t == null ? '' : t);
    for (var i = 0; i < ERSATZ.length; i++) s = s.replace(ERSATZ[i][0], ERSATZ[i][1]);
    return s;
  }

  function zahl(v, stellen) {
    if (typeof v !== 'number' || isNaN(v)) return null;
    return v.toFixed(stellen == null ? 0 : stellen);
  }

  /* Callsign bevorzugen, wie phpVMS' eigener atc()-Accessor: Freifluege
     tragen oft Flugnummer 0 und den echten Identifier im Callsign. */
  function kennung(s) {
    if (!s) return 'AeroACARS';
    return s.callsign || ((s.airline_icao || '') + (s.flight_number || '')) || 'AeroACARS';
  }

  /* v3.6 (F1): Die Hoehen-Zelle, phasenrichtig.

     Oberhalb der Transition fliegt niemand nach Fuss ueber Grund oder
     Meereshoehe — da zaehlt das Flight Level aus der DRUCKhoehe. Zwei
     Ausloeser, jeder allein reicht:
       - Druckhoehe ueber ~17.500 ft (die USA wechseln bei 18.000, Europa
         frueher — 17.500 nimmt beide mit, und in der Übergangszone ist
         FL ohnehin die richtigere Anzeige), oder
       - QNH steht auf Standard 1013 (der Pilot HAT auf FL gewechselt,
         egal in welcher Hoehe — z.B. Transition Altitude 5.000 in EDDB).
     Darunter: echte Meereshoehe (ALT). Ganz ohne Barometrik (alte App,
     Sim ohne SimVar): AGL wie bisher — nie eine leere Zelle. */
  function hoehenZelle(live) {
    if (!live) return ['AGL', null];
    var p = live.altitude_pressure_ft;
    var qnhStd = typeof live.qnh_hpa === 'number' && Math.abs(live.qnh_hpa - 1013.25) < 0.6;
    if (typeof p === 'number' && (p >= 17500 || qnhStd)) {
      var fl = Math.round(p / 100);
      return ['FL', (fl < 100 ? ('00' + fl).slice(-3) : String(fl))];
    }
    if (typeof live.altitude_msl_ft === 'number') {
      return ['ALT', Math.round(live.altitude_msl_ft / 100) * 100 + ' ft'];
    }
    return ['AGL', typeof live.altitude_agl_ft === 'number'
      ? Math.round(live.altitude_agl_ft / 100) * 100 + ' ft' : null];
  }

  /* v3.6 (F3): Minuten als "1h04m" — dasselbe Format wie verstrichen(). */
  function alsDauer(min) {
    if (typeof min !== 'number' || min < 0) return null;
    var h = Math.floor(min / 60), m = min % 60;
    return h > 0 ? h + 'h' + (m < 10 ? '0' : '') + m + 'm' : m + 'm';
  }

  /* v3.9: Die Zeit-Zelle, rotierend. Kandidaten der Reihe nach:
       ETE  Restflugzeit vom Server (Grosskreis zum Ziel / Groundspeed)
       ETA  Ankunfts-UHRZEIT = jetzt + ETE, in Z — dieselbe Konvention
            wie die Flugwerte-Karte der App
       FLT  geflogene Zeit seit dem Abheben
     Kandidaten ohne Daten (kein ete_min: rollt gerade oder alter
     Server) fallen aus der Rotation, statt eine leere Zelle zu zeigen.
     Reihenfolge und Takt sind fest — die Zelle soll vorhersehbar
     ticken, nicht zappeln. */
  function zeitZelle(s, live) {
    var kandidaten = [];
    if (live && typeof live.ete_min === 'number') {
      kandidaten.push(['ETE', alsDauer(live.ete_min)]);
      var eta = new Date(Date.now() + live.ete_min * 60000);
      kandidaten.push(['ETA',
        ('0' + eta.getUTCHours()).slice(-2) + ':' + ('0' + eta.getUTCMinutes()).slice(-2) + 'z']);
    }
    if (s.takeoff_at) kandidaten.push(['FLT', verstrichen(s.takeoff_at)]);
    if (!kandidaten.length) return null;
    var i = (Math.floor(Date.now() / ZEIT_ROTATION_MS) + zeitKlickOffset) % kandidaten.length;
    var k = kandidaten[i];
    return [k[0], k[1], null, 'zeit'];
  }

  /* v3.7 (F2, zweiter Anlauf nach Pilotenfoto): METAR PARSEN statt roh
     kuerzen.

     Das Foto zeigte, warum rohes Kuerzen die falschen Teile trifft:
     "METAR EDDB AUTO 26012KT 240V300 9999 -SHRA FEW///TCU 30/13 Q1011"
     wurde nach "-SHRA" abgeschnitten — Fuellwoerter (METAR, AUTO),
     Windvarianz (240V300) und Standardsicht (9999) frassen den Platz,
     waehrend QNH, Temperatur und die Gewitterwolke hinten runterfielen.
     Also genau verkehrt herum: das Wichtigste stand am Ende.

     Deshalb: Tokens filtern und NUR die Kernwerte setzen —
       Station | Wind (mit Boeen) | Sicht (nur wenn nicht 9999) |
       Wetter (-SHRA, TS, FG ...) | niedrigste BKN/OVC-Schicht |
       CB/TCU-Warnung (auch aus FEW///TCU) | Temp/Taupunkt | QNH/Alti
     Aus dem Foto-Beispiel wird so: "EDDB 26012KT -SHRA TCU 30/13 Q1011"
     — 34 Zeichen, alles Wesentliche, nichts abgeschnitten.

     Alles nach BECMG/TEMPO/NOSIG/RMK ist Trend/Anhang und faellt weg.
     Liefert das Parsen zu wenig (exotisches Format), faellt die Anzeige
     auf den gekuerzten Rohtext zurueck — lieber haesslich als leer. */
  /* v3.8 (#hud-metar): BEVORZUGT die vom Server DEKODIERTEN Felder.

     Thomas' Vorgabe nach dem Foto der Desktop-App: "der muss alles
     erfassen was es geben kann". Die Antwort ist kein zweiter Parser,
     sondern derselbe Dekoder wie in der App: seit v1.5.2 liefert der
     Server `dep_metar_decoded`/`arr_metar_decoded` (metar-Crate, exakt
     die Quelle der App-Wetterkarte). Dieses Widget FORMATIERT nur noch:

       EDDB -SHRA 260/12kt VIS 6km BKN012 30/13 Q1011

     Reihenfolge und Auswahl wie die App-Karte (Station, Wetter, Wind,
     Sicht, tiefste bedeckende Schicht, T/TP, QNH). Der Text-Parser
     darunter bleibt als Rueckfall fuer Server vor v1.5.2. */
  function wxAusDekodiert(d) {
    if (!d) return null;
    var teile = [];
    if (d.icao) teile.push(nurAscii(d.icao));
    if (d.weather) teile.push(nurAscii(d.weather));
    if (typeof d.wind_speed_kt === 'number') {
      var richtung = (typeof d.wind_direction_deg === 'number')
        ? ('00' + Math.round(d.wind_direction_deg)).slice(-3)
        : 'VRB';
      teile.push(richtung + '/' + Math.round(d.wind_speed_kt) +
        (typeof d.gust_kt === 'number' ? 'G' + Math.round(d.gust_kt) : '') + 'kt');
    }
    if (typeof d.visibility_m === 'number') {
      /* Wie die App: ab 10 km ist die Zahl egal. */
      teile.push(d.visibility_m >= 9999 ? 'VIS 10km+'
        : 'VIS ' + (d.visibility_m >= 5000
            ? Math.round(d.visibility_m / 1000) + 'km'
            : d.visibility_m + 'm'));
    }
    if (d.cloud_layers && d.cloud_layers.length) {
      /* Die tiefste Schicht, die eine Decke bildet — sonst die tiefste
         ueberhaupt. CB/TCU-Zusatz bleibt am cover-Text erhalten. */
      var beste = null;
      for (var i = 0; i < d.cloud_layers.length; i++) {
        var c = d.cloud_layers[i];
        if (typeof c.base_ft !== 'number') continue;
        var deckt = /^(BKN|OVC|VV)/.test(c.cover || '');
        if (!beste || (deckt && !beste.deckt) ||
            (deckt === beste.deckt && c.base_ft < beste.base_ft)) {
          beste = { cover: c.cover, base_ft: c.base_ft, deckt: deckt };
        }
      }
      if (beste) {
        teile.push(nurAscii(beste.cover) + ('00' + Math.round(beste.base_ft / 100)).slice(-3));
      }
    }
    if (typeof d.temperature_c === 'number' && typeof d.dewpoint_c === 'number') {
      var g = function (x) { var r = Math.round(x); return (r < 0 ? 'M' : '') + ('0' + Math.abs(r)).slice(-2); };
      teile.push(g(d.temperature_c) + '/' + g(d.dewpoint_c));
    }
    if (typeof d.qnh_hpa === 'number') teile.push('Q' + Math.round(d.qnh_hpa));
    return teile.length >= 2 ? teile.join(' ') : null;
  }

  function wxText(raw) {
    if (!raw) return null;
    var t = nurAscii(String(raw)).replace(/\s+/g, ' ').trim();
    var tok = t.split(' ');
    var station = null, wind = null, sicht = null, temp = null, druck = null;
    var wetter = [], wolkeTief = null, wolkeTiefFt = Infinity, konvektiv = null;
    var kern = 0;
    for (var i = 0; i < tok.length; i++) {
      var w = tok[i];
      if (/^(BECMG|TEMPO|NOSIG|RMK)$/.test(w)) break;
      if (/^(METAR|SPECI|AUTO|COR)$/.test(w)) continue;
      if (i < 2 && /^[A-Z]{4}$/.test(w) && !station) { station = w; continue; }
      if (/^\d{6}Z$/.test(w)) continue;
      if (/^\d{3}V\d{3}$/.test(w)) continue;              // Windvarianz
      if (/^R\d{2}[LRC]?\//.test(w)) continue;            // RVR
      if (/^(\d{3}|VRB)\d{2,3}(G\d{2,3})?(KT|MPS)$/.test(w)) { wind = w; kern++; continue; }
      if (w === 'CAVOK') { sicht = 'CAVOK'; kern++; continue; }
      if (/^\d{4}(NDV)?$/.test(w)) { if (w.slice(0, 4) !== '9999') sicht = w.slice(0, 4); kern++; continue; }
      if (/SM$/.test(w)) { if (w !== '10SM') sicht = w; kern++; continue; }
      if (/^(VC)?[+-]?(MI|PR|BC|DR|BL|SH|TS|FZ)?(DZ|RA|SN|SG|IC|PL|GR|GS|UP|BR|FG|FU|VA|DU|SA|HZ|PO|SQ|FC|SS|DS)+$/.test(w)) {
        if (wetter.length < 3) wetter.push(w);
        kern++;
        continue;
      }
      var wolke = /^(FEW|SCT|BKN|OVC|VV)(\d{3}|\/\/\/)(CB|TCU)?$/.exec(w);
      if (wolke) {
        kern++;
        if (wolke[3]) konvektiv = wolke[3];               // CB/TCU von JEDER Schicht
        if ((wolke[1] === 'BKN' || wolke[1] === 'OVC' || wolke[1] === 'VV') && /^\d{3}$/.test(wolke[2])) {
          var ft = parseInt(wolke[2], 10);
          if (ft < wolkeTiefFt) { wolkeTiefFt = ft; wolkeTief = wolke[1] + wolke[2]; }
        }
        continue;
      }
      if (/^M?\d{2}\/M?\d{2}$/.test(w)) { temp = w; kern++; continue; }
      if (/^[QA]\d{4}$/.test(w)) { druck = w; kern++; continue; }
      // Unbekanntes Token: ignorieren, nicht stolpern.
    }
    if (kern < 2) {
      /* Rueckfall: Rohtext ohne Fuellwoerter, an der Wortgrenze gekuerzt. */
      t = t.replace(/^(METAR|SPECI) /, '').replace(/\b\d{6}Z\b ?/, '')
           .replace(/ (NOSIG|RMK .*)$/, '');
      if (t.length <= 48) return t;
      var schnitt = t.lastIndexOf(' ', 45);
      if (schnitt < 28) schnitt = 45;
      return t.slice(0, schnitt) + '...';
    }
    var teile = [];
    if (station) teile.push(station);
    if (wind) teile.push(wind);
    if (sicht) teile.push(sicht);
    for (var j = 0; j < wetter.length; j++) teile.push(wetter[j]);
    if (wolkeTief) teile.push(wolkeTief);
    if (konvektiv) teile.push(konvektiv);
    if (temp) teile.push(temp);
    if (druck) teile.push(druck);
    return teile.join(' ');
  }

  function verstrichen(iso) {
    if (!iso) return null;
    var ms = Date.parse(iso);
    if (isNaN(ms)) return null;
    var min = Math.floor((Date.now() - ms) / 60000);
    if (min < 0) return null;
    var h = Math.floor(min / 60);
    return (h > 0 ? h + ' h ' : '') + (min % 60) + ' min';
  }

  /* Nur fuer echte Flugzeiten. Der Server liefert UTC → Anzeige UTC, mit
     Z, damit es niemand fuer Ortszeit haelt. */
  function uhrzeitZ(iso) {
    if (!iso) return null;
    var d = new Date(iso);
    if (isNaN(d.getTime())) return null;
    function p(n) { return (n < 10 ? '0' : '') + n; }
    return p(d.getUTCHours()) + ':' + p(d.getUTCMinutes()) + ':' + p(d.getUTCSeconds()) + 'Z';
  }

  function alter(iso) {
    if (!iso) return '';
    var ms = Date.parse(iso);
    if (isNaN(ms)) return '';
    var s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
    if (s < 60) return 'vor ' + s + ' s';
    var m = Math.floor(s / 60);
    if (m < 60) return 'vor ' + m + ' min';
    return 'vor ' + Math.floor(m / 60) + ' h';
  }

  /* Ist gegen Soll in kg. Ohne Planwert (Freiflug, kein OFP) nur der
     gemessene Wert statt einer irrefuehrenden Null. */
  function istSoll(ist, soll) {
    if (typeof ist !== 'number') return null;
    var links = String(Math.round(ist));
    if (typeof soll !== 'number' || soll <= 0) return links + ' kg';
    return links + ' / ' + Math.round(soll);
  }
  /* Toleranzfenster um den Planwert: darin gilt die Beladung als passend.
     Nach oben etwas weiter, weil ein bisschen Mehrbetankung ueblich und
     unkritisch ist, nach unten enger, weil fehlender Sprit das eigentliche
     Problem waere. */
  var BELADUNG_MIN = 0.98;
  var BELADUNG_MAX = 1.05;

  function beladungsFarbe(ist, soll) {
    if (typeof ist !== 'number' || typeof soll !== 'number' || soll <= 0) return '';
    var anteil = ist / soll;
    if (anteil >= BELADUNG_MIN && anteil <= BELADUNG_MAX) return 'aa2-g';
    /* Gelb, nicht rot — obwohl das Loadsheet in AeroACARS die Abweichung
       rot faerbt. Grund: in DIESEM Streifen bedeutet rot "gefaehrlich"
       (Sinkrate weit ausserhalb des Korridors, Querwind ab 25 kt). Einen
       noch nicht fertig betankten Flieger am Gate mit einem instabilen
       Anflug gleichzusetzen waere eine falsche Dringlichkeit. Gelb heisst
       hier: stimmt noch nicht, schau hin. */
    return 'aa2-w';
  }

  /* Sinkraten-Korridor. Ziel folgt dem Gleitwinkel der geschaetzten Bahn,
     sonst 3 Grad. Sinkrate ~ Grundgeschwindigkeit x tan(Winkel) x 101,3. */
  function vsFarbe(live, gleitwinkel) {
    if (!live || typeof live.vertical_speed_fpm !== 'number') return '';
    var gs = typeof live.gs_kt === 'number' ? live.gs_kt : 0;
    if (gs < 40) return '';
    var grad = (typeof gleitwinkel === 'number' && gleitwinkel >= 2 && gleitwinkel <= 7.5)
      ? gleitwinkel : 3.0;
    var ziel = gs * Math.tan(grad * Math.PI / 180) * 101.3;
    var ist = -live.vertical_speed_fpm;
    if (ist < ziel * 0.55 || ist > ziel * 1.45) return 'aa2-b';
    if (ist < ziel * 0.75 || ist > ziel * 1.25) return 'aa2-w';
    return 'aa2-g';
  }

  /* Pfeil zeigt, WOHIN der Wind weht, relativ zur Nase — Konvention wie im
     Landeprotokoll (Gegenwind positiv, Querwind positiv von rechts). */
  function windWinkel(gegen, quer) {
    return (Math.atan2(quer, -gegen) * 180 / Math.PI + 180) % 360;
  }

  /* Drei ehrliche Verbindungszustaende statt an/aus; der mittlere (blau,
     Rueckstand wird nachgesendet) ist der wichtigste. */
  function punktKlasse(s) {
    if (!Z.verbunden) return 'aa2-off';
    if (!s) return '';
    /* Pausiert ist NICHT gruen: aufgezeichnet wird gerade nichts. */
    if (s.paused_since) return 'aa2-sync';
    if (s.connection_state === 'failing') return 'aa2-off';
    if (s.queued_position_count > 0) return 'aa2-sync';
    return 'aa2-live';
  }

  // ═══════════════════════════════════════════════════════════════════
  // 7. Anzeige
  // ═══════════════════════════════════════════════════════════════════

  function zeichneTicker() {
    if (ICH.tot || !K.ticker) return;
    /* Ohne Verbindung keine Log-Zeile — sonst stuende neben "Keine
       Verbindung" ein alternder Alt-Eintrag: zwei widerspruechliche
       Aussagen nebeneinander. */
    var a = Z.verbunden ? Z.aktivitaet : null;
    zeige(K.ticker, !!a);
    zeige(K.rule, !!a);
    if (!a) { K.age.textContent = ''; K.msg.textContent = ''; return; }
    K.age.textContent = alter(a.timestamp);
    K.msg.textContent = kuerze(nurAscii(a.detail ? a.message + ' - ' + a.detail : a.message));
    var stufe = String(a.level || '').toLowerCase();
    K.msg.className = 'aa2-msg' + (stufe === 'warn' ? ' aa2-w' : stufe === 'error' ? ' aa2-e' : '');
  }

  function zeichne() {
    if (ICH.tot || !K.data) return;
    var s = Z.status;
    var live = (s && s.live) || null;
    var d = Z.debrief;
    var l = lage();

    K.streifen.className = 'aa2-strip' + ((l === 'unterwegs' || l === 'bereit') ? ' aa2-quiet' : '');
    K.dot.className = 'aa2-dot ' + punktKlasse(s);
    K.ident.textContent = kennung(s);

    // Grundstellung; jede Ansicht schaltet nur an, was sie braucht.
    zeige(K.spin, false);
    zeige(K.wind, false);
    zeige(K.score, false);
    setzeZellen([]);
    K.state.textContent = '';
    K.state.className = 'aa2-state';
    zeige(K.state, false);
    K.tail.textContent = '';
    zeige(K.tail, false);

    function lageText(t, klasse) {
      K.state.textContent = t;
      K.state.className = 'aa2-state' + (klasse ? ' ' + klasse : '');
      zeige(K.state, true);
    }
    function anhang(t) {
      K.tail.textContent = t;
      zeige(K.tail, !!t);
    }

    switch (l) {
      case 'getrennt':
        K.ident.textContent = 'AeroACARS';
        lageText('Keine Verbindung - ' + HOSTS[hostIndex] + ':' + PORT);
        /* Grund und Dauer daneben: ein Foto vom Bildschirm reicht dann,
           um die Ursache zu benennen. */
        setzeZellen([
          ['seit', Z.fehlerSeit ? Math.round((Date.now() - Z.fehlerSeit) / 1000) + ' s' : null],
          ['Grund', Z.fehlerText ? nurAscii(Z.fehlerText).slice(0, 40) : null],
        ]);
        break;

      case 'bereit':
        K.ident.textContent = 'AeroACARS';
        lageText('Bereit - wartet auf Flug');
        break;

      case 'pausiert':
        lageText('Sim getrennt - Flug pausiert', 'aa2-w');
        setzeZellen([
          ['seit', verstrichen(s.paused_since)],
          ['Route', (s.dpt_airport || '----') + ' > ' + (s.arr_airport || '----')],
        ]);
        anhang('In AeroACARS fortsetzen');
        break;

      case 'unterwegs': {
        var route = (s.dpt_airport || '----') + ' > ' + (s.arr_airport || '----');
        if (s.takeoff_at) {
          /* v3.6: Hoehe phasenrichtig (FL/ALT/AGL, siehe hoehenZelle),
             Zeit umschaltbar (ETE/FLT, siehe zeitZelle). Das Zielwetter
             ab dem Sinkflug, bis ~10.000 ft — darunter uebernimmt die
             Anflug-Ansicht, und die hat fuer ein METAR keinen Platz. */
          var wxSink = (s.phase === 'descent' &&
            (!live || typeof live.altitude_msl_ft !== 'number' || live.altitude_msl_ft > 10000))
            ? (wxAusDekodiert(s.arr_metar_decoded) || wxText(s.arr_metar)) : null;
          setzeZellen([
            ['Route', route],
            hoehenZelle(live),
            zeitZelle(s, live),
            wxSink ? ['WX', wxSink] : null,
          ]);
        } else {
          /* Am Boden ist die Frage "ist geladen, was geladen sein soll":
             sim_* = gemessen, planned_* = OFP. ZFW, weil genau der Wert im
             OFP steht. */
          setzeZellen([
            ['Route', route],
            ['Fuel', istSoll(s.sim_fuel_kg, s.planned_block_fuel_kg),
              beladungsFarbe(s.sim_fuel_kg, s.planned_block_fuel_kg)],
            ['ZFW', istSoll(s.sim_zfw_kg, s.planned_zfw_kg),
              beladungsFarbe(s.sim_zfw_kg, s.planned_zfw_kg)],
            /* v3.6 (F2): Abflugwetter am Gate — der Server holt es seit
               v1.5.1 schon beim Boarding statt erst nach dem Abheben. */
            ['WX', wxAusDekodiert(s.dep_metar_decoded) || wxText(s.dep_metar)],
          ]);
        }
        anhang(PHASEN[s.phase] || s.phase || '');
        break;
      }

      case 'anflug': {
        setzeZellen([
          ['V/S', zahl(live && live.vertical_speed_fpm, 0), vsFarbe(live, s.approach_glideslope_angle)],
          ['IAS', zahl(live && live.ias_kt, 0)],
          ['AGL', zahl(live && live.altitude_agl_ft, 0)],
          ['Bank', zahl(live && live.bank_deg, 1)],
        ]);
        var gegen = live && live.headwind_kt, quer = live && live.crosswind_kt;
        if (typeof gegen === 'number' && typeof quer === 'number') {
          zeige(K.wind, true);
          K.windGesamt.textContent = String(Math.round(Math.sqrt(gegen * gegen + quer * quer)));
          var q = Math.abs(quer);
          K.windQuer.textContent = String(Math.round(q));
          K.windQuer.className = 'aa2-val aa2-mono' + (q >= 25 ? ' aa2-b' : q >= 15 ? ' aa2-w' : '');
          K.windPfeil.style.transform = 'rotate(' + windWinkel(gegen, quer).toFixed(0) + 'deg)';
        }
        /* Geschaetzte Bahn (MSFS liefert die ATC-Bahn nicht) — mit Tilde,
           damit die Anzeige nicht mehr behauptet, als sie weiss. */
        if (s.predicted_runway) anhang('~RWY ' + s.predicted_runway);
        break;
      }

      case 'auswertung':
        zeige(K.spin, true);
        lageText('Landung wird ausgewertet');
        break;

      case 'ergebnis':
      case 'rollen': {
        zeige(K.score, true);
        K.scoreVal.textContent = (d && d.score_numeric != null) ? String(d.score_numeric) : '--';
        var etikett = (d && d.score_label) || '';
        K.scoreBand.textContent = etikett ? etikett.toUpperCase() : '--';
        K.scoreBand.className = 'aa2-band ' + (BAND[etikett.toLowerCase()] || '');
        if (l === 'rollen') {
          setzeZellen([['Rate', d ? zahl(d.landing_rate_fpm, 0) : null]]);
          anhang('Rollen zum Stand');
        } else {
          setzeZellen([
            ['Rate', d ? zahl(d.landing_rate_fpm, 0) : null],
            ['G', d ? zahl(d.landing_scored_g_force != null ? d.landing_scored_g_force : d.landing_g_force, 2) : null],
            ['Bounces', (d && d.bounce_count != null) ? String(d.bounce_count) : null],
            ['Bahn', (d && d.runway_match && d.runway_match.runway_ident) || null],
          ]);
        }
        break;
      }

      case 'amStand':
        lageText('Am Stand');
        setzeZellen([
          ['Block an', uhrzeitZ(s.block_on_at)],
          ['Blockzeit', verstrichen(s.block_off_at)],
        ]);
        break;

      case 'eingereicht': {
        /* Das Haekchen ist eine Behauptung ueber den Server: Phase
           eingereicht UND leere Warteschlange UND letzte Uebertragung in
           Ordnung — zwei von drei reichen nicht. */
        var sauber = s.connection_state !== 'failing' && !s.queued_position_count;
        lageText(sauber ? 'PIREP eingereicht' : 'PIREP wartet', sauber ? 'aa2-g' : 'aa2-w');
        setzeZellen([sauber
          ? ['Gesendet', 'OK']
          : ['Offen', String(s.queued_position_count || 0)]]);
        break;
      }
    }

    raeumeTrenner();
  }

  // ═══════════════════════════════════════════════════════════════════
  // 8. Lebenszyklus — die offiziellen Flow-Haken
  // ═══════════════════════════════════════════════════════════════════

  var sichtbarkeit = false;

  function haupttakt() {
    if (ICH.tot) return;
    if (!K.data && !baueAnzeige()) return; // Buehne noch nicht da
    statusTakt();
    /* Altersangabe und Getrennt-Dauer laufen sichtbar weiter, auch wenn
       zwischen zwei Antworten nichts Neues kommt. */
    zeichneTicker();
    if (!Z.verbunden) zeichne();
  }

  function wendeSichtbarkeitAn() {
    var root = (wurzelEl && wurzelEl.classList && wurzelEl.classList.contains('aa2-root'))
      ? wurzelEl
      : document.querySelector('.aa2-root');
    if (!root) return;
    root.className = sichtbarkeit ? 'aa2-root aa2-visible' : 'aa2-root';
  }

  /* Offiziell: liefert das eigene Wurzelelement. Damit entfaellt der
     globale Blindflug — zwei nebeneinander installierte Widgets koennten
     sich nicht einmal mehr in die Quere kommen. */
  if (typeof html_created === 'function') {
    html_created(function (el) {
      if (ICH.tot) return;
      wurzelEl = findeWurzel(el);
      baueAnzeige();
      wendeSichtbarkeitAn();
      zeichne();
      zeichneTicker();
    });
  }

  /* Offiziell: Flow-verwalteter Sekundentakt — endet mit dem Skript,
     genau die Eigenschaft, die unseren setInterval-Schleifen fehlte.
     Rueckfall auf setInterval nur, falls eine Flow-Version den Haken
     nicht kennt; dann raeumt exit()/raeumeAuf() auf. */
  if (typeof loop_1hz === 'function') {
    loop_1hz(function () { haupttakt(); });
  } else {
    var t = setInterval(function () {
      if (ICH.tot) { clearInterval(t); return; }
      haupttakt();
    }, 1000);
    ICH.wecker.push(t);
  }

  /* ZWEI Einstiegswege, EINE Quelldatei (v3.3, native Portierung):

     Flow Pro:    run() laeuft bei jedem Klick auf die Wheel-Kachel — Flow
                  schaltet nichts von allein um, das Widget verwaltet seine
                  Sichtbarkeit selbst (verifiziert am vatsim-atis-Widget).

     Natives      run()/loop_1hz()/exit() existieren dort nicht (die
     MSFS-Panel:  typeof-Weichen oben waehlen automatisch die Rueckfall-
                  Pfade). Sichtbarkeit steuert das Toolbar-Symbol des Sims,
                  der Fensterrahmen uebernimmt Ziehen und Groesse — also:
                  sofort aufbauen, sofort abfragen, fertig. Verschieben ist
                  im Feld verifiziert (Runde-3-Stand, 09.08.2026), sobald
                  die Framework-Einbindungen in der HTML-Datei stehen. */
  if (typeof run === 'function') {
    run(function () {
      if (ICH.tot) return false;
      if (!wurzelEl) wurzelEl = findeWurzel(null);
      baueAnzeige();
      sichtbarkeit = !sichtbarkeit;
      wendeSichtbarkeitAn();
      haupttakt();
      return false; // Wheel sofort schliessen
    });
  } else {
    var starteNativ = function () {
      if (ICH.tot) return;
      if (!wurzelEl) wurzelEl = findeWurzel(null);
      baueAnzeige();
      sichtbarkeit = true;
      wendeSichtbarkeitAn();
      haupttakt();
    };
    if (typeof document !== 'undefined' && document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', starteNativ);
    } else {
      starteNativ();
    }
  }
})();
