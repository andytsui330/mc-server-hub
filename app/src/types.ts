export type ServerState = 'stopped' | 'starting' | 'running' | 'stopping' | 'external';
export interface Server {
  id: string; name: string; dir: string; state: ServerState; version: string; port: number;
  online: number | null; max: number; names: string[]; mem_mb: number | null; xmx_mb: number | null; force_wait_s?:number;
  mem_hist: number[]; uptime_s: number | null; stopping_s?: number; can_force?: boolean;
  loader?:string; maintenance?:{busy:boolean;ok?:boolean;text:string}; automation_note?:string;restart_attempts?:number;restart_pending?:boolean;backup_schedule_note?:string; backup_busy?: boolean; backup?: {ok?: boolean; text: string}; problem?: string; modded?: boolean;
  address?: string; icon?: string; color?: string;
}
export interface Command {id: string; name: string; cat: string; tpl: string; fields: string[]; danger?: ''|'confirm'|'strong'; preview?: string; note?: string; uncertain?: boolean; special?: string; confirm_if_false?: boolean}
export interface FieldSpec {label: string; kind: string; min?: number; max?: number; default?: number; optional?: boolean; picks?: [string,string,number?][]}
export interface Commands {commands: Command[]; fields: Record<string,FieldSpec>}
export interface Note {id?: string; name: string; tpl: string; cat: string; danger: ''|'confirm'|'strong'; note: string}
export interface Preferences {launch_at_login:boolean;start_hidden:boolean;close_action:'quit'|'tray';close_asked:boolean;auto_start:string[];channel:'stable'|'beta';theme:'dark'|'light'|'system';font_size:number;sound:boolean;volume:number;language:'zh-TW';default_ram:number;server_dir:string;java_path:string;stop_wait:number;auto_backup:boolean;backup_dir:string;backup_keep:number;backup_schedule:boolean;backup_time:string;backup_snapshot:boolean}
export const defaultPreferences:Preferences={launch_at_login:false,start_hidden:false,close_action:'quit',close_asked:false,auto_start:[],channel:'stable',theme:'dark',font_size:14,sound:true,volume:40,language:'zh-TW',default_ram:4,server_dir:'',java_path:'',stop_wait:120,auto_backup:false,backup_dir:'',backup_keep:10,backup_schedule:false,backup_time:'04:00',backup_snapshot:true};
export interface Settings {custom_commands: Note[]; links: {name: string; url: string}[]; join_template: string; update_check?: boolean;preferences?:Preferences}
export interface Job {state: string; msg: string; pct?: number | null; lines?: string[]; id?: string; error?: string; help_url?: string}
export interface State {servers: Server[]; update?: string; job?: Job}
export interface Info {join_text: string; seed: string; seed_note?: string; ram_gb: number; ram_total_gb: number; auto_backup: boolean; dir: string; java?: string; pid?: number}
export interface Version {id: string; fabric?: boolean; forge?: {ok: boolean; version?: string; why?: string}; neoforge?: {ok: boolean; version?: string; why?: string}}
export const labels: Record<ServerState,string> = {stopped:'已關機',starting:'開機中…',running:'執行中',stopping:'關機中…',external:'在別處執行中'};
export const store = {get<T>(key: string, fallback: T): T {try {const x=localStorage.getItem('hub.'+key); return x === null ? fallback : JSON.parse(x)} catch {return fallback}}, set(key: string,value: unknown){try {localStorage.setItem('hub.'+key,JSON.stringify(value))} catch {}}};
export const safeUrl = (url?: string | null) => /^https:\/\/[^\s<>"']+$/.test(url || '') ? url! : null;
export const gb = (n: number) => (n/1024).toFixed(1);
// Match legacy readVals: confirmation, target name and transmitted values share one trimmed form.
export const normalizeValues = (values:Record<string,string>) => Object.fromEntries(Object.entries(values).map(([key,value])=>[key,value.trim()]));
export const pad2 = (n: number) => String(n).padStart(2,'0');
export function fmtUp(s: number | null) {if(s == null)return '—';const h=Math.floor(s/3600),m=Math.floor(s%3600/60),x=Math.floor(s%60);return h?`${h}:${pad2(m)}:${pad2(x)}`:`${m}:${pad2(x)}`}
export function powerSpec(s: Server){if(s.state==='stopped')return {cls:'',text:s.maintenance?.busy?'處理中…':s.restart_pending?'等候自動重開':s.backup_busy?'備份中…':'開機',disabled:!!(s.backup_busy||s.maintenance?.busy||s.restart_pending)};if(s.state==='starting')return {cls:'busy',text:'開機中…'};if(s.state==='running')return {cls:'out',text:'關機'};if(s.state==='stopping')return {cls:s.can_force?'force':'busy',text:s.can_force?'強制結束':`關機中…（${s.stopping_s??0} 秒）`};return {cls:'ext',text:'在別處開的'}}

/** 指令搜尋快捷鍵的顯示文字：Mac 用 ⌘K，其他平台用 Ctrl K（兩種按法程式都接受）。 */
export const searchKey = typeof navigator !== 'undefined' && /Mac/i.test(navigator.userAgent) ? '⌘K' : 'Ctrl K';
