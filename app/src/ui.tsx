import {useEffect,useRef,useId,type ReactNode,type CSSProperties} from 'react';
import {pixelArt} from './pixels';
import {type Server,safeUrl} from './types';
import {api} from './api';
export async function copyToClipboard(text:string){
  try {await navigator.clipboard.writeText(text);return true} catch {
    const textarea=document.createElement('textarea');
    textarea.value=text;textarea.setAttribute('aria-label','複製文字');
    textarea.style.cssText='position:fixed;opacity:0;pointer-events:none';
    const back=document.activeElement as HTMLElement|null;
    document.body.append(textarea);textarea.select();
    try{return document.execCommand('copy')}catch{return false}finally{textarea.remove();back?.focus()}
  }
}
export const colorStyle=(color:string)=>({'--c':color} as CSSProperties);
export function look(s:Server,index:number){return {icon:pixelArt.icons.some(x=>x.id===s.icon)?s.icon!:'grass',color:pixelArt.colors.some(x=>x[1]===s.color)?s.color!:pixelArt.colors[index%8][1]}}
export function Pixel({id}:{id:string}) {const ref=useRef<HTMLCanvasElement>(null);useEffect(()=>{const cv=ref.current,c=cv?.getContext('2d');if(!c)return;c.clearRect(0,0,16,16);const art=pixelArt.icons.find(x=>x.id===id)??pixelArt.icons[0];art.pixels.forEach((row,y)=>row.forEach((col,x)=>{if(col){c.fillStyle=col;c.fillRect(x,y,1,1)}}))},[id]);return <canvas ref={ref} className={'px'+(['enderman','wither','dragon','warden'].includes(id)?' dk':'')} width={16} height={16} aria-hidden="true"/>}
export function IconTile({s,index,children}:{s:Server;index:number;children?:ReactNode}){const l=look(s,index);return <span className="ic" style={colorStyle(l.color)}><Pixel id={l.icon}/>{children}</span>}
export function Svg({kind}:{kind:'power'|'chevron'|'plus'|'folder'|'overview'|'pen'}) {return <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{kind==='power'?<><path d="M12 3v8"/><path d="M6.3 6.8a8 8 0 1 0 11.4 0"/></>:kind==='chevron'?<path d="M15 6l-6 6 6 6"/>:kind==='plus'?<path d="M12 5v14M5 12h14"/>:kind==='folder'?<path d="M3 7h6l2 2h10v10H3z"/>:kind==='pen'?<path d="M16 3l4 4L9 20H4v-5z"/>:<>{[4,10,16].map(y=><rect key={y} x={3} y={y} width={18} height={4} rx={1}/>)}</>}</svg>}
export function Spark({s,width=120,height=24}:{s:Server;width?:number;height?:number}){const hist=s.mem_hist||[];if(hist.length<2)return <svg className="spark" viewBox={`0 0 ${width} ${height}`} aria-hidden="true"/>;const top=Math.max(s.xmx_mb||Math.max(...hist)*1.2,...hist),x0=width-(hist.length-1)*width/59,pts=hist.map((v,i)=>[x0+i*width/59,height-1-Math.min(v,top)/top*(height-3)]),col=s.xmx_mb&&hist.at(-1)!>s.xmx_mb*.85?'#e8b86a':'#57c1ff',d='M'+pts.map(p=>p.map(x=>x.toFixed(1)).join(' ')).join(' L'),cap=height-1-(s.xmx_mb||0)/top*(height-3);return <svg className="spark" viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" aria-hidden="true">{s.xmx_mb&&<line x1={0} x2={width} y1={cap} y2={cap} stroke="#33363b" strokeDasharray="2 3" vectorEffect="non-scaling-stroke"/>}<path d={`${d} L${width} ${height} L${x0} ${height}Z`} fill={col+'14'}/><path d={d} fill="none" stroke={col} strokeWidth="1.3" vectorEffect="non-scaling-stroke"/><circle cx={width} cy={pts.at(-1)![1]} r="2.4" fill={col}/></svg>}
export function External({url,children,className}:{url:string;children:ReactNode;className?:string}){const safe=safeUrl(url);return <a className={className} href={safe??undefined} onClick={e=>{e.preventDefault();if(safe)void api('/api/open-url',{url:safe})}}>{children}</a>}
export function Modal({title,children,onClose,wide=false,className='',busy=false}:{title:string;children:ReactNode;onClose:()=>void;wide?:boolean;className?:string;busy?:boolean}) {
  const ref=useRef<HTMLDivElement>(null),id=useId(),close=useRef(onClose);
  close.current=onClose;
  const locked=useRef(busy);locked.current=busy;
  useEffect(()=>{
    const back=document.activeElement as HTMLElement|null,el=ref.current;
    const first=el?.querySelector<HTMLElement>('input:not(:disabled),select:not(:disabled),textarea,button:not(:disabled),a[href]');
    (first??el)?.focus();
    const key=(e:KeyboardEvent)=>{
      const modals=[...document.querySelectorAll('.modal')];
      if(modals.at(-1)!==el)return;
      if(e.key==='Escape') {e.preventDefault();e.stopPropagation();if(!locked.current)close.current()}
      if(e.key==='Tab') {
        const items=[...el!.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea,a[href],[tabindex="0"]')].filter(x=>x.getClientRects().length>0);
        if(!items.length) {e.preventDefault();el?.focus();return}
        const first=items[0],last=items.at(-1)!;
        if(!el?.contains(document.activeElement)||document.activeElement===el) {e.preventDefault();(e.shiftKey?last:first).focus()}
        else if(e.shiftKey&&document.activeElement===first) {e.preventDefault();last.focus()}
        else if(!e.shiftKey&&document.activeElement===last) {e.preventDefault();first.focus()}
      }
    };
    document.addEventListener('keydown',key,true);
    return()=>{document.removeEventListener('keydown',key,true);if(back?.isConnected)back.focus()};
  },[]);
  return <div className={'modal '+(className==='pal'?'pal':'')} ref={ref} tabIndex={-1} role="dialog" aria-modal="true" aria-labelledby={id}><div className={className==='pal'?'palbox':'sheet'+(wide?' wide':'')+(className?' '+className:'')}>
    {className!=='pal'&&className!=='icons'?<h3 id={id}>{title}</h3>:<span id={id} className="sr-only">{title}</span>}{children}
  </div></div>;
}
export function ErrorHint({error}:{error:string}){return error?<div className="hint err" role="alert">{error}</div>:null}
