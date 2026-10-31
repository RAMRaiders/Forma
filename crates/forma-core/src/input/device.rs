//! What Batocera's configgen wants to know about a pad beyond its GUID and name: the device node
//! and SDL's button, hat and axis counts, which EmulationStation passes as `-pNdevicepath`,
//! `-pNnbbuttons`, `-pNnbhats` and `-pNnbaxes`. configgen 43 fails without the device node.
//!
//! The counts come from the kernel's capability bitmaps in sysfs, counted the way SDL counts
//! them: buttons are the key codes from `BTN_MISC` up, hats are the `ABS_HAT*` pairs, axes are
//! every other absolute axis.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    /// `/dev/input/eventN`.
    pub path: PathBuf,
    pub buttons: u32,
    pub hats: u32,
    pub axes: u32,
}

const BTN_MISC: usize = 0x100;
const KEY_CNT: usize = 0x300;
const ABS_HAT0X: usize = 0x10;
const ABS_HAT3Y: usize = 0x17;
const ABS_CNT: usize = 0x40;

impl Device {
    /// Reads the counts for an event device from sysfs; `None` when it is not there.
    pub fn read(path: &Path) -> Option<Self> {
        let caps = Path::new("/sys/class/input")
            .join(path.file_name()?)
            .join("device/capabilities");
        let read = |name: &str| std::fs::read_to_string(caps.join(name)).ok();
        Some(Self::from_capabilities(
            path,
            &read("key")?,
            &read("abs").unwrap_or_default(),
        ))
    }

    /// The counts from sysfs's `key` and `abs` bitmaps.
    pub fn from_capabilities(path: &Path, key: &str, abs: &str) -> Self {
        let keys = bits(key);
        let abs = bits(abs);
        let set = |bits: &[usize], range: std::ops::Range<usize>| {
            bits.iter().filter(|b| range.contains(b)).count() as u32
        };
        let hats = (ABS_HAT0X..=ABS_HAT3Y)
            .step_by(2)
            .filter(|h| abs.contains(h) || abs.contains(&(h + 1)))
            .count() as u32;
        Self {
            path: path.to_path_buf(),
            buttons: set(&keys, BTN_MISC..KEY_CNT),
            hats,
            axes: set(&abs, 0..ABS_CNT) - set(&abs, ABS_HAT0X..ABS_HAT3Y + 1),
        }
    }
}

/// The set bits of a sysfs bitmap: hex words of one `long` each, most significant first.
fn bits(bitmap: &str) -> Vec<usize> {
    let word = usize::BITS as usize;
    bitmap
        .split_whitespace()
        .rev()
        .enumerate()
        .flat_map(|(i, w)| {
            let value = u64::from_str_radix(w, 16).unwrap_or(0);
            (0..64)
                .filter(move |b| value >> b & 1 == 1)
                .map(move |b| i * word + b)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_an_xbox_360_pad_as_sdl_does() {
        // Read from a Raspberry Pi 5 (64-bit `long`s): SDL reports 11 buttons, 1 hat, 6 axes.
        let pad = Device::from_capabilities(
            Path::new("/dev/input/event10"),
            "7cdb000000000000 0 0 0 0\n",
            "3003f\n",
        );
        assert_eq!((pad.buttons, pad.hats, pad.axes), (11, 1, 6));
        assert_eq!(pad.path, Path::new("/dev/input/event10"));
    }

    #[test]
    fn keyboard_keys_are_not_buttons_and_an_empty_bitmap_is_nothing() {
        // KEY_A (30) and BTN_SOUTH (0x130) on a 64-bit `long` layout.
        let dev = Device::from_capabilities(
            Path::new("/dev/input/event1"),
            "1000000000000 0 0 0 40000000",
            "0",
        );
        assert_eq!((dev.buttons, dev.hats, dev.axes), (1, 0, 0));
    }
}
