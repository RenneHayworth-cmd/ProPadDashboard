import { familyNames } from './types';
import type { ControllerSnapshot, InputUpdate, ControllerInput, PowerState, DashboardState } from './types';

export interface Presentation {
  name: string; percent: number | null; percentage: string; status: string;
  transport: string; segments: number; tone: 'normal' | 'low' | 'muted';
  charging: boolean; dimmed: boolean; power: PowerState;
}

export function present(snapshot: ControllerSnapshot): Presentation {
  const battery = snapshot.battery;
  const rawPercent = battery?.percentageKnown ? (battery.percentOverride ?? battery.level * 100 / 8) : null;
  const percent = rawPercent !== null && Number.isFinite(rawPercent) ? Math.max(0, Math.min(100, Math.round(rawPercent))) : null;
  const power = battery?.reportedPower ?? (battery?.charging
    ? (battery.level === 8 ? 'charged' : 'charging')
    : battery?.externalPower ? (battery.level === 8 ? 'charged' : 'externalPower') : battery ? 'onBattery' : 'unknown');
  const result: Presentation = {
    name: snapshot.name || familyNames[snapshot.family], percent,
    percentage: percent === null ? '—' : `${percent}%`,
    status: '电量暂不可读',
    transport: snapshot.transport === 'bluetooth' ? '蓝牙' : snapshot.transport === 'usb' ? 'USB' : snapshot.transport === 'wireless' ? '无线' : '离线',
    segments: percent === null ? 0 : Math.ceil(percent / 25),
    tone: 'normal', charging: false, dimmed: false, power,
  };
  if (snapshot.connection === 'disconnected') {
    return { ...result, percent: null, percentage: '—', status: '手柄已断开', transport: '离线', segments: 0, tone: 'muted', dimmed: true, power: 'unknown' };
  }
  if (snapshot.connection === 'reading' && !battery) {
    return { ...result, percentage: '···', status: '正在读取电量', tone: 'muted' };
  }
  if (snapshot.connection === 'unavailable' || snapshot.isStale) {
    return { ...result, status: battery ? '读数未更新' : '电量暂不可读', tone: 'muted' };
  }
  if (!battery) return { ...result, status: '电量暂不可读', tone: 'muted' };
  const low = percent !== null && percent <= 25 && !battery.charging;
  return { ...result, status: power === 'charging' ? '正在充电' : power === 'charged' ? '已充满'
    : power === 'externalPower' ? '外部供电' : power === 'onBattery' ? low ? '电量偏低' : '电池供电' : '电量暂不可读',
    tone: low ? 'low' : power === 'unknown' ? 'muted' : 'normal', charging: power === 'charging' };
}

// Input updates and snapshots have independent sequence counters. Never compare them.
export class InputFrameGate {
  private sequence = -1;
  private deviceId = '';
  private input: ControllerInput | null = null;
  private accepting = true;
  applyState(state: Pick<DashboardState, 'snapshot' | 'visible'>) {
    this.select(state.snapshot.deviceId);
    this.accepting = state.visible && state.snapshot.connection !== 'disconnected';
    if (!this.accepting) this.clear();
  }
  select(deviceId: string) {
    if (this.deviceId !== deviceId) { this.deviceId = deviceId; this.input = null; }
  }
  accept(update: InputUpdate): boolean {
    if (!this.accepting) return false;
    if (update.sequence <= this.sequence) return false;
    this.sequence = update.sequence;
    if (update.input && update.input.deviceId !== this.deviceId) return false;
    this.input = update.input;
    return true;
  }
  clear() { this.input = null; }
  current() { return this.input; }
}
