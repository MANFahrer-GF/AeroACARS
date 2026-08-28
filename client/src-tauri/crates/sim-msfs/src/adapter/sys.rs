//! Thin re-exports over the bindgen-generated SimConnect FFI.
//!
//! We bring in the auto-generated symbols once here and then expose
//! only what `adapter` uses, so the unsafe surface is tightly scoped.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]
#![allow(clippy::missing_safety_doc)]

include!(concat!(env!("OUT_DIR"), "/simconnect_bindings.rs"));

// HRESULT used by SimConnect_GetNextDispatch when the queue is empty.
pub const E_FAIL: i32 = 0x8000_4005u32 as i32;

// Bindgen turns the C++ scoped enum constants into i32; the
// `dwID` field on SIMCONNECT_RECV is a DWORD (u32) — cast at the
// re-export boundary so call sites can compare cleanly.
pub const SIMCONNECT_RECV_ID_OPEN: DWORD = SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_OPEN as DWORD;
pub const SIMCONNECT_RECV_ID_QUIT: DWORD = SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_QUIT as DWORD;
pub const SIMCONNECT_RECV_ID_EXCEPTION: DWORD =
    SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_EXCEPTION as DWORD;
pub const SIMCONNECT_RECV_ID_SIMOBJECT_DATA: DWORD =
    SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_SIMOBJECT_DATA as DWORD;

pub const SIMCONNECT_DATATYPE_FLOAT64: SIMCONNECT_DATATYPE =
    SIMCONNECT_DATATYPE_SIMCONNECT_DATATYPE_FLOAT64;
pub const SIMCONNECT_DATATYPE_INT32: SIMCONNECT_DATATYPE =
    SIMCONNECT_DATATYPE_SIMCONNECT_DATATYPE_INT32;
pub const SIMCONNECT_DATATYPE_STRING256: SIMCONNECT_DATATYPE =
    SIMCONNECT_DATATYPE_SIMCONNECT_DATATYPE_STRING256;

pub const SIMCONNECT_PERIOD_SECOND: SIMCONNECT_PERIOD = SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_SECOND;
/// One callback per rendered video frame — typically ~30 Hz on
/// MSFS 2024. Required for touchdown V/S / G capture so the ring
/// buffer has 30× more samples in its 5-second window and the
/// actual touchdown subframe gets recorded.
pub const SIMCONNECT_PERIOD_VISUAL_FRAME: SIMCONNECT_PERIOD =
    SIMCONNECT_PERIOD_SIMCONNECT_PERIOD_VISUAL_FRAME;
