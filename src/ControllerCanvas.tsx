import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef } from 'react';
import { buttons } from './types';
import type { ControllerFamily, ControllerInput } from './types';
import styles from './Dashboard.module.css';

type Stick = { x: number; y: number; radius: number };
type Spot = { button: number; x: number; y: number; w: number; h: number; round: boolean; label?: string };
type Profile = { file: string; clip: string; left: Stick; right: Stick; spots: Spot[] };
const circle = (button: number, x: number, y: number, r: number): Spot => ({ button, x: x-r, y: y-r, w: r*2, h: r*2, round: true });
const box = (button: number, x: number, y: number, w: number, h: number, label?: string): Spot => ({ button, x, y, w, h, round: false, label });
const shoulder = (family: ControllerFamily) => {
  const labels = family === 'xbox' ? ['LB','LT','RB','RT'] : family === 'dualSense' ? ['L1','L2','R1','R2'] : ['L','ZL','R','ZR'];
  return [box(buttons.L,252,123,132,30,labels[0]), box(buttons.ZL,285,77,132,30,labels[1]),
    box(buttons.R,1055,123,132,30,labels[2]), box(buttons.ZR,1033,77,132,30,labels[3])];
};
const profiles: Record<ControllerFamily, Profile> = {
  switchPro: {
    file: 'switch-pro-ai.png', left: {x:321,y:341,radius:87}, right:{x:920,y:571,radius:87},
    clip:'M12,812 C36,617 61,449 92,360 C94,270 120,217 170,167 C190,103 246,66 330,66 C385,66 425,85 465,101 L985,101 C1023,82 1065,63 1110,65 C1195,64 1251,101 1274,168 C1321,216 1340,273 1353,360 C1388,480 1434,710 1435,826 C1439,930 1402,1008 1349,1027 C1303,1040 1251,1000 1217,941 L1107,770 C1093,752 1080,743 1058,740 L391,740 C370,747 351,766 337,789 L230,953 C200,1001 154,1037 111,1025 C52,1010 8,947 12,812 Z',
    spots:[circle(buttons.A,1216,347,49),circle(buttons.B,1116,443,49),circle(buttons.X,1117,253,49),circle(buttons.Y,1018,348,49),
      circle(buttons.Minus,547,263,31),circle(buttons.Plus,902,263,31),circle(buttons.Home,822,373,35),box(buttons.Capture,596,340,59,59),
      box(buttons.Up,469,444,75,75),box(buttons.Down,469,582,75,75),box(buttons.Left,402,510,75,75),box(buttons.Right,544,510,75,75),...shoulder('switchPro')],
  },
  xbox: {
    file:'xbox-ai.png',left:{x:350,y:360,radius:81},right:{x:929,y:584,radius:80},
    clip:'M1,815 C20,626 59,431 109,313 C129,250 153,204 191,168 C203,105 253,63 344,64 C414,61 436,79 473,113 L974,113 C1012,78 1051,62 1106,64 C1196,66 1247,106 1260,168 C1301,217 1325,273 1346,342 C1383,490 1430,677 1446,836 C1451,940 1415,1020 1357,1039 C1306,1056 1265,1022 1224,968 L1097,812 C1080,794 1050,784 1024,784 L424,784 C397,786 370,796 352,813 L224,971 C181,1024 142,1056 92,1038 C31,1016 -3,929 1,815 Z',
    spots:[circle(buttons.A,1121,455,48),circle(buttons.B,1220,354,48),circle(buttons.X,1018,354,48),circle(buttons.Y,1120,255,48),
      circle(buttons.Home,722,219,57),circle(buttons.Minus,612,355,34),circle(buttons.Plus,832,355,34),box(buttons.Capture,680,409,85,47),
      box(buttons.Up,491,491,75,77),box(buttons.Down,491,621,75,77),box(buttons.Left,426,555,77,75),box(buttons.Right,554,555,77,75),...shoulder('xbox')],
  },
  dualSense: {
    file:'dualsense-ai.png',left:{x:491,y:550,radius:84},right:{x:956,y:550,radius:84},
    clip:'M9,788 C15,624 45,459 96,313 C124,240 148,190 178,157 L188,121 C208,91 251,78 299,77 C339,75 361,87 388,108 L438,118 C612,85 832,85 1009,118 L1061,108 C1089,87 1117,75 1155,77 C1204,79 1244,95 1261,123 L1272,158 C1322,214 1351,292 1380,391 C1414,512 1439,675 1437,813 C1436,910 1414,984 1374,1014 C1339,1042 1298,1027 1276,1008 C1236,964 1207,875 1169,793 L1127,706 C1104,698 1081,699 1059,700 L389,700 C361,698 340,700 321,707 L278,798 C241,882 211,966 172,1007 C143,1034 107,1037 73,1016 C27,988 8,906 9,788 Z',
    spots:[circle(buttons.A,1173,454,46),circle(buttons.B,1271,355,46),circle(buttons.X,1077,355,46),circle(buttons.Y,1173,255,46),
      circle(buttons.Home,721,532,43),box(buttons.Touchpad,440,110,568,286),box(buttons.Minus,369,166,34,64),box(buttons.Plus,1043,166,34,64),
      box(buttons.Microphone,688,605,67,23),box(buttons.Up,238,237,69,81),box(buttons.Down,238,387,69,81),box(buttons.Left,157,314,87,75),box(buttons.Right,296,314,87,75),...shoulder('dualSense')],
  },
};

function stick(ctx: CanvasRenderingContext2D, image: HTMLImageElement, at: Stick, x: number, y: number, pressed: boolean, accent: string) {
  const length = Math.hypot(x,y);
  if (length>1) { x/=length; y/=length; }
  ctx.fillStyle='#172025'; ctx.beginPath(); ctx.arc(at.x,at.y,at.radius+15,0,Math.PI*2);ctx.fill();
  const cx=at.x+x*at.radius*.3,cy=at.y+y*at.radius*.3+(pressed?3:0),r=at.radius*(pressed?.96:1);
  ctx.save();ctx.beginPath();ctx.arc(cx,cy,r,0,Math.PI*2);ctx.clip();
  ctx.drawImage(image,at.x-at.radius,at.y-at.radius,at.radius*2,at.radius*2,cx-r,cy-r,r*2,r*2);ctx.restore();
  if (pressed || length>.025) {
    ctx.beginPath();ctx.arc(cx,cy,r,0,Math.PI*2);ctx.strokeStyle=accent;ctx.lineWidth=pressed?8:5;ctx.stroke();
    if(pressed){ctx.globalAlpha=.25;ctx.fillStyle=accent;ctx.fill();ctx.globalAlpha=1;}
  }
}

export interface ControllerCanvasHandle { redraw(): void }
interface Props { family: ControllerFamily; visible: boolean; dimmed: boolean; getInput(): ControllerInput | null }
export const ControllerCanvas = forwardRef<ControllerCanvasHandle, Props>(function ControllerCanvas({family,visible,dimmed,getInput},ref) {
  const canvasRef=useRef<HTMLCanvasElement>(null);
  const imageRef=useRef<HTMLImageElement|null>(null);
  const layerRef=useRef<HTMLCanvasElement|null>(null);
  const frameRef=useRef(0);
  const draw=useCallback(() => {
    frameRef.current=0;
    const canvas=canvasRef.current,image=imageRef.current;
    if(!canvas || !visible || !image?.complete || !image.naturalWidth) return;
    const rect=canvas.getBoundingClientRect(), dpr=window.devicePixelRatio||1;
    const width=Math.round(rect.width*dpr),height=Math.round(rect.height*dpr);
    if(!width||!height)return;
    if(canvas.width!==width||canvas.height!==height){canvas.width=width;canvas.height=height;}
    const ctx=canvas.getContext('2d');if(!ctx)return;
    const profile=profiles[family],scale=Math.min(width/1448,height/1000);
    const x=(width-1448*scale)/2,y=(height-1000*scale)/2-55*scale;
    const path=new Path2D(profile.clip);
    const apply=(context:CanvasRenderingContext2D)=>{context.translate(x,y);context.scale(scale,scale);context.clip(path);};
    ctx.clearRect(0,0,width,height);ctx.globalAlpha=dimmed?.3:1;
    ctx.save();apply(ctx);ctx.drawImage(image,0,0,1448,1086);ctx.restore();
    const pose=dimmed?null:getInput();
    if(!pose || pose.deviceId==='')return;
    const layer=layerRef.current??=document.createElement('canvas');
    if(layer.width!==width||layer.height!==height){layer.width=width;layer.height=height;}
    const overlay=layer.getContext('2d');if(!overlay)return;
    overlay.clearRect(0,0,width,height);overlay.save();apply(overlay);
    const accent=getComputedStyle(canvas).getPropertyValue('--accent').trim()||'#00a889';
    stick(overlay,image,profile.left,pose.leftX,pose.leftY,!!(pose.buttons&buttons.LeftStick),accent);
    stick(overlay,image,profile.right,pose.rightX,pose.rightY,!!(pose.buttons&buttons.RightStick),accent);
    for(const spot of profile.spots){
      if(!(pose.buttons&spot.button))continue;
      overlay.beginPath();
      if(spot.round)overlay.ellipse(spot.x+spot.w/2,spot.y+spot.h/2,spot.w/2,spot.h/2,0,0,Math.PI*2);
      else overlay.roundRect(spot.x,spot.y,spot.w,spot.h,10);
      overlay.fillStyle=accent;overlay.globalAlpha=.34;overlay.fill();overlay.globalAlpha=1;
      overlay.strokeStyle=accent;overlay.lineWidth=7;overlay.stroke();
      if(spot.label){overlay.fillStyle='white';overlay.font='600 24px "Segoe UI"';overlay.textAlign='center';overlay.textBaseline='middle';overlay.fillText(spot.label,spot.x+spot.w/2,spot.y+spot.h/2);}
    }
    overlay.globalCompositeOperation='destination-in';overlay.drawImage(image,0,0,1448,1086);overlay.globalCompositeOperation='source-over';overlay.restore();
    ctx.drawImage(layer,0,0);
  },[family,visible,dimmed,getInput]);
  const redraw=useCallback(()=>{if(visible&&!frameRef.current)frameRef.current=requestAnimationFrame(draw);},[visible,draw]);
  useImperativeHandle(ref,()=>({redraw}),[redraw]);
  useEffect(()=>{
    const image=new Image();image.onload=()=>{imageRef.current=image;redraw();};image.src=`/assets/${profiles[family].file}`;
    return()=>{image.onload=null;imageRef.current=null;};
  },[family,redraw]);
  useEffect(()=>{
    const canvas=canvasRef.current;if(!canvas)return;
    const observer=new ResizeObserver(redraw);observer.observe(canvas);window.addEventListener('resize',redraw);redraw();
    return()=>{observer.disconnect();window.removeEventListener('resize',redraw);if(frameRef.current)cancelAnimationFrame(frameRef.current);frameRef.current=0;};
  },[redraw]);
  return <canvas ref={canvasRef} className={styles.controllerCanvas} role="img" aria-label="手柄按键和摇杆实时状态"/>;
});
