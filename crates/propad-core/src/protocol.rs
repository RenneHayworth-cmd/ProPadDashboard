use crate::{buttons as b, BatteryReading, ControllerInput, DeviceIdentity, PowerState, Transport};

pub fn is_address(address: &str) -> bool {
    address.len() == 12
        && address.bytes().all(|b| b.is_ascii_hexdigit())
        && address != "000000000000"
        && !address.eq_ignore_ascii_case("FFFFFFFFFFFF")
}
pub fn normalize_address(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}
pub fn address_from_hid_serial(serial: &str, transport: Transport) -> String {
    if transport != Transport::Bluetooth {
        return String::new();
    }
    let address = normalize_address(serial);
    if is_address(&address) {
        address
    } else {
        String::new()
    }
}
fn hex(bytes: impl Iterator<Item = u8>) -> String {
    bytes.map(|b| format!("{b:02X}")).collect()
}
pub fn read_switch_battery(report: &[u8], now: u64) -> Option<BatteryReading> {
    if report.len() < 13 || !matches!(report[0], 0x21 | 0x30 | 0x31) {
        return None;
    }
    if report[0] == 0x21 && (report.len() < 15 || report[13] & 0x80 == 0) {
        return None;
    }
    let level = (report[2] & 0xe0) >> 4;
    if level > 8 {
        return None;
    }
    Some(BatteryReading::switch(
        level,
        report[2] & 0x10 != 0,
        report[2] & 1 != 0,
        now,
    ))
}
pub fn read_switch_identity(report: &[u8]) -> Option<DeviceIdentity> {
    if report.len() >= 27 && report[0] == 0x21 && report[14] == 2 && report[13] & 0x80 != 0 {
        let address = hex(report[19..25].iter().copied());
        if is_address(&address) {
            return Some(DeviceIdentity {
                address,
                firmware: Some(format!("{}.{}", report[15], report[16])),
                device_type: Some(report[17]),
            });
        }
    }
    if report.len() >= 10 && report[0] == 0x81 && report[1] == 1 {
        let address = hex(report[4..10].iter().rev().copied());
        if is_address(&address) {
            return Some(DeviceIdentity {
                address,
                firmware: None,
                device_type: Some(report[3]),
            });
        }
    }
    None
}
pub fn switch_device_info_request(output_length: usize, sequence: u8) -> Vec<u8> {
    switch_subcommand_request(output_length, sequence, 2, &[])
}
pub fn switch_subcommand_request(
    output_length: usize,
    sequence: u8,
    command: u8,
    payload: &[u8],
) -> Vec<u8> {
    let mut data = vec![0; output_length.max(49)];
    assert!(
        payload.len() <= data.len() - 11,
        "subcommand payload exceeds report length"
    );
    data[0] = 1;
    data[1] = sequence & 15;
    data[2..10].copy_from_slice(&[0, 1, 0x40, 0x40, 0, 1, 0x40, 0x40]);
    data[10] = command;
    data[11..11 + payload.len()].copy_from_slice(payload);
    data
}
pub fn switch_usb_command(command: u8, output_length: usize) -> Vec<u8> {
    let mut data = vec![0; output_length.max(64)];
    data[0] = 0x80;
    data[1] = command;
    data
}
fn dead_zone(value: f64) -> f64 {
    let value = value.clamp(-1.0, 1.0);
    if value.abs() <= 0.08 {
        0.0
    } else {
        ((value.abs() - 0.08) / 0.92).copysign(value)
    }
}
fn norm16(bytes: &[u8]) -> f64 {
    let delta = u16::from_le_bytes([bytes[0], bytes[1]]) as i32 - 32768;
    dead_zone(delta as f64 / if delta < 0 { 32768.0 } else { 32767.0 })
}
fn norm12(value: u16) -> f64 {
    dead_zone((value as f64 - 2048.0) / 1536.0)
}
fn hat(value: u8) -> u32 {
    match value {
        0 => b::UP,
        1 => b::UP | b::RIGHT,
        2 => b::RIGHT,
        3 => b::DOWN | b::RIGHT,
        4 => b::DOWN,
        5 => b::DOWN | b::LEFT,
        6 => b::LEFT,
        7 => b::UP | b::LEFT,
        _ => 0,
    }
}
fn mapped(value: u8, pairs: &[(u8, u32)]) -> u32 {
    pairs
        .iter()
        .filter(|(mask, _)| value & mask != 0)
        .fold(0, |buttons, (_, button)| buttons | button)
}
pub fn decode_switch_input(
    report: &[u8],
    device_id: &str,
    transport: Transport,
    now: u64,
) -> Option<ControllerInput> {
    if report.len() < 12 {
        return None;
    }
    let (buttons, lx, ly, rx, ry) = match report[0] {
        0x3f => {
            let face = report[1];
            let system = report[2];
            let mut buttons = mapped(
                face,
                &[
                    (1, b::B),
                    (2, b::A),
                    (4, b::Y),
                    (8, b::X),
                    (16, b::L),
                    (32, b::R),
                    (64, b::ZL),
                    (128, b::ZR),
                ],
            );
            buttons |= mapped(
                system,
                &[
                    (1, b::MINUS),
                    (2, b::PLUS),
                    (4, b::LEFT_STICK),
                    (8, b::RIGHT_STICK),
                    (16, b::HOME),
                    (32, b::CAPTURE),
                    (128, b::ZR),
                ],
            ) | hat(report[3]);
            (
                buttons,
                norm16(&report[4..6]),
                norm16(&report[6..8]),
                norm16(&report[8..10]),
                norm16(&report[10..12]),
            )
        }
        0x21 | 0x30 | 0x31 => {
            if report.len() < 13
                || (report[0] == 0x21 && (report.len() < 15 || report[13] & 0x80 == 0))
            {
                return None;
            }
            let buttons = mapped(
                report[3],
                &[
                    (1, b::Y),
                    (2, b::X),
                    (4, b::B),
                    (8, b::A),
                    (64, b::R),
                    (128, b::ZR),
                ],
            ) | mapped(
                report[4],
                &[
                    (1, b::MINUS),
                    (2, b::PLUS),
                    (4, b::RIGHT_STICK),
                    (8, b::LEFT_STICK),
                    (16, b::HOME),
                    (32, b::CAPTURE),
                ],
            ) | mapped(
                report[5],
                &[
                    (1, b::DOWN),
                    (2, b::UP),
                    (4, b::RIGHT),
                    (8, b::LEFT),
                    (64, b::L),
                    (128, b::ZL),
                ],
            );
            (
                buttons,
                norm12(report[6] as u16 | ((report[7] as u16 & 15) << 8)),
                -norm12((report[7] as u16 >> 4) | ((report[8] as u16) << 4)),
                norm12(report[9] as u16 | ((report[10] as u16 & 15) << 8)),
                -norm12((report[10] as u16 >> 4) | ((report[11] as u16) << 4)),
            )
        }
        _ => return None,
    };
    Some(ControllerInput {
        device_id: device_id.into(),
        transport,
        buttons,
        left_x: lx,
        left_y: ly,
        right_x: rx,
        right_y: ry,
        timestamp: now,
        report_id: report[0],
    })
}
fn dualsense_data(report: &[u8]) -> Option<(&[u8], bool)> {
    if report.len() < 10 || !matches!(report[0], 1 | 0x31) {
        return None;
    }
    let simple = report[0] == 1 && (report.len() == 10 || report.len() == 78);
    let offset = if report[0] == 0x31 { 2 } else { 1 };
    if !simple && report.len() < offset + 54 {
        return None;
    }
    Some((&report[offset..], simple))
}
pub fn decode_dualsense_input(
    report: &[u8],
    device_id: &str,
    transport: Transport,
    now: u64,
) -> Option<ControllerInput> {
    let (data, simple) = dualsense_data(report)?;
    let b0 = data[if simple { 4 } else { 7 }];
    let b1 = data[if simple { 5 } else { 8 }];
    let b2 = data[if simple { 6 } else { 9 }];
    let mut buttons = mapped(b0, &[(16, b::X), (32, b::A), (64, b::B), (128, b::Y)])
        | mapped(
            b1,
            &[
                (1, b::L),
                (2, b::R),
                (4, b::ZL),
                (8, b::ZR),
                (16, b::MINUS),
                (32, b::PLUS),
                (64, b::LEFT_STICK),
                (128, b::RIGHT_STICK),
            ],
        )
        | mapped(b2, &[(1, b::HOME), (2, b::TOUCHPAD)])
        | hat(b0 & 15);
    if data[if simple { 7 } else { 4 }] > 30 {
        buttons |= b::ZL;
    }
    if data[if simple { 8 } else { 5 }] > 30 {
        buttons |= b::ZR;
    }
    if !simple && b2 & 4 != 0 {
        buttons |= b::MICROPHONE;
    }
    let axis =
        |value: u8| dead_zone((value as f64 - 128.0) / if value < 128 { 128.0 } else { 127.0 });
    Some(ControllerInput {
        device_id: device_id.into(),
        transport,
        buttons,
        left_x: axis(data[0]),
        left_y: axis(data[1]),
        right_x: axis(data[2]),
        right_y: axis(data[3]),
        timestamp: now,
        report_id: report[0],
    })
}
pub fn read_dualsense_battery(report: &[u8], now: u64) -> Option<BatteryReading> {
    let (data, simple) = dualsense_data(report)?;
    if simple {
        return None;
    }
    let status = data[52] >> 4;
    let level = data[52] & 15;
    let known = status <= 2 && level <= 10;
    let percent = if status == 2 {
        100
    } else {
        level.saturating_mul(10).saturating_add(5).min(100)
    };
    let power = match status {
        0 => PowerState::OnBattery,
        1 => PowerState::Charging,
        2 => PowerState::Charged,
        _ => PowerState::Unknown,
    };
    Some(BatteryReading {
        level: (percent as f64 * 8.0 / 100.0).round() as u8,
        charging: status == 1,
        external_power: data[53] & 8 != 0 || report[0] == 1,
        timestamp: now,
        percent_override: Some(percent),
        percentage_known: known,
        reported_power: Some(power),
    })
}
pub fn read_dualsense_address(report: &[u8]) -> Option<String> {
    if report.len() < 7 || report[0] != 9 {
        return None;
    }
    let address = hex(report[1..7].iter().rev().copied());
    if is_address(&address) {
        Some(address)
    } else {
        None
    }
}
#[allow(clippy::too_many_arguments)]
pub fn decode_xbox_input(
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    lx: i16,
    ly: i16,
    rx: i16,
    ry: i16,
    device_id: &str,
    transport: Transport,
    now: u64,
) -> ControllerInput {
    let pairs = [
        (1, b::UP),
        (2, b::DOWN),
        (4, b::LEFT),
        (8, b::RIGHT),
        (0x10, b::PLUS),
        (0x20, b::MINUS),
        (0x40, b::LEFT_STICK),
        (0x80, b::RIGHT_STICK),
        (0x100, b::L),
        (0x200, b::R),
        (0x400, b::HOME),
        (0x1000, b::A),
        (0x2000, b::B),
        (0x4000, b::X),
        (0x8000, b::Y),
    ];
    let mut result = pairs
        .into_iter()
        .filter(|(mask, _)| buttons & mask != 0)
        .fold(0, |a, (_, b)| a | b);
    if left_trigger > 30 {
        result |= b::ZL;
    }
    if right_trigger > 30 {
        result |= b::ZR;
    }
    let axis = |v: i16| dead_zone(v as f64 / if v < 0 { 32768.0 } else { 32767.0 });
    ControllerInput {
        device_id: device_id.into(),
        transport,
        buttons: result,
        left_x: axis(lx),
        left_y: -axis(ly),
        right_x: axis(rx),
        right_y: -axis(ry),
        timestamp: now,
        report_id: 0,
    }
}
pub fn read_xbox_battery(battery_type: u8, level: u8, now: u64) -> Option<BatteryReading> {
    if battery_type == 1 {
        return Some(BatteryReading {
            level: 0,
            charging: false,
            external_power: true,
            timestamp: now,
            percent_override: None,
            percentage_known: false,
            reported_power: Some(PowerState::ExternalPower),
        });
    }
    if !matches!(battery_type, 2 | 3) || level > 3 {
        return None;
    }
    let percent = [0, 25, 50, 100][level as usize];
    Some(BatteryReading {
        level: (percent as u16 * 8 / 100) as u8,
        charging: false,
        external_power: false,
        timestamp: now,
        percent_override: Some(percent),
        percentage_known: true,
        reported_power: Some(PowerState::OnBattery),
    })
}
