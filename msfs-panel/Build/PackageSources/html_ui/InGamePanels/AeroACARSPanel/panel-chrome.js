/*
 * AeroACARS — Fensterdekoration fuer das NATIVE MSFS-2024-Panel.
 * v0.5.0, 09.08.2026.
 *
 * ─── Warum das eine eigene Datei ist ────────────────────────────────
 *
 * `panel.js` ist byte-identisch mit dem `code.js` des Flow-Pro-Widgets —
 * eine Quelle, zwei Ziele. Alles, was NUR nativ gilt, gehoert deshalb
 * hierher und nicht dort hinein. Diese Datei wird ausschliesslich von
 * AeroACARSPanel.html geladen; Flow sieht sie nie.
 *
 * ─── Das Problem, das sie loest ─────────────────────────────────────
 *
 * Thomas' Feldbefund vom 09.08.2026 (Foto aus dem Sim, Boarding EDDB):
 * der Streifen laeuft, beide Zeilen sind da, Live-Daten kommen an —
 * aber oben klebt eine Titelleiste "AEROACARS", und das Fenster laesst
 * sich nicht verschieben.
 *
 * Beides kommt vom Simulator, nicht von uns: seit wir `<ingame-ui>`
 * entfernt haben, legt MSFS seinen EIGENEN Fensterrahmen um die Seite
 * und beschriftet ihn mit dem `Name`-Attribut aus der
 * InGamePanelDefinition. Ein dokumentiertes Attribut zum Abschalten
 * dieser Leiste gibt es nicht (SDK-Doku und Community-Referenzen
 * durchgesehen, 09.08.2026).
 *
 * Und das Ziehen haengt genau an dieser Leiste: MSFS-Panels werden AM
 * TITEL gezogen. Der bestaetigte Asobo-Fehler (der Sim haengt Panel-
 * Elementen seit SU2 selbst die Klassen `hide`/`panelInvisible` an)
 * bricht sie — und damit auch das Ziehen.
 *
 * ─── Warum diese Datei sich selbst diagnostiziert ───────────────────
 *
 * Die genaue DOM-Struktur um ein MSFS-2024-Panel ist nicht
 * dokumentiert, und ich kann sie von einem Mac aus nicht messen.
 * Statt zu raten und Thomas nochmal in einen Blindtest zu schicken,
 * probiert das Skript die drei plausiblen Wege NACHEINANDER durch,
 * merkt sich welcher gegriffen hat, und schreibt das Ergebnis in eine
 * kleine Zeile im Panel — abfotografierbar. Beim naechsten Mal wissen
 * wir es dann genau, statt wieder zu vermuten.
 *
 * ALLES hier ist in try/catch gekapselt und rein additiv: greift kein
 * einziger Weg, bleibt das Panel exakt so nutzbar wie jetzt. Diese
 * Datei darf niemals der Grund sein, dass ein funktionierender
 * Streifen ausfaellt.
 */
(function () {
  'use strict';

  var BEFUND = [];            // was hat gegriffen, was nicht
  var ziehZiel = null;        // das Element, das wir tatsaechlich bewegen

  function notiz(t) { try { BEFUND.push(t); } catch (e) {} }

  /* ═══════════════════════════════════════════════════════════════════
     1. Die Vorfahrenkette einsammeln
     ═══════════════════════════════════════════════════════════════════
     Vom Streifen aus nach oben, bis zum Dokument. Jedes Element mit
     Tag/Klasse/Id notieren — das ist die Information, die uns bisher
     fehlt und ohne die jeder weitere Versuch Raten bleibt. */

  function kette() {
    var aus = [];
    try {
      var el = document.querySelector('.aa2-strip');
      var tiefe = 0;
      while (el && tiefe < 12) {
        var name = el.tagName ? el.tagName.toLowerCase() : '?';
        if (el.id) name += '#' + el.id;
        if (el.className && typeof el.className === 'string' && el.className.trim()) {
          name += '.' + el.className.trim().split(/\s+/).join('.');
        }
        aus.push(name);
        el = el.parentElement;
        tiefe++;
      }
    } catch (e) { aus.push('Kette nicht lesbar: ' + e); }
    return aus;
  }

  /* ═══════════════════════════════════════════════════════════════════
     2. Asobo-Fehler entschaerfen: hide/panelInvisible abraeumen
     ═══════════════════════════════════════════════════════════════════
     Der in mehreren DevSupport-Threads beschriebene Workaround. Wir
     wenden ihn auf die ganze Kette an, nicht nur auf ein geratenes
     Element — und WIEDERHOLT, weil der Sim die Klassen nach dem ersten
     Bildaufbau erneut setzen kann. */

  function raeumeKlassen() {
    var getroffen = 0;
    try {
      var el = document.querySelector('.aa2-strip');
      var tiefe = 0;
      while (el && tiefe < 12) {
        if (el.classList) {
          if (el.classList.contains('hide')) { el.classList.remove('hide'); getroffen++; }
          if (el.classList.contains('panelInvisible')) { el.classList.remove('panelInvisible'); getroffen++; }
        }
        el = el.parentElement;
        tiefe++;
      }
    } catch (e) {}
    return getroffen;
  }

  /* ═══════════════════════════════════════════════════════════════════
     3. Titelleiste verstecken
     ═══════════════════════════════════════════════════════════════════
     Thomas will sie weg. Wir kennen ihren genauen Namen nicht, also
     suchen wir nach dem, was eine Kopfzeile ausmacht: bekannte
     Klassennamen aus dem MSFS-UI, plus jedes Element in der Kette,
     dessen Textinhalt exakt unser Panelname ist. */

  function versteckeTitel() {
    var weg = 0;
    var muster = [
      '.ingameUiHeader', '.ingameUiHeaderTitle', 'ingame-ui-header',
      '[class*="ingameUiHeader"]', '[class*="panelHeader"]', '[class*="TitleBar"]',
      '[class*="titleBar"]',
    ];
    try {
      for (var i = 0; i < muster.length; i++) {
        var treffer = document.querySelectorAll(muster[i]);
        for (var j = 0; j < treffer.length; j++) {
          treffer[j].style.display = 'none';
          weg++;
        }
      }
      /* Zusaetzlich: irgendein Vorfahren-Geschwister, dessen Text genau
         "AeroACARS" ist — so heisst die Leiste laut Foto. */
      var el = document.querySelector('.aa2-strip');
      var tiefe = 0;
      while (el && tiefe < 12) {
        var eltern = el.parentElement;
        if (eltern && eltern.children) {
          for (var k = 0; k < eltern.children.length; k++) {
            var g = eltern.children[k];
            if (g === el) continue;
            var txt = (g.textContent || '').trim().toLowerCase();
            if (txt === 'aeroacars' && g.children.length <= 3) {
              g.style.display = 'none';
              weg++;
            }
          }
        }
        el = eltern;
        tiefe++;
      }
    } catch (e) {}
    return weg;
  }

  /* ═══════════════════════════════════════════════════════════════════
     4. Eigenes Ziehen
     ═══════════════════════════════════════════════════════════════════
     Ohne Titelleiste gibt MSFS uns keinen Griff mehr — also bauen wir
     einen. Bewegt wird der oberste Vorfahre, der sich ueberhaupt
     bewegen laesst (position absolute/fixed); gibt es keinen, nehmen
     wir den obersten der Kette und setzen ihn selbst auf absolut.

     Bewusst `mousedown/move/up` und NICHT Pointer Events: der Renderer
     ist aelter, und Pointer Events sind dort nicht verlaesslich. Aus
     demselben Grund kein `setPointerCapture`. */

  function findeZiehZiel() {
    try {
      var streifen = document.querySelector('.aa2-strip');
      if (!streifen) return null;

      /* WICHTIG: die Suche beginnt beim ELTERN-Element, nie beim
         Streifen selbst. Im Test gegen einen nachgebauten Sim-Rahmen
         (09.08.2026) hat die erste Fassung genau das falsch gemacht —
         sie fand den Streifen (der in Flow `position: fixed` traegt),
         und dann verschob das Ziehen den Inhalt IM Fenster, statt das
         Fenster zu bewegen. Sichtbar identisch, Wirkung falsch. */
      var el = streifen.parentElement;
      var bester = null;
      var tiefe = 0;
      while (el && tiefe < 12) {
        var tag = el.tagName ? el.tagName.toLowerCase() : '';
        if (tag === 'body' || tag === 'html') break;

        /* Erste Wahl: sieht aus wie ein Fensterrahmen. */
        var kl = (typeof el.className === 'string') ? el.className : '';
        if (/ingameui|frame|panel|window/i.test(kl) || tag === 'ingame-ui') return el;

        /* Zweite Wahl: laesst sich ueberhaupt bewegen. */
        var pos = '';
        try { pos = window.getComputedStyle(el).position; } catch (e) {}
        if (pos === 'absolute' || pos === 'fixed') { if (!bester) bester = el; }

        bester = bester || el;
        el = el.parentElement;
        tiefe++;
      }
      return bester;  // null heisst: der Streifen haengt direkt am Body
    } catch (e) { return null; }
  }

  /* Haengt der Streifen direkt am Body, liegt der Fensterrahmen NICHT in
     unserem Dokument — dann kaeme man nur ueber das Elterndokument
     heran. Ob das erreichbar ist, wissen wir nicht; genau deshalb wird
     es geprueft und BERICHTET statt geraten. */
  function elterndokumentErreichbar() {
    try {
      if (!window.parent || window.parent === window) return 'kein Elternfenster';
      var d = window.parent.document;
      return d ? 'ja (' + (d.body ? d.body.children.length : '?') + ' Kinder)' : 'nein';
    } catch (e) { return 'nein (abgeschottet)'; }
  }

  function ruesteZiehenAus() {
    var ziel = findeZiehZiel();
    if (!ziel) {
      notiz('Ziehen: Rahmen NICHT in unserem Dokument; Eltern ' + elterndokumentErreichbar());
      return false;
    }
    ziehZiel = ziel;

    var greift = false, startX = 0, startY = 0, startL = 0, startT = 0;

    function runter(e) {
      try {
        var r = ziel.getBoundingClientRect();
        var pos = window.getComputedStyle(ziel).position;
        if (pos !== 'absolute' && pos !== 'fixed') {
          ziel.style.position = 'absolute';
          ziel.style.left = r.left + 'px';
          ziel.style.top = r.top + 'px';
        }
        startL = parseFloat(ziel.style.left || r.left) || 0;
        startT = parseFloat(ziel.style.top || r.top) || 0;
        startX = e.clientX; startY = e.clientY;
        greift = true;
        e.preventDefault();
      } catch (err) {}
    }
    function bewegt(e) {
      if (!greift) return;
      try {
        ziel.style.left = (startL + (e.clientX - startX)) + 'px';
        ziel.style.top  = (startT + (e.clientY - startY)) + 'px';
        e.preventDefault();
      } catch (err) {}
    }
    function hoch() { greift = false; }

    try {
      var griff = document.querySelector('.aa2-strip');
      griff.addEventListener('mousedown', runter, true);
      document.addEventListener('mousemove', bewegt, true);
      document.addEventListener('mouseup', hoch, true);
      griff.style.cursor = 'move';
      return true;
    } catch (e) { notiz('Ziehen: Ereignisse nicht setzbar'); return false; }
  }

  /* ═══════════════════════════════════════════════════════════════════
     5. Befund anzeigen — abfotografierbar
     ═══════════════════════════════════════════════════════════════════
     Nur wenn etwas NICHT geklappt hat oder die Kette unbekannt ist.
     Laeuft alles, bleibt das Panel sauber. Nach 90 s blendet die Zeile
     sich aus, damit sie im Flug nicht stoert. */

  function zeigeBefund() {
    try {
      var alt = document.getElementById('aa2-chrome-befund');
      if (alt && alt.parentNode) alt.parentNode.removeChild(alt);
      var d = document.createElement('div');
      d.id = 'aa2-chrome-befund';
      d.style.cssText = 'position:fixed;left:0;right:0;bottom:0;z-index:99;' +
        'background:rgba(10,16,26,0.94);color:#8fb0d8;font:10px monospace;' +
        'padding:3px 6px;white-space:pre-wrap;line-height:1.35;';
      d.textContent = 'AA2-CHROME  ' + BEFUND.join('  |  ') + '\nKETTE  ' + kette().join('  <  ');
      document.body.appendChild(d);
      setTimeout(function () {
        try { if (d.parentNode) d.parentNode.removeChild(d); } catch (e) {}
      }, 90000);
    } catch (e) {}
  }

  /* ═══════════════════════════════════════════════════════════════════
     6. Ablauf
     ═══════════════════════════════════════════════════════════════════
     Der Sim baut seinen Rahmen NACH unserer Seite auf — deshalb nicht
     einmal sofort, sondern gestaffelt nachfassen. Die Klassen kann er
     ausserdem spaeter erneut setzen. */

  function lauf(erster) {
    var k = raeumeKlassen();
    var t = versteckeTitel();
    if (erster) {
      var z = ruesteZiehenAus();
      notiz('Klassen ' + k);
      notiz('Titel ' + t);
      notiz('Ziehen ' + (z ? 'aktiv auf ' + (ziehZiel && ziehZiel.tagName
            ? ziehZiel.tagName.toLowerCase() +
              (ziehZiel.className ? '.' + String(ziehZiel.className).trim().split(/\s+/)[0] : '')
            : '?') : 'FEHLT'));
    }
  }

  function start() {
    try {
      lauf(true);
      /* Nachfassen: 300 ms / 1 s / 3 s. Deckt einen langsam
         aufgebauten Rahmen ab, ohne dauerhaft im Takt zu laufen. */
      setTimeout(function () { lauf(false); }, 300);
      setTimeout(function () { lauf(false); }, 1000);
      setTimeout(function () { lauf(false); zeigeBefund(); }, 3000);
    } catch (e) {}
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', start);
  } else {
    start();
  }
})();
