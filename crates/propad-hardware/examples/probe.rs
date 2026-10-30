//! Explicit opt-in real-device diagnostic, independent of Tauri/WebView2.
//! Unit tests never open controller handles.
use propad_hardware::Monitor;
use std::time::Duration;

fn main() {
    let seconds = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(8)
        .min(120);
    let monitor = Monitor::start();
    monitor.set_visible(true);
    for _ in 0..seconds {
        std::thread::sleep(Duration::from_secs(1));
        let states = monitor.devices();
        println!("devices={}", states.len());
        for state in states {
            println!(
                "{} {:?} {:?} battery={:?} stale={}",
                state.device_id, state.transport, state.connection, state.battery, state.is_stale
            );
            if let Some(input) = monitor.input(&state.device_id) {
                println!(
                    "input report={:02x} buttons={:05x} sticks=({:.2},{:.2}) ({:.2},{:.2})",
                    input.report_id,
                    input.buttons,
                    input.left_x,
                    input.left_y,
                    input.right_x,
                    input.right_y
                );
            }
        }
    }
    let diagnostics = monitor.diagnostics();
    println!(
        "reports={} observed_buttons={:05x} observed_axes={:02x}",
        diagnostics.reports, diagnostics.observed_buttons, diagnostics.observed_axes
    );
}
