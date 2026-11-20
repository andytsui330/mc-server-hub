//! 設定檔：讀寫 config.json；第一次開 2.0 時先備份 1.x 的 hub_config.json 再整份匯入。
//! 規則（計畫書第三節原則 5）：先寫暫存再替換；壞掉先存 .bad 再用預設值；不認識的欄位原樣保留。
use crate::{fs_safe, Result, UserError};
use serde_json::{json, Map, Value};
use std::io;
use std::path::{Path, PathBuf};

pub const FILE: &str = "config.json";
pub const LEGACY_FILE: &str = "hub_config.json";
pub const LEGACY_BACKUP: &str = "hub_config.1x-backup.json";

pub fn default_preferences() -> Value {
    json!({"launch_at_login":false,"start_hidden":false,"close_action":"quit","close_asked":false,"auto_start":[],
        "channel":"stable","theme":"dark","font_size":14,"sound":true,"volume":40,"language":"zh-TW",
        "default_ram":4,"server_dir":"","java_path":"","stop_wait":120,"auto_backup":false,
        "backup_dir":"","backup_keep":10,"backup_schedule":false,"backup_time":"04:00","backup_snapshot":true})
}

/// 只接受設定頁已知欄位，整批檢查通過才存；舊檔的未知欄位另外保留。
pub fn clean_preferences(current: &Value, patch: &Value) -> Result<Value> {
    let mut next = default_preferences();
    if let Some(saved) = current.as_object() { next.as_object_mut().unwrap().extend(saved.clone()); }
    let obj = patch.as_object().ok_or_else(|| UserError::new("設定格式不正確"))?;
    for (k,v) in obj {
        let valid = match k.as_str() {
            "launch_at_login"|"start_hidden"|"sound"|"auto_backup"|"backup_schedule"|"close_asked" => v.is_boolean(),
            "backup_snapshot" => v==true,
            "backup_keep" => v.as_u64().is_some_and(|n|(1..=100).contains(&n)),
            "backup_time" => v.as_str().is_some_and(valid_backup_time),
            "close_action" => matches!(v.as_str(), Some("quit"|"tray")),
            "channel" => matches!(v.as_str(), Some("stable"|"beta")),
            "theme" => matches!(v.as_str(), Some("dark"|"light"|"system")),
            "language" => v == "zh-TW",
            "font_size" => v.as_u64().is_some_and(|n| (12..=20).contains(&n)),
            "volume" => v.as_u64().is_some_and(|n| n<=100),
            "default_ram" => v.as_u64().is_some_and(|n| (1..=64).contains(&n)),
            "stop_wait" => v.as_u64().is_some_and(|n| (120..=600).contains(&n)),
            "auto_start" => v.as_array().is_some_and(|a| a.len()<=100 && a.iter().all(Value::is_string)),
            "server_dir"|"java_path"|"backup_dir" => v.as_str().is_some_and(|s| s.is_empty() || (Path::new(s).is_absolute() && if k=="java_path" { Path::new(s).is_file() } else { Path::new(s).is_dir() })),
            _ => false,
        };
        if !valid { return Err(UserError::new(format!("設定「{k}」不正確，請檢查選項或路徑"))); }
        next[k] = v.clone();
    }
    Ok(next)
}

pub fn valid_backup_time(s:&str)->bool {s.len()==5 && s.is_ascii() && s.as_bytes()[2]==b':' && s.as_bytes().iter().enumerate().all(|(i,c)|i==2||c.is_ascii_digit()) && s[..2].parse::<u8>().is_ok_and(|n|n<24) && s[3..].parse::<u8>().is_ok_and(|n|n<60)}

pub const DEFAULT_JOIN: &str = "【加入伺服器】\n1. 用 Minecraft Java 版（或 PrismLauncher）開 <版本>。\n\
2. 有裝模組的伺服器（<載入器>）：先裝同一個載入器，把伺服器給你的模組放進 mods 資料夾。\
Forge 和 NeoForge 的模組不能互通。\n3. 多人遊戲 → 新增伺服器 → 位址填：<連線網址>\n";

pub fn default_links() -> Value {
    json!([{"name": "Minecraft Wiki", "url": "https://minecraft.wiki/"},
           {"name": "Chunkbase（找村莊、要塞）", "url": "https://www.chunkbase.com/apps/"}])
}

#[derive(Debug)]
pub struct Settings {
    pub path: PathBuf,
    pub data: Map<String, Value>,
}

impl Settings {
    pub fn load(dir: &Path) -> io::Result<Settings> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(FILE);
        let (data, write_now) = match std::fs::read(&path) {
            Ok(bytes) => match parse(&bytes) {
                Some(d) => (d, false),
                None => {
                    // 壞掉的設定留著讓人救，並馬上寫一份預設值，免得下次開又去匯入 1.x 舊資料蓋過來
                    std::fs::rename(&path, dir.join(format!("{FILE}.{}.bad", unix_now())))?;
                    (Map::new(), true)
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => (import_1x(dir)?, true),
            Err(e) => return Err(e),
        };
        let mut s = Settings { path, data };
        s.data.entry("servers").or_insert_with(|| json!([]));
        if write_now {
            s.save()?;
        }
        Ok(s)
    }

    pub fn save(&self) -> io::Result<()> {
        let text = serde_json::to_string_pretty(&self.data).map_err(io::Error::other)?;
        fs_safe::write_atomic(&self.path, text.as_bytes())
    }

    /// 給設定頁看的值（沒設過的給預設值）。
    pub fn view(&self) -> Value {
        let d = &self.data;
        json!({
            "custom_commands": d.get("custom_commands").cloned().unwrap_or_else(|| json!([])),
            "links": d.get("links").cloned().unwrap_or_else(default_links),
            "join_template": d.get("join_template").cloned().unwrap_or_else(|| json!(DEFAULT_JOIN)),
            "update_check": self.update_check(),
            "preferences": self.preferences(),
        })
    }

    /// 使用者關掉新版提醒就完全不連線（只有明確設 false 才算關）。
    pub fn update_check(&self) -> bool {
        self.data.get("update_check") != Some(&Value::Bool(false))
    }

    pub fn preferences(&self) -> Value {
        // 每個舊值單獨驗證，壞值不會影響其他有效設定。
        let mut prefs = default_preferences();
        if let Some(values) = self.data.get("preferences").and_then(Value::as_object) {
            for (k,v) in values { if let Ok(p) = clean_preferences(&prefs, &json!({k:v})) { prefs = p; } }
        }
        // 測試頻道沒有 beta Release（2.0.1 隱藏選項）：舊設定存 beta 一律當穩定版。
        if prefs["channel"] == "beta" { prefs["channel"] = json!("stable"); }
        prefs
    }

    pub fn reset(&mut self) -> io::Result<PathBuf> {
        let backup = self.path.with_file_name(format!("config.before-reset-{}.json", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
        std::fs::copy(&self.path, &backup)?;
        let old = self.data.clone();
        if let Some(values)=self.data.get_mut("preferences").and_then(Value::as_object_mut){
            let known=default_preferences();values.retain(|k,_|known.get(k).is_none());
        }
        for k in ["update_check","custom_commands","links","join_template"] { self.data.remove(k); }
        if let Err(e) = self.save() { self.data = old; return Err(e); }
        Ok(backup)
    }
}

fn parse(bytes: &[u8]) -> Option<Map<String, Value>> {
    match serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)) {
        Ok(Value::Object(m)) => Some(m),
        _ => None,
    }
}

/// 1.x 設定整份照搬（伺服器清單、我的筆記、連結、加入說明、新版提醒開關，以及任何不認識的欄位）。
/// 1.x 的檔案本身不動，另外留一份備份；1.x 檔壞了就從空白開始，舊檔仍在原處。
fn import_1x(dir: &Path) -> io::Result<Map<String, Value>> {
    let old = dir.join(LEGACY_FILE);
    let bytes = match std::fs::read(&old) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e),
    };
    let backup = dir.join(LEGACY_BACKUP);
    if !backup.exists() {
        fs_safe::write_atomic(&backup, &bytes)?;
    }
    Ok(parse(&bytes).unwrap_or_default())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Python 的 str()：字串原樣，其他型別照 1.x 會顯示的樣子（None、True…）。
pub(crate) fn py_str(v: Option<&Value>) -> String {
    match v {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "None".into(),
        Some(Value::Bool(b)) => if *b { "True" } else { "False" }.into(),
        Some(other) => other.to_string(),
    }
}

/// 實用連結：最多 20 個，名稱 1–40 字，網址要 https:// 開頭。
pub fn clean_links(items: &Value) -> Result<Vec<Value>> {
    let items = items.as_array().filter(|a| a.len() <= 20).ok_or_else(|| UserError::new("實用連結最多 20 個"))?;
    items
        .iter()
        .map(|l| {
            let get = |k| py_str(l.as_object().and_then(|o| o.get(k)).or(Some(&Value::String(String::new()))));
            let (name, url) = (get("name").trim().to_string(), get("url").trim().to_string());
            let url_ok = url.strip_prefix("https://").is_some_and(|rest| {
                (1..=500).contains(&rest.chars().count()) && !rest.chars().any(|c| c.is_whitespace() || "<>\"'".contains(c))
            });
            if !(1..=40).contains(&name.chars().count()) || !url_ok {
                let label = if name.is_empty() { &url } else { &name };
                return Err(UserError::new(format!("連結「{label}」：名稱 1–40 字，網址要以 https:// 開頭")));
            }
            Ok(json!({"name": name, "url": url}))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mchub-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn fresh_start_gives_defaults() {
        let d = temp("fresh");
        let s = Settings::load(&d).unwrap();
        assert_eq!(s.data["servers"], json!([]));
        assert_eq!(s.view()["links"], default_links());
        assert!(s.update_check());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn imports_1x_and_keeps_unknown_fields() {
        let d = temp("import");
        let old = "{\"servers\": [{\"id\": \"a1\", \"dir\": \"D:\\\\mc\\\\伺服器\", \"name\": \"生存\", \"bat\": \"run.bat\", \"future\": 1}],\n \"update_check\": false, \"zzz\": {\"keep\": true}, \"aaa\": 2}";
        fs::write(d.join(LEGACY_FILE), old).unwrap();
        let s = Settings::load(&d).unwrap();
        assert_eq!(s.data["servers"][0]["dir"], "D:\\mc\\伺服器");
        assert_eq!(s.data["servers"][0]["future"], 1);
        assert_eq!(s.data["zzz"], json!({"keep": true}));
        assert!(!s.update_check());
        assert_eq!(s.data.keys().collect::<Vec<_>>(), ["servers", "update_check", "zzz", "aaa"], "欄位順序照原樣");
        assert_eq!(fs::read_to_string(d.join(LEGACY_FILE)).unwrap(), old, "1.x 檔不能動");
        assert_eq!(fs::read_to_string(d.join(LEGACY_BACKUP)).unwrap(), old);
        assert!(d.join(FILE).exists(), "匯入後馬上存成 2.0 設定檔");
        // 第二次開：讀 2.0 的檔，不再匯入（1.x 檔之後被改也不影響）
        fs::write(d.join(LEGACY_FILE), "{\"servers\": []}").unwrap();
        assert_eq!(Settings::load(&d).unwrap().data["servers"][0]["id"], "a1");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn broken_file_is_kept_as_bad() {
        let d = temp("bad");
        fs::write(d.join(LEGACY_FILE), "{\"servers\": [{\"id\": \"old\"}]}").unwrap();
        fs::write(d.join(FILE), "{oops").unwrap();
        let s = Settings::load(&d).unwrap();
        assert_eq!(s.data["servers"], json!([]), "壞掉就用預設值，不拿 1.x 舊資料蓋");
        let bad: Vec<_> = fs::read_dir(&d).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".bad")).collect();
        assert_eq!(fs::read_to_string(bad[0].path()).unwrap(), "{oops");
        assert_eq!(Settings::load(&d).unwrap().data["servers"], json!([]), "下次開也不會跑去匯入 1.x");
        fs::remove_dir_all(d).unwrap();
    }

    /// 拿這台電腦真正的 1.x 設定檔（複製到暫存資料夾，不碰正本）匯入，內容要一字不漏：cargo test -- --ignored
    #[test]
    #[ignore]
    fn import_this_machine_1x() {
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap();
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
        let real = crate::platform::data_dir(crate::platform::OS, appdata.as_deref(), Path::new(&home)).join(LEGACY_FILE);
        let d = temp("real");
        fs::copy(&real, d.join(LEGACY_FILE)).unwrap();
        let want: Value = serde_json::from_slice(&fs::read(&real).unwrap()).unwrap();
        let got = Settings::load(&d).unwrap();
        assert_eq!(Value::Object(got.data.clone()), want);
        println!("匯入 {} 台伺服器、{} 個欄位", got.data["servers"].as_array().unwrap().len(), got.data.len());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn save_round_trip() {
        let d = temp("save");
        let mut s = Settings::load(&d).unwrap();
        s.data.insert("join_template".into(), json!("嗨 <版本>"));
        s.save().unwrap();
        assert_eq!(Settings::load(&d).unwrap().view()["join_template"], "嗨 <版本>");
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn old_prefs_still_ask_on_close_and_beta_reads_as_stable() {
        let mut data = Map::new();
        data.insert("preferences".into(), json!({"close_action":"quit","channel":"beta"}));
        let s = Settings { path: PathBuf::new(), data };
        let p = s.preferences();
        assert_eq!(p["close_asked"], false, "2.0.0 使用者也要被問一次");
        assert_eq!(p["channel"], "stable");
        assert_eq!(clean_preferences(&p, &json!({"close_asked":true})).unwrap()["close_asked"], true);
        assert!(clean_preferences(&p, &json!({"close_asked":"yes"})).is_err());
    }
}
