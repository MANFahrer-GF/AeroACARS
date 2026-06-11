//! PMDG 777X SimConnect SDK data structures.
//!
//! Mirrors `PMDG_777X_SDK.h` (Copyright PMDG, ships with the
//! pmdg-aircraft-77er / -77w / -77f / -77l installations under
//! `Documentation/SDK/PMDG_777X_SDK.h`).
//!
//! # Coverage
//!
//! Phase 5.1b — full struct replication for all four 777 variants
//! that PMDG ships under one shared SDK header:
//!
//! * 777-200LR (`pmdg-aircraft-77l`) — AircraftModel `4`
//! * 777-300ER (`pmdg-aircraft-77er`) — AircraftModel `6`
//! * 777F freighter (`pmdg-aircraft-77f`) — AircraftModel `5`
//! * 777W variant (`pmdg-aircraft-77w`) — typically -300ER, AircraftModel `6`
//!
//! Plus the SDK's `AircraftModel` field also enumerates `-200`,
//! `-200ER`, `-300` (codes 1/2/3) which PMDG hasn't shipped for
//! MSFS yet but the SDK accommodates.
//!
//! # Differences vs. NG3
//!
//! * Three CDU channels (Capt / F/O / AUX) vs. NG3's two.
//! * MCP layout is push-button-engagement style with a separate
//!   FPA (Flight Path Angle) value — no "CMD A/B" classic split.
//! * Includes ECL_ChecklistComplete[10] for the electronic
//!   checklist completion state.
//! * No `IRS_*` overhead struct (777 ADIRU is simpler).
//! * Bigger `EFIS_*` block (independent left/right minimums sets).

#![allow(non_snake_case)]
#![allow(dead_code)]

// ---------------------------------------------------------------
// SimConnect ClientData identifiers — must match the header verbatim.
// ---------------------------------------------------------------

pub const PMDG_777X_DATA_NAME: &str = "PMDG_777X_Data";
pub const PMDG_777X_DATA_ID: u32 = 0x504D_4447;
pub const PMDG_777X_DATA_DEFINITION: u32 = 0x504D_4448;
pub const PMDG_777X_CONTROL_NAME: &str = "PMDG_777X_Control";
pub const PMDG_777X_CONTROL_ID: u32 = 0x504D_4449;
pub const PMDG_777X_CONTROL_DEFINITION: u32 = 0x504D_444A;
pub const PMDG_777X_CDU_0_NAME: &str = "PMDG_777X_CDU_0";
pub const PMDG_777X_CDU_1_NAME: &str = "PMDG_777X_CDU_1";
pub const PMDG_777X_CDU_2_NAME: &str = "PMDG_777X_CDU_2";

// ---------------------------------------------------------------
// Memory-layout-exact replica of `struct PMDG_777X_Data`.
// ---------------------------------------------------------------

/// Field-by-field 1:1 replica of `PMDG_777X_Data` from the SDK
/// header. **Do not reorder fields** — SimConnect's
/// `AddToClientDataDefinition` uses `sizeof(struct)` so a field
/// reorder would silently mis-parse every field after the move.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Pmdg777XRawData {
    // ============================================================
    // Overhead Maintenance Panel
    // ============================================================
    pub ICE_WindowHeatBackUp_Sw_OFF: [u8; 2],
    pub ELEC_StandbyPowerSw: u8,
    pub FCTL_WingHydValve_Sw_SHUT_OFF: [u8; 3],
    pub FCTL_TailHydValve_Sw_SHUT_OFF: [u8; 3],
    pub FCTL_annunTailHydVALVE_CLOSED: [u8; 3],
    pub FCTL_annunWingHydVALVE_CLOSED: [u8; 3],
    pub FCTL_PrimFltComputersSw_AUTO: u8,
    pub FCTL_annunPrimFltComputersDISC: u8,
    pub APU_Power_Sw_TEST: u8,
    pub ENG_EECPower_Sw_TEST: [u8; 2],
    pub ELEC_TowingPower_Sw_BATT: u8,
    pub ELEC_annunTowingPowerON_BATT: u8,
    pub AIR_CargoTemp_Selector: [u8; 2],
    pub AIR_CargoTemp_MainDeckFwd_Sel: u8,
    pub AIR_CargoTemp_MainDeckAft_Sel: u8,
    pub AIR_CargoTemp_LowerFwd_Sel: u8,
    pub AIR_CargoTemp_LowerAft_Sel: u8,

    // ============================================================
    // Overhead Panel
    // ============================================================
    pub ADIRU_Sw_On: u8,
    pub ADIRU_annunOFF: u8,
    pub ADIRU_annunON_BAT: u8,
    pub FCTL_ThrustAsymComp_Sw_AUTO: u8,
    pub FCTL_annunThrustAsymCompOFF: u8,
    pub ELEC_CabUtilSw: u8,
    pub ELEC_annunCabUtilOFF: u8,
    pub ELEC_IFEPassSeatsSw: u8,
    pub ELEC_annunIFEPassSeatsOFF: u8,
    pub ELEC_Battery_Sw_ON: u8,
    pub ELEC_annunBattery_OFF: u8,
    pub ELEC_annunAPU_GEN_OFF: u8,
    pub ELEC_APUGen_Sw_ON: u8,
    pub ELEC_APU_Selector: u8, // 0=OFF 1=ON 2=START
    pub ELEC_annunAPU_FAULT: u8,
    pub ELEC_BusTie_Sw_AUTO: [u8; 2],
    pub ELEC_annunBusTieISLN: [u8; 2],
    pub ELEC_ExtPwrSw: [u8; 2],
    pub ELEC_annunExtPowr_ON: [u8; 2],
    pub ELEC_annunExtPowr_AVAIL: [u8; 2],
    pub ELEC_Gen_Sw_ON: [u8; 2],
    pub ELEC_annunGenOFF: [u8; 2],
    pub ELEC_BackupGen_Sw_ON: [u8; 2],
    pub ELEC_annunBackupGenOFF: [u8; 2],
    pub ELEC_IDGDiscSw: [u8; 2],
    pub ELEC_annunIDGDiscDRIVE: [u8; 2],
    pub WIPERS_Selector: [u8; 2],
    pub LTS_EmerLightsSelector: u8,
    pub COMM_ServiceInterphoneSw: u8,
    pub OXY_PassOxygen_Sw_On: u8,
    pub OXY_annunPassOxygenON: u8,
    pub ICE_WindowHeat_Sw_ON: [u8; 4],
    pub ICE_annunWindowHeatINOP: [u8; 4],
    pub HYD_RamAirTurbineSw: u8,
    pub HYD_annunRamAirTurbinePRESS: u8,
    pub HYD_annunRamAirTurbineUNLKD: u8,
    pub HYD_PrimaryEngPump_Sw_ON: [u8; 2],
    pub HYD_PrimaryElecPump_Sw_ON: [u8; 2],
    pub HYD_DemandElecPump_Selector: [u8; 2],
    pub HYD_DemandAirPump_Selector: [u8; 2],
    pub HYD_annunPrimaryEngPumpFAULT: [u8; 2],
    pub HYD_annunPrimaryElecPumpFAULT: [u8; 2],
    pub HYD_annunDemandElecPumpFAULT: [u8; 2],
    pub HYD_annunDemandAirPumpFAULT: [u8; 2],
    pub SIGNS_NoSmokingSelector: u8,
    pub SIGNS_SeatBeltsSelector: u8,
    pub LTS_DomeLightKnob: u8,
    pub LTS_CircuitBreakerKnob: u8,
    pub LTS_OvereadPanelKnob: u8,
    pub LTS_GlareshieldPNLlKnob: u8,
    pub LTS_GlareshieldFLOODKnob: u8,
    pub LTS_Storm_Sw_ON: u8,
    pub LTS_MasterBright_Sw_ON: u8,
    pub LTS_MasterBrigntKnob: u8,
    pub LTS_IndLightsTestSw: u8,
    pub LTS_LandingLights_Sw_ON: [u8; 3],
    pub LTS_Beacon_Sw_ON: u8,
    pub LTS_NAV_Sw_ON: u8,
    pub LTS_Logo_Sw_ON: u8,
    pub LTS_Wing_Sw_ON: u8,
    pub LTS_RunwayTurnoff_Sw_ON: [u8; 2],
    pub LTS_Taxi_Sw_ON: u8,
    pub LTS_Strobe_Sw_ON: u8,
    pub FIRE_CargoFire_Sw_Arm: [u8; 2],
    pub FIRE_annunCargoFire: [u8; 2],
    pub FIRE_CargoFireDisch_Sw: u8,
    pub FIRE_annunCargoDISCH: u8,
    pub FIRE_FireOvhtTest_Sw: u8,
    pub FIRE_APUHandle: u8,
    pub FIRE_APUHandleUnlock_Sw: u8,
    pub FIRE_annunAPU_BTL_DISCH: u8,
    pub FIRE_EngineHandleIlluminated: [u8; 2],
    pub FIRE_APUHandleIlluminated: u8,
    pub FIRE_EngineHandleIsUnlocked: [u8; 2],
    pub FIRE_APUHandleIsUnlocked: u8,
    pub FIRE_annunMainDeckCargoFire: u8,
    pub FIRE_annunCargoDEPR: u8,
    pub ENG_EECMode_Sw_NORM: [u8; 2],
    pub ENG_Start_Selector: [u8; 2],
    pub ENG_Autostart_Sw_ON: u8,
    pub ENG_annunALTN: [u8; 2],
    pub ENG_annunAutostartOFF: u8,
    pub FUEL_CrossFeedFwd_Sw: u8,
    pub FUEL_CrossFeedAft_Sw: u8,
    pub FUEL_PumpFwd_Sw: [u8; 2],
    pub FUEL_PumpAft_Sw: [u8; 2],
    pub FUEL_PumpCtr_Sw: [u8; 2],
    pub FUEL_JettisonNozle_Sw: [u8; 2],
    pub FUEL_JettisonArm_Sw: u8,
    pub FUEL_FuelToRemain_Sw_Pulled: u8,
    pub FUEL_FuelToRemain_Selector: u8,
    pub FUEL_annunFwdXFEED_VALVE: u8,
    pub FUEL_annunAftXFEED_VALVE: u8,
    pub FUEL_annunLOWPRESS_Fwd: [u8; 2],
    pub FUEL_annunLOWPRESS_Aft: [u8; 2],
    pub FUEL_annunLOWPRESS_Ctr: [u8; 2],
    pub FUEL_annunJettisonNozleVALVE: [u8; 2],
    pub FUEL_annunArmFAULT: u8,
    pub ICE_WingAntiIceSw: u8,
    pub ICE_EngAntiIceSw: [u8; 2],
    pub AIR_Pack_Sw_AUTO: [u8; 2],
    pub AIR_TrimAir_Sw_On: [u8; 2],
    pub AIR_RecircFan_Sw_On: [u8; 2],
    pub AIR_TempSelector: [u8; 2],
    pub AIR_AirCondReset_Sw_Pushed: u8,
    pub AIR_EquipCooling_Sw_AUTO: u8,
    pub AIR_Gasper_Sw_On: u8,
    pub AIR_annunPackOFF: [u8; 2],
    pub AIR_annunTrimAirFAULT: [u8; 2],
    pub AIR_annunEquipCoolingOVRD: u8,
    pub AIR_AltnVentSw_ON: u8,
    pub AIR_annunAltnVentFAULT: u8,
    pub AIR_MainDeckFlowSw_NORM: u8,
    pub AIR_EngBleedAir_Sw_AUTO: [u8; 2],
    pub AIR_APUBleedAir_Sw_AUTO: u8,
    pub AIR_IsolationValve_Sw: [u8; 2],
    pub AIR_CtrIsolationValve_Sw: u8,
    pub AIR_annunEngBleedAirOFF: [u8; 2],
    pub AIR_annunAPUBleedAirOFF: u8,
    pub AIR_annunIsolationValveCLOSED: [u8; 2],
    pub AIR_annunCtrIsolationValveCLOSED: u8,
    pub AIR_OutflowValve_Sw_AUTO: [u8; 2],
    pub AIR_OutflowValveManual_Selector: [u8; 2],
    pub AIR_LdgAlt_Sw_Pulled: u8,
    pub AIR_LdgAlt_Selector: u8,
    pub AIR_annunOutflowValve_MAN: [u8; 2],

    // ============================================================
    // Forward panel — Center
    // ============================================================
    pub GEAR_Lever: u8,
    pub GEAR_LockOvrd_Sw: u8,
    pub GEAR_AltnGear_Sw_DOWN: u8,
    pub GPWS_FlapInhibitSw_OVRD: u8,
    pub GPWS_GearInhibitSw_OVRD: u8,
    pub GPWS_TerrInhibitSw_OVRD: u8,
    pub GPWS_RunwayOvrdSw_OVRD: u8,
    pub GPWS_GSInhibit_Sw: u8,
    pub GPWS_annunGND_PROX_top: u8,
    pub GPWS_annunGND_PROX_bottom: u8,
    pub BRAKES_AutobrakeSelector: u8, // 0=RTO 1=OFF 2=DISARM 3..5=AUTO

    // Standby - ISFD
    pub ISFD_Baro_Sw_Pushed: u8,
    pub ISFD_RST_Sw_Pushed: u8,
    pub ISFD_Minus_Sw_Pushed: u8,
    pub ISFD_Plus_Sw_Pushed: u8,
    pub ISFD_APP_Sw_Pushed: u8,
    pub ISFD_HP_IN_Sw_Pushed: u8,

    // Left
    pub ISP_Nav_L_Sw_CDU: u8,
    pub ISP_DsplCtrl_L_Sw_Altn: u8,
    pub ISP_AirDataAtt_L_Sw_Altn: u8,
    pub DSP_InbdDspl_L_Selector: u8,
    pub EFIS_HdgRef_Sw_Norm: u8,
    pub EFIS_annunHdgRefTRUE: u8,
    pub BRAKES_BrakePressNeedle: i32, // C++ `int`, 0..100 = 0..4000 PSI
    pub BRAKES_annunBRAKE_SOURCE: u8,

    // Right
    pub ISP_Nav_R_Sw_CDU: u8,
    pub ISP_DsplCtrl_R_Sw_Altn: u8,
    pub ISP_AirDataAtt_R_Sw_Altn: u8,
    pub ISP_FMC_Selector: u8,
    pub DSP_InbdDspl_R_Selector: u8,

    // Sidewalls
    pub AIR_ShoulderHeaterKnob: [u8; 2],
    pub AIR_FootHeaterSelector: [u8; 2],
    pub LTS_LeftFwdPanelPNLKnob: u8,
    pub LTS_LeftFwdPanelFLOODKnob: u8,
    pub LTS_LeftOutbdDsplBRIGHTNESSKnob: u8,
    pub LTS_LeftInbdDsplBRIGHTNESSKnob: u8,
    pub LTS_RightFwdPanelPNLKnob: u8,
    pub LTS_RightFwdPanelFLOODKnob: u8,
    pub LTS_RightInbdDsplBRIGHTNESSKnob: u8,
    pub LTS_RightOutbdDsplBRIGHTNESSKnob: u8,

    // Chronometers
    pub CHR_Chr_Sw_Pushed: [u8; 2],
    pub CHR_TimeDate_Sw_Pushed: [u8; 2],
    pub CHR_TimeDate_Selector: [u8; 2],
    pub CHR_Set_Selector: [u8; 2],
    pub CHR_ET_Selector: [u8; 2],

    // ============================================================
    // Glareshield — EFIS + MCP
    // ============================================================
    pub EFIS_MinsSelBARO: [u8; 2],
    pub EFIS_BaroSelHPA: [u8; 2],
    pub EFIS_VORADFSel1: [u8; 2],
    pub EFIS_VORADFSel2: [u8; 2],
    pub EFIS_ModeSel: [u8; 2],
    pub EFIS_RangeSel: [u8; 2],
    pub EFIS_MinsKnob: [u8; 2],
    pub EFIS_BaroKnob: [u8; 2],

    pub EFIS_MinsRST_Sw_Pushed: [u8; 2],
    pub EFIS_BaroSTD_Sw_Pushed: [u8; 2],
    pub EFIS_ModeCTR_Sw_Pushed: [u8; 2],
    pub EFIS_RangeTFC_Sw_Pushed: [u8; 2],
    pub EFIS_WXR_Sw_Pushed: [u8; 2],
    pub EFIS_STA_Sw_Pushed: [u8; 2],
    pub EFIS_WPT_Sw_Pushed: [u8; 2],
    pub EFIS_ARPT_Sw_Pushed: [u8; 2],
    pub EFIS_DATA_Sw_Pushed: [u8; 2],
    pub EFIS_POS_Sw_Pushed: [u8; 2],
    pub EFIS_TERR_Sw_Pushed: [u8; 2],

    // ---- MCP ----
    pub MCP_IASMach: f32,         // Mach if < 10.0
    pub MCP_IASBlank: u8,
    pub MCP_Heading: u16,
    pub MCP_Altitude: u16,
    pub MCP_VertSpeed: i16,
    pub MCP_FPA: f32,             // Flight Path Angle (777-specific)
    pub MCP_VertSpeedBlank: u8,

    pub MCP_FD_Sw_On: [u8; 2],
    pub MCP_ATArm_Sw_On: [u8; 2],
    pub MCP_BankLimitSel: u8,     // 0=AUTO 1=5 .. 5=25
    pub MCP_AltIncrSel: u8,
    pub MCP_DisengageBar: u8,
    pub MCP_Speed_Dial: u8,       // 0..99
    pub MCP_Heading_Dial: u8,
    pub MCP_Altitude_Dial: u8,
    pub MCP_VS_Wheel: u8,

    pub MCP_HDGDial_Mode: u8,     // 0=HDG 1=TRK
    pub MCP_VSDial_Mode: u8,      // 0=VS 1=FPA

    // MCP momentary push-button switches
    pub MCP_AP_Sw_Pushed: [u8; 2],
    pub MCP_CLB_CON_Sw_Pushed: u8,
    pub MCP_AT_Sw_Pushed: u8,
    pub MCP_LNAV_Sw_Pushed: u8,
    pub MCP_VNAV_Sw_Pushed: u8,
    pub MCP_FLCH_Sw_Pushed: u8,
    pub MCP_HDG_HOLD_Sw_Pushed: u8,
    pub MCP_VS_FPA_Sw_Pushed: u8,
    pub MCP_ALT_HOLD_Sw_Pushed: u8,
    pub MCP_LOC_Sw_Pushed: u8,
    pub MCP_APP_Sw_Pushed: u8,
    pub MCP_Speeed_Sw_Pushed: u8,
    pub MCP_Heading_Sw_Pushed: u8,
    pub MCP_Altitude_Sw_Pushed: u8,
    pub MCP_IAS_MACH_Toggle_Sw_Pushed: u8,
    pub MCP_HDG_TRK_Toggle_Sw_Pushed: u8,
    pub MCP_VS_FPA_Toggle_Sw_Pushed: u8,

    // MCP annunciator lights
    pub MCP_annunAP: [u8; 2],
    pub MCP_annunAT: u8,
    pub MCP_annunLNAV: u8,
    pub MCP_annunVNAV: u8,
    pub MCP_annunFLCH: u8,
    pub MCP_annunHDG_HOLD: u8,
    pub MCP_annunVS_FPA: u8,
    pub MCP_annunALT_HOLD: u8,
    pub MCP_annunLOC: u8,
    pub MCP_annunAPP: u8,

    // Display Select Panel
    pub DSP_L_INBD_Sw: u8,
    pub DSP_R_INBD_Sw: u8,
    pub DSP_LWR_CTR_Sw: u8,
    pub DSP_ENG_Sw: u8,
    pub DSP_STAT_Sw: u8,
    pub DSP_ELEC_Sw: u8,
    pub DSP_HYD_Sw: u8,
    pub DSP_FUEL_Sw: u8,
    pub DSP_AIR_Sw: u8,
    pub DSP_DOOR_Sw: u8,
    pub DSP_GEAR_Sw: u8,
    pub DSP_FCTL_Sw: u8,
    pub DSP_CAM_Sw: u8,
    pub DSP_CHKL_Sw: u8,
    pub DSP_COMM_Sw: u8,
    pub DSP_NAV_Sw: u8,
    pub DSP_CANC_RCL_Sw: u8,
    pub DSP_annunL_INBD: u8,
    pub DSP_annunR_INBD: u8,
    pub DSP_annunLWR_CTR: u8,

    // Master Warning/Caution
    pub WARN_Reset_Sw_Pushed: [u8; 2],
    pub WARN_annunMASTER_WARNING: [u8; 2],
    pub WARN_annunMASTER_CAUTION: [u8; 2],

    // ============================================================
    // Forward Aisle Stand Panel
    // ============================================================
    pub ISP_DsplCtrl_C_Sw_Altn: u8,
    pub LTS_UpperDsplBRIGHTNESSKnob: u8,
    pub LTS_LowerDsplBRIGHTNESSKnob: u8,
    pub EICAS_EventRcd_Sw_Pushed: u8,

    // Three CDUs (vs. NG3's two)
    pub CDU_annunEXEC: [u8; 3],
    pub CDU_annunDSPY: [u8; 3],
    pub CDU_annunFAIL: [u8; 3],
    pub CDU_annunMSG: [u8; 3],
    pub CDU_annunOFST: [u8; 3],
    pub CDU_BrtKnob: [u8; 3],

    // ============================================================
    // Control Stand
    // ============================================================
    pub FCTL_AltnFlaps_Sw_ARM: u8,
    pub FCTL_AltnFlaps_Control_Sw: u8,
    pub FCTL_StabCutOutSw_C_NORMAL: u8,
    pub FCTL_StabCutOutSw_R_NORMAL: u8,
    pub FCTL_AltnPitch_Lever: u8,
    pub FCTL_Speedbrake_Lever: u8, // 0..100
    /// 777 flaps: 0=UP 1=1 2=5 3=15 4=20 5=25 6=30
    pub FCTL_Flaps_Lever: u8,
    pub ENG_FuelControl_Sw_RUN: [u8; 2],
    pub BRAKES_ParkingBrakeLeverOn: u8,

    // ============================================================
    // Aft Aisle Stand Panel
    // ============================================================
    pub COMM_SelectedMic: [u8; 3],
    pub COMM_ReceiverSwitches: [u16; 3],
    pub COMM_OBSAudio_Selector: u8,

    pub COMM_SelectedRadio: [u8; 3],
    pub COMM_RadioTransfer_Sw_Pushed: [u8; 3],
    pub COMM_RadioPanelOff: [u8; 3],
    pub COMM_annunAM: [u8; 3],

    // TCAS Panel
    pub XPDR_XpndrSelector_R: u8,
    pub XPDR_AltSourceSel_ALTN: u8,
    pub XPDR_ModeSel: u8,         // 0=STBY .. 4=TA/RA
    pub XPDR_Ident_Sw_Pushed: u8,

    // Engine Fire
    pub FIRE_EngineHandle: [u8; 2],
    pub FIRE_EngineHandleUnlock_Sw: [u8; 2],
    pub FIRE_annunENG_BTL_DISCH: [u8; 2],

    // Aileron & Rudder Trim
    pub FCTL_AileronTrim_Switches: u8,
    pub FCTL_RudderTrim_Knob: u8,
    pub FCTL_RudderTrimCancel_Sw_Pushed: u8,

    // Evacuation Panel
    pub EVAC_Command_Sw_ON: u8,
    pub EVAC_PressToTest_Sw_Pressed: u8,
    pub EVAC_HornSutOff_Sw_Pulled: u8,
    pub EVAC_LightIlluminated: u8,

    pub LTS_AisleStandPNLKnob: u8,
    pub LTS_AisleStandFLOODKnob: u8,
    pub LTS_FloorLightsSw: u8,

    /// Door state — 16 doors. Values: 0=open, 1=closed,
    /// 2=closed and armed, 3=closing, 4=opening.
    pub DOOR_state: [u8; 16],
    pub DOOR_CockpitDoorOpen: u8,

    // ============================================================
    // Additional variables
    // ============================================================
    pub ENG_StartValve: [u8; 2],
    pub AIR_DuctPress: [f32; 2],   // PSI
    pub FUEL_QtyCenter: f32,       // LBS
    pub FUEL_QtyLeft: f32,
    pub FUEL_QtyRight: f32,
    pub FUEL_QtyAux: f32,          // 777-specific (no equivalent in NG3)
    pub IRS_aligned: u8,

    pub EFIS_BaroMinimumsSet: [u8; 2],
    pub EFIS_BaroMinimums: [i32; 2],   // C++ `int`
    pub EFIS_RadioMinimumsSet: [u8; 2],
    pub EFIS_RadioMinimums: [i32; 2],

    /// Display formats — 6 display units, values 0..16 per the
    /// `EFIS_Display` table in the SDK comment.
    pub EFIS_Display: [u8; 6],

    /// Aircraft model:
    /// 1=-200 2=-200ER 3=-300 4=-200LR 5=777F 6=-300ER
    pub AircraftModel: u8,
    pub WeightInKg: u8,
    pub GPWS_V1CallEnabled: u8,
    pub GroundConnAvailable: u8,

    // FMC — pilot's plan
    pub FMC_TakeoffFlaps: u8,
    pub FMC_V1: u8,
    pub FMC_VR: u8,
    pub FMC_V2: u8,
    /// Thrust reduction altitude. Values 1 or 5 = "FLAPS 1/5",
    /// otherwise feet.
    pub FMC_ThrustRedAlt: u16,
    pub FMC_AccelerationAlt: u16,
    pub FMC_EOAccelerationAlt: u16,
    pub FMC_LandingFlaps: u8,
    pub FMC_LandingVREF: u8,
    pub FMC_CruiseAlt: u16,
    pub FMC_LandingAltitude: i16,
    pub FMC_TransitionAlt: u16,
    pub FMC_TransitionLevel: u16,
    pub FMC_PerfInputComplete: u8,
    pub FMC_DistanceToTOD: f32,
    pub FMC_DistanceToDest: f32,
    pub FMC_flightNumber: [u8; 9],
    pub WheelChocksSet: u8,
    pub APURunning: u8,

    /// Thrust limit mode 0..16 per SDK comment table.
    pub FMC_ThrustLimitMode: u8,

    /// Electronic checklist completion — 10 phases.
    pub ECL_ChecklistComplete: [u8; 10],

    pub reserved: [u8; 84],
}

// ---------------------------------------------------------------
// Compile-time size guard. Pinned at 684 bytes — the size we
// measured by compiling our 1:1 mirror of `PMDG_777X_SDK.h`
// shipped with the user's pmdg-aircraft-77er v3.x installation
// (header dated 2024).
//
// **If this assertion fires after a PMDG update** → diff our mirror
// against the new header in `Documentation/SDK/PMDG_777X_SDK.h`,
// add/reorder fields to match, update this constant.
// SimConnect's `AddToClientDataDefinition` uses `sizeof(struct)`
// so a mismatch silently mis-parses every field after the drift.
// ---------------------------------------------------------------
const _: () = {
    assert!(std::mem::size_of::<Pmdg777XRawData>() == 684);
};

// ---------------------------------------------------------------
// Aircraft-variant decoding
// ---------------------------------------------------------------

/// Aircraft variant from the `AircraftModel` field. Path-only
/// detection (used during AircraftLoaded resolution before any
/// raw data has arrived) is on the `Pmdg777XPathVariant` enum
/// below — once we have raw data, prefer this enum derived from
/// the actual SDK field.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pmdg777XAircraftModel {
    B777_200,
    B777_200ER,
    B777_300,
    B777_200LR,
    B777F,
    B777_300ER,
    Unknown(u8),
}

impl Pmdg777XAircraftModel {
    pub fn from_byte(v: u8) -> Self {
        match v {
            1 => Self::B777_200,
            2 => Self::B777_200ER,
            3 => Self::B777_300,
            4 => Self::B777_200LR,
            5 => Self::B777F,
            6 => Self::B777_300ER,
            other => Self::Unknown(other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::B777_200 => "777-200",
            Self::B777_200ER => "777-200ER",
            Self::B777_300 => "777-300",
            Self::B777_200LR => "777-200LR",
            Self::B777F => "777F",
            Self::B777_300ER => "777-300ER",
            Self::Unknown(_) => "777 (unknown)",
        }
    }
}

/// Path-derived 777 variant (used pre-data-arrival).
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pmdg777XPathVariant {
    B777_200LR,
    B777_300ER,
    B777_200F,
    B777_W,
    Unknown,
}

impl Pmdg777XPathVariant {
    pub fn from_air_path(p: &str) -> Self {
        let lp = p.to_lowercase();
        if lp.contains("pmdg-aircraft-77l") || lp.contains("777-200lr") {
            Self::B777_200LR
        } else if lp.contains("pmdg-aircraft-77er") || lp.contains("777-300er") {
            Self::B777_300ER
        } else if lp.contains("pmdg-aircraft-77f") || lp.contains("777f") {
            Self::B777_200F
        } else if lp.contains("pmdg-aircraft-77w") {
            Self::B777_W
        } else {
            Self::Unknown
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::B777_200LR => "777-200LR",
            Self::B777_300ER => "777-300ER",
            Self::B777_200F => "777-200F",
            Self::B777_W => "777-W",
            Self::Unknown => "777 (unknown)",
        }
    }
}

// ---------------------------------------------------------------
// Higher-level "useful subset" — what AeroACARS actually consumes.
// ---------------------------------------------------------------

/// 777 autobrake: same enum-style as NG3 but the values differ
/// (777 has a "DISARM" position + numbered 1..MAX-AUTO).
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pmdg777XAutobrake {
    Rto, Off, Disarm, One, Two, Three, FourMaxAuto,
    Unknown(u8),
}

impl Pmdg777XAutobrake {
    pub fn from_byte(v: u8) -> Self {
        match v {
            0 => Self::Rto,
            1 => Self::Off,
            2 => Self::Disarm,
            3 => Self::One,
            4 => Self::Two,
            5 => Self::FourMaxAuto, // SDK: 5=MAX AUTO; "Three" not in real layout
            other => Self::Unknown(other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rto => "RTO",
            Self::Off => "OFF",
            Self::Disarm => "DISARM",
            Self::One => "1",
            Self::Two => "2",
            Self::Three => "3",
            Self::FourMaxAuto => "MAX AUTO",
            Self::Unknown(_) => "?",
        }
    }
}

/// 777 flap handle position decoded.
/// SDK: `FCTL_Flaps_Lever` 0=UP 1=1 2=5 3=15 4=20 5=25 6=30
pub fn x777_flap_label(handle_pos: u8) -> &'static str {
    match handle_pos {
        0 => "UP",
        1 => "1",
        2 => "5",
        3 => "15",
        4 => "20",
        5 => "25",
        6 => "30",
        _ => "?",
    }
}

/// MCP autoflight modes for the 777. Different from NG3:
/// no CMD A/B (push-button engagement), has FLCH instead of LVL CHG,
/// HDG HOLD instead of HDG SEL, VS_FPA combined annunciator.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pmdg777XFmaState {
    pub at: bool,
    pub lnav: bool,
    pub vnav: bool,
    pub flch: bool,
    pub hdg_hold: bool,
    pub vs_fpa: bool,
    pub alt_hold: bool,
    pub loc: bool,
    pub app: bool,
    pub ap_capt: bool,
    pub ap_fo: bool,
    pub fd_capt: bool,
    pub fd_fo: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Pmdg777XVSpeeds {
    pub v1_kt: Option<u8>,
    pub vr_kt: Option<u8>,
    pub v2_kt: Option<u8>,
    pub vref_kt: Option<u8>,
}

/// "Useful subset" view of `Pmdg777XRawData`.
#[derive(Debug, Clone)]
pub struct Pmdg777XSnapshot {
    /// Aircraft model from the SDK field (more authoritative than
    /// path detection once data is flowing).
    pub model: Pmdg777XAircraftModel,
    pub weight_in_kg: bool,
    pub apu_running: bool,

    // MCP — selected autopilot targets
    pub mcp_speed_raw: f32,
    pub mcp_speed_blanked: bool,
    pub mcp_heading_deg: u16,
    pub mcp_altitude_ft: u16,
    pub mcp_vs_fpm: i16,
    /// 777-specific: Flight Path Angle when VSDial_Mode==1.
    pub mcp_fpa: f32,
    pub mcp_vs_blanked: bool,
    /// True when the dial shows TRK rather than HDG.
    pub mcp_dial_in_trk_mode: bool,
    /// True when the V/S wheel shows FPA rather than V/S.
    pub mcp_dial_in_fpa_mode: bool,

    // FMA
    pub fma: Pmdg777XFmaState,

    // Controls
    pub flap_handle_label: &'static str, // "UP" / "1" / "5" / etc.
    pub flap_handle_pos: u8,
    /// Speedbrake lever 0..100. 25=ARMED, 26..100=DEPLOYED.
    pub speedbrake_lever_pos: u8,
    pub speedbrake_armed: bool,
    pub speedbrake_extended: bool,
    pub gear_lever_down: bool,
    pub autobrake: Pmdg777XAutobrake,
    pub parking_brake_set: bool,

    // FMC plan
    pub fmc_takeoff_flaps_deg: u8,
    pub fmc_landing_flaps_deg: u8,
    pub fmc_v_speeds: Pmdg777XVSpeeds,
    /// Thrust reduction: special values 1 or 5 mean "FLAPS 1/5",
    /// any other value is feet.
    pub fmc_thrust_red_alt_ft: u16,
    pub fmc_acceleration_alt_ft: u16,
    pub fmc_cruise_alt_ft: u16,
    pub fmc_landing_altitude_ft: i16,
    pub fmc_perf_input_complete: bool,
    pub fmc_distance_to_tod_nm: f32,
    pub fmc_distance_to_dest_nm: f32,
    pub fmc_flight_number: String,
    /// 777-specific: thrust limit mode (TO, TO 1, CLB, CRZ, ...).
    pub fmc_thrust_limit_mode: u8,

    // Misc
    pub xpdr_mode: u8,
    pub gpws_top_warn: bool,
    pub gpws_bottom_warn: bool,

    // ---- v0.16.10 (#Premium): deep-data annunciators + fuel + minimums ----
    /// MASTER CAUTION lit on either side (`WARN_annunMASTER_CAUTION[0|1]`).
    pub master_caution: bool,
    /// MASTER WARNING lit on either side (`WARN_annunMASTER_WARNING[0|1]`).
    /// Unlike the 737, the 777 has a real red MASTER WARNING light.
    pub master_warning: bool,
    /// Per-tank fuel quantity in kg, order **[left, center, right, aux]**
    /// (FUEL_QtyLeft / FUEL_QtyCenter / FUEL_QtyRight / FUEL_QtyAux —
    /// the aux tank is 777-specific). Already unit-canonicalised via
    /// the raw struct's `WeightInKg` flag — see
    /// [`super::pmdg_fuel_qty_to_kg`].
    pub fuel_per_tank_kg: [f64; 4],
    /// Captain-side EFIS baro minimums (DA/MDA, ft):
    /// `EFIS_BaroMinimums[0]`, gated on `EFIS_BaroMinimumsSet[0]` —
    /// `None` until the pilot has dialed a baro minimums value. The
    /// SDK note "check EFIS_MinsSelBARO to determine if the active
    /// minimums is BARO or RADIO" is deliberately not applied: the
    /// dialed DA stays the pilot's baro minimums even while the
    /// selector shows RADIO.
    pub minimums_baro_ft: Option<i32>,

    // ---- Extras for v0.2.2 wider integration ----
    /// Wheel chocks set at the gate. Pre-flight ground state.
    pub wheel_chocks_set: bool,
    /// Electronic Checklist completion — 10 phases (see SDK
    /// header for index meanings).
    pub ecl_complete: [bool; 10],

    // ---- Cockpit lights + systems for Premium-First override (v0.2.3) ----
    /// Any of the 3 landing-light switches (L/R/Nose) on.
    pub light_landing: bool,
    pub light_beacon: bool,
    pub light_strobe: bool,
    pub light_taxi: bool,
    pub light_nav: bool,
    pub light_logo: bool,
    pub light_wing: bool,
    pub wing_anti_ice: bool,    // 0=OFF 1=AUTO 2=ON → not OFF = active
    pub engine_anti_ice: bool,  // either eng on
    pub battery_master: bool,
    /// 777 has window heat (incl. BackUp_Sw_OFF) but no separate
    /// pitot/probe heat switch in the SDK — pitot heat is part
    /// of the broader window heat system.
    pub pitot_heat: bool,
}

impl Pmdg777XSnapshot {
    pub fn from_raw(raw: &Pmdg777XRawData) -> Self {
        let v_speed = |raw: u8| if raw == 0 { None } else { Some(raw) };

        let flight_num = std::str::from_utf8(&raw.FMC_flightNumber)
            .unwrap_or("")
            .trim_end_matches('\0')
            .to_string();

        Self {
            model: Pmdg777XAircraftModel::from_byte(raw.AircraftModel),
            weight_in_kg: raw.WeightInKg != 0,
            apu_running: raw.APURunning != 0,

            mcp_speed_raw: raw.MCP_IASMach,
            mcp_speed_blanked: raw.MCP_IASBlank != 0,
            mcp_heading_deg: raw.MCP_Heading,
            mcp_altitude_ft: raw.MCP_Altitude,
            mcp_vs_fpm: raw.MCP_VertSpeed,
            mcp_fpa: raw.MCP_FPA,
            mcp_vs_blanked: raw.MCP_VertSpeedBlank != 0,
            mcp_dial_in_trk_mode: raw.MCP_HDGDial_Mode == 1,
            mcp_dial_in_fpa_mode: raw.MCP_VSDial_Mode == 1,

            fma: Pmdg777XFmaState {
                at: raw.MCP_annunAT != 0,
                lnav: raw.MCP_annunLNAV != 0,
                vnav: raw.MCP_annunVNAV != 0,
                flch: raw.MCP_annunFLCH != 0,
                hdg_hold: raw.MCP_annunHDG_HOLD != 0,
                vs_fpa: raw.MCP_annunVS_FPA != 0,
                alt_hold: raw.MCP_annunALT_HOLD != 0,
                loc: raw.MCP_annunLOC != 0,
                app: raw.MCP_annunAPP != 0,
                ap_capt: raw.MCP_annunAP[0] != 0,
                ap_fo: raw.MCP_annunAP[1] != 0,
                fd_capt: raw.MCP_FD_Sw_On[0] != 0,
                fd_fo: raw.MCP_FD_Sw_On[1] != 0,
            },

            flap_handle_label: x777_flap_label(raw.FCTL_Flaps_Lever),
            flap_handle_pos: raw.FCTL_Flaps_Lever,
            speedbrake_lever_pos: raw.FCTL_Speedbrake_Lever,
            // SDK: 25=ARMED, 26..100=DEPLOYED.
            speedbrake_armed: raw.FCTL_Speedbrake_Lever == 25,
            speedbrake_extended: raw.FCTL_Speedbrake_Lever > 25,
            gear_lever_down: raw.GEAR_Lever == 1,
            autobrake: Pmdg777XAutobrake::from_byte(raw.BRAKES_AutobrakeSelector),
            parking_brake_set: raw.BRAKES_ParkingBrakeLeverOn != 0,

            fmc_takeoff_flaps_deg: raw.FMC_TakeoffFlaps,
            fmc_landing_flaps_deg: raw.FMC_LandingFlaps,
            fmc_v_speeds: Pmdg777XVSpeeds {
                v1_kt: v_speed(raw.FMC_V1),
                vr_kt: v_speed(raw.FMC_VR),
                v2_kt: v_speed(raw.FMC_V2),
                vref_kt: v_speed(raw.FMC_LandingVREF),
            },
            fmc_thrust_red_alt_ft: raw.FMC_ThrustRedAlt,
            fmc_acceleration_alt_ft: raw.FMC_AccelerationAlt,
            fmc_cruise_alt_ft: raw.FMC_CruiseAlt,
            fmc_landing_altitude_ft: raw.FMC_LandingAltitude,
            fmc_perf_input_complete: raw.FMC_PerfInputComplete != 0,
            fmc_distance_to_tod_nm: raw.FMC_DistanceToTOD,
            fmc_distance_to_dest_nm: raw.FMC_DistanceToDest,
            fmc_flight_number: flight_num,
            fmc_thrust_limit_mode: raw.FMC_ThrustLimitMode,

            xpdr_mode: raw.XPDR_ModeSel,
            gpws_top_warn: raw.GPWS_annunGND_PROX_top != 0,
            gpws_bottom_warn: raw.GPWS_annunGND_PROX_bottom != 0,

            // ---- v0.16.10 (#Premium) deep-data fields ----
            // Side-paired annunciators (Capt/FO): OR — "the light
            // is on" is what the cockpit shows.
            master_caution: raw.WARN_annunMASTER_CAUTION[0] != 0
                || raw.WARN_annunMASTER_CAUTION[1] != 0,
            master_warning: raw.WARN_annunMASTER_WARNING[0] != 0
                || raw.WARN_annunMASTER_WARNING[1] != 0,
            // Order: [left, center, right, aux]. SDK floats follow
            // the pilot's weight-unit option (WeightInKg flag) —
            // convert to canonical kg here.
            fuel_per_tank_kg: [
                super::pmdg_fuel_qty_to_kg(raw.FUEL_QtyLeft, raw.WeightInKg != 0),
                super::pmdg_fuel_qty_to_kg(raw.FUEL_QtyCenter, raw.WeightInKg != 0),
                super::pmdg_fuel_qty_to_kg(raw.FUEL_QtyRight, raw.WeightInKg != 0),
                super::pmdg_fuel_qty_to_kg(raw.FUEL_QtyAux, raw.WeightInKg != 0),
            ],
            // Captain-side baro minimums, only when dialed/valid.
            minimums_baro_ft: (raw.EFIS_BaroMinimumsSet[0] != 0)
                .then_some(raw.EFIS_BaroMinimums[0]),

            wheel_chocks_set: raw.WheelChocksSet != 0,
            ecl_complete: [
                raw.ECL_ChecklistComplete[0] != 0,
                raw.ECL_ChecklistComplete[1] != 0,
                raw.ECL_ChecklistComplete[2] != 0,
                raw.ECL_ChecklistComplete[3] != 0,
                raw.ECL_ChecklistComplete[4] != 0,
                raw.ECL_ChecklistComplete[5] != 0,
                raw.ECL_ChecklistComplete[6] != 0,
                raw.ECL_ChecklistComplete[7] != 0,
                raw.ECL_ChecklistComplete[8] != 0,
                raw.ECL_ChecklistComplete[9] != 0,
            ],

            // ---- Lights + systems (v0.2.3 Premium-First) ----
            light_landing: raw.LTS_LandingLights_Sw_ON[0] != 0
                || raw.LTS_LandingLights_Sw_ON[1] != 0
                || raw.LTS_LandingLights_Sw_ON[2] != 0,
            light_beacon: raw.LTS_Beacon_Sw_ON != 0,
            light_strobe: raw.LTS_Strobe_Sw_ON != 0,
            light_taxi: raw.LTS_Taxi_Sw_ON != 0,
            light_nav: raw.LTS_NAV_Sw_ON != 0,
            light_logo: raw.LTS_Logo_Sw_ON != 0,
            light_wing: raw.LTS_Wing_Sw_ON != 0,
            // 777 anti-ice: 0=OFF 1=AUTO 2=ON. Active = not OFF.
            wing_anti_ice: raw.ICE_WingAntiIceSw != 0,
            engine_anti_ice: raw.ICE_EngAntiIceSw[0] != 0
                || raw.ICE_EngAntiIceSw[1] != 0,
            // Battery: ELEC_Battery_Sw_ON is direct boolean.
            battery_master: raw.ELEC_Battery_Sw_ON != 0,
            // 777 has 4 window-heat switches (L-Side/L-Fwd/R-Fwd/
            // R-Side); pitot heat is part of that system. ANY on
            // = pitot heat effectively on for our reporting.
            pitot_heat: raw.ICE_WindowHeat_Sw_ON[0] != 0
                || raw.ICE_WindowHeat_Sw_ON[1] != 0
                || raw.ICE_WindowHeat_Sw_ON[2] != 0
                || raw.ICE_WindowHeat_Sw_ON[3] != 0,
        }
    }
}

/// Map an FMC thrust-limit-mode byte to the EICAS label.
/// Per the SDK comment table:
/// 0=TO 1=TO 1 2=TO 2 3=TO B 4=CLB 5=CLB 1 6=CLB 2 7=CRZ 8=CON 9=G/A
/// 10..16=D-TO/A-TO variants.
pub fn x777_thrust_limit_label(mode: u8) -> &'static str {
    match mode {
        0 => "TO",
        1 => "TO 1",
        2 => "TO 2",
        3 => "TO B",
        4 => "CLB",
        5 => "CLB 1",
        6 => "CLB 2",
        7 => "CRZ",
        8 => "CON",
        9 => "G/A",
        10 => "D-TO",
        11 => "D-TO 1",
        12 => "D-TO 2",
        13 => "A-TO",
        14 => "A-TO 1",
        15 => "A-TO 2",
        16 => "A-TO B",
        _ => "?",
    }
}

/// Human label for an ECL phase index.
pub fn ecl_phase_label(idx: usize) -> &'static str {
    match idx {
        0 => "Preflight",
        1 => "Before Start",
        2 => "Before Taxi",
        3 => "Before Takeoff",
        4 => "After Takeoff",
        5 => "Descent",
        6 => "Approach",
        7 => "Landing",
        8 => "Shutdown",
        9 => "Secure",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact-byte-size check — pinned at 684 bytes per the 1:1
    /// mirror of PMDG_777X_SDK.h v3.x. If this fails after a PMDG
    /// update, see the const-assert in the module body for the
    /// drift recovery procedure.
    #[test]
    fn struct_size_matches_pmdg_layout() {
        const EXPECTED_BYTES: usize = 684;
        let n = std::mem::size_of::<Pmdg777XRawData>();
        assert_eq!(
            n, EXPECTED_BYTES,
            "Pmdg777XRawData layout drift! got {n}, expected {EXPECTED_BYTES}. \
             Diff against pmdg-aircraft-77er/Documentation/SDK/PMDG_777X_SDK.h."
        );
    }

    #[test]
    fn aircraft_model_decoding() {
        assert_eq!(Pmdg777XAircraftModel::from_byte(4), Pmdg777XAircraftModel::B777_200LR);
        assert_eq!(Pmdg777XAircraftModel::from_byte(5), Pmdg777XAircraftModel::B777F);
        assert_eq!(Pmdg777XAircraftModel::from_byte(6), Pmdg777XAircraftModel::B777_300ER);
        assert_eq!(Pmdg777XAircraftModel::from_byte(99), Pmdg777XAircraftModel::Unknown(99));
    }

    #[test]
    fn flap_label_decoding() {
        assert_eq!(x777_flap_label(0), "UP");
        assert_eq!(x777_flap_label(1), "1");
        assert_eq!(x777_flap_label(2), "5");
        assert_eq!(x777_flap_label(6), "30");
    }

    #[test]
    fn path_variant_detection() {
        assert_eq!(
            Pmdg777XPathVariant::from_air_path("E:\\MSFS24_Community\\Community\\pmdg-aircraft-77er\\..."),
            Pmdg777XPathVariant::B777_300ER
        );
        assert_eq!(
            Pmdg777XPathVariant::from_air_path("/path/pmdg-aircraft-77l/..."),
            Pmdg777XPathVariant::B777_200LR
        );
        assert_eq!(
            Pmdg777XPathVariant::from_air_path("/path/pmdg-aircraft-77f/..."),
            Pmdg777XPathVariant::B777_200F
        );
        assert_eq!(
            Pmdg777XPathVariant::from_air_path("/path/pmdg-aircraft-77w/..."),
            Pmdg777XPathVariant::B777_W
        );
        assert_eq!(
            Pmdg777XPathVariant::from_air_path("/random/path"),
            Pmdg777XPathVariant::Unknown
        );
    }

    #[test]
    fn snapshot_extracts_useful_subset_from_zeroed_raw() {
        let raw: Pmdg777XRawData = unsafe { std::mem::zeroed() };
        let s = Pmdg777XSnapshot::from_raw(&raw);
        assert_eq!(s.fmc_v_speeds.v1_kt, None);
        assert_eq!(s.flap_handle_label, "UP");
        assert!(!s.fma.vnav);
        assert!(!s.gear_lever_down); // 0 = UP
        // Autobrake: byte 0 = RTO (per SDK comment "0: RTO")
        assert_eq!(s.autobrake, Pmdg777XAutobrake::Rto);
        // v0.16.10: zeroed raw → premium defaults
        assert!(!s.master_caution);
        assert!(!s.master_warning);
        assert_eq!(s.minimums_baro_ft, None);
        assert_eq!(s.fuel_per_tank_kg, [0.0; 4]);
    }

    // ---- v0.16.10 (#Premium) deep-data field decoding ----

    fn zeroed_raw() -> Pmdg777XRawData {
        unsafe { std::mem::zeroed() }
    }

    /// MASTER CAUTION / MASTER WARNING are Capt/FO-sided — OR.
    #[test]
    fn premium_caution_and_warning_or_both_sides() {
        let mut raw = zeroed_raw();
        raw.WARN_annunMASTER_CAUTION = [1, 0];
        raw.WARN_annunMASTER_WARNING = [0, 1];
        let s = Pmdg777XSnapshot::from_raw(&raw);
        assert!(s.master_caution);
        assert!(s.master_warning);
    }

    /// GND PROX: top OR bottom annunciator counts (already-parsed
    /// fields, pinned here because the mapper now consumes them).
    #[test]
    fn premium_gnd_prox_top_or_bottom() {
        let mut raw = zeroed_raw();
        raw.GPWS_annunGND_PROX_top = 1;
        let s = Pmdg777XSnapshot::from_raw(&raw);
        assert!(s.gpws_top_warn && !s.gpws_bottom_warn);

        let mut raw = zeroed_raw();
        raw.GPWS_annunGND_PROX_bottom = 1;
        let s = Pmdg777XSnapshot::from_raw(&raw);
        assert!(!s.gpws_top_warn && s.gpws_bottom_warn);
    }

    /// Fuel: [left, center, right, aux] order + WeightInKg unit
    /// handling both ways.
    #[test]
    fn premium_fuel_per_tank_kg_units_and_order() {
        let mut raw = zeroed_raw();
        raw.FUEL_QtyLeft = 1000.0;
        raw.FUEL_QtyCenter = 2000.0;
        raw.FUEL_QtyRight = 3000.0;
        raw.FUEL_QtyAux = 500.0;

        // Pounds mode (WeightInKg = 0).
        raw.WeightInKg = 0;
        let s = Pmdg777XSnapshot::from_raw(&raw);
        assert!((s.fuel_per_tank_kg[0] - 453.59237).abs() < 1e-6);
        assert!((s.fuel_per_tank_kg[1] - 907.18474).abs() < 1e-6);
        assert!((s.fuel_per_tank_kg[2] - 1360.77711).abs() < 1e-6);
        assert!((s.fuel_per_tank_kg[3] - 226.796185).abs() < 1e-6);

        // Kg mode (WeightInKg = 1) — passthrough.
        raw.WeightInKg = 1;
        let s = Pmdg777XSnapshot::from_raw(&raw);
        assert_eq!(s.fuel_per_tank_kg, [1000.0, 2000.0, 3000.0, 500.0]);
    }

    /// Baro minimums: value only valid when the captain-side
    /// `EFIS_BaroMinimumsSet[0]` flag says it's been dialed.
    #[test]
    fn premium_baro_minimums_valid_and_invalid() {
        let mut raw = zeroed_raw();
        raw.EFIS_BaroMinimums = [740, 0];

        // Not set → None even though a stale value is present.
        raw.EFIS_BaroMinimumsSet = [0, 0];
        assert_eq!(Pmdg777XSnapshot::from_raw(&raw).minimums_baro_ft, None);

        // Set on the captain side → Some(value).
        raw.EFIS_BaroMinimumsSet = [1, 0];
        assert_eq!(Pmdg777XSnapshot::from_raw(&raw).minimums_baro_ft, Some(740));

        // FO-side set alone does NOT count (captain-side gate).
        raw.EFIS_BaroMinimumsSet = [0, 1];
        assert_eq!(Pmdg777XSnapshot::from_raw(&raw).minimums_baro_ft, None);
    }
}
