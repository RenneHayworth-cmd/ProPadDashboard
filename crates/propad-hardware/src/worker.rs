use super::*;
use native::{Candidate, Connection};
use std::io;

pub(crate) struct Worker {
    cancel: Arc<AtomicBool>,
    finished_at: Arc<AtomicU64>,
    thread: JoinHandle<()>,
}
impl Worker {
    pub fn start(candidate: Candidate, shared: Arc<Shared>) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let finished_at = Arc::new(AtomicU64::new(0));
        let generation = shared.generation.fetch_add(1, Ordering::Relaxed);
        shared
            .epochs
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(candidate.path.clone(), generation);
        let mut state = State {
            address: candidate.address.clone(),
            candidate,
            shared,
            cancel: cancel.clone(),
            generation,
            battery: None,
            received_input: false,
            usb_initialized: false,
            sequence: 0,
            stream_report_id: 0,
            last_stream_input: 0,
            mode_requested: 0,
        };
        let completed = finished_at.clone();
        let thread = thread::Builder::new()
            .name("hid-controller".into())
            .spawn(move || {
                let result = state.run();
                if let Err(error) = result {
                    // An actual transport failure releases held controls immediately;
                    // only the battery reading may remain stale during retry.
                    state.input(None);
                    let code = error.raw_os_error();
                    if !state.stopping()
                        && !matches!(code, Some(6 | 1167 | 995))
                        && (!state.address.is_empty() || state.received_input)
                    {
                        state.snapshot(
                            ConnectionState::Unavailable,
                            now_ms(),
                            Some("设备暂被占用或无法读取"),
                            state.battery.is_some(),
                        );
                        state.pause(5_000);
                    } else {
                        state.snapshot(ConnectionState::Disconnected, now_ms(), None, false);
                    }
                }
                state.input(None);
                // Unavailable remains observable until the next retry; explicit removals
                // are made by the supervisor, never by an obsolete worker generation.
                if state.stopping() {
                    state.snapshot(ConnectionState::Disconnected, now_ms(), None, false);
                }
                completed.store(now_ms(), Ordering::Release);
            })
            .expect("HID worker thread");
        Self {
            cancel,
            finished_at,
            thread,
        }
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn finished(&self) -> bool {
        self.thread.is_finished()
    }
    pub fn finished_at(&self) -> u64 {
        self.finished_at.load(Ordering::Acquire)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

struct State {
    candidate: Candidate,
    shared: Arc<Shared>,
    cancel: Arc<AtomicBool>,
    generation: u64,
    address: String,
    battery: Option<BatteryReading>,
    received_input: bool,
    usb_initialized: bool,
    sequence: u8,
    stream_report_id: u8,
    last_stream_input: u64,
    mode_requested: u64,
}
impl State {
    fn stopping(&self) -> bool {
        self.cancel.load(Ordering::Acquire) || self.shared.stop.load(Ordering::Acquire)
    }
    fn visible(&self) -> bool {
        self.shared.visible.load(Ordering::Acquire)
    }
    fn id(&self) -> String {
        device_id(
            self.candidate.family,
            if self.address.is_empty() {
                &self.candidate.path
            } else {
                &self.address
            },
        )
    }
    fn pause(&self, millis: u64) {
        let deadline = now_ms() + millis;
        while !self.stopping() && now_ms() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn input(&self, input: Option<ControllerInput>) {
        let epochs = self.shared.epochs.read().unwrap_or_else(|e| e.into_inner());
        if epochs.get(&self.candidate.path) == Some(&self.generation) {
            self.shared.on_input(&self.candidate.path, input);
        }
    }
    fn snapshot(
        &self,
        connection: ConnectionState,
        now: u64,
        detail: Option<&str>,
        is_stale: bool,
    ) {
        let epochs = self.shared.epochs.read().unwrap_or_else(|e| e.into_inner());
        if epochs.get(&self.candidate.path) != Some(&self.generation) {
            return;
        }
        // Superseded workers cannot resurrect an old device after resume/retry.
        if self.stopping() {
            return;
        }
        let snapshot = ControllerSnapshot {
            device_id: self.id(),
            transport: Some(self.candidate.transport),
            connection,
            battery: self.battery.clone(),
            timestamp: now,
            detail: detail.map(str::to_owned),
            is_stale,
            family: self.candidate.family,
            name: if self.candidate.product_id == 0x0df2 {
                Some("PS5 DualSense Edge".into())
            } else {
                None
            },
        };
        // Preserve the sampling timestamp when only the supervisor heartbeat changes.
        let previous = self
            .shared
            .snapshots
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&self.candidate.path)
            .cloned();
        let same = previous.as_ref().is_some_and(|p| {
            p.device_id == snapshot.device_id
                && p.transport == snapshot.transport
                && p.connection == snapshot.connection
                && p.battery == snapshot.battery
                && p.detail == snapshot.detail
                && p.is_stale == snapshot.is_stale
        });
        if !same {
            self.shared.set_snapshot(&self.candidate.path, snapshot);
        }
    }
    fn next_sequence(&mut self) -> u8 {
        let value = self.sequence;
        self.sequence = self.sequence.wrapping_add(1);
        value
    }
    fn run(&mut self) -> io::Result<()> {
        let mut connection = Connection::open(self.candidate.clone())?;
        if self.candidate.family == ControllerFamily::DualSense {
            // Reading feature 09 also enables enhanced Bluetooth input on DualSense.
            if let Some(feature) = connection.feature(0x09) {
                if let Some(address) = read_dualsense_address(&feature) {
                    self.address = address;
                }
            }
        }
        let started = now_ms();
        let next_probe = started + 500;
        let mut last_query = 0;
        while !self.stopping() {
            let report = connection.read(150)?;
            let now = now_ms();
            if let Some(report) = report {
                if !report.is_empty() {
                    self.observe(&report, now);
                }
            }
            if self.candidate.family == ControllerFamily::SwitchPro
                && self.candidate.transport == Transport::Usb
                && self.usb_initialized
                && self.visible()
                && connection.writable
                && self.stream_report_id == 0
                && now.saturating_sub(started) > 2_000
                && now.saturating_sub(self.mode_requested) > 10_000
            {
                let sequence = self.next_sequence();
                connection.send(&switch_subcommand_request(
                    self.candidate.output_length,
                    sequence,
                    0x03,
                    &[0x30],
                ))?;
                self.mode_requested = now;
            }
            let interval = if self.visible() { 5_000 } else { 15_000 };
            let needs_battery = self
                .battery
                .as_ref()
                .map(|b| now.saturating_sub(b.timestamp) >= interval)
                .unwrap_or(true);
            if self.candidate.family == ControllerFamily::SwitchPro
                && connection.writable
                && now >= next_probe
                && (needs_battery || self.address.is_empty())
                && now.saturating_sub(last_query) >= interval
            {
                if self.candidate.transport == Transport::Usb
                    && !self.usb_initialized
                    && self.battery.is_none()
                {
                    self.usb_exchange(&mut connection, 0x01, true)?;
                    if !self.stopping() && self.usb_exchange(&mut connection, 0x02, true)? {
                        self.usb_exchange(&mut connection, 0x03, true)?;
                        self.usb_exchange(&mut connection, 0x02, true)?;
                        self.usb_exchange(&mut connection, 0x04, false)?;
                        self.usb_initialized = true;
                    }
                }
                if !self.stopping() {
                    let sequence = self.next_sequence();
                    connection.send(&switch_device_info_request(
                        self.candidate.output_length,
                        sequence,
                    ))?;
                    last_query = now_ms();
                }
            }
            if !self.address.is_empty()
                || (self.candidate.family == ControllerFamily::DualSense && self.received_input)
            {
                let stale_after = if self.visible() { 12_000 } else { 35_000 };
                let stale = self
                    .battery
                    .as_ref()
                    .is_some_and(|b| now.saturating_sub(b.timestamp) > stale_after);
                let no_battery = self.candidate.family == ControllerFamily::SwitchPro
                    && self.battery.is_none()
                    && now.saturating_sub(started) > 4_000;
                if self.received_input
                    || (self.candidate.transport == Transport::Bluetooth
                        && native::bluetooth_connected(&self.address) == Some(true))
                {
                    let status = if stale || no_battery {
                        ConnectionState::Unavailable
                    } else if self.battery.is_none()
                        && self.candidate.family == ControllerFamily::SwitchPro
                    {
                        ConnectionState::Reading
                    } else {
                        ConnectionState::Connected
                    };
                    self.snapshot(
                        status,
                        now,
                        if stale || no_battery {
                            Some("设备未返回新的电量信息")
                        } else if self.candidate.family == ControllerFamily::DualSense
                            && self.battery.is_none()
                        {
                            Some("此连接未提供电量")
                        } else {
                            None
                        },
                        stale,
                    );
                }
            }
        }
        Ok(())
    }
    fn usb_exchange(
        &mut self,
        connection: &mut Connection,
        command: u8,
        wait: bool,
    ) -> io::Result<bool> {
        if self.stopping()
            || !connection.send(&switch_usb_command(command, self.candidate.output_length))?
        {
            return Ok(false);
        }
        if !wait {
            return Ok(true);
        }
        let deadline = now_ms() + 650;
        while !self.stopping() && now_ms() < deadline {
            if let Some(report) = connection.read(100)? {
                self.observe(&report, now_ms());
                if report.len() >= 2 && report[0] == 0x81 && report[1] == command {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    fn observe(&mut self, report: &[u8], now: u64) {
        if self.candidate.family == ControllerFamily::DualSense {
            if let Some(input) =
                decode_dualsense_input(report, &self.id(), self.candidate.transport, now)
            {
                self.received_input = true;
                if let Some(battery) = read_dualsense_battery(report, now) {
                    self.battery = Some(battery);
                }
                self.input(Some(input));
                // Publish once in the main loop, after calculating staleness.
                // A simple input packet must not briefly make an old battery fresh.
            }
            return;
        }
        if !matches!(report.first(), Some(0x21 | 0x30 | 0x31 | 0x3f | 0x81)) {
            return;
        }
        self.received_input = true;
        if let Some(identity) = read_switch_identity(report) {
            self.address = identity.address;
        }
        if matches!(report[0], 0x3f | 0x30 | 0x31) {
            self.stream_report_id = report[0];
            self.last_stream_input = now;
        }
        if self.address.is_empty() {
            return;
        }
        if let Some(battery) = read_switch_battery(report, now) {
            self.battery = Some(battery);
        }
        if report[0] == 0x21
            && (self.stream_report_id == 0x3f || now.saturating_sub(self.last_stream_input) < 250)
        {
            return;
        }
        if let Some(input) = decode_switch_input(report, &self.id(), self.candidate.transport, now)
        {
            self.input(Some(input));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> State {
        let shared = Shared::new();
        shared.epochs.write().unwrap().insert("test-path".into(), 1);
        State {
            candidate: Candidate {
                path: "test-path".into(),
                transport: Transport::Bluetooth,
                address: "C0FFEE010203".into(),
                input_length: 362,
                output_length: 49,
                feature_length: 64,
                family: ControllerFamily::SwitchPro,
                product_id: 0x2009,
            },
            shared,
            cancel: Arc::new(AtomicBool::new(false)),
            generation: 1,
            address: "C0FFEE010203".into(),
            battery: None,
            received_input: false,
            usb_initialized: false,
            sequence: 0,
            stream_report_id: 0,
            last_stream_input: 0,
            mode_requested: 0,
        }
    }
    #[test]
    fn slow_query_reply_does_not_replace_simple_input_pose() {
        let mut s = state();
        let mut simple = vec![0x3f, 2, 0, 8, 0, 128, 0, 128, 0, 128, 0, 128];
        simple[4] = 255;
        simple[5] = 255;
        s.observe(&simple, 1000);
        let mut reply = vec![0; 49];
        reply[0] = 0x21;
        reply[2] = 0x40;
        reply[13] = 0x80;
        s.observe(&reply, 2000);
        let inputs = s.shared.inputs.read().unwrap();
        let input = inputs.get("test-path").unwrap();
        assert_eq!(input.timestamp, 1000);
        assert_eq!(input.left_x, 1.0);
        assert_eq!(s.battery.as_ref().unwrap().percent(), 50);
    }
    #[test]
    fn obsolete_worker_cannot_clear_new_generation_input() {
        let s = state();
        let mut new_input = ControllerInput::neutral();
        new_input.buttons = buttons::A;
        s.shared.on_input("test-path", Some(new_input));
        s.shared
            .epochs
            .write()
            .unwrap()
            .insert("test-path".into(), 2);
        s.input(None);
        s.snapshot(
            ConnectionState::Unavailable,
            1000,
            Some("old worker"),
            false,
        );
        assert_eq!(
            s.shared
                .inputs
                .read()
                .unwrap()
                .get("test-path")
                .unwrap()
                .buttons,
            buttons::A
        );
        assert!(s.shared.snapshots.read().unwrap().is_empty());
    }
    #[test]
    fn invalid_report_does_not_create_battery_or_input() {
        let mut s = state();
        s.observe(&[0, 1, 2], 1000);
        assert!(s.battery.is_none());
        assert!(!s.received_input);
        assert!(s.shared.inputs.read().unwrap().is_empty());
    }
    #[test]
    fn request_sequence_wraps_without_panicking() {
        let mut s = state();
        s.sequence = 255;
        assert_eq!(s.next_sequence(), 255);
        assert_eq!(s.next_sequence(), 0);
    }
    #[test]
    fn simple_dualsense_input_does_not_refresh_old_battery() {
        let mut s = state();
        s.candidate.family = ControllerFamily::DualSense;
        let mut battery_report = vec![0; 13];
        battery_report[0] = 0x30;
        battery_report[2] = 0x40;
        s.battery = read_switch_battery(&battery_report, 1000);
        s.observe(&[1, 128, 128, 128, 128, 8, 0, 0, 0, 0], 50_000);
        assert!(s.received_input);
        assert_eq!(s.battery.as_ref().unwrap().timestamp, 1000);
        assert!(s.shared.snapshots.read().unwrap().is_empty());
    }
}
