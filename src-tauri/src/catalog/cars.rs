//! Car code → display name ("Porsche 911 GT3 RS (992) '22").
//!
//! Source: ddm999/gt7info (MIT-0), the community database of GT7 car IDs.

use std::collections::HashMap;
use std::sync::OnceLock;

const CARS_CSV: &str = include_str!("../../data/gt7_cars.csv");

fn table() -> &'static HashMap<i32, &'static str> {
    static TABLE: OnceLock<HashMap<i32, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        CARS_CSV
            .lines()
            .skip(1)
            .filter_map(|line| {
                let (id, name) = line.split_once(',')?;
                Some((id.trim().parse().ok()?, name.trim()))
            })
            .collect()
    })
}

/// The car's full name, or None for a code the table doesn't know yet
/// (a car added in a newer GT7 update).
pub fn car_name(code: i32) -> Option<&'static str> {
    table().get(&code).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_cars_have_names() {
        assert!(table().len() > 500, "only {} cars loaded", table().len());
        assert_eq!(car_name(3442), Some("Autobianchi A112 Abarth '85"));
        assert_eq!(car_name(24), Some("Nissan 180SX Type X '96"));
        assert_eq!(car_name(-1), None);
        assert_eq!(car_name(0), None);
    }
}
