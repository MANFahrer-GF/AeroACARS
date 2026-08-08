/*
 * AeroACARS in-sim toolbar panel — v0.2.0 (feasibility-spike build, round 2).
 *
 * Talks to the AeroACARS desktop app's EXISTING LAN Remote Control server
 * (client/src-tauri/src/remote/) via three unauthenticated, loopback-only
 * routes added specifically for this panel (v0.1.1, #msfs-panel):
 *   - GET /panel/status  -> flight_status JSON (poll fallback)
 *   - GET /panel/debrief -> landing_get_current JSON, pulled once per landing
 *   - GET /panel/ws      -> pushes `flight_status` @ 1Hz (live data)
 *
 * v0.2.0 dropped the original PIN-pairing design entirely. That flow reused
 * the LAN Remote Control server's tablet-auth model (bearer token from a
 * typed PIN) — the right call for a TABLET on the LAN, which is a different
 * device that has to prove it's authorized. This panel is not that: it only
 * ever runs alongside AeroACARS on the SAME PC, so there is no cross-device
 * trust question to solve, and the typed PIN was pure friction — it also
 * directly caused round 1's blocker (Coherent GT toolbar-panel text inputs
 * don't get keyboard focus by default; see the FOCUS_INPUT_FIELD comment
 * that used to be here). Dropping the PIN removes that whole bug class: this
 * panel now needs NO typed input and NO keyboard focus at all. Security
 * boundary is now strictly the loopback check server-side (see
 * remote/router.rs's module doc) — nothing this panel can do about that
 * being right or wrong, it just consumes the three routes above.
 *
 * This panel is loaded from file:// inside MSFS's Coherent GT engine, NOT a
 * normal browser tab — `location.host` is empty there, so every request
 * below uses an absolute http(s)/ws(s) URL. See docs/spec/
 * msfs-ingame-landing-debrief-panel.v1.yaml for the full spec and the
 * feasibility_spike this build exists to run.
 *
 * KNOWN UNVERIFIED ASSUMPTION (the whole point of this build): whether
 * Coherent GT permits fetch()/WebSocket to 127.0.0.1 at all. If the panel
 * silently fails to connect in-sim, that IS the spike result — report it,
 * don't assume a code bug first. See LIM-001 in the spec.
 */
(function () {
  'use strict';

  // ---------------------------------------------------------------------
  // Config
  // ---------------------------------------------------------------------

  var DEFAULT_PORT = 8765;
  var LS_PORT = 'aeroacars_panel_port'; // optional override, no UI for it yet
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
    mode: 'disconnected', // disconnected | ready_monitoring | flight_active | approach_monitor | scoring | landing_score | full_debrief
    connected: false,     // WS open (or poll succeeding)
    status: null,         // last flight_status payload (ActiveFlightInfo) or null
    debrief: null,        // last landing_get_current payload (LandingRecord) or null
    debriefFetchedForFlight: null, // pirep_id we already fetched the debrief for
  };

  // ---------------------------------------------------------------------
  // Transport — WebSocket primary, polling fallback (SHOULD-009)
  // ---------------------------------------------------------------------

  var ws = null;
  var wsRetryMs = RECONNECT_BASE_MS;
  var pollTimer = null;
  var wsGaveUp = false; // after repeated WS failures, fall back to polling for THIS session

  function startTransport() {
    if (!wsGaveUp) {
      connectWs();
    } else {
      startPolling();
    }
  }

  function connectWs() {
    try {
      ws = new WebSocket(wsBase() + '/panel/ws');
    } catch (e) {
      onWsFailed();
      return;
    }
    ws.onopen = function () {
      wsRetryMs = RECONNECT_BASE_MS;
      state.connected = true;
      setMode(deriveMode());
      render();
    };
    ws.onmessage = function (evt) {
      var msg;
      try { msg = JSON.parse(evt.data); } catch (e) { return; }
      if (msg && msg.event === 'flight_status') {
        onFlightStatus(msg.payload);
      }
    };
    ws.onerror = function () { /* onclose follows; handle retry there */ };
    ws.onclose = function () {
      state.connected = false;
      ws = null;
      if (wsRetryMs >= RECONNECT_MAX_MS) {
        wsGaveUp = true;
        startPolling();
        return;
      }
      setMode('disconnected');
      render();
      scheduleWsRetry();
    };
  }

  function onWsFailed() {
    state.connected = false;
    wsGaveUp = wsRetryMs >= RECONNECT_MAX_MS;
    if (wsGaveUp) {
      startPolling();
    } else {
      setMode('disconnected');
      render();
      scheduleWsRetry();
    }
  }

  function scheduleWsRetry() {
    setTimeout(function () {
      wsRetryMs = Math.min(wsRetryMs * 2, RECONNECT_MAX_MS);
      connectWs();
    }, wsRetryMs);
  }

  function startPolling() {
    if (pollTimer) return;
    function tick() {
      fetch(httpBase() + '/panel/status')
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
          render();
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
    var next = deriveMode();
    setMode(next);
    // Fetch the debrief exactly once per flight, only on the
    // scoring -> landing_score edge (MUST-012a) — never earlier, the
    // pre-finalize numbers are known-wrong (see spec panel_modes.scoring).
    if (next === 'landing_score' && payload && state.debriefFetchedForFlight !== payload.pirep_id) {
      fetchDebrief(payload.pirep_id);
    }
    render();
  }

  function deriveMode() {
    if (!state.connected) return 'disconnected';
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
    fetch(httpBase() + '/panel/debrief')
      .then(function (res) {
        if (!res.ok) throw new Error('HTTP ' + res.status);
        return res.json();
      })
      .then(function (record) {
        state.debrief = record;
        state.debriefFetchedForFlight = pirepId;
        render();
      })
      .catch(function () { /* next flight_status tick will retry via the guard above */ });
  }

  // ---------------------------------------------------------------------
  // Rendering
  // ---------------------------------------------------------------------

  function fmt(n, digits, fallback) {
    if (n === null || n === undefined || typeof n !== 'number' || isNaN(n)) return fallback || '--';
    return n.toFixed(digits === undefined ? 0 : digits);
  }

  function show(view) {
    ['view-monitor', 'view-approach', 'view-scoring', 'view-score', 'view-debrief'].forEach(function (id) {
      var el = document.getElementById(id);
      if (el) el.style.display = id === view ? 'flex' : 'none';
    });
  }

  function render() {
    var s = state.status;
    var d = state.debrief;

    switch (state.mode) {
      case 'disconnected':
        show('view-monitor');
        setDot(false);
        setText('monitor-subtitle', 'AeroACARS nicht erreichbar - laeuft die App? (Port ' + getPort() + ')');
        break;
      case 'ready_monitoring':
        show('view-monitor');
        setDot(true);
        setText('monitor-subtitle', 'Bereit - wartet auf aktiven Flug');
        break;
      case 'flight_active':
        show('view-monitor');
        setDot(true);
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

  function setDot(ok) {
    var el = document.getElementById('monitor-dot');
    if (el) el.className = 'dot ' + (ok ? 'dot-good' : 'dot-bad');
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
