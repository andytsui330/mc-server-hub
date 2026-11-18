//! 平台本體（1.x 的 Hub 類別）：伺服器清單、建服工作、設定存檔、每 3 秒巡檢。
//! 對外只有 `call(路徑, 參數)`，路徑與回傳格式照 1.x 的 /api/…，畫面換外殼也不用改。
use crate::commands::{build_command, builtin, clean_custom_commands, commands_for, fields_json, join_text};
use crate::create::{self, Catalog, Progress};
use crate::server::{launch, sys, Blocked, Server, ICON_COLORS, ICON_IDS};
use crate::settings::{clean_links, Settings};
use crate::{java, net, platform, UserError};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::collections::HashMap;

const ACCENTS: [&str; 6] = ["#5cc8ff", "#ff7ab6", "#3ddc97", "#ffb547", "#a98bff", "#ff8a4c"];

struct Job {
    state: &'static str,
    id: Option<String>,
    error: Option<String>,
    help_url: Option<String>,
    progress: Arc<Mutex<Progress>>,
}

pub struct Hub {
    settings: Mutex<Settings>,
    servers: Mutex<Vec<Server>>,
    job: Mutex<Option<Job>>,
    update: Mutex<Option<String>>,
    catalog: Catalog,
    data_dir: PathBuf,
    operation: Mutex<bool>,
    mod_plans:Mutex<HashMap<String,(String,crate::mods::InstallPlan)>>,
    java_plans:Mutex<HashMap<String,(String,crate::java_install::TemurinRelease)>>,
    java_work:Mutex<Value>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 1.x 的 _reply：沒事回 ok、字串當錯誤、物件原樣。
fn reply(r: Result<(), String>) -> Value {
    match r {
        Ok(()) => json!({"ok": true}),
        Err(e) => json!({"error": e}),
    }
}

fn user_err(e: UserError) -> Value {
    json!({"error": e.msg, "help_url": e.help_url})
}

fn blocked(b: Blocked) -> Value {
    match b {
        Blocked::NeedEula => json!({"need": "eula"}),
        Blocked::Error(e) if e.help_url.is_some() => user_err(e),
        Blocked::Error(e) => json!({"error": e.msg}),
    }
}

impl Hub {
    pub fn new(data_dir: &Path) -> std::io::Result<Arc<Hub>> {
        let settings = Settings::load(data_dir)?;
        let servers = settings.data["servers"].as_array().into_iter().flatten().filter(|c| c["id"].is_string()).cloned().map(Server::new).collect();
        Ok(Arc::new(Hub {
            settings: Mutex::new(settings),
            servers: Mutex::new(servers),
            job: Mutex::new(None),
            update: Mutex::new(None),
            catalog: Catalog::default(),
            data_dir: data_dir.to_path_buf(),
            operation: Mutex::new(false),
            mod_plans:Mutex::new(HashMap::new()),java_plans:Mutex::new(HashMap::new()),
            java_work:Mutex::new(json!({"busy":false,"text":""})),
        }))
    }

    fn all(&self) -> Vec<Server> {
        lock(&self.servers).clone()
    }

    fn find(&self, id: &Value) -> Option<Server> {
        self.all().into_iter().find(|s| Value::String(s.id()) == *id)
    }

    /// 伺服器清單以各台自己的 cfg 為準，整份寫回設定檔（其他欄位原樣保留）。
    fn save(&self) -> Result<(), String> {
        let list: Vec<Value> = self.all().iter().map(Server::cfg).collect();
        let mut st = lock(&self.settings);
        st.data.insert("servers".into(), Value::Array(list));
        st.save().map_err(|e| format!("設定檔寫不進去：{e}"))
    }

    /// 還有伺服器在跑、在備份，或正在建服：平台不能關。
    pub fn busy(&self) -> bool {
        lock(&self.java_work)["busy"]==true || self.all().iter().any(|s| s.alive() || s.backup_busy() || s.maintenance_busy()) || lock(&self.job).as_ref().is_some_and(|j| j.state == "running")
    }

    pub fn preferences(&self) -> Value { lock(&self.settings).preferences() }
    pub fn update_enabled(&self) -> bool { lock(&self.settings).update_check() }
    pub fn data_dir(&self) -> &Path { &self.data_dir }

    pub fn begin_update(&self) -> Result<(), String> {
        let mut updating = lock(&self.operation);
        if *updating { return Err("已經在更新中".into()); }
        if self.all().iter().any(Server::busy) || self.busy() { return Err("請先關閉所有伺服器，並等建服與備份完成再更新".into()); }
        *updating = true;
        Ok(())
    }
    pub fn finish_update(&self) { *lock(&self.operation) = false; }
    pub fn updating(&self) -> bool { *lock(&self.operation) }

    pub fn auto_start(self: &Arc<Self>) -> Vec<Value> {
        // 開機指定服優先；不要先被補做當日排程占住，下一次巡檢會明示跳過執行中的服。
        for s in self.all(){s.poll_status();}
        self.preferences()["auto_start"].as_array().into_iter().flatten().map(|id| self.call("/api/start", &json!({"id":id}))).collect()
    }

    fn diagnostics(&self) -> std::io::Result<PathBuf> {
        let dir = self.data_dir.join("reports");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("diagnostic-{}.json", random_hex(16)));
        // 僅列白名單數值，完全不匯出設定、主控台、玩家、名稱或個人路徑。
        let report = json!({"app_version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
            "server_count":self.all().len(),"busy":self.busy(),"report_format":1});
        crate::fs_safe::write_atomic(&path, serde_json::to_string_pretty(&report)?.as_bytes())?;
        Ok(path)
    }

    /// 巡檢（外殼每 3 秒呼叫一次）。
    pub fn poll(&self) {
        for s in self.all() {
            s.poll_status();
        }
        self.run_backup_schedule(&platform::local_stamp());
        self.run_automation(&platform::local_stamp());
    }

    pub fn run_automation(&self,stamp:&str){
        if lock(&self.java_work)["busy"]==true{return;}
        if stamp.len()!=15||!stamp.is_ascii(){return;}let Ok(guard)=self.operation.try_lock()else{return};if *guard{return;}
        let day=&stamp[..8];let minute=format!("{}:{}",&stamp[9..11],&stamp[11..13]);
        for s in self.all(){let cfg=s.cfg();let policy=crate::automation::Policy::parse(&cfg["automation"]).unwrap_or_default();let mut actions=vec![];
            if policy.schedule_enabled&&minute>=policy.schedule_time&&cfg["restart_last_day"]!=day{s.set_cfg("restart_last_day",json!(day));if self.save().is_ok(){actions.extend(s.schedule_restart());}else{s.set_cfg("restart_last_day",Value::Null);s.automation_failed("每日紀錄寫不進去");}}
            actions.extend(s.automation_tick());
            for action in actions{match action{crate::automation::Action::Broadcast(seconds)=>{let _=s.send(&format!("say 平台將在約 {seconds} 秒後存檔重開，請先到安全位置"));s.add_log(format!("» 平台：排程重開倒數 {seconds} 秒"));},crate::automation::Action::Stop=>{if let Err(e)=s.stop(){s.automation_failed(&e);}},crate::automation::Action::Start=>{if let Err(e)=self.start(&s){s.automation_failed(&format!("{e:?}"));}}}}
        }
    }

    fn mod_target(&self,s:&Server,body:&Value)->crate::Result<crate::mods::Target>{
        let cfg=s.cfg();let inspected=create::inspect_server(&s.dir());let version=inspected["version"].as_str().filter(|v|!v.is_empty()).or_else(||cfg["version"].as_str().and_then(|v|v.split_whitespace().find(|p|p.contains('.')&&p.chars().all(|c|c.is_ascii_digit()||c=='.'))));
        let requested=body["minecraft"].as_str().filter(|s|!s.is_empty());if version.zip(requested).is_some_and(|(a,b)|a!=b){return Err(UserError::new("模組遊戲版本與這台伺服器不同，請重新確認"));}let minecraft=version.or(requested).unwrap_or("").to_string();
        let current_loader=cfg["loader"].as_str().filter(|s|!s.is_empty()).map(str::to_ascii_lowercase);let requested_loader=body["loader"].as_str().filter(|s|!s.is_empty()).map(str::to_ascii_lowercase);if current_loader.as_ref().zip(requested_loader.as_ref()).is_some_and(|(a,b)|a!=b){return Err(UserError::new("模組載入器與這台伺服器不同"));}let loader=current_loader.or(requested_loader).unwrap_or_default();
        if !["fabric","forge","neoforge"].contains(&loader.as_str())||minecraft.is_empty(){return Err(UserError::new("請確認 Minecraft 版本與 Fabric／Forge／NeoForge 載入器；原版不能安裝模組"));}
        Ok(crate::mods::Target{minecraft,loader})
    }

    fn managed_java(&self,s:&Server)->Option<PathBuf>{let need=s.cfg()["java_need"].as_u64().map(|n|n as u32).or_else(||create::inspect_server(&s.dir())["java"].as_u64().map(|n|n as u32)).unwrap_or(8);crate::java_install::installed_java(&self.data_dir,need).into_iter().filter(|(_,p)|java::java_major(p).is_some_and(|m|m>=need)).min_by_key(|(m,_)|*m).map(|(_,p)|p)}

    fn java_api(self:&Arc<Self>,path:&str,body:&Value)->Value{
        if path=="/api/java/status"{return lock(&self.java_work).clone();}
        let key=body["id"].as_str().unwrap_or("platform").to_string();
        if path=="/api/java/plan"{let major=body["major"].as_u64().filter(|n|(8..=99).contains(n)).unwrap_or(21) as u32;let arch=if platform::OS==platform::Os::Mac{platform::mac_arch()}else{"x64"};return match crate::java_install::resolve_temurin(major,platform::OS,arch){Ok(r)=>{let token=random_hex(24);let view=json!({"token":token,"major":r.major,"version":r.version,"size":r.size,"sha256":r.sha256,"url":r.archive_url});lock(&self.java_plans).insert(key,(token,r));view},Err(e)=>user_err(e)};}
        if body["confirm"]!=true{return json!({"error":"請先確認下載 Java 的版本與來源"});}
        let release={let plans=lock(&self.java_plans);match plans.get(&key){Some((token,r))if body["token"]==*token=>r.clone(),_=>return json!({"error":"Java下載資訊已失效，請重新檢查"})}};
        if self.busy()||self.all().iter().any(Server::busy){return json!({"error":"請先關閉伺服器、取消待重開，並等其他工作完成"});}
        *lock(&self.java_work)=json!({"busy":true,"text":"準備下載 Java…"});lock(&self.java_plans).remove(&key);let me=self.clone();std::thread::spawn(move||{
            let result=crate::java_install::install_temurin(&me.data_dir,&release,true,&mut |pct|*lock(&me.java_work)=json!({"busy":true,"text":format!("正在下載 Java：{:.0}%",pct*100.0)})).and_then(|path|{if java::java_major(&path)!=Some(release.major){return Err(UserError::new("安裝後 Java 版本核對失敗，未設定使用"));}Ok(path)});
            *lock(&me.java_work)=match result{Ok(path)=>json!({"busy":false,"ok":true,"text":"Java 已安裝到平台專用資料夾；未修改系統或既有啟動檔","path":path}),Err(e)=>json!({"busy":false,"ok":false,"text":e.msg})};
        });json!({"ok":true})
    }

    fn apply_backup_policy(&self,s:&Server){
        use sha2::{Digest,Sha256};
        let p=self.preferences();let configured=p["backup_dir"].as_str().unwrap_or_default();
        let dir=s.dir();let canonical=dir.canonicalize().unwrap_or(dir.clone());
        let hash=format!("{:x}",Sha256::digest(canonical.to_string_lossy().as_bytes()));
        let root=if configured.is_empty(){dir.join("backups")}else{Path::new(configured).join(format!("server-{}",&hash[..16]))};
        s.set_backup_policy(root,p["backup_keep"].as_u64().unwrap_or(10) as usize);
    }

    /// 每天只嘗試一次；平台未開不補昨天，執行中跳過並留下可見原因。
    pub fn run_backup_schedule(&self,stamp:&str){
        if stamp.len()!=15||!stamp.is_ascii(){return;}
        let Ok(guard)=self.operation.try_lock()else{return};if *guard{return;}
        let p=self.preferences();if p["backup_schedule"]!=true{return;}
        let Some(time)=p["backup_time"].as_str().filter(|t|crate::settings::valid_backup_time(t)) else{return};
        let minute=format!("{}:{}",&stamp[9..11],&stamp[11..13]);if minute.as_str()<time{return;}
        let day=&stamp[..8];
        for s in self.all(){
            if s.cfg()["backup_last_day"]==day{continue;}
            s.set_cfg("backup_last_day",json!(day));
            if let Err(e)=self.save(){s.set_cfg("backup_last_day",Value::Null);s.add_log(format!("» 平台：排程無法記錄，沒有備份：{e}"));continue;}
            self.apply_backup_policy(&s);
            let result=if s.busy(){Err("伺服器執行中或正在處理資料，今天排程已跳過；請關機後手動備份".to_string())}else{s.backup()};
            let text=result.err().unwrap_or_else(||"已開始每日排程備份".into());s.set_cfg("backup_schedule_note",json!(text));s.add_log(format!("» 平台：{text}"));let _=self.save();
        }
    }

    /// 使用者沒關新版提醒才連線（背景呼叫一次）。
    pub fn check_update(&self) {
        let on = lock(&self.settings).update_check();
        *lock(&self.update) = if on { net::check_update() } else { None };
    }

    fn register(&self, dir: &Path, name: &str, version: &str, loader: &str, java_need: Option<u32>, bat: &str) -> Result<Server, String> {
        let s = {
            let mut list = lock(&self.servers);
            let id = loop {
                let id = format!("s{}", random_hex(8));
                if !list.iter().any(|s| s.id() == id) {
                    break id;
                }
            };
            let cfg = json!({"id": id, "name": name, "version": version, "dir": dir.display().to_string(), "bat": bat, "loader": loader,
                             "java_need": java_need, "accent": ACCENTS[list.len() % ACCENTS.len()], "address": "",
                             "stop_wait":self.preferences()["stop_wait"],"auto_backup":self.preferences()["auto_backup"]});
            let s = Server::new(cfg);
            list.push(s.clone());
            s
        };
        self.save()?;
        Ok(s)
    }

    fn add_existing(&self, folder: &Value, name: &Value) -> Value {
        let folder = folder.as_str().unwrap_or("").trim().trim_matches('"');
        let d = PathBuf::from(folder);
        if folder.is_empty() || !d.is_absolute() || !d.is_dir() {
            return json!({"error": "找不到這個資料夾，請重新選擇"});
        }
        let key = |p: &Path| std::path::absolute(p).map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default();
        // shortcut: 比對不分大小寫（照 1.x 的 normcase 在 Windows 的行為），Mac 區分大小寫的磁碟極少見, 有人回報再分平台
        if self.all().iter().any(|s| key(&s.dir()) == key(&d)) {
            return json!({"error": "這個資料夾已經在清單裡了"});
        }
        let Some(bat) = launch::find_bat(&d, platform::OS) else {
            let script = platform::start_script_name(platform::OS);
            let ext = script.split('.').nth(1).unwrap_or("");
            return json!({"error": format!("這個資料夾裡找不到 {script}（或其他含 java 指令的 {ext} 檔）。沒有伺服器的話，可以改用「建立新伺服器」。")});
        };
        let info = create::inspect_server(&d);
        let (v, l) = (info["version"].as_str().unwrap_or(""), info["loader"].as_str().unwrap_or(""));
        let label = [v, l].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join(" · ");
        let label = if label.is_empty() { "版本未知".to_string() } else { label };
        let name: String = name.as_str().unwrap_or("").trim().chars().take(40).collect();
        let name = if name.is_empty() { d.file_name().map(|n| n.to_string_lossy().chars().take(40).collect()).unwrap_or_default() } else { name };
        match self.register(&d, &name, &label, l, None, &bat) {
            Ok(s) => json!({"ok": true, "id": s.id()}),
            Err(e) => json!({"error": e}),
        }
    }

    fn remove(&self, s: &Server) -> Result<(), String> {
        if s.busy() {
            return Err("這台還在執行或備份中，請先關機".into());
        }
        lock(&self.servers).retain(|x| x.id() != s.id());
        let mut st=lock(&self.settings);
        if let Some(ids)=st.data.get_mut("preferences").and_then(|p|p.get_mut("auto_start")).and_then(Value::as_array_mut){ids.retain(|id|id!=&json!(s.id()));}
        drop(st);
        self.save()
    }

    fn save_settings(&self, body: &Value) -> crate::Result<Value> {
        let mut changes = serde_json::Map::new();
        if let Some(v) = body.get("preferences") {
            let raw=lock(&self.settings).data.get("preferences").cloned().unwrap_or_else(||json!({}));
            let mut current=self.preferences();
            if let Some(obj)=raw.as_object(){for(k,value)in obj{if current.get(k).is_none(){current[k]=value.clone();}}}
            let prefs = crate::settings::clean_preferences(&current, v)?;
            if prefs["auto_start"].as_array().unwrap().iter().any(|id| self.find(id).is_none()) { return Err(UserError::new("自動開機清單有不存在的伺服器")); }
            changes.insert("preferences".into(), prefs);
        }
        if let Some(v) = body.get("custom_commands") {
            changes.insert("custom_commands".into(), Value::Array(clean_custom_commands(v)?));
        }
        if let Some(v) = body.get("links") {
            changes.insert("links".into(), Value::Array(clean_links(v)?));
        }
        if let Some(v) = body.get("join_template") {
            // 1.x：str(值 or "")，空值一律當空字串
            let empty = [json!(null), json!(false), json!(0), json!(""), json!([]), json!({})];
            let t = if empty.contains(v) { String::new() } else { crate::settings::py_str(Some(v)) };
            if t.chars().count() > 2000 {
                return Err(UserError::new("加入說明最多 2000 字"));
            }
            changes.insert("join_template".into(), json!(t.replace("\r\n", "\n")));
        }
        if let Some(v) = body.get("update_check") {
            let on = *v == json!(true);
            changes.insert("update_check".into(), json!(on));
            if !on {
                *lock(&self.update) = None;
            }
        }
        let mut st = lock(&self.settings);
        let old = st.data.clone();
        st.data.extend(changes);
        if let Err(e) = st.save() { st.data = old; return Err(UserError::new(format!("設定檔寫不進去：{e}"))); }
        let result=st.view();drop(st);for s in self.all(){self.apply_backup_policy(&s);}Ok(result)
    }

    fn start(&self, s: &Server) -> Result<(), Blocked> {
        self.apply_backup_policy(s);
        s.set_runtime_java(self.managed_java(s));
        let cfg = s.cfg();
        let need = cfg["java_need"].as_u64().filter(|&n| n > 0).map(|n| n as u32).or_else(|| create::inspect_server(&s.dir())["java"].as_u64().map(|n| n as u32));
        s.set_cfg("stop_wait", self.preferences()["stop_wait"].clone());
        s.start(need)
    }

    fn run_command(&self, s: &Server, body: &Value) -> Value {
        let cid = crate::settings::py_str(body.get("cmd"));
        let custom = lock(&self.settings).data.get("custom_commands").and_then(Value::as_array).cloned().unwrap_or_default();
        let Some(cmd) = builtin().into_iter().chain(custom).find(|c| c["id"].as_str() == Some(&cid)) else {
            return json!({"error": "找不到這個指令"});
        };
        if cmd["special"] == "clear_hostile" {
            if body["confirm"] != "strong" {
                return json!({"error": "這是會刪東西或給權限的指令，要先在畫面上強確認"});
            }
            return reply(s.clear_hostile());
        }
        let label = s.cfg()["version"].as_str().unwrap_or("").to_string();
        let text = match build_command(&cmd, &body["values"], &body["confirm"], &label) {
            Ok(t) => t,
            Err(e) => return json!({"error": e.msg}),
        };
        if let Err(e) = s.send(&text) {
            return json!({"error": e});
        }
        s.add_log(format!("> {text}"));
        json!({"ok": true, "sent": text})
    }

    /// 每台的小設定：圖示與顏色（開著也能改）、記憶體（要先關機）、關機後自動備份。
    fn server_settings(&self, s: &Server, body: &Value) -> Result<(), String> {
        if body.get("icon").is_some() || body.get("color").is_some() {
            let cfg = s.cfg();
            let icon = body.get("icon").unwrap_or(&cfg["icon"]);
            let color = body.get("color").unwrap_or(&cfg["color"]);
            if !icon.as_str().is_some_and(|i| ICON_IDS.contains(&i)) {
                return Err("沒有這個圖示，請重新選一個".into());
            }
            if !color.as_str().is_some_and(|c| ICON_COLORS.contains(&c)) {
                return Err("沒有這個顏色，請重新選一個".into());
            }
            let (icon, color) = (icon.clone(), color.clone());
            s.set_cfg("icon", icon);
            s.set_cfg("color", color);
            self.save()?;
        }
        if let Some(v) = body.get("ram_gb") {
            if s.busy() {
                return Err("要先關機才能改記憶體".into());
            }
            let gb = match v {
                Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
                Value::String(t) => t.trim().parse().ok(),
                _ => None,
            }
            .ok_or("記憶體要選一個數字")?;
            let top = sys::total_ram_gb().min(64) as i64;
            if !(1..=top).contains(&gb) {
                return Err(format!("記憶體要在 1 到 {top} GB 之間"));
            }
            launch::set_ram(&s.dir().join(bat_of(&s.cfg())), gb as u32).map_err(|e| e.msg)?;
        }
        if let Some(v) = body.get("auto_backup") {
            s.set_cfg("auto_backup", json!(*v == json!(true)));
            self.save()?;
        }
        Ok(())
    }

    fn server_info(&self, s: &Server) -> Value {
        let cfg = s.cfg();
        let dir = s.dir();
        let seed = launch::read_prop(&dir, "level-seed").unwrap_or_default();
        let (pid, java) = s.pid_java();
        let template = lock(&self.settings).view()["join_template"].as_str().unwrap_or("").to_string();
        json!({"seed": seed, "seed_note": if seed.is_empty() { json!("server.properties 沒寫種子碼（隨機產生）；伺服器開著時可用「查種子碼」指令看") } else { Value::Null },
               "ram_gb": launch::read_ram_gb(&dir.join(bat_of(&cfg))), "ram_total_gb": sys::total_ram_gb(),
               "auto_backup": cfg["auto_backup"] == json!(true), "pid": pid, "java": java, "dir": dir.display().to_string(),
               "join_text": join_text(&template, &cfg)})
    }

    fn start_create(self: &Arc<Self>, req: &Value) -> Result<(), String> {
        let mut req=req.clone();
        let prefs=self.preferences();
        for (field,key) in [("ram_gb","default_ram"),("parent","server_dir"),("java_path","java_path")] {
            if req.get(field).is_none() { req[field]=prefs[key].clone(); }
        }
        if req["java_path"].as_str()==Some(""){let found:Vec<_>=crate::java_install::installed_java(&self.data_dir,8).into_iter().filter(|(m,p)|java::java_major(p)==Some(*m)).collect();if !found.is_empty()&&let Ok(status)=self.catalog.java_status(req["version"].as_str().unwrap_or(""),&found)&&status["ok"]==true{req["java_path"]=status["path"].clone();}}
        let mut job = lock(&self.job);
        if job.as_ref().is_some_and(|j| j.state == "running") {
            return Err("已經在建立另一台了，請等它完成".into());
        }
        let plan = create::plan(&req)?;
        let progress = Arc::new(Mutex::new(Progress { msg: "準備中…".into(), ..Default::default() }));
        *job = Some(Job { state: "running", id: None, error: None, help_url: None, progress: progress.clone() });
        let me = self.clone();
        std::thread::spawn(move || {
            let used: Vec<u16> = me.all().iter().map(Server::port).collect();
            let r = create::create(&plan, &me.catalog, &progress, &me.data_dir, &used).and_then(|m| {
                let s = me.register(&plan.target, &plan.name, &m.label, m.loader, Some(m.java_need), m.bat).map_err(UserError::new)?;
                Ok((s, m))
            });
            let mut j = lock(&me.job);
            let Some(job) = j.as_mut() else { return };
            match r {
                Ok((s, _)) => {
                    job.id = Some(s.id());
                    lock(&progress).msg = "第一次開機（要產生世界，約 1–3 分鐘）…".into();
                    drop(j);
                    let err = me.start(&s).err().map(|b| match b {
                        Blocked::NeedEula => "要先同意 EULA".to_string(),
                        Blocked::Error(e) => e.msg,
                    });
                    let mut j = lock(&me.job);
                    if let Some(job) = j.as_mut() {
                        (job.state, job.error) = ("done", err);
                    }
                }
                Err(e) => (job.state, job.error, job.help_url) = ("error", Some(e.msg), e.help_url),
            }
        });
        Ok(())
    }

    fn job_json(&self) -> Value {
        let job = lock(&self.job);
        let Some(j) = job.as_ref() else { return Value::Null };
        let p = lock(&j.progress);
        let mut v = json!({"state": j.state, "msg": p.msg, "pct": p.pct, "id": j.id, "error": j.error, "help_url": j.help_url});
        if !p.lines.is_empty() {
            v["lines"] = json!(p.lines);
        }
        v
    }

    /// 畫面呼叫後端的唯一入口：路徑與回傳格式照 1.x 的 /api/…。
    pub fn call(self: &Arc<Self>, path: &str, body: &Value) -> Value {
        // 與 begin_update 共用一把鎖：更新前檢查與開服不會交錯。
        let mutation = matches!(path, "/api/start"|"/api/stop"|"/api/force"|"/api/cmd"|"/api/create"|"/api/add"|"/api/remove"|"/api/backup"|"/api/restore"|"/api/reset-settings"|"/api/server-settings"|"/api/players/action"|"/api/eula"|"/api/mods/apply"|"/api/java/install"|"/api/automation"|"/api/automation/cancel") || (path=="/api/settings" || path=="/api/properties") && body.as_object().is_some_and(|o| o.contains_key("values")||path=="/api/settings"&&!o.is_empty());
        let guard = mutation.then(|| lock(&self.operation));
        if guard.as_ref().is_some_and(|g| **g) { return json!({"error":"平台正在更新，完成前不能變更設定或開服"}); }
        if mutation&&lock(&self.java_work)["busy"]==true&&!matches!(path,"/api/stop"|"/api/force"|"/api/cmd"|"/api/automation/cancel"){return json!({"error":"平台正在安裝 Java，請等完成再變更或開服"});}
        match path {
            "/api/java/plan"|"/api/java/install"|"/api/java/status"=>self.java_api(path,body),
            "/api/diagnostics" => self.diagnostics().map(|p| json!({"ok":true,"path":p})).unwrap_or_else(|e| json!({"error":format!("診斷檔寫不進去：{e}")})),
            "/api/reset-settings" if body["confirm"]!=true => json!({"error":"請先確認重設設定"}),
            "/api/reset-settings" => {
                if self.busy() { return json!({"error":"請先關機並等備份、建服完成"}); }
                let mut st=lock(&self.settings);
                match st.reset() { Ok(p)=> { let mut v=st.view(); v["backup"]=json!(p); v }, Err(e)=>json!({"error":format!("重設失敗：{e}")}) }
            }
            "/api/state" => json!({"servers": self.all().iter().map(Server::snapshot).collect::<Vec<_>>(), "job": self.job_json(), "update": *lock(&self.update)}),
            "/api/settings" if body.as_object().is_none_or(|o| o.is_empty()) => lock(&self.settings).view(),
            "/api/settings" => self.save_settings(body).unwrap_or_else(|e| json!({"error": e.msg})),
            "/api/commands" => {
                let label = self.find(&body["id"]).map(|s| s.cfg()["version"].as_str().unwrap_or("").to_string()).unwrap_or_default();
                let custom = lock(&self.settings).data.get("custom_commands").and_then(Value::as_array).cloned().unwrap_or_default();
                json!({"fields": fields_json(), "commands": commands_for(&label, &custom)})
            }
            "/api/mc/versions" => {
                let home = std::env::home_dir().unwrap_or_default();
                let prefs=self.preferences();
                let parent=prefs["server_dir"].as_str().filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(||home.join("MCServers"));
                self.catalog.mc_versions(&parent).map(|mut v| {v["default_ram"]=prefs["default_ram"].clone();v}).unwrap_or_else(user_err)
            }
            "/api/mc/java" => {
                let prefs=self.preferences();
                let mut found=match prefs["java_path"].as_str().filter(|s| !s.is_empty()) {Some(p)=>java::java_major(Path::new(p)).map(|v|vec![(v,PathBuf::from(p))]).unwrap_or_default(),None=>java::installed_javas()};
                if prefs["java_path"].as_str().is_none_or(str::is_empty){found.extend(crate::java_install::installed_java(&self.data_dir,8).into_iter().filter(|(m,p)|java::java_major(p)==Some(*m)));}
                self.catalog.java_status(body["version"].as_str().unwrap_or(""), &found).unwrap_or_else(user_err)
            }
            "/api/add" => self.add_existing(&body["dir"], &body["name"]),
            "/api/create" => reply(self.start_create(body)),
            _ => {
                let Some(s) = self.find(&body["id"]) else { return json!({"error": "找不到這台伺服器"}) };
                self.call_server(&s, path, body)
            }
        }
    }

    fn call_server(&self, s: &Server, path: &str, body: &Value) -> Value {
        match path {
            "/api/automation" if body.get("policy").is_none()=>json!({"policy":crate::automation::Policy::parse(&s.cfg()["automation"]).unwrap_or_default().to_json()}),
            "/api/automation"=>{let p=match crate::automation::Policy::parse(&body["policy"]){Ok(p)=>p,Err(e)=>return user_err(e)};let old=s.cfg()["automation"].clone();s.set_cfg("automation",p.to_json());if let Err(e)=self.save(){s.set_cfg("automation",old);return json!({"error":e});}
                if !p.crash_enabled&&!p.schedule_enabled{s.cancel_restart();}json!({"ok":true,"policy":p.to_json()})},
            "/api/automation/cancel"=>{s.cancel_restart();json!({"ok":true})},
            "/api/mods/list"=>crate::mods::list_installed(&s.dir()).unwrap_or_else(user_err),
            "/api/mods/search"|"/api/mods/plan"|"/api/mods/updates"|"/api/mods/update-plan"=>{
                let target=match self.mod_target(s,body){Ok(t)=>t,Err(e)=>return user_err(e)};
                let mut client=crate::mods::Modrinth::new(|url:&str|net::fetch_json_from(url,&net::Source::Modrinth));
                if path=="/api/mods/search"{return client.search(&target,body["query"].as_str().unwrap_or("")).map(|hits|json!({"hits":hits})).unwrap_or_else(user_err);}
                if path=="/api/mods/updates"{return client.check_updates(&target,&s.dir()).map(|updates|json!({"updates":updates})).unwrap_or_else(user_err);}
                let plan=if path=="/api/mods/plan"{client.plan_install(&target,body["project_id"].as_str().unwrap_or(""),&s.dir())}else{let ids:Vec<String>=body["projects"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();client.plan_updates(&target,&ids,&s.dir())};
                match plan{Ok(plan)=>{let token=random_hex(24);let mut view=plan.to_json();view["token"]=json!(token);lock(&self.mod_plans).insert(s.id(),(token,plan));view},Err(e)=>user_err(e)}
            },
            "/api/mods/apply"=>{
                if body["confirm"]!=true{return json!({"error":"請先確認模組變更清單"});}
                let plan={let plans=lock(&self.mod_plans);match plans.get(&s.id()){Some((token,plan))if body["token"]==*token=>plan.clone(),_=>return json!({"error":"變更清單已失效，請重新檢查"})}};
                if let Err(e)=s.begin_maintenance(){return json!({"error":e});}lock(&self.mod_plans).remove(&s.id());let me=s.clone();std::thread::spawn(move||{let r=crate::mods::apply_plan(&me.dir(),&plan,|url,path,hash,size|{me.maintenance_progress("正在下載並核對模組…");net::download_from(url,path,Some(hash),Some(size),&mut |_|{},&net::Source::Modrinth)});me.finish_maintenance(r);});json!({"ok":true})
            },
            "/api/preflight" => {s.set_runtime_java(self.managed_java(s));let cfg=s.cfg();let need=cfg["java_need"].as_u64().map(|n|n as u32).or_else(||create::inspect_server(&s.dir())["java"].as_u64().map(|n|n as u32));s.preflight_report(need)},
            "/api/properties" => s.properties(body).unwrap_or_else(user_err),
            "/api/backups" => {self.apply_backup_policy(s);crate::backup::list_backups(&s.dir(),&s.backup_root()).unwrap_or_else(user_err)},
            "/api/restore" if body["confirm"]!=true => json!({"error":"請先確認還原；會先建立原世界快照"}),
            "/api/restore" => {self.apply_backup_policy(s);reply(s.restore(body["name"].as_str().unwrap_or_default()))},
            "/api/players" => s.players().unwrap_or_else(user_err),
            "/api/players/action" => s.player_action(body).unwrap_or_else(user_err),
            "/api/log" => {
                let (seq, lines) = s.logs_since(body["since"].as_u64().unwrap_or(0));
                json!({"seq": seq, "lines": lines})
            }
            "/api/info" => self.server_info(s),
            "/api/start" => {s.manual_start();self.start(s).map_or_else(blocked, |_| json!({"ok": true}))},
            "/api/stop" => {s.cancel_restart();reply(s.stop())},
            "/api/force" => {s.cancel_restart();reply(s.force_kill())},
            "/api/backup" => {self.apply_backup_policy(s);reply(s.backup())},
            "/api/remove" => reply(self.remove(s)),
            "/api/eula" if body["agree"] != json!(true) => json!({"error": "要勾選同意才能繼續"}),
            "/api/eula" => reply(launch::write_eula(&s.dir()).map_err(|e| format!("寫不進 eula.txt：{e}"))),
            "/api/open" => {
                let sub = match body["what"].as_str() {
                    Some("mods") => "mods",
                    Some("backups") => "backups",
                    _ => "",
                };
                self.apply_backup_policy(s);
                let target = if sub=="backups"{s.backup_root()}else{s.dir().join(sub)};
                reply(std::fs::create_dir_all(&target).and_then(|_| platform::open_path(&target)).map_err(|e| format!("打不開資料夾：{e}")))
            }
            "/api/run" => self.run_command(s, body),
            "/api/server-settings" => reply(self.server_settings(s, body)),
            "/api/cmd" => {
                let raw = crate::settings::py_str(body.get("cmd"));
                let text = raw.trim();
                let r = if raw.contains(['\n', '\r']) {
                    Err("指令不能換行".to_string())
                } else if text.is_empty() {
                    Err("指令是空的".to_string())
                } else {
                    if text.trim_start_matches('/')=="stop"{s.cancel_restart();}
                    s.send(text)
                };
                if r.is_ok() {
                    s.add_log(format!("> {text}"));
                }
                reply(r)
            }
            "/api/address" => {
                let a: String = crate::settings::py_str(body.get("address")).trim().chars().take(200).collect();
                s.set_cfg("address", json!(a));
                reply(self.save())
            }
            _ => json!({"error": "not found"}),
        }
    }
}

fn bat_of(cfg: &Value) -> String {
    cfg["bat"].as_str().unwrap_or("start.bat").to_string()
}

fn random_hex(n: usize) -> String {
    use std::hash::{BuildHasher, RandomState};
    let mut value=String::new();while value.len()<n{let v=RandomState::new().hash_one(std::time::SystemTime::now());value.push_str(&format!("{v:016x}"));}value.truncate(n);value
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_safe::tests::temp;

    #[test]
    fn add_settings_remove_roundtrip() {
        let root = temp("hub");
        let (data, srv) = (root.join("data"), root.join("我的伺服器"));
        std::fs::create_dir_all(&srv).unwrap();
        let hub = Hub::new(&data).unwrap();
        let call = |p: &str, b: Value| hub.call(p, &b);
        // 沒有啟動檔的資料夾不收
        assert!(call("/api/add", json!({"dir": srv.display().to_string()}))["error"].as_str().unwrap().contains("找不到"));
        let script = platform::start_script_name(platform::OS);
        std::fs::write(srv.join(script), "java -Xmx2G -jar server.jar nogui\n").unwrap();
        let id = call("/api/add", json!({"dir": srv.display().to_string(), "name": "  "}))["id"].as_str().unwrap().to_string();
        assert_eq!(call("/api/add", json!({"dir": srv.display().to_string()}))["error"], "這個資料夾已經在清單裡了");
        let st = call("/api/state", json!({}));
        assert_eq!(st["servers"][0]["name"], "我的伺服器"); // 名稱留空就用資料夾名
        assert_eq!(st["servers"][0]["version"], "版本未知");
        assert_eq!(st["job"], Value::Null);
        // 每台小設定：白名單外的圖示擋下、合法的存檔；記憶體改啟動檔
        assert_eq!(call("/api/server-settings", json!({"id": id, "icon": "../x", "color": "#5fd39a"}))["error"], "沒有這個圖示，請重新選一個");
        assert_eq!(call("/api/server-settings", json!({"id": id, "icon": "pig", "color": "#5fd39a", "ram_gb": 1, "auto_backup": true})), json!({"ok": true}));
        assert_eq!(call("/api/info", json!({"id": id}))["ram_gb"], json!(1.0));
        assert_eq!(call("/api/address", json!({"id": id, "address": " play.example.com "})), json!({"ok": true}));
        assert_eq!(call("/api/cmd", json!({"id": id, "cmd": "say a\nb"}))["error"], "指令不能換行");
        assert_eq!(call("/api/start", json!({"id": "nope"}))["error"], "找不到這台伺服器");
        // 全域設定只改有帶的，其他欄位留著
        assert_eq!(call("/api/settings", json!({"join_template": null, "update_check": false}))["update_check"], false);
        assert_eq!(call("/api/settings", json!({}))["join_template"], "");
        // 重開平台：清單、圖示、地址都還在
        drop(hub);
        let hub2 = Hub::new(&data).unwrap();
        let s = &hub2.call("/api/state", &json!({}))["servers"][0];
        assert_eq!((s["icon"].as_str(), s["address"].as_str()), (Some("pig"), Some("play.example.com")));
        assert_eq!(hub2.call("/api/remove", &json!({"id": id})), json!({"ok": true}));
        assert_eq!(Hub::new(&data).unwrap().call("/api/state", &json!({}))["servers"], json!([]));
        assert!(!hub2.busy());
    }
    #[test]
    fn maintenance_keeps_server_registered_and_blocks_update(){
        let root=std::env::temp_dir().join(format!("mchub-maintenance-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));let srv=root.join("srv");std::fs::create_dir_all(&srv).unwrap();
        let hub=Hub::new(&root.join("data")).unwrap();let server=hub.register(&srv,"fixture","1.21.1","Fabric",Some(21),"start.bat").unwrap();server.begin_maintenance().unwrap();
        assert!(hub.call("/api/remove",&json!({"id":server.id()}))["error"].is_string());assert!(hub.begin_update().is_err());assert!(hub.busy());
        server.finish_maintenance(Ok(json!({"text":"done"})));assert_eq!(hub.call("/api/remove",&json!({"id":server.id()}))["ok"],true);assert!(srv.exists());
    }

    #[test]
    fn plan_tokens_support_longer_identifiers(){for n in [8,16,24,32]{let value=random_hex(n);assert_eq!(value.len(),n);assert!(value.bytes().all(|b|b.is_ascii_hexdigit()));}}

}
