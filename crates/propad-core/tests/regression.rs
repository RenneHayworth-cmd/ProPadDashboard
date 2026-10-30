use propad_core::{buttons as b, *};
use std::collections::HashMap;
const NOW: u64 = 1_798_459_200_000;
fn report(id: u8, battery: u8) -> Vec<u8> {
    let mut r = vec![0; if id == 0x21 { 49 } else { 64 }];
    r[0] = id;
    r[2] = battery;
    if id == 0x21 {
        r[13] = 0x82;
        r[14] = 2;
    }
    r
}
fn battery(level: u8, charging: bool, external: bool) -> BatteryReading {
    BatteryReading::switch(level, charging, external, NOW)
}
fn snapshot(battery: Option<BatteryReading>) -> ControllerSnapshot {
    ControllerSnapshot {
        device_id: "021122334455".into(),
        transport: Some(Transport::Bluetooth),
        connection: ConnectionState::Connected,
        battery,
        timestamp: NOW,
        detail: None,
        is_stale: false,
        family: ControllerFamily::SwitchPro,
        name: None,
    }
}
fn simple(face: u8, system: u8, hat: u8, axes: [u16; 4]) -> Vec<u8> {
    let mut r = vec![0; 12];
    r[0] = 0x3f;
    r[1] = face;
    r[2] = system;
    r[3] = hat;
    for (i, v) in axes.into_iter().enumerate() {
        r[4 + i * 2..6 + i * 2].copy_from_slice(&v.to_le_bytes());
    }
    r
}
fn full(right: u8, system: u8, left: u8, axes: [u16; 4]) -> Vec<u8> {
    let mut r = vec![0; 13];
    r[0] = 0x30;
    r[2] = 0x40;
    r[3] = right;
    r[4] = system;
    r[5] = left;
    for (i, pair) in axes.chunks(2).enumerate() {
        let o = 6 + i * 3;
        let (x, y) = (pair[0], pair[1]);
        r[o] = x as u8;
        r[o + 1] = ((x >> 8) | ((y & 15) << 4)) as u8;
        r[o + 2] = (y >> 4) as u8;
    }
    r
}
fn decode(r: &[u8]) -> ControllerInput {
    decode_switch_input(r, "021122334455", Transport::Bluetooth, NOW).unwrap()
}
fn ps5(bluetooth: bool, battery: u8) -> Vec<u8> {
    let o = if bluetooth { 2 } else { 1 };
    let mut r = vec![0; if bluetooth { 78 } else { 64 }];
    r[0] = if bluetooth { 0x31 } else { 1 };
    for i in 0..4 {
        r[o + i] = 128;
    }
    r[o + 7] = 8;
    r[o + 52] = battery;
    r[o + 53] = if bluetooth { 0 } else { 8 };
    r
}

macro_rules! switch_bucket {
    ($name:ident,$raw:expr,$percent:expr) => {
        #[test]
        fn $name() {
            let r = read_switch_battery(&report(0x30, $raw), NOW).unwrap();
            assert_eq!(r.percent(), $percent);
            assert_eq!(r.power(), PowerState::OnBattery);
        }
    };
}
switch_bucket!(switch_bucket_empty, 0, 0);
switch_bucket!(switch_bucket_low, 0x20, 25);
switch_bucket!(switch_bucket_half, 0x40, 50);
switch_bucket!(switch_bucket_three_quarters, 0x60, 75);
switch_bucket!(switch_bucket_full, 0x80, 100);
#[test]
fn known_device_21_7e_40() {
    let mut r = report(0x21, 0x40);
    r[1] = 0x7e;
    let b = read_switch_battery(&r, NOW).unwrap();
    assert_eq!(b.percent(), 50);
    assert!(!b.charging);
}
#[test]
fn charging_flag_separate_from_level_and_transport() {
    let b = read_switch_battery(&report(0x21, 0x50), NOW).unwrap();
    assert_eq!(b.percent(), 50);
    assert_eq!(b.power(), PowerState::Charging);
    assert!(!b.external_power);
}
#[test]
fn full_and_charging_is_charged() {
    let b = read_switch_battery(&report(0x30, 0x91), NOW).unwrap();
    assert_eq!(b.percent(), 100);
    assert_eq!(b.power(), PowerState::Charged);
    assert!(b.external_power);
}
#[test]
fn full_wireless_is_still_on_battery() {
    assert_eq!(
        read_switch_battery(&report(0x30, 0x80), NOW)
            .unwrap()
            .power(),
        PowerState::OnBattery
    );
}
#[test]
fn external_supply_without_charging() {
    assert_eq!(
        read_switch_battery(&report(0x30, 0x41), NOW)
            .unwrap()
            .power(),
        PowerState::ExternalPower
    );
}
#[test]
fn simple_report_never_contains_battery() {
    assert!(read_switch_battery(&report(0x3f, 0x80), NOW).is_none());
}
#[test]
fn usb_ack_not_battery() {
    assert!(read_switch_battery(&report(0x81, 0x80), NOW).is_none());
}
#[test]
fn empty_truncated_battery_rejected() {
    for r in [vec![], vec![0x30, 1, 0x40], vec![0x21; 13]] {
        assert!(read_switch_battery(&r, NOW).is_none());
    }
}
#[test]
fn negative_ack_rejected() {
    let mut r = report(0x21, 0x40);
    r[13] = 2;
    assert!(read_switch_battery(&r, NOW).is_none());
}
#[test]
fn impossible_level_rejected() {
    assert!(read_switch_battery(&report(0x30, 0xe0), NOW).is_none());
}
#[test]
fn info_identity_fixture_mac() {
    let mut r = report(0x21, 0x40);
    r[15] = 5;
    r[16] = 2;
    r[17] = 3;
    r[19..25].copy_from_slice(&[0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
    let i = read_switch_identity(&r).unwrap();
    assert_eq!(i.address, "021122334455");
    assert_eq!(i.firmware.as_deref(), Some("5.2"));
    assert_eq!(i.device_type, Some(3));
}
#[test]
fn usb_identity_little_endian() {
    let mut r = vec![0; 64];
    r[0] = 0x81;
    r[1] = 1;
    r[3] = 3;
    r[4..10].copy_from_slice(&[0x55, 0x44, 0x33, 0x22, 0x11, 0x02]);
    assert_eq!(read_switch_identity(&r).unwrap().address, "021122334455");
}
#[test]
fn zero_address_cannot_bind() {
    assert!(read_switch_identity(&report(0x21, 0x40)).is_none());
}
#[test]
fn usb_placeholder_is_not_physical_address() {
    assert_eq!(address_from_hid_serial("000000000001", Transport::Usb), "");
}
#[test]
fn bluetooth_serial_normalizes() {
    assert_eq!(
        address_from_hid_serial("02:11:22:33:44:55", Transport::Bluetooth),
        "021122334455"
    );
}
#[test]
fn request_header_and_neutral_payload() {
    let r = switch_device_info_request(49, 0x17);
    assert_eq!(r.len(), 49);
    assert_eq!(r[0], 1);
    assert_eq!(r[1], 7);
    assert_eq!(r[10], 2);
    assert_eq!(&r[2..10], &[0, 1, 0x40, 0x40, 0, 1, 0x40, 0x40]);
}
#[test]
fn usb_and_long_output_reports_padded() {
    assert_eq!(switch_device_info_request(64, 0).len(), 64);
    let r = switch_usb_command(2, 64);
    assert_eq!(r.len(), 64);
    assert_eq!(&r[..2], &[0x80, 2]);
}
#[test]
fn unavailable_never_displays_zero() {
    let mut s = snapshot(None);
    s.connection = ConnectionState::Unavailable;
    let p = presentation(&s);
    assert_eq!(p.percentage, "—");
    assert_eq!(p.status, "电量暂不可读");
    assert_eq!(p.filled_segments, 0);
}
#[test]
fn stale_retains_value_without_charging_animation() {
    let mut s = snapshot(Some(battery(4, true, false)));
    s.connection = ConnectionState::Unavailable;
    s.is_stale = true;
    let p = presentation(&s);
    assert_eq!(p.percentage, "约 50%");
    assert_eq!(p.status, "读数未更新");
    assert!(!p.animate_charging);
    assert_eq!(p.tone, "muted");
}
#[test]
fn low_threshold_includes_25_excludes_charging() {
    assert_eq!(
        presentation(&snapshot(Some(battery(2, false, false)))).tone,
        "low"
    );
    assert_eq!(
        presentation(&snapshot(Some(battery(2, true, false)))).tone,
        "green"
    );
}
#[test]
fn only_active_charging_animates() {
    assert!(presentation(&snapshot(Some(battery(4, true, false)))).animate_charging);
    assert!(!presentation(&snapshot(Some(battery(8, true, false)))).animate_charging);
}
#[test]
fn reading_and_offline_distinct_from_empty() {
    let mut s = snapshot(None);
    s.connection = ConnectionState::Reading;
    assert_eq!(presentation(&s).percentage, "···");
    assert!(presentation(&ControllerSnapshot::disconnected(NOW)).dimmed);
    assert_eq!(
        presentation(&snapshot(Some(battery(0, false, false)))).percentage,
        "约 0%"
    );
}

#[test]
fn connection_opens_once() {
    let mut s = VisibilitySession::new();
    assert!(s.observe(true, NOW));
    assert!(s.visible && s.has_session);
    assert!(!s.observe(true, NOW + 1000));
}
#[test]
fn manual_close_survives_refresh() {
    let mut s = VisibilitySession::new();
    s.observe(true, NOW);
    s.hide_manually();
    assert!(!s.observe(true, NOW + 5000));
    assert!(!s.visible && s.hidden_by_user);
}
#[test]
fn handover_within_three_seconds_preserves_session() {
    let mut s = VisibilitySession::new();
    s.observe(true, NOW);
    s.hide_manually();
    s.observe(false, NOW + 1000);
    assert!(!s.observe(true, NOW + 3000));
    assert!(s.has_session && !s.visible && s.hidden_by_user);
}
#[test]
fn disconnect_merges_then_holds_notice() {
    let mut s = VisibilitySession::new();
    s.observe(true, NOW);
    s.observe(false, NOW + 1000);
    s.observe(false, NOW + 3000);
    assert!(s.has_session);
    s.observe(false, NOW + 4000);
    assert!(!s.has_session && s.visible);
    assert!(!s.tick(NOW + 6000));
    assert!(s.tick(NOW + 7000));
    assert!(!s.visible);
}
#[test]
fn hidden_disconnect_never_opens_notice() {
    let mut s = VisibilitySession::new();
    s.observe(true, NOW);
    s.hide_manually();
    s.observe(false, NOW + 1000);
    s.observe(false, NOW + 4000);
    assert!(!s.visible && s.hide_at.is_none());
}
#[test]
fn reconnection_opens_after_manual_hide() {
    let mut s = VisibilitySession::new();
    s.observe(true, NOW);
    s.hide_manually();
    s.observe(false, NOW + 1000);
    s.observe(false, NOW + 4000);
    assert!(s.observe(true, NOW + 8000));
    assert!(s.visible && !s.hidden_by_user);
}
#[test]
fn reconnection_cancels_delayed_hide() {
    let mut s = VisibilitySession::new();
    s.observe(true, NOW);
    s.observe(false, NOW + 1000);
    s.observe(false, NOW + 4000);
    s.observe(true, NOW + 5000);
    assert!(!s.tick(NOW + 8000));
    assert!(s.visible);
}

macro_rules! simple_mapping {
    ($name:ident,$face:expr,$system:expr,$button:expr) => {
        #[test]
        fn $name() {
            assert_eq!(
                decode(&simple($face, $system, 8, [32768; 4])).buttons,
                $button
            );
        }
    };
}
simple_mapping!(simple_a, 2, 0, b::A);
simple_mapping!(simple_b, 1, 0, b::B);
simple_mapping!(simple_x, 8, 0, b::X);
simple_mapping!(simple_y, 4, 0, b::Y);
simple_mapping!(simple_l, 16, 0, b::L);
simple_mapping!(simple_r, 32, 0, b::R);
simple_mapping!(simple_zl, 64, 0, b::ZL);
simple_mapping!(simple_zr, 128, 0, b::ZR);
simple_mapping!(simple_minus, 0, 1, b::MINUS);
simple_mapping!(simple_plus, 0, 2, b::PLUS);
simple_mapping!(simple_l3, 0, 4, b::LEFT_STICK);
simple_mapping!(simple_r3, 0, 8, b::RIGHT_STICK);
simple_mapping!(simple_home, 0, 16, b::HOME);
simple_mapping!(simple_capture, 0, 32, b::CAPTURE);
macro_rules! full_mapping {
    ($name:ident,$right:expr,$system:expr,$left:expr,$button:expr) => {
        #[test]
        fn $name() {
            assert_eq!(
                decode(&full($right, $system, $left, [2048; 4])).buttons,
                $button
            );
        }
    };
}
full_mapping!(full_a, 8, 0, 0, b::A);
full_mapping!(full_b, 4, 0, 0, b::B);
full_mapping!(full_x, 2, 0, 0, b::X);
full_mapping!(full_y, 1, 0, 0, b::Y);
full_mapping!(full_r, 64, 0, 0, b::R);
full_mapping!(full_zr, 128, 0, 0, b::ZR);
full_mapping!(full_l, 0, 0, 64, b::L);
full_mapping!(full_zl, 0, 0, 128, b::ZL);
full_mapping!(full_minus, 0, 1, 0, b::MINUS);
full_mapping!(full_plus, 0, 2, 0, b::PLUS);
full_mapping!(full_r3, 0, 4, 0, b::RIGHT_STICK);
full_mapping!(full_l3, 0, 8, 0, b::LEFT_STICK);
full_mapping!(full_home, 0, 16, 0, b::HOME);
full_mapping!(full_capture, 0, 32, 0, b::CAPTURE);
full_mapping!(full_down, 0, 0, 1, b::DOWN);
full_mapping!(full_up, 0, 0, 2, b::UP);
full_mapping!(full_right, 0, 0, 4, b::RIGHT);
full_mapping!(full_left, 0, 0, 8, b::LEFT);
macro_rules! hat_mapping {
    ($name:ident,$hat:expr,$buttons:expr) => {
        #[test]
        fn $name() {
            assert_eq!(decode(&simple(0, 0, $hat, [32768; 4])).buttons, $buttons);
        }
    };
}
hat_mapping!(hat_up, 0, b::UP);
hat_mapping!(hat_up_right, 1, b::UP | b::RIGHT);
hat_mapping!(hat_right, 2, b::RIGHT);
hat_mapping!(hat_down_right, 3, b::DOWN | b::RIGHT);
hat_mapping!(hat_down, 4, b::DOWN);
hat_mapping!(hat_down_left, 5, b::DOWN | b::LEFT);
hat_mapping!(hat_left, 6, b::LEFT);
hat_mapping!(hat_up_left, 7, b::UP | b::LEFT);
hat_mapping!(hat_neutral, 8, 0);
#[test]
fn simple_stick_neutral_unsigned() {
    let i = decode(&simple(0, 0, 8, [32768; 4]));
    assert_eq!([i.left_x, i.left_y, i.right_x, i.right_y], [0.; 4]);
}
#[test]
fn simple_sticks_independent_screen_directions() {
    let i = decode(&simple(0, 0, 8, [0, 0, 65535, 65535]));
    assert_eq!(
        [i.left_x, i.left_y, i.right_x, i.right_y],
        [-1., -1., 1., 1.]
    );
}
#[test]
fn full_12_bit_sticks_vertical_inverted() {
    let i = decode(&full(0, 0, 0, [512, 3584, 3584, 512]));
    assert_eq!(
        [i.left_x, i.left_y, i.right_x, i.right_y],
        [-1., -1., 1., 1.]
    );
}
#[test]
fn stick_noise_inside_dead_zone() {
    let a = decode(&simple(0, 0, 8, [33000, 32000, 32768, 32768]));
    let c = decode(&full(0, 0, 0, [2100, 2000, 2048, 2048]));
    assert_eq!([a.left_x, a.left_y, c.left_x, c.left_y], [0.; 4]);
}
#[test]
fn combinations_simultaneous() {
    let i = decode(&simple(0x52, 6, 1, [32768; 4]));
    for key in [b::A, b::L, b::ZL, b::PLUS, b::LEFT_STICK, b::UP, b::RIGHT] {
        assert!(i.down(key));
    }
}
#[test]
fn release_clears_all_buttons() {
    let down = decode(&simple(255, 63, 1, [32768; 4]));
    let up = decode(&simple(0, 0, 8, [32768; 4]));
    assert_ne!(down.buttons, 0);
    assert_eq!(up.buttons, 0);
    assert!(!down.same_pose(&up));
}
#[test]
fn truncated_input_and_usb_ack_rejected() {
    assert!(decode_switch_input(&[0x3f, 0, 0, 8], "", Transport::Usb, NOW).is_none());
    assert!(decode_switch_input(&[0x81; 64], "", Transport::Usb, NOW).is_none());
}
#[test]
fn timestamp_change_does_not_redraw_pose() {
    let a = decode(&simple(0, 0, 8, [32768; 4]));
    let mut c = a.clone();
    c.timestamp += 1000;
    assert!(a.same_pose(&c));
}
#[test]
fn usb_stream_request_standard_subcommand() {
    let r = switch_subcommand_request(64, 4, 3, &[0x30]);
    assert_eq!(r.len(), 64);
    assert_eq!(r[0], 1);
    assert_eq!(r[10], 3);
    assert_eq!(r[11], 0x30);
}

#[test]
fn catalogue_recognizes_supported_families() {
    assert_eq!(
        identify_controller(0x057e, 0x2009),
        Some(ControllerFamily::SwitchPro)
    );
    assert_eq!(
        identify_controller(0x054c, 0x0ce6),
        Some(ControllerFamily::DualSense)
    );
    assert_eq!(
        identify_controller(0x054c, 0x0df2),
        Some(ControllerFamily::DualSense)
    );
    assert_eq!(
        identify_controller(0x045e, 0x0b13),
        Some(ControllerFamily::Xbox)
    );
    assert_eq!(identify_controller(0x054c, 0x09cc), None);
}
#[test]
fn new_switch_discovered_without_hardcoded_target() {
    let mut a = snapshot(Some(battery(4, false, false)));
    a.device_id = "switchpro:001122334455".into();
    assert_eq!(
        choose_snapshot(
            &[a.clone()],
            "",
            &HashMap::from([(a.device_id.clone(), NOW)])
        ),
        Some(a)
    );
}
#[test]
fn bluetooth_usb_same_physical_merge() {
    let mut a = snapshot(Some(battery(4, false, false)));
    a.device_id = "switchpro:ABCDEF123456".into();
    let mut c = a.clone();
    c.transport = Some(Transport::Usb);
    let v = merge_snapshots(&[a, c]);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].transport, Some(Transport::Usb));
}
#[test]
fn two_pro_controllers_select_independently() {
    let mut a = snapshot(Some(battery(4, false, false)));
    a.device_id = "switchpro:001122334455".into();
    let mut c = a.clone();
    c.device_id = "switchpro:556677889900".into();
    let times = HashMap::from([
        (a.device_id.clone(), NOW),
        (c.device_id.clone(), NOW + 1000),
    ]);
    assert_eq!(merge_snapshots(&[a.clone(), c.clone()]).len(), 2);
    assert_eq!(
        choose_snapshot(&[a.clone(), c], &a.device_id, &times),
        Some(a)
    );
}
#[test]
fn auto_selects_latest_family() {
    let mut a = snapshot(Some(battery(4, false, false)));
    a.device_id = "switchpro:A".into();
    let mut c = a.clone();
    c.device_id = "xinput:0".into();
    c.family = ControllerFamily::Xbox;
    let times = HashMap::from([
        (a.device_id.clone(), NOW),
        (c.device_id.clone(), NOW + 1000),
    ]);
    assert_eq!(choose_snapshot(&[a, c.clone()], "", &times), Some(c));
}
#[test]
fn offline_preference_does_not_block_new_device() {
    let mut c = snapshot(Some(battery(4, false, false)));
    c.device_id = "ps5:AA".into();
    c.family = ControllerFamily::DualSense;
    assert_eq!(
        choose_snapshot(&[c.clone()], "old-device", &HashMap::new()),
        Some(c)
    );
}
#[test]
fn changing_device_opens_new_session() {
    let mut s = VisibilitySession::new();
    s.observe(true, NOW);
    s.hide_manually();
    s.reset_for_device();
    assert!(s.observe(true, NOW + 1000) && s.visible);
}
#[test]
fn xbox_wired_external_power_unknown_percent() {
    let battery = read_xbox_battery(1, 0, NOW).unwrap();
    assert!(!battery.percentage_known);
    let mut s = snapshot(Some(battery));
    s.family = ControllerFamily::Xbox;
    s.transport = Some(Transport::Usb);
    let p = presentation(&s);
    assert_eq!(p.percentage, "—");
    assert_eq!(p.status, "外部供电");
}
macro_rules! xbox_bucket {
    ($name:ident,$level:expr,$percent:expr) => {
        #[test]
        fn $name() {
            assert_eq!(
                read_xbox_battery(2, $level, NOW).unwrap().percent(),
                $percent
            );
        }
    };
}
xbox_bucket!(xbox_empty, 0, 0);
xbox_bucket!(xbox_low, 1, 25);
xbox_bucket!(xbox_medium, 2, 50);
xbox_bucket!(xbox_full, 3, 100);
#[test]
fn xbox_unknown_not_guessed() {
    assert!(read_xbox_battery(255, 3, NOW).is_none());
}
#[test]
fn xbox_faces_triggers_guide_axes() {
    let i = decode_xbox_input(
        0xf440,
        255,
        255,
        -32768,
        32767,
        32767,
        -32768,
        "xinput:0",
        Transport::Wireless,
        NOW,
    );
    for key in [b::A, b::B, b::X, b::Y, b::HOME, b::LEFT_STICK, b::ZL, b::ZR] {
        assert!(i.down(key));
    }
    assert_eq!(
        [i.left_x, i.left_y, i.right_x, i.right_y],
        [-1., -1., 1., 1.]
    );
}

macro_rules! ps5_transport {
    ($name:ident,$bt:expr,$transport:expr) => {
        #[test]
        fn $name() {
            let r = ps5($bt, 0x15);
            let i = decode_dualsense_input(&r, "ps5:A", $transport, NOW).unwrap();
            let b = read_dualsense_battery(&r, NOW).unwrap();
            assert_eq!(i.buttons, 0);
            assert_eq!(i.left_x, 0.);
            assert_eq!(i.right_y, 0.);
            assert_eq!(b.percent(), 55);
            assert_eq!(b.power(), PowerState::Charging);
        }
    };
}
ps5_transport!(dualsense_usb_charging, false, Transport::Usb);
ps5_transport!(dualsense_bluetooth_charging, true, Transport::Bluetooth);
macro_rules! ps5_power {
    ($name:ident,$raw:expr,$power:expr) => {
        #[test]
        fn $name() {
            assert_eq!(
                read_dualsense_battery(&ps5(false, $raw), NOW)
                    .unwrap()
                    .power(),
                $power
            );
        }
    };
}
ps5_power!(dualsense_on_battery, 0x05, PowerState::OnBattery);
ps5_power!(dualsense_power_charging, 0x15, PowerState::Charging);
ps5_power!(dualsense_charged, 0x20, PowerState::Charged);
#[test]
fn dualsense_touchpad_mic_faces_simultaneous() {
    let mut r = ps5(false, 0x15);
    r[8] = 0xf8;
    r[9] = 0xf3;
    r[10] = 7;
    let i = decode_dualsense_input(&r, "ps5:A", Transport::Usb, NOW).unwrap();
    for key in [b::TOUCHPAD, b::MICROPHONE, b::HOME, b::A, b::B, b::X, b::Y] {
        assert!(i.down(key));
    }
}
#[test]
fn dualsense_simple_input_no_invented_battery() {
    let mut r = vec![0; 10];
    r[0] = 1;
    for v in &mut r[1..5] {
        *v = 128;
    }
    r[5] = 0x28;
    let i = decode_dualsense_input(&r, "ps5:A", Transport::Bluetooth, NOW).unwrap();
    assert!(i.down(b::A));
    assert!(read_dualsense_battery(&r, NOW).is_none());
}
#[test]
fn dualsense_bad_status_unknown_not_empty() {
    let b = read_dualsense_battery(&ps5(false, 0xf0), NOW).unwrap();
    assert!(!b.percentage_known);
    let mut s = snapshot(Some(b));
    s.family = ControllerFamily::DualSense;
    assert_eq!(presentation(&s).percentage, "—");
}
#[test]
fn dualsense_serial_unifies_usb_bluetooth() {
    assert_eq!(
        read_dualsense_address(&[9, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]).as_deref(),
        Some("112233445566")
    );
}
#[test]
fn truncated_enhanced_dualsense_rejected() {
    let mut r = vec![0; 13];
    r[0] = 0x31;
    assert!(decode_dualsense_input(&r, "ps5:A", Transport::Bluetooth, NOW).is_none());
    assert!(read_dualsense_battery(&r, NOW).is_none());
}
#[test]
fn accessibility_follows_family() {
    let mut s = snapshot(Some(battery(4, false, false)));
    s.family = ControllerFamily::DualSense;
    let p = presentation(&s);
    assert!(p.accessible_text.contains("PS5 DualSense"));
    assert!(!p.accessible_text.contains("Nintendo"));
}

// Migration-specific regressions: serialized contracts, transport ranking and input edges.
#[test]
fn serde_contract_uses_camel_case() {
    let mut s = snapshot(Some(battery(4, false, false)));
    s.family = ControllerFamily::DualSense;
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["family"], "dualSense");
    assert_eq!(v["transport"], "bluetooth");
    assert_eq!(v["deviceId"], "021122334455");
    assert!(v.get("device_id").is_none());
    assert_eq!(serde_json::from_value::<ControllerSnapshot>(v).unwrap(), s);
}
#[test]
fn unknown_battery_stays_unknown_after_serialization() {
    let b = read_xbox_battery(1, 0, NOW).unwrap();
    let b: BatteryReading = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
    assert!(!b.percentage_known);
    assert_eq!(b.power(), PowerState::ExternalPower);
}
#[test]
fn invalid_mac_variants_rejected() {
    for a in [
        "",
        "000000000000",
        "FFFFFFFFFFFF",
        "ffffffffffff",
        "123",
        "11223344556Z",
        "１１２２３３４４５５６６",
    ] {
        assert!(!is_address(a));
    }
}
#[test]
fn normal_address_case_and_prefix_preserved() {
    assert!(is_address("aabbccddeeff"));
    assert_eq!(
        device_id(ControllerFamily::SwitchPro, "aa:bb"),
        "switchpro:AA:BB"
    );
    assert_eq!(
        device_id(ControllerFamily::DualSense, "aabbccddeeff"),
        "dualsense:AABBCCDDEEFF"
    );
}
#[test]
fn report_minimum_lengths_never_panic() {
    for id in [1, 9, 0x21, 0x30, 0x31, 0x3f, 0x81] {
        for len in 0..80 {
            let mut r = vec![0; len];
            if len > 0 {
                r[0] = id;
            }
            let _ = read_switch_battery(&r, NOW);
            let _ = read_switch_identity(&r);
            let _ = decode_switch_input(&r, "", Transport::Usb, NOW);
            let _ = read_dualsense_battery(&r, NOW);
            let _ = decode_dualsense_input(&r, "", Transport::Bluetooth, NOW);
            let _ = read_dualsense_address(&r);
        }
    }
}
#[test]
fn stale_usb_cannot_override_fresh_bluetooth() {
    let a = snapshot(Some(battery(4, false, false)));
    let mut c = a.clone();
    c.transport = Some(Transport::Usb);
    c.is_stale = true;
    let v = merge_snapshots(&[c, a]);
    assert_eq!(v[0].transport, Some(Transport::Bluetooth));
}
#[test]
fn selection_merges_case_insensitively() {
    let a = snapshot(Some(battery(4, false, false)));
    let mut c = a.clone();
    c.device_id = c.device_id.to_lowercase();
    c.transport = Some(Transport::Usb);
    assert_eq!(merge_snapshots(&[a, c]).len(), 1);
}
#[test]
fn disconnected_and_empty_id_not_selectable() {
    let a = ControllerSnapshot::disconnected(NOW);
    let mut c = snapshot(None);
    c.device_id.clear();
    assert!(merge_snapshots(&[a, c]).is_empty());
}
#[test]
fn tied_auto_selection_is_deterministic() {
    let mut a = snapshot(None);
    a.device_id = "b".into();
    let mut c = a.clone();
    c.device_id = "a".into();
    assert_eq!(
        choose_snapshot(&[a, c], "", &HashMap::new())
            .unwrap()
            .device_id,
        "a"
    );
}
#[test]
fn empty_device_list_has_no_selection() {
    assert!(choose_snapshot(&[], "saved-device", &HashMap::new()).is_none());
}
#[test]
fn bounded_timestamps_do_not_underflow() {
    let mut s = VisibilitySession::new();
    s.observe(true, 10_000);
    s.observe(false, 10_000);
    s.observe(false, 1);
    assert!(s.has_session);
}
#[test]
fn manual_show_cancels_pending_hide() {
    let mut s = VisibilitySession::new();
    s.observe(true, NOW);
    s.observe(false, NOW + 1000);
    s.observe(false, NOW + 4000);
    s.show_manually();
    assert!(!s.tick(NOW + 8000));
    assert!(s.visible && !s.hidden_by_user);
}
#[test]
fn dualsense_padded_simple_never_invents_battery() {
    let mut r = vec![0; 78];
    r[0] = 1;
    r[5] = 8;
    for v in &mut r[1..5] {
        *v = 128;
    }
    assert!(decode_dualsense_input(&r, "", Transport::Bluetooth, NOW).is_some());
    assert!(read_dualsense_battery(&r, NOW).is_none());
}
#[test]
fn dualsense_invalid_level_stays_unknown() {
    assert!(
        !read_dualsense_battery(&ps5(true, 0x0f), NOW)
            .unwrap()
            .percentage_known
    );
}
#[test]
fn dualsense_full_charge_percent_100() {
    assert_eq!(
        read_dualsense_battery(&ps5(true, 0x20), NOW)
            .unwrap()
            .percent(),
        100
    );
}
#[test]
fn dualsense_stick_endpoints_screen_directions() {
    let mut r = ps5(false, 5);
    r[1] = 0;
    r[2] = 0;
    r[3] = 255;
    r[4] = 255;
    let i = decode_dualsense_input(&r, "", Transport::Usb, NOW).unwrap();
    assert_eq!(
        [i.left_x, i.left_y, i.right_x, i.right_y],
        [-1., -1., 1., 1.]
    );
}
#[test]
fn xbox_invalid_level_not_empty() {
    assert!(read_xbox_battery(2, 4, NOW).is_none());
    assert!(read_xbox_battery(3, 255, NOW).is_none());
}
#[test]
fn xbox_rechargeable_level_supported() {
    assert_eq!(read_xbox_battery(3, 2, NOW).unwrap().percent(), 50);
}
#[test]
fn xbox_trigger_threshold_is_strict() {
    let i = decode_xbox_input(0, 30, 31, 0, 0, 0, 0, "", Transport::Usb, NOW);
    assert!(!i.down(b::ZL));
    assert!(i.down(b::ZR));
}
#[test]
fn switch_full_sticks_clamp_outside_calibrated_range() {
    let i = decode(&full(0, 0, 0, [0, 4095, 4095, 0]));
    assert_eq!(
        [i.left_x, i.left_y, i.right_x, i.right_y],
        [-1., -1., 1., 1.]
    );
}
#[test]
fn neutral_input_has_no_device_or_pose() {
    let i = ControllerInput::neutral();
    assert!(i.device_id.is_empty());
    assert_eq!(i.buttons, 0);
    assert_eq!([i.left_x, i.left_y, i.right_x, i.right_y], [0.; 4]);
}
#[test]
fn input_identity_survives_json() {
    let i = decode(&simple(2, 0, 8, [32768; 4]));
    let v = serde_json::to_value(&i).unwrap();
    assert_eq!(v["reportId"], 0x3f);
    assert_eq!(v["buttons"], b::A);
    assert_eq!(serde_json::from_value::<ControllerInput>(v).unwrap(), i);
}
#[test]
fn malformed_power_has_muted_tone() {
    let mut s = snapshot(read_dualsense_battery(&ps5(true, 0xf0), NOW));
    s.family = ControllerFamily::DualSense;
    assert_eq!(presentation(&s).tone, "muted");
}
#[test]
fn unknown_old_reading_has_no_filled_segments() {
    let mut s = snapshot(read_xbox_battery(1, 0, NOW));
    s.is_stale = true;
    let p = presentation(&s);
    assert_eq!(p.percentage, "—");
    assert_eq!(p.filled_segments, 0);
    assert!(!p.animate_charging);
}
