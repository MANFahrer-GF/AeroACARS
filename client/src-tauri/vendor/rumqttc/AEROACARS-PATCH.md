# rumqttc 0.25.1 mit AeroACARS-Patch: Happy Eyeballs

Diese Kopie ist rumqttc 0.25.1 von crates.io (Prüfsumme im alten
`Cargo.lock`: `0feff8d882bff0b2fddaf99355a10336d43dd3ed44204f85ece28cf9626ab519`),
eingebunden über `[patch.crates-io]` in `client/src-tauri/Cargo.toml`.
Sie ist kein Workspace-Mitglied (`exclude`), ihre eigenen Tests und
Beispiele laufen nicht mit `cargo test --workspace`.

## Warum

`socket_connect` in `src/eventloop.rs` probierte die Adressen aus
`lookup_host` streng nacheinander. Mit AAAA-Eintrag für live.kant.ovh und
einem Netz, das IPv6 verschluckt (Pakete verschwinden ohne Antwort), hing
der erste Versuch bis zum `connection_timeout` (5 s), danach begann
rumqttc von vorn, IPv4 kam nie dran. Ein Pilot mit halb kaputtem IPv6
wäre dauerhaft vom Live-Server getrennt. HTTP-Verbindungen des Clients
(reqwest/hyper-util) sind nicht betroffen, hyper-util probiert IPv4 von
sich aus nach 300 ms parallel.

## Was geändert ist

- `src/happy_eyeballs.rs` (neu): Reihenfolge nach RFC 8305 Abschnitt 4
  (Familie der ersten Resolver-Adresse zuerst, dann abwechselnd) und
  Verbindungsaufbau nach Abschnitt 5: nächster Versuch nach 250 ms oder
  sofort, wenn ein Versuch scheitert; die erste Verbindung gewinnt, die
  übrigen Versuche werden verworfen.
- `src/eventloop.rs`: `socket_connect` löst den Namen auf und ruft
  `happy_eyeballs::verbinden`; der Rumpf der alten Schleife ist unverändert
  als `socket_connect_addr` (ein Versuch) erhalten. Nebeneffekt: Scheitert
  das Anlegen eines Sockets (z. B. kein IPv6-Stack), wird die nächste
  Adresse probiert statt abzubrechen.
- `src/lib.rs`: `pub mod happy_eyeballs;` (öffentlich, damit
  `crates/aeroacars-mqtt/tests/happy_eyeballs.rs` die Logik testet).

Alles andere ist byte-gleich zum Original. Prüfen:

```bash
diff -ru ~/.cargo/registry/src/index.crates.io-*/rumqttc-0.25.1 client/src-tauri/vendor/rumqttc
```

## Warum eine Kopie und kein Upgrade

Stand 19.11.2026 ist 0.25.1 die neueste Version auf crates.io, und auch
`main` von bytebeamio/rumqtt verbindet noch nacheinander. rumqttc bietet
keinen Haken für einen eigenen TCP-Connector: `network_connect` baut den
Strom für Tcp/Tls/Ws/Wss immer über `socket_connect`. Der einzige andere
Weg wäre, WSS, TLS und den MQTT-Aufbau selbst nachzubauen; das wäre viel
mehr Code als diese kleine Änderung.

## Ausbauen

Kann eine künftige rumqttc-Version Happy Eyeballs (oder einen eigenen
Connector), den `[patch.crates-io]`-Eintrag und `exclude` entfernen, den
Ordner löschen, die Tests in `crates/aeroacars-mqtt/tests/happy_eyeballs.rs`
auf die neue Schnittstelle umstellen (die Fälle bleiben).
