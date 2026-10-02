//! Wording. Facts and numbers come from the caller; only the phrasing
//! rotates, so the engineer doesn't sound scripted.

use std::collections::HashMap;

#[derive(Default)]
pub struct Phrases {
    next: HashMap<&'static str, usize>,
}

impl Phrases {
    /// Rotate through equivalent wordings for `key`, in order.
    pub fn pick(&mut self, key: &'static str, options: &[&str]) -> String {
        let i = self.next.entry(key).or_insert(0);
        let s = options[*i % options.len()].to_string();
        *i += 1;
        s
    }
}

/// "1 34 point 7": reads naturally through TTS.
pub fn speak_time(ms: u32) -> String {
    let m = ms / 60_000;
    let s = (ms % 60_000) / 1000;
    let tenths = (ms % 1000) / 100;
    match (m, s) {
        (0, _) => format!("{s} point {tenths}"),
        (_, 0..=9) => format!("{m} oh {s} point {tenths}"),
        _ => format!("{m} {s} point {tenths}"),
    }
}

pub fn speak_gap(ms: u32) -> String {
    match ms {
        0..=49 => "a few hundredths".into(),
        50..=149 => "a tenth".into(),
        150..=949 => format!("{} tenths", (ms + 50) / 100),
        _ => format!("{:.1} seconds", ms as f32 / 1000.0),
    }
}

/// Laps of fuel, rounded to the nearest half: "about 2 and a half laps".
pub fn speak_laps(laps: f32) -> String {
    if laps < 0.75 {
        return "less than a lap".into();
    }
    let halves = (laps * 2.0).round() as u32;
    let (whole, half) = (halves / 2, halves % 2 == 1);
    match (whole, half) {
        (1, false) => "about one lap".into(),
        (1, true) => "about one and a half laps".into(),
        (n, false) => format!("about {n} laps"),
        (n, true) => format!("about {n} and a half laps"),
    }
}

pub fn word(n: usize) -> &'static str {
    ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"].get(n).copied().unwrap_or("")
}

pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_numbers() {
        assert_eq!(speak_time(94_700), "1 34 point 7");
        assert_eq!(speak_time(64_350), "1 oh 4 point 3");
        assert_eq!(speak_time(42_000), "42 point 0");
        assert_eq!(speak_gap(320), "3 tenths");
        assert_eq!(speak_gap(1460), "1.5 seconds");
        assert_eq!(speak_laps(1.4), "about one and a half laps");
        assert_eq!(speak_laps(6.1), "about 6 laps");
        assert_eq!(capitalize("a tenth"), "A tenth");
    }

    #[test]
    fn phrases_rotate_deterministically() {
        let mut p = Phrases::default();
        let opts = ["A.", "B.", "C."];
        let got: Vec<_> = (0..4).map(|_| p.pick("k", &opts)).collect();
        assert_eq!(got, ["A.", "B.", "C.", "A."]);
    }
}
