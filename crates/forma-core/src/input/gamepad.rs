//! The gamepad thread: gilrs events in, [`InputEvent`]s out.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gilrs::{Axis, EventType, Gilrs};

use super::{Button, InputEvent, InputOptions, Mapper, StickAxis};
use crate::launch::Controller;

/// How often the thread wakes when nothing is happening, to notice a stop request.
const IDLE_WAKE: Duration = Duration::from_millis(100);

/// Runs until dropped.
pub struct InputHandle {
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    controllers: Arc<Mutex<Vec<Controller>>>,
    thread: Option<JoinHandle<()>>,
}

impl InputHandle {
    /// While a game runs the emulator owns the controllers: pause so the UI does not react to
    /// gameplay, and forget anything held when it resumes.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    /// Connected gamepads in player order, for the launcher's `%CONTROLLERSCONFIG%`.
    pub fn controllers(&self) -> Vec<Controller> {
        self.controllers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for InputHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts reading gamepads. Fails only if the platform's gamepad backend cannot start.
pub fn spawn(
    options: InputOptions,
    mut on_event: impl FnMut(InputEvent) + Send + 'static,
) -> Result<InputHandle, String> {
    let paused = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let controllers = Arc::new(Mutex::new(Vec::new()));
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let thread = {
        let (paused, stop, controllers) = (paused.clone(), stop.clone(), controllers.clone());
        std::thread::Builder::new()
            .name("forma-input".into())
            .spawn(move || {
                // Gilrs is not Send: it is created on the thread that uses it.
                let mut gilrs = match Gilrs::new() {
                    Ok(g) => {
                        let _ = ready_tx.send(Ok(()));
                        g
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                list_controllers(&gilrs, &controllers);
                let mut mapper = Mapper::new(options);
                let mut was_paused = false;

                while !stop.load(Ordering::SeqCst) {
                    let timeout = mapper
                        .next_deadline()
                        .map_or(IDLE_WAKE, |d| d.saturating_duration_since(Instant::now()))
                        .min(IDLE_WAKE);
                    let event = gilrs.next_event_blocking(Some(timeout));
                    let now = Instant::now();

                    if paused.load(Ordering::SeqCst) {
                        was_paused = true;
                        continue;
                    }
                    if was_paused {
                        mapper.reset();
                        was_paused = false;
                    }

                    if let Some(event) = event {
                        let pad: usize = event.id.into();
                        let produced = match event.event {
                            EventType::ButtonPressed(b, _) => {
                                button(b).map(|b| mapper.button(pad, b, true, now))
                            }
                            EventType::ButtonReleased(b, _) => {
                                button(b).map(|b| mapper.button(pad, b, false, now))
                            }
                            EventType::AxisChanged(Axis::LeftStickX, v, _) => {
                                Some(mapper.stick(pad, StickAxis::X, v, now))
                            }
                            // gilrs reports up as positive; screens count down.
                            EventType::AxisChanged(Axis::LeftStickY, v, _) => {
                                Some(mapper.stick(pad, StickAxis::Y, -v, now))
                            }
                            EventType::Connected | EventType::Disconnected => {
                                list_controllers(&gilrs, &controllers);
                                None
                            }
                            _ => None,
                        };
                        produced.into_iter().flatten().for_each(&mut on_event);
                    }
                    mapper.tick(now).into_iter().for_each(&mut on_event);
                }
            })
            .map_err(|e| e.to_string())?
    };

    ready_rx
        .recv()
        .map_err(|_| "input thread exited before starting".to_owned())??;
    Ok(InputHandle {
        paused,
        stop,
        controllers,
        thread: Some(thread),
    })
}

fn button(b: gilrs::Button) -> Option<Button> {
    use gilrs::Button as G;
    Some(match b {
        G::DPadUp => Button::DpadUp,
        G::DPadDown => Button::DpadDown,
        G::DPadLeft => Button::DpadLeft,
        G::DPadRight => Button::DpadRight,
        G::South => Button::South,
        G::East => Button::East,
        G::West => Button::West,
        G::North => Button::North,
        G::LeftTrigger => Button::L1,
        G::RightTrigger => Button::R1,
        G::LeftTrigger2 => Button::L2,
        G::RightTrigger2 => Button::R2,
        G::LeftThumb => Button::L3,
        G::RightThumb => Button::R3,
        G::Start => Button::Start,
        G::Select => Button::Select,
        G::Mode => Button::Hotkey,
        _ => return None,
    })
}

fn list_controllers(gilrs: &Gilrs, out: &Mutex<Vec<Controller>>) {
    let list = gilrs
        .gamepads()
        .enumerate()
        .map(|(index, (_, pad))| Controller {
            index,
            // gilrs reports SDL's GUID layout, which is what Batocera's configgen matches on.
            guid: pad.uuid().iter().map(|b| format!("{b:02x}")).collect(),
            name: pad.name().to_owned(),
        })
        .collect();
    *out.lock().unwrap_or_else(PoisonError::into_inner) = list;
}
