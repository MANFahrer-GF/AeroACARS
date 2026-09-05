# Prüfstand — PIREP-Infrastruktur-Härtung (nachgereicht aus v1.7.17)

Zwei Befunde, die Codex (adversarial) waehrend der langen v1.7.17-QS-Kette
(siehe `docs/qs/v1.7.17-pruefstand.md`, Runden 13 und 17) am Rande fand,
aber die NICHT spezifisch fuer das Departure-Gate-Feature sind — sie
betreffen Infrastruktur, die JEDE PIREP-Einreichung nutzt, seit v0.5.49
bzw. laenger. Beide wurden bewusst NICHT in der v1.7.17-Runde selbst
behoben (Scope), sondern als eigene `spawn_task`-Vorschlaege vorgemerkt
und hier, auf Thomas' Wunsch, nachgereicht bearbeitet.

## Befund 1 — `pirep_queue`-Worker gibt nach ~50 Versuchen fuer immer und still auf

**Gefunden:** Codex, Runde 13 der v1.7.17-QS-Kette (04.09.2026).

Der Hintergrund-Worker (`spawn_pirep_queue_worker`, seit v0.5.49) versucht
einen gequeueten PIREP alle 60 Sekunden erneut einzureichen. Bei
`attempt_count >= MAX_ATTEMPTS` (50) gab er bisher fuer immer auf — nur
eine `tracing::warn!`-Zeile, die kein Pilot je zu sehen bekam. Da ein
Eintrag bereits mit `attempt_count: 3` eingereiht wird (die 3 Live-Retries
via `file_pirep_with_retry` sind schon verbraucht), blieben effektiv nur
~47 weitere Versuche — ein phpVMS-Ausfall oder eine anhaltende Sperre
laenger als ~47 Minuten verlor einen bereits geflogenen, fertigen PIREP
dauerhaft und lautlos.

**Gefixt:**

1. **Kein permanentes Aufgeben mehr.** Nach Ueberschreiten von
   `PIREP_QUEUE_MAX_FAST_ATTEMPTS` (weiterhin 50) wechselt der Worker in
   eine LANGSAMERE Phase (`PIREP_QUEUE_SLOW_RETRY_INTERVAL_SECS`, 30
   Minuten) statt aufzugeben — er versucht es einfach seltener weiter,
   potenziell fuer Stunden, bis der Server sich erholt oder der Pilot/
   Admin manuell eingreift.
2. **Einmalige, sichtbare Warnung statt Stille.** Beim ERSTEN
   Ueberschreiten der Schnellphase schreibt der Worker einen Activity-
   Log-Eintrag ("PIREP konnte nach N Versuchen noch nicht eingereicht
   werden ... bitte beim VA-Team melden") — einmalig (`dead_letter_notified`-
   Flag, persistiert), kein Log-Spam bei jedem weiteren Versuch.
3. **`Retry-After` wird jetzt honoriert.** Ein `ApiError::RateLimited`
   setzt `retry_not_before` auf die vom Server diktierte Wartezeit
   (gedeckelt auf `PIREP_QUEUE_RATE_LIMIT_CAP_SECS`, eine Stunde — siehe
   unten fuer die Begruendung dieser Zahl im Vergleich zur
   Departure-Gate-Arbeit) statt stur jede Minute erneut anzufragen.

**Warum die Rate-Limit-Obergrenze hier LOCKERER ist als bei der
Departure-Gate-Rueckzugs-Meldung** (`RATE_LIMIT_WARTEZEIT_OBERGRENZE_SEC`,
eine Minute, siehe v1.7.17 Runde 11): jene Grenze schuetzt einen
ABGETRENNTEN, nicht persistenten Tokio-Task vor stundenlangem Schlaf.
`pirep_queue` dagegen ist ein DAUERHAFTER, plattenbasierter Hintergrund-
Worker, der ohnehin jede Minute neu nachschaut — ein laengeres Warten
kostet hier nur einen uebersprungenen Tick, kein verlorenes Ergebnis.

**Tests:** `pirep_queue_dead_letter_tests` (8 Tests) — die reine
Entscheidungslogik (`pirep_queue_eintrag_ist_faellig`,
`pirep_queue_dead_letter_warnung_faellig`,
`pirep_queue_slow_phase_wartezeit_sec`) ist vollstaendig unabhaengig vom
Worker-Loop selbst testbar. Der Worker-Loop bleibt (wie der gesamte
Rest dieser Infrastruktur) async/Tauri-gebunden und nicht isoliert
testbar.

## Befund 2 — `save_active_flight`/`flight.stop` ist ein Time-of-Check-to-Time-of-Use-Rennen

**Gefunden:** Codex, Runde 17 der v1.7.17-QS-Kette (04./05.09.2026), beim
Review zweier NEUER `!flight.stop.load(...)`-Wachen — aber zurueckverfolgt
auf den LANGE bestehenden periodischen Checkpoint (Kommentar dort
zitiert bereits das Risiko: "Re-check stop *before* writing").

Es gibt genau EINE Aktiv-Flug-Ablage (fester Pfad, `active_flight_path`,
keine Datei-pro-PIREP wie bei `pirep_queue`). Ueberall, wo
`save_active_flight` nur HINTER einem separaten `if !flight.stop.load(...)`
aufgerufen wurde, lagen Pruefung und Schreiben unsynchronisiert
auseinander: zwischen beiden konnte `flight_end`/`flight_cancel`/
`flight_forget` die Ablage loeschen — der laengst "erlaubte" Schreibzugriff
haette sie danach wiederbelebt, oder schlimmer, die Ablage eines
INZWISCHEN gestarteten neuen Fluges ueberschrieben (derselbe feste Pfad).

**Gefixt:** neues `AppState::persistence_lock` (`std::sync::Mutex<()>`,
`#[derive(Default)]` — keine Aenderung an der einzigen Konstruktionsstelle
noetig). `save_active_flight` prueft `flight.stop` und schreibt jetzt in
EINEM kritischen Abschnitt (`schreiben_falls_aktiv`, eine reine, von
`AppHandle` losgeloeste Hilfsfunktion). `clear_persisted_flight` nimmt
denselben Lock, bevor sie die Ablage loescht. An JEDER der sechs echten
Aufrufstellen war `flight.stop` bereits VOR dem `clear_persisted_flight`-
Aufruf gesetzt (direkt davor oder frueher in derselben Funktion) — es war
KEINE Aenderung an den `stop.store(true, ...)`-Aufrufen selbst noetig,
nur an den beiden Stellen, die tatsaechlich lesen/schreiben/loeschen.

**Zwei resume-Discard-Aufrufstellen (`try_resume_flight`) urspruenglich
bewusst nicht angefasst — diese Einschaetzung war UNVOLLSTAENDIG, siehe
Nachtrag unten.** Die urspruengliche Begruendung ("dort existiert noch gar
kein `ActiveFlight`/`flight.stop`, der Flug wird erst DANACH ins
`Arc<ActiveFlight>` geladen, ohne gleichzeitigen Schreiber kann dort keine
Race auftreten") stimmt fuer `state.active_flight` selbst — uebersah aber,
dass die Ablage-DATEI (ein fester Pfad fuer "den" aktiven Flug, unabhaengig
davon ob er schon im State liegt) waehrend des Awaits vor dem Loeschen
sehr wohl von einem PARALLEL gestarteten neuen Flug beschrieben werden
kann. Der Fix dafuer ist Teil des Nachtrags.

**Tests:** `persistence_lock_tests` (3 Tests) — die reine Synchronisations-
Grundlage (`schreiben_falls_aktiv`) ist unabhaengig von `AppHandle`/Tauri
testbar. Der wichtigste Test (`schreiben_und_loeschen_ueberlappen_sich_nie`)
erzwingt ueber 200 Durchlaeufe mit kuenstlich verzoegerten kritischen
Abschnitten (Threads + `Barrier`) eine echte Wettlaufsituation und prueft,
dass sich Schreiben und Loeschen NIE zeitlich ueberlappen — eine
Gegenprobe (Lock aus der Pruef-Funktion entfernt) bestaetigte, dass der
Test eine echte Regression zuverlaessig faengt.

## Nachtrag (05.09.2026): Codex-Folgefund — Loeschung kannte keinen Eigentuemer

**Gefunden:** Codex, adversarial-Review GEGEN den Commit dieser Sitzung
(alle vier hier dokumentierten Fixes plus den Accident-Klassifikator-Fix
zusammen), unmittelbar nach dem Push. Verdict: `needs-attention`.

Der `persistence_lock` aus Befund 2 serialisiert Schreiben und Loeschen nur
GEGENEINANDER — er sagt nichts darueber, WESSEN Flug gerade an dem einen
festen Pfad (`active_flight_path`) liegt. `flight_cancel` nimmt den alten
Flug per `guard.take()` aus `state.active_flight`, setzt `stop`, und haengt
DANACH an einem echten Await (`client.cancel_pirep(...).await`, ein
Server-Roundtrip). In dieser Luecke ist der State-Slot leer —
`flight_start` kann in dieser Zeit bereits einen NEUEN Flug anlegen und
dessen ersten Checkpoint an denselben Pfad schreiben. Setzt der alte Cancel
danach fort und ruft `clear_persisted_flight`, loeschte diese Funktion
bisher blind — und riss damit den gerade erst gestarteten neuen Flug weg.
Ein Absturz vor dessen naechstem Checkpoint haette dessen Recovery-Zustand
dauerhaft verloren.

Derselbe Spalt betrifft — entgegen der urspruenglichen Einschaetzung oben —
auch die beiden resume-Discard-Stellen in `try_resume_flight`: zwischen
`client.get_pirep(...).await` und der Loeschung kann ein Pilot am
Programmstart bereits manuell einen neuen Flug gestartet haben.

**Gefixt:** neue reine Funktion `sollte_persistierten_flug_loeschen(
erwartete_pirep_id: Option<&str>, tatsaechliche_pirep_id_auf_platte:
Option<&str>) -> bool`. `clear_persisted_flight` bekommt jetzt an JEDER
Aufrufstelle den PIREP der eigenen Flug-Teardown mit (`Some(&pirep_id)`)
und liest — noch INNERHALB desselben `persistence_lock`-Abschnitts, damit
zwischen Lesen und Loeschen kein neuer Schreiber dazwischenkommt — die
tatsaechlich auf der Platte liegende `pirep_id` per `read_persisted_flight`.
Geloescht wird nur, wenn beide uebereinstimmen (oder kein Eigentuemer
erwartet wird, oder die Ablage nicht lesbar/vorhanden ist — dann gibt es
nichts fremdes zu schuetzen).

`flight_forget` musste dafuer leicht umgebaut werden: die `pirep_id` wird
jetzt aus dem `if let Some(flight) = ...`-Block herausgetragen (statt mit
dem Block-Ende zu verschwinden), damit sie beim Aufruf noch verfuegbar ist.

**Tests:** `persistierten_flug_loeschen_eigentuemer_tests` (4 Tests) — die
reine Entscheidung deckt: gleicher Eigentuemer loescht, ANDERER (neuerer)
Eigentuemer loescht NICHT (der eigentliche Befund), kein erwarteter
Eigentuemer loescht (verwaiste Datei ohne State-Gegenstueck), unlesbare/
fehlende Ablage loescht (kein fremder Eigentuemer feststellbar). Gegenprobe
durchgefuehrt: mit der Entscheidung fest auf `true` gesetzt schlaegt
`loescht_nicht_wenn_die_ablage_inzwischen_einem_neueren_flug_gehoert`
zuverlaessig fehl.

## Nicht behoben (bewusst außerhalb des Umfangs)

* **`pirep_queue`s 50-Versuche-Grenze selbst** bleibt als Konzept
  bestehen (nur die Reaktion DANACH ist jetzt anders) — eine vollstaendig
  unbegrenzte, aggressiv nachfassende Warteschlange fuer eine derart
  seltene Situation (>47 Minuten Ausfall) waere unverhaeltnismaessig.
* **Die dep_gate-Reconciliation-Luecke** (v1.7.17, Runden 17-19) bleibt
  wie dort dokumentiert offen — siehe `docs/qs/v1.7.17-pruefstand.md`,
  Abschnitt „Was noch aussteht". Diese Sitzung ergaenzte dort NUR die
  sichtbare Warnung beim Resume (Vorschlag b aus dem Auftrag), nicht die
  Server-Read-Back-Reconciliation (Vorschlag a) — letztere braucht
  serverseitige phpVMS-Kenntnisse ausserhalb dieses Repos (siehe dort).
