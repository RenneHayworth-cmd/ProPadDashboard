//! Windows controller transport. Protocol decoding lives in `propad-core`.
//! No exclusive handles, remapping driver, light commands or vibration commands are used.
use propad_core::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
mod native;
#[cfg(windows)]
mod worker;
#[cfg(windows)]
mod xbox;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Debug, Default)]
pub struct InputDiagnostics {
    pub reports: u64,
    pub observed_buttons: u32,
    pub observed_axes: u32,
}

struct Shared {
    stop: AtomicBool,
    visible: AtomicBool,
    reset: AtomicBool,
    wake: (Mutex<bool>, Condvar),
    snapshots: RwLock<HashMap<String, ControllerSnapshot>>,
    inputs: RwLock<HashMap<String, ControllerInput>>,
    reports: AtomicU64,
    observed_buttons: AtomicU32,
    observed_axes: AtomicU32,
    generation: AtomicU64,
    epochs: RwLock<HashMap<String, u64>>,
}

impl Shared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            stop: AtomicBool::new(false),
            visible: AtomicBool::new(false),
            reset: AtomicBool::new(false),
            wake: (Mutex::new(false), Condvar::new()),
            snapshots: RwLock::new(HashMap::new()),
            inputs: RwLock::new(HashMap::new()),
            reports: AtomicU64::new(0),
            observed_buttons: AtomicU32::new(0),
            observed_axes: AtomicU32::new(0),
            generation: AtomicU64::new(1),
            epochs: RwLock::new(HashMap::new()),
        })
    }
    fn notify(&self) {
        *self.wake.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.wake.1.notify_all();
    }
    fn wait(&self, millis: u64) -> bool {
        let wake = self.wake.0.lock().unwrap_or_else(|e| e.into_inner());
        let (mut wake, _) = self
            .wake
            .1
            .wait_timeout_while(wake, Duration::from_millis(millis), |v| {
                !*v && !self.stop.load(Ordering::Acquire)
            })
            .unwrap_or_else(|e| e.into_inner());
        let value = *wake;
        *wake = false;
        value
    }
    fn on_input(&self, key: &str, input: Option<ControllerInput>) {
        let mut inputs = self.inputs.write().unwrap_or_else(|e| e.into_inner());
        if let Some(input) = input {
            self.reports.fetch_add(1, Ordering::Relaxed);
            self.observed_buttons
                .fetch_or(input.buttons, Ordering::Relaxed);
            let axes = (if input.left_x < -0.25 { 1 } else { 0 })
                | (if input.left_x > 0.25 { 2 } else { 0 })
                | (if input.left_y < -0.25 { 4 } else { 0 })
                | (if input.left_y > 0.25 { 8 } else { 0 })
                | (if input.right_x < -0.25 { 16 } else { 0 })
                | (if input.right_x > 0.25 { 32 } else { 0 })
                | (if input.right_y < -0.25 { 64 } else { 0 })
                | (if input.right_y > 0.25 { 128 } else { 0 });
            self.observed_axes.fetch_or(axes, Ordering::Relaxed);
            inputs.insert(key.to_owned(), input);
        } else {
            inputs.remove(key);
        }
    }
    fn set_snapshot(&self, key: &str, snapshot: ControllerSnapshot) {
        self.snapshots
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.to_owned(), snapshot);
    }
    fn remove(&self, key: &str) {
        self.epochs
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
        self.snapshots
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
        self.on_input(key, None);
    }
}

/// Owns native background threads. Readers never block on HID I/O.
pub struct Monitor {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::start()
    }
}
impl Monitor {
    pub fn start() -> Self {
        let shared = Shared::new();
        let state = shared.clone();
        let handle = thread::Builder::new()
            .name("controller-discovery".into())
            .spawn(move || {
                #[cfg(windows)]
                run(state);
                #[cfg(not(windows))]
                while !state.stop.load(Ordering::Acquire) {
                    state.wait(300);
                }
            })
            .expect("controller supervisor thread");
        Self {
            shared,
            thread: Some(handle),
        }
    }
    pub fn snapshot(&self) -> Vec<ControllerSnapshot> {
        self.devices()
    }
    pub fn devices(&self) -> Vec<ControllerSnapshot> {
        let values = self
            .shared
            .snapshots
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        merge_snapshots(&values)
    }
    pub fn input(&self, device_id: &str) -> Option<ControllerInput> {
        // Match the transport chosen for the battery snapshot, preferring USB on a tie.
        let transport = self
            .devices()
            .iter()
            .find(|s| s.device_id == device_id)
            .and_then(|s| s.transport);
        self.shared
            .inputs
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|i| i.device_id == device_id)
            .max_by_key(|i| {
                (
                    Some(i.transport) == transport,
                    i.transport == Transport::Usb,
                    i.timestamp,
                )
            })
            .cloned()
    }
    pub fn set_visible(&self, visible: bool) {
        self.shared.visible.store(visible, Ordering::Release);
    }
    /// Re-enumerate promptly, keeping healthy workers intact.
    pub fn refresh(&self) {
        self.shared.notify();
    }
    /// Power-resume path: stale native handles are replaced, with no synthetic disconnection event.
    pub fn resume(&self) {
        self.shared.reset.store(true, Ordering::Release);
        self.shared.notify();
    }
    pub fn diagnostics(&self) -> InputDiagnostics {
        InputDiagnostics {
            reports: self.shared.reports.load(Ordering::Relaxed),
            observed_buttons: self.shared.observed_buttons.load(Ordering::Relaxed),
            observed_axes: self.shared.observed_axes.load(Ordering::Relaxed),
        }
    }
    pub fn reset_diagnostics(&self) {
        self.shared.reports.store(0, Ordering::Relaxed);
        self.shared.observed_buttons.store(0, Ordering::Relaxed);
        self.shared.observed_axes.store(0, Ordering::Relaxed);
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.notify();
        // A defective kernel driver must not freeze the UI on application exit.
        // Detached workers retain their Arc and native buffers until native I/O actually completes.
        if let Some(handle) = self.thread.take() {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while !handle.is_finished() && std::time::Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(windows)]
fn run(shared: Arc<Shared>) {
    let _notifications = native::DeviceNotifications::new(shared.clone());
    let xbox_shared = shared.clone();
    let xbox = thread::Builder::new()
        .name("xinput-monitor".into())
        .spawn(move || xbox::run(xbox_shared))
        .ok();
    let mut workers: HashMap<String, worker::Worker> = HashMap::new();
    let mut next_scan = 0;
    while !shared.stop.load(Ordering::Acquire) {
        let now = now_ms();
        if shared.reset.swap(false, Ordering::AcqRel) {
            for worker in workers.values() {
                worker.stop();
            }
            workers.clear();
            shared
                .epochs
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            shared
                .inputs
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|k, _| k.starts_with("xinput:"));
            shared
                .snapshots
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|k, _| k.starts_with("xinput:"));
            next_scan = 0;
        }
        if now >= next_scan {
            if let Ok(candidates) = native::enumerate() {
                let candidates = candidates
                    .into_iter()
                    .filter(|d| d.family != ControllerFamily::Xbox)
                    .filter(|d| {
                        !(d.transport == Transport::Bluetooth
                            && is_address(&d.address)
                            && native::bluetooth_connected(&d.address) == Some(false))
                    })
                    .collect::<Vec<_>>();
                let gone = workers
                    .keys()
                    .filter(|path| !candidates.iter().any(|d| &d.path == *path))
                    .cloned()
                    .collect::<Vec<_>>();
                for key in gone {
                    if let Some(worker) = workers.remove(&key) {
                        worker.stop();
                    }
                    shared.remove(&key);
                }
                for candidate in candidates {
                    let key = candidate.path.clone();
                    let retry = workers
                        .get(&key)
                        .map(|w| w.finished() && now.saturating_sub(w.finished_at()) >= 4_000)
                        .unwrap_or(true);
                    if retry {
                        if let Some(prior) = workers.remove(&key) {
                            prior.stop();
                        }
                        workers.insert(key, worker::Worker::start(candidate, shared.clone()));
                    }
                }
            }
            next_scan = now + 2_000;
        }
        if shared.wait(300) {
            next_scan = 0;
        }
    }
    for worker in workers.values() {
        worker.stop();
    }
    // Shutdown collection is bounded. Native I/O always owns its storage until reaped.
    let deadline = std::time::Instant::now() + Duration::from_millis(1800);
    while workers.values().any(|w| !w.finished()) && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if let Some(xbox) = xbox {
        if xbox.is_finished() {
            let _ = xbox.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_reset_does_not_change_devices() {
        let m = Monitor {
            shared: Shared::new(),
            thread: None,
        };
        m.shared.reports.store(7, Ordering::Relaxed);
        m.reset_diagnostics();
        assert_eq!(m.diagnostics().reports, 0);
    }
    #[test]
    fn stale_input_device_is_not_selected() {
        let m = Monitor {
            shared: Shared::new(),
            thread: None,
        };
        let mut input = ControllerInput::neutral();
        input.device_id = "old".into();
        m.shared.on_input("path", Some(input));
        assert!(m.input("new").is_none());
    }
}
