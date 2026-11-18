//! 玩家與權限：執行中只產生安全指令；離線名單先備份，未知欄位原樣留存。
use crate::{Result, UserError, fs_safe};
use serde_json::{Value, json};
use std::{fs, path::Path};

const LISTS: [(&str, &str); 4] = [
    ("ops", "ops.json"),
    ("whitelist", "whitelist.json"),
    ("banned", "banned-players.json"),
    ("known", "usercache.json"),
];
fn read_list(dir: &Path, file: &str) -> Result<Vec<Value>> {
    for p in dir.ancestors() {
        let m = fs::symlink_metadata(p)
            .map_err(|e| UserError::new(format!("玩家名單路徑讀不到：{e}")))?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return Err(UserError::new("玩家名單路徑含捷徑連結，已停止"));
            }
        }
        if m.file_type().is_symlink() {
            return Err(UserError::new("玩家名單路徑含捷徑連結，已停止"));
        }
    }
    let path = fs_safe::child(dir, Path::new(file))?;
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(UserError::new(format!("讀不到 {file}：{e}"))),
    };
    let list: Value = serde_json::from_slice(&bytes)
        .map_err(|_| UserError::new(format!("{file} 格式損壞，請先修復；平台不會覆蓋")))?;
    list.as_array()
        .filter(|a| {
            a.iter()
                .all(|v| v["name"].is_string() && v["uuid"].as_str().is_some_and(valid_uuid))
        })
        .cloned()
        .ok_or_else(|| UserError::new(format!("{file} 名單格式不正確，平台不會覆蓋")))
}
pub fn read(dir: &Path) -> Result<Value> {
    let mut out = json!({});
    for (key, file) in LISTS {
        out[key] = json!(read_list(dir, file)?);
    }
    Ok(out)
}
pub fn valid_name(s: &str) -> bool {
    (1..=16).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}
pub fn valid_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn ban_time() -> Result<String> {
    #[cfg(windows)]
    {
        let mut t = unsafe { std::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetSystemTime(&mut t) };
        Ok(format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} +0000",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        ))
    }
    #[cfg(not(windows))]
    {
        crate::platform::run_with_timeout(
            crate::platform::command("date").args(["-u", "+%Y-%m-%d %H:%M:%S +0000"]),
            std::time::Duration::from_secs(5),
        )
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .ok_or_else(|| UserError::new("讀不到目前時間，尚未寫入封鎖名單"))
    }
}
pub fn command(body: &Value) -> Result<String> {
    let name = body["name"]
        .as_str()
        .filter(|s| valid_name(s))
        .ok_or_else(|| {
            UserError::new("玩家名稱限 1–16 個英文字母、數字或底線，不接受 @a 等選取器")
        })?;
    if body["confirm"] != true {
        return Err(UserError::new("請先確認玩家與權限變更"));
    }
    let prefix = match body["action"].as_str() {
        Some("op") => "op",
        Some("deop") => "deop",
        Some("whitelist-add") => "whitelist add",
        Some("whitelist-remove") => "whitelist remove",
        Some("ban") => "ban",
        Some("pardon") => "pardon",
        Some("kick") => "kick",
        _ => return Err(UserError::new("沒有這個玩家操作")),
    };
    Ok(format!("{prefix} {name}"))
}
pub fn change(dir: &Path, body: &Value) -> Result<Value> {
    command(body)?;
    let action = body["action"].as_str().unwrap();
    let name = body["name"].as_str().unwrap();
    let (file, add) = match action {
        "op" => ("ops.json", true),
        "deop" => ("ops.json", false),
        "whitelist-add" => ("whitelist.json", true),
        "whitelist-remove" => ("whitelist.json", false),
        "ban" => ("banned-players.json", true),
        "pardon" => ("banned-players.json", false),
        _ => return Err(UserError::new("踢人只能在伺服器執行中使用")),
    };
    let mut list = read_list(dir, file)?;
    let all = read(dir)?;
    let known = LISTS
        .iter()
        .flat_map(|(key, _)| all[*key].as_array().into_iter().flatten())
        .find(|v| {
            v["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        });
    let supplied = body["uuid"].as_str().unwrap_or("");
    let uuid = known.and_then(|v| v["uuid"].as_str()).unwrap_or(supplied);
    if !valid_uuid(uuid) {
        return Err(UserError::new(
            "關機時找不到這位玩家的 UUID，請填入正確 UUID，或開服後用玩家名稱操作",
        ));
    }
    if !supplied.is_empty() && (!valid_uuid(supplied) || !uuid.eq_ignore_ascii_case(supplied)) {
        return Err(UserError::new("UUID 與既有玩家紀錄不同，請重新核對"));
    }
    let pos = list.iter().position(|v| {
        v["uuid"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case(uuid))
    });
    if add {
        if pos.is_none() {
            let mut row = json!({"uuid":uuid.to_lowercase(),"name":name});
            if action == "op" {
                row["level"] = json!(4);
                row["bypassesPlayerLimit"] = json!(false);
            }
            if action == "ban" {
                row["created"] = json!(ban_time()?);
                row["source"] = json!("MC Server Hub");
                row["expires"] = json!("forever");
                row["reason"] = json!("由平台封鎖");
            }
            list.push(row);
        }
    } else if let Some(i) = pos {
        list.remove(i);
    } else {
        return Err(UserError::new("這位玩家不在這份名單裡，沒有修改"));
    }
    let path = fs_safe::child(dir, Path::new(file))?;
    let bak = fs_safe::child(dir, Path::new(&format!("{file}.bak")))?;
    fs_safe::child(dir, Path::new(&format!("{file}.tmp")))?;
    fs_safe::child(dir, Path::new(&format!("{file}.bak.tmp")))?;
    if let Ok(bytes) = fs::read(&path) {
        fs_safe::write_atomic(&bak, &bytes)
            .map_err(|e| UserError::new(format!("備份 {file} 失敗，沒有修改：{e}")))?;
    }
    fs_safe::write_atomic(
        &path,
        serde_json::to_string_pretty(&list).unwrap().as_bytes(),
    )
    .map_err(|e| UserError::new(format!("寫入 {file} 失敗，原檔備份仍在：{e}")))?;
    Ok(json!({"ok":true,"mode":"offline","text":"已備份並更新名單，下次開機生效"}))
}
