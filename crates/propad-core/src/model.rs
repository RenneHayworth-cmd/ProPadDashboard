use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Transport {
    Bluetooth,
    Usb,
    Wireless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControllerFamily {
    SwitchPro,
    Xbox,
    DualSense,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionState {
    Disconnected,
    Reading,
    Connected,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerState {
    Unknown,
    OnBattery,
    Charging,
    Charged,
    ExternalPower,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryReading {
    pub level: u8,
    pub charging: bool,
    pub external_power: bool,
    pub timestamp: u64,
    pub percent_override: Option<u8>,
    pub percentage_known: bool,
    pub reported_power: Option<PowerState>,
}

impl BatteryReading {
    pub fn percent(&self) -> u8 {
        self.percent_override
            .unwrap_or_else(|| ((self.level as u16 * 100 / 8).min(100)) as u8)
            .min(100)
    }
    pub fn low(&self) -> bool {
        self.percentage_known && self.percent() <= 25 && !self.charging
    }
    pub fn power(&self) -> PowerState {
        self.reported_power.unwrap_or(if self.charging {
            if self.level == 8 {
                PowerState::Charged
            } else {
                PowerState::Charging
            }
        } else if self.external_power {
            if self.level == 8 {
                PowerState::Charged
            } else {
                PowerState::ExternalPower
            }
        } else {
            PowerState::OnBattery
        })
    }
    pub fn switch(level: u8, charging: bool, external_power: bool, timestamp: u64) -> Self {
        Self {
            level,
            charging,
            external_power,
            timestamp,
            percent_override: None,
            percentage_known: true,
            reported_power: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdentity {
    pub address: String,
    pub firmware: Option<String>,
    pub device_type: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerSnapshot {
    pub device_id: String,
    pub transport: Option<Transport>,
    pub connection: ConnectionState,
    pub battery: Option<BatteryReading>,
    pub timestamp: u64,
    pub detail: Option<String>,
    pub is_stale: bool,
    pub family: ControllerFamily,
    pub name: Option<String>,
}
impl ControllerSnapshot {
    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .unwrap_or_else(|| crate::family_name(self.family))
    }
    pub fn disconnected(now: u64) -> Self {
        Self {
            device_id: String::new(),
            transport: None,
            connection: ConnectionState::Disconnected,
            battery: None,
            timestamp: now,
            detail: None,
            is_stale: false,
            family: ControllerFamily::SwitchPro,
            name: None,
        }
    }
}

pub mod buttons {
    pub const NONE: u32 = 0;
    pub const A: u32 = 1;
    pub const B: u32 = 1 << 1;
    pub const X: u32 = 1 << 2;
    pub const Y: u32 = 1 << 3;
    pub const UP: u32 = 1 << 4;
    pub const DOWN: u32 = 1 << 5;
    pub const LEFT: u32 = 1 << 6;
    pub const RIGHT: u32 = 1 << 7;
    pub const L: u32 = 1 << 8;
    pub const R: u32 = 1 << 9;
    pub const ZL: u32 = 1 << 10;
    pub const ZR: u32 = 1 << 11;
    pub const MINUS: u32 = 1 << 12;
    pub const PLUS: u32 = 1 << 13;
    pub const HOME: u32 = 1 << 14;
    pub const CAPTURE: u32 = 1 << 15;
    pub const LEFT_STICK: u32 = 1 << 16;
    pub const RIGHT_STICK: u32 = 1 << 17;
    pub const TOUCHPAD: u32 = 1 << 18;
    pub const MICROPHONE: u32 = 1 << 19;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerInput {
    pub device_id: String,
    pub transport: Transport,
    pub buttons: u32,
    pub left_x: f64,
    pub left_y: f64,
    pub right_x: f64,
    pub right_y: f64,
    pub timestamp: u64,
    pub report_id: u8,
}
impl ControllerInput {
    pub fn neutral() -> Self {
        Self {
            device_id: String::new(),
            transport: Transport::Bluetooth,
            buttons: 0,
            left_x: 0.0,
            left_y: 0.0,
            right_x: 0.0,
            right_y: 0.0,
            timestamp: 0,
            report_id: 0,
        }
    }
    pub fn down(&self, button: u32) -> bool {
        self.buttons & button != 0
    }
    pub fn same_pose(&self, other: &Self) -> bool {
        self.buttons == other.buttons
            && (self.left_x - other.left_x).abs() < 0.004
            && (self.left_y - other.left_y).abs() < 0.004
            && (self.right_x - other.right_x).abs() < 0.004
            && (self.right_y - other.right_y).abs() < 0.004
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardPresentation {
    pub percentage: String,
    pub status: String,
    pub transport: String,
    pub filled_segments: u8,
    pub tone: String,
    pub animate_charging: bool,
    pub dimmed: bool,
    pub accessible_text: String,
}

pub fn presentation(snapshot: &ControllerSnapshot) -> DashboardPresentation {
    let name = snapshot.display_name();
    let transport = match snapshot.transport {
        Some(Transport::Bluetooth) => "蓝牙",
        Some(Transport::Usb) => "USB",
        Some(Transport::Wireless) => "无线",
        None => "离线",
    };
    let make = |percentage: &str,
                status: &str,
                conn: &str,
                filled_segments,
                tone: &str,
                animate_charging,
                dimmed,
                accessible_text: String| DashboardPresentation {
        percentage: percentage.into(),
        status: status.into(),
        transport: conn.into(),
        filled_segments,
        tone: tone.into(),
        animate_charging,
        dimmed,
        accessible_text,
    };
    if snapshot.connection == ConnectionState::Disconnected {
        return make(
            "—",
            "手柄已断开",
            "离线",
            0,
            "muted",
            false,
            true,
            format!("{name}，手柄已断开"),
        );
    }
    if snapshot.connection == ConnectionState::Reading && snapshot.battery.is_none() {
        return make(
            "···",
            "正在读取电量",
            transport,
            0,
            "muted",
            false,
            false,
            format!("{name}，正在读取电量"),
        );
    }
    if snapshot.connection == ConnectionState::Unavailable || snapshot.is_stale {
        let percent = snapshot
            .battery
            .as_ref()
            .filter(|b| b.percentage_known)
            .map_or_else(|| "—".into(), |b| format!("约 {}%", b.percent()));
        let status = if snapshot.battery.is_none() {
            "电量暂不可读"
        } else {
            "读数未更新"
        };
        let segments = snapshot
            .battery
            .as_ref()
            .filter(|b| b.percentage_known)
            .map_or(0, |b| b.percent().div_ceil(25).min(4));
        return make(
            &percent,
            status,
            transport,
            segments,
            "muted",
            false,
            false,
            format!("{name}，{status}，{percent}"),
        );
    }
    let Some(battery) = snapshot.battery.as_ref() else {
        return make(
            "—",
            snapshot
                .detail
                .as_deref()
                .unwrap_or("已连接 · 电量暂不可读"),
            transport,
            0,
            "muted",
            false,
            false,
            format!("{name}，电量暂不可读"),
        );
    };
    let text = match battery.power() {
        PowerState::Charging => "正在充电",
        PowerState::Charged => "已充满",
        PowerState::ExternalPower => "外部供电",
        PowerState::OnBattery if battery.low() => "电量偏低",
        PowerState::OnBattery => "电池供电",
        PowerState::Unknown => "电量暂不可读",
    };
    let percentage = if battery.percentage_known {
        format!("约 {}%", battery.percent())
    } else {
        "—".into()
    };
    let segments = if battery.percentage_known {
        battery.percent().div_ceil(25).min(4)
    } else {
        0
    };
    let extra = if battery.percentage_known {
        format!("，约 {}%", battery.percent())
    } else {
        String::new()
    };
    make(
        &percentage,
        text,
        transport,
        segments,
        if battery.low() {
            "low"
        } else if battery.power() == PowerState::Unknown {
            "muted"
        } else {
            "green"
        },
        battery.power() == PowerState::Charging,
        false,
        format!("{name}，{transport}，{text}{extra}"),
    )
}
