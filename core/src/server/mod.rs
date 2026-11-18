//! 單台伺服器：開機前檢查、開關機、主控台即時輸出、送指令、狀態與記憶體（1.x 的 Server 類別）。
pub mod launch;
pub mod sys;

use crate::{java, platform, UserError};
use launch::{Launch, Mode};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// 送出 stop 超過這麼久還沒關，才允許「強制結束」。
pub const STOP_GRACE: Duration = Duration::from_secs(120);
/// 記憶體折線每台保留最近幾筆（巡檢每 3 秒一次＝約 3 分鐘）。
pub const MEM_HISTORY: usize = 60;
const LOG_LINES: usize = 3000;

pub fn disk_check(free:Option<u64>)->Value {
    let (state,text)=match free {Some(n) if n<1_073_741_824=>("error","剩餘不到 1 GB，請先騰出空間"),Some(n) if n<5*1_073_741_824=>("warning","剩餘不到 5 GB，建議先騰出空間"),Some(_)=>("ok","磁碟空間足夠開機；請持續留意世界成長"),None=>("warning","無法查詢可用空間，請先人工確認硬碟容量")};
    json!({"key":"disk","label":"硬碟空間","state":state,"text":text,"free_bytes":free})
}

/// 伺服器圖示與顏色白名單：畫面自己畫，這裡只負責存檔前檢查（1.x 的 ICON_IDS／ICON_COLORS）。
pub const ICON_IDS: [&str; 93] = [
    "grass", "dirt", "stone", "cobble", "sand", "snow", "ice", "water", "lava", "planks", "log", "leaves", "bricks",
    "glass", "pumpkin", "obsidian", "nether", "endstone", "amethyst", "ore-diamond", "ore-gold", "ore-emerald",
    "ore-redstone", "ore-lapis", "ore-iron", "ore-coal", "diamond", "emerald", "ruby", "gold", "iron", "crown",
    "star", "heart", "sword", "pickaxe", "axe", "shield", "bomb", "torch", "potion", "chest", "book", "map",
    "compass", "pad", "apple", "carrot", "cake", "fish", "mushroom", "tree", "flower", "fire", "island", "castle",
    "house", "portal", "moon", "sun", "pig", "cow", "sheep", "chicken", "horse", "rabbit", "wolf", "cat", "fox",
    "panda", "bee", "parrot", "villager", "golem", "axolotl", "frog", "turtle", "squid", "dolphin", "creeper",
    "zombie", "skeleton", "spider", "enderman", "slime", "witch", "pillager", "blaze", "ghast", "piglin", "wither",
    "dragon", "warden",
];
pub const ICON_COLORS: [&str; 8] = ["#5fd39a", "#57c1ff", "#b48cff", "#ff8ab8", "#ff6b6b", "#ffa45c", "#f2d16b", "#f4f4f6"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Stopped,
    Starting,
    Running,
    Stopping,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Stopped => "stopped",
            Phase::Starting => "starting",
            Phase::Running => "running",
            Phase::Stopping => "stopping",
        }
    }
}

/// 開機被擋下的原因：白話錯誤（可能附下載 Java 的網址），或要先讓使用者本人同意 EULA。
#[derive(Debug, PartialEq)]
pub enum Blocked {
    Error(UserError),
    NeedEula,
}

impl From<String> for Blocked {
    fn from(msg: String) -> Self {
        Blocked::Error(UserError::new(msg))
    }
}

struct State {
    cfg: Value,
    dir: PathBuf,
    port: u16,
    phase: Phase,
    launch: Option<Launch>,
    problem: Option<String>,
    started_at: Option<Instant>,
    stop_sent_at: Option<Instant>,
    ping: Option<Value>,
    external: bool,
    saw_done: bool,
    log: VecDeque<(u64, String)>,
    seq: u64,
    mem_hist: VecDeque<u64>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    backup_busy: bool,
    backup_info: Value,
    backup_root: Option<PathBuf>,
    backup_keep: usize,
    maintenance_busy:bool,
    maintenance_info:Value,
    automation:crate::automation::Runtime,
    automation_epoch:Instant,
    runtime_java:Option<PathBuf>,
}

impl State {
    fn add_log(&mut self, text: impl Into<String>) {
        self.seq += 1;
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back((self.seq, text.into()));
    }

    fn alive(&mut self) -> bool {
        self.child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    fn refresh_port(&mut self){if !self.alive(){self.port=launch::read_port(&self.dir,self.port);}}

    fn send(&mut self, text: &str) -> Result<(), String> {
        if !self.alive() {
            return Err("伺服器沒有在執行".into());
        }
        let text = text.trim();
        if text.contains(['\n', '\r']) {
            return Err("指令不能換行".into());
        }
        let stdin = self.stdin.as_mut().ok_or("伺服器沒有在執行")?;
        stdin
            .write_all(format!("{}\n", text.trim_start_matches('/')).as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("指令送不出去：{e}"))
    }

    fn line(&mut self, raw: &[u8]) {
        let line = platform::decode_text(raw);
        let line = line.trim_end_matches(['\r', '\n']).to_string();
        if line.contains("Done (") && line.contains("help") {
            self.saw_done = true;
            if self.phase == Phase::Starting {
                self.phase = Phase::Running;
            }
        }
        self.add_log(line);
    }
}

/// 可以複製給多個執行緒用的同一台伺服器。
#[derive(Clone)]
pub struct Server {
    inner: Arc<Mutex<State>>,
}

impl Server {
    /// cfg 是設定檔裡這台的那一筆（id、name、dir、bat、version、loader…），欄位原樣保留。
    pub fn new(cfg: Value) -> Self {
        let dir = PathBuf::from(cfg["dir"].as_str().unwrap_or_default());
        let port = launch::read_port(&dir, 25565);
        Server {
            inner: Arc::new(Mutex::new(State {
                cfg, dir, port, phase: Phase::Stopped, launch: None, problem: None, started_at: None, stop_sent_at: None,
                ping: None, external: false, saw_done: false, log: VecDeque::new(), seq: 0, mem_hist: VecDeque::new(),
                child: None, stdin: None, backup_busy: false, backup_info: Value::Null,backup_root:None,backup_keep:10,
                maintenance_busy:false,maintenance_info:Value::Null,automation:Default::default(),automation_epoch:Instant::now(),runtime_java:None,
            })),
        }
    }

    fn st(&self) -> MutexGuard<'_, State> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn id(&self) -> String {
        self.st().cfg["id"].as_str().unwrap_or_default().to_string()
    }

    pub fn dir(&self) -> PathBuf {
        self.st().dir.clone()
    }

    pub fn alive(&self) -> bool {
        self.st().alive()
    }

    /// 設定檔裡這台的那一筆（平台存檔時整份寫回）。
    pub fn cfg(&self) -> Value {
        self.st().cfg.clone()
    }

    pub fn set_cfg(&self, key: &str, value: Value) {
        self.st().cfg[key] = value;
    }

    /// 開著、別處開的、或正在備份：這三種都不能改記憶體。
    pub fn busy(&self) -> bool {
        let mut st = self.st();
        st.alive() || st.external || st.backup_busy || st.maintenance_busy || st.automation.pending()
    }

    pub fn backup_busy(&self) -> bool {
        self.st().backup_busy
    }

    pub fn maintenance_busy(&self)->bool{self.st().maintenance_busy}
    pub fn begin_maintenance(&self)->Result<(),String>{let mut st=self.st();st.refresh_port();if st.alive()||st.external||st.backup_busy||st.maintenance_busy||st.automation.pending()||!launch::port_free(st.port){return Err("請先關機、取消待重開工作，並等備份或下載完成".into());}st.maintenance_busy=true;st.maintenance_info=json!({"busy":true,"text":"準備中…"});Ok(())}
    pub fn maintenance_progress(&self,text:&str){self.st().maintenance_info=json!({"busy":true,"text":text});}
    pub fn finish_maintenance(&self,result:crate::Result<Value>){let mut st=self.st();st.maintenance_busy=false;st.maintenance_info=match result{Ok(v)=>json!({"busy":false,"ok":true,"text":v["text"].as_str().unwrap_or("工作完成"),"result":v}),Err(e)=>json!({"busy":false,"ok":false,"text":e.msg})};let text=st.maintenance_info["text"].as_str().unwrap_or_default().to_string();st.add_log(format!("» 平台：{text}"));}
    pub fn set_runtime_java(&self,path:Option<PathBuf>){self.st().runtime_java=path;}
    pub fn manual_start(&self){self.st().automation.manual_start();}
    pub fn cancel_restart(&self){self.st().automation.cancel();}
    pub fn automation_failed(&self,msg:&str){self.st().automation.start_failed(msg);}
    pub fn schedule_restart(&self)->Vec<crate::automation::Action>{let mut st=self.st();let p=crate::automation::Policy::parse(&st.cfg["automation"]).unwrap_or_default();let now=st.automation_epoch.elapsed().as_millis() as u64;let running=st.alive()&&st.phase==Phase::Running&&!st.external&&!st.maintenance_busy;st.automation.schedule(&p,now,running)}
    pub fn automation_tick(&self)->Vec<crate::automation::Action>{let mut st=self.st();let p=crate::automation::Policy::parse(&st.cfg["automation"]).unwrap_or_default();let now=st.automation_epoch.elapsed().as_millis() as u64;let running=st.alive();let ready=!running&&st.phase==Phase::Stopped&&!st.external&&!st.backup_busy&&!st.maintenance_busy&&launch::port_free(st.port);st.automation.tick(&p,now,running,ready)}

    pub fn set_backup_policy(&self,root:PathBuf,keep:usize){let mut st=self.st();st.backup_root=Some(root);st.backup_keep=keep;}
    pub fn backup_root(&self)->PathBuf{let st=self.st();st.backup_root.clone().unwrap_or_else(||st.dir.join("backups"))}

    pub fn port(&self) -> u16 {
        let mut st=self.st();st.refresh_port();st.port
    }

    /// 程序編號（執行中才有）與上次開機用的 Java（同 1.x：關機後仍顯示上次那支）。
    pub fn pid_java(&self) -> (Option<u32>, Option<String>) {
        let mut st = self.st();
        let pid = if st.alive() { st.child.as_ref().map(Child::id) } else { None };
        (pid, st.launch.as_ref().and_then(|l| l.java.clone()))
    }

    pub fn add_log(&self, text: impl Into<String>) {
        self.st().add_log(text);
    }

    /// 主控台：回傳 (最新編號, 編號大於 since 的每一行)。
    pub fn logs_since(&self, since: u64) -> (u64, Vec<(u64, String)>) {
        let st = self.st();
        (st.seq, st.log.iter().filter(|l| l.0 > since).cloned().collect())
    }

    /// 開機前檢查：找得到啟動檔、Java 版本夠、EULA 已同意、連接埠沒被占用。
    /// java_need：設定檔記的需求，沒記就由呼叫端從伺服器資料夾判斷（第 4 步 inspect_server）。
    fn preflight(&self, dir: &Path, bat: &str, need: Option<u32>, port: u16) -> Result<Launch, Blocked> {
        crate::backup::ensure_restore_safe(dir).map_err(Blocked::Error)?;
        crate::mods::ensure_safe(dir).map_err(Blocked::Error)?;
        let script = platform::start_script_name(platform::OS);
        let mut launch = launch::parse_launch(dir, bat)
            .map_err(|_| format!("在「{}」找不到 {bat}。資料夾可能被移動或刪除了，請從清單移除後重新加入。", dir.display()))?;
        if launch.mode==Mode::Direct&&launch.java.as_deref().is_some_and(|j|matches!(j.to_ascii_lowercase().as_str(),"java"|"java.exe"))
            && let Some(path)=self.st().runtime_java.clone()
            && java::java_major(&path).is_some_and(|major|major>=need.unwrap_or(8))
            && let Some((_,end))=launch::first_token(&launch.cmd){let text=path.display().to_string();launch=Launch{cmd:format!("\"{text}\"{}",&launch.cmd[end..]),java:Some(text),mode:Mode::Direct};}
        if let Some(j) = launch.java.clone() {
            let have = java::java_major(Path::new(&j));
            if have.is_none() || need.is_some_and(|n| have < Some(n)) {
                let plain = matches!(j.to_lowercase().as_str(), "java" | "java.exe");
                let better = if launch.mode == Mode::Direct && plain { java::best_java(need, &java::installed_javas()) } else { None };
                if let Some((major, path)) = better {
                    // PATH 上的 Java 不夠新，但電腦裡有裝夠新的，就直接用它
                    self.add_log(format!("» 平台：改用電腦裡的 Java {major}（{}）", path.display()));
                    let (_, end) = launch::first_token(&launch.cmd).expect("direct 一定有執行檔");
                    let p = path.display().to_string();
                    launch = Launch { cmd: format!("\"{p}\"{}", &launch.cmd[end..]), java: Some(p), mode: Mode::Direct };
                } else {
                    let wanted = need.map_or("Java".into(), |n| format!("Java {n} 以上"));
                    let now = have.map_or("找不到 Java".into(), |h| format!("目前用的是 Java {h}"));
                    let hint = if launch.mode == Mode::Script || !plain { format!("（已經裝了的話，請檢查 {script} 裡寫的 Java 路徑。）") } else { String::new() };
                    let arch = if platform::OS == platform::Os::Mac { platform::mac_arch() } else { "x64" };
                    return Err(Blocked::Error(UserError {
                        msg: format!("這台伺服器需要 {wanted}，但電腦上{now}。請點下方按鈕下載安裝，裝好再按一次開機，平台不用重開。{hint}"),
                        help_url: Some(platform::java_help_url(need, platform::OS, arch)),
                    }));
                }
            }
        }
        if !launch::eula_agreed(dir) {
            return Err(Blocked::NeedEula);
        }
        if !launch::port_free(port) {
            return Err(format!("連接埠 {port} 已經被別的程式佔用。請先關掉另一台伺服器，或改這台 server.properties 的 server-port。").into());
        }
        if disk_check(sys::disk_free(dir))["state"]=="error" {return Err("硬碟剩餘空間不足 1 GB，請先騰出空間再開機，避免世界存檔損壞".to_string().into());}
        Ok(launch)
    }

    pub fn preflight_report(&self,need:Option<u32>)->Value {
        let cfg=self.cfg();let dir=self.dir();let port=launch::read_port(&dir,self.port());
        let result=self.preflight(&dir,cfg["bat"].as_str().unwrap_or("start.bat"),need,port);
        let mut checks=vec![disk_check(sys::disk_free(&dir))];
        if self.busy(){checks.push(json!({"key":"busy","label":"伺服器狀態","state":"error","text":"伺服器正在執行或處理備份／還原，請完成後再開機"}));}
        checks.push(json!({"key":"eula","label":"遊戲授權","state":if launch::eula_agreed(&dir){"ok"}else{"action"},"text":if launch::eula_agreed(&dir){"已同意 EULA"}else{"開機時會請你本人閱讀並同意 EULA"}}));
        let free=launch::port_free(port);checks.push(json!({"key":"port","label":"連接埠","state":if free{"ok"}else{"error"},"text":format!("連接埠 {port}{}",if free{"可使用"}else{"已被其他程式占用"})}));
        let (ready,text,url)=match result {Ok(_)=>(true,"啟動檔與 Java 檢查通過；開機時仍會重新檢查".to_string(),None),Err(Blocked::NeedEula)=>(true,"請完成授權確認後開機".into(),None),Err(Blocked::Error(e))=>(false,e.msg,e.help_url)};
        checks.push(json!({"key":"launch","label":"啟動檔與 Java","state":if ready{"ok"}else{"error"},"text":text}));
        let ready=ready&&!checks.iter().any(|c|c["state"]=="error")&&!self.busy();
        json!({"ready":ready,"checks":checks,"help_url":url,"note":"不足 1 GB 不開機；不足 5 GB 提醒。模組包與長時間遊玩可能需要更多空間。"})
    }

    pub fn start(&self, java_need: Option<u32>) -> Result<(), Blocked> {
        let (dir, bat, port) = {
            let mut st = self.st();
            if st.alive() {
                return Err("已經在執行中了".to_string().into());
            }
            if st.backup_busy || st.maintenance_busy {
                return Err("正在備份世界，等備份完成再開機".to_string().into());
            }
            if st.external {
                return Err(format!("連接埠 {} 已經有伺服器在跑（不是從平台開的），請先在那邊關掉", st.port).into());
            }
            st.port = launch::read_port(&st.dir.clone(), st.port);
            (st.dir.clone(), st.cfg["bat"].as_str().unwrap_or("start.bat").to_string(), st.port)
        };
        let launch = self.preflight(&dir, &bat, java_need, port)?;
        let mut st = self.st();
        if st.alive()||st.external||st.backup_busy||st.maintenance_busy {return Err("伺服器狀態已改變，請等目前操作結束再開機".to_string().into());}
        (st.problem, st.saw_done) = (None, false);
        st.add_log(format!("» 平台：開機中…（{}）", launch.cmd));
        if launch.mode == Mode::Script {
            st.add_log(format!("» 平台：這份 {} 寫法比較複雜，平台直接執行它", platform::start_script_name(platform::OS)));
        }
        let (mut child, out) = match spawn(&launch, &dir) {
            Ok(x) => x,
            Err(e) => {
                st.add_log(format!("» 平台：開機失敗 {e}"));
                return Err(format!("開機失敗：{e}").into());
            }
        };
        st.stdin = child.stdin.take();
        st.child = Some(child);
        (st.phase, st.started_at, st.stop_sent_at) = (Phase::Starting, Some(Instant::now()), None);
        st.mem_hist.clear();
        let script_mode = launch.mode == Mode::Script;
        st.launch = Some(launch);
        let me = self.clone();
        std::thread::spawn(move || me.reader(out, script_mode));
        Ok(())
    }

    fn reader(&self, mut out: io::PipeReader, script_mode: bool) {
        // bat 裡的 pause 會卡住等按鍵（提示沒有換行），替使用者按一下才能結束。
        static PAUSE: LazyLock<regex::bytes::Regex> = LazyLock::new(|| regex::bytes::Regex::new(r"\. \. \.\s*$").unwrap());
        let (mut buf, mut chunk) = (Vec::new(), vec![0u8; 65536]);
        loop {
            let n = match out.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            buf.extend_from_slice(&chunk[..n]);
            let mut st = self.st();
            while let Some(i) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=i).collect();
                st.line(&line[..i]);
            }
            if script_mode && PAUSE.is_match(&buf) {
                st.line(&std::mem::take(&mut buf));
                if let Some(stdin) = st.stdin.as_mut() {
                    let _ = stdin.write_all(b"\n").and_then(|_| stdin.flush());
                }
            }
        }
        if !buf.is_empty() {
            self.st().line(&buf);
        }
        let code = loop {
            {
                let mut st = self.st();
                match st.child.as_mut().map(Child::try_wait) {
                    Some(Ok(Some(status))) => break exit_code(status),
                    Some(Ok(None)) => {}
                    _ => break -1,
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let mut st = self.st();
        st.add_log(format!("» 平台：伺服器已關閉（結束代碼 {code}）"));
        if st.stop_sent_at.is_none() {
            let recent: Vec<String> = st.log.iter().map(|l| l.1.clone()).collect();
            st.problem = launch::explain_failure(&recent, code, st.saw_done);
            if let Some(p) = st.problem.clone() {
                st.add_log(format!("» 平台：{p}"));
            }
        }
        (st.phase, st.ping, st.started_at, st.child, st.stdin) = (Phase::Stopped, None, None, None, None);
        let policy=crate::automation::Policy::parse(&st.cfg["automation"]).unwrap_or_default();let now=st.automation_epoch.elapsed().as_millis() as u64;st.automation.exited(&policy,code,now);
        if code==0&&st.stop_sent_at.is_some() && st.cfg["auto_backup"] == json!(true) {
            // 只在正常關機後備份，閃退不備份
            st.add_log("» 平台：已關機，依設定自動備份世界…");
            st.backup_busy=true;st.backup_info=json!({"ok":null,"text":"備份中…"});
            drop(st);
            self.launch_backup();
        }
    }

    /// 在背景備份世界；進度與結果看 snapshot 的 backup_busy／backup。
    pub fn backup(&self) -> Result<(), String> {
        {
            let mut st = self.st();
            st.refresh_port();
            if st.alive() || st.external || !launch::port_free(st.port) {
                return Err("伺服器開著不能備份，請先關機".into());
            }
            if st.backup_busy||st.maintenance_busy||st.automation.pending() {
                return Err("已經在備份了".into());
            }
            (st.backup_busy, st.backup_info) = (true, json!({"ok": null, "text": "備份中…"}));
        }
        self.launch_backup();Ok(())
    }
    fn launch_backup(&self){
        let (me, dir) = (self.clone(), self.dir());
        let (root,keep)={let st=self.st();(st.backup_root.clone().unwrap_or_else(||st.dir.join("backups")),st.backup_keep)};
        std::thread::spawn(move || {
            let info = crate::backup::backup_world_to(&dir,&root,keep).unwrap_or_else(|e| json!({"ok": false, "text": format!("備份失敗：{e}")}));
            let mut st = me.st();
            let text = info["text"].as_str().unwrap_or_default().to_string();
            (st.backup_busy, st.backup_info) = (false, info);
            st.add_log(format!("» 平台：{text}"));
        });
    }

    pub fn restore(&self,name:&str)->Result<(),String>{
        let (dir,root)={let mut st=self.st();st.refresh_port();if st.alive()||st.external||st.backup_busy||st.maintenance_busy||st.automation.pending()||!launch::port_free(st.port){return Err("請先關機、取消待重開，並等備份／還原／下載完成".into());}st.backup_busy=true;st.backup_info=json!({"ok":null,"text":"正在建立還原前快照並還原…"});(st.dir.clone(),st.backup_root.clone().unwrap_or_else(||st.dir.join("backups")))};
        let me=self.clone();let name=name.to_string();std::thread::spawn(move||{let info=crate::backup::restore_world(&dir,&root,&name).unwrap_or_else(|e|json!({"ok":false,"text":format!("還原失敗：{e}")}));let mut st=me.st();st.add_log(format!("» 平台：{}",info["text"].as_str().unwrap_or_default()));st.backup_info=info;st.backup_busy=false;});Ok(())
    }

    pub fn send(&self, text: &str) -> Result<(), String> {
        self.st().send(text)
    }

    pub fn players(&self) -> crate::Result<Value> {
        let mut result=crate::players::read(&self.dir())?;
        let (alive,external,since)={let mut st=self.st();let alive=st.alive();let since=st.seq;if alive && st.phase==Phase::Running {st.send("list").map_err(UserError::new)?;} (alive,st.external,since)};
        let mut online=None;
        if alive {
            let end=Instant::now()+Duration::from_secs(2);
            while Instant::now()<end {
                for (_,line) in self.logs_since(since).1 {
                    if let Some((_,names))=line.split_once("players online:") {online=Some(names.trim().split(',').map(str::trim).filter(|n|crate::players::valid_name(n)).map(String::from).collect::<Vec<_>>());}
                }
                if online.is_some(){break;}std::thread::sleep(Duration::from_millis(50));
            }
        }
        result["online"]=json!(online);result["mode"]=json!(if external{"external"}else if alive{"running"}else{"offline"});
        result["online_note"]=json!(if alive && online.is_none(){"伺服器沒有回應完整玩家清單，請看主控台；狀態查詢樣本不能當完整名單"}else if external{"在別處執行，請回原視窗管理"}else{"線上名單由 list 指令取得；權限名單來自伺服器檔案"});
        Ok(result)
    }

    pub fn player_action(&self,body:&Value)->crate::Result<Value> {
        let cmd=crate::players::command(body)?;
        let mut st=self.st();
        st.refresh_port();
        if st.external||st.backup_busy||st.maintenance_busy||(!st.alive()&&!launch::port_free(st.port)) {return Err(UserError::new("請先結束外部執行或備份／還原／下載，再修改玩家"));}
        if st.alive(){if st.phase!=Phase::Running{return Err(UserError::new("請等開機或關機完成再管理玩家"));} st.send(&cmd).map_err(UserError::new)?;st.add_log(format!("> {cmd}"));Ok(json!({"ok":true,"mode":"command","text":"指令已送出；請看主控台確認伺服器回覆"}))}
        else{crate::players::change(&st.dir,body)}
    }

    pub fn properties(&self,body:&Value)->crate::Result<Value>{
        let mut st=self.st();
        st.refresh_port();
        if body.get("values").is_none(){return crate::properties::read(&st.dir);}
        if st.external||st.backup_busy||st.maintenance_busy||(!st.alive()&&!launch::port_free(st.port))||matches!(st.phase,Phase::Starting|Phase::Stopping){return Err(UserError::new("請等開關機、備份、下載或外部執行結束，再修改遊戲設定"));}
        let running=st.alive();let mut result=crate::properties::save(&st.dir,body)?;
        result["restart_required"]=json!(running);result["text"]=json!(if running{"已儲存，重開伺服器後生效"}else{"已儲存，下次開機生效"});Ok(result)
    }

    /// 一律走遊戲內 stop 指令，讓伺服器自己存檔後關閉。
    pub fn stop(&self) -> Result<(), String> {
        let mut st = self.st();
        if !st.alive() {
            return Err("伺服器沒有在執行".into());
        }
        st.add_log("» 平台：送出 stop，伺服器會先存檔再關閉…");
        st.send("stop")?;
        (st.phase, st.stop_sent_at) = (Phase::Stopping, Some(Instant::now()));
        Ok(())
    }

    fn can_force(st: &State) -> bool {
        st.phase == Phase::Stopping && st.stop_sent_at.is_some_and(|t| t.elapsed() > Duration::from_secs(st.cfg["stop_wait"].as_u64().unwrap_or(STOP_GRACE.as_secs()).clamp(120,600)))
    }

    pub fn force_kill(&self) -> Result<(), String> {
        // shortcut: bat 模式裡看不懂的自動重開迴圈，stop 後會再開一次，只能靠這裡強制結束, 真有人用到再解析
        let pid = {
            let mut st = self.st();
            if !Self::can_force(&st) {
                return Err(format!("送出 stop 後要等 {} 秒還沒關，才能強制結束", st.cfg["stop_wait"].as_u64().unwrap_or(STOP_GRACE.as_secs()).clamp(120,600)));
            }
            st.add_log("» 平台：強制結束（最後手段）");
            st.child.as_ref().map(Child::id)
        };
        if let Some(pid) = pid {
            sys::kill_tree(pid);
        }
        Ok(())
    }

    /// 清敵對生物：讀回目前難度 → 切和平 → 2 秒後切回。讀不到難度就什麼都不改。
    pub fn clear_hostile(&self) -> Result<(), String> {
        static DIFF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"The difficulty is (\w+)").unwrap());
        let since = {
            let mut st = self.st();
            if !st.alive() {
                return Err("伺服器沒有在執行".into());
            }
            let since = st.seq;
            st.send("difficulty")?;
            since
        };
        let (mut cur, end) = (None, Instant::now() + Duration::from_secs(3));
        while cur.is_none() && Instant::now() < end {
            for (_, t) in self.logs_since(since).1 {
                if let Some(m) = DIFF.captures(&t) {
                    cur = Some(m[1].to_lowercase());
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let cur = cur.filter(|c| ["peaceful", "easy", "normal", "hard"].contains(&c.as_str())).ok_or("讀不到目前的難度（3 秒內沒回應），什麼都沒改")?;
        if cur == "peaceful" {
            return Err("現在已經是和平難度，本來就沒有敵對生物".into());
        }
        let mut st = self.st();
        st.add_log(format!("» 平台：清敵對生物：暫時切到和平，2 秒後切回 {cur}"));
        st.send("difficulty peaceful")?;
        let me = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            let mut st = me.st();
            if st.send(&format!("difficulty {cur}")).is_err() {
                st.add_log(format!("» 平台：難度沒切回去，請手動送 difficulty {cur}"));
            }
        });
        Ok(())
    }

    /// 巡檢（每 3 秒）：問一次遊戲列表狀態、判斷是不是別處開的、記一筆記憶體。
    pub fn poll_status(&self) {
        let port = self.port();
        let ping = slp_ping(port, Duration::from_millis(1500)).ok();
        let pid = {
            let mut st = self.st();
            let mine = st.alive();
            st.external = ping.is_some() && !mine;
            if mine && ping.is_some() && st.phase == Phase::Starting {
                st.phase = Phase::Running;
            }
            st.ping = ping;
            if mine { st.child.as_ref().map(Child::id) } else { None }
        };
        if let Some(mb) = pid.and_then(sys::tree_mem_mb) {
            let mut st = self.st();
            if st.mem_hist.len() == MEM_HISTORY {
                st.mem_hist.pop_front();
            }
            st.mem_hist.push_back(mb);
        }
    }

    /// 給畫面的狀態（欄位與 1.x /api/state 的每台一樣）。
    pub fn snapshot(&self) -> Value {
        let (mut v, pid) = {
            let mut st = self.st();
            let alive = st.alive();
            let cfg = st.cfg.clone();
            let get = |k: &str| cfg.get(k).and_then(Value::as_str);
            let p = st.ping.clone().unwrap_or(Value::Null);
            let players = p.get("players").filter(|x| x.is_object()).cloned().unwrap_or(json!({}));
            static XMX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-Xmx(\d+)([GgMm])").unwrap());
            let xmx = st.launch.as_ref().filter(|l| alive && l.mode == Mode::Direct).and_then(|l| XMX.captures(&l.cmd)).and_then(|m| {
                let n: u64 = m[1].parse().ok()?;
                Some(if m[2].eq_ignore_ascii_case("g") { n * 1024 } else { n })
            });
            let version = get("version").unwrap_or("");
            let names: Vec<Value> = players["sample"].as_array().into_iter().flatten().map(|x| x.get("name").cloned().unwrap_or(json!(""))).collect();
            let v = json!({
                "id": get("id"), "name": cfg.get("name"), "version": version,
                "accent": get("accent").unwrap_or("#7c9cff"), "address": get("address").unwrap_or(""),
                "icon": get("icon").filter(|i| ICON_IDS.contains(i)), "color": get("color").filter(|c| ICON_COLORS.contains(c)),
                "port": st.port, "state": if st.external { "external" } else { st.phase.as_str() },
                "dir": st.dir.display().to_string(), "problem": st.problem, "backup": st.backup_info, "backup_busy": st.backup_busy,"backup_schedule_note":cfg["backup_schedule_note"],
                "maintenance":st.maintenance_info,"automation":crate::automation::Policy::parse(&cfg["automation"]).unwrap_or_default().to_json(),"automation_note":st.automation.note,"restart_attempts":st.automation.attempts,"restart_pending":st.automation.pending(),
                "loader":get("loader"),"modded": get("loader") == Some("Fabric") || version.contains("Fabric") || st.dir.join("mods").is_dir(),
                "online": if st.ping.is_some() { players.get("online").cloned().unwrap_or(Value::Null) } else { Value::Null },
                "max": players.get("max"), "names": names, "xmx_mb": xmx,
                "mem_hist": if alive { st.mem_hist.iter().copied().collect() } else { vec![] },
                "uptime_s": st.started_at.filter(|_| alive).map(|t| t.elapsed().as_secs()),
                "can_force": Self::can_force(&st),
                "force_wait_s": st.cfg["stop_wait"].as_u64().unwrap_or(STOP_GRACE.as_secs()).clamp(120,600),
                "stopping_s": st.stop_sent_at.filter(|_| st.phase == Phase::Stopping).map(|t| t.elapsed().as_secs()),
            });
            (v, if alive { st.child.as_ref().map(Child::id) } else { None })
        };
        v["mem_mb"] = json!(pid.and_then(sys::tree_mem_mb));
        v
    }
}

/// 開伺服器子程序：stdout 和 stderr 接到同一條管線，主控台才看得到錯誤。
/// Windows 把指令列原封不動交給系統（1.x 傳整串字串給 Popen）；Mac 照 sh 規則拆字，並開新程序群組讓強制結束能整組關。
fn spawn(launch: &Launch, dir: &Path) -> io::Result<(Child, io::PipeReader)> {
    let (reader, writer) = io::pipe()?;
    let mut cmd: Command;
    if cfg!(windows) {
        let (exe, end) = launch::first_token(&launch.cmd).ok_or_else(|| io::Error::other("指令是空的"))?;
        cmd = platform::command(exe);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let rest = launch.cmd[end..].trim_start();
            if !rest.is_empty() {
                cmd.raw_arg(rest);
            }
        }
        let _ = end;
    } else {
        let parts = match launch.cmd.strip_prefix("/bin/sh ./") {
            Some(script) if launch.mode == Mode::Script => vec!["/bin/sh".into(), format!("./{script}")],
            _ => launch::sh_split(&launch.cmd).filter(|p| !p.is_empty()).ok_or_else(|| io::Error::other("看不懂啟動指令"))?,
        };
        cmd = platform::command(&parts[0]);
        cmd.args(&parts[1..]);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
    }
    cmd.current_dir(dir).stdin(Stdio::piped()).stdout(writer.try_clone()?).stderr(writer);
    let child = cmd.spawn()?;
    drop(cmd); // 父程序手上的寫入端要關掉，伺服器結束時讀取端才會收到結尾
    Ok((child, reader))
}

/// Windows 的結束代碼是無號整數（1.x 顯示 3221225786 這種）；Mac 被訊號砍掉時照 Python 顯示負的訊號編號。
fn exit_code(status: std::process::ExitStatus) -> i64 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return -i64::from(sig);
        }
    }
    match status.code() {
        Some(c) if cfg!(windows) => i64::from(c as u32),
        Some(c) => i64::from(c),
        None => -1,
    }
}

fn varint(mut n: u32, out: &mut Vec<u8>) {
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        out.push(if n != 0 { b | 0x80 } else { b });
        if n == 0 {
            return;
        }
    }
}

fn read_varint(s: &mut impl Read) -> io::Result<u32> {
    let (mut num, mut shift) = (0u32, 0);
    loop {
        let mut c = [0u8];
        s.read_exact(&mut c)?;
        num |= u32::from(c[0] & 0x7F).checked_shl(shift).ok_or_else(|| io::Error::other("varint 太長"))?;
        if c[0] & 0x80 == 0 {
            return Ok(num);
        }
        shift += 7;
    }
}

/// Minecraft 狀態查詢（Server List Ping，遊戲列表看到的那份資料）；連不上或格式不對回錯誤。
pub fn slp_ping(port: u16, timeout: Duration) -> io::Result<Value> {
    let mut s = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), timeout)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    let host = b"127.0.0.1";
    let mut hs = Vec::new();
    varint(0, &mut hs);
    varint(767, &mut hs);
    varint(host.len() as u32, &mut hs);
    hs.extend_from_slice(host);
    hs.extend_from_slice(&port.to_be_bytes());
    varint(1, &mut hs);
    let mut msg = Vec::new();
    varint(hs.len() as u32, &mut msg);
    msg.extend(hs);
    msg.extend([1, 0]);
    s.write_all(&msg)?;
    read_varint(&mut s)?;
    read_varint(&mut s)?;
    let size = read_varint(&mut s)? as usize;
    if size > 1 << 21 {
        return Err(io::Error::other("狀態資料太大"));
    }
    let mut buf = vec![0; size];
    s.read_exact(&mut buf)?;
    serde_json::from_str(&String::from_utf8_lossy(&buf)).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_round_trip() {
        for n in [0, 1, 127, 128, 767, 25565, u32::MAX] {
            let mut b = Vec::new();
            varint(n, &mut b);
            assert_eq!(read_varint(&mut b.as_slice()).unwrap(), n);
        }
        assert!(read_varint(&mut [0xFFu8; 6].as_slice()).is_err(), "太長的 varint 要擋");
    }

    #[test]
    fn icon_whitelist_unique() {
        let mut ids = ICON_IDS.to_vec();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 93);
    }
}
