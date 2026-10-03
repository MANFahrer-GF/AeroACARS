/*
 * AeroACARS — Rahmen des NATIVEN MSFS-2024-Panels (v0.12.0, 03.10.2026).
 * Ersetzt panel-chrome.js (dessen Klassen-Entfernung wird nicht mehr
 * gebraucht; die Datei bleibt als Fundstelle im Ordner, wird aber nicht
 * mehr geladen).
 *
 * Bauart wie die Standard-Panels des Sims: eigenes Wurzel-Element, das
 * von TemplateElement erbt, + checkAutoload(). Danach:
 *   fit():    host unter die Titelleiste legen (Hoehe/Seiten messen)
 *   shrink(): Rahmen auf Titelleiste + Streifen verkleinern, Rest des
 *             Bildschirms bleibt Sim (ui.sendRect() meldet das Rechteck).
 * Alles in try/catch; scheitert etwas, bleibt das Fenster nutzbar.
 */
(function () {
  'use strict';
  var FALLBACK_TITLE_PX = 34;
  var TICK_MS = 500;

  function log(m) { try { console.log('[AeroACARS-Panel] ' + m); } catch (e) {} }

  if (typeof TemplateElement !== 'function') {
    log('TemplateElement fehlt - Framework-Skripte nicht geladen?');
    return;
  }

  class IngamePanelAeroACARS extends TemplateElement {
    constructor() {
      super();
      this.stop = null;
      this.lastFit = '';
    }
    connectedCallback() {
      try { super.connectedCallback(); } catch (e) { log('super.connectedCallback: ' + e); }
      this.start();
    }
    disconnectedCallback() {
      if (this.stop) this.stop();
      this.stop = null;
      try { if (super.disconnectedCallback) super.disconnectedCallback(); } catch (e) {}
    }
    ui() { return this.querySelector('ingame-ui'); }
    header() {
      var ui = this.ui();
      if (!ui) return null;
      var c = ui.querySelectorAll('ingame-ui-header, .ingameUiHeader, [class*="Header"]');
      for (var i = 0; i < c.length; i++) {
        var h = c[i].getBoundingClientRect().height;
        if (h > 0 && h < 120) return c[i];
      }
      return null;
    }
    fit() {
      var ui = this.ui(), host = this.querySelector('#aa2-host');
      if (!ui || !host) return;
      var top = FALLBACK_TITLE_PX, left = 0, right = 0;
      var h = this.header();
      if (h) {
        var u = ui.getBoundingClientRect(), r = h.getBoundingClientRect(), cs = getComputedStyle(ui);
        top = Math.max(0, Math.round(r.bottom - u.top - (parseFloat(cs.borderTopWidth) || 0)));
        left = Math.max(0, Math.round(r.left - u.left - (parseFloat(cs.borderLeftWidth) || 0)));
        right = Math.max(0, Math.round(u.right - r.right - (parseFloat(cs.borderRightWidth) || 0)));
      }
      var key = top + '/' + left + '/' + right;
      if (key !== this.lastFit) {
        this.lastFit = key;
        host.style.top = top + 'px';
        host.style.left = left + 'px';
        host.style.right = right + 'px';
        log('host top ' + top + 'px, Seiten ' + left + '/' + right + 'px');
      }
    }
    shrink() {
      var ui = this.ui(), host = this.querySelector('#aa2-host');
      var strip = host && host.querySelector('.aa2-strip');
      if (!ui || !host || !strip || strip.offsetHeight <= 0) return;
      var cs = getComputedStyle(ui);
      var px = function (v) { return parseFloat(v) || 0; };
      var want = Math.round(px(cs.borderTopWidth) + px(host.style.top) + strip.offsetHeight +
                            px(cs.paddingBottom) + px(cs.borderBottomWidth)) + 'px';
      if (ui.style.height !== want) {
        ui.style.height = want;
        ui.style.maxHeight = want;
        ui.style.setProperty('--min-height', want);
        if (typeof ui.sendRect === 'function') ui.sendRect();
        log('Rahmen ' + want + ' hoch');
      }
    }
    start() {
      if (this.stop) this.stop();
      var self = this;
      var tick = function () { try { self.fit(); self.shrink(); } catch (e) { log('tick: ' + e); } };
      var ui = this.ui();
      window.addEventListener('resize', tick);
      if (ui) ui.addEventListener('panelActive', tick);
      requestAnimationFrame(tick);
      var iv = window.setInterval(tick, TICK_MS);
      this.stop = function () {
        window.removeEventListener('resize', tick);
        if (ui) ui.removeEventListener('panelActive', tick);
        window.clearInterval(iv);
      };
    }
  }

  if (!window.customElements.get('ingamepanel-aeroacars')) {
    window.customElements.define('ingamepanel-aeroacars', IngamePanelAeroACARS);
  }
  if (typeof checkAutoload === 'function') checkAutoload();
})();
