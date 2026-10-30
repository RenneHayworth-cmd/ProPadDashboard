import { buttons, familyNames } from './types';
import type { ControllerFamily, DashboardState, ControllerInput } from './types';

export function demoState(family: ControllerFamily, state = 'battery', expanded = false): DashboardState {
  const now = Date.now();
  const snapshot: DashboardState['snapshot'] = {
    deviceId: `demo:${family}`, family, name: familyNames[family], transport: 'bluetooth',
    connection: 'connected', detail: null, isStale: false, timestamp: now,
    battery: { level: 6, charging: false, externalPower: false, timestamp: now,
      percentOverride: null, percentageKnown: true, reportedPower: null },
  };
  if (state === 'charging') { snapshot.transport = 'usb'; snapshot.battery!.charging = true; snapshot.battery!.externalPower = true; snapshot.battery!.level = 4; }
  if (state === 'full') { snapshot.transport = 'usb'; snapshot.battery!.level = 8; snapshot.battery!.reportedPower = 'charged'; snapshot.battery!.externalPower = true; }
  if (state === 'low') snapshot.battery!.level = 2;
  if (state === 'unknown') { snapshot.battery = null; snapshot.detail = '设备没有提供电量信息'; }
  if (state === 'wired') { snapshot.transport = 'usb'; snapshot.battery!.percentageKnown = false; snapshot.battery!.reportedPower = 'externalPower'; }
  if (state === 'stale') { snapshot.isStale = true; snapshot.battery!.timestamp -= 30000; }
  if (state === 'reading') { snapshot.connection = 'reading'; snapshot.battery = null; }
  if (state === 'disconnected') { snapshot.connection = 'disconnected'; snapshot.battery = null; snapshot.transport = null; }
  return { snapshot, devices: snapshot.connection === 'disconnected' ? [] : [snapshot],
    preferredDeviceId: '', autostart: true, reducedMotion: false, expanded, visible: true, sequence: 1 };
}
export function demoInput(family: ControllerFamily, elapsed: number): ControllerInput {
  return { deviceId: `demo:${family}`, transport: 'bluetooth', buttons: buttons.A | buttons.L | buttons.Up,
    leftX: Math.sin(elapsed / 900) * .7, leftY: Math.cos(elapsed / 900) * .7,
    rightX: .4, rightY: -.35, reportId: 0x30, timestamp: Date.now() };
}
