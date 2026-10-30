export type ControllerFamily = 'switchPro' | 'xbox' | 'dualSense';
export type Transport = 'bluetooth' | 'usb' | 'wireless';
export type ConnectionState = 'disconnected' | 'reading' | 'connected' | 'unavailable';
export type PowerState = 'unknown' | 'onBattery' | 'charging' | 'charged' | 'externalPower';

export interface BatteryReading {
  level: number;
  charging: boolean;
  externalPower: boolean;
  timestamp: number;
  percentOverride: number | null;
  percentageKnown: boolean;
  reportedPower: PowerState | null;
}
export interface ControllerSnapshot {
  deviceId: string;
  transport: Transport | null;
  connection: ConnectionState;
  battery: BatteryReading | null;
  timestamp: number;
  detail: string | null;
  isStale: boolean;
  family: ControllerFamily;
  name: string | null;
}
export interface ControllerInput {
  deviceId: string;
  transport: Transport;
  buttons: number;
  leftX: number;
  leftY: number;
  rightX: number;
  rightY: number;
  timestamp: number;
  reportId: number;
}
export interface DashboardState {
  snapshot: ControllerSnapshot;
  devices: ControllerSnapshot[];
  preferredDeviceId: string;
  autostart: boolean;
  reducedMotion: boolean;
  expanded: boolean;
  visible: boolean;
  sequence: number;
}
export interface InputUpdate { sequence: number; input: ControllerInput | null }
export const buttons = {
  A: 1 << 0, B: 1 << 1, X: 1 << 2, Y: 1 << 3,
  Up: 1 << 4, Down: 1 << 5, Left: 1 << 6, Right: 1 << 7,
  L: 1 << 8, R: 1 << 9, ZL: 1 << 10, ZR: 1 << 11,
  Minus: 1 << 12, Plus: 1 << 13, Home: 1 << 14, Capture: 1 << 15,
  LeftStick: 1 << 16, RightStick: 1 << 17, Touchpad: 1 << 18, Microphone: 1 << 19,
} as const;
export const familyNames: Record<ControllerFamily, string> = {
  switchPro: 'Nintendo Switch Pro', xbox: 'Xbox Controller', dualSense: 'PS5 DualSense',
};
export const initialState: DashboardState = {
  snapshot: { deviceId: '', transport: null, connection: 'disconnected', battery: null,
    timestamp: 0, detail: null, isStale: false, family: 'switchPro', name: null },
  devices: [], preferredDeviceId: '', autostart: false, reducedMotion: false,
  expanded: false, visible: false, sequence: 0,
};
