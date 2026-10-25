//! Runs side-button actions and plays macros.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread::Thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::config::{Action, KeyRef, Macro, MouseButton, PlayMode, Step};
use super::device::Cmd;
use super::input;
use super::state::{self, Shared};

pub enum Msg {
    Button { forward: bool, down: bool },
    /// A keyboard key bound in the keyboard settings.
    Key { scan: u16, ext: bool, down: bool },
    /// Play a macro once from the UI's Test button.
    Test { id: String },
    /// Stop every running macro (profile switch, macro edited).
    CancelAll,
}

pub fn spawn(app: AppHandle, rx: Receiver<Msg>) {
    std::thread::Builder::new()
        .name("exec".into())
        .spawn(move || {
            let mut exec = Executor { app, pressed: [None, None], keys: HashMap::new(), running: HashMap::new() };
            for msg in rx {
                exec.handle(msg);
            }
        })
        .expect("spawn exec thread");
}

struct Executor {
    app: AppHandle,
    /// Action captured at press time so the release matches it even if the
    /// profile changes in between.
    pressed: [Option<Action>; 2],
    /// The same for bound keyboard keys.
    keys: HashMap<(u16, bool), Action>,
    running: HashMap<String, Arc<Run>>,
}

impl Executor {
    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::CancelAll => {
                for run in self.running.drain().map(|(_, r)| r) {
                    run.cancel();
                }
            }
            Msg::Test { id } => {
                if let Some(m) = self.macro_def(&id) {
                    if let Some(run) = self.running.remove(&id) {
                        run.cancel();
                    }
                    self.running.insert(id, Run::start(m.steps, Some(m.repeat)));
                }
            }
            Msg::Button { forward, down } => {
                let side = forward as usize;
                if down {
                    let action = {
                        let shared = self.app.state::<Shared>();
                        let core = shared.core.lock().unwrap();
                        let p = core.config.active_profile();
                        if forward { p.forward.clone() } else { p.back.clone() }
                    };
                    self.pressed[side] = Some(action.clone());
                    self.press(&action, Some(Self::native(forward)));
                } else if let Some(action) = self.pressed[side].take() {
                    self.release(&action, Some(Self::native(forward)));
                }
            }
            Msg::Key { scan, ext, down } => {
                if down {
                    if let Some(action) = crate::keyboard::state::key_action(&self.app, scan, ext) {
                        self.keys.insert((scan, ext), action.clone());
                        self.press(&action, None);
                    }
                } else if let Some(action) = self.keys.remove(&(scan, ext)) {
                    self.release(&action, None);
                }
            }
        }
    }

    fn native(forward: bool) -> MouseButton {
        if forward { MouseButton::Forward } else { MouseButton::Back }
    }

    /// `native` is the mouse button a side button stands for (none for keys).
    fn press(&mut self, action: &Action, native: Option<MouseButton>) {
        match action {
            // Intercept flag was stale; pass the click through.
            Action::Default => native.into_iter().for_each(|b| input::send_mouse(b, true)),
            Action::Disabled => {}
            Action::Keys { keys } => keys.iter().for_each(|k| input::send_key(k, true)),
            Action::Macro { id } => self.press_macro(id),
            Action::DpiShift { dpi } => self.device(Cmd::TempDpi(Some(*dpi))),
            Action::DpiCycle => state::cycle_dpi(&self.app),
            Action::NextProfile => state::next_profile(&self.app),
            Action::Profile { id } => state::activate(&self.app, id),
        }
    }

    fn release(&mut self, action: &Action, native: Option<MouseButton>) {
        match action {
            Action::Default => native.into_iter().for_each(|b| input::send_mouse(b, false)),
            Action::Keys { keys } => keys.iter().rev().for_each(|k| input::send_key(k, false)),
            Action::DpiShift { .. } => self.device(Cmd::TempDpi(None)),
            Action::Macro { id } => {
                let held = self.macro_def(id).is_some_and(|m| m.mode == PlayMode::WhileHeld);
                if held {
                    if let Some(run) = self.running.remove(id) {
                        run.cancel();
                    }
                }
            }
            _ => {}
        }
    }

    fn device(&self, cmd: Cmd) {
        let _ = self.app.state::<Shared>().device_tx.send(cmd);
    }

    fn macro_def(&self, id: &str) -> Option<Macro> {
        let shared = self.app.state::<Shared>();
        let core = shared.core.lock().unwrap();
        core.config.macro_by_id(id).cloned()
    }

    fn press_macro(&mut self, id: &str) {
        let Some(m) = self.macro_def(id) else { return };
        self.running.retain(|_, r| !r.done.load(Ordering::SeqCst));
        if let Some(run) = self.running.get(id) {
            // Toggle stops a running loop; other modes ignore presses while playing.
            if m.mode == PlayMode::Toggle {
                run.cancel();
                self.running.remove(id);
            }
            return;
        }
        let run = match m.mode {
            PlayMode::Once => Run::start(m.steps, Some(m.repeat)),
            PlayMode::WhileHeld | PlayMode::Toggle => Run::start(m.steps, None),
        };
        self.running.insert(id.to_string(), run);
    }
}

pub struct Run {
    cancelled: AtomicBool,
    done: AtomicBool,
    thread: std::sync::OnceLock<Thread>,
}

impl Run {
    /// Play `steps` `times` times, or forever when `None`, until cancelled.
    fn start(steps: Vec<Step>, times: Option<u32>) -> Arc<Run> {
        let run = Arc::new(Run {
            cancelled: AtomicBool::new(false),
            done: AtomicBool::new(false),
            thread: std::sync::OnceLock::new(),
        });
        let r = run.clone();
        let handle = std::thread::spawn(move || {
            play(&r, &steps, times);
            r.done.store(true, Ordering::SeqCst);
        });
        let _ = run.thread.set(handle.thread().clone());
        run
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.get() {
            t.unpark();
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Sleep that wakes early on cancel. Returns false if cancelled.
    fn sleep(&self, ms: u32) -> bool {
        let deadline = Instant::now() + Duration::from_millis(ms as u64);
        while !self.is_cancelled() {
            let now = Instant::now();
            if now >= deadline {
                return true;
            }
            std::thread::park_timeout(deadline - now);
        }
        false
    }
}

fn play(run: &Run, steps: &[Step], times: Option<u32>) {
    let mut held_keys: Vec<KeyRef> = Vec::new();
    let mut held_buttons: Vec<MouseButton> = Vec::new();
    let has_delay = steps.iter().any(|s| matches!(s, Step::Delay { ms } if *ms > 0));
    let mut iteration = 0u32;
    'outer: while times.is_none_or(|t| iteration < t) {
        iteration += 1;
        for step in steps {
            if run.is_cancelled() {
                break 'outer;
            }
            match step {
                Step::Key { key, down } => {
                    input::send_key(key, *down);
                    if *down {
                        held_keys.push(key.clone());
                    } else {
                        held_keys.retain(|k| k.vk != key.vk);
                    }
                }
                Step::Mouse { button, down } => {
                    input::send_mouse(*button, *down);
                    if *down {
                        held_buttons.push(*button);
                    } else {
                        held_buttons.retain(|b| b != button);
                    }
                }
                Step::Delay { ms } => {
                    if !run.sleep(*ms) {
                        break 'outer;
                    }
                }
            }
        }
        // Never spin a loop with no delays at full speed.
        if !has_delay && !run.sleep(10) {
            break;
        }
    }
    // Never leave keys or buttons stuck down.
    for key in held_keys.iter().rev() {
        input::send_key(key, false);
    }
    for button in held_buttons.iter().rev() {
        input::send_mouse(*button, false);
    }
}
