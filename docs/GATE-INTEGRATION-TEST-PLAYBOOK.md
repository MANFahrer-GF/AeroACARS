# AeroACARS-Integrity-Gate ↔ VPS-Recorder — Integration-Test-Playbook

**Spec:** v0.13.0  
**Repos:**
- VPS Recorder: `aeroacars-live` (Netcup, live.kant.ovh)
- Gate-Modul: `aeroacars-integrity-gate` (private, MANFahrer-GF)
- phpVMS-Host: `german-sky-group.eu`

---

## Übersicht

Das Modul lebt auf dem phpVMS-Host und tauscht HMAC-signierte Calls mit dem Recorder auf der VPS aus:

```
                         ┌──────────────────────┐
                         │  PILOT-CLIENT (Tauri)│
                         └──────┬───────────────┘
                                │ MQTT positions+pirep
                                ▼
       ┌────────────────────────────────────────┐
       │     VPS RECORDER (live.kant.ovh)        │
       │  - integrityValidator                   │
       │  - scoreTrust                           │
       │  - /api/integrity-check/:pirep_id       │◄────┐
       │  - /api/integrity-check/:id/gate-ack    │     │ HMAC
       │  - /api/admin/pireps/:id/review         │     │
       └─────────────────────┬───────────────────┘     │
                             │ HMAC out                │
                             │ webhook decision        │
                             ▼                         │
       ┌────────────────────────────────────────┐     │
       │   PHPVMS HOST (german-sky-group.eu)     │     │
       │  - PirepFiled-Event                     │     │
       │  - AeroACARSIntegrityGate-Modul         │─────┘
       │  - /api/aeroacars-gate/webhook/decision │
       │  - /admin/aeroacars-gate (Review-UI)    │
       └────────────────────────────────────────┘
```

---

## Phase 1 — Deployment-Vorbereitung

### 1.1 Gate-Modul auf phpVMS-Host installieren

Auf `german-sky-group.eu` (oder Testumgebung):

```bash
cd /var/www/phpvms  # oder wo immer phpVMS liegt
cd modules/  # falls modules-Verzeichnis existiert; sonst per nwidart/laravel-modules
git clone git@github.com:MANFahrer-GF/aeroacars-integrity-gate.git AeroACARSIntegrityGate
cd AeroACARSIntegrityGate
composer install --no-dev
cd ../..

# Migrations laufen lassen
php artisan migrate --path=modules/AeroACARSIntegrityGate/database/migrations

# Modul aktivieren (Konvention)
php artisan module:enable AeroACARSIntegrityGate

# Config publishen
php artisan vendor:publish --tag=aeroacars-integrity-gate-config

# Cache räumen
php artisan config:clear && php artisan cache:clear && php artisan route:clear
```

### 1.2 Shared-Secret generieren + auf BEIDEN Hosts setzen

```bash
# Einmal generieren — z.B. auf der VPS
openssl rand -hex 32
# → kopiere den Output, z.B.:
#   3f8e9a2b4c5d6e7f8901a2b3c4d5e6f7890a1b2c3d4e5f60718293a4b5c6d7e8
```

**Auf phpVMS-Host (`/var/www/phpvms/.env`):**
```
AEROACARS_GATE_VPS_URL=https://live.kant.ovh
AEROACARS_GATE_SHARED_SECRET=3f8e9a2b4c5d6e7f8901a2b3c4d5e6f7890a1b2c3d4e5f60718293a4b5c6d7e8
AEROACARS_GATE_DISCORD_WEBHOOK=https://discord.com/api/webhooks/...  # optional
AEROACARS_GATE_AUTO_REPAIR_ON_BOOT=true
```

**Auf VPS (`/etc/aeroacars-recorder/env` oder via systemd-unit):**
```
AEROACARS_GATE_BASE_URL=https://german-sky-group.eu
AEROACARS_GATE_SHARED_SECRET=3f8e9a2b4c5d6e7f8901a2b3c4d5e6f7890a1b2c3d4e5f60718293a4b5c6d7e8
```

phpVMS-Cache + Service-Restart:
```bash
# phpVMS-Host
php artisan config:cache

# VPS
sudo systemctl restart aeroacars-recorder
```

### 1.3 Auto-Repair für alle Ranks initial laufen lassen

Auf phpVMS-Host:
```bash
php artisan aeroacars:integrity-auto-repair-check
# Erwartete Ausgabe wenn drift: "Auto-repaired X rank(s) where ..."
# Wenn schon clean: "All ranks clean."
```

Im phpVMS-Admin: `/admin/aeroacars-gate` öffnen → "Gate-Safety-Status" muss **grün** sein.

---

## Phase 2 — Connectivity-Smoke-Tests von der VPS

### 2.1 Automatischer Test via CLI

Auf der VPS:
```bash
cd /opt/aeroacars-live/recorder
# Source der Service-ENV damit AEROACARS_GATE_SHARED_SECRET da ist
export $(systemctl show aeroacars-recorder -p Environment | tr ' ' '\n' | grep AEROACARS_GATE_SHARED_SECRET | sed 's/Environment=//')
# Oder manuell exportieren

npm run test-gate-integration https://german-sky-group.eu
```

**Erwartet:** 5 Tests grün:
1. ✓ gate base reachable (HTTP 200/404 — TLS + DNS OK)
2. ✓ webhook rejects unsigned request (HTTP 401)
3. ✓ webhook rejects bad HMAC (HTTP 401)
4. ✓ signed webhook passed HMAC, rejected on validation (HTTP 422 / 404 — HMAC OK, pirep_id=0 invalid)
5. ✓ admin status endpoint exists + auth-gated (HTTP 302/401/403)

**Fehler-Diagnose:**

| Test 4 → HTTP 401 | HMAC SECRET MISMATCH | Beide ENVs vergleichen, exakt gleich? |
| Test 1 → unreachable | DNS oder Firewall | `curl -v https://german-sky-group.eu` |
| Test 2 → HTTP 200 | Webhook nicht HMAC-verified | Code-Bug, Webhook-Controller checken |
| Test 4 → HTTP 500 | Server-Error in phpVMS | Laravel-Log: `tail -f storage/logs/laravel.log` |

### 2.2 Manuelle Verbindungs-Probe vom Admin-UI

Im phpVMS-Browser:
1. Login als Admin
2. Öffnen `/admin/aeroacars-gate`
3. "Verbindung testen"-Button mit PIREP-ID `1`
4. Erwartete grüne Box: *"VPS responded with no_telemetry (404) for pirep_id=1 — connection works, this pirep_id has no recorder data."*

---

## Phase 3 — End-to-End mit echtem Test-Flug

### 3.1 Sauberer Flug (verdict=clean)

1. **Pilot**: Sim starten, AeroACARS-Client öffnen, Bid wählen (z.B. GSG123 EDDF→LOWW), Flug fliegen normal.
2. **PIREP filen** via AeroACARS-Client.
3. **Recorder-Log beobachten** (VPS):
   ```
   journalctl -u aeroacars-recorder -f | grep integrity
   ```
   Erwartet: `[integrity] gate-client configured...`, dann beim PIREP-Submit: score_trust_level=trusted.

4. **phpVMS-Log beobachten** (phpVMS-Host):
   ```
   tail -f storage/logs/laravel.log | grep AeroACARS-Gate
   ```
   Erwartet:
   - `AeroACARS-Gate per-PIREP auto-repair` (nur wenn drift)
   - `AeroACARS-Gate verdict applied (in-flow) ... verdict=clean`
   - KEINE Exception/Error-Logs

5. **phpVMS PIREP-Liste** (`/admin/pireps`):
   - PIREP-State: `ACCEPTED`
   - PIREP-Status: phpVMS-default (nicht `INT_HOLD_*`)

### 3.2 Verdächtiger Flug (verdict=untrusted)

Simuliere Anomalie:
- Slew-Mode AN während Cruise (= Position-Δ-Flag)
- ODER: Sim-Pause + Fuel-Reset (= FUEL_RATE + SIM_STATE_RESET_SIGNATURE Flags)

1. Flug + PIREP-File wie oben.
2. **Recorder-Log:** Validator-Flags werden erzeugt, score_trust_level=untrusted.
3. **phpVMS-Log:**
   ```
   AeroACARS-Gate verdict applied (in-flow) ... verdict=untrusted
   ```
4. **phpVMS PIREP:**
   - State: `PENDING` (NICHT ACCEPTED!)
   - Status: `INT_HOLD_UNTR`
5. **Gate-Review-UI** (`/admin/aeroacars-gate/review`):
   - PIREP erscheint mit `held_untrusted` Badge
   - Reasons sichtbar (hard_trigger_gs_zero, etc.)
   - Flag-Types aufgelistet

### 3.3 Admin-Decision Roundtrip

1. **In AeroACARS-Live Webapp** (`https://live.kant.ovh/admin/#/review`):
   - PIREP aus Review-Queue auswählen
   - "Reject" mit Reason "Sim-Crash recovered" klicken
2. **Recorder-Log:**
   ```
   [integrity] gate-queue drained {attempted: 1, succeeded: 1}
   ```
3. **phpVMS-Log:**
   ```
   AeroACARS-Gate webhook: decision applied
     decision_id=dec-XXX target_state=rejected reviewer=admin
   ```
4. **phpVMS PIREP:**
   - State: `REJECTED`
   - Lifecycle-Events gefeuert (Discord-Post falls konfiguriert)
5. **Gate-Review-UI:**
   - PIREP verschwindet aus default-Queue
   - Mit `?history=1` sichtbar mit Decision-Info
6. **Recorder-pireps-Row:**
   - `gate_sync_status='synced'`
   - `decision_committed_at` gesetzt
   - `gate_ack_state='admin_decision'`

---

## Phase 4 — Failure-Mode-Tests (Chaos-Engineering)

### 4.1 Gate temporär down

```bash
# Auf phpVMS-Host
sudo systemctl stop nginx  # oder apache2
```

1. PIREP filen (sollte funktionieren — Recorder ist nicht betroffen)
2. **Recorder-Log:** Verdict-Call schlägt fehl → `gate_outbound_queue` enqueued
3. Service wieder hochfahren: `sudo systemctl start nginx`
4. Warten max. 5 Min (Drainer-Intervall)
5. **Recorder-Log:** `[integrity] gate-queue drained {succeeded: N}`
6. **phpVMS PIREP:** Lifecycle nachträglich gefeuert

### 4.2 Recorder temporär down

```bash
# Auf VPS
sudo systemctl stop aeroacars-recorder
```

1. PIREP filen via phpVMS-API
2. **phpVMS-Log:** `AeroACARS-Gate: VPS unreachable, PENDING-Hold`
3. **phpVMS PIREP:** State `PENDING`, Status `INT_HOLD_TIMEOUT`
4. Recorder restart: `sudo systemctl start aeroacars-recorder`
5. Warten max. 5 Min (gate AsyncVerdictRetryQueue)
6. **phpVMS-Log:** `AeroACARS-Gate verdict applied (off-flow)`
7. **phpVMS PIREP:** State korrigiert je Verdict (ACCEPTED bei clean, bleibt PENDING bei review)

### 4.3 HMAC-Secret-Mismatch (Negativ-Test)

```bash
# Auf phpVMS-Host nur — Secret falsch setzen
echo "AEROACARS_GATE_SHARED_SECRET=wrong-secret-for-testing-only" >> .env.test
php artisan config:cache
```

1. PIREP filen
2. **phpVMS-Log:** `AeroACARS-Gate webhook: hmac_invalid`
3. ALLE PIREPs landen in `INT_HOLD_ERROR` weil verdict-call 401 zurück
4. Secret korrigieren + config:cache, dann sind neue PIREPs wieder OK
5. Alte PIREPs müssen manuell via Admin-Webapp-Review entschieden werden

---

## Phase 5 — Production-Health-Monitoring

Im laufenden Betrieb regelmäßig prüfen:

### Auf phpVMS-Host
```bash
# Status-JSON (braucht admin-Cookie via curl --cookie)
curl https://german-sky-group.eu/admin/aeroacars-gate/status.json | jq

# Erwartete Indikatoren:
#   auto_repair.gate_safe: true                  ← MUSS true sein
#   auto_repair.unsafe_count: 0                  ← MUSS 0 sein
#   retry_queue.abandoned: < 5                   ← falls > 0, investigieren
#   callback_queue.abandoned: < 5                ← dito
#   claims.in_progress: < 10                     ← falls hoch, Worker-Crash?
```

### Auf VPS
```bash
# Recorder-side gate-queue status
curl -H "Authorization: Bearer $ADMIN_TOKEN" https://live.kant.ovh/api/admin/integrity/gate-queue/status | jq

# Erwartete Indikatoren:
#   pending: < 10
#   abandoned: < 5
```

### Daily-Cron-Verifikation

phpVMS-Cron-Tab muss enthalten:
```cron
0 4 * * * cd /var/www/phpvms && php artisan schedule:run >> /dev/null 2>&1
```

Im Schedule (`app/Console/Kernel.php`):
```php
$schedule->command('aeroacars:integrity-auto-repair-check')->dailyAt('04:00');
$schedule->command('aeroacars:integrity-retry-queue-drain')->everyFiveMinutes();
$schedule->command('aeroacars:integrity-recorder-callback-drain')->everyTwoMinutes();
```

---

## Troubleshooting-Cheatsheet

| Symptom | Wahrscheinliche Ursache | Fix |
|---|---|---|
| Alle PIREPs landen in `INT_HOLD_TIMEOUT` | Recorder unreachable vom phpVMS-Host | `curl https://live.kant.ovh` von phpVMS aus, Firewall |
| Alle PIREPs in `INT_HOLD_ERROR` | HMAC SECRET MISMATCH | Beide ENVs prüfen, exakt gleich? `config:cache` nach Änderung |
| Alle PIREPs sofort ACCEPTED | `auto_approve_acars=true` UND Auto-Repair down | `aeroacars:integrity-auto-repair-check` manuell, dann Discord-Alert investigieren |
| `INT_HOLD_PENDING` Stuck | not_ready-Loop, TD-Event nie angekommen | Pilot-Client-Log prüfen, Session-last_seen schauen |
| `INT_REJ_REPAIR_FAIL` | AutoRepair-Failure (DB-Lock, Plugin-Conflict) | Laravel-Log, manuelle DB-Korrektur, then `php artisan aeroacars:integrity-claim-cleanup` |
| 409 `claim_incomplete` von Webhook | Worker hängt oder gecrashed | `ps aux \| grep queue:work`, ggf `aeroacars:integrity-claim-cleanup --decision-id=...` |
| Recorder-Review-Tab inkonsistent zu phpVMS | Callback-Queue abandoned | `aeroacars:integrity-recorder-callback-drain --limit=200` manuell laufen |
