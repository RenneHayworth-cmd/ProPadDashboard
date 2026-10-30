use super::*;
use std::ffi::c_void;
use std::io;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use windows_sys::core::GUID;
use windows_sys::Win32::Devices::Bluetooth::*;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::System::IO::*;

#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub path: String,
    pub transport: Transport,
    pub address: String,
    pub input_length: usize,
    pub output_length: usize,
    pub feature_length: usize,
    pub family: ControllerFamily,
    pub product_id: u16,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

struct OwnedHandle(HANDLE);
impl OwnedHandle {
    fn new(handle: HANDLE) -> io::Result<Self> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn path_hex(path: &str, token: &str) -> Option<u16> {
    let start = path.find(token)? + token.len();
    let value = path[start..]
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect::<String>();
    if value.len() < 4 {
        return None;
    }
    u16::from_str_radix(&value[value.len() - 4..], 16).ok()
}

pub(crate) fn enumerate() -> io::Result<Vec<Candidate>> {
    // All buffers passed to Win32 stay alive for the duration of each synchronous query.
    unsafe {
        let mut guid = GUID::default();
        HidD_GetHidGuid(&mut guid);
        let mut paths = Vec::new();
        for _ in 0..3 {
            let mut length = 0;
            let result = CM_Get_Device_Interface_List_SizeW(&mut length, &guid, null(), 0);
            if result != 0 || length > 1_048_576 {
                return Err(io::Error::other(format!(
                    "HID interface list size: {result}"
                )));
            }
            paths = vec![0u16; length as usize];
            let result =
                CM_Get_Device_Interface_ListW(&guid, null(), paths.as_mut_ptr(), length, 0);
            if result == 0 {
                break;
            }
            paths.clear();
            if result != 0x1a {
                return Err(io::Error::other(format!("HID interface list: {result}")));
            }
        }
        let mut candidates = Vec::new();
        for path in paths
            .split(|ch| *ch == 0)
            .filter(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
        {
            let lower = path.to_ascii_lowercase();
            let vendor = path_hex(&lower, "vid_").or_else(|| path_hex(&lower, "vid&"));
            let product = path_hex(&lower, "pid_").or_else(|| path_hex(&lower, "pid&"));
            let (Some(vendor), Some(product)) = (vendor, product) else {
                continue;
            };
            let Some(family) = identify_controller(vendor, product) else {
                continue;
            };
            if family == ControllerFamily::Xbox {
                continue;
            }
            let transport = if lower.contains("vid&") || lower.contains("bthenum") {
                Transport::Bluetooth
            } else {
                Transport::Usb
            };
            let mut candidate = Candidate {
                path: path.clone(),
                transport,
                address: String::new(),
                input_length: if family == ControllerFamily::DualSense {
                    if transport == Transport::Usb {
                        64
                    } else {
                        78
                    }
                } else if transport == Transport::Usb {
                    64
                } else {
                    362
                },
                output_length: if transport == Transport::Usb { 64 } else { 49 },
                feature_length: 64,
                family,
                product_id: product,
            };
            let mut gamepad = true;
            let path_wide = wide(&path);
            if let Ok(file) = OwnedHandle::new(CreateFileW(
                path_wide.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            )) {
                let mut serial = [0u16; 256];
                if HidD_GetSerialNumberString(
                    file.0,
                    serial.as_mut_ptr().cast(),
                    size_of::<[u16; 256]>() as u32,
                ) {
                    let end = serial.iter().position(|c| *c == 0).unwrap_or(serial.len());
                    candidate.address = address_from_hid_serial(
                        &String::from_utf16_lossy(&serial[..end]),
                        transport,
                    );
                }
                let mut preparsed = 0;
                if HidD_GetPreparsedData(file.0, &mut preparsed) {
                    let mut caps = HIDP_CAPS::default();
                    if HidP_GetCaps(preparsed, &mut caps) >= 0 {
                        for (target, value) in [
                            (&mut candidate.input_length, caps.InputReportByteLength),
                            (&mut candidate.output_length, caps.OutputReportByteLength),
                            (&mut candidate.feature_length, caps.FeatureReportByteLength),
                        ] {
                            if value > 0 && value <= 4096 {
                                *target = value as usize;
                            }
                        }
                        gamepad = caps.UsagePage == 1 && matches!(caps.Usage, 4 | 5);
                    }
                    HidD_FreePreparsedData(preparsed);
                }
            }
            if gamepad {
                candidates.push(candidate);
            }
        }
        Ok(candidates)
    }
}

pub(crate) fn bluetooth_connected(address: &str) -> Option<bool> {
    if !is_address(address) {
        return None;
    }
    let address = u64::from_str_radix(address, 16).ok()?;
    unsafe {
        let mut info = BLUETOOTH_DEVICE_INFO {
            dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
            Address: BLUETOOTH_ADDRESS {
                Anonymous: BLUETOOTH_ADDRESS_0 { ullLong: address },
            },
            ..Default::default()
        };
        if BluetoothGetDeviceInfo(null_mut(), &mut info) == 0 {
            Some(info.fConnected != 0)
        } else {
            None
        }
    }
}

pub(crate) struct DeviceNotifications {
    handle: HCMNOTIFICATION,
    // The Arc's allocation stays alive until CM_Unregister_Notification returns.
    context: Arc<Shared>,
}
impl DeviceNotifications {
    pub fn new(shared: Arc<Shared>) -> Self {
        let context = shared;
        let mut handle = null_mut();
        unsafe {
            let mut guid = GUID::default();
            HidD_GetHidGuid(&mut guid);
            let filter = CM_NOTIFY_FILTER {
                cbSize: size_of::<CM_NOTIFY_FILTER>() as u32,
                FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
                u: CM_NOTIFY_FILTER_0 {
                    DeviceInterface: CM_NOTIFY_FILTER_0_0 { ClassGuid: guid },
                },
                ..Default::default()
            };
            let result = CM_Register_Notification(
                &filter,
                Arc::as_ptr(&context).cast(),
                Some(notification),
                &mut handle,
            );
            if result != 0 {
                handle = null_mut();
            }
        }
        Self { handle, context }
    }
}
unsafe extern "system" fn notification(
    _: HCMNOTIFICATION,
    context: *const c_void,
    _: CM_NOTIFY_ACTION,
    _: *const CM_NOTIFY_EVENT_DATA,
    _: u32,
) -> u32 {
    if !context.is_null() {
        // No HID query or UI operation is allowed inside a Configuration Manager callback.
        let _ = std::panic::catch_unwind(|| (&*(context as *const Shared)).notify());
    }
    0
}
impl Drop for DeviceNotifications {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                if CM_Unregister_Notification(self.handle) != 0 {
                    // A failed unregistration must not leave a callback pointing
                    // at freed Rust storage. Keep one allocation alive on this
                    // exceptional OS-error path; the process reclaims it on exit.
                    let _ = Arc::into_raw(self.context.clone());
                }
            }
        }
        let _ = &self.context;
    }
}

/// Serial I/O owned by one worker. Every pending request is cancelled and reaped
/// before its OVERLAPPED or buffer can be reused or dropped.
pub(crate) struct Connection {
    file: OwnedHandle,
    event: OwnedHandle,
    overlapped: Box<OVERLAPPED>,
    buffer: Vec<u8>,
    pub writable: bool,
    pub candidate: Candidate,
}
impl Connection {
    pub fn open(candidate: Candidate) -> io::Result<Self> {
        unsafe {
            let path = wide(&candidate.path);
            let opened = OwnedHandle::new(CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            ));
            let writable = opened.is_ok();
            let file = match opened {
                Ok(file) => file,
                Err(_) => OwnedHandle::new(CreateFileW(
                    path.as_ptr(),
                    GENERIC_READ,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    null_mut(),
                ))?,
            };
            let event = OwnedHandle::new(CreateEventW(null(), 1, 0, null()))?;
            let capacity = 512.max(candidate.input_length).max(candidate.output_length);
            Ok(Self {
                file,
                event,
                overlapped: Box::new(zeroed()),
                buffer: vec![0; capacity],
                writable,
                candidate,
            })
        }
    }
    pub fn read(&mut self, timeout: u32) -> io::Result<Option<Vec<u8>>> {
        let count = self.transfer(None, self.candidate.input_length, timeout)?;
        Ok(if count == 0 {
            None
        } else {
            Some(self.buffer[..count].to_vec())
        })
    }
    pub fn send(&mut self, report: &[u8]) -> io::Result<bool> {
        if !self.writable {
            return Ok(false);
        }
        Ok(self.transfer(Some(report), report.len(), 650)? == report.len())
    }
    pub fn feature(&mut self, id: u8) -> Option<Vec<u8>> {
        let mut data = vec![0; self.candidate.feature_length.clamp(64, 4096)];
        data[0] = id;
        unsafe {
            if HidD_GetFeature(self.file.0, data.as_mut_ptr().cast(), data.len() as u32) {
                Some(data)
            } else {
                None
            }
        }
    }
    fn transfer(
        &mut self,
        output: Option<&[u8]>,
        length: usize,
        timeout: u32,
    ) -> io::Result<usize> {
        if length > self.buffer.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "HID transfer exceeds capability buffer",
            ));
        }
        unsafe {
            ResetEvent(self.event.0);
            *self.overlapped = zeroed();
            self.overlapped.hEvent = self.event.0;
            if let Some(output) = output {
                self.buffer[..length].copy_from_slice(output);
            }
            let mut count = 0;
            let ok = if output.is_some() {
                WriteFile(
                    self.file.0,
                    self.buffer.as_ptr(),
                    length as u32,
                    &mut count,
                    &mut *self.overlapped,
                )
            } else {
                ReadFile(
                    self.file.0,
                    self.buffer.as_mut_ptr(),
                    length as u32,
                    &mut count,
                    &mut *self.overlapped,
                )
            };
            if ok == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                    return Err(error);
                }
                let wait = WaitForSingleObject(self.event.0, timeout);
                if wait != WAIT_OBJECT_0 {
                    // CancelIoEx can race with completion; GetOverlappedResult(TRUE) is
                    // still required in both cases before freeing memory.
                    CancelIoEx(self.file.0, &*self.overlapped);
                    GetOverlappedResult(self.file.0, &*self.overlapped, &mut count, 1);
                    if wait == WAIT_TIMEOUT {
                        return Ok(0);
                    }
                    return Err(io::Error::last_os_error());
                }
                if GetOverlappedResult(self.file.0, &*self.overlapped, &mut count, 0) == 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            if count as usize > length {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "HID returned oversized report",
                ));
            }
            Ok(count as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vendor_ids_cover_bluetooth_and_usb_paths() {
        assert_eq!(path_hex("hid#vid&0002057e_pid&2009", "vid&"), Some(0x057e));
        assert_eq!(path_hex("hid#vid_054c&pid_0ce6", "pid_"), Some(0x0ce6));
        assert_eq!(path_hex("hid#vid_x", "vid_"), None);
    }
    #[test]
    fn native_filter_layout_matches_sdk() {
        assert_eq!(size_of::<CM_NOTIFY_FILTER>(), 416);
    }
}
