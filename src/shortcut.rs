use serde::{Deserialize, Serialize};

pub const ALT: u32 = 1;
pub const CONTROL: u32 = 2;
pub const SHIFT: u32 = 4;
pub const WIN: u32 = 8;
pub const LEFT_ALT: u32 = 0xa4;
const TAP_INTERVAL: u32 = 350;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    pub modifiers: u32,
    pub key: u32,
}

impl Shortcut {
    pub fn is_combination(self) -> bool {
        (self.modifiers != 0 || (0x70..=0x7a).contains(&self.key))
            && self.modifiers & !(ALT | CONTROL | SHIFT | WIN) == 0
            && (7..0xff).contains(&self.key)
            && !matches!(self.key, 0x10..=0x12 | 0x1b | 0x5b..=0x5c | 0x7b | 0xa0..=0xa5 | 0xe5 | 0xe7)
    }
}

#[derive(Default)]
pub struct AltTaps {
    pressed: Option<u32>,
    released: Option<u32>,
}

impl AltTaps {
    pub fn key(&mut self, key: u32, down: bool, time: u32, other_modifier: bool) -> bool {
        if key != LEFT_ALT || other_modifier {
            if down || key == LEFT_ALT {
                *self = Self::default();
            }
            return false;
        }
        if down {
            self.pressed.get_or_insert(time);
            return false;
        }
        let Some(pressed) = self.pressed.take() else {
            return false;
        };
        if time.wrapping_sub(pressed) > TAP_INTERVAL {
            self.released = None;
            return false;
        }
        let triggered = self
            .released
            .is_some_and(|previous| time.wrapping_sub(previous) <= TAP_INTERVAL);
        self.released = (!triggered).then_some(time);
        triggered
    }
}
