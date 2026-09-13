use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Mode numbers follow ANSI/DEC; the boolean is true for DEC's `?` prefix.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Modes {
    values: BTreeMap<(bool, u16), bool>,
    saved: BTreeMap<(bool, u16), bool>,
}

const ANSI: &[u16] = &[2, 4, 12, 20];
const DEC: &[u16] = &[
    1, 3, 4, 5, 6, 7, 8, 9, 12, 25, 40, 45, 47, 66, 67, 69, 1000, 1002, 1003, 1004, 1005, 1006,
    1007, 1015, 1016, 1035, 1036, 1039, 1045, 1047, 1048, 1049, 2004, 2026, 2027, 2031, 2033, 2048,
    5522,
];

impl Default for Modes {
    fn default() -> Self {
        let mut values = BTreeMap::new();
        for &mode in ANSI {
            values.insert((false, mode), mode == 12);
        }
        for &mode in DEC {
            values.insert((true, mode), matches!(mode, 7 | 25 | 1007 | 1035 | 1036));
        }
        Self {
            values,
            saved: BTreeMap::new(),
        }
    }
}

impl Modes {
    pub fn get(&self, private: bool, mode: u16) -> bool {
        self.values.get(&(private, mode)).copied().unwrap_or(false)
    }
    pub fn dec(&self, mode: u16) -> bool {
        self.get(true, mode)
    }
    pub fn set(&mut self, private: bool, mode: u16, value: bool) -> bool {
        if let Some(current) = self.values.get_mut(&(private, mode)) {
            *current = value;
            true
        } else {
            false
        }
    }
    pub fn save(&mut self, private: bool, mode: u16) {
        self.saved.insert((private, mode), self.get(private, mode));
    }
    pub fn restore(&mut self, private: bool, mode: u16) -> bool {
        let value = self.saved.get(&(private, mode)).copied().unwrap_or(false);
        self.set(private, mode, value);
        value
    }
    pub fn report(&self, private: bool, mode: u16) -> u8 {
        if private && mode == 117 {
            return 4;
        }
        self.values
            .get(&(private, mode))
            .map_or(0, |v| if *v { 1 } else { 2 })
    }
}
