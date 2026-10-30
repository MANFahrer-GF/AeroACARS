// v3 (Beta 1): Klartext je Systembefehl für den Bestätigungsdialog der Hülle.
//
// Bewusst HIER im Programm und nicht in `bundle/texte/*.json`: Die Texte der
// Hülle dürfen nie aus dem Laufzeit-Paket kommen (Entscheidung Etappe 5) —
// sonst könnte ein fremdes Paket „Alle Flugprotokolle löschen“ als harmlose
// Aktion beschriften. Das gilt auch für Titel und Knöpfe des Dialogs
// (`RAHMEN`). Ein Test verlangt für jeden Befehl der Stufe „system“ in
// `src-tauri/befehle.json` Frage und Knopf in allen drei Sprachen.
//
// 30.10.2026 (Thomas' erster Start): Der Dialog stellt eine konkrete Frage
// mit dem tatsächlichen Wert („… ausschalten?“ statt „ein- oder
// ausschalten“), der Knopf nennt das Verb, Befehl und Rohwerte stehen nur
// noch unter „Technische Details“.

export type Sprache = "de" | "en" | "it";

export interface Text {
  frage: string;
  knopf: string;
}

/** Je Sprache Frage und Knopf; `aus` gilt, wenn der Befehl mit `enabled: false` kommt. */
export interface Eintrag {
  de: Text;
  en: Text;
  it: Text;
  aus?: Record<Sprache, Text>;
}

export const BEFEHLSTEXTE: Record<string, Eintrag> = {
  activity_log_clear: {
    de: { frage: "Das Aktivitätsprotokoll in der App leeren?", knopf: "Leeren" },
    en: { frage: "Clear the activity log in the app?", knopf: "Clear" },
    it: { frage: "Svuotare il registro attività nell'app?", knopf: "Svuota" },
  },
  ascan_submit: {
    de: { frage: "Das gewählte Flugzeugpaket packen und an den GSG-Live-Server senden?", knopf: "Senden" },
    en: { frage: "Pack the selected aircraft package and send it to the GSG live server?", knopf: "Send" },
    it: { frage: "Comprimere il pacchetto aereo scelto e inviarlo al server live GSG?", knopf: "Invia" },
  },
  flight_forget_remote: {
    de: { frage: "Den hängenden Flug {{pirepId}} nur auf diesem Rechner vergessen? phpVMS erfährt davon nichts.", knopf: "Vergessen" },
    en: { frage: "Forget the stuck flight {{pirepId}} on this computer only? phpVMS is not told.", knopf: "Forget" },
    it: { frage: "Dimenticare il volo bloccato {{pirepId}} solo su questo computer? phpVMS non viene avvisato.", knopf: "Dimentica" },
  },
  flight_logs_auto_purge_set: {
    de: { frage: "Alte Flugprotokolle automatisch löschen? Protokolle älter als 30 Tage werden sofort gelöscht.", knopf: "Einschalten" },
    en: { frage: "Delete old flight logs automatically? Logs older than 30 days are deleted right away.", knopf: "Turn on" },
    it: { frage: "Eliminare automaticamente i vecchi registri di volo? I registri più vecchi di 30 giorni vengono eliminati subito.", knopf: "Attiva" },
    aus: {
      de: { frage: "Das automatische Löschen alter Flugprotokolle ausschalten?", knopf: "Ausschalten" },
      en: { frage: "Turn off automatic deletion of old flight logs?", knopf: "Turn off" },
      it: { frage: "Disattivare l'eliminazione automatica dei vecchi registri di volo?", knopf: "Disattiva" },
    },
  },
  flight_logs_delete_all: {
    de: { frage: "Alle Flugprotokolle auf diesem Rechner löschen? Das lässt sich nicht rückgängig machen.", knopf: "Alle löschen" },
    en: { frage: "Delete all flight logs on this computer? This cannot be undone.", knopf: "Delete all" },
    it: { frage: "Eliminare tutti i registri di volo su questo computer? Non si può annullare.", knopf: "Elimina tutto" },
  },
  flight_logs_purge_older_than: {
    de: { frage: "Flugprotokolle auf diesem Rechner löschen, die älter als {{olderThanDays}} Tage sind? Bei 0 sind das alle bereits hochgeladenen.", knopf: "Löschen" },
    en: { frage: "Delete flight logs on this computer older than {{olderThanDays}} days? With 0 that is every log already uploaded.", knopf: "Delete" },
    it: { frage: "Eliminare i registri di volo su questo computer più vecchi di {{olderThanDays}} giorni? Con 0 sono tutti quelli già caricati.", knopf: "Elimina" },
  },
  hoppie_clear_logon_code: {
    de: { frage: "Den gespeicherten Hoppie-Logon-Code (CPDLC) entfernen?", knopf: "Entfernen" },
    en: { frage: "Remove the stored Hoppie logon code (CPDLC)?", knopf: "Remove" },
    it: { frage: "Rimuovere il codice di logon Hoppie salvato (CPDLC)?", knopf: "Rimuovi" },
  },
  hoppie_set_logon_code: {
    de: { frage: "Den Hoppie-Logon-Code (CPDLC) auf diesem Rechner speichern?", knopf: "Speichern" },
    en: { frage: "Store the Hoppie logon code (CPDLC) on this computer?", knopf: "Save" },
    it: { frage: "Salvare il codice di logon Hoppie (CPDLC) su questo computer?", knopf: "Salva" },
  },
  kabinen_musik_entfernen: {
    de: { frage: "Das Musikstück „{{datei}}“ aus der Bordmusik entfernen?", knopf: "Entfernen" },
    en: { frage: "Remove the track “{{datei}}” from the cabin music?", knopf: "Remove" },
    it: { frage: "Rimuovere il brano «{{datei}}» dalla musica di bordo?", knopf: "Rimuovi" },
  },
  kabinen_musik_hinzufuegen: {
    de: { frage: "Die gewählte Musikdatei in die Bordmusik von {{airline}} kopieren?", knopf: "Hinzufügen" },
    en: { frage: "Copy the selected music file into the cabin music of {{airline}}?", knopf: "Add" },
    it: { frage: "Copiare il file musicale scelto nella musica di bordo di {{airline}}?", knopf: "Aggiungi" },
  },
  landing_delete: {
    de: { frage: "Diese Landung ({{pirepId}}) auf diesem Rechner löschen?", knopf: "Löschen" },
    en: { frage: "Delete this landing ({{pirepId}}) on this computer?", knopf: "Delete" },
    it: { frage: "Eliminare questo atterraggio ({{pirepId}}) su questo computer?", knopf: "Elimina" },
  },
  remote_server_revoke_pairing: {
    de: { frage: "Die Kopplung der LAN-Fernbedienung aufheben? Verbundene Geräte müssen sich neu koppeln.", knopf: "Aufheben" },
    en: { frage: "Revoke the LAN remote pairing? Connected devices must pair again.", knopf: "Revoke" },
    it: { frage: "Revocare l'abbinamento del telecomando LAN? I dispositivi collegati devono abbinarsi di nuovo.", knopf: "Revoca" },
  },
  remote_server_set_port: {
    de: { frage: "Die LAN-Fernbedienung auf Port {{port}} umstellen?", knopf: "Umstellen" },
    en: { frage: "Switch the LAN remote to port {{port}}?", knopf: "Switch" },
    it: { frage: "Spostare il telecomando LAN sulla porta {{port}}?", knopf: "Cambia" },
  },
  remote_server_start: {
    de: { frage: "Die LAN-Fernbedienung starten? Geräte in deinem Netz können sich dann mit diesem Rechner koppeln.", knopf: "Starten" },
    en: { frage: "Start the LAN remote? Devices on your network can then pair with this computer.", knopf: "Start" },
    it: { frage: "Avviare il telecomando LAN? I dispositivi della tua rete potranno abbinarsi a questo computer.", knopf: "Avvia" },
  },
  shell_auftraege: {
    de: { frage: "Offene Anfragen an die Hülle lesen?", knopf: "Lesen" },
    en: { frage: "Read open requests to the shell?", knopf: "Read" },
    it: { frage: "Leggere le richieste aperte alla shell?", knopf: "Leggi" },
  },
  shell_auftrag_beantworten: {
    de: { frage: "Eine Anfrage an die Hülle beantworten?", knopf: "Beantworten" },
    en: { frage: "Answer a request to the shell?", knopf: "Answer" },
    it: { frage: "Rispondere a una richiesta alla shell?", knopf: "Rispondi" },
  },
  update_installieren: {
    de: { frage: "Das bereitstehende AeroACARS-Update installieren? Das Programm startet danach neu.", knopf: "Installieren" },
    en: { frage: "Install the available AeroACARS update? The program restarts afterwards.", knopf: "Install" },
    it: { frage: "Installare l'aggiornamento di AeroACARS disponibile? Il programma si riavvia dopo.", knopf: "Installa" },
  },
  vermessung_senden: {
    de: { frage: "Die Vermessung des Flugzeugs an den GSG-Live-Server senden?", knopf: "Senden" },
    en: { frage: "Send the aircraft measurement to the GSG live server?", knopf: "Send" },
    it: { frage: "Inviare la misurazione dell'aereo al server live GSG?", knopf: "Invia" },
  },
  xplane_install_plugin: {
    de: { frage: "Das AeroACARS-Plugin in X-Plane installieren?", knopf: "Installieren" },
    en: { frage: "Install the AeroACARS plugin in X-Plane?", knopf: "Install" },
    it: { frage: "Installare il plugin AeroACARS in X-Plane?", knopf: "Installa" },
  },
};

/** Titel, Knöpfe und Hinweise des Dialogs — ebenfalls nie aus dem Paket. */
export const RAHMEN: Record<Sprache, { titel: string; abbrechen: string; details: string; weitere: string; leer: string; unbekannt: string; ausfuehren: string }> = {
  de: {
    titel: "Bitte bestätigen",
    abbrechen: "Abbrechen",
    details: "Technische Details",
    weitere: "Danach warten noch {{anzahl}} weitere Anfragen.",
    leer: "Keine offene Anfrage.",
    unbekannt: "Die Oberfläche möchte eine Aktion ausführen, die dieses Programm nicht kennt. Nur ausführen, wenn du sie gerade selbst angestoßen hast.",
    ausfuehren: "Ausführen",
  },
  en: {
    titel: "Please confirm",
    abbrechen: "Cancel",
    details: "Technical details",
    weitere: "{{anzahl}} more requests are waiting.",
    leer: "No open request.",
    unbekannt: "The interface wants to run an action this program does not know. Only run it if you just started it yourself.",
    ausfuehren: "Run",
  },
  it: {
    titel: "Conferma",
    abbrechen: "Annulla",
    details: "Dettagli tecnici",
    weitere: "Restano in attesa altre {{anzahl}} richieste.",
    leer: "Nessuna richiesta aperta.",
    unbekannt: "L'interfaccia vuole eseguire un'azione che questo programma non conosce. Eseguila solo se l'hai appena avviata tu.",
    ausfuehren: "Esegui",
  },
};

export function sprache(roh: string | undefined): Sprache {
  const kurz = (roh ?? "").split("-")[0];
  return kurz === "de" || kurz === "en" || kurz === "it" ? kurz : "en";
}

/** Platzhalter `{{name}}` aus den Werten; einzeilig und gekürzt, weil die
 *  Werte von der Oberfläche kommen (sie wählen nur das Wort in der Frage,
 *  nie den Befehl). Fehlt ein Wert, bleibt „…“. */
function einsetzen(vorlage: string, werte: Record<string, unknown> | undefined): string {
  return vorlage.replace(/\{\{(\w+)\}\}/g, (_, k: string) => {
    const v = werte?.[k];
    if (v === undefined || v === null || GEHEIM.test(k)) return "…";
    // Steuer- und Richtungszeichen (U+202E u. a.) raus: Ein Wert darf den
    // Satz nicht optisch umdrehen (Pruefer 30.10.).
    const s = String(typeof v === "object" ? JSON.stringify(v) : v)
      .replace(/[\p{Cc}\p{Cf}]/gu, " ")
      .replace(/\s+/g, " ")
      .trim();
    return s.length > 60 ? `${s.slice(0, 60)}…` : s || "…";
  });
}

/** Frage und Knopf für einen Auftrag, oder `null` für einen unbekannten Befehl. */
export function dialogText(befehl: string, werte: Record<string, unknown> | undefined, roh: string | undefined): Text | null {
  const e = Object.prototype.hasOwnProperty.call(BEFEHLSTEXTE, befehl) ? BEFEHLSTEXTE[befehl] : undefined;
  if (!e) return null;
  const s = sprache(roh);
  const t = werte?.enabled === false && e.aus ? e.aus[s] : e[s];
  return { frage: einsetzen(t.frage, werte), knopf: t.knopf };
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
