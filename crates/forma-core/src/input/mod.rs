//! Gamepad input, normalised into the actions a sofa UI needs.
//!
//! Gamepads are read in Rust rather than through the WebView's Gamepad API, which WebKitGTK does
//! not support reliably. Themes receive [`InputEvent`]s: a semantic action (`confirm`, `back`,
//! `up`…) plus the physical button, so a theme can follow conventions or show button glyphs.
//!
//! [`Mapper`] holds all of the logic (stick deadzones, hold-to-repeat, the confirm/back swap)
//! and is driven by plain values and timestamps, so it is tested without a controller.

mod device;
mod gamepad;

use std::time::{Duration, Instant};

use serde::Serialize;

pub use device::Device;
pub use gamepad::{InputHandle, spawn};

/// What a controller is, for an icon: by its maker, else its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ControllerKind {
    Xbox,
    Playstation,
    Nintendo,
    /// Anything else: 8BitDo, arcade sticks, generic USB pads…
    Generic,
}

/// A controller's kind from its SDL GUID (whose bytes 4–5 are the USB vendor, little-endian)
/// and its name. The name decides for a third-party pad that copies a console's layout.
pub fn controller_kind(guid: &str, name: &str) -> ControllerKind {
    let name = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&["xbox", "x-box", "xinput"]) {
        return ControllerKind::Xbox;
    }
    if has(&[
        "playstation",
        "dualshock",
        "dualsense",
        "ps3",
        "ps4",
        "ps5",
        "sony",
    ]) {
        return ControllerKind::Playstation;
    }
    if has(&[
        "nintendo",
        "joy-con",
        "pro controller",
        "switch",
        "wii",
        "gamecube",
    ]) {
        return ControllerKind::Nintendo;
    }
    let vendor = guid
        .get(8..12)
        .and_then(|v| u16::from_str_radix(v, 16).ok())
        .map(u16::swap_bytes);
    match vendor {
        Some(0x045e) => ControllerKind::Xbox,
        Some(0x054c) => ControllerKind::Playstation,
        Some(0x057e) => ControllerKind::Nintendo,
        _ => ControllerKind::Generic,
    }
}

/// A physical control, by position (Xbox/SDL convention: `south` is the bottom face button).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Button {
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    South,
    East,
    West,
    North,
    L1,
    R1,
    L2,
    R2,
    L3,
    R3,
    Start,
    Select,
    Hotkey,
}

/// What a UI does with a button press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    /// The remaining face buttons, by position: what they do is up to the theme.
    West,
    North,
    PageUp,
    PageDown,
    L2,
    R2,
    L3,
    R3,
    Start,
    Select,
    Hotkey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputEvent {
    pub action: Action,
    /// `None` when the action came from an analog stick.
    pub button: Option<Button>,
    /// Which gamepad, in connection order.
    pub gamepad: usize,
    pub pressed: bool,
    /// A repeat generated while a direction is held.
    pub repeat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputOptions {
    /// Nintendo layout: confirm on the east button, back on the south one.
    pub swap_confirm: bool,
    /// How long a direction is held before it starts repeating.
    pub repeat_delay: Duration,
    pub repeat_interval: Duration,
    /// Stick deflection that counts as a press, and the lower one that releases it again.
    /// The gap stops a stick resting near the threshold from flickering.
    pub stick_press: f32,
    pub stick_release: f32,
}

impl Default for InputOptions {
    fn default() -> Self {
        Self {
            swap_confirm: false,
            repeat_delay: Duration::from_millis(400),
            repeat_interval: Duration::from_millis(80),
            stick_press: 0.6,
            stick_release: 0.3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickAxis {
    X,
    Y,
}

/// Turns button and stick changes into [`InputEvent`]s, including repeats.
#[derive(Debug)]
pub struct Mapper {
    options: InputOptions,
    /// The direction currently repeating, and when it next fires.
    held: Option<Held>,
    /// Per gamepad: the direction each stick axis is pressing, if any.
    sticks: Vec<[Option<Action>; 2]>,
}

#[derive(Debug, Clone, Copy)]
struct Held {
    action: Action,
    button: Option<Button>,
    gamepad: usize,
    next: Instant,
}

impl Mapper {
    pub fn new(options: InputOptions) -> Self {
        Self {
            options,
            held: None,
            sticks: Vec::new(),
        }
    }

    pub fn action_for(&self, button: Button) -> Action {
        let (confirm, back) = if self.options.swap_confirm {
            (Button::East, Button::South)
        } else {
            (Button::South, Button::East)
        };
        match button {
            b if b == confirm => Action::Confirm,
            b if b == back => Action::Back,
            Button::DpadUp => Action::Up,
            Button::DpadDown => Action::Down,
            Button::DpadLeft => Action::Left,
            Button::DpadRight => Action::Right,
            Button::South | Button::East => unreachable!("covered by confirm/back"),
            Button::West => Action::West,
            Button::North => Action::North,
            Button::L1 => Action::PageUp,
            Button::R1 => Action::PageDown,
            Button::L2 => Action::L2,
            Button::R2 => Action::R2,
            Button::L3 => Action::L3,
            Button::R3 => Action::R3,
            Button::Start => Action::Start,
            Button::Select => Action::Select,
            Button::Hotkey => Action::Hotkey,
        }
    }

    pub fn button(
        &mut self,
        gamepad: usize,
        button: Button,
        pressed: bool,
        now: Instant,
    ) -> Vec<InputEvent> {
        let action = self.action_for(button);
        self.press(gamepad, action, Some(button), pressed, now)
    }

    /// A stick axis moved; `value` is -1.0 (left/up) to 1.0 (right/down).
    pub fn stick(
        &mut self,
        gamepad: usize,
        axis: StickAxis,
        value: f32,
        now: Instant,
    ) -> Vec<InputEvent> {
        if self.sticks.len() <= gamepad {
            self.sticks.resize(gamepad + 1, [None, None]);
        }
        let slot = &mut self.sticks[gamepad][axis as usize];
        let wanted = match (axis, value) {
            (StickAxis::X, v) if v <= -self.options.stick_press => Some(Action::Left),
            (StickAxis::X, v) if v >= self.options.stick_press => Some(Action::Right),
            (StickAxis::Y, v) if v <= -self.options.stick_press => Some(Action::Up),
            (StickAxis::Y, v) if v >= self.options.stick_press => Some(Action::Down),
            // Between the thresholds a pressed direction stays pressed.
            (_, v) if v.abs() > self.options.stick_release => *slot,
            _ => None,
        };
        if wanted == *slot {
            return Vec::new();
        }
        let previous = std::mem::replace(slot, wanted);
        let mut out = Vec::new();
        if let Some(action) = previous {
            out.extend(self.press(gamepad, action, None, false, now));
        }
        if let Some(action) = wanted {
            out.extend(self.press(gamepad, action, None, true, now));
        }
        out
    }

    /// Repeats due at `now`. Call at [`Mapper::next_deadline`].
    pub fn tick(&mut self, now: Instant) -> Vec<InputEvent> {
        let Some(held) = &mut self.held else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while held.next <= now {
            out.push(InputEvent {
                action: held.action,
                button: held.button,
                gamepad: held.gamepad,
                pressed: true,
                repeat: true,
            });
            held.next += self.options.repeat_interval;
        }
        out
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.held.map(|h| h.next)
    }

    /// Forgets everything held, e.g. while a game owns the controllers.
    pub fn reset(&mut self) {
        self.held = None;
        self.sticks.clear();
    }

    fn press(
        &mut self,
        gamepad: usize,
        action: Action,
        button: Option<Button>,
        pressed: bool,
        now: Instant,
    ) -> Vec<InputEvent> {
        let directional = matches!(
            action,
            Action::Up | Action::Down | Action::Left | Action::Right
        );
        if directional {
            if pressed {
                // The latest direction wins: pressing right while holding up repeats right.
                self.held = Some(Held {
                    action,
                    button,
                    gamepad,
                    next: now + self.options.repeat_delay,
                });
            } else if self
                .held
                .is_some_and(|h| h.action == action && h.gamepad == gamepad)
            {
                self.held = None;
            }
        }
        vec![InputEvent {
            action,
            button,
            gamepad,
            pressed,
            repeat: false,
        }]
    }
}

#[cfg(test)]
mod tests;
