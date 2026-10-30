// v3 (Beta 1): Klartext je Systembefehl für den Bestätigungsdialog der Hülle.
//
// Bewusst HIER im Programm und nicht in `bundle/texte/*.json`: Die Texte der
// Hülle dürfen nie aus dem Laufzeit-Paket kommen (Entscheidung Etappe 5) —
// sonst könnte ein fremdes Paket „Alle Flugprotokolle löschen“ als harmlose
// Aktion beschriften. Ein Test verlangt für jeden Befehl der Stufe „system“
// in `src-tauri/befehle.json` einen Satz in allen drei Sprachen.

export type Sprache = "de" | "en" | "it";

export const BEFEHLSTEXTE: Record<string, Record<Sprache, string>> = {
  activity_log_clear: {
    de: "Das Aktivitätsprotokoll in der App leeren",
    en: "Clear the activity log in the app",
    it: "Svuotare il registro attività nell'app",
  },
  ascan_collect: {
    de: "Die Dateiliste des gewählten Flugzeugpakets zusammenstellen (noch nichts senden)",
    en: "Collect the file list of the selected aircraft package (nothing is sent yet)",
    it: "Raccogliere l'elenco dei file del pacchetto aereo scelto (non viene ancora inviato nulla)",
  },
  ascan_list_aircraft: {
    de: "Die Flugzeugordner des Simulators auf diesem Rechner durchsuchen",
    en: "Search the simulator's aircraft folders on this computer",
    it: "Cercare le cartelle degli aerei del simulatore su questo computer",
  },
  ascan_submit: {
    de: "Das gewählte Flugzeugpaket packen und an den GSG-Live-Server senden",
    en: "Pack the selected aircraft package and send it to the GSG live server",
    it: "Comprimere il pacchetto aereo scelto e inviarlo al server live GSG",
  },
  auto_start_set_enabled: {
    de: "Den automatischen Flugstart ein- oder ausschalten (startet den Flug selbst, wenn Buchung und Abflughafen passen)",
    en: "Turn automatic flight start on or off (starts the flight by itself when booking and departure airport match)",
    it: "Attivare o disattivare l'avvio automatico del volo (avvia il volo da solo quando prenotazione e aeroporto di partenza coincidono)",
  },
  error_reporting_set_consent: {
    de: "Das Senden anonymer Fehlerberichte erlauben oder abschalten",
    en: "Allow or stop sending anonymous error reports",
    it: "Consentire o disattivare l'invio di segnalazioni di errore anonime",
  },
  flight_forget_remote: {
    de: "Einen hängenden Flug nur auf diesem Rechner vergessen (ohne Meldung an phpVMS)",
    en: "Forget a stuck flight on this computer only (without telling phpVMS)",
    it: "Dimenticare un volo bloccato solo su questo computer (senza avvisare phpVMS)",
  },
  flight_logs_auto_purge_set: {
    de: "Das automatische Löschen alter Flugprotokolle ein- oder ausschalten (beim Einschalten werden Protokolle älter als 30 Tage sofort gelöscht)",
    en: "Turn automatic deletion of old flight logs on or off (turning it on immediately deletes logs older than 30 days)",
    it: "Attivare o disattivare l'eliminazione automatica dei vecchi registri di volo (attivandola, i registri più vecchi di 30 giorni vengono eliminati subito)",
  },
  flight_logs_delete_all: {
    de: "Alle Flugprotokolle auf diesem Rechner löschen",
    en: "Delete all flight logs on this computer",
    it: "Eliminare tutti i registri di volo su questo computer",
  },
  flight_logs_purge_older_than: {
    de: "Flugprotokolle auf diesem Rechner löschen, die älter als die angegebene Zahl Tage sind",
    en: "Delete flight logs on this computer older than the given number of days",
    it: "Eliminare i registri di volo su questo computer più vecchi del numero di giorni indicato",
  },
  hoppie_clear_logon_code: {
    de: "Den gespeicherten Hoppie-Logon-Code (CPDLC) entfernen",
    en: "Remove the stored Hoppie logon code (CPDLC)",
    it: "Rimuovere il codice di logon Hoppie salvato (CPDLC)",
  },
  hoppie_set_logon_code: {
    de: "Einen Hoppie-Logon-Code (CPDLC) auf diesem Rechner speichern",
    en: "Store a Hoppie logon code (CPDLC) on this computer",
    it: "Salvare un codice di logon Hoppie (CPDLC) su questo computer",
  },
  kabinen_musik_entfernen: {
    de: "Ein Musikstück aus der Bordmusik entfernen",
    en: "Remove a track from the cabin music",
    it: "Rimuovere un brano dalla musica di bordo",
  },
  kabinen_musik_hinzufuegen: {
    de: "Eine Musikdatei in den Ordner der Bordmusik kopieren",
    en: "Copy a music file into the cabin music folder",
    it: "Copiare un file musicale nella cartella della musica di bordo",
  },
  kabinen_musik_ordner_oeffnen: {
    de: "Den Ordner der Bordmusik im Dateimanager öffnen",
    en: "Open the cabin music folder in the file manager",
    it: "Aprire la cartella della musica di bordo nel file manager",
  },
  kabinen_musik_pruefen: {
    de: "Eine gewählte Musikdatei lesen und prüfen, ob sie abspielbar ist",
    en: "Read a selected music file and check that it can be played",
    it: "Leggere un file musicale scelto e verificare che sia riproducibile",
  },
  landing_backup_now: {
    de: "Die Landungshistorie jetzt auf dem GSG-Live-Server sichern",
    en: "Back up the landing history to the GSG live server now",
    it: "Salvare ora lo storico degli atterraggi sul server live GSG",
  },
  landing_delete: {
    de: "Eine Landung aus der Landungshistorie auf diesem Rechner löschen",
    en: "Delete a landing from the landing history on this computer",
    it: "Eliminare un atterraggio dallo storico degli atterraggi su questo computer",
  },
  panel_server_set_enabled: {
    de: "Den Panel-Server für das Simulator-Panel ein- oder ausschalten (wirkt beim nächsten Start)",
    en: "Turn the panel server for the simulator panel on or off (takes effect on next start)",
    it: "Attivare o disattivare il server del pannello del simulatore (effettivo al prossimo avvio)",
  },
  phpvms_login: {
    de: "Mit dem angegebenen API-Schlüssel bei phpVMS anmelden",
    en: "Sign in to phpVMS with the given API key",
    it: "Accedere a phpVMS con la chiave API indicata",
  },
  phpvms_logout: {
    de: "Von phpVMS abmelden",
    en: "Sign out of phpVMS",
    it: "Disconnettersi da phpVMS",
  },
  remote_server_revoke_pairing: {
    de: "Alle gekoppelten Geräte der Fernbedienung (Tablet/Handy) trennen",
    en: "Unpair all remote devices (tablet/phone)",
    it: "Scollegare tutti i dispositivi remoti associati (tablet/telefono)",
  },
  remote_server_set_port: {
    de: "Den Netzwerk-Port der Fernbedienung ändern",
    en: "Change the network port of the remote control",
    it: "Cambiare la porta di rete del controllo remoto",
  },
  remote_server_start: {
    de: "Die Fernbedienung im lokalen Netz starten (andere Geräte können sich verbinden)",
    en: "Start the remote control on the local network (other devices can connect)",
    it: "Avviare il controllo remoto nella rete locale (altri dispositivi possono connettersi)",
  },
  remote_server_stop: {
    de: "Die Fernbedienung im lokalen Netz beenden",
    en: "Stop the remote control on the local network",
    it: "Arrestare il controllo remoto nella rete locale",
  },
  set_minimize_to_tray: {
    de: "Einstellen, ob AeroACARS beim Schließen in den Infobereich minimiert",
    en: "Set whether AeroACARS minimizes to the tray when closed",
    it: "Impostare se AeroACARS si riduce nell'area di notifica alla chiusura",
  },
  shell_auftraege: {
    de: "Die offenen Bestätigungsanfragen anzeigen",
    en: "Show the pending confirmation requests",
    it: "Mostrare le richieste di conferma in sospeso",
  },
  shell_auftrag_beantworten: {
    de: "Eine Bestätigungsanfrage beantworten",
    en: "Answer a confirmation request",
    it: "Rispondere a una richiesta di conferma",
  },
  telemetrie_csv_speichern: {
    de: "Die Telemetrie als CSV-Datei speichern (Speicherort wählst du im nächsten Dialog)",
    en: "Save the telemetry as a CSV file (you choose the location in the next dialog)",
    it: "Salvare la telemetria come file CSV (scegli la posizione nella finestra successiva)",
  },
  telemetrie_fenster_oeffnen: {
    de: "Das Telemetrie-Fenster öffnen",
    en: "Open the telemetry window",
    it: "Aprire la finestra della telemetria",
  },
  update_installieren: {
    de: "Das bereitstehende AeroACARS-Update installieren und die App neu starten",
    en: "Install the available AeroACARS update and restart the app",
    it: "Installare l'aggiornamento di AeroACARS disponibile e riavviare l'app",
  },
  vdgs_fenster_oeffnen: {
    de: "Das Andockführungs-Fenster (VDGS) öffnen",
    en: "Open the docking guidance window (VDGS)",
    it: "Aprire la finestra della guida all'attracco (VDGS)",
  },
  vermessung_senden: {
    de: "Das Ergebnis der Flugzeugvermessung an den GSG-Live-Server senden",
    en: "Send the aircraft measurement result to the GSG live server",
    it: "Inviare il risultato della misurazione dell'aereo al server live GSG",
  },
  xplane_band_set_enabled: {
    de: "Das Senden des X-Plane-Bands ein- oder ausschalten",
    en: "Turn sending the X-Plane band on or off",
    it: "Attivare o disattivare l'invio della banda X-Plane",
  },
  xplane_check_install_path: {
    de: "Einen gewählten Ordner prüfen, ob er eine X-Plane-Installation ist",
    en: "Check whether a selected folder is an X-Plane installation",
    it: "Verificare se una cartella scelta è un'installazione di X-Plane",
  },
  xplane_install_plugin: {
    de: "Das AeroACARS-Plugin in den X-Plane-Ordner installieren",
    en: "Install the AeroACARS plugin into the X-Plane folder",
    it: "Installare il plugin AeroACARS nella cartella di X-Plane",
  },
};

/** Satz zum Befehl in der Sprache; `null` = unbekannter Befehl (dann zeigt
 *  der Dialog nur den Befehlsnamen). */
export function befehlstext(befehl: string, sprache: string | undefined): string | null {
  const texte = Object.prototype.hasOwnProperty.call(BEFEHLSTEXTE, befehl) ? BEFEHLSTEXTE[befehl] : undefined;
  if (!texte) return null;
  const kurz = (sprache ?? "").split("-")[0];
  return kurz === "de" || kurz === "en" || kurz === "it" ? texte[kurz] : texte.en;
}

/** Schlüssel, deren Wert geheim ist (Tauri übergibt camelCase: `apiKey`, `code`). */
const GEHEIM = /pass|key|token|secret|code|logon/i;
/** Längere Texte (z. B. CSV-Inhalt) gekürzt, damit die Werte lesbar bleiben. */
const HOECHSTENS = 200;

const MASKE = "••••••";

/** Geheimnisse auch in verschachtelten Werten maskieren (Codex 30.10.). */
function maskiert(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(maskiert);
  if (v && typeof v === "object") {
    return Object.fromEntries(Object.entries(v).map(([k, w]) => [k, GEHEIM.test(k) ? MASKE : maskiert(w)]));
  }
  return v;
}

/** Werte für die Anzeige: Geheimnisse maskiert, lange Texte gekürzt. */
export function werteAnzeige(werte: Record<string, unknown> | undefined): [string, string][] {
  return Object.entries(werte ?? {}).map(([k, v]) => {
    if (GEHEIM.test(k)) return [k, MASKE];
    const text = typeof v === "string" ? v : JSON.stringify(maskiert(v)) ?? String(v);
    return [k, text.length > HOECHSTENS ? `${text.slice(0, HOECHSTENS)}… (${text.length})` : text];
  });
}
