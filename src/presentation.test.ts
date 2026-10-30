import { describe, expect, it } from 'vitest';
import { demoState } from './demo';
import { InputFrameGate, present } from './presentation';
import type { ControllerInput } from './types';

describe('battery presentation preserves reported certainty',()=>{
  it.each([0,2,4,6,8])('maps Switch level %i without inventing precision',level=>{
    const snapshot=demoState('switchPro').snapshot;snapshot.battery!.level=level;
    const result=present(snapshot);expect(result.percent).toBe(level*100/8);expect(result.segments).toBe(level/2);
  });
  it('never shows read failure as zero',()=>{
    const result=present(demoState('switchPro','unknown').snapshot);
    expect(result.percent).toBeNull();expect(result.percentage).toBe('—');expect(result.status).toBe('电量暂不可读');
  });
  it('keeps an explicitly reported zero as zero',()=>{
    const snapshot=demoState('switchPro').snapshot;snapshot.battery!.level=0;
    expect(present(snapshot)).toMatchObject({percent:0,status:'电量偏低',tone:'low'});
  });
  it('does not infer battery percentage from a USB cable',()=>{
    expect(present(demoState('xbox','wired').snapshot)).toMatchObject({percent:null,status:'外部供电',segments:0});
  });
  it('preserves actual battery while charging',()=>{
    expect(present(demoState('switchPro','charging').snapshot)).toMatchObject({percent:50,charging:true,status:'正在充电',segments:2});
  });
  it('stops the charging animation when full',()=>{
    expect(present(demoState('dualSense','full').snapshot)).toMatchObject({percent:100,charging:false,status:'已充满'});
  });
  it('does not mark a full wireless battery as plugged in',()=>{
    const snapshot=demoState('switchPro').snapshot;snapshot.battery!.level=8;
    expect(present(snapshot)).toMatchObject({percent:100,status:'电池供电',power:'onBattery'});
  });
  it('marks stale charge and keeps the last known value without animation',()=>{
    const snapshot=demoState('dualSense','charging').snapshot;snapshot.isStale=true;
    expect(present(snapshot)).toMatchObject({percent:50,status:'读数未更新',tone:'muted',charging:false});
  });
  it('does not turn stale unknown wired battery into a number',()=>{
    const snapshot=demoState('xbox','wired').snapshot;snapshot.isStale=true;
    expect(present(snapshot)).toMatchObject({percent:null,percentage:'—',tone:'muted'});
  });
  it('shows the reading state independently of failure',()=>{
    expect(present(demoState('switchPro','reading').snapshot)).toMatchObject({percent:null,percentage:'···',status:'正在读取电量'});
  });
  it('disconnects without reverting the family or preserving a stale percentage',()=>{
    const snapshot=demoState('dualSense','full').snapshot;snapshot.connection='disconnected';
    expect(present(snapshot)).toMatchObject({name:'PS5 DualSense',percent:null,status:'手柄已断开',dimmed:true});
  });
  it('prefers a reported percentage and power state over legacy buckets',()=>{
    const snapshot=demoState('dualSense').snapshot;snapshot.battery!.percentOverride=65;snapshot.battery!.reportedPower='onBattery';
    expect(present(snapshot)).toMatchObject({percent:65,status:'电池供电',segments:3});
  });
  it('does not render NaN as a battery number',()=>{
    const snapshot=demoState('xbox').snapshot;snapshot.battery!.percentOverride=NaN;
    expect(present(snapshot).percent).toBeNull();
  });
  it('shows low battery at 25 percent but uses the charging state while charging',()=>{
    const snapshot=demoState('switchPro','low').snapshot;snapshot.battery!.charging=true;
    expect(present(snapshot)).toMatchObject({tone:'normal',status:'正在充电'});
  });
});

describe('input frame gate',()=>{
  const input=(deviceId:string,buttons=1):ControllerInput=>({deviceId,buttons,transport:'bluetooth',leftX:.5,leftY:0,rightX:0,rightY:0,timestamp:1,reportId:0x30});
  it.each(['reading','unavailable'] as const)('keeps live controls working while battery is %s',connection=>{
    const state=demoState('switchPro','unknown');state.snapshot.connection=connection;
    const gate=new InputFrameGate();gate.applyState(state);
    expect(gate.accept({sequence:1,input:input(state.snapshot.deviceId)})).toBe(true);
    expect(gate.current()?.leftX).toBe(.5);
    expect(present(state.snapshot).percent).toBeNull();
  });
  it('clears held controls and rejects late frames on disconnect or hide',()=>{
    const state=demoState('switchPro');const gate=new InputFrameGate();gate.applyState(state);
    gate.accept({sequence:1,input:input(state.snapshot.deviceId)});
    state.visible=false;gate.applyState(state);expect(gate.current()).toBeNull();
    expect(gate.accept({sequence:2,input:input(state.snapshot.deviceId)})).toBe(false);
    state.visible=true;state.snapshot.connection='disconnected';gate.applyState(state);
    expect(gate.accept({sequence:3,input:input(state.snapshot.deviceId)})).toBe(false);
  });
  it('drops reordered and duplicate input frames',()=>{
    const gate=new InputFrameGate();gate.select('a');
    expect(gate.accept({sequence:3,input:input('a',1)})).toBe(true);
    expect(gate.accept({sequence:2,input:input('a',2)})).toBe(false);
    expect(gate.accept({sequence:3,input:input('a',2)})).toBe(false);
    expect(gate.current()?.buttons).toBe(1);
  });
  it('neutralizes the old pose immediately on device selection',()=>{
    const gate=new InputFrameGate();gate.select('a');gate.accept({sequence:1,input:input('a')});
    gate.select('b');expect(gate.current()).toBeNull();
    expect(gate.accept({sequence:2,input:input('a')})).toBe(false);
    expect(gate.accept({sequence:3,input:input('b')})).toBe(true);
    expect(gate.current()?.deviceId).toBe('b');
  });
  it('accepts explicit neutral frames and clears input on hide',()=>{
    const gate=new InputFrameGate();gate.select('a');gate.accept({sequence:1,input:input('a')});
    expect(gate.accept({sequence:2,input:null})).toBe(true);expect(gate.current()).toBeNull();
    gate.accept({sequence:3,input:input('a')});gate.clear();expect(gate.current()).toBeNull();
  });
  it('keeps sequence ordering across selection changes',()=>{
    const gate=new InputFrameGate();gate.select('a');gate.accept({sequence:5,input:input('a')});gate.select('b');
    expect(gate.accept({sequence:4,input:input('b')})).toBe(false);
    expect(gate.accept({sequence:6,input:input('b')})).toBe(true);
  });
});
