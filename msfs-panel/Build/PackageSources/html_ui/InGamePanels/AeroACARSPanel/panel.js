/*
 * AeroACARS in-sim toolbar panel — v0.1.0 (feasibility-spike build).
 *
 * Talks to the AeroACARS desktop app's EXISTING LAN Remote Control server
 * (client/src-tauri/src/remote/, built for v0.16.0 tablet control) — no new
 * server, no new routes. Two calls, both already live:
 *   - POST /api/auth {pin} -> {token}                          (pairing)
 *   - GET  /ws?token=...    -> pushes `flight_status` @ 1Hz     (live data)
 *   - POST /api/cmd/flight_status                               (poll fallback)
 *   - POST /api/cmd/landing_get_current                         (debrief pull)
 *
 * This panel is loaded from file:// inside MSFS's Coherent GT engine, NOT a
 * normal browser tab — `location.host` is empty there, so every request
 * below uses an absolute http(s)/ws(s) URL. See docs/spec/
 * msfs-ingame-landing-debrief-panel.v1.yaml for the full spec and the
 * feasibility_spike this build exists to run.
 *
 * KNOWN UNVERIFIED ASSUMPTION (the whole point of this build): whether
 * Coherent GT permits fetch()/WebSocket to 127.0.0.1 at all. If pairing or
 * the live connection silently fails in-sim, that IS the spike result —
 * report it, don't assume a code bug first. See LIM-001 in the spec.
 */
(function () {
  'use strict';

  // ---------------------------------------------------------------------
  // Config + persistence
  // ---------------------------------------------------------------------

  var DEFAULT_PORT = 8765;
  var LS_PORT = 'aeroacars_panel_port';
  var LS_TOKEN = 'aeroacars_panel_token';
  var RECONNECT_BASE_MS = 1000;
  var RECONNECT_MAX_MS = 15000;
  var POLL_INTERVAL_MS = 1000;

  function getPort() {
    try {
      var p = parseInt(window.localStorage.getItem(LS_PORT), 10);
      return p > 0 ? p : DEFAULT_PORT;
    } catch (e) {
      return DEFAULT_PORT;
    }
  }
  function setPort(p) {
    try { window.localStorage.setItem(LS_PORT, String(p)); } catch (e) { /* no-op: localStorage may be unavailable in Coherent GT */ }
  }
  function getToken() {
    try { return window.localStorage.getItem(LS_TOKEN) || null; } catch (e) { return null; }
  }
  function setToken(t) {
    try { window.localStorage.setItem(LS_TOKEN, t); } catch (e) { /* no-op */ }
  }
  function clearToken() {
    try { window.localStorage.removeItem(LS_TOKEN); } catch (e) { /* no-op */ }
  }

  function httpBase() { return 'http://127.0.0.1:' + getPort(); }
  function wsBase() { return 'ws://127.0.0.1:' + getPort(); }

  // ---------------------------------------------------------------------
  // Phase helpers — must match phase_to_snake() in client/src-tauri/src/lib.rs
  // ---------------------------------------------------------------------

  var APPROACH_PHASES = ['approach', 'final'];
  var POST_TOUCHDOWN_PHASES = ['landing', 'taxi_in', 'blocks_on', 'arrived', 'pirep_submitted'];

  // ---------------------------------------------------------------------
  // App state
  // ---------------------------------------------------------------------

  var state = {
    mode: 'disconnected', // disconnected | unauthenticated | ready_monitoring | flight_active | approach_monitor | scoring | landing_score | full_debrief
    connected: false,     // WS open (or poll succeeding)
    transport: null,      // 'ws' | 'poll' | null
    status: null,         // last flight_status payload (ActiveFlightInfo) or null
    debrief: null,        // last landing_get_current payload (LandingRecord) or null
    debriefFetchedForFlight: null, // pirep_id we already fetched the debrief for
    lastError: null,
  };

  var els = {}; // filled in initDom()

  // ---------------------------------------------------------------------
  // Auth
  // ---------------------------------------------------------------------

  function pair(pin) {
    setStatusLine('Verbinde...');
    return fetch(httpBase() + '/api/auth', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ pin: pin }),
    })
      .then(function (res) {
        if (!res.ok) throw new Error('HTTP ' + res.status);
        return res.json();
      })
      .then(function (data) {
        if (!data || !data.token) throw new Error('no token in response');
        setToken(data.token);
        startTransport();
      })
      .catch(function (err) {
        state.lastError = String(err && err.message ? err.message : err);
        setMode('unauthenticated');
        setStatusLine('PIN falsch oder AeroACARS nicht erreichbar (' + state.lastError + ')');
      });
  }

  // ---------------------------------------------------------------------
  // Transport — WebSocket primary, polling fallback (SHOULD-009)
  // ---------------------------------------------------------------------

  var ws = null;
  var wsRetryMs = RECONNECT_BASE_MS;
  var pollTimer = null;
  var wsGaveUp = false; // after repeated WS failures, fall back to polling for THIS session

  function startTransport() {
    var token = getToken();
    if (!token) { setMode('unauthenticated'); return; }
    if (!wsGaveUp) {
      connectWs(token);
    } else {
      startPolling(token);
    }
  }

  function connectWs(token) {
    try {
      ws = new WebSocket(wsBase() + '/ws?token=' + encodeURIComponent(token));
    } catch (e) {
      onWsFailed(String(e));
      return;
    }
    ws.onopen = function () {
      wsRetryMs = RECONNECT_BASE_MS;
      state.connected = true;
      state.transport = 'ws';
      if (state.mode === 'disconnected' || state.mode === 'unauthenticated') {
        setMode(state.status ? deriveMode() : 'ready_monitoring');
      }
    };
    ws.onmessage = function (evt) {
      var msg;
      try { msg = JSON.parse(evt.data); } catch (e) { return; }
      if (msg && msg.event === 'flight_status') {
        onFlightStatus(msg.payload);
      }
    };
    ws.onerror = function () { /* onclose follows; handle retry there */ };
    ws.onclose = function (evt) {
      state.connected = false;
      ws = null;
      // 401/403-shaped closes (bad/revoked token) surface as an abnormal
      // close early; treat any close before a successful open as "pairing
      // no longer valid" rather than retrying forever with a dead token.
      if (!state.status && evt && evt.code && evt.code !== 1000 && wsRetryMs >= RECONNECT_MAX_MS) {
        wsGaveUp = true;
        startPolling(getToken());
        return;
      }
      setMode('disconnected');
      scheduleWsRetry();
    };
  }

  function onWsFailed() {
    state.connected = false;
    wsGaveUp = wsRetryMs >= RECONNECT_MAX_MS;
    if (wsGaveUp) {
      startPolling(getToken());
    } else {
      setMode('disconnected');
      scheduleWsRetry();
    }
  }

  function scheduleWsRetry() {
    var token = getToken();
    if (!token) return;
    setTimeout(function () {
      wsRetryMs = Math.min(wsRetryMs * 2, RECONNECT_MAX_MS);
      connectWs(token);
    }, wsRetryMs);
  }

  function startPolling(token) {
    if (!token || pollTimer) return;
    state.transport = 'poll';
    function tick() {
      fetch(httpBase() + '/api/cmd/flight_status', {
        method: 'POST',
        headers: { 'X-AeroACARS-Token': token, 'Content-Type': 'application/json' },
        body: '{}',
      })
        .then(function (res) {
          if (!res.ok) throw new Error('HTTP ' + res.status);
          return res.json();
        })
        .then(function (payload) {
          state.connected = true;
          onFlightStatus(payload);
        })
        .catch(function () {
          state.connected = false;
          setMode('disconnected');
        });
    }
    tick();
    pollTimer = setInterval(tick, POLL_INTERVAL_MS);
  }

  // ---------------------------------------------------------------------
  // flight_status handling + mode derivation
  // ---------------------------------------------------------------------

  function onFlightStatus(payload) {
    state.status = payload || null;
    if (!payload) {
      state.debrief = null;
      state.debriefFetchedForFlight = null;
    }
    var prevMode = state.mode;
    var next = deriveMode();
    setMode(next);
    // Fetch the debrief exactly once per flight, only on the
    // scoring -> landing_score edge (MUST-012a) — never earlier, the
    // pre-finalize numbers are known-wrong (see spec panel_modes.scoring).
    if (next === 'landing_score' && state.debriefFetchedForFlight !== payload.pirep_id) {
      fetchDebrief(payload.pirep_id);
    }
    void prevMode;
    render();
  }

  function deriveMode() {
    if (!state.connected) return 'disconnected';
    if (!getToken()) return 'unauthenticated';
    var s = state.status;
    if (!s) return 'ready_monitoring';
    var phase = s.phase;
    if (POST_TOUCHDOWN_PHASES.indexOf(phase) !== -1) {
      return s.landing_score_finalized ? 'landing_score' : 'scoring';
    }
    if (APPROACH_PHASES.indexOf(phase) !== -1) return 'approach_monitor';
    return 'flight_active';
  }

  function setMode(m) {
    if (m === state.mode) return;
    state.mode = m;
    // A manual "Open Full Debrief" tap can also set mode to 'full_debrief'
    // directly (see wireButtons) — deriveMode() never returns that value
    // on its own, so an automatic flight_status update won't stomp it
    // back to landing_score while the pilot is reading the full debrief,
    // UNLESS the flight itself changes (new pirep_id), which the debrief
    // fetch guard above already resets.
  }

  function fetchDebrief(pirepId) {
    var token = getToken();
    if (!token) return;
    fetch(httpBase() + '/api/cmd/landing_get_current', {
      method: 'POST',
      headers: { 'X-AeroACARS-Token': token, 'Content-Type': 'application/json' },
      body: '{}',
    })
      .then(function (res) {
        if (!res.ok) throw new Error('HTTP ' + res.status);
        return res.json();
      })
      .then(function (record) {
        state.debrief = record;
        state.debriefFetchedForFlight = pirepId;
        render();
      })
      .catch(function (err) {
        state.lastError = String(err && err.message ? err.message : err);
      });
  }

  // ---------------------------------------------------------------------
  // Rendering
  // ---------------------------------------------------------------------

  function fmt(n, digits, fallback) {
    if (n === null || n === undefined || typeof n !== 'number' || isNaN(n)) return fallback || '--';
    return n.toFixed(digits === undefined ? 0 : digits);
  }

  function setStatusLine(text) {
    if (els.statusLine) els.statusLine.textContent = text;
  }

  function show(view) {
    ['view-pair', 'view-monitor', 'view-approach', 'view-scoring', 'view-score', 'view-debrief'].forEach(function (id) {
      var el = document.getElementById(id);
      if (el) el.style.display = id === view ? 'flex' : 'none';
    });
  }

  function render() {
    var s = state.status;
    var d = state.debrief;

    switch (state.mode) {
      case 'disconnected':
        show('view-pair');
        setStatusLine('AeroACARS nicht erreichbar - lauft die App? (Port ' + getPort() + ')');
        break;
      case 'unauthenticated':
        show('view-pair');
        setStatusLine('Verbunden - PIN eingeben (Einstellungen > Fernzugriff in AeroACARS)');
        break;
      case 'ready_monitoring':
        show('view-monitor');
        setText('monitor-subtitle', 'Bereit - wartet auf aktiven Flug');
        break;
      case 'flight_active':
        show('view-monitor');
        setText('monitor-subtitle', 'Ueberwacht Flugtelemetrie (' + (s ? s.phase : '--') + ')');
        break;
      case 'approach_monitor':
        show('view-approach');
        renderApproach(s);
        break;
      case 'scoring':
        show('view-scoring');
        break;
      case 'landing_score':
        show('view-score');
        renderScore(d);
        break;
      case 'full_debrief':
        show('view-debrief');
        renderDebrief(d);
        break;
    }
  }

  function setText(id, text) {
    var el = document.getElementById(id);
    if (el) el.textContent = text;
  }

  function renderApproach(s) {
    if (!s) return;
    var live = s.live || {};
    setText('ap-aircraft', s.aircraft_name || s.aircraft_icao || '--');
    setText('ap-route', (s.dpt_airport || '----') + ' -> ' + (s.arr_airport || '----'));
    setText('ap-fpm', fmt(live.vertical_speed_fpm, 0) + ' fpm');
    setText('ap-g', fmt(live.g_force, 2) + ' G');
    setText('ap-iasgs', fmt(live.ias_kt, 0) + ' | ' + fmt(live.gs_kt, 0) + ' kts');
    setText('ap-pitch', fmt(live.pitch_deg, 1) + '°');
    setText('ap-bank', fmt(live.bank_deg, 1) + '°');
    setText('ap-oat', fmt(live.oat_c, 0) + '°C');
    setText(
      'ap-wind',
      live.wind_dir_deg != null && live.wind_speed_kt != null
        ? fmt(live.wind_dir_deg, 0) + '° ' + fmt(live.wind_speed_kt, 0) + ' kt'
        : '--'
    );
  }

  function scoreBand(label) {
    return (label || '--').toUpperCase();
  }

  function renderScore(d) {
    if (!d) { setText('score-total', '--'); return; }
    var rw = d.runway_match || {};
    setText('score-route', (d.dpt_airport || '----') + ' -> ' + (d.touchdown_airport || d.arr_airport || '----'));
    setText(
      'score-sub',
      (rw.runway_ident ? 'RWY ' + rw.runway_ident : '') +
        (rw.length_ft ? ' · ' + fmt(rw.length_ft, 0) + ' ft' : '')
    );
    setText('score-total', String(d.score_numeric != null ? d.score_numeric : '--'));
    setText('score-band', scoreBand(d.score_label));
    var bar = document.getElementById('score-bar-fill');
    if (bar) bar.style.width = Math.max(0, Math.min(100, d.score_numeric || 0)) + '%';
    setText('score-vs', fmt(d.landing_rate_fpm, 0) + ' fpm');
    setText('score-g', fmt(d.landing_scored_g_force != null ? d.landing_scored_g_force : d.landing_g_force, 2) + ' G');
    setText('score-bounces', String(d.bounce_count != null ? d.bounce_count : 0) + ' bounces');
    setText(
      'score-wind',
      d.headwind_kt != null || d.crosswind_kt != null
        ? fmt(d.headwind_kt, 0) + ' kt HW / ' + fmt(d.crosswind_kt, 0) + ' kt XW'
        : '--'
    );
  }

  function renderDebrief(d) {
    renderScore(d);
    var list = document.getElementById('debrief-subscores');
    if (!list) return;
    list.innerHTML = '';
    var subs = (d && d.sub_scores) || [];
    if (!subs.length) {
      var empty = document.createElement('div');
      empty.className = 'debrief-row muted';
      empty.textContent = 'Keine Detail-Aufschluesselung verfuegbar.';
      list.appendChild(empty);
      return;
    }
    subs.forEach(function (sub) {
      var row = document.createElement('div');
      row.className = 'debrief-row';
      var dot = document.createElement('span');
      dot.className = 'dot dot-' + (sub.band || 'neutral');
      var label = document.createElement('span');
      label.className = 'debrief-label';
      label.textContent = sub.label_key || sub.key || '--';
      var value = document.createElement('span');
      value.className = 'debrief-value';
      value.textContent = sub.value || (sub.score != null ? String(sub.score) : '--');
      row.appendChild(dot);
      row.appendChild(label);
      row.appendChild(value);
      list.appendChild(row);
    });
  }

  // ---------------------------------------------------------------------
  // Wiring
  // ---------------------------------------------------------------------

  function wireButtons() {
    var pairBtn = document.getElementById('pair-submit');
    var pinInput = document.getElementById('pair-pin');
    if (pairBtn && pinInput) {
      pairBtn.addEventListener('click', function () {
        var pin = (pinInput.value || '').trim();
        if (pin.length === 6) pair(pin);
      });
      pinInput.addEventListener('keydown', function (e) {
        if (e.key === 'Enter') pairBtn.click();
      });
      // v0.1.1 (Windows in-sim test, 2026-08-08): Coherent GT otherwise still
      // routes keystrokes to MSFS's own control bindings while this field has
      // "focus" — a text field never actually receives typed input in-sim
      // without explicitly claiming/releasing it via Coherent.trigger. See
      // MSFS DevSupport "Disable Bound Key Events when input into toolbar
      // apps" (confirmed working by a PMDG dev in that thread). Applied but
      // NOT YET independently re-verified live — the test session that wrote
      // this fix crashed (unrelated CTD, cause unknown) before confirming it
      // actually restores typing. Next in-sim test should check this first.
      var notifyCoherentFocus = function () {
        if (window.Coherent && Coherent.trigger) {
          Coherent.trigger('FOCUS_INPUT_FIELD', pinInput.id, '', '', pinInput.value || '', false);
        }
      };
      pinInput.addEventListener('focus', notifyCoherentFocus);
      pinInput.addEventListener('input', notifyCoherentFocus);
      pinInput.addEventListener('blur', function () {
        if (window.Coherent && Coherent.trigger) {
          Coherent.trigger('UNFOCUS_INPUT_FIELD', pinInput.id);
        }
      });
    }
    var openDebrief = document.getElementById('open-full-debrief');
    if (openDebrief) {
      openDebrief.addEventListener('click', function () {
        state.mode = 'full_debrief';
        render();
      });
    }
    var backBtn = document.getElementById('debrief-back');
    if (backBtn) {
      backBtn.addEventListener('click', function () {
        state.mode = 'landing_score';
        render();
      });
    }
    var forgetBtn = document.getElementById('forget-pairing');
    if (forgetBtn) {
      forgetBtn.addEventListener('click', function () {
        clearToken();
        if (ws) { try { ws.close(); } catch (e) { /* no-op */ } }
        if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
        wsGaveUp = false;
        setMode('unauthenticated');
        render();
      });
    }
  }

  // ---------------------------------------------------------------------
  // Boot
  // ---------------------------------------------------------------------

  function boot() {
    wireButtons();
    render();
    startTransport();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', boot);
  } else {
    boot();
  }
})();
