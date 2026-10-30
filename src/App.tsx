import { useCallback, useEffect, useRef, useState } from 'react';
import type { PointerEvent } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ControllerCanvas } from './ControllerCanvas';
import type { ControllerCanvasHandle } from './ControllerCanvas';
import { demoInput, demoState } from './demo';
import { InputFrameGate, present } from './presentation';
import { familyNames, initialState } from './types';
import type { ControllerFamily, DashboardState, InputUpdate, Transport } from './types';
import styles from './Dashboard.module.css';

const native = isTauri();
const query = new URLSearchParams(location.search);
const demoFamily = query.get('demo');
const demo = !native && (demoFamily === 'switchPro' || demoFamily === 'xbox' || demoFamily === 'dualSense');
const demoInitial = demo ? demoState(demoFamily as ControllerFamily, query.get('state') || 'battery', query.get('expanded') === '1') : initialState;
const familyLabels: Record<ControllerFamily,string> = {switchPro:'NINTENDO SWITCH',xbox:'XBOX',dualSense:'PLAYSTATION 5'};

function Chevron({up=false}:{up?:boolean}) {return <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true" style={{transform:up?'rotate(180deg)':undefined}}><path d="m4 6 4 4 4-4" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>;}
function ControllerIcon(){return <svg width="18" height="18" viewBox="0 0 24 24" fill="none" aria-hidden="true"><path d="M8 6h8c3 0 4 3 5 7 .8 4-.8 6-3 4l-3-3H9l-3 3c-2.2 2-3.8 0-3-4 1-4 2-7 5-7Z" stroke="currentColor" strokeWidth="1.6"/><path d="M8 8.5v5M5.5 11h5M16 9.6h.01M18.2 11.8h.01" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round"/></svg>;}
function ConnectionIcon({transport}:{transport:Transport|null}) {
  if(transport==='usb')return <svg width="10" height="12" viewBox="0 0 10 12" aria-hidden="true"><path d="M5 11V1m0 0L3 3m2-2 2 2M5 8 2 6V4m3 2 3-2V3" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round"/></svg>;
  if(transport==='bluetooth')return <svg width="9" height="12" viewBox="0 0 9 12" aria-hidden="true"><path d="m2 3 5 5-3 3V1l3 3-5 5" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round"/></svg>;
  return <svg width="11" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M1 4a8 8 0 0 1 10 0M3 6a5 5 0 0 1 6 0M5 8a2 2 0 0 1 2 0" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round"/><circle cx="6" cy="10" r=".7" fill="currentColor"/></svg>;
}

function Battery({percent,family,charging}:{percent:number|null;family:ControllerFamily;charging:boolean}) {
  const total=family==='dualSense'?3:4;
  const filled=percent===null?0:Math.ceil(percent*total/100);
  return <span className={`${styles.battery} ${charging?styles.charging:''}`} aria-hidden="true">
    <svg width="39" height="23" viewBox="0 0 39 23"><rect x="1" y="3" width="32" height="17" rx="2.5" fill="none" stroke="currentColor" strokeWidth="1.8"/><rect x="35" y="8" width="3" height="7" rx="1" fill="currentColor"/>{Array.from({length:total},(_,i)=><rect key={i} x={4+i*26/total} y="6" width={26/total-2} height="11" rx=".5" fill="currentColor" opacity={i<filled?1:.14}/>)}</svg>
    {charging&&<svg className={styles.bolt} width="11" height="16" viewBox="0 0 11 16"><path d="M6.4 0 0 9h4.4l-.8 7L11 6H6.2Z" fill="currentColor"/></svg>}
  </span>;
}

export default function App(){
  const [state,setState]=useState<DashboardState>(demoInitial);
  const [error,setError]=useState('');
  const stateRef=useRef(state);
  const gate=useRef(new InputFrameGate());
  const canvas=useRef<ControllerCanvasHandle>(null);
  const getInput=useCallback(()=>gate.current.current(),[]);
  const acceptState=useCallback((next:DashboardState)=>{
    if(next.sequence<stateRef.current.sequence)return;
    gate.current.applyState(next);
    stateRef.current=next;setState(next);canvas.current?.redraw();
  },[]);
  useEffect(()=>{
    gate.current.applyState(stateRef.current);
    if(!native)return;
    let cancelled=false;
    const unlisten: Array<()=>void>=[];
    const add=(remove:()=>void)=>cancelled?remove():unlisten.push(remove);
    void Promise.all([
      listen<DashboardState>('dashboard-state',event=>acceptState(event.payload)).then(add),
      listen<InputUpdate>('controller-input',event=>{
        if(gate.current.accept(event.payload))canvas.current?.redraw();
      }).then(add),
    ]).then(()=>cancelled?undefined:invoke<DashboardState>('get_dashboard_state').then(acceptState)).catch(()=>setError('连接看板服务失败，请重新打开程序'));
    return()=>{cancelled=true;for(const remove of unlisten)remove();};
  },[acceptState]);
  useEffect(()=>{
    if(!demo)return;
    if(query.get('state')==='controls'){
      gate.current.accept({sequence:1,input:demoInput(demoFamily as ControllerFamily,750)});canvas.current?.redraw();
    }
    if(query.get('animate')!=='1')return;
    let sequence=1;
    const started=performance.now();
    const timer=setInterval(()=>{
      gate.current.accept({sequence:++sequence,input:demoInput(demoFamily as ControllerFamily,performance.now()-started)});canvas.current?.redraw();
    },32);
    return()=>clearInterval(timer);
  },[]);
  const command=async(name:string,args?:Record<string,unknown>)=>{
    setError('');
    if(demo){
      const next={...stateRef.current,sequence:stateRef.current.sequence+1};
      if(name==='set_panel_mode')next.expanded=!!args?.expanded;
      if(name==='set_autostart')next.autostart=!!args?.enabled;
      if(name==='set_reduced_motion')next.reducedMotion=!!args?.enabled;
      if(name==='select_controller')next.preferredDeviceId=String(args?.deviceId||'');
      acceptState(next);return;
    }
    if(!native){setError('请从桌面应用打开看板');return;}
    try{await invoke(name,args);}catch{setError('操作未完成，请稍后重试');}
  };
  const view=present(state.snapshot);
  const connection=state.snapshot.connection;
  const connected=connection!=='disconnected';
  const sampled=state.snapshot.battery?.timestamp;
  const sampleText=sampled?new Date(sampled).toLocaleTimeString('zh-CN',{hour12:false}):'尚无读数';
  const deviceOffline=state.preferredDeviceId&&!state.devices.some(d=>d.deviceId===state.preferredDeviceId);
  const beginDrag=(event:PointerEvent)=>{
    if(event.button!==0||(event.target as HTMLElement).closest('button,select,input,label'))return;
    if(native)void getCurrentWindow().startDragging().catch(()=>{});
  };
  return <main className={`${styles.shell} ${styles[state.snapshot.family]} ${state.expanded?styles.expanded:''} ${state.reducedMotion?styles.reducedMotion:''}`} data-theme={state.snapshot.family} data-demo={demo||undefined}>
    <section className={styles.card} aria-label={`${view.name}，${view.status}，${view.percent===null?'电量未知':`约 ${view.percent}%`}`}>
      <header className={styles.header} onPointerDown={beginDrag}>
        <span className={styles.heading}><ControllerIcon/><span>控制器</span></span>
        <div className={styles.headerActions}>
          <span className={`${styles.connection} ${!connected?styles.offline:''}`}><ConnectionIcon transport={state.snapshot.transport}/>{view.transport}</span>
          <button className={styles.close} onClick={()=>void command('hide_panel')} title="隐藏到系统托盘" aria-label="隐藏到系统托盘">×</button>
        </div>
      </header>
      <div className={`${styles.hero} ${view.dimmed?styles.dimmed:''}`} onPointerDown={beginDrag}>
        <div className={styles.orbit}/>
        <ControllerCanvas ref={canvas} family={state.snapshot.family} visible={state.visible||demo} dimmed={view.dimmed} getInput={getInput}/>
        <div className={styles.productName}>{view.name}</div>
      </div>
      <div className={`${styles.power} ${view.tone==='low'?styles.low:view.tone==='muted'?styles.muted:''}`}>
        <div className={styles.powerReading}><Battery percent={view.percent} family={state.snapshot.family} charging={view.charging}/><span className={styles.percentage}>{view.percent!==null&&<span className={styles.approx}>约</span>}{view.percentage}</span></div>
        <div className={styles.powerStatus}><i/>{view.status}</div>
      </div>
      {state.expanded&&<section className={styles.details} aria-label="设备与设置">
        <div className={styles.deviceRow}><label htmlFor="controller-select">当前设备</label><select id="controller-select" value={state.preferredDeviceId} onChange={event=>void command('select_controller',{deviceId:event.target.value})}>
          <option value="">自动选择最近连接</option>
          {deviceOffline&&<option value={state.preferredDeviceId}>优先设备暂未连接</option>}
          {state.devices.map(device=><option key={device.deviceId} value={device.deviceId}>{device.name||familyNames[device.family]} · {device.deviceId.slice(-6)}</option>)}
        </select></div>
        <div className={styles.readingRow}><span>最近采样 <strong>{sampleText}</strong></span><span className={state.snapshot.isStale?styles.stale:''}>{state.snapshot.isStale?'等待刷新':connection==='connected'?'已连接':connection==='reading'?'读取中':connection==='unavailable'?'等待重试':'已断开'}</span></div>
        <div className={styles.settingsRow}><label htmlFor="autostart">登录时启动</label><button id="autostart" className={`${styles.toggle} ${state.autostart?styles.checked:''}`} role="switch" aria-checked={state.autostart} onClick={()=>void command('set_autostart',{enabled:!state.autostart})}><span/></button></div>
        <div className={styles.settingsRow}><label htmlFor="motion">减少动态效果</label><button id="motion" className={`${styles.toggle} ${state.reducedMotion?styles.checked:''}`} role="switch" aria-checked={state.reducedMotion} onClick={()=>void command('set_reduced_motion',{enabled:!state.reducedMotion})}><span/></button></div>
        <p className={styles.detailHint} title={state.snapshot.detail||undefined}>{state.snapshot.detail||(view.percent===null&&connected?'设备尚未提供可用电量':'电量按手柄报告估算，按键与摇杆实时同步')}</p>
      </section>}
      <footer className={styles.footer} onPointerDown={beginDrag}>
        <span className={`${styles.familyLabel} ${error?styles.error:''}`} title={error||undefined}>{error||familyLabels[state.snapshot.family]}</span>
        <button className={styles.expandButton} onClick={()=>void command('set_panel_mode',{expanded:!state.expanded})} aria-expanded={state.expanded}>{state.expanded?'收起':'设备与设置'}<Chevron up={state.expanded}/></button>
      </footer>
    </section>
  </main>;
}
