use crate::{
    native,
    settings::{self, Settings},
};
use propad_core::*;
use propad_hardware::{now_ms, Monitor};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_autostart::ManagerExt;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardState {
    snapshot: ControllerSnapshot,
    devices: Vec<ControllerSnapshot>,
    preferred_device_id: String,
    autostart: bool,
    reduced_motion: bool,
    expanded: bool,
    visible: bool,
    sequence: u64,
}
struct Inner {
    settings: Settings,
    state: DashboardState,
    session: VisibilitySession,
    connected_at: HashMap<String, u64>,
    present: HashSet<String>,
    last_seen: HashMap<String, u64>,
    position_dirty: Option<Instant>,
}
pub struct Coordinator {
    inner: Mutex<Inner>,
    monitor: Option<Monitor>,
    directory: PathBuf,
    demo: Option<String>,
    demo_state: String,
    stop: Arc<AtomicBool>,
    ready: AtomicBool,
    focus_preserved: Arc<AtomicBool>,
    resume_requested: Arc<AtomicBool>,
    resizing: AtomicBool,
    input_sequence: AtomicU64,
    power: Mutex<Option<native::PowerRegistration>>,
    startup_managed: bool,
}
impl Coordinator {
    fn state(&self) -> DashboardState {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .state
            .clone()
    }
    fn save(&self, inner: &mut Inner) {
        if let Err(error) = inner.settings.save(&self.directory) {
            settings::log(&self.directory, &format!("保存设置失败: {error}"));
        }
    }
    fn emit(&self, app: &tauri::AppHandle) {
        let state = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.state.sequence += 1;
            inner.state.clone()
        };
        let _ = app.emit("dashboard-state", state);
    }
    fn show(&self, app: &tauri::AppHandle) {
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.session.show_manually();
            inner.state.visible = true;
        }
        self.resize(app, false);
        if let Some(window) = app.get_webview_window("main") {
            native::show_without_focus(&window, self.focus_preserved.clone());
        }
        self.emit(app);
    }
    fn hide(&self, app: &tauri::AppHandle) {
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.session.hide_manually();
            inner.state.visible = false;
        }
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
        self.resize(app, false);
        self.emit(app);
    }
    fn select(&self, app: &tauri::AppHandle, id: String) -> Result<(), String> {
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !id.is_empty() && !inner.state.devices.iter().any(|s| s.device_id == id) {
                return Err("这只手柄已断开，请重新选择".into());
            }
            inner.settings.preferred_device_id = id.clone();
            inner.state.preferred_device_id = id;
            self.save(&mut inner);
        }
        if let Some(monitor) = &self.monitor {
            monitor.refresh();
        }
        self.refresh(app);
        Ok(())
    }
    fn resize(&self, app: &tauri::AppHandle, expanded: bool) {
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        let settings = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.state.expanded = expanded;
            inner.settings.clone()
        };
        self.resizing.store(true, Ordering::Release);
        let scale = window.scale_factor().unwrap_or(1.0);
        let compact = tauri::PhysicalSize::new(
            (360.0 * scale).round() as u32,
            (288.0 * scale).round() as u32,
        );
        let size = tauri::PhysicalSize::new(
            ((if expanded { 440.0 } else { 360.0 }) * scale).round() as u32,
            ((if expanded { 460.0 } else { 288.0 }) * scale).round() as u32,
        );
        let monitors = window.available_monitors().unwrap_or_default();
        let preferred = settings.left.zip(settings.top);
        let monitor = preferred
            .and_then(|(x, y)| {
                monitors
                    .iter()
                    .find(|m| {
                        let r = m.work_area();
                        x >= r.position.x
                            && y >= r.position.y
                            && x < r.position.x + r.size.width as i32
                            && y < r.position.y + r.size.height as i32
                    })
                    .cloned()
            })
            .or_else(|| window.primary_monitor().ok().flatten());
        if let Some(monitor) = monitor {
            let area = monitor.work_area();
            let padding = (20.0 * monitor.scale_factor()) as i32;
            let (x, y) = preferred.unwrap_or((
                area.position.x + area.size.width as i32 - compact.width as i32 - padding,
                area.position.y + area.size.height as i32 - compact.height as i32 - padding,
            ));
            let mut x = x - (size.width as i32 - compact.width as i32);
            let mut y = y - (size.height as i32 - compact.height as i32);
            x = x.clamp(
                area.position.x,
                (area.position.x + area.size.width as i32 - size.width as i32).max(area.position.x),
            );
            y = y.clamp(
                area.position.y,
                (area.position.y + area.size.height as i32 - size.height as i32)
                    .max(area.position.y),
            );
            native::geometry_without_focus(&window, Some(tauri::PhysicalPosition::new(x, y)), size);
        } else {
            native::geometry_without_focus(&window, None, size);
        }
        self.resizing.store(false, Ordering::Release);
    }
    fn refresh(&self, app: &tauri::AppHandle) {
        let now = now_ms();
        if self.resume_requested.swap(false, Ordering::AcqRel) {
            if let Some(monitor) = &self.monitor {
                monitor.resume();
            }
            self.resize(app, self.state().expanded);
            settings::log(&self.directory, "系统恢复，重新检测手柄");
        }
        let devices = if let Some(family) = &self.demo {
            vec![demo_snapshot(family, &self.demo_state, now)]
        } else {
            self.monitor
                .as_ref()
                .map(Monitor::devices)
                .unwrap_or_default()
        };
        let (auto_show, visible) = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let present = devices
                .iter()
                .map(|s| s.device_id.clone())
                .collect::<HashSet<_>>();
            for id in &present {
                if !inner
                    .last_seen
                    .get(id)
                    .is_some_and(|last| now.saturating_sub(*last) < 3_000)
                {
                    inner.connected_at.insert(id.clone(), now);
                }
                inner.last_seen.insert(id.clone(), now);
            }
            inner.present = present;
            inner.state.devices = devices.clone();
            // Keep the current logical device during a short transport handover,
            // even when another controller remains connected in the background.
            let current_id = &inner.state.snapshot.device_id;
            let explicit_target = devices
                .iter()
                .any(|s| s.device_id == inner.settings.preferred_device_id);
            let handover = inner.session.has_session
                && !current_id.is_empty()
                && !inner.present.contains(current_id)
                && now.saturating_sub(inner.session.missing_since.unwrap_or(now)) < 3_000
                && !explicit_target;
            let selected = if handover {
                None
            } else {
                choose_snapshot(
                    &devices,
                    &inner.settings.preferred_device_id,
                    &inner.connected_at,
                )
            };
            let mut auto_show = false;
            if let Some(snapshot) = selected {
                if inner.state.snapshot.device_id != snapshot.device_id {
                    inner.session.reset_for_device();
                }
                auto_show = inner.session.observe(true, now);
                inner.state.snapshot = snapshot;
            } else {
                inner.session.observe(false, now);
                if !inner.session.has_session {
                    let old = inner.state.snapshot.clone();
                    inner.state.snapshot = ControllerSnapshot {
                        family: old.family,
                        name: old.name,
                        ..ControllerSnapshot::disconnected(now)
                    };
                }
            }
            inner.session.tick(now);
            inner.state.visible = inner.session.visible;
            if auto_show {
                inner.state.expanded = false;
            }
            if inner
                .position_dirty
                .map(|at| at.elapsed() >= Duration::from_millis(650))
                .unwrap_or(false)
            {
                inner.position_dirty = None;
                self.save(&mut inner);
            }
            (auto_show, inner.state.visible)
        };
        if let Some(monitor) = &self.monitor {
            monitor.set_visible(visible);
        }
        if self.ready.load(Ordering::Acquire) {
            if let Some(window) = app.get_webview_window("main") {
                let shown = window.is_visible().unwrap_or(false);
                if visible && !shown {
                    self.resize(app, false);
                    native::show_without_focus(&window, self.focus_preserved.clone());
                } else if !visible && shown {
                    let _ = window.hide();
                }
                if auto_show {
                    self.resize(app, false);
                }
            }
        }
        self.emit(app);
    }
    fn input(&self, state: &DashboardState) -> Option<ControllerInput> {
        if !state.visible || state.snapshot.connection == ConnectionState::Disconnected {
            return None;
        }
        if self.demo.is_some() {
            let mut input = ControllerInput::neutral();
            input.device_id = state.snapshot.device_id.clone();
            input.transport = state.snapshot.transport.unwrap_or(Transport::Bluetooth);
            input.timestamp = now_ms();
            if self.demo_state == "controls" {
                input.buttons = buttons::A | buttons::L | buttons::RIGHT | buttons::LEFT_STICK;
                input.left_x = 0.6;
                input.left_y = -0.45;
                input.right_y = 0.65;
            }
            return Some(input);
        }
        self.monitor
            .as_ref()
            .and_then(|m| m.input(&state.snapshot.device_id))
    }
    fn diagnostics(&self, app: &tauri::AppHandle) -> serde_json::Value {
        let state = self.state();
        let input = self
            .monitor
            .as_ref()
            .map(Monitor::diagnostics)
            .unwrap_or_default();
        let window = app.get_webview_window("main");
        serde_json::json!({"version":"3.0.0","runtime":"Tauri 2 / Rust","state":state,"reports":input.reports,"observedButtons":input.observed_buttons,"observedAxes":input.observed_axes,"lastShowPreservedFocus":self.focus_preserved.load(Ordering::Acquire),"windowVisible":window.as_ref().and_then(|w|w.is_visible().ok()),"windowPosition":window.as_ref().and_then(|w|w.outer_position().ok()),"windowSize":window.as_ref().and_then(|w|w.outer_size().ok()),"demo":self.demo.is_some()})
    }
}
#[tauri::command]
fn get_dashboard_state(state: tauri::State<'_, Arc<Coordinator>>) -> DashboardState {
    state.ready.store(true, Ordering::Release);
    state.state()
}
#[tauri::command]
fn select_controller(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Coordinator>>,
    device_id: String,
) -> Result<(), String> {
    state.select(&app, device_id)
}
#[tauri::command]
fn hide_panel(app: tauri::AppHandle, state: tauri::State<'_, Arc<Coordinator>>) {
    state.hide(&app)
}
#[tauri::command]
fn set_panel_mode(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Coordinator>>,
    expanded: bool,
) {
    state.resize(&app, expanded);
    state.emit(&app);
}
#[tauri::command]
fn set_reduced_motion(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Coordinator>>,
    enabled: bool,
) {
    let mut inner = state.inner.lock().unwrap_or_else(|e| e.into_inner());
    inner.settings.reduced_motion = enabled;
    inner.state.reduced_motion = enabled;
    state.save(&mut inner);
    drop(inner);
    state.emit(&app);
}
#[tauri::command]
fn set_autostart(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Coordinator>>,
    enabled: bool,
) -> Result<(), String> {
    if !state.startup_managed {
        return Err("模拟或检查模式不修改登录启动项".into());
    }
    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|e| e.to_string())?;
    let actual = manager.is_enabled().map_err(|e| e.to_string())?;
    let mut inner = state.inner.lock().unwrap_or_else(|e| e.into_inner());
    inner.state.autostart = actual;
    inner.settings.startup_enabled = Some(actual);
    state.save(&mut inner);
    drop(inner);
    state.emit(&app);
    Ok(())
}
fn tray_menu(app: &tauri::AppHandle, state: &DashboardState) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "toggle",
        if state.visible {
            "隐藏看板"
        } else {
            "显示看板"
        },
        true,
        None::<&str>,
    )?)?;
    let devices = Submenu::new(app, "选择手柄", true)?;
    devices.append(&CheckMenuItem::with_id(
        app,
        "select:auto",
        "自动选择最近连接",
        true,
        state.preferred_device_id.is_empty(),
        None::<&str>,
    )?)?;
    for device in &state.devices {
        let suffix = device
            .device_id
            .chars()
            .rev()
            .take(4)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        devices.append(&CheckMenuItem::with_id(
            app,
            format!("select:{}", device.device_id),
            format!("{} · {}", device.display_name(), suffix),
            true,
            state.preferred_device_id == device.device_id,
            None::<&str>,
        )?)?;
    }
    menu.append(&devices)?;
    menu.append(&CheckMenuItem::with_id(
        app,
        "startup",
        "登录时启动",
        true,
        state.autostart,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(app, "exit", "退出", true, None::<&str>)?)?;
    Ok(menu)
}
fn argument(name: &str) -> Option<String> {
    std::env::args()
        .find_map(|a| a.strip_prefix(&format!("{name}=")).map(str::to_owned))
        .or_else(|| {
            std::env::args()
                .skip_while(|a| a != name)
                .nth(1)
                .filter(|a| !a.starts_with("--"))
        })
}
fn demo_snapshot(family: &str, state: &str, now: u64) -> ControllerSnapshot {
    let family = match family {
        "xbox" => ControllerFamily::Xbox,
        "dualSense" | "ps5" => ControllerFamily::DualSense,
        _ => ControllerFamily::SwitchPro,
    };
    let mut s = ControllerSnapshot {
        device_id: format!("demo:{family:?}"),
        transport: Some(Transport::Bluetooth),
        connection: ConnectionState::Connected,
        battery: Some(BatteryReading::switch(6, false, false, now)),
        timestamp: now,
        detail: Some("模拟状态 · 未读取实体手柄".into()),
        is_stale: false,
        family,
        name: None,
    };
    match state {
        "charging" => {
            s.transport = Some(Transport::Usb);
            s.battery = Some(BatteryReading::switch(4, true, true, now));
        }
        "full" => {
            s.transport = Some(Transport::Usb);
            s.battery = Some(BatteryReading::switch(8, false, true, now));
        }
        "low" => s.battery = Some(BatteryReading::switch(2, false, false, now)),
        "unknown" => {
            s.battery = None;
            s.connection = ConnectionState::Unavailable;
        }
        "reading" => {
            s.battery = None;
            s.connection = ConnectionState::Reading;
        }
        "stale" => {
            s.is_stale = true;
            s.battery.as_mut().unwrap().timestamp = now.saturating_sub(45000);
        }
        "wired" => {
            s.transport = Some(Transport::Usb);
            s.battery = Some(BatteryReading {
                percentage_known: false,
                reported_power: Some(PowerState::ExternalPower),
                ..BatteryReading::switch(0, false, true, now)
            });
        }
        "disconnected" => {
            s.connection = ConnectionState::Disconnected;
            s.battery = None;
            s.transport = None;
        }
        _ => {}
    }
    s
}
pub fn run() {
    let is_demo = std::env::args().any(|a| a == "--demo" || a.starts_with("--demo="));
    if let Some(command) = argument("--command") {
        let result = native::send(&command, is_demo)
            .unwrap_or_else(|error| serde_json::json!({"error":error}).to_string());
        if let Some(output) = argument("--output") {
            let _ = std::fs::write(output, &result);
        } else {
            println!("{result}");
        }
        return;
    }
    let demo = if is_demo {
        Some(argument("--demo").unwrap_or_else(|| "switchPro".into()))
    } else {
        None
    };
    let mut context = tauri::generate_context!();
    if is_demo {
        context.config_mut().identifier.push_str(".demo");
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !args.iter().any(|a| a == "--background") {
                if let Some(state) = app.try_state::<Arc<Coordinator>>() {
                    state.show(app);
                }
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--background"])
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_dashboard_state,
            select_controller,
            set_autostart,
            set_reduced_motion,
            set_panel_mode,
            hide_panel
        ])
        .setup(move |app| {
            let directory = settings::directory(is_demo);
            let settings = Settings::load(&directory);
            let startup_managed = !is_demo && !std::env::args().any(|a| a == "--no-startup");
            let autostart = if startup_managed {
                native::startup_registered() && app.autolaunch().is_enabled().unwrap_or(false)
            } else {
                false
            };
            if autostart {
                let _ = app.autolaunch().enable();
            }
            let initial = DashboardState {
                snapshot: ControllerSnapshot::disconnected(now_ms()),
                devices: Vec::new(),
                preferred_device_id: settings.preferred_device_id.clone(),
                autostart,
                reduced_motion: settings.reduced_motion,
                expanded: false,
                visible: false,
                sequence: 0,
            };
            let state = Arc::new(Coordinator {
                inner: Mutex::new(Inner {
                    settings,
                    state: initial,
                    session: VisibilitySession::default(),
                    connected_at: HashMap::new(),
                    present: HashSet::new(),
                    last_seen: HashMap::new(),
                    position_dirty: None,
                }),
                monitor: if is_demo {
                    None
                } else {
                    Some(Monitor::start())
                },
                directory,
                demo: demo.clone(),
                demo_state: argument("--state").unwrap_or_else(|| "battery".into()),
                stop: Arc::new(AtomicBool::new(false)),
                ready: AtomicBool::new(false),
                focus_preserved: Arc::new(AtomicBool::new(false)),
                resume_requested: Arc::new(AtomicBool::new(false)),
                resizing: AtomicBool::new(false),
                input_sequence: AtomicU64::new(0),
                power: Mutex::new(None),
                startup_managed,
            });
            *state.power.lock().unwrap() =
                native::PowerRegistration::new(state.resume_requested.clone());
            app.manage(state.clone());
            state.resize(app.handle(), false);
            let window = app.get_webview_window("main").expect("main window");
            let handle = app.handle().clone();
            let events = state.clone();
            window.on_window_event(move |event| match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    events.hide(&handle);
                }
                tauri::WindowEvent::Moved(position) => {
                    if !events.resizing.load(Ordering::Acquire) {
                        let mut inner = events.inner.lock().unwrap_or_else(|e| e.into_inner());
                        if !inner.state.expanded && inner.state.visible {
                            inner.settings.left = Some(position.x);
                            inner.settings.top = Some(position.y);
                            inner.position_dirty = Some(Instant::now());
                        }
                    }
                }
                tauri::WindowEvent::ScaleFactorChanged { .. } => {
                    events.resize(&handle, events.state().expanded)
                }
                _ => {}
            });
            let menu = tray_menu(app.handle(), &state.state())?;
            TrayIconBuilder::with_id("controller")
                .icon(tauri::image::Image::from_bytes(include_bytes!(
                    "../icons/icon.png"
                ))?)
                .tooltip("手柄电量看板 · 等待连接")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        let state = app.state::<Arc<Coordinator>>();
                        if state.state().visible {
                            state.hide(app);
                        } else {
                            state.show(app);
                        }
                    }
                })
                .on_menu_event(|app, event| {
                    let state = app.state::<Arc<Coordinator>>();
                    let id = event.id().as_ref();
                    match id {
                        "toggle" => {
                            if state.state().visible {
                                state.hide(app)
                            } else {
                                state.show(app)
                            }
                        }
                        "startup" => {
                            let enabled = !state.state().autostart;
                            let _ = set_autostart(app.clone(), state, enabled);
                        }
                        "exit" => app.exit(0),
                        _ => {
                            if let Some(device) = id.strip_prefix("select:") {
                                let _ = state.select(
                                    app,
                                    if device == "auto" {
                                        String::new()
                                    } else {
                                        device.into()
                                    },
                                );
                            }
                        }
                    }
                })
                .build(app)?;
            let diagnostics = state.clone();
            let diagnostics_app = app.handle().clone();
            native::serve(is_demo, state.stop.clone(), move |command| match command {
                "status" | "devices" | "input-status" => {
                    diagnostics.diagnostics(&diagnostics_app).to_string()
                }
                "input-reset" => {
                    if let Some(monitor) = &diagnostics.monitor {
                        monitor.reset_diagnostics();
                    }
                    "{\"ok\":true}".into()
                }
                "show" => {
                    diagnostics.show(&diagnostics_app);
                    "{\"ok\":true}".into()
                }
                "hide" => {
                    diagnostics.hide(&diagnostics_app);
                    "{\"ok\":true}".into()
                }
                "expand" => {
                    diagnostics.resize(&diagnostics_app, true);
                    diagnostics.emit(&diagnostics_app);
                    "{\"ok\":true}".into()
                }
                "collapse" => {
                    diagnostics.resize(&diagnostics_app, false);
                    diagnostics.emit(&diagnostics_app);
                    "{\"ok\":true}".into()
                }
                "refresh" | "resume" => {
                    diagnostics.resume_requested.store(true, Ordering::Release);
                    "{\"ok\":true}".into()
                }
                "exit" => {
                    let app = diagnostics_app.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(250));
                        app.exit(0);
                    });
                    "{\"ok\":true}".into()
                }
                _ => {
                    if let Some(id) = command.strip_prefix("select:") {
                        match diagnostics.select(&diagnostics_app, id.into()) {
                            Ok(()) => "{\"ok\":true}".into(),
                            Err(error) => serde_json::json!({"error":error}).to_string(),
                        }
                    } else {
                        "{\"error\":\"未知指令\"}".into()
                    }
                }
            });
            let coordinator = state.clone();
            let worker_app = app.handle().clone();
            std::thread::Builder::new()
                .name("dashboard-coordinator".into())
                .spawn(move || {
                    let mut next_state = Instant::now();
                    let mut next_tray = Instant::now();
                    let mut last_input: Option<ControllerInput> = None;
                    let mut last_device = String::new();
                    let mut last_menu = String::new();
                    while !coordinator.stop.load(Ordering::Acquire) {
                        if Instant::now() >= next_state {
                            coordinator.refresh(&worker_app);
                            next_state = Instant::now() + Duration::from_millis(250);
                        }
                        let current = coordinator.state();
                        let input = coordinator.input(&current);
                        let changed = last_device != current.snapshot.device_id
                            || match (&last_input, &input) {
                                (Some(a), Some(b)) => !a.same_pose(b),
                                (None, None) => false,
                                _ => true,
                            };
                        if changed && current.visible && coordinator.ready.load(Ordering::Acquire) {
                            let sequence =
                                coordinator.input_sequence.fetch_add(1, Ordering::AcqRel) + 1;
                            let _ = worker_app.emit(
                                "controller-input",
                                serde_json::json!({"sequence":sequence,"input":input}),
                            );
                        }
                        last_input = if current.visible && coordinator.ready.load(Ordering::Acquire)
                        {
                            input
                        } else {
                            None
                        };
                        last_device = current.snapshot.device_id.clone();
                        if Instant::now() >= next_tray {
                            if let Some(tray) = worker_app.tray_by_id("controller") {
                                let _ = tray.set_tooltip(Some(
                                    presentation(&current.snapshot).accessible_text,
                                ));
                                let fingerprint = format!(
                                    "{}:{}:{}:{:?}",
                                    current.visible,
                                    current.autostart,
                                    current.preferred_device_id,
                                    current
                                        .devices
                                        .iter()
                                        .map(|s| (&s.device_id, s.display_name()))
                                        .collect::<Vec<_>>()
                                );
                                if fingerprint != last_menu {
                                    if let Ok(menu) = tray_menu(&worker_app, &current) {
                                        let _ = tray.set_menu(Some(menu));
                                        last_menu = fingerprint;
                                    }
                                }
                            }
                            next_tray = Instant::now() + Duration::from_secs(1);
                        }
                        std::thread::sleep(Duration::from_millis(if current.visible {
                            16
                        } else {
                            100
                        }));
                    }
                })?;
            settings::log(
                &state.directory,
                if is_demo {
                    "Tauri 3.0 模拟模式启动"
                } else {
                    "Tauri 3.0 硬件监测启动"
                },
            );
            Ok(())
        })
        .build(context)
        .expect("无法启动手柄看板");
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            if let Some(state) = app.try_state::<Arc<Coordinator>>() {
                state.stop.store(true, Ordering::Release);
                let mut inner = state.inner.lock().unwrap_or_else(|e| e.into_inner());
                state.save(&mut inner);
            }
        }
    });
}
