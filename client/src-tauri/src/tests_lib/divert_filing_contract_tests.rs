    //! A divert is a normal arrival at a different field.
    //!
    //! Until v1.7.0 it was not: `flight_end` had a second, parallel filing path
    //! that never called `/file` and instead mass-assigned `state=PENDING`
    //! through the update endpoint. Two things followed from that, and both bit
    //! on 23.08.2026 (EJA9 EHAM->EDSB, gear failure, back to EHAM):
    //!
    //!  * Nothing hanging off phpVMS' `PirepFiled` event ever ran — not its own
    //!    divert bookkeeping, not the bid cleanup, not the integrity gate, and
    //!    not the per-rank auto-approve. The PIREP sat waiting for an admin and
    //!    the pilot was locked out of the dispatch hub for an hour.
    //!  * It depended on phpVMS forwarding unvalidated request input straight
    //!    into mass-assignment — an upstream internal, not a contract.
    //!
    //! These are source-text invariants, in the same spirit as
    //! `display_and_filing_paths_never_read_raw_vs_fields`: the rule stood in a
    //! comment before and a comment enforces nothing.

    const SRC: &str = crate::quelle_gesamt::PRODUKTION;

    /// Body of one filing path, comment lines dropped — comments may well name
    /// the forbidden shapes, they are what explains them.
    fn filing_path(from: &str, to: &str) -> String {
        let start = SRC.find(from).unwrap_or_else(|| {
            panic!("Filing-Pfad nicht mehr gefunden: {from} — Test anpassen, nicht loeschen")
        });
        let rest = &SRC[start..];
        let end = rest
            .find(to)
            .unwrap_or_else(|| panic!("Ende des Filing-Pfads nicht mehr gefunden: {to}"));
        rest[..end]
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Wie `filing_path`, aber ohne jeden Leerraum.
    ///
    /// ⚠ Diese Pruefungen suchen Quelltext. Ein Muster wie
    /// `fields.insert("Diversion Airport"` haengt damit an der
    /// FORMATIERUNG — und die gehoert nicht uns, sondern `rustfmt`.
    ///
    /// Am 28.08.2026 genau so passiert: Ein `cargo fmt` brach den Aufruf
    /// im manuellen Pfad ueber drei Zeilen um, und die Wache schlug an,
    /// obwohl sich am Verhalten nichts geaendert hatte. Eine Wache, die
    /// bei einem Formatierungslauf rot wird, verliert genau das
    /// Vertrauen, das sie braucht.
    ///
    /// Ohne Leerraum ist der Vergleich davon unabhaengig. Die Suchmuster
    /// muessen entsprechend ebenfalls leerraumfrei geschrieben werden.
    fn filing_path_ohne_leerraum(from: &str, to: &str) -> String {
        filing_path(from, to)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect()
    }

    fn regulaerer_pfad() -> String {
        filing_path("async fn flight_end(", "async fn flight_end_manual(")
    }

    fn manueller_pfad() -> String {
        filing_path(
            "async fn flight_end_manual(",
            "\nasync fn flight_resume_check_position(",
        )
    }

    #[test]
    fn divert_reist_im_feld_nicht_in_arr_airport_id() {
        // phpVMS keys its divert handling on the custom field whose slug is
        // `diversion-airport`, derived from the field NAME. Overriding
        // arr_airport_id ourselves on top of it makes phpVMS rewrite the divert
        // airport to itself and file it as its own alternate.
        for (name, body) in [
            (
                "flight_end",
                filing_path_ohne_leerraum("async fn flight_end(", "async fn flight_end_manual("),
            ),
            (
                "flight_end_manual",
                filing_path_ohne_leerraum(
                    "async fn flight_end_manual(",
                    "\nasync fn flight_resume_check_position(",
                ),
            ),
        ] {
            assert!(
                body.contains(r#"fields.insert("DiversionAirport""#),
                "{name} schickt das Divert-Feld nicht mehr mit — phpVMS erfaehrt \
                 dann nichts von der Ausweichlandung (kein alt_airport_id, \
                 Flugzeug und Pilot bleiben am geplanten Ziel stehen)."
            );
            assert!(
                !body.contains("arr_airport_id: divert_to"),
                "{name} ueberschreibt arr_airport_id wieder selbst. Zusammen mit \
                 dem Divert-Feld schreibt phpVMS daraus EHAM->EHAM."
            );
        }
    }

    #[test]
    fn kein_zweiter_filing_weg_am_file_endpunkt_vorbei() {
        let body = regulaerer_pfad();
        assert!(
            !body.contains("state: Some(1)"),
            "flight_end setzt den PIREP-Status wieder von Hand auf PENDING. Genau \
             das hat den Piloten am 23.08.2026 eine Stunde ausgesperrt: ohne \
             /file feuert PirepFiled nie, also laeuft weder die Divert-Behandlung \
             noch der Auto-Approve nach Rang."
        );
        assert!(
            !body.contains("pirep_source::MANUAL"),
            "flight_end faelscht die PIREP-Quelle wieder auf MANUAL, um am \
             Auto-Approve vorbeizukommen. Ein Divert braucht keine Freigabe."
        );
        assert_eq!(
            body.matches("file_pirep_with_retry(").count(),
            1,
            "es darf genau EINEN Aufruf des /file-Endpunkts in flight_end geben — \
             Divert und normale Ankunft laufen denselben Weg."
        );
    }

    #[test]
    fn der_manuelle_weg_setzt_den_status_nicht_von_hand() {
        // `manueller_pfad()` stand hier, ohne dass ein Test ihn benutzte —
        // aufgefallen erst, als `cargo check --all-targets` am 20.09.2026
        // die tote Funktion meldete. Eine ungenutzte Testhilfe heisst oft,
        // dass ein Waechter fehlt, nicht dass Code ueberfluessig ist.
        //
        // Gegenstueck zu `kein_zweiter_filing_weg_am_file_endpunkt_vorbei`:
        // Der regulaere Weg darf die PIREP-Quelle NICHT auf MANUAL
        // faelschen, der manuelle Weg MUSS sie so setzen — er ist ja einer.
        // Gemeinsam ist beiden: Den Status von Hand auf PENDING zu setzen
        // sperrt den Piloten aus (23.08.2026, eine Stunde).
        let body = manueller_pfad();
        assert!(
            !body.contains("state: Some(1)"),
            "flight_end_manual setzt den PIREP-Status von Hand auf PENDING. \
             Ohne /file feuert PirepFiled nie — weder Divert-Behandlung noch \
             Auto-Approve laufen dann."
        );
        assert!(
            body.contains("pirep_source::MANUAL"),
            "flight_end_manual kennzeichnet den Bericht nicht mehr als \
             manuell. Genau das unterscheidet ihn vom regulaeren Weg."
        );
    }

    #[test]
    fn quellen_konstanten_stimmen_mit_phpvms_ueberein() {
        // phpVMS App\Enums\PirepSource. Waren bis v1.7.0 vertauscht, wodurch
        // jedes "auf MANUAL setzen" in Wahrheit ACARS schrieb.
        assert_eq!(api_client::pirep_source::MANUAL, 0);
        assert_eq!(api_client::pirep_source::ACARS, 1);
    }
