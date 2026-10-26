//! SatHunterSDR satellite logic: SatNOGS / CelesTrak data, SGP4 orbits, passes, Doppler
//! tracking and Maidenhead locators. Platform independent, no I/O.

pub mod db;
pub mod locator;
pub mod orbit;
pub mod passes;
pub mod tracker;

pub use db::{Band, Filter, Satellite, Transmitter};
pub use orbit::{doppler, Look, Orbit, Station};
pub use passes::{next_pass, track, Pass};
