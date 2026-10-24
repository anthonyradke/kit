//! Background thread that owns the HID connection to the base station.
//!
//! Settings live on the headset, but other software (GG, the mobile app) can
//! change them, so whenever the headset connects the worker reads them back
//! and rewrites whatever differs from ours. Unchanged settings aren't
//! rewritten: every saved write goes to the headset's flash.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use arctis::{Error, Headset};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::HeadsetSettings;
use crate::state::Shared;

pub enum Cmd {
    /// Desired settings changed (also sent once at startup).
    Apply { eq: arctis::Eq, settings: HeadsetSettings },
    /// Live EQ while a band is being dragged; not saved on the headset.
    PreviewEq(arctis::Eq),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// The USB base station is plugged in.
    pub present: bool,
    /// The headset is on and linked to the base station.
    pub connected: bool,
    pub battery: Option<u8>,
    pub charging: bool,
    pub error: Option<String>,
}

const OPEN_RETRY: Duration = Duration::from_secs(2);
/// Status polls are answered by the base station itself, so they're cheap.
const POLL_EVERY: Duration = Duration::from_secs(2);

pub fn spawn(app: AppHandle, rx: Receiver<Cmd>) {
    std::thread::Builder::new()
        .name("headset".into())
        .spawn(move || Worker::new(app, rx).run())
        .expect("spawn headset thread");
}

struct Worker {
    app: AppHandle,
    rx: Receiver<Cmd>,
    dev: Option<Headset>,
    desired: Option<(arctis::Eq, HeadsetSettings)>,
    /// What we last wrote (or verified) for each setting.
    written: Written,
    next_open: Instant,
    next_poll: Instant,
    status: Status,
}

/// Last value known to be on the headset; `None` means unknown, write it.
#[derive(Default)]
struct Written {
    eq: Option<[u8; 6 * arctis::BANDS]>,
    sidetone: Option<u8>,
    mic_volume: Option<u8>,
    volume_limiter: Option<bool>,
    mic_led: Option<u8>,
    auto_off: Option<u8>,
}

impl Worker {
    fn new(app: AppHandle, rx: Receiver<Cmd>) -> Self {
        let now = Instant::now();
        Worker {
            app,
            rx,
            dev: None,
            desired: None,
            written: Written::default(),
            next_open: now,
            next_poll: now,
            status: Status::default(),
        }
    }

    fn run(mut self) {
        loop {
            let prev = self.status.clone();
            if self.dev.is_none() && Instant::now() >= self.next_open {
                self.open();
            }
            if self.dev.is_some() && Instant::now() >= self.next_poll {
                self.poll();
            }
            if self.status != prev {
                self.publish();
            }
            let next = if self.dev.is_some() { self.next_poll } else { self.next_open };
            match self.rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Apply { eq, settings } => {
                self.desired = Some((eq, settings));
                self.apply();
            }
            Cmd::PreviewEq(eq) => {
                // Coalesce a burst of drag updates into the latest one.
                let mut eq = eq;
                while let Ok(next) = self.rx.try_recv() {
                    match next {
                        Cmd::PreviewEq(e) => eq = e,
                        other => {
                            self.handle(other);
                            return;
                        }
                    }
                }
                if self.with_dev(|h| h.set_eq(&eq, false)).is_some() {
                    // Not saved: make sure the next Apply saves it.
                    self.written.eq = None;
                }
            }
        }
    }

    fn open(&mut self) {
        match Headset::open() {
            Ok(h) => {
                self.dev = Some(h);
                self.status.present = true;
                self.status.error = None;
                self.next_poll = Instant::now();
            }
            Err(e) => {
                self.status = Status { error: Some(e.to_string()), ..Status::default() };
                self.next_open = Instant::now() + OPEN_RETRY;
            }
        }
    }

    fn poll(&mut self) {
        self.next_poll = Instant::now() + POLL_EVERY;
        let Some(st) = self.with_dev(|h| h.status()) else { return };
        let was_connected = self.status.connected;
        self.status.connected = st.connected;
        self.status.battery = st.connected.then_some(st.battery);
        self.status.charging = st.connected && st.charging;
        if st.connected && !was_connected {
            self.on_connect();
        }
    }

    /// The headset just came on: read back what it has and fix what differs.
    fn on_connect(&mut self) {
        self.written = Written::default();
        if let Some(s) = self.with_dev(|h| h.settings()) {
            self.written.eq = Some(arctis::encode_eq(&s.eq));
            self.written.sidetone = Some(s.sidetone);
            self.written.mic_volume = Some(s.mic_volume);
            self.written.volume_limiter = Some(s.volume_limiter);
        }
        self.apply();
    }

    fn apply(&mut self) {
        let Some((eq, s)) = self.desired.clone() else { return };
        let encoded = arctis::encode_eq(&eq);
        if self.written.eq != Some(encoded) && self.with_dev(|h| h.set_eq(&eq, true)).is_some() {
            self.written.eq = Some(encoded);
        }
        if self.written.sidetone != Some(s.sidetone) && self.with_dev(|h| h.set_sidetone(s.sidetone)).is_some() {
            self.written.sidetone = Some(s.sidetone);
        }
        if self.written.mic_volume != Some(s.mic_volume) && self.with_dev(|h| h.set_mic_volume(s.mic_volume)).is_some() {
            self.written.mic_volume = Some(s.mic_volume);
        }
        if self.written.volume_limiter != Some(s.volume_limiter)
            && self.with_dev(|h| h.set_volume_limiter(s.volume_limiter)).is_some()
        {
            self.written.volume_limiter = Some(s.volume_limiter);
        }
        // Not readable, so these are written once per connection.
        if self.written.mic_led != Some(s.mic_led) && self.with_dev(|h| h.set_mic_led(s.mic_led)).is_some() {
            self.written.mic_led = Some(s.mic_led);
        }
        if self.written.auto_off != Some(s.auto_off) && self.with_dev(|h| h.set_inactive_time(s.auto_off)).is_some() {
            self.written.auto_off = Some(s.auto_off);
        }
    }

    /// Run a request; a HID error means the base station was unplugged.
    fn with_dev<T>(&mut self, f: impl FnOnce(&Headset) -> arctis::Result<T>) -> Option<T> {
        let h = self.dev.as_ref()?;
        match f(h) {
            Ok(v) => {
                self.status.error = None;
                Some(v)
            }
            Err(Error::Hid(e)) => {
                self.dev = None;
                self.written = Written::default();
                self.status = Status { error: Some(e.to_string()), ..Status::default() };
                self.next_open = Instant::now() + OPEN_RETRY;
                None
            }
            Err(e) => {
                self.status.error = Some(e.to_string());
                None
            }
        }
    }

    fn publish(&self) {
        let shared = self.app.state::<Shared>();
        *shared.headset.lock().unwrap() = self.status.clone();
        let _ = self.app.emit("headset", &self.status);
        crate::tray::refresh_tooltip(&self.app);
    }
}
