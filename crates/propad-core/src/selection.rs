use crate::{ConnectionState, ControllerFamily, ControllerSnapshot, Transport};
use std::collections::HashMap;

pub fn family_name(family: ControllerFamily) -> &'static str {
    match family {
        ControllerFamily::SwitchPro => "Nintendo Switch Pro",
        ControllerFamily::Xbox => "Xbox Controller",
        ControllerFamily::DualSense => "PS5 DualSense",
    }
}
pub fn identify_controller(vendor: u16, product: u16) -> Option<ControllerFamily> {
    match (vendor, product) {
        (0x057e, 0x2009) => Some(ControllerFamily::SwitchPro),
        (0x054c, 0x0ce6 | 0x0df2) => Some(ControllerFamily::DualSense),
        (
            0x045e,
            0x028e | 0x02d1 | 0x02dd | 0x02e0 | 0x02ea | 0x02fd | 0x0b00 | 0x0b05 | 0x0b12 | 0x0b13
            | 0x0b20 | 0x0b21 | 0x0b22,
        ) => Some(ControllerFamily::Xbox),
        _ => None,
    }
}
pub fn device_id(family: ControllerFamily, address_or_path: &str) -> String {
    let prefix = match family {
        ControllerFamily::SwitchPro => "switchpro",
        ControllerFamily::Xbox => "xbox",
        ControllerFamily::DualSense => "dualsense",
    };
    format!("{prefix}:{}", address_or_path.to_uppercase())
}
pub fn merge_snapshots(snapshots: &[ControllerSnapshot]) -> Vec<ControllerSnapshot> {
    let mut combined: Vec<ControllerSnapshot> = Vec::new();
    let rank = |s: &ControllerSnapshot| {
        (
            s.battery.is_some() && !s.is_stale,
            s.transport == Some(Transport::Usb),
        )
    };
    for s in snapshots
        .iter()
        .filter(|s| s.connection != ConnectionState::Disconnected && !s.device_id.is_empty())
    {
        if let Some(existing) = combined
            .iter_mut()
            .find(|x| x.device_id.eq_ignore_ascii_case(&s.device_id))
        {
            if rank(s) > rank(existing) {
                *existing = s.clone();
            }
        } else {
            combined.push(s.clone());
        }
    }
    combined
}
pub fn choose_snapshot(
    snapshots: &[ControllerSnapshot],
    preferred: &str,
    connected_at: &HashMap<String, u64>,
) -> Option<ControllerSnapshot> {
    if let Some(chosen) = snapshots
        .iter()
        .find(|s| s.device_id.eq_ignore_ascii_case(preferred))
    {
        return Some(chosen.clone());
    }
    snapshots
        .iter()
        .min_by(|a, b| {
            connected_at
                .get(&b.device_id)
                .copied()
                .unwrap_or(0)
                .cmp(&connected_at.get(&a.device_id).copied().unwrap_or(0))
                .then_with(|| a.device_id.cmp(&b.device_id))
        })
        .cloned()
}
