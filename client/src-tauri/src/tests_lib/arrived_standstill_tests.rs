    use super::arrived_standstill_condition as cond;

    #[test]
    fn fa50_stuck_engine_count_still_arrives() {
        // Contrail FA50: am Ziel voll gestanden, Parkbremse gesetzt — der
        // Stillstands-Pfad greift, obwohl engines_running (hier irrelevant)
        // klemmen würde. Das ist der Kern des Auto-File-Fixes.
        assert!(cond(true, true, 0.0, true));
    }

    #[test]
    fn requires_parking_brake() {
        // Ohne Parkbremse NICHT — verhindert Fehlauslösung z. B. beim Warten
        // auf eine freie Bahn während des Rollens (Kreuzungshalt, real auch
        // mehrere Minuten).
        assert!(!cond(true, true, 0.0, false));
    }

    #[test]
    fn requires_stationary() {
        // Noch in Bewegung (Taxi) → nein.
        assert!(!cond(true, true, 5.0, true));
    }

    #[test]
    fn requires_on_ground() {
        assert!(!cond(true, false, 0.0, true));
    }

    #[test]
    fn requires_near_planned_destination() {
        // Fernab vom geplanten Ziel läuft der Stillstands-Pfad NICHT — dort
        // greift stattdessen der strikte engines-off-Divert-Pfad.
        assert!(!cond(false, true, 0.0, true));
    }
