use super::*;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};
use windows_sys::Win32::UI::Input::XboxController::*;

type ReadState = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;

/// Optional ordinal 100 includes Guide on supported Windows versions.
/// Loading is restricted to System32; public XInput is always the fallback.
struct StateReader {
    module: windows_sys::Win32::Foundation::HMODULE,
    extended: Option<ReadState>,
}
impl StateReader {
    fn new() -> Self {
        unsafe {
            let filename = "xinput1_4.dll\0".encode_utf16().collect::<Vec<_>>();
            let module =
                LoadLibraryExW(filename.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
            let address = if module.is_null() {
                None
            } else {
                GetProcAddress(module, 100usize as *const u8)
            };
            let extended = address
                .map(|p| std::mem::transmute::<unsafe extern "system" fn() -> isize, ReadState>(p));
            Self { module, extended }
        }
    }
    fn read(&self, index: u32, state: &mut XINPUT_STATE) -> u32 {
        unsafe {
            if let Some(extended) = self.extended {
                extended(index, state)
            } else {
                XInputGetState(index, state)
            }
        }
    }
}
impl Drop for StateReader {
    fn drop(&mut self) {
        if !self.module.is_null() {
            unsafe {
                FreeLibrary(self.module);
            }
        }
    }
}

pub(crate) fn run(shared: Arc<Shared>) {
    let reader = StateReader::new();
    let mut battery_at = [0u64; 4];
    let mut next_probe = [0u64; 4];
    let mut transports = [Transport::Usb; 4];
    let mut packets = [None; 4];
    let mut previous_buttons = [0u16; 4];
    while !shared.stop.load(Ordering::Acquire) {
        let now = now_ms();
        for slot in 0..4 {
            if packets[slot].is_none() && now < next_probe[slot] {
                continue;
            }
            let key = format!("xinput:{slot}");
            let mut state = XINPUT_STATE::default();
            if reader.read(slot as u32, &mut state) != 0 {
                shared.remove(&key);
                packets[slot] = None;
                next_probe[slot] = now + 1_000;
                continue;
            }
            let fresh = packets[slot].is_none();
            if fresh {
                let mut capabilities = XINPUT_CAPABILITIES::default();
                let wireless = unsafe {
                    XInputGetCapabilities(slot as u32, 0, &mut capabilities) == 0
                        && capabilities.Flags & 2 != 0
                };
                transports[slot] = if wireless {
                    Transport::Wireless
                } else {
                    Transport::Usb
                };
            }
            let interval = if shared.visible.load(Ordering::Acquire) {
                5_000
            } else {
                15_000
            };
            if fresh || now.saturating_sub(battery_at[slot]) >= interval {
                let mut info = XINPUT_BATTERY_INFORMATION::default();
                let status = unsafe { XInputGetBatteryInformation(slot as u32, 0, &mut info) };
                let previous = shared
                    .snapshots
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&key)
                    .cloned();
                let mut battery = if status == 0 {
                    read_xbox_battery(info.BatteryType, info.BatteryLevel, now)
                } else {
                    None
                };
                let stale =
                    status != 0 && previous.as_ref().and_then(|s| s.battery.as_ref()).is_some();
                if stale {
                    battery = previous.and_then(|s| s.battery);
                }
                if battery.as_ref().is_some_and(|b| !b.external_power) {
                    transports[slot] = Transport::Wireless;
                }
                if status == 0 && battery.as_ref().is_some_and(|b| b.external_power) {
                    transports[slot] = Transport::Usb;
                }
                let detail = if status != 0 {
                    Some("电量暂不可读".into())
                } else if battery.is_none() {
                    Some("此连接未提供电量".into())
                } else {
                    None
                };
                shared.set_snapshot(
                    &key,
                    ControllerSnapshot {
                        device_id: key.clone(),
                        transport: Some(transports[slot]),
                        connection: if status != 0 {
                            ConnectionState::Unavailable
                        } else {
                            ConnectionState::Connected
                        },
                        battery,
                        timestamp: now,
                        detail,
                        is_stale: stale,
                        family: ControllerFamily::Xbox,
                        name: Some(format!("Xbox Controller · {}", slot + 1)),
                    },
                );
                battery_at[slot] = now;
            }
            // Some implementations change Guide without advancing dwPacketNumber.
            if packets[slot] != Some(state.dwPacketNumber)
                || previous_buttons[slot] != state.Gamepad.wButtons
            {
                let p = state.Gamepad;
                let input = decode_xbox_input(
                    p.wButtons,
                    p.bLeftTrigger,
                    p.bRightTrigger,
                    p.sThumbLX,
                    p.sThumbLY,
                    p.sThumbRX,
                    p.sThumbRY,
                    &key,
                    transports[slot],
                    now,
                );
                shared.on_input(&key, Some(input));
                packets[slot] = Some(state.dwPacketNumber);
                previous_buttons[slot] = p.wButtons;
            }
        }
        thread::sleep(Duration::from_millis(
            if shared.visible.load(Ordering::Acquire) {
                16
            } else {
                120
            },
        ));
    }
    for slot in 0..4 {
        shared.remove(&format!("xinput:{slot}"));
    }
}
