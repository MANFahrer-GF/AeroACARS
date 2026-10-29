    use super::position_plausibel;

    #[test]
    fn menue_scheinpositionen_aus_den_logs_taugen_nicht() {
        // Echte Werte aus n13rGYv4aLpBlZw4 und aKRgO8gxYzPnLR8B.
        assert!(!position_plausibel(-0.0, 90.0003, -2_109_340.0));
        assert!(!position_plausibel(-0.0, 90.0003, -287_007.0));
        assert!(!position_plausibel(-0.0, 90.0003, 228.0));
    }

    #[test]
    fn echte_positionen_taugen() {
        assert!(position_plausibel(45.6291, 8.7188, 742.0)); // LIMC
        assert!(position_plausibel(58.8836, 5.6302, 37.0)); // ENZV
        assert!(position_plausibel(52.3, 4.76, -11.0)); // EHAM unter NN
        assert!(position_plausibel(0.0, 0.0, 35_000.0)); // Reiseflug ueber dem Nullpunkt
    }
