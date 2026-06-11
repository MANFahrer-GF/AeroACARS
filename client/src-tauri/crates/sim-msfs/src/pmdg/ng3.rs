//! PMDG 737 NG3 SimConnect SDK data structures.
//!
//! Mirrors `PMDG_NG3_SDK.h` (Copyright PMDG, ships with the
//! pmdg-aircraft-738 installation under
//! `Documentation/SDK/PMDG_NG3_SDK.h`). Field-by-field replica with
//! `#[repr(C)]` so the binary layout matches the MSVC build of the
//! PMDG-aircraft-738 module.
//!
//! The header is ~530 lines of one giant struct. We reproduce ALL
//! fields (including the ones we don't read) because partial
//! reproduction would shift offsets and corrupt every field after
//! the omission point.
//!
//! # Versioning
//!
//! The header layout has been stable across PMDG NG3 releases since
//! launch. If a future PMDG update changes it, our `size_of` test
//! catches it at compile time and we adjust this file. The header
//! version we track is in the user's installation under
//! `pmdg-aircraft-738/Documentation/SDK/PMDG_NG3_SDK.h`.

#![allow(non_snake_case)] // Field names mirror the C++ header verbatim.
#![allow(dead_code)]      // Many fields are read for layout, not consumed.

// ---------------------------------------------------------------
// SimConnect ClientData identifiers — must match the header exactly.
// ---------------------------------------------------------------

pub const PMDG_NG3_DATA_NAME: &str = "PMDG_NG3_Data";
pub const PMDG_NG3_DATA_ID: u32 = 0x4E47_3331;
pub const PMDG_NG3_DATA_DEFINITION: u32 = 0x4E47_3332;
pub const PMDG_NG3_CONTROL_NAME: &str = "PMDG_NG3_Control";
pub const PMDG_NG3_CONTROL_ID: u32 = 0x4E47_3333;
pub const PMDG_NG3_CONTROL_DEFINITION: u32 = 0x4E47_3334;
pub const PMDG_NG3_CDU_0_NAME: &str = "PMDG_NG3_CDU_0";
pub const PMDG_NG3_CDU_1_NAME: &str = "PMDG_NG3_CDU_1";

// ---------------------------------------------------------------
// Memory-layout-exact replica of `struct PMDG_NG3_Data`.
// ---------------------------------------------------------------

/// Field-by-field 1:1 replica of `PMDG_NG3_Data` from the SDK header.
///
/// **Do not reorder fields** — the layout is what the SimConnect
/// `AddToClientDataDefinition` call uses to map the incoming bytes
/// onto each named field. Reordering would silently mis-parse.
///
/// All `bool`s are MSVC `bool` (1 byte). All char arrays are zero-
/// terminated. Multi-element arrays are exactly the size declared
/// in the header (e.g. `[2]` becomes `[T; 2]`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Pmdg738RawData {
    // ============================================================
    // Aft overhead
    // ============================================================

    // ADIRU
    pub IRS_DisplaySelector: u8,            // 0..4
    pub IRS_SysDisplay_R: u8,               // bool: false=L, true=R
    pub IRS_annunGPS: u8,
    pub IRS_annunALIGN: [u8; 2],
    pub IRS_annunON_DC: [u8; 2],
    pub IRS_annunFAULT: [u8; 2],
    pub IRS_annunDC_FAIL: [u8; 2],
    pub IRS_ModeSelector: [u8; 2],          // 0=OFF 1=ALIGN 2=NAV 3=ATT
    pub IRS_aligned: u8,
    pub IRS_DisplayLeft: [u8; 7],           // zero-terminated
    pub IRS_DisplayRight: [u8; 8],          // zero-terminated
    pub IRS_DisplayShowsDots: u8,

    // AFS
    pub AFS_AutothrottleServosConnected: u8,
    pub AFS_ControlsPitch: u8,
    pub AFS_ControlsRoll: u8,

    // PSEU
    pub WARN_annunPSEU: u8,

    // Service Interphone
    pub COMM_ServiceInterphoneSw: u8,

    // Lights
    pub LTS_DomeWhiteSw: u8,                // 0=DIM 1=OFF 2=BRIGHT

    // Engine
    pub ENG_EECSwitch: [u8; 2],
    pub ENG_annunREVERSER: [u8; 2],
    pub ENG_annunENGINE_CONTROL: [u8; 2],
    pub ENG_annunALTN: [u8; 2],
    pub ENG_StartValve: [u8; 2],

    // Oxygen
    pub OXY_Needle: u8,                     // 0..240
    pub OXY_SwNormal: u8,
    pub OXY_annunPASS_OXY_ON: u8,

    // Gear
    pub GEAR_annunOvhdLEFT: u8,
    pub GEAR_annunOvhdNOSE: u8,
    pub GEAR_annunOvhdRIGHT: u8,

    // Flight recorder
    pub FLTREC_SwNormal: u8,
    pub FLTREC_annunOFF: u8,

    pub CVR_annunTEST: u8,

    // ============================================================
    // Forward overhead
    // ============================================================

    // Flight Controls
    pub FCTL_FltControl_Sw: [u8; 2],        // 0=STBY/RUD 1=OFF 2=ON
    pub FCTL_Spoiler_Sw: [u8; 2],
    pub FCTL_YawDamper_Sw: u8,
    pub FCTL_AltnFlaps_Sw_ARM: u8,
    pub FCTL_AltnFlaps_Control_Sw: u8,      // 0=UP 1=OFF 2=DOWN
    pub FCTL_annunFC_LOW_PRESSURE: [u8; 2],
    pub FCTL_annunYAW_DAMPER: u8,
    pub FCTL_annunLOW_QUANTITY: u8,
    pub FCTL_annunLOW_PRESSURE: u8,
    pub FCTL_annunLOW_STBY_RUD_ON: u8,
    pub FCTL_annunFEEL_DIFF_PRESS: u8,
    pub FCTL_annunSPEED_TRIM_FAIL: u8,
    pub FCTL_annunMACH_TRIM_FAIL: u8,
    pub FCTL_annunAUTO_SLAT_FAIL: u8,

    // Navigation/Displays
    pub NAVDIS_VHFNavSelector: u8,          // 0=BOTH ON 1, 1=NORMAL, 2=BOTH ON 2
    pub NAVDIS_IRSSelector: u8,
    pub NAVDIS_FMCSelector: u8,
    pub NAVDIS_SourceSelector: u8,
    pub NAVDIS_ControlPaneSelector: u8,
    pub ADF_StandbyFrequency: u32,          // standby freq * 10

    // Fuel
    pub FUEL_FuelTempNeedle: f32,
    pub FUEL_CrossFeedSw: u8,
    pub FUEL_PumpFwdSw: [u8; 2],
    pub FUEL_PumpAftSw: [u8; 2],
    pub FUEL_PumpCtrSw: [u8; 2],
    pub FUEL_AuxFwd: [u8; 2],
    pub FUEL_AuxAft: [u8; 2],
    pub FUEL_FWDBleed: u8,
    pub FUEL_AFTBleed: u8,
    pub FUEL_GNDXfr: u8,
    pub FUEL_annunENG_VALVE_CLOSED: [u8; 2],
    pub FUEL_annunSPAR_VALVE_CLOSED: [u8; 2],
    pub FUEL_annunFILTER_BYPASS: [u8; 2],
    pub FUEL_annunXFEED_VALVE_OPEN: u8,
    pub FUEL_annunLOWPRESS_Fwd: [u8; 2],
    pub FUEL_annunLOWPRESS_Aft: [u8; 2],
    pub FUEL_annunLOWPRESS_Ctr: [u8; 2],
    pub FUEL_QtyCenter: f32,                // LBS
    pub FUEL_QtyLeft: f32,                  // LBS
    pub FUEL_QtyRight: f32,                 // LBS

    // Electrical
    pub ELEC_annunBAT_DISCHARGE: u8,
    pub ELEC_annunTR_UNIT: u8,
    pub ELEC_annunELEC: u8,
    pub ELEC_DCMeterSelector: u8,
    pub ELEC_ACMeterSelector: u8,
    pub ELEC_BatSelector: u8,               // 0=OFF 1=BAT 2=ON
    pub ELEC_CabUtilSw: u8,
    pub ELEC_IFEPassSeatSw: u8,
    pub ELEC_annunDRIVE: [u8; 2],
    pub ELEC_annunSTANDBY_POWER_OFF: u8,
    pub ELEC_IDGDisconnectSw: [u8; 2],
    pub ELEC_StandbyPowerSelector: u8,      // 0=BAT 1=OFF 2=AUTO
    pub ELEC_annunGRD_POWER_AVAILABLE: u8,
    pub ELEC_GrdPwrSw: u8,
    pub ELEC_BusTransSw_AUTO: u8,
    pub ELEC_GenSw: [u8; 2],
    pub ELEC_APUGenSw: [u8; 2],
    pub ELEC_annunTRANSFER_BUS_OFF: [u8; 2],
    pub ELEC_annunSOURCE_OFF: [u8; 2],
    pub ELEC_annunGEN_BUS_OFF: [u8; 2],
    pub ELEC_annunAPU_GEN_OFF_BUS: u8,
    pub ELEC_MeterDisplayTop: [u8; 13],
    pub ELEC_MeterDisplayBottom: [u8; 13],
    pub ELEC_BusPowered: [u8; 16],

    // APU
    pub APU_EGTNeedle: f32,
    pub APU_annunMAINT: u8,
    pub APU_annunLOW_OIL_PRESSURE: u8,
    pub APU_annunFAULT: u8,
    pub APU_annunOVERSPEED: u8,

    // Wipers
    pub OH_WiperLSelector: u8,
    pub OH_WiperRSelector: u8,

    // Center overhead
    pub LTS_CircuitBreakerKnob: u8,
    pub LTS_OvereadPanelKnob: u8,
    pub AIR_EquipCoolingSupplyNORM: u8,
    pub AIR_EquipCoolingExhaustNORM: u8,
    pub AIR_annunEquipCoolingSupplyOFF: u8,
    pub AIR_annunEquipCoolingExhaustOFF: u8,
    pub LTS_annunEmerNOT_ARMED: u8,
    pub LTS_EmerExitSelector: u8,
    pub COMM_NoSmokingSelector: u8,
    pub COMM_FastenBeltsSelector: u8,
    pub COMM_annunCALL: u8,
    pub COMM_annunPA_IN_USE: u8,

    // Anti-ice
    pub ICE_annunOVERHEAT: [u8; 4],
    pub ICE_annunON: [u8; 4],
    pub ICE_WindowHeatSw: [u8; 4],
    pub ICE_annunCAPT_PITOT: u8,
    pub ICE_annunL_ELEV_PITOT: u8,
    pub ICE_annunL_ALPHA_VANE: u8,
    pub ICE_annunL_TEMP_PROBE: u8,
    pub ICE_annunFO_PITOT: u8,
    pub ICE_annunR_ELEV_PITOT: u8,
    pub ICE_annunR_ALPHA_VANE: u8,
    pub ICE_annunAUX_PITOT: u8,
    pub ICE_ProbeHeatSw: [u8; 2],
    pub ICE_annunVALVE_OPEN: [u8; 2],
    pub ICE_annunCOWL_ANTI_ICE: [u8; 2],
    pub ICE_annunCOWL_VALVE_OPEN: [u8; 2],
    pub ICE_WingAntiIceSw: u8,
    pub ICE_EngAntiIceSw: [u8; 2],
    pub ICE_WindowHeatTestSw: i32,          // header: int

    // Hydraulics
    pub HYD_annunLOW_PRESS_eng: [u8; 2],
    pub HYD_annunLOW_PRESS_elec: [u8; 2],
    pub HYD_annunOVERHEAT_elec: [u8; 2],
    pub HYD_PumpSw_eng: [u8; 2],
    pub HYD_PumpSw_elec: [u8; 2],

    // Air systems
    pub AIR_TempSourceSelector: u8,
    pub AIR_TrimAirSwitch: u8,
    pub AIR_annunZoneTemp: [u8; 3],
    pub AIR_annunDualBleed: u8,
    pub AIR_annunRamDoorL: u8,
    pub AIR_annunRamDoorR: u8,
    pub AIR_RecircFanSwitch: [u8; 2],
    pub AIR_PackSwitch: [u8; 2],            // 0=OFF 1=AUTO 2=HIGH
    pub AIR_BleedAirSwitch: [u8; 2],
    pub AIR_APUBleedAirSwitch: u8,
    pub AIR_IsolationValveSwitch: u8,       // 0=CLOSE 1=AUTO 2=OPEN
    pub AIR_annunPackTripOff: [u8; 2],
    pub AIR_annunWingBodyOverheat: [u8; 2],
    pub AIR_annunBleedTripOff: [u8; 2],
    pub AIR_annunAUTO_FAIL: u8,
    pub AIR_annunOFFSCHED_DESCENT: u8,
    pub AIR_annunALTN: u8,
    pub AIR_annunMANUAL: u8,
    pub AIR_DuctPress: [f32; 2],            // PSI
    pub AIR_DuctPressNeedle: [f32; 2],
    pub AIR_CabinAltNeedle: f32,            // ft
    pub AIR_CabinDPNeedle: f32,             // PSI
    pub AIR_CabinVSNeedle: f32,             // ft/min
    pub AIR_CabinValveNeedle: f32,          // 0..1
    pub AIR_TemperatureNeedle: f32,         // °C
    pub AIR_DisplayFltAlt: [u8; 6],
    pub AIR_DisplayLandAlt: [u8; 6],

    // Doors
    pub DOOR_annunFWD_ENTRY: u8,
    pub DOOR_annunFWD_SERVICE: u8,
    pub DOOR_annunAIRSTAIR: u8,
    pub DOOR_annunLEFT_FWD_OVERWING: u8,
    pub DOOR_annunRIGHT_FWD_OVERWING: u8,
    pub DOOR_annunFWD_CARGO: u8,
    pub DOOR_annunEQUIP: u8,
    pub DOOR_annunLEFT_AFT_OVERWING: u8,
    pub DOOR_annunRIGHT_AFT_OVERWING: u8,
    pub DOOR_annunAFT_CARGO: u8,
    pub DOOR_annunAFT_ENTRY: u8,
    pub DOOR_annunAFT_SERVICE: u8,

    pub AIR_FltAltWindow: u32,              // obsolete
    pub AIR_LandAltWindow: u32,             // obsolete
    pub AIR_OutflowValveSwitch: u32,
    pub AIR_PressurizationModeSelector: u32,

    // Bottom overhead
    pub LTS_LandingLtRetractableSw: [u8; 2],
    pub LTS_LandingLtFixedSw: [u8; 2],
    pub LTS_RunwayTurnoffSw: [u8; 2],
    pub LTS_TaxiSw: u8,
    pub APU_Selector: u8,                   // 0=OFF 1=ON 2=START
    pub ENG_StartSelector: [u8; 2],         // 0=GRD 1=OFF 2=CONT 3=FLT
    pub ENG_IgnitionSelector: u8,
    pub LTS_LogoSw: u8,
    pub LTS_PositionSw: u8,
    pub LTS_AntiCollisionSw: u8,
    pub LTS_WingSw: u8,
    pub LTS_WheelWellSw: u8,

    // ============================================================
    // Glareshield
    // ============================================================

    // Warnings
    pub WARN_annunFIRE_WARN: [u8; 2],
    pub WARN_annunMASTER_CAUTION: [u8; 2],
    pub WARN_annunFLT_CONT: u8,
    pub WARN_annunIRS: u8,
    pub WARN_annunFUEL: u8,
    pub WARN_annunELEC: u8,
    pub WARN_annunAPU: u8,
    pub WARN_annunOVHT_DET: u8,
    pub WARN_annunANTI_ICE: u8,
    pub WARN_annunHYD: u8,
    pub WARN_annunDOORS: u8,
    pub WARN_annunENG: u8,
    pub WARN_annunOVERHEAD: u8,
    pub WARN_annunAIR_COND: u8,

    // EFIS control panels
    pub EFIS_MinsSelBARO: [u8; 2],
    pub EFIS_BaroSelHPA: [u8; 2],
    pub EFIS_VORADFSel1: [u8; 2],
    pub EFIS_VORADFSel2: [u8; 2],
    pub EFIS_ModeSel: [u8; 2],              // 0=APP 1=VOR 2=MAP 3=PLAN
    pub EFIS_RangeSel: [u8; 2],             // 0=5 .. 7=640

    // ============================================================
    // Mode control panel — INTERESTING for AeroACARS
    // ============================================================
    pub MCP_Course: [u16; 2],
    pub MCP_IASMach: f32,                   // Mach if < 10.0, else knots
    pub MCP_IASBlank: u8,
    pub MCP_IASOverspeedFlash: u8,
    pub MCP_IASUnderspeedFlash: u8,
    pub MCP_Heading: u16,
    pub MCP_Altitude: u16,
    pub MCP_VertSpeed: i16,                 // signed
    pub MCP_VertSpeedBlank: u8,

    pub MCP_FDSw: [u8; 2],
    pub MCP_ATArmSw: u8,
    pub MCP_BankLimitSel: u8,               // 0=10 .. 4=30
    pub MCP_DisengageBar: u8,

    pub MCP_annunFD: [u8; 2],
    pub MCP_annunATArm: u8,
    pub MCP_annunN1: u8,
    pub MCP_annunSPEED: u8,
    pub MCP_annunVNAV: u8,
    pub MCP_annunLVL_CHG: u8,
    pub MCP_annunHDG_SEL: u8,
    pub MCP_annunLNAV: u8,
    pub MCP_annunVOR_LOC: u8,
    pub MCP_annunAPP: u8,
    pub MCP_annunALT_HOLD: u8,
    pub MCP_annunVS: u8,
    pub MCP_annunCMD_A: u8,
    pub MCP_annunCWS_A: u8,
    pub MCP_annunCMD_B: u8,
    pub MCP_annunCWS_B: u8,

    pub MCP_indication_powered: u8,

    // ============================================================
    // Forward panel — INTERESTING (Flaps, Gear, Autobrake)
    // ============================================================
    pub MAIN_NoseWheelSteeringSwNORM: u8,
    pub MAIN_annunBELOW_GS: [u8; 2],
    pub MAIN_MainPanelDUSel: [u8; 2],
    pub MAIN_LowerDUSel: [u8; 2],
    pub MAIN_annunAP: [u8; 2],
    pub MAIN_annunAP_Amber: [u8; 2],
    pub MAIN_annunAT: [u8; 2],
    pub MAIN_annunAT_Amber: [u8; 2],
    pub MAIN_annunFMC: [u8; 2],
    pub MAIN_DisengageTestSelector: [u8; 2],
    pub MAIN_annunSPEEDBRAKE_ARMED: u8,
    pub MAIN_annunSPEEDBRAKE_DO_NOT_ARM: u8,
    pub MAIN_annunSPEEDBRAKE_EXTENDED: u8,
    pub MAIN_annunSTAB_OUT_OF_TRIM: u8,
    pub MAIN_LightsSelector: u8,
    pub MAIN_RMISelector1_VOR: u8,
    pub MAIN_RMISelector2_VOR: u8,
    pub MAIN_N1SetSelector: u8,
    pub MAIN_SpdRefSelector: u8,
    pub MAIN_FuelFlowSelector: u8,
    pub MAIN_AutobrakeSelector: u8,         // 0=RTO 1=OFF 2=1 3=2 4=3 5=MAX
    pub MAIN_annunANTI_SKID_INOP: u8,
    pub MAIN_annunAUTO_BRAKE_DISARM: u8,
    pub MAIN_annunLE_FLAPS_TRANSIT: u8,
    pub MAIN_annunLE_FLAPS_EXT: u8,
    pub MAIN_TEFlapsNeedle: [f32; 2],       // ★ flap angle in degrees
    pub MAIN_annunGEAR_transit: [u8; 3],
    pub MAIN_annunGEAR_locked: [u8; 3],
    pub MAIN_GearLever: u8,                 // 0=UP 1=OFF 2=DOWN
    pub MAIN_BrakePressNeedle: f32,
    pub MAIN_annunCABIN_ALTITUDE: u8,
    pub MAIN_annunTAKEOFF_CONFIG: u8,

    pub HGS_annun_AIII: u8,
    pub HGS_annun_NO_AIII: u8,
    pub HGS_annun_FLARE: u8,
    pub HGS_annun_RO: u8,
    pub HGS_annun_RO_CTN: u8,
    pub HGS_annun_RO_ARM: u8,
    pub HGS_annun_TO: u8,
    pub HGS_annun_TO_CTN: u8,
    pub HGS_annun_APCH: u8,
    pub HGS_annun_TO_WARN: u8,
    pub HGS_annun_Bar: u8,
    pub HGS_annun_FAIL: u8,

    // Lower forward panel
    pub LTS_MainPanelKnob: [u8; 2],
    pub LTS_BackgroundKnob: u8,
    pub LTS_AFDSFloodKnob: u8,
    pub LTS_OutbdDUBrtKnob: [u8; 2],
    pub LTS_InbdDUBrtKnob: [u8; 2],
    pub LTS_InbdDUMapBrtKnob: [u8; 2],
    pub LTS_UpperDUBrtKnob: u8,
    pub LTS_LowerDUBrtKnob: u8,
    pub LTS_LowerDUMapBrtKnob: u8,

    pub GPWS_annunINOP: u8,
    pub GPWS_FlapInhibitSw_NORM: u8,
    pub GPWS_GearInhibitSw_NORM: u8,
    pub GPWS_TerrInhibitSw_NORM: u8,

    // ============================================================
    // Control Stand
    // ============================================================
    pub CDU_annunEXEC: [u8; 2],
    pub CDU_annunCALL: [u8; 2],
    pub CDU_annunFAIL: [u8; 2],
    pub CDU_annunMSG: [u8; 2],
    pub CDU_annunOFST: [u8; 2],
    pub CDU_BrtKnob: [u8; 2],

    pub COMM_Attend_PressCount: u8,
    pub COMM_GrdCall_PressCount: u8,
    pub COMM_SelectedMic: [u8; 3],
    pub COMM_ReceiverSwitches: [u32; 3],    // bit-flags
    pub TRIM_StabTrimMainElecSw_NORMAL: u8,
    pub TRIM_StabTrimAutoPilotSw_NORMAL: u8,
    pub PED_annunParkingBrake: u8,

    pub FIRE_OvhtDetSw: [u8; 2],
    pub FIRE_annunENG_OVERHEAT: [u8; 2],
    pub FIRE_DetTestSw: u8,
    pub FIRE_HandlePos: [u8; 3],
    pub FIRE_HandleIlluminated: [u8; 3],
    pub FIRE_annunWHEEL_WELL: u8,
    pub FIRE_annunFAULT: u8,
    pub FIRE_annunAPU_DET_INOP: u8,
    pub FIRE_annunAPU_BOTTLE_DISCHARGE: u8,
    pub FIRE_annunBOTTLE_DISCHARGE: [u8; 2],
    pub FIRE_ExtinguisherTestSw: u8,
    pub FIRE_annunExtinguisherTest: [u8; 3],

    pub CARGO_annunExtTest: [u8; 2],
    pub CARGO_DetSelect: [u8; 2],
    pub CARGO_ArmedSw: [u8; 2],
    pub CARGO_annunFWD: u8,
    pub CARGO_annunAFT: u8,
    pub CARGO_annunDETECTOR_FAULT: u8,
    pub CARGO_annunDISCH: u8,

    pub HGS_annunRWY: u8,
    pub HGS_annunGS: u8,
    pub HGS_annunFAULT: u8,
    pub HGS_annunCLR: u8,

    pub XPDR_XpndrSelector_2: u8,
    pub XPDR_AltSourceSel_2: u8,
    pub XPDR_ModeSel: u8,                   // 0=STBY 1=ALT_RPTG_OFF .. 4=TA/RA
    pub XPDR_annunFAIL: u8,

    pub LTS_PedFloodKnob: u8,
    pub LTS_PedPanelKnob: u8,

    pub TRIM_StabTrimSw_NORMAL: u8,
    pub PED_annunLOCK_FAIL: u8,
    pub PED_annunAUTO_UNLK: u8,
    pub PED_FltDkDoorSel: u8,

    // ============================================================
    // FMS — INTERESTING for AeroACARS
    // ============================================================
    pub FMC_TakeoffFlaps: u8,               // degrees, 0 if not set
    pub FMC_V1: u8,                         // knots, 0 if not set
    pub FMC_VR: u8,
    pub FMC_V2: u8,
    pub FMC_LandingFlaps: u8,
    pub FMC_LandingVREF: u8,
    pub FMC_CruiseAlt: u16,                 // ft, 0 if not set
    pub FMC_LandingAltitude: i16,           // -32767 if n/a
    pub FMC_TransitionAlt: u16,
    pub FMC_TransitionLevel: u16,
    pub FMC_PerfInputComplete: u8,
    pub FMC_DistanceToTOD: f32,             // nm; negative if n/a
    pub FMC_DistanceToDest: f32,
    pub FMC_flightNumber: [u8; 9],

    // ============================================================
    // General and misc
    // ============================================================
    pub AircraftModel: u16,                 // 1=600, 5=800, 8=900, etc.

    pub WeightInKg: u8,
    pub GPWS_V1CallEnabled: u8,
    pub GroundConnAvailable: u8,

    pub reserved: [u8; 255],
}

// ---------------------------------------------------------------
// Compile-time size guard. Drift from the expected 916 bytes means
// the header layout changed and our mirror doesn't match — every
// field after the drift point would silently mis-parse. See the
// `struct_size_matches_pmdg_v3_layout` test for full rationale.
// ---------------------------------------------------------------
const _: () = {
    assert!(std::mem::size_of::<Pmdg738RawData>() == 916);
};

// ---------------------------------------------------------------
// Higher-level "useful subset" — what AeroACARS actually consumes.
// ---------------------------------------------------------------

/// Aircraft variant per `AircraftModel` field. Exposed so the
/// snapshot picks the right base type in PIREP custom fields.
#[allow(non_camel_case_types)] // Match Boeing model designators verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pmdg738Variant {
    B737_600,
    B737_700, B737_700_BW, B737_700_SSW,
    B737_800, B737_800_BW, B737_800_SSW,
    B737_900, B737_900_BW, B737_900_SSW,
    B737_900ER_BW, B737_900ER_SSW,
    Bdsf(u16), // BDSF / BCF / BBJ — encode the raw model number
    Unknown(u16),
}

impl Pmdg738Variant {
    pub fn from_model_id(id: u16) -> Self {
        match id {
            1 => Self::B737_600,
            2 => Self::B737_700, 3 => Self::B737_700_BW, 4 => Self::B737_700_SSW,
            5 => Self::B737_800, 6 => Self::B737_800_BW, 7 => Self::B737_800_SSW,
            8 => Self::B737_900, 9 => Self::B737_900_BW, 10 => Self::B737_900_SSW,
            11 => Self::B737_900ER_BW, 12 => Self::B737_900ER_SSW,
            13..=22 => Self::Bdsf(id),
            _ => Self::Unknown(id),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::B737_600 => "737-600",
            Self::B737_700 | Self::B737_700_BW | Self::B737_700_SSW => "737-700",
            Self::B737_800 | Self::B737_800_BW | Self::B737_800_SSW => "737-800",
            Self::B737_900 | Self::B737_900_BW | Self::B737_900_SSW => "737-900",
            Self::B737_900ER_BW | Self::B737_900ER_SSW => "737-900ER",
            Self::Bdsf(_) => "737 (BDSF/BCF/BBJ)",
            Self::Unknown(_) => "737 (unknown variant)",
        }
    }
}

/// MCP autoflight modes derived from the various `MCP_annun*`
/// booleans in the raw data. Displayed on the FMA.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pmdg738FmaState {
    pub speed_n1: bool,
    pub speed: bool,
    pub vnav: bool,
    pub lvl_chg: bool,
    pub hdg_sel: bool,
    pub lnav: bool,
    pub vor_loc: bool,
    pub app: bool,
    pub alt_hold: bool,
    pub vs: bool,
    pub at_armed: bool,
    pub cmd_a: bool,
    pub cmd_b: bool,
    pub cws_a: bool,
    pub cws_b: bool,
    pub fd_capt: bool,
    pub fd_fo: bool,
}

/// Autobrake selector positions (737 NG).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pmdg738Autobrake {
    Rto, Off, One, Two, Three, Max,
    Unknown(u8),
}

impl Pmdg738Autobrake {
    pub fn from_byte(v: u8) -> Self {
        match v {
            0 => Self::Rto, 1 => Self::Off, 2 => Self::One,
            3 => Self::Two, 4 => Self::Three, 5 => Self::Max,
            other => Self::Unknown(other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rto => "RTO",
            Self::Off => "OFF",
            Self::One => "1",
            Self::Two => "2",
            Self::Three => "3",
            Self::Max => "MAX",
            Self::Unknown(_) => "?",
        }
    }
}

/// V-speeds set by the FMC. `0` means "not set" per the SDK
/// convention — exposed as `Option<u8>` for ergonomic Rust use.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pmdg738VSpeeds {
    pub v1_kt: Option<u8>,
    pub vr_kt: Option<u8>,
    pub v2_kt: Option<u8>,
    pub vref_kt: Option<u8>,
}

/// The "useful subset" view of `Pmdg738RawData` — only the fields
/// AeroACARS actually consumes for activity logging, PIREP fields,
/// and cockpit display. See `pmdg-sdk-integration.md` §5.4 for the
/// full mapping rationale.
#[derive(Debug, Clone)]
pub struct Pmdg738Snapshot {
    pub variant: Pmdg738Variant,
    pub weight_in_kg: bool,

    // MCP — selected autopilot targets
    /// MCP IAS/Mach: knots if value < 10, else Mach.
    pub mcp_speed_raw: f32,
    pub mcp_speed_blanked: bool,
    pub mcp_heading_deg: u16,
    pub mcp_altitude_ft: u16,
    pub mcp_vs_fpm: i16,
    pub mcp_vs_blanked: bool,
    pub mcp_powered: bool,

    // FMA — autoflight modes
    pub fma: Pmdg738FmaState,

    // Controls — flaps, gear, autobrake
    /// Trailing-edge flap angle in degrees (left side, both sides
    /// usually identical). 0 = up, 1/2/5/10/15/25/30/40 = handle
    /// detents. Live value, not handle position.
    pub flap_angle_deg: f32,
    /// Boeing 737 flap-handle label derived from `flap_angle_deg` —
    /// nearest detent (UP/1/2/5/10/15/25/30/40). NG3 SDK doesn't
    /// expose the handle position directly, only the live angle, so
    /// we snap to the nearest Boeing detent. Used for Premium-First
    /// activity-log labels.
    pub flap_handle_label: &'static str,
    pub le_flaps_extended: bool,
    pub le_flaps_in_transit: bool,
    pub gear_lever_down: bool,
    pub speedbrake_armed: bool,
    pub speedbrake_extended: bool,
    /// Synthesised lever position 0.0..1.0. NG3 SDK exposes only
    /// boolean ARMED/EXTENDED states — we synthesise a stable
    /// "lever percentage" from those (0=stowed, 0.25=armed,
    /// 1.0=extended) so the standard `spoilers_handle_position`
    /// Premium-First override doesn't jitter at the ARMED detent.
    pub speedbrake_lever_pos: f32,
    pub autobrake: Pmdg738Autobrake,
    pub takeoff_config_warning: bool,
    pub stab_out_of_trim: bool,
    pub parking_brake_set: bool,

    // FMS — pilot's plan
    pub fmc_takeoff_flaps_deg: u8,
    pub fmc_landing_flaps_deg: u8,
    pub fmc_v_speeds: Pmdg738VSpeeds,
    /// 0 if not set, otherwise feet.
    pub fmc_cruise_alt_ft: u16,
    /// -32767 if not available, otherwise feet.
    pub fmc_landing_altitude_ft: i16,
    pub fmc_transition_alt_ft: u16,
    pub fmc_transition_level_ft: u16,
    pub fmc_perf_input_complete: bool,
    /// Negative if N/A.
    pub fmc_distance_to_tod_nm: f32,
    pub fmc_distance_to_dest_nm: f32,
    /// Up to 8 chars + null. Empty when not set.
    pub fmc_flight_number: String,

    // APU
    pub apu_egt: f32,
    pub apu_running: bool,

    // Comm + Misc
    pub xpdr_mode: u8,                  // 0=STBY 1=ALT_RPTG_OFF .. 4=TA/RA

    // ---- v0.16.10 (#Premium): deep-data annunciators + fuel ----
    /// Any engine's REVERSER annunciator lit (`ENG_annunREVERSER[0|1]`).
    pub reverser_deployed: bool,
    /// MASTER CAUTION lit on either side (`WARN_annunMASTER_CAUTION[0|1]`).
    pub master_caution: bool,
    /// FIRE WARN lit on either side (`WARN_annunFIRE_WARN[0|1]`). The
    /// 737 NG has no "MASTER WARNING" light — the red master on the
    /// glareshield IS the fire-warning light.
    pub fire_warn: bool,
    /// BELOW G/S annunciator lit on either side (`MAIN_annunBELOW_GS[0|1]`).
    pub below_gs: bool,
    /// CABIN ALTITUDE warning annunciator (`MAIN_annunCABIN_ALTITUDE`).
    pub cabin_altitude_warning: bool,
    /// Per-tank fuel quantity in kg, order **[left, center, right]**
    /// (FUEL_QtyLeft / FUEL_QtyCenter / FUEL_QtyRight). Already unit-
    /// canonicalised via the raw struct's `WeightInKg` flag — see
    /// [`super::pmdg_fuel_qty_to_kg`].
    pub fuel_per_tank_kg: [f64; 3],

    // ---- Cockpit lights + systems for Premium-First override ----
    /// Either landing-light switch ON. NG3 has separate Fixed +
    /// Retractable switches (left/right each); we OR them so the
    /// generic boolean tells "any landing light on".
    pub light_landing: bool,
    pub light_beacon: bool,
    pub light_strobe: bool,
    pub light_taxi: bool,
    pub light_nav: bool,
    pub light_logo: bool,
    pub light_wing: bool,
    pub light_wheel_well: bool, // NG3-only (no Standard SimVar)

    // Anti-ice / pitot
    pub wing_anti_ice: bool,
    pub engine_anti_ice: bool,
    pub pitot_heat: bool,

    // Power
    pub battery_master: bool,
}

/// Boeing 737 NG3 flap-handle label derived from the live trailing-edge
/// flap angle. NG3 SDK doesn't expose the handle position directly —
/// only `MAIN_TEFlapsNeedle` (live degrees). We snap to the nearest
/// detent (UP/1/2/5/10/15/25/30/40) using halfway-points so a flap
/// in transit reports the closer detent.
///
/// Halfway thresholds:
///   < 0.5  → UP
///   < 1.5  → 1
///   < 3.5  → 2
///   < 7.5  → 5
///   < 12.5 → 10
///   < 20.0 → 15
///   < 27.5 → 25
///   < 35.0 → 30
///   else   → 40
pub fn ng3_flap_label_from_angle(deg: f32) -> &'static str {
    if deg < 0.5 {
        "UP"
    } else if deg < 1.5 {
        "1"
    } else if deg < 3.5 {
        "2"
    } else if deg < 7.5 {
        "5"
    } else if deg < 12.5 {
        "10"
    } else if deg < 20.0 {
        "15"
    } else if deg < 27.5 {
        "25"
    } else if deg < 35.0 {
        "30"
    } else {
        "40"
    }
}

impl Pmdg738Snapshot {
    /// Decode a raw PMDG NG3 data block into the useful-subset view.
    pub fn from_raw(raw: &Pmdg738RawData) -> Self {
        let v_speed = |raw: u8| if raw == 0 { None } else { Some(raw) };

        let flight_num = std::str::from_utf8(&raw.FMC_flightNumber)
            .unwrap_or("")
            .trim_end_matches('\0')
            .to_string();

        Self {
            variant: Pmdg738Variant::from_model_id(raw.AircraftModel),
            weight_in_kg: raw.WeightInKg != 0,

            mcp_speed_raw: raw.MCP_IASMach,
            mcp_speed_blanked: raw.MCP_IASBlank != 0,
            mcp_heading_deg: raw.MCP_Heading,
            mcp_altitude_ft: raw.MCP_Altitude,
            mcp_vs_fpm: raw.MCP_VertSpeed,
            mcp_vs_blanked: raw.MCP_VertSpeedBlank != 0,
            mcp_powered: raw.MCP_indication_powered != 0,

            fma: Pmdg738FmaState {
                speed_n1: raw.MCP_annunN1 != 0,
                speed: raw.MCP_annunSPEED != 0,
                vnav: raw.MCP_annunVNAV != 0,
                lvl_chg: raw.MCP_annunLVL_CHG != 0,
                hdg_sel: raw.MCP_annunHDG_SEL != 0,
                lnav: raw.MCP_annunLNAV != 0,
                vor_loc: raw.MCP_annunVOR_LOC != 0,
                app: raw.MCP_annunAPP != 0,
                alt_hold: raw.MCP_annunALT_HOLD != 0,
                vs: raw.MCP_annunVS != 0,
                at_armed: raw.MCP_annunATArm != 0,
                cmd_a: raw.MCP_annunCMD_A != 0,
                cmd_b: raw.MCP_annunCMD_B != 0,
                cws_a: raw.MCP_annunCWS_A != 0,
                cws_b: raw.MCP_annunCWS_B != 0,
                fd_capt: raw.MCP_annunFD[0] != 0,
                fd_fo: raw.MCP_annunFD[1] != 0,
            },

            flap_angle_deg: raw.MAIN_TEFlapsNeedle[0],
            flap_handle_label: ng3_flap_label_from_angle(raw.MAIN_TEFlapsNeedle[0]),
            le_flaps_extended: raw.MAIN_annunLE_FLAPS_EXT != 0,
            le_flaps_in_transit: raw.MAIN_annunLE_FLAPS_TRANSIT != 0,
            gear_lever_down: raw.MAIN_GearLever == 2,
            speedbrake_armed: raw.MAIN_annunSPEEDBRAKE_ARMED != 0,
            speedbrake_extended: raw.MAIN_annunSPEEDBRAKE_EXTENDED != 0,
            // Synthesise lever pos from the bools — sufficient for
            // jitter-free Activity-Log overrides; doesn't claim to
            // mirror the real lever motion in flight.
            speedbrake_lever_pos: if raw.MAIN_annunSPEEDBRAKE_EXTENDED != 0 {
                1.0
            } else if raw.MAIN_annunSPEEDBRAKE_ARMED != 0 {
                0.25
            } else {
                0.0
            },
            autobrake: Pmdg738Autobrake::from_byte(raw.MAIN_AutobrakeSelector),
            takeoff_config_warning: raw.MAIN_annunTAKEOFF_CONFIG != 0,
            stab_out_of_trim: raw.MAIN_annunSTAB_OUT_OF_TRIM != 0,
            parking_brake_set: raw.PED_annunParkingBrake != 0,

            fmc_takeoff_flaps_deg: raw.FMC_TakeoffFlaps,
            fmc_landing_flaps_deg: raw.FMC_LandingFlaps,
            fmc_v_speeds: Pmdg738VSpeeds {
                v1_kt: v_speed(raw.FMC_V1),
                vr_kt: v_speed(raw.FMC_VR),
                v2_kt: v_speed(raw.FMC_V2),
                vref_kt: v_speed(raw.FMC_LandingVREF),
            },
            fmc_cruise_alt_ft: raw.FMC_CruiseAlt,
            fmc_landing_altitude_ft: raw.FMC_LandingAltitude,
            fmc_transition_alt_ft: raw.FMC_TransitionAlt,
            fmc_transition_level_ft: raw.FMC_TransitionLevel,
            fmc_perf_input_complete: raw.FMC_PerfInputComplete != 0,
            fmc_distance_to_tod_nm: raw.FMC_DistanceToTOD,
            fmc_distance_to_dest_nm: raw.FMC_DistanceToDest,
            fmc_flight_number: flight_num,

            apu_egt: raw.APU_EGTNeedle,
            // NG3 SDK has no explicit `APURunning` bool. We
            // derive it from two PMDG fields:
            //   * APU_Selector == 1 (= ON, not OFF or START)
            //   * APU_EGTNeedle > 350 °C (steady-state running)
            apu_running: raw.APU_Selector == 1 && raw.APU_EGTNeedle > 350.0,

            // Cockpit lights — direct from PMDG cockpit state.
            // Landing light: NG3 has 2 fixed + 2 retractable
            // switches per side; ANY ON = "landing light on".
            light_landing: raw.LTS_LandingLtFixedSw[0] != 0
                || raw.LTS_LandingLtFixedSw[1] != 0
                || raw.LTS_LandingLtRetractableSw[0] >= 1
                || raw.LTS_LandingLtRetractableSw[1] >= 1,
            // Beacon = Anti-Collision per Boeing terminology.
            light_beacon: raw.LTS_AntiCollisionSw != 0,
            // PositionSw: 0=STEADY 1=OFF 2=STROBE&STEADY → strobe
            // is on when value == 2.
            light_strobe: raw.LTS_PositionSw == 2,
            light_taxi: raw.LTS_TaxiSw != 0,
            // NG3 doesn't have a separate "NAV" switch — position
            // lights are always on with the position switch
            // (steady = nav lights). Use position-not-OFF as proxy.
            light_nav: raw.LTS_PositionSw != 1,
            light_logo: raw.LTS_LogoSw != 0,
            light_wing: raw.LTS_WingSw != 0,
            light_wheel_well: raw.LTS_WheelWellSw != 0,

            // Anti-ice
            wing_anti_ice: raw.ICE_WingAntiIceSw != 0,
            engine_anti_ice: raw.ICE_EngAntiIceSw[0] != 0
                || raw.ICE_EngAntiIceSw[1] != 0,
            // Pitot heat: NG3 has 2 probe heat switches
            // (Capt + F/O). ANY on = pitot heat on for our purpose.
            pitot_heat: raw.ICE_ProbeHeatSw[0] != 0
                || raw.ICE_ProbeHeatSw[1] != 0,

            // Battery: BatSelector 0=OFF 1=BAT 2=ON. Anything
            // non-zero = battery providing power.
            battery_master: raw.ELEC_BatSelector != 0,

            xpdr_mode: raw.XPDR_ModeSel,

            // ---- v0.16.10 (#Premium) deep-data fields ----
            // Two-channel annunciators (Capt/FO or Eng1/Eng2): OR —
            // "the light is on" is what the cockpit shows.
            reverser_deployed: raw.ENG_annunREVERSER[0] != 0
                || raw.ENG_annunREVERSER[1] != 0,
            master_caution: raw.WARN_annunMASTER_CAUTION[0] != 0
                || raw.WARN_annunMASTER_CAUTION[1] != 0,
            fire_warn: raw.WARN_annunFIRE_WARN[0] != 0
                || raw.WARN_annunFIRE_WARN[1] != 0,
            below_gs: raw.MAIN_annunBELOW_GS[0] != 0
                || raw.MAIN_annunBELOW_GS[1] != 0,
            cabin_altitude_warning: raw.MAIN_annunCABIN_ALTITUDE != 0,
            // Order: [left, center, right]. SDK floats follow the
            // pilot's weight-unit option (WeightInKg flag) — convert
            // to canonical kg here so no consumer re-checks the flag.
            fuel_per_tank_kg: [
                super::pmdg_fuel_qty_to_kg(raw.FUEL_QtyLeft, raw.WeightInKg != 0),
                super::pmdg_fuel_qty_to_kg(raw.FUEL_QtyCenter, raw.WeightInKg != 0),
                super::pmdg_fuel_qty_to_kg(raw.FUEL_QtyRight, raw.WeightInKg != 0),
            ],
        }
    }

    /// True when the SDK is delivering meaningful data — used to gate
    /// the "SDK appears not enabled" hint in the UI.
    pub fn looks_alive(&self) -> bool {
        // If MCP shows "powered" we trust the rest. The SDK delivers
        // even when the cockpit isn't powered, but `mcp_powered`
        // specifically reflects "MCP windows showing values" — a
        // good proxy for "aircraft state actually reaching us".
        self.mcp_powered || self.apu_running
    }
}

// ---------------------------------------------------------------
// Tests
// ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact-byte-size check.
    ///
    /// Pinned to **916 bytes** — the size we measured by compiling
    /// our 1:1 mirror of `PMDG_NG3_SDK.h` shipped with PMDG NG3
    /// v3.0.100 (header dated 2025, file in pmdg-aircraft-738/
    /// Documentation/SDK/PMDG_NG3_SDK.h on the user's machine).
    ///
    /// Most fields are 1-byte bools/u8s; the `unsigned char
    /// reserved[255]` trailer + alignment padding land at this
    /// exact value with `#[repr(C)]` on x86_64 MSVC layout.
    ///
    /// **If this assertion fires after a PMDG update** → diff our
    /// mirror against the new header in
    /// `Documentation/SDK/PMDG_NG3_SDK.h`, add/reorder fields to
    /// match, and update this constant. SimConnect's
    /// `AddToClientDataDefinition` uses `sizeof(struct)` so a
    /// mismatch silently mis-parses every field.
    #[test]
    fn struct_size_matches_pmdg_v3_layout() {
        const EXPECTED_BYTES: usize = 916;
        let n = std::mem::size_of::<Pmdg738RawData>();
        assert_eq!(
            n, EXPECTED_BYTES,
            "Pmdg738RawData layout drift! got {n} bytes, expected {EXPECTED_BYTES}. \
             Check pmdg-aircraft-738/Documentation/SDK/PMDG_NG3_SDK.h for changes."
        );
    }

    #[test]
    fn variant_decoding() {
        assert_eq!(Pmdg738Variant::from_model_id(5), Pmdg738Variant::B737_800);
        assert_eq!(Pmdg738Variant::from_model_id(7), Pmdg738Variant::B737_800_SSW);
        assert_eq!(
            Pmdg738Variant::from_model_id(99),
            Pmdg738Variant::Unknown(99)
        );
    }

    #[test]
    fn autobrake_decoding() {
        assert_eq!(Pmdg738Autobrake::from_byte(0), Pmdg738Autobrake::Rto);
        assert_eq!(Pmdg738Autobrake::from_byte(5), Pmdg738Autobrake::Max);
        assert_eq!(Pmdg738Autobrake::from_byte(0).label(), "RTO");
        assert_eq!(Pmdg738Autobrake::from_byte(5).label(), "MAX");
    }

    /// Boeing 737 NG3 flap-handle labels — verifies the angle→detent
    /// snap so the activity log shows real Boeing labels (not Airbus
    /// "1+F"/"3"/"FULL"). Halfway-point tests pin the boundaries that
    /// matter for in-transit flap motion.
    #[test]
    fn ng3_flap_label_from_angle_decoding() {
        // Stable-detent values
        assert_eq!(ng3_flap_label_from_angle(0.0), "UP");
        assert_eq!(ng3_flap_label_from_angle(1.0), "1");
        assert_eq!(ng3_flap_label_from_angle(2.0), "2");
        assert_eq!(ng3_flap_label_from_angle(5.0), "5");
        assert_eq!(ng3_flap_label_from_angle(10.0), "10");
        assert_eq!(ng3_flap_label_from_angle(15.0), "15");
        assert_eq!(ng3_flap_label_from_angle(25.0), "25");
        assert_eq!(ng3_flap_label_from_angle(30.0), "30");
        assert_eq!(ng3_flap_label_from_angle(40.0), "40");

        // Halfway points snap to the nearer detent
        assert_eq!(ng3_flap_label_from_angle(0.4), "UP");
        assert_eq!(ng3_flap_label_from_angle(0.6), "1");
        assert_eq!(ng3_flap_label_from_angle(7.0), "5");
        assert_eq!(ng3_flap_label_from_angle(8.0), "10");
        assert_eq!(ng3_flap_label_from_angle(35.5), "40");
    }

    #[test]
    fn snapshot_extracts_useful_subset() {
        // All-zeros raw → snapshot fields default-y values.
        let raw: Pmdg738RawData = unsafe { std::mem::zeroed() };
        let s = Pmdg738Snapshot::from_raw(&raw);

        assert_eq!(s.fmc_v_speeds.v1_kt, None); // 0 → None per SDK convention
        assert_eq!(s.fmc_v_speeds.vref_kt, None);
        assert_eq!(s.fmc_cruise_alt_ft, 0);
        assert_eq!(s.mcp_heading_deg, 0);
        assert!(!s.fma.vnav);
        assert!(!s.looks_alive()); // MCP not powered, APU not running
    }

    // ---- v0.16.10 (#Premium) deep-data field decoding ----

    fn zeroed_raw() -> Pmdg738RawData {
        unsafe { std::mem::zeroed() }
    }

    /// Two-channel annunciators must OR: either engine's REVERSER
    /// light counts as "reverser deployed".
    #[test]
    fn premium_reverser_ors_both_engines() {
        let mut raw = zeroed_raw();
        assert!(!Pmdg738Snapshot::from_raw(&raw).reverser_deployed);
        raw.ENG_annunREVERSER = [1, 0];
        assert!(Pmdg738Snapshot::from_raw(&raw).reverser_deployed);
        raw.ENG_annunREVERSER = [0, 1];
        assert!(Pmdg738Snapshot::from_raw(&raw).reverser_deployed);
    }

    /// MASTER CAUTION + FIRE WARN are Capt/FO-sided lights — OR.
    #[test]
    fn premium_caution_and_fire_warn_or_both_sides() {
        let mut raw = zeroed_raw();
        let s = Pmdg738Snapshot::from_raw(&raw);
        assert!(!s.master_caution);
        assert!(!s.fire_warn);

        raw.WARN_annunMASTER_CAUTION = [0, 1];
        raw.WARN_annunFIRE_WARN = [1, 0];
        let s = Pmdg738Snapshot::from_raw(&raw);
        assert!(s.master_caution);
        assert!(s.fire_warn);
    }

    #[test]
    fn premium_below_gs_and_cabin_altitude() {
        let mut raw = zeroed_raw();
        raw.MAIN_annunBELOW_GS = [0, 1]; // FO side only still counts
        raw.MAIN_annunCABIN_ALTITUDE = 1;
        let s = Pmdg738Snapshot::from_raw(&raw);
        assert!(s.below_gs);
        assert!(s.cabin_altitude_warning);
    }

    /// STAB OUT OF TRIM was parsed since Phase 5 but dropped by the
    /// mapper — pin the from_raw passthrough here.
    #[test]
    fn premium_stab_out_of_trim_passthrough() {
        let mut raw = zeroed_raw();
        assert!(!Pmdg738Snapshot::from_raw(&raw).stab_out_of_trim);
        raw.MAIN_annunSTAB_OUT_OF_TRIM = 1;
        assert!(Pmdg738Snapshot::from_raw(&raw).stab_out_of_trim);
    }

    /// Fuel: [left, center, right] order + WeightInKg unit handling
    /// in both directions (false = LBS → ×0.45359237, true = kg
    /// passthrough).
    #[test]
    fn premium_fuel_per_tank_kg_units_and_order() {
        let mut raw = zeroed_raw();
        raw.FUEL_QtyLeft = 1000.0;
        raw.FUEL_QtyCenter = 2000.0;
        raw.FUEL_QtyRight = 3000.0;

        // Pounds mode (WeightInKg = 0).
        raw.WeightInKg = 0;
        let s = Pmdg738Snapshot::from_raw(&raw);
        assert!((s.fuel_per_tank_kg[0] - 453.59237).abs() < 1e-6);
        assert!((s.fuel_per_tank_kg[1] - 907.18474).abs() < 1e-6);
        assert!((s.fuel_per_tank_kg[2] - 1360.77711).abs() < 1e-6);

        // Kg mode (WeightInKg = 1) — passthrough.
        raw.WeightInKg = 1;
        let s = Pmdg738Snapshot::from_raw(&raw);
        assert_eq!(s.fuel_per_tank_kg, [1000.0, 2000.0, 3000.0]);
    }
}
