//! Built-in GT7 reference data: car names and track layouts.
//!
//! GT7 sends a numeric car code and no track at all, so both are looked up in
//! tables compiled into the binary. Rebuild them with
//! `scripts/build_gt7_data.py` when Polyphony adds cars or tracks.

pub mod cars;
pub mod tracks;
