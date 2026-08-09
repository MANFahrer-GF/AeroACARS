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

  /* Doppel-Lauf-Schutz. QS-Befund vom 09.08.2026: laedt der Sim beim
     Wiederoeffnen des Panels dieselbe Ansicht erneut, ohne das Dokument
     zu verwerfen, haengt jede Kopie ein weiteres Paar Maus-Zuhoerer an —
     und dann bewegt ein Ziehen das Fenster um das Doppelte, Dreifache,
     ... Genau die Fehlerklasse, die uns beim Flow-Widget als "springende
     Anzeige" Stunden gekostet hat; hier vorher zugemacht statt hinterher
     gesucht. Die aeltere Kopie raeumt ihre Zuhoerer selbst ab. */
  var G = (typeof globalThis !== 'undefined') ? globalThis
        : (typeof window !== 'undefined') ? window : this;
  try {
    if (G && G.__aa2ChromeStop && typeof G.__aa2ChromeStop === 'function') G.__aa2ChromeStop();
  } catch (e) {}

  var BEFUND = [];            // was hat gegriffen, was nicht
  var ziehZiel = null;        // das Element, das wir tatsaechlich bewegen
  var abraeumer = [];         // alles, was beim Neustart zurueckzunehmen ist

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
     3. Titelleiste: schlank machen statt verstecken
     ═══════════════════════════════════════════════════════════════════
     KORREKTUR vom 09.08.2026, nach der ersten Befundzeile aus dem Sim:

       KETTE  div.aa2-strip.aa2-quiet < body.aa2-nativ < html
       Ziehen: Rahmen NICHT in unserem Dokument; Eltern kein Elternfenster

     Damit ist zweierlei bewiesen:

     (a) Ueber unserer Seite liegt NICHTS, was wir erreichen koennen. Das
         Fenster gehoert dem Simulator, ausserhalb unseres Dokuments und
         ohne Elternfenster. Ein eigenes Ziehen kann es also gar nicht
         bewegen — der Fehler war nicht im Code, die Sache ist von aussen
         zugenagelt.

     (b) Die Titelleiste dagegen liegt IN unserem Dokument (der Bericht
         zaehlte "Titel 1", und auf dem Foto war sie danach weg). Und
         genau sie ist der Griff, an dem MSFS seine Panels bewegt.

     Daraus folgt der Fehler in meinem ersten Entwurf: indem ich die
     Leiste versteckt habe, habe ich den einzigen vorhandenen Griff
     entfernt — und dann selbst gebaut, was ohne ihn nicht geht. Beide
     Wuensche gleichzeitig ("keine Leiste" UND "verschiebbar") sind so
     nicht erfuellbar.

     Der Ausweg ist ein Kompromiss, der beidem so nah wie moeglich kommt:
     die Leiste BLEIBT, wird aber auf einen schmalen, stillen Streifen
     zusammengezogen — kein Text, kaum Hoehe, dezent. Sichtbar genug zum
     Anfassen, unauffaellig genug, um nicht zu stoeren. Beim Ueberfahren
     mit der Maus tritt sie hervor, damit man sie findet.

     Wer sie doch ganz weg will: HOEHE auf 0 setzen. Dann ist das Panel
     unverschiebbar und sitzt dort, wo die InGamePanelDefinition es
     hinstellt (defaultTop/defaultLeft) — eine legitime Wahl, aber eine
     bewusste. */

  var LEISTE_HOEHE = 14;   // 0 = ganz weg, dann aber unverschiebbar

  function schlankeTitelleiste() {
    var behandelt = 0;
    var kandidaten = [];
    try {
      var muster = ['.ingameUiHeader', '.ingameUiHeaderTitle', 'ingame-ui-header',
        '[class*="ingameUiHeader"]', '[class*="panelHeader"]',
        '[class*="TitleBar"]', '[class*="titleBar"]'];
      for (var i = 0; i < muster.length; i++) {
        var tr = document.querySelectorAll(muster[i]);
        for (var j = 0; j < tr.length; j++) kandidaten.push(tr[j]);
      }
      /* Und jedes Geschwister in der Kette, dessen Text genau unser
         Panelname ist — so hat sich die Leiste im Sim gezeigt. */
      var el = document.querySelector('.aa2-strip');
      var tiefe = 0;
      while (el && tiefe < 12) {
        var eltern = el.parentElement;
        if (eltern && eltern.children) {
          for (var k = 0; k < eltern.children.length; k++) {
            var g = eltern.children[k];
            if (g === el || g.id === 'aa2-chrome-befund') continue;
            var txt = (g.textContent || '').trim().toLowerCase();
            if (txt === 'aeroacars' && g.children.length <= 3) kandidaten.push(g);
          }
        }
        el = eltern;
        tiefe++;
      }

      for (var m = 0; m < kandidaten.length; m++) {
        var c = kandidaten[m];
        if (c.getAttribute && c.getAttribute('data-aa2-leiste') === '1') continue;
        if (LEISTE_HOEHE <= 0) {
          c.style.display = 'none';
        } else {
          /* Nicht `display:none` — der Griff muss anfassbar bleiben.
             Der Text verschwindet ueber die Schriftgroesse statt ueber
             `visibility`, damit das Element seine Klickflaeche behaelt. */
          c.style.height = LEISTE_HOEHE + 'px';
          c.style.minHeight = LEISTE_HOEHE + 'px';
          c.style.lineHeight = LEISTE_HOEHE + 'px';
          c.style.padding = '0';
          c.style.margin = '0';
          c.style.fontSize = '0';
          c.style.overflow = 'hidden';
          c.style.opacity = '0.30';
          c.style.cursor = 'move';
          c.style.borderBottom = '0';
          try {
            c.addEventListener('mouseenter', function () { this.style.opacity = '0.85'; });
            c.addEventListener('mouseleave', function () { this.style.opacity = '0.30'; });
          } catch (e) {}
        }
        if (c.setAttribute) c.setAttribute('data-aa2-leiste', '1');
        behandelt++;
      }
    } catch (e) {}
    return behandelt;
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
    /* Am Bildrand festhalten. QS-Befund vom 09.08.2026: ohne diese
       Schranke laesst sich das Fenster restlos aus dem Bild ziehen — und
       dann ist es WEG. Es gibt ja keine Titelleiste mehr, an der man es
       zurueckholen koennte, und das Toolbar-Symbol blendet es nur an
       derselben unsichtbaren Stelle wieder ein. Ein Pilot, dem das im
       Anflug passiert, hat sein HUD verloren und keine Handhabe.

       Es bleiben immer mindestens RAND Pixel greifbar — an jeder Kante,
       auch oben, damit ein zu weit nach oben geschobenes Fenster nicht
       hinter der Sim-Toolbar verschwindet. */
    var RAND = 90;

    /* Bildgroesse ueber eine KETTE bestimmen, nicht ueber einen Wert.
       QS-Befund vom 09.08.2026: in der Messumgebung lieferte
       `window.innerWidth` glatte 0 — ein Wert, mit dem jede Schranke
       unsinnig wird. Nur weil ein Rueckfallwert dahinterstand, ging es
       gut aus. Coherent GT ist kein normaler Browser; auf einen einzigen
       Messwert zu bauen waere hier leichtsinnig. */
    function bildmass(quer) {
      var kandidaten = quer
        ? [document.documentElement && document.documentElement.clientWidth,
           window.innerWidth, window.screen && window.screen.width, 1920]
        : [document.documentElement && document.documentElement.clientHeight,
           window.innerHeight, window.screen && window.screen.height, 1080];
      for (var i = 0; i < kandidaten.length; i++) {
        if (typeof kandidaten[i] === 'number' && kandidaten[i] > 200) return kandidaten[i];
      }
      return quer ? 1920 : 1080;
    }

    function halteImBild(l, t) {
      try {
        var b = ziel.offsetWidth || 200;
        var bw = bildmass(true), bh = bildmass(false);
        if (l > bw - RAND) l = bw - RAND;
        if (l < RAND - b) l = RAND - b;
        if (t > bh - RAND) t = bh - RAND;
        if (t < 0) t = 0;
      } catch (e) {}
      return [l, t];
    }

    function bewegt(e) {
      if (!greift) return;
      try {
        var lt = halteImBild(startL + (e.clientX - startX), startT + (e.clientY - startY));
        ziel.style.left = lt[0] + 'px';
        ziel.style.top  = lt[1] + 'px';
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
      abraeumer.push(function () {
        try {
          griff.removeEventListener('mousedown', runter, true);
          document.removeEventListener('mousemove', bewegt, true);
          document.removeEventListener('mouseup', hoch, true);
        } catch (e) {}
      });
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
      /* NICHT `position:fixed;bottom:0`. QS-Befund vom 09.08.2026: das
         Panelfenster ist nur rund 130 px hoch — eine am unteren Rand
         festgenagelte Zeile legt sich dann ueber die Datenzeile, also
         genau ueber das, was der Pilot lesen will. Sie haengt jetzt im
         normalen Fluss UNTER dem Streifen: ist Platz da, steht sie da;
         ist keiner da, ist sie einfach nicht sichtbar — statt etwas
         Wichtiges zu verdecken. */
      d.style.cssText = 'position:static;margin-top:2px;' +
        'background:rgba(10,16,26,0.94);color:#8fb0d8;font:10px monospace;' +
        'padding:3px 6px;white-space:pre-wrap;line-height:1.35;';
      d.textContent = 'AA2-CHROME  ' + BEFUND.join('  |  ') + '\nKETTE  ' + kette().join('  <  ');

      /* Direkt HINTER den Streifen haengen, nicht an den Body. Zweiter
         Anlauf beim selben QS-Befund: `position:static` allein reichte
         nicht, weil der Fensterrahmen absolut positioniert ist — am Body
         angehaengt landete die Zeile oben links und lag damit wieder
         ueber der Datenzeile. Als Geschwister des Streifens sitzt sie
         zwangslaeufig darunter. Fehlt der Rahmen (Streifen haengt direkt
         am Body), bleibt der Body die Rueckfallebene. */
      var streifen = document.querySelector('.aa2-strip');
      var wohin = (streifen && streifen.parentNode) ? streifen.parentNode : document.body;
      if (streifen && streifen.nextSibling) wohin.insertBefore(d, streifen.nextSibling);
      else wohin.appendChild(d);
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
    var t = schlankeTitelleiste();
    if (erster) {
      var z = ruesteZiehenAus();
      notiz('Klassen ' + k);
      notiz('Leiste ' + t + '@' + LEISTE_HOEHE + 'px');
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

  try {
    G.__aa2ChromeStop = function () {
      for (var i = 0; i < abraeumer.length; i++) { try { abraeumer[i](); } catch (e) {} }
      abraeumer.length = 0;
    };
  } catch (e) {}

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', start);
  } else {
    start();
  }
})();
