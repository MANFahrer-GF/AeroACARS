//! Runway database + great-circle math + runway-centerline geometry.
//!
//! Used by:
//!   * Departure runway detection (spec §14)
//!   * Arrival runway detection (spec §15)
//!   * Centerline deviation (spec §16)
//!   * Heading deviation (spec §17)
//!   * Threshold distance (spec §19)
//!
//! Status: Phase 3. Phase 1 only places the type skeleton.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runway {
    pub airport_icao: String,
    pub ident: String,
    pub heading_true_deg: f32,
    pub heading_magnetic_deg: f32,
    pub length_m: f32,
    pub width_m: f32,
    pub threshold_lat: f64,
    pub threshold_lon: f64,
    pub end_lat: f64,
    pub end_lon: f64,
    pub displaced_threshold_m: Option<f32>,
    pub elevation_ft: Option<f32>,
    pub surface: Option<String>,
}

/// Great-circle distance in meters between two points (haversine formula).
pub fn distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_M: f64 = 6_371_008.8;
    let to_rad = |d: f64| d.to_radians();
    let (phi1, phi2) = (to_rad(lat1), to_rad(lat2));
    let dphi = to_rad(lat2 - lat1);
    let dlambda = to_rad(lon2 - lon1);
    let a = (dphi / 2.0).sin().powi(2) + phi1.cos() * phi2.cos() * (dlambda / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * a.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_known_distance() {
        // EDDF -> EDDM, ~300 km nominal great-circle.
        let d = distance_m(50.033333, 8.570556, 48.353783, 11.786086);
        assert!((d - 304_000.0).abs() < 5_000.0, "got {d}");
    }
}
