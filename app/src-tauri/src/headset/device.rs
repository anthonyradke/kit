//! Background thread that owns the HID connection to the base station.
//!
//! Settings live on the headset, but other software (GG, the mobile app) can
//! change them, so whenever the headset connects the worker reads them back
//! and rewrites whatever differs from ours. Unchanged settings aren't
//! rewritten: every saved write goes to the headset's flash.
//!
//! The status (on or off, battery, charging) is asked for
//! once when the base station is found and when the headset comes on. After
//! that the worker only listens: the base station pushes every change on its
//! event interface, which a second thread reads. Asking for the status every
//! few seconds, as kit used to, is what GG doesn't do, and another Nova 5
//! tool measured crackles in the audio after each such request.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use arctis::{Error, Headset};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::config::HeadsetSettings;
use super::state::Shared;

pub enum Cmd {
    /// Desired settings changed (also sent once at startup).
    Apply(HeadsetSettings),
    /// The base station pushed a change; from the event thread of the given
    /// connection.
    Event(u64, arctis::Event),
    /// The event thread of the given connection lost the base station.
    EventsLost(u64),
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
/// Only when the event interface can't be opened: ask for the status this
/// often instead.
const FALLBACK_POLL: Duration = Duration::from_secs(30);
/// While events arrive, how soon to ask again after a status request got no answer.
const STATUS_RETRY: Duration = Duration::from_secs(5);
/// How long the event thread waits per read, so it notices a new connection.
const EVENT_WAIT: Duration = Duration::from_secs(2);
/// Nothing scheduled: wait for a command this long at most.
const IDLE: Duration = Duration::from_secs(3600);

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
    desired: Option<HeadsetSettings>,
    /// What we last wrote (or verified) for each setting.
    written: Written,
    next_open: Instant,
    /// When to ask for the status again; `None` while events arrive and the last answer came.
    next_poll: Option<Instant>,
    /// The base station pushes events, so the status is only asked for on connect.
    listening: bool,
    /// Counts connections to the base station; an event thread runs while
    /// its connection is the current one.
    connection: Arc<AtomicU64>,
    status: Status,
    published: Status,
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
        Worker {
            app,
            rx,
            dev: None,
            desired: None,
            written: Written::default(),
            next_open: Instant::now(),
            next_poll: None,
            listening: false,
            connection: Arc::new(AtomicU64::new(0)),
            status: Status::default(),
            published: Status::default(),
        }
    }

    fn run(mut self) {
        loop {
            let now = Instant::now();
            if self.dev.is_none() && now >= self.next_open {
                self.open();
            }
            if self.dev.is_some() && self.next_poll.is_some_and(|t| now >= t) {
                self.query_status();
            }
            if self.status != self.published {
                self.publish();
            }
            let wake = if self.dev.is_none() { Some(self.next_open) } else { self.next_poll };
            let timeout = wake.map_or(IDLE, |t| t.saturating_duration_since(Instant::now()));
            match self.rx.recv_timeout(timeout) {
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Apply(settings) => {
                self.desired = Some(settings);
                self.apply();
            }
            Cmd::Event(connection, event) if connection == self.connection.load(Ordering::Relaxed) => self.on_event(event),
            Cmd::EventsLost(connection) if connection == self.connection.load(Ordering::Relaxed) => {
                self.lost("the base station stopped answering".into());
            }
            // From the event thread of an earlier connection.
            Cmd::Event(..) | Cmd::EventsLost(_) => {}
        }
    }

    fn open(&mut self) {
        match Headset::open() {
            Ok(h) => {
                self.dev = Some(h);
                self.status.present = true;
                self.status.error = None;
                let connection = self.connection.fetch_add(1, Ordering::Relaxed) + 1;
                let listening = match arctis::Events::open() {
                    Ok(events) => {
                        self.spawn_events(events, connection);
                        true
                    }
                    Err(e) => {
                        log::warn!("headset: no event interface ({e}); asking for the status every {} s", FALLBACK_POLL.as_secs());
                        false
                    }
                };
                self.listening = listening;
                self.next_poll = (!listening).then(Instant::now);
                if listening {
                    self.query_status();
                }
            }
            Err(e) => {
                self.status = Status { error: Some(e.to_string()), ..Status::default() };
                self.next_open = Instant::now() + OPEN_RETRY;
            }
        }
    }

    /// Read the base station's pushed events on a thread of their own and
    /// hand them to the worker, until another connection replaces this one.
    fn spawn_events(&self, events: arctis::Events, connection: u64) {
        let tx = self.app.state::<Shared>().headset_tx.clone();
        let current = self.connection.clone();
        let spawned = std::thread::Builder::new().name("headset-events".into()).spawn(move || {
            while current.load(Ordering::Relaxed) == connection {
                match events.next(EVENT_WAIT) {
                    Ok(Some(event)) => {
                        if tx.send(Cmd::Event(connection, event)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {
                        let _ = tx.send(Cmd::EventsLost(connection));
                        return;
                    }
                }
            }
        });
        if let Err(e) = spawned {
            log::error!("headset: can't start the event thread: {e}");
        }
    }

    /// Ask the base station for the status once. Without events it's asked again every
    /// FALLBACK_POLL; with them, only until one answer comes, since pushes only report changes
    /// and a missed first answer would otherwise leave the headset shown off.
    fn query_status(&mut self) {
        let reply = self.with_dev(|h| h.status());
        if self.dev.is_some() {
            self.next_poll = match (self.listening, &reply) {
                (false, _) => Some(Instant::now() + FALLBACK_POLL),
                (true, Some(_)) => None,
                (true, None) => Some(Instant::now() + STATUS_RETRY),
            };
        }
        let Some(st) = reply else { return };
        if st.connected {
            self.status.battery = Some(st.battery);
            self.status.charging = st.charging;
        }
        self.set_connected(st.connected);
    }

    fn on_event(&mut self, event: arctis::Event) {
        match event {
            // The status reply fills in battery, charging and the dial,
            // which the pushes only send when they change.
            arctis::Event::Connected(true) if !self.status.connected => self.query_status(),
            arctis::Event::Connected(on) => self.set_connected(on),
            // Battery news while shown off may mean a missed connect. Asking is harmless if not.
            _ if !self.status.connected => self.query_status(),
            arctis::Event::Battery(percent) => self.status.battery = Some(percent),
            arctis::Event::Charging(charging) => self.status.charging = charging,
        }
    }

    fn set_connected(&mut self, connected: bool) {
        let was = self.status.connected;
        self.status.connected = connected;
        if !connected {
            self.status.battery = None;
            self.status.charging = false;
        }
        if connected && !was {
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
        let Some(s) = self.desired.clone() else { return };
        // The EQ runs in kit's APO; the headset's own stays flat.
        let eq = [None; arctis::BANDS];
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

    /// The base station is gone (unplugged): stop its event thread, give
    /// every app its volume back and look for it again shortly.
    fn lost(&mut self, error: String) {
        self.dev = None;
        self.connection.fetch_add(1, Ordering::Relaxed);
        self.written = Written::default();
        self.next_poll = None;
        self.status = Status { error: Some(error), ..Status::default() };
        self.next_open = Instant::now() + OPEN_RETRY;
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
                self.lost(e.to_string());
                None
            }
            Err(e) => {
                crate::logging::set_error(&mut self.status.error, "headset", e.to_string());
                None
            }
        }
    }

    fn publish(&mut self) {
        self.published = self.status.clone();
        let shared = self.app.state::<Shared>();
        *shared.headset.lock().unwrap() = self.status.clone();
        let _ = self.app.emit("headset", &self.status);
        crate::tray::refresh_status(&self.app);
    }
}
