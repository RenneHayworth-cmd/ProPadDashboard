use std::{
    ffi::OsStr,
    io::{Read, Write},
    os::windows::{ffi::OsStrExt, io::FromRawHandle},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{Pipes::*, Power::*, Registry::*, Threading::*},
    UI::WindowsAndMessaging::*,
};

pub fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
pub fn sid() -> String {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return std::env::var("USERNAME").unwrap_or_else(|_| "current-user".into());
        }
        let mut needed = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            CloseHandle(token);
            return "current-user".into();
        }
        let user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let mut text = std::ptr::null_mut();
        let success = ConvertSidToStringSidW(user.User.Sid, &mut text);
        CloseHandle(token);
        if success == 0 {
            return "current-user".into();
        }
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        value
    }
}
pub fn pipe_name(demo: bool) -> String {
    format!(
        r"\\.\pipe\ProPadDashboard.Tauri.{}{}",
        sid(),
        if demo { ".Demo" } else { "" }
    )
}

const MAX_RESPONSE: usize = 65536;
const MAX_REQUEST: usize = 1024;
fn cancelled(stop: Option<&AtomicBool>) -> bool {
    stop.is_some_and(|flag| flag.load(Ordering::Acquire))
}
fn retryable(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
        || matches!(error.raw_os_error(),Some(value) if value==ERROR_NO_DATA as i32 || value==ERROR_PIPE_LISTENING as i32)
}
fn read_line_deadline(
    reader: &mut impl Read,
    limit: usize,
    deadline: Instant,
    stop: Option<&AtomicBool>,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    while Instant::now() < deadline && !cancelled(stop) {
        let mut chunk = [0u8; 4096];
        match reader.read(&mut chunk) {
            Ok(0) => std::thread::sleep(Duration::from_millis(5)),
            Ok(count) => {
                if let Some(end) = chunk[..count].iter().position(|&value| value == b'\n') {
                    if bytes.len() + end > limit {
                        return Err("本机通信数据过长".into());
                    }
                    bytes.extend_from_slice(&chunk[..end]);
                    return Ok(bytes);
                }
                if bytes.len() + count > limit {
                    return Err("本机通信数据过长".into());
                }
                bytes.extend_from_slice(&chunk[..count]);
            }
            Err(error) if retryable(&error) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => return Err(format!("本机通信读取失败：{error}")),
        }
    }
    Err(if cancelled(stop) {
        "本机通信已停止"
    } else {
        "等待本机通信响应超时"
    }
    .into())
}
fn write_deadline(
    writer: &mut impl Write,
    mut bytes: &[u8],
    deadline: Instant,
    stop: Option<&AtomicBool>,
) -> Result<(), String> {
    while !bytes.is_empty() && Instant::now() < deadline && !cancelled(stop) {
        match writer.write(bytes) {
            Ok(0) => std::thread::sleep(Duration::from_millis(5)),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if retryable(&error) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => return Err(format!("本机通信写入失败：{error}")),
        }
    }
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(if cancelled(stop) {
            "本机通信已停止"
        } else {
            "本机通信写入超时"
        }
        .into())
    }
}

pub fn send(command: &str, demo: bool) -> Result<String, String> {
    unsafe {
        if command.len() > MAX_REQUEST || command.contains(['\r', '\n', '\0']) {
            return Err("无效的本机通信命令".into());
        }
        let name = wide(pipe_name(demo));
        let connect_deadline = Instant::now() + Duration::from_millis(1500);
        let handle = loop {
            let candidate = CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            );
            if candidate != INVALID_HANDLE_VALUE {
                break candidate;
            }
            let error = GetLastError();
            if !matches!(error, ERROR_PIPE_BUSY | ERROR_FILE_NOT_FOUND) {
                return Err(std::io::Error::from_raw_os_error(error as i32).to_string());
            }
            if Instant::now() >= connect_deadline {
                return Err("程序尚未运行或本机通信暂时繁忙".into());
            }
            // A missing named pipe makes WaitNamedPipe return immediately; retain a bounded backoff.
            WaitNamedPipeW(name.as_ptr(), 40);
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut file = std::fs::File::from_raw_handle(handle);
        // Both directions must be nonblocking: WaitNamedPipe bounds connection only, not ReadFile.
        let mode = PIPE_READMODE_BYTE | PIPE_NOWAIT;
        if SetNamedPipeHandleState(handle, &mode, std::ptr::null(), std::ptr::null()) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        write_deadline(&mut file, format!("{command}\n").as_bytes(), deadline, None)?;
        let response = read_line_deadline(&mut file, MAX_RESPONSE, deadline, None)?;
        // The server disconnects only after this acknowledgement or its short deadline.
        let _ = write_deadline(
            &mut file,
            b"ack\n",
            Instant::now() + Duration::from_millis(100),
            None,
        );
        String::from_utf8(response)
            .map(|value| value.trim_end_matches('\r').to_owned())
            .map_err(|_| "本机通信返回了无效文本".into())
    }
}
pub fn serve(demo: bool, stop: Arc<AtomicBool>, handler: impl Fn(&str) -> String + Send + 'static) {
    std::thread::Builder::new()
        .name("local-diagnostics".into())
        .spawn(move || unsafe {
            let sddl = wide(format!("D:P(A;;GA;;;{})(A;;GA;;;SY)", sid()));
            let mut descriptor = std::ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            ) == 0
            {
                return;
            }
            let security = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            let handle = CreateNamedPipeW(
                wide(pipe_name(demo)).as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65536,
                1024,
                500,
                &security,
            );
            LocalFree(descriptor);
            if handle == INVALID_HANDLE_VALUE {
                return;
            }
            let mut file = std::fs::File::from_raw_handle(handle);
            while !stop.load(Ordering::Acquire) {
                let connected = ConnectNamedPipe(handle, std::ptr::null_mut()) != 0
                    || GetLastError() == ERROR_PIPE_CONNECTED;
                if !connected {
                    std::thread::sleep(Duration::from_millis(40));
                    continue;
                }
                if let Ok(request) = read_line_deadline(
                    &mut file,
                    MAX_REQUEST,
                    Instant::now() + Duration::from_secs(2),
                    Some(&stop),
                ) {
                    if let Ok(command) = std::str::from_utf8(&request) {
                        let response = handler(command.trim());
                        let response = if response.len() > MAX_RESPONSE || response.contains('\n') {
                            r#"{"error":"本机通信响应过长或格式无效"}"#.to_owned()
                        } else {
                            response
                        };
                        if write_deadline(
                            &mut file,
                            format!("{response}\n").as_bytes(),
                            Instant::now() + Duration::from_secs(2),
                            Some(&stop),
                        )
                        .is_ok()
                        {
                            // PeekNamedPipe reports inbound bytes, never reply consumption. The client
                            // explicitly acknowledges a complete line; old clients simply time out.
                            let _ = read_line_deadline(
                                &mut file,
                                16,
                                Instant::now() + Duration::from_millis(500),
                                Some(&stop),
                            );
                        }
                    }
                }
                DisconnectNamedPipe(handle);
            }
        })
        .expect("local diagnostics thread");
}
pub fn startup_registered() -> bool {
    unsafe {
        let mut data = [0u16; 2048];
        let mut bytes = std::mem::size_of_val(&data) as u32;
        if RegGetValueW(
            HKEY_CURRENT_USER,
            wide(r"Software\Microsoft\Windows\CurrentVersion\Run").as_ptr(),
            wide("ProPadDashboard").as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut bytes,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let end = data
            .iter()
            .position(|&value| value == 0)
            .unwrap_or(data.len());
        let command = String::from_utf16_lossy(&data[..end]);
        let executable = if let Some(quoted) = command.trim().strip_prefix('"') {
            quoted.split('"').next().unwrap_or("")
        } else {
            command.split_ascii_whitespace().next().unwrap_or("")
        };
        let Some(local) = std::env::var_os("LOCALAPPDATA") else {
            return false;
        };
        let expected = std::path::PathBuf::from(local)
            .join("ProPadDashboard")
            .join("ProPadDashboard.exe");
        let actual = executable.replace('/', "\\");
        actual.eq_ignore_ascii_case(&expected.to_string_lossy())
            || std::env::current_exe()
                .map(|path| actual.eq_ignore_ascii_case(&path.to_string_lossy()))
                .unwrap_or(false)
    }
}
thread_local! { static NONACTIVATING_TARGET: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
struct ActivationGuard {
    hook: HHOOK,
    previous: usize,
}
impl ActivationGuard {
    unsafe fn new(hwnd: HWND) -> Self {
        unsafe extern "system" fn filter(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
            let blocked = NONACTIVATING_TARGET.with(|target| {
                let target = target.get();
                target != 0
                    && (code == HCBT_ACTIVATE as i32 || code == HCBT_SETFOCUS as i32)
                    && (wparam == target
                        || (!((wparam as HWND).is_null())
                            && GetAncestor(wparam as HWND, GA_ROOT) as usize == target))
            });
            if blocked {
                1
            } else {
                CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
            }
        }
        let previous = NONACTIVATING_TARGET.with(|target| target.replace(hwnd as usize));
        let hook = SetWindowsHookExW(
            WH_CBT,
            Some(filter),
            std::ptr::null_mut(),
            GetCurrentThreadId(),
        );
        Self { hook, previous }
    }
    fn active(&self) -> bool {
        !self.hook.is_null()
    }
}
impl Drop for ActivationGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.hook.is_null() {
                UnhookWindowsHookEx(self.hook);
            }
        }
        NONACTIVATING_TARGET.with(|target| target.set(self.previous));
    }
}

// Tauri/Tao's visibility cache must agree with Win32. Tao also calls SW_SHOW
// during geometry changes, so guard just these programmatic UI operations;
// user clicks and keyboard focus remain available after the guard is dropped.
pub fn geometry_without_focus(
    window: &tauri::WebviewWindow,
    position: Option<tauri::PhysicalPosition<i32>>,
    size: tauri::PhysicalSize<u32>,
) {
    let window = window.clone();
    let executor = window.clone();
    let _ = executor.run_on_main_thread(move || unsafe {
        if let Ok(raw) = window.hwnd() {
            let hwnd = raw.0 as HWND;
            let guard = ActivationGuard::new(hwnd);
            if guard.active() {
                let _ = window.set_size(tauri::Size::Physical(size));
                if let Some(position) = position {
                    let _ = window.set_position(position);
                }
            } else {
                let (x, y, flags) = position
                    .map(|p| (p.x, p.y, SWP_NOACTIVATE | SWP_NOZORDER))
                    .unwrap_or((0, 0, SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOZORDER));
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    x,
                    y,
                    size.width as i32,
                    size.height as i32,
                    flags,
                );
            }
        }
    });
}
pub fn show_without_focus(window: &tauri::WebviewWindow, preserved: Arc<AtomicBool>) {
    preserved.store(false, Ordering::Release);
    if let Ok(raw) = window.hwnd() {
        let handle = raw.0 as usize;
        let managed_window = window.clone();
        let _ = window.run_on_main_thread(move || unsafe {
            let foreground = GetForegroundWindow();
            let hwnd = handle as HWND;
            let guard = ActivationGuard::new(hwnd);
            if guard.active() {
                let _ = managed_window.show();
            }
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            preserved.store(foreground == GetForegroundWindow(), Ordering::Release);
        });
    }
}

pub struct PowerRegistration {
    handle: usize,
    _params: Box<DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>,
    _context: Arc<AtomicBool>,
}
// The OS callback only accesses an Arc<AtomicBool>; registration is stable until unregistered.
unsafe impl Send for PowerRegistration {}
unsafe impl Sync for PowerRegistration {}
impl PowerRegistration {
    pub fn new(flag: Arc<AtomicBool>) -> Option<Self> {
        unsafe {
            unsafe extern "system" fn callback(
                context: *const std::ffi::c_void,
                event: u32,
                _setting: *const std::ffi::c_void,
            ) -> u32 {
                if matches!(
                    event,
                    PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND | PBT_APMRESUMECRITICAL
                ) && !context.is_null()
                {
                    let flag = &*(context as *const AtomicBool);
                    flag.store(true, Ordering::Release);
                }
                0
            }
            let context = flag;
            let params = Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
                Callback: Some(callback),
                Context: Arc::as_ptr(&context).cast_mut().cast(),
            });
            let mut handle = std::ptr::null_mut();
            if PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                (&*params as *const DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS)
                    .cast_mut()
                    .cast(),
                &mut handle,
            ) != 0
            {
                return None;
            }
            Some(Self {
                handle: handle as usize,
                _params: params,
                _context: context,
            })
        }
    }
}
impl Drop for PowerRegistration {
    fn drop(&mut self) {
        unsafe {
            PowerUnregisterSuspendResumeNotification(self.handle as HPOWERNOTIFY);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct PartialReader(Cursor<Vec<u8>>);
    impl Read for PartialReader {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let length = bytes.len().min(3);
            self.0.read(&mut bytes[..length])
        }
    }
    struct PartialWriter {
        bytes: Vec<u8>,
        blocked: bool,
    }
    impl Write for PartialWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if !self.blocked {
                self.blocked = true;
                return Err(std::io::ErrorKind::WouldBlock.into());
            }
            let length = bytes.len().min(3);
            self.bytes.extend_from_slice(&bytes[..length]);
            Ok(length)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn diagnostic_reads_all_chunks_until_newline() {
        let mut reader = PartialReader(Cursor::new(
            b"{\"sequence\":123,\"device\":\"abc\"}\n".to_vec(),
        ));
        let value = read_line_deadline(
            &mut reader,
            MAX_RESPONSE,
            Instant::now() + Duration::from_millis(100),
            None,
        )
        .unwrap();
        assert_eq!(value, b"{\"sequence\":123,\"device\":\"abc\"}");
    }
    #[test]
    fn diagnostic_writes_partial_chunks_after_would_block() {
        let mut writer = PartialWriter {
            bytes: Vec::new(),
            blocked: false,
        };
        write_deadline(
            &mut writer,
            b"complete response\n",
            Instant::now() + Duration::from_millis(100),
            None,
        )
        .unwrap();
        assert_eq!(writer.bytes, b"complete response\n");
    }
    #[test]
    fn diagnostic_caps_response_size() {
        let mut reader = Cursor::new(b"oversized response\n".to_vec());
        assert!(read_line_deadline(
            &mut reader,
            4,
            Instant::now() + Duration::from_millis(100),
            None
        )
        .is_err());
    }
    #[test]
    fn diagnostic_incomplete_line_has_bounded_timeout() {
        let started = Instant::now();
        let mut reader = Cursor::new(b"truncated".to_vec());
        assert!(read_line_deadline(
            &mut reader,
            MAX_RESPONSE,
            started + Duration::from_millis(15),
            None
        )
        .is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn diagnostic_shutdown_cancels_read_and_write() {
        let stop = AtomicBool::new(true);
        let deadline = Instant::now() + Duration::from_secs(5);
        assert!(read_line_deadline(
            &mut Cursor::new(Vec::new()),
            MAX_RESPONSE,
            deadline,
            Some(&stop)
        )
        .is_err());
        assert!(write_deadline(&mut Vec::new(), b"response", deadline, Some(&stop)).is_err());
    }
}
