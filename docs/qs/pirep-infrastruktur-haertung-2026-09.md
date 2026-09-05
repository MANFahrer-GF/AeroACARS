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

## Nachtrag #2 (05.09.2026): dritte Codex-Runde — zwei weitere Befunde

Dieselbe adversarial-Review, die Befund 1 im Nachtrag oben fand, ging danach
noch einmal ueber den ANGEWACHSENEN Diff (alle Fixes dieser Sitzung
zusammen, inkl. Nachtrag #1) und fand zwei weitere, unabhaengige Luecken.

### Befund 3 — `flight_cancel`/`flight_forget` hatten die Eigentuemer-Pruefung, aber nicht den State-Lock

Der Ownership-Check aus Nachtrag #1 verhindert nur, dass eine Loeschung eine
fremde DATEI trifft. Er sagt nichts darueber, dass `flight_cancel` (Await
vor der Loeschung) und `flight_forget` (kein Await, aber Tauri-Kommandos
laufen auf einem Mehr-Thread-Runtime — echte Parallelitaet, kein Await
noetig) einen NEUEN `flight_start`/`flight_adopt` ueberlappen koennten,
waehrend `state.active_flight` schon leer ist. `flight_end` haelt fuer
genau dieses Fenster schon seit v0.20.x `FlightSetupGuard` — `flight_cancel`
und `flight_forget` taten das nicht.

**Gefixt:** beide nehmen jetzt denselben `FlightSetupGuard` fuer ihre
gesamte Teardown-Dauer (kein `disarm()` noetig, da beide Pfade nie wieder
einen Flug in `active_flight` zurueckschreiben). Ein gleichzeitiger
`flight_start`/`flight_adopt` bekommt in dem Fall den bestehenden
"another flight start or adopt is already in progress"-Fehler und der
Pilot versucht es einfach nochmal — ein akzeptabler Trade-off gegen
Datenverlust.

### Befund 4 — `pending_pireps/` kennt keinen Piloten

Die unbegrenzte Slow-Phase aus Befund 1 macht das Zeitfenster, in dem ein
ANDERER Pilot auf derselben Maschine eingeloggt sein kann
(`phpvms_logout` erlaubt das ausdruecklich), potenziell beliebig lang statt
auf ~47 Minuten begrenzt. Ohne Eigentuemer-Pruefung haette der Worker Pilot
As PIREP mit Pilot Bs Credentials eingereicht — Erfolg: falsch zugeordnet;
403/404 (als "nicht transient" eingestuft): geloescht, Pilot As Flug
dauerhaft verloren.

**Gefixt:** neue `Client::identity_fingerprint()` (api-client-Crate) — ein
nicht-kryptographischer, aber fuer Gleichheitspruefungen ausreichender Hash
aus Basis-URL + API-Key, NIE der Rohschluessel selbst. `QueuedPirep` traegt
jetzt `owner_identity: Option<String>` (beim Einreihen gesetzt). Reine
Entscheidungsfunktion `pirep_queue_eintrag_gehoert_aktuellem_piloten`: ein
Eintrag wird nur bearbeitet, wenn sein Eigentuemer exakt dem GERADE
eingeloggten Client entspricht. Alt-Eintraege ohne das Feld (`None`) zaehlen
als "Eigentuemer unbekannt" — NICHT als "niemandes, also frei" — und werden
wie ein Fremd-Eigentuemer in Quarantaene belassen (weder eingereicht noch
geloescht, kein Versuchszaehler/Retry-Zeit angefasst).

**Tests:** `pirep_queue_eigentuemer_tests` (3 Tests) + 4 neue Tests fuer
`Client::identity_fingerprint` im api-client-Crate (gleiche Verbindung →
gleicher Fingerabdruck, anderer Key/andere VA → anderer Fingerabdruck,
Fingerabdruck enthaelt den Rohschluessel nicht). Gegenprobe fuer die
Eigentuemer-Entscheidung durchgefuehrt: mit `None` absichtlich als "frei"
behandelt schlaegt `unbekannter_eigentuemer_gilt_nicht_als_frei` zuverlaessig
fehl.

## Nachtrag #3 (05.09.2026): vierte Codex-Runde — die Eigentuemer-Pruefung selbst hatte zwei Ausfaelle

Adversarial-Review gegen den ANGEWACHSENEN Diff (alle bisherigen Nachtraege
zusammen) fand zwei "no-ship"-Probleme GENAU in der Eigentuemer-Pruefung
aus Befund 4 — beide haetten fuer sich genommen wartende PIREPs dauerhaft
verwaisen lassen, also exakt das Gegenteil dessen bewirkt, was der Fix
verhindern sollte.

**Ausfall A — der Upgrade-Moment selbst.** Jeder VOR diesem Fix bereits
gequeuete PIREP deserialisiert mit `owner_identity: None` (der
`#[serde(default)]`). Die reine Fingerabdruck-Pruefung aus Befund 4 laesst
`None` nie durch — ein bereits wartender, fertig geflogener PIREP waere
beim ersten Tick nach dem Update fuer immer in Quarantaene gelandet, auch
wenn derselbe Pilot, der ihn eingereiht hat, noch eingeloggt ist.

**Ausfall B — API-Key-Rotation.** Der Fingerabdruck haengt am Rohschluessel;
rotiert ein Pilot seinen eigenen API-Key (phpVMS-Einstellungen), aendert
sich der Fingerabdruck, obwohl es derselbe Account bleibt — jeder zuvor
gequeuete Eintrag faellt danach dauerhaft durch dieselbe Pruefung.
Verschaerft durch einen dritten, unabhaengigen Fund: die erste Fassung von
`identity_fingerprint` nahm `std::collections::hash_map::DefaultHasher`,
dessen Ausgabe laut eigener std-Doku NICHT ueber Rust-/Compiler-Versionen
stabil ist — waere also bei JEDEM Client-Update ohnehin fuer ALLE
Eintraege, nicht nur nach einer Key-Rotation, neu ausgefallen.

**Gefixt (zwei Teile):**

1. `identity_fingerprint` nimmt jetzt FNV-1a von Hand statt `DefaultHasher`
   — deterministisch fuer immer, weil es eigener Code ist, nicht
   std-internes, ausdruecklich unspezifiziertes Verhalten.
2. Vor der endgueltigen Quarantaene fragt der Worker EINMAL pro Tick
   serverseitig nach (`GET /api/user/pireps?state=0` via
   `get_user_pireps_in_progress()`, bereits serverseitig auf den
   eingeloggten Piloten gefiltert): steht der Eintrag dort noch als
   IN_PROGRESS, gehoert er unabhaengig vom lokalen Fingerabdruck demselben
   Account — der Worker schreibt den aktuellen Fingerabdruck auf den
   Eintrag (reklamiert ihn) statt ihn verwaisen zu lassen. Ein echter
   Fremd-Eigentuemer (Pilot A eingeloggt als Pilot B) taucht in Pilot Bs
   eigener, serverseitig gefilterter Liste nie auf und bleibt korrekt in
   Quarantaene.

**Tests:** golden-value-Test fuer `identity_fingerprint` (fest verdrahteter
Erwartungswert, faengt einen kuenftigen Ruecktausch auf einen std-Hasher —
ein reiner "gleiche Eingabe -> gleiche Ausgabe"-Test haette das NICHT
gefangen, weil er auch mit `DefaultHasher` innerhalb eines Testlaufs
bestanden haette). Quelltext-Wächter fuer den Reklamier-Pfad im
Worker-Loop (Tauri-/async-gebunden, nicht isoliert testbar) — verlangt
sowohl den Server-Aufruf als auch das Umschreiben von `owner_identity`.
**Eigene Lehre aus dieser Runde:** die erste Fassung dieses Wächters
suchte per `.find("fn spawn_pirep_queue_worker")` nach der Zielfunktion —
genau dieses Literal stand aber bereits VORHER im eigenen Testcode (als
Teil der Fehlermeldung), `include_str!` liest die gesamte Datei
einschliesslich dieser Zeile, also fand sich der Test beinahe selbst statt
der echten Funktion. Behoben nach demselben Muster wie
`vor_der_server_auskunft_wird_nichts_geloescht` weiter oben in dieser
Datei: die Suchnadel wird aus zwei Teilen zur Laufzeit zusammengesetzt,
damit sie als zusammenhaengendes Literal nirgends im eigenen Testcode
steht. Gegenprobe fuer beide Haelften des Wächters durchgefuehrt.

## Nachtrag #4 (05.09.2026): fuenfte Codex-Runde — Eigentuemer-Identitaet war nicht am Flug festgemacht

Adversarial-Review gegen den weiter angewachsenen Diff fand: die
Eigentuemer-Fixes aus Nachtrag #3 leiteten den Fingerabdruck bei JEDER
Speicherung/Einreihung frisch aus `current_client(&state)` ab — also aus
WER GERADE eingeloggt ist, nicht aus wem der Flug tatsaechlich gehoert.
`phpvms_logout` leert `state.client`, aber ausdruecklich NICHT
`state.active_flight` ("ein anderer Pilot kann sich auf derselben Maschine
anmelden"). Zwei konkrete Ausfaelle daraus:

* **`try_resume_flight`** fragte den PIREP direkt per ID ab
  (`client.get_pirep`), bevor irgendeine Eigentuemer-Pruefung lief. Meldet
  phpVMS dabei `NotFound` fuer den falschen (inzwischen eingeloggten)
  Account, loeschte der Code Pilot As Ablage dauerhaft. Antwortet phpVMS
  stattdessen `Ok` (falls die Abfrage nicht pro Account beschraenkt ist),
  haette der Code Pilot As Flug unter Pilot Bs Session wiederaufgenommen.
* **`pirep_queue`s Einreihung** (Nachtrag #3) stempelte den Fingerabdruck
  des Piloten, der GERADE eingeloggt ist, wenn `flight_end` laeuft — nicht
  den des Piloten, der den Flug gestartet hat. Loggt sich Pilot A aus
  (ohne den Flug zu beenden) und Pilot B ein, bevor ein transienter
  Filing-Fehler den PIREP in die Queue schiebt, traegt der Eintrag Pilot Bs
  Fingerabdruck — der Worker haette ihn spaeter fuer B, nicht fuer A,
  behandelt.

**Gefixt:** neues `AppState::active_flight_owner_identity` (`Mutex<Option
<String>>`, Default-abgeleitet — keine Aenderung an der einzigen
`AppState`-Konstruktionsstelle noetig, im Unterschied zu `ActiveFlight` mit
seinen 22 Konstruktionsstellen). Wird EINMAL gesetzt, in `flight_start`,
`flight_adopt` und beim erfolgreichen Resume — direkt wenn der Flug in
`state.active_flight` installiert wird — aus dem GERADE eingeloggten
Client. `PersistedFlight` traegt jetzt `owner_identity: Option<String>`
(bei jedem Speichern aus `active_flight_owner_identity` uebernommen, nicht
aus `current_client`), `pirep_queue::QueuedPirep::owner_identity` liest
beim Einreihen ebenfalls aus `active_flight_owner_identity` statt aus
`current_client(&state)`.

`try_resume_flight` prueft die Eigentuemerschaft jetzt VOR der direkten
PIREP-ID-Abfrage: stimmt der Fingerabdruck ueberein, laeuft die bestehende
Logik unveraendert. Stimmt er nicht ueberein ODER fehlt er (Alt-Ablage von
vor diesem Feld) — derselbe Reklamier-Weg wie bei `pirep_queue`: einmalig
serverseitig nachfragen (`get_user_pireps_in_progress`, bereits
serverseitig auf den eingeloggten Piloten gefiltert), ob der PIREP
TROTZDEM zum aktuellen Account gehoert. Wenn ja: reklamieren (Fingerabdruck
neu schreiben), normal fortfahren. Wenn nein: Resume ueberspringen, Ablage
unangetastet lassen (weder Loeschen noch Uebernehmen) — dieselbe
Quarantaene-Philosophie wie bei der Warteschlange.

**Tests:** ein Quelltext-Wächter (`die_eigentuemer_pruefung_laeuft_vor_der_
server_abfrage_per_id`) verlangt, dass die Eigentuemer-Pruefung textuell VOR
der direkten `client.get_pirep`-Abfrage in `try_resume_flight` steht.
Gegenprobe durchgefuehrt: Pruefung entfernt (Zeilen geloescht, ein einzelner
Ersatz fuer die dadurch fehlende Variable eingefuegt, damit es weiter
kompiliert) — der Wächter schlaegt zuverlaessig fehl.

### Ein zweiter, unabhaengiger Fund derselben Runde: G-Kraft-Merge kam zu spaet fuer den Klassifikator

Dieselbe Codex-Runde fand ausserdem, dass der Fruehdump-Fix aus
`docs/qs/accident-klassifikator-bounce-aufprall-2026-09.md` selbst noch
eine Ordnungs-Luecke hatte — Details dort im Nachtrag. Kurzfassung: der
Merge von `peak_g_post_500ms` nach `landing_peak_g_force` lief bisher NUR
NACH dem `apply_accident_heuristic`-Aufruf (Zeile ~180 weiter unten, nach
`drop(stats)` + Re-Lock) — der Klassifikator sah damit fuer GENAU den
Touchdown, den er bewerten soll, entweder `None` (nach dem Klettern-Reset
aus dem urspruenglichen Fruehdump-Fix) oder einen veralteten Wert. Neue
Hilfsfunktion `peak_g_force_verschmelzen` (aus den zwei bisherigen
Kopien der Merge-Logik zusammengezogen), jetzt zusaetzlich VOR dem
Klassifikator-Aufruf angewendet.

## Nachtrag #5 (05.09.2026): sechste Codex-Runde — der erste Checkpoint und die lebenden API-Aufrufe

Adversarial-Review gegen den weiter angewachsenen Diff fand zwei letzte
Luecken in derselben Eigentuemer-Kette.

**Befund A — der ERSTE Checkpoint eines neuen Fluges trug noch den
Eigentuemer des vorigen.** Alle drei Flug-Erzeugungspfade (`flight_start`,
`flight_adopt`, manueller Plan) rufen `save_active_flight` UNMITTELBAR nach
dem Bauen des `ActiveFlight`-Objekts auf — die Eigentuemer-Zuweisung aus
Nachtrag #4 sass aber erst SPAETER, wenn der Flug in `state.active_flight`
installiert wird. Da `active_flight_owner_identity` beim Beenden eines
Fluges nicht geleert wird, haette dieser allererste Checkpoint den
Fingerabdruck des VORHERIGEN Piloten getragen (oder gar keinen, beim
allerersten Flug ueberhaupt). Ein Absturz genau in diesem schmalen Fenster
haette den falschen Eigentuemer auf der Platte eingefroren.

**Gefixt:** die Eigentuemer-Zuweisung wandert an alle drei Erzeugungspfaden
VOR den jeweils ersten `save_active_flight`-Aufruf, direkt nachdem
`client` (der gerade authentifizierte Account) feststeht.

**Befund B — die Eigentuemer-Bindung schuetzte nur, was auf der Platte
liegt, nicht die laufenden API-Aufrufe.** Alle bisherigen Fixes (Resume-
Ablage, PIREP-Queue) greifen nur an den Stellen, die den `owner_identity`-
Wert tatsaechlich lesen. Positions-Updates, normales/manuelles PIREP-
Filing, Cancel und MQTT-Finalisierung lesen dagegen bei JEDEM Aufruf frisch
`current_client(&state)` — waere waehrend eines laufenden Fluges bereits
ein anderer Pilot eingeloggt, haetten ALLE diese Aufrufe klaglos mit dessen
Credentials gearbeitet, unabhaengig vom Eigentuemer-Feld. Jede einzelne
dieser Aufrufstellen im gesamten Flug-Lebenszyklus abzusichern waere eine
sehr breite Aenderung mit hohem Streu-Risiko gewesen.

**Gefixt an der Wurzel statt an jeder Aufrufstelle:** `phpvms_logout` lehnt
jetzt ab, solange ein Flug aktiv ist ("bitte zuerst beenden oder
abbrechen"). `phpvms_login` prueft zusaetzlich, ob ein bereits laufender
Flug einem ANDEREN Account gehoert als dem neu eingegebenen Schluessel —
ein Re-Login DESSELBEN Piloten (z. B. nach einem abgelaufenen Key) bleibt
ausdruecklich erlaubt, ein Kontowechsel waehrend eines fremden laufenden
Fluges wird verweigert. Damit kann `state.client` waehrend eines Fluges
gar nicht mehr auf einen anderen Account wechseln — die Wurzel des
gesamten Befundklasse ist verriegelt, ohne dass jede einzelne der vielen
API-Aufrufstellen einzeln geprueft werden musste.

**Tests:** Quelltext-Wächter fuer beide Riegel (`logout_prueft_
aktiven_flug_vor_dem_leeren_von_state_client`,
`login_prueft_den_flug_eigentuemer_vor_dem_ueberschreiben_von_state_
client`), beide gegen Whitespace/Zeilenumbrueche gehaertet (`ohne_
leerraum`) — ein rustfmt-Lauf brach die erste, naive Fassung sofort, weil
er die mehrteilige Bedingung im Logout-Riegel auf mehrere Zeilen umbrach.
Gegenprobe fuer beide Riegel durchgefuehrt: Reihenfolge vertauscht, beide
Wächter schlagen zuverlaessig fehl.

**Eigene Lehre aus dieser Runde:** gleich zwei Mal in Folge brach ein
eigener Doc-Kommentar (nicht der Code selbst) den Klammer-Zaehler von
`tests/angeschlossen.rs`, weil er ein einzelnes Klammerzeichen in
Backticks zitierte, um Code zu erklaeren. Ab jetzt: Code-Fragmente in
Kommentaren innerhalb dieser Datei nie mit einer einzelnen, unausgeglichenen
`{` oder `}` zitieren — lieber umschreiben.

## Nachtrag #6 (05.09.2026): siebte Codex-Runde — der Fingerabdruck selbst war die falsche Grundlage

Adversarial-Review gegen den weiter angewachsenen Diff verwarf die
Identitaets-QUELLE aus allen bisherigen Nachtraegen — nicht nur einzelne
Aufrufstellen. Drei zusammenhaengende Befunde:

**Befund A (medium) — API-Key-Rotation sperrte den eigenen Piloten aus.**
`Client::identity_fingerprint()` hashte Basis-URL + API-Key. Eine legitime
Rotation DESSELBEN Piloten (z. B. nach einem abgelaufenen Key neu erzeugt)
aendert den Hash, obwohl der Account derselbe bleibt — kombiniert mit dem
Logout-Riegel aus Nachtrag #5 (kein Logout waehrend ein Flug aktiv ist)
und dem Login-Riegel (lehnt eine "andere" Identitaet ab) waere der Pilot
eingesperrt gewesen: weder aus- noch mit dem neuen Key wieder einloggen,
ohne die App neu zu starten oder den Flug aufzugeben.

**Befund B (medium) — ein abgeschlossener Flug sperrte JEDEN naechsten
Login dauerhaft.** Der Login-Riegel aus Nachtrag #5 prüfte
`active_flight_owner_identity` OHNE zu pruefen, ob ueberhaupt noch ein Flug
aktiv ist — dieses Feld wird beim Beenden/Abbrechen eines Fluges nicht
geleert. Nach dem ALLERERSTEN abgeschlossenen Flug haette jeder folgende
Login (auch desselben Piloten nach normalem Logout) mit "ein anderer
Account ist aktiv" abgelehnt, obwohl `active_flight` laengst leer war.

**Befund C (high) — Login/Flugstart konnten trotzdem interleaven.** Der
Login-Riegel prieft und gibt seinen Lock frei, BEVOR `get_profile()`
(ein echter Server-Roundtrip) laeuft; `state.client` wird erst DANACH
committed. Ohne denselben Lifecycle-Lock wie `flight_start` haette ein
zweiter Account waehrend genau dieses Roundtrips (kein aktiver Flug zum
Pruefzeitpunkt) unbemerkt einen Flug starten koennen — das Login committet
danach trotzdem, der neue Flug liefe unter falschen Credentials weiter.

**Gefixt (Grundlagenwechsel, nicht nur Reihenfolge):**

1. `Client::identity_fingerprint()` komplett entfernt (samt seiner 5
   Tests) — ersetzt durch `Profile.pilot_id` (server-verifiziert,
   ueberlebt eine Key-Rotation). Neues `AppState::authenticated_pilot_id`
   (`Mutex<Option<i64>>`), gesetzt bei jedem erfolgreichen Login
   (`phpvms_login`, `phpvms_load_session`) — `flight_start`/`flight_adopt`/
   der manuelle Plan-Pfad und `try_resume_flight` lesen daraus statt einen
   weiteren `get_profile()`-Roundtrip zu brauchen.
2. `phpvms_login` prueft die Eigentuemerschaft jetzt ERST NACH
   `get_profile()` (braucht `profile.pilot_id`) UND nur, wenn
   `state.active_flight.is_some()` TATSAECHLICH zutrifft (Befund B).
3. `phpvms_login` UND `phpvms_logout` nehmen jetzt denselben
   `FlightSetupGuard`/`flight_setup_in_progress`-Lock wie `flight_start`/
   `flight_adopt`/Resume — `phpvms_login` gibt ihn frei, BEVOR es selbst
   `try_resume_flight` aufruft (sonst haette Resume sich staendig selbst
   blockiert: "resume already in progress").

**Tests:** die beiden Quelltext-Wächter aus Nachtrag #5 blieben gueltig
(neu gegengeprueft nach dem Umbau — Reihenfolge weiterhin korrekt), plus
ein neuer, gezielter Wächter fuer Befund B
(`login_prueft_zusaetzlich_ob_ueberhaupt_ein_flug_aktiv_ist`). Gegenprobe
fuer alle drei durchgefuehrt.

## Nachtrag #7 (05.09.2026): achte Codex-Runde — Frontend-Riegel, Race im Queue-Worker, ehrliche Grenze bei Key-Rotation

Adversarial-Review gegen den weiter gewachsenen Diff (jetzt HEAD~7) fand
drei neue, voneinander unabhaengige Befunde — zwei davon werteten die
Riegel aus den Nachtraegen #5/#6 ab, ohne sie selbst zu betreffen.

**Befund 1 (high) — das Frontend ignorierte die Ablehnung des Backends.**
`App.tsx::handleLogout` fing JEDEN Fehler aus `invoke("phpvms_logout")`
pauschal ab (urspruenglich fuer einen ganz anderen Fehlerfall gedacht:
ein nicht erreichbarer Schluesselbund) und loggte die Oberflaeche
IMMER aus — auch wenn das Backend den Logout wegen eines aktiven Fluges
(Nachtrag #5) korrekt mit `flight_active` verweigert hatte. Der Riegel im
Backend war also fachlich vorhanden, wurde dem Piloten aber nie sichtbar
gemacht: die App zeigte den Logout als erfolgreich an, obwohl serverseitig
gar nichts passiert war.

**Befund 2 (high) — Client und Piloten-ID konnten im Queue-Worker
auseinanderlaufen.** `spawn_pirep_queue_worker` las `state.client` und
`state.authenticated_pilot_id` bislang an zwei getrennten Stellen im
selben Tick, getrennt durch einen echten Await-Punkt
(`drain_pending_bid_cleanup(...).await`). Ein Logout/Login-Kontowechsel
genau in dieser Luecke haette Pilot As Client mit Pilot Bs Identitaet
gepaart — ein Warteschlangen-Eintrag, der tatsaechlich Pilot B gehoert,
haette die Eigentuemer-Pruefung bestanden, waere aber mit Pilot As
Credentials eingereicht worden (und bei einem 403/404 faelschlich
geloescht statt in Quarantaene zu bleiben).

**Befund 3 (high) — die seit Nachtrag #6 erlaubte Key-Rotation
DESSELBEN Piloten hilft laufenden Hintergrund-Aufgaben nichts.**
`spawn_position_streamer` nimmt seinen `Client` per Wert entgegen und
haelt ihn fuer die gesamte Laufzeit des Fluges in einer `async move`-
Task — er loest `current_client(&state)` nie erneut auf. Ein Re-Login
DESSELBEN Piloten mit neuem Schluessel (seit Nachtrag #6 ausdruecklich
erlaubt) aendert an dieser laufenden Positions-Uebertragung nichts: sie
meldet mit dem ALTEN Schluessel weiter, bis der Flug endet. Gegengeprueft:
`flight_end`/`flight_cancel` loesen `current_client(&state)` dagegen bei
JEDEM Aufruf frisch auf — ein geordnetes Beenden/Abbrechen profitiert vom
Re-Login also durchaus, nur die Positions-Uebertragung selbst nicht.

**Gefixt:**

1. `handleLogout` unterscheidet jetzt `code === "flight_active"` von
   jedem anderen Fehler und bricht in diesem Fall VOR dem Aufraeumen des
   Session-Zustands ab, statt die Oberflaeche trotzdem auszuloggen. Ein
   `Notice`/`Button`-Banner (Hausmuster aus `IntegrityBanner.tsx`, bewusst
   KEIN `window.alert()` — das wird unter macOS WKWebView lautlos
   verschluckt, siehe bestehender Kommentar in `SettingsPanel.tsx`) zeigt
   dem Piloten den Grund an.
2. `spawn_pirep_queue_worker` erfasst `client` UND `authenticated_pilot_id`
   jetzt als EINEN atomaren Schnappschuss, bevor irgendein Await in diesem
   Tick laeuft — `drain_pending_bid_cleanup(...).await` folgt erst danach.
3. Fuer Befund 3 KEINE Architekturaenderung (jeder Langlaeufer muesste
   sonst pro Tick neu `current_client(&state)` aufloesen — eine sehr
   breite, streuende Aenderung fuer eine seltene Situation). Stattdessen
   eine ehrliche, sichtbare Warnung: `phpvms_login` protokolliert per
   `log_activity_handle`, dass Positions-Updates bei einem erkannten
   Kontowechsel waehrend eines laufenden Fluges mit dem ALTEN Zugang
   weiterlaufen, bis der Flug endet — mit dem Hinweis, den Flug bei
   anhaltenden Problemen zu beenden/abzubrechen und neu zu starten.

**Tests:** neuer Quelltext-Waechter
`client_und_piloten_id_werden_vor_dem_ersten_await_gemeinsam_erfasst`
fuer Befund 2 (prueft, dass `authenticated_pilot_id` textuell VOR
`drain_pending_bid_cleanup(&app, &client).await` im Funktionskoerper
steht) — Gegenprobe durchgefuehrt (Reihenfolge vertauscht, Waechter
schlaegt fehl; wiederhergestellt, Waechter besteht wieder). Fuer Befund 1
(Frontend) und Befund 3 (Log-Warnung) keine dedizierten neuen Tests —
Befund 1 ist durch `npx tsc -b` und den bestehenden Vitest-Lauf (731 grün)
nur indirekt abgedeckt, Befund 3 ist eine reine Transparenz-Ergaenzung
ohne eigenen Kontrollfluss, der sich sinnvoll gegenpruefen liesse.

## Nachtrag #8 (05.09.2026): neunte Codex-Runde — Bid-Cleanup-Queue, Best-Effort-Nachbearbeitung, Resume-Lock-Reihenfolge

Adversarial-Review gegen alle acht bisherigen Commits dieser Serie fand
sechs weitere, voneinander unabhaengige Befunde (vier mittel, zwei gering)
— keiner davon eine bereits gefixte Fehlerklasse.

**Befund 1 (mittel) — die separate Bid-Cleanup-Warteschlange hatte GAR
KEINE Kontobindung.** `PendingBidCleanup` (eigene, kleine Warteschlange
neben dem Haupt-PIREP-Queue, fuer `delete_bid`-Retries nach transientem
Scheitern) besass kein `owner_identity`-Feld — jeder Eintrag wurde mit dem
gerade angemeldeten Client verarbeitet, unabhaengig davon wer ihn
eingereiht hatte. Ein Kontowechsel waehrend ein Eintrag noch offen war,
haette `delete_bid` mit dem FALSCHEN Account ausgefuehrt.

**Befund 2 (mittel) — Best-Effort-Nachbearbeitung im Queue-Worker las
nach dem Filing erneut den aktuellen Zustand.** Das phpVMS-Filing selbst
nutzt korrekt den am Tick-Anfang erfassten Client. Die MQTT-Publish- und
JSONL-Upload-Schritte DANACH lasen aber erneut `state.mqtt` bzw. frisch
aus dem Keyring — nach mehreren Awaits seit dem Schnappschuss. Ein
Kontowechsel in dieser Luecke haette Pilot As bereits korrekt
eingereichten PIREP ueber Pilot Bs MQTT-Verbindung/Zugangsdaten
weitergesendet.

**Befund 3 (mittel) — das Frontend behandelte `flight_setup_in_progress`
wie einen erfolgreichen Logout.** Der Fix aus Nachtrag #7 unterschied nur
`flight_active` von anderen Fehlern. `phpvms_logout` kann aber auch mit
`flight_setup_in_progress` ablehnen (der `FlightSetupGuard` wird gerade
von einem Flugstart/einer Uebernahme gehalten) — ebenfalls VOR jeder
Zustandsaenderung. Dieser Fall fiel weiterhin durch zum „trotzdem
ausloggen"-Pfad.

**Befund 4 (mittel) — `try_resume_flight` nahm seinen Lifecycle-Lock erst
NACH zwei Server-Roundtrips.** Der `FlightSetupGuard` wurde erst nach der
Eigentuemer-Reklamierung (`get_user_pireps_in_progress`) und `get_pirep`
erworben, nicht davor. Ein zeitgleicher `flight_start`/`flight_adopt`
(z. B. der Auto-Start-Watcher) haette in dieser Luecke einen neuen Flug
anlegen und den Ablage-Platz ueberschreiben koennen — der zu
wiederaufnehmende PIREP waere danach lokal verwaist und serverseitig fuer
immer IN_PROGRESS steckengeblieben.

**Befund 5 (gering) — der neue Logout-Banner (Nachtrag #7) war hart
Deutsch,** ohne i18n-Anbindung, obwohl das Projekt DE/EN/IT unterstuetzt.

**Befund 6 (gering) — die Logout-Sperr-Meldung ueberlebte einen spaeter
erfolgreichen Logout/Login.** `logoutBlockedMessage` wurde nur ueber den
Schliessen-Button zurueckgesetzt.

**Gefixt:**

1. `PendingBidCleanup` bekam ein `owner_identity`-Feld (`#[serde(default)]`
   fuer Altbestand). `drain_pending_bid_cleanup` prueft es jetzt vor
   `delete_bid` — bei Unbekannt/Fremd EINMAL serverseitig nachfragen
   (`GET /api/user/bids`, serverseitig auf den eingeloggten Piloten
   gefiltert): steht der Bid dort, wird reklamiert; sonst Quarantaene
   (weder geloescht noch versucht), analog zum bestehenden Reklamier-Weg
   der Haupt-Queue.
2. Der Queue-Worker prueft direkt vor dem MQTT-Publish/JSONL-Upload eines
   Eintrags erneut, ob `authenticated_pilot_id` noch mit dem
   Tick-Schnappschuss uebereinstimmt — bei Kontowechsel werden beide
   Best-Effort-Kanaele fuer diesen Eintrag uebersprungen (das Filing selbst
   bleibt unangetastet, es ist bereits korrekt erfolgt).
3. `handleLogout` unterscheidet jetzt `flight_active` UND
   `flight_setup_in_progress` als „nichts veraendert" von jedem anderen
   Code (der immer erst NACH dem Leeren von `state.client` auftritt).
4. Der `FlightSetupGuard` in `try_resume_flight` wird jetzt vor der
   Eigentuemer-Reklamierung erworben, nicht danach; die dadurch redundante
   zweite Lock-Anforderung weiter unten (StrictMode-Doppelmount-Schutz) ist
   entfernt, da der Lock ab jetzt schon die ganze Funktion ueber gehalten
   wird.
5. Neue i18n-Schluessel (`flight.error.flight_active`,
   `flight.error.flight_setup_in_progress`, `logout.blocked_title`,
   `logout.dismiss`) in DE/EN/IT — Parity-Test bleibt gruen.
6. `logoutBlockedMessage` wird jetzt sowohl bei einem spaeter erfolgreichen
   Logout als auch bei einem neuen Login zurueckgesetzt.

**Tests:** drei neue Quelltext-Waechter
(`pending_bid_cleanup_prueft_eigentuemer_vor_delete_bid`,
`queue_worker_prueft_identitaet_erneut_vor_mqtt_und_log_upload`,
`try_resume_flight_haelt_lifecycle_lock_vor_den_server_roundtrips`), alle
drei per Gegenprobe verifiziert. Fuer Befund 5/6 (Frontend) keine
dedizierten neuen Tests, dafuer die bestehende i18n-Parity-Suite plus
`npx tsc -b`/Vitest weiterhin gruen — konsistent mit dem in Nachtrag #7
etablierten Massstab.

**Eigene Lehre aus dieser Runde:** zwei der drei neuen Quelltext-Waechter
hatten anfangs den klassischen Selbst-Treffer-Fehler (Runde 3, siehe oben)
— die Namens-Endboundary `\nfn ` allein reicht nicht, wenn die naechste
Funktion selbst eine `async fn` ist (das Suchfenster ueberschiesst dann in
spaetere, unverwandte Funktionen). Zusaetzlich hat der Test fuer
`try_resume_flight` einen VOLLSTAENDIG ANDEREN, laengst bestehenden
Waechter (`vor_der_server_auskunft_wird_nichts_geloescht` u. a., alle in
`wiederaufnahme_langstrecke_tests`) mit rot gemacht — dessen
whitespace-stripping-Suche fand versehentlich mein eigenes Test-Literal
zuerst, weil eine der beiden zur Laufzeit zusammengesetzten Haelften
("async fn try_resume_flight") exakt dem Suchbegriff jenes Waechters
entsprach. Beide Male erst durch den vollen Testlauf aufgefallen, nicht
durch den isolierten Testlauf des neuen Tests allein — **neue
Quelltext-Waechter deshalb ab jetzt immer gegen die VOLLE Testsuite laufen
lassen, nicht nur isoliert.**

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
