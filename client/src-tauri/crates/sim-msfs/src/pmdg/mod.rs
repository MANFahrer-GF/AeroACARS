//! PMDG SimConnect SDK integration.
//!
//! Two PMDG aircraft families ship custom SimConnect ClientData
//! channels exposing the full cockpit state — **way** more than the
//! standard MSFS SimVars expose:
//!
//! * **NG3** — covers the PMDG 737-700/-800/-900 (and BBJ/BCF/BDSF
//!   variants of those airframes). Channel name `PMDG_NG3_Data`.
//! * **777X** — covers the PMDG 777-200LR / -300ER / 777F / 777W.
//!   Channel name `PMDG_777X_Data`.
//!
//! Both families share an identical SimConnect subscription pattern —
//! `MapClientDataNameToID`, `AddToClientDataDefinition`, `RequestClientData`
//! with `PERIOD_ON_SET + FLAG_CHANGED`. Only the names, IDs, and
//! struct shapes differ.
//!
//! # Activation by the pilot
//!
//! PMDG does NOT broadcast the data by default. The pilot must edit
//! the aircraft's options file and add:
//!
//! ```ini
//! [SDK]
//! EnableDataBroadcast=1
//! ```
//!
//! Files (auto-detected from the aircraft's installation path):
//!
//! * 737NG3: `<MSFS_Community>/pmdg-aircraft-738/work/737NG3_Options.ini`
//! * 777X:   `<MSFS_Community>/pmdg-aircraft-77er/work/777X_Options.ini`
//!
//! AeroACARS detects "PMDG aircraft loaded but no data flowing" and
//! surfaces a Settings-tab modal with the exact path and the lines
//! to add. See the `pmdg-sdk-integration.md` doc.
//!
//! # Memory layout — `#[repr(C)]`
//!
//! The PMDG SDK headers are MSVC C++. C++ `bool` is 1 byte, the rest
//! is the standard MSVC layout (4-byte int, 4-byte float, 8-byte
//! double, no struct-end padding for trailing-byte arrays). Rust
//! `#[repr(C)]` gives us the same layout because the alignment rules
//! match for all primitive types we use.
//!
//! Critical: **never** use `#[repr(packed)]` here — that would force
//! 1-byte alignment which differs from the MSVC default and would
//! mis-parse fields whose offset depends on alignment padding (e.g.
//! a `float` after a sequence of `bool`s).
//!
//! Each `*_RawData` struct mirrors the SDK header **field-by-field
//! in the exact same order**. We then expose a higher-level
//! `*_Snapshot` struct with only the fields useful to AeroACARS,
//! converted to ergonomic Rust types (`bool` for the C++ `bool`,
//! `Option<u16>` for "0 means not set", etc.).

pub mod ng3;
pub mod x777;

/// Variant tag for the PMDG aircraft we're talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmdgVariant {
    /// PMDG 737 NG3 family (700/800/900 + BBJ/BCF/BDSF).
    Ng3,
    /// PMDG 777X family (777-200LR / -300ER / 777F / 777W).
    X777,
}

impl PmdgVariant {
    /// Match the active aircraft's `.air` file path against the known
    /// PMDG installation prefixes. Returns `None` for non-PMDG aircraft.
    ///
    /// Pulled from `SimConnect_RequestSystemState("AircraftLoaded")`
    /// — that returns the .air file path the user loaded. Examples:
    /// `"E:\MSFS24_Community\Community\pmdg-aircraft-738\SimObjects\Airplanes\PMDG 737-800\aircraft.cfg"`
    pub fn detect_from_air_path(air_path: &str) -> Option<Self> {
        let p = air_path.to_lowercase();
        // PMDG 737 NG3 family — covers ALL Boeing 737 variants
        // PMDG ships under one shared SDK. Folder naming:
        //   pmdg-aircraft-736  = 737-600
        //   pmdg-aircraft-737  = 737-700
        //   pmdg-aircraft-738  = 737-800
        //   pmdg-aircraft-739  = 737-900
        // Plus liveries inherit the parent folder (e.g.
        // pmdg-aircraft-738-sxs-tc-snn) so a substring match
        // catches them too. The trailing "73x" path also covers
        // any future numerical variant PMDG might ship.
        if p.contains("pmdg-aircraft-736")
            || p.contains("pmdg-aircraft-737")
            || p.contains("pmdg-aircraft-738")
            || p.contains("pmdg-aircraft-739")
            || p.contains("pmdg 737")
            || p.contains("pmdg 736")
            || p.contains("pmdg 738")
            || p.contains("pmdg 739")
        {
            Some(Self::Ng3)
        } else if p.contains("pmdg-aircraft-77")
            || p.contains("pmdg 777")
        {
            Some(Self::X777)
        } else {
            None
        }
    }

    /// Human-readable label for log lines / UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ng3 => "PMDG 737 NG3",
            Self::X777 => "PMDG 777X",
        }
    }

    /// Best-guess generic ICAO code from the variant alone, used as a
    /// fallback when MSFS reports an unusable `ATC MODEL` string (e.g.
    /// the localisation key "ATCCOM.AC_MODEL_B738.0.text" that some
    /// liveries set). The variant alone can't tell us which 737 sub-
    /// variant or which 777 sub-variant it is, so we return the
    /// family default. Concrete sub-variant identification needs the
    /// SDK-specific aircraft-model byte from the snapshot.
    pub fn fallback_icao(self) -> &'static str {
        match self {
            Self::Ng3 => "B738",  // most common 737 NG3 variant
            Self::X777 => "B77W", // most common 777 variant in PMDG fleet
        }
    }
}

/// Canonicalise a PMDG SDK fuel quantity to kilograms (v0.16.10
/// #Premium). The `FUEL_Qty*` floats in both PMDG_NG3_SDK.h and
/// PMDG_777X_SDK.h arrive in the unit the pilot selected in the
/// aircraft options — signalled by the struct's `WeightInKg` flag
/// (header comment: "false: LBS  true: KG"; the inline "// LBS"
/// next to the FUEL_Qty fields documents the default, not a fixed
/// unit). Shared by both snapshot decoders so the
/// `PmdgState::fuel_per_tank_kg` carrier is always kg.
pub fn pmdg_fuel_qty_to_kg(raw: f32, weight_in_kg: bool) -> f64 {
    const LBS_TO_KG: f64 = 0.453_592_37;
    if weight_in_kg {
        f64::from(raw)
    } else {
        f64::from(raw) * LBS_TO_KG
    }
}

/// Decode a PMDG XPDR mode selector byte to a cockpit-readable label.
/// Both NG3 and 777X use the same mapping
/// (`XPDR_ModeSel`: 0=STBY 1=ALT_RPTG_OFF 2=XPNDR 3=TA 4=TA/RA).
pub fn pmdg_xpdr_mode_label(mode: u8) -> &'static str {
    match mode {
        0 => "STBY",
        1 => "ALT-OFF",
        2 => "XPNDR",
        3 => "TA",
        4 => "TA-RA",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::PmdgVariant;

    #[test]
    fn detect_ng3_from_typical_paths() {
        assert_eq!(
            PmdgVariant::detect_from_air_path(
                "E:\\MSFS24_Community\\Community\\pmdg-aircraft-738\\SimObjects\\Airplanes\\PMDG 737-800\\aircraft.cfg"
            ),
            Some(PmdgVariant::Ng3)
        );
        assert_eq!(
            PmdgVariant::detect_from_air_path("/Some/Path/PMDG 737-900/aircraft.cfg"),
            Some(PmdgVariant::Ng3)
        );
    }

    #[test]
    fn detect_x777_from_typical_paths() {
        assert_eq!(
            PmdgVariant::detect_from_air_path(
                "E:\\MSFS24_Community\\Community\\pmdg-aircraft-77er\\SimObjects\\Airplanes\\PMDG 777-300ER\\aircraft.cfg"
            ),
            Some(PmdgVariant::X777)
        );
        assert_eq!(
            PmdgVariant::detect_from_air_path("path/pmdg-aircraft-77w/something"),
            Some(PmdgVariant::X777)
        );
        assert_eq!(
            PmdgVariant::detect_from_air_path("PMDG 777-200LR"),
            Some(PmdgVariant::X777)
        );
    }

    #[test]
    fn xpdr_mode_label_decoding() {
        use super::pmdg_xpdr_mode_label;
        assert_eq!(pmdg_xpdr_mode_label(0), "STBY");
        assert_eq!(pmdg_xpdr_mode_label(1), "ALT-OFF");
        assert_eq!(pmdg_xpdr_mode_label(2), "XPNDR");
        assert_eq!(pmdg_xpdr_mode_label(3), "TA");
        assert_eq!(pmdg_xpdr_mode_label(4), "TA-RA");
        assert_eq!(pmdg_xpdr_mode_label(99), ""); // unknown stays silent
    }

    #[test]
    fn fallback_icao_for_pmdg_variant() {
        use super::PmdgVariant;
        assert_eq!(PmdgVariant::Ng3.fallback_icao(), "B738");
        assert_eq!(PmdgVariant::X777.fallback_icao(), "B77W");
    }

    /// v0.16.10 (#Premium): SDK fuel quantities follow the cockpit
    /// weight-unit option (`WeightInKg` flag) — both directions must
    /// canonicalise to kg.
    #[test]
    fn fuel_qty_to_kg_both_unit_modes() {
        use super::pmdg_fuel_qty_to_kg;
        // Flag says kg → passthrough.
        assert!((pmdg_fuel_qty_to_kg(1000.0, true) - 1000.0).abs() < 1e-9);
        // Flag says lbs → ×0.45359237.
        assert!((pmdg_fuel_qty_to_kg(1000.0, false) - 453.59237).abs() < 1e-6);
        assert!((pmdg_fuel_qty_to_kg(0.0, false)).abs() < 1e-9);
    }

    #[test]
    fn detect_returns_none_for_other_aircraft() {
        assert_eq!(
            PmdgVariant::detect_from_air_path(
                "/Asobo/SimObjects/Airplanes/Asobo_A320_NEO/aircraft.cfg"
            ),
            None
        );
        assert_eq!(
            PmdgVariant::detect_from_air_path(
                "/Community/flybywire-aircraft-a320-neo/aircraft.cfg"
            ),
            None
        );
        assert_eq!(PmdgVariant::detect_from_air_path(""), None);
    }
}
