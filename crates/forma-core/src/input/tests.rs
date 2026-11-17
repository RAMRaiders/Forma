use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn actions(events: &[InputEvent]) -> Vec<(Action, bool, bool)> {
    events
        .iter()
        .map(|e| (e.action, e.pressed, e.repeat))
        .collect()
}

#[test]
fn face_buttons_map_to_confirm_and_back_and_can_be_swapped() {
    let xbox = Mapper::new(InputOptions::default());
    assert_eq!(xbox.action_for(Button::South), Action::Confirm);
    assert_eq!(xbox.action_for(Button::East), Action::Back);
    let nintendo = Mapper::new(InputOptions {
        swap_confirm: true,
        ..InputOptions::default()
    });
    assert_eq!(nintendo.action_for(Button::East), Action::Confirm);
    assert_eq!(nintendo.action_for(Button::South), Action::Back);
    assert_eq!(nintendo.action_for(Button::L1), Action::PageUp);
}

#[test]
fn a_held_direction_repeats_after_the_delay() {
    let mut m = Mapper::new(InputOptions::default());
    let t0 = Instant::now();
    assert_eq!(
        actions(&m.button(0, Button::DpadDown, true, t0)),
        [(Action::Down, true, false)]
    );
    assert!(m.tick(t0 + ms(399)).is_empty());
    assert_eq!(m.tick(t0 + ms(400)).len(), 1);
    // 400 + 3 × 80 = 640: three more repeats due by then.
    assert_eq!(m.tick(t0 + ms(640)).len(), 3);
    assert_eq!(
        actions(&m.button(0, Button::DpadDown, false, t0 + ms(650))),
        [(Action::Down, false, false)]
    );
    assert!(m.next_deadline().is_none());
}

#[test]
fn buttons_other_than_directions_never_repeat() {
    let mut m = Mapper::new(InputOptions::default());
    let t0 = Instant::now();
    m.button(0, Button::South, true, t0);
    assert!(m.next_deadline().is_none());
    assert!(m.tick(t0 + ms(2000)).is_empty());
}

#[test]
fn the_latest_direction_wins_and_releasing_the_old_one_keeps_it() {
    let mut m = Mapper::new(InputOptions::default());
    let t0 = Instant::now();
    m.button(0, Button::DpadUp, true, t0);
    m.button(0, Button::DpadRight, true, t0 + ms(100));
    m.button(0, Button::DpadUp, false, t0 + ms(200));
    let repeats = m.tick(t0 + ms(500));
    assert_eq!(actions(&repeats), [(Action::Right, true, true)]);
}

#[test]
fn a_stick_presses_past_the_threshold_and_releases_below_the_lower_one() {
    let mut m = Mapper::new(InputOptions::default());
    let t0 = Instant::now();
    assert!(
        m.stick(0, StickAxis::Y, 0.5, t0).is_empty(),
        "under the press threshold"
    );
    assert_eq!(
        actions(&m.stick(0, StickAxis::Y, 0.7, t0)),
        [(Action::Down, true, false)]
    );
    // Drifting back to 0.4 is still above the release threshold: no flicker.
    assert!(m.stick(0, StickAxis::Y, 0.4, t0).is_empty());
    assert_eq!(
        actions(&m.stick(0, StickAxis::Y, 0.1, t0)),
        [(Action::Down, false, false)]
    );
}

#[test]
fn flicking_a_stick_across_releases_one_side_and_presses_the_other() {
    let mut m = Mapper::new(InputOptions::default());
    let t0 = Instant::now();
    m.stick(0, StickAxis::X, -0.9, t0);
    assert_eq!(
        actions(&m.stick(0, StickAxis::X, 0.9, t0)),
        [(Action::Left, false, false), (Action::Right, true, false)]
    );
    let events = m.stick(0, StickAxis::X, 0.9, t0);
    assert!(events.is_empty());
    assert!(
        m.tick(t0 + ms(400))[0].button.is_none(),
        "stick repeats carry no button"
    );
}

#[test]
fn reset_forgets_held_directions() {
    let mut m = Mapper::new(InputOptions::default());
    let t0 = Instant::now();
    m.button(0, Button::DpadLeft, true, t0);
    m.reset();
    assert!(m.tick(t0 + ms(1000)).is_empty());
}

#[test]
fn the_gamepad_thread_starts_pauses_and_stops_without_a_controller() {
    let handle = match spawn(InputOptions::default(), String::new(), |_| {}) {
        Ok(handle) => handle,
        // Some CI sandboxes have no HID access at all; that is the backend, not this code.
        Err(e) => return eprintln!("skipped: gamepad backend unavailable: {e}"),
    };
    handle.set_paused(true);
    handle.set_paused(false);
    assert!(handle.controllers().iter().all(|c| c.guid.len() == 32));
    let started = Instant::now();
    drop(handle);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "stop is noticed within IDLE_WAKE"
    );
}

#[test]
fn a_controller_kind_comes_from_its_maker_or_its_name() {
    use super::{ControllerKind as K, controller_kind as kind};
    // An Xbox 360 pad: Microsoft's vendor id, 0x045e, at bytes 4–5 of the GUID.
    assert_eq!(
        kind("030000005e0400008e02000014010000", "Generic X-Box pad"),
        K::Xbox
    );
    assert_eq!(
        kind("030000005e0400008e02000014010000", "Controller"),
        K::Xbox
    );
    assert_eq!(
        kind("030000004c050000cc09000011810000", "Wireless Controller"),
        K::Playstation
    );
    assert_eq!(
        kind("050000007e0500000920000001800000", "Pro Controller"),
        K::Nintendo
    );
    assert_eq!(
        kind("03000000c82d00000160000011010000", "8BitDo SN30 Pro"),
        K::Generic
    );
    assert_eq!(
        kind("03000000c82d00000160000011010000", "8BitDo Xbox mode"),
        K::Xbox
    );
    assert_eq!(kind("", "USB Gamepad"), K::Generic);
}
