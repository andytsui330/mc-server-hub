//! server.properties：只改指定值，保留原始註解、其他行與未知欄位。
use crate::{Result, UserError};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const FILE: &str = "server.properties";
const LIMIT: u64 = 1024 * 1024;
static SAVE_LOCK: Mutex<()> = Mutex::new(());
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

struct Field {
    key: &'static str,
    label: &'static str,
    description: &'static str,
    kind: &'static str,
    options: &'static [&'static str],
    min: i64,
    max: i64,
}
const FIELDS: &[Field] = &[
    Field {
        key: "motd",
        label: "伺服器介紹",
        description: "多人遊戲清單顯示的介紹文字。",
        kind: "text",
        options: &[],
        min: 0,
        max: 0,
    },
    Field {
        key: "difficulty",
        label: "遊戲難度",
        description: "和平、簡單、普通或困難。",
        kind: "enum",
        options: &["peaceful", "easy", "normal", "hard"],
        min: 0,
        max: 0,
    },
    Field {
        key: "gamemode",
        label: "預設遊戲模式",
        description: "新玩家加入時的遊戲模式。",
        kind: "enum",
        options: &["survival", "creative", "adventure", "spectator"],
        min: 0,
        max: 0,
    },
    Field {
        key: "max-players",
        label: "玩家人數上限",
        description: "同時連線的玩家人數上限。",
        kind: "integer",
        options: &[],
        min: 1,
        max: 2147483647,
    },
    Field {
        key: "pvp",
        label: "玩家互相攻擊",
        description: "是否允許玩家互相造成傷害。",
        kind: "boolean",
        options: &[],
        min: 0,
        max: 0,
    },
    Field {
        key: "white-list",
        label: "啟用白名單",
        description: "開啟後只允許白名單中的玩家加入。",
        kind: "boolean",
        options: &[],
        min: 0,
        max: 0,
    },
    Field {
        key: "online-mode",
        label: "驗證正版帳號",
        description: "建議開啟；關閉會失去官方帳號身分驗證。",
        kind: "boolean",
        options: &[],
        min: 0,
        max: 0,
    },
    Field {
        key: "server-port",
        label: "連線通訊埠",
        description: "玩家連線使用的通訊埠；通常是 25565。",
        kind: "integer",
        options: &[],
        min: 1,
        max: 65535,
    },
    Field {
        key: "view-distance",
        label: "視野距離",
        description: "伺服器傳給玩家的區塊距離，越高越吃資源。",
        kind: "integer",
        options: &[],
        min: 3,
        max: 32,
    },
    Field {
        key: "simulation-distance",
        label: "模擬距離",
        description: "生物與作物等持續運作的區塊距離；舊版可能不支援。",
        kind: "integer",
        options: &[],
        min: 3,
        max: 32,
    },
    Field {
        key: "spawn-protection",
        label: "出生點保護範圍",
        description: "出生點周圍禁止一般玩家修改的範圍；0 為關閉。",
        kind: "integer",
        options: &[],
        min: 0,
        max: 2147483647,
    },
    Field {
        key: "allow-flight",
        label: "允許飛行",
        description: "避免模組飛行玩家被伺服器踢出，不會自動給予飛行能力。",
        kind: "boolean",
        options: &[],
        min: 0,
        max: 0,
    },
    Field {
        key: "enable-command-block",
        label: "允許指令方塊",
        description: "允許指令方塊執行指令；部分新版可能不再支援此設定。",
        kind: "boolean",
        options: &[],
        min: 0,
        max: 0,
    },
    Field {
        key: "force-gamemode",
        label: "強制預設模式",
        description: "每次加入時套用預設遊戲模式。",
        kind: "boolean",
        options: &[],
        min: 0,
        max: 0,
    },
    Field {
        key: "level-seed",
        label: "世界種子碼",
        description: "只影響新世界的生成；修改不會重建現有世界。",
        kind: "text",
        options: &[],
        min: 0,
        max: 0,
    },
];

struct Entry {
    start: usize,
    end: usize,
    key: String,
    prefix: String,
    ending: String,
}
struct Document {
    text: String,
    entries: Vec<Entry>,
    values: Map<String, Value>,
    revision: String,
    newline: String,
}

fn error(msg: impl Into<String>) -> UserError {
    UserError::new(msg)
}
fn malformed() -> UserError {
    error("server.properties 格式損壞或編碼不是 UTF-8，請先人工檢查原檔；未覆寫。")
}
fn conflict() -> UserError {
    error("server.properties 已被其他程式修改，請重新讀取後再儲存；未覆寫。")
}

fn no_links(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
    {
        return Err(error("設定檔路徑必須是完整路徑，且不能包含上層跳轉。"));
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(m) => {
                #[cfg(windows)]
                let linked = {
                    use std::os::windows::fs::MetadataExt;
                    m.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let linked = m.file_type().is_symlink();
                if linked {
                    return Err(error(
                        "設定檔路徑包含符號連結或連接點，為避免改到其他資料夾，已停止。",
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(error(format!("無法檢查設定檔路徑：{e}"))),
        }
    }
    Ok(())
}

fn load_bytes(dir: &Path) -> Result<Vec<u8>> {
    let path = dir.join(FILE);
    no_links(&path)?;
    let f = fs::File::open(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            error("找不到 server.properties，請先啟動伺服器完成第一次產生設定檔，再重新讀取。")
        } else {
            error(format!("無法讀取 server.properties：{e}"))
        }
    })?;
    if !f.metadata().map_err(|e| error(e.to_string()))?.is_file() {
        return Err(error("server.properties 不是一般檔案，已停止。"));
    }
    let mut bytes = Vec::new();
    f.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| error(format!("無法讀取 server.properties：{e}")))?;
    if bytes.len() as u64 > LIMIT {
        return Err(error("server.properties 超過 1 MB，請先人工檢查原檔。"));
    }
    Ok(bytes)
}

fn load(dir: &Path) -> Result<(Vec<u8>, Document)> {
    let bytes = load_bytes(dir)?;
    let doc = parse(&bytes)?;
    Ok((bytes, doc))
}

/// 既有伺服器仍可依系統編碼讀設定；編輯器維持嚴格 UTF-8，不猜測後覆寫。
pub fn effective_value(dir: &Path, key: &str) -> Option<String> {
    let bytes = load_bytes(dir).ok()?;
    let decoded = if std::str::from_utf8(&bytes).is_err() {
        crate::platform::decode_text(&bytes).into_bytes()
    } else {
        bytes
    };
    parse(&decoded)
        .ok()?
        .values
        .get(key)?
        .as_str()
        .map(String::from)
}

/// 缺少欄位不填入預設值；values 的其他鍵供進階區編輯。
pub fn read(dir: &Path) -> Result<Value> {
    let (_, doc) = load(dir)?;
    Ok(view(&doc))
}

fn view(doc: &Document) -> Value {
    let fields:Vec<Value>=FIELDS.iter().map(|f|{
        let mut v=json!({"key":f.key,"label":f.label,"description":f.description,"type":f.kind,"present":doc.values.contains_key(f.key)});
        if f.kind=="enum" {v["options"]=json!(f.options);}
        if f.kind=="integer" {v["min"]=json!(f.min);v["max"]=json!(f.max);}
        v
    }).collect();
    json!({"fields":fields,"values":doc.values,"revision":doc.revision})
}

/// changes = {expected_revision: read.revision, values: {key: string}}。
pub fn save(dir: &Path, changes: &Value) -> Result<Value> {
    let _guard = SAVE_LOCK
        .lock()
        .map_err(|_| error("設定檔正在處理，請稍後重試。"))?;
    let expected = changes
        .get("expected_revision")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| error("請先重新讀取 server.properties，再儲存設定。"))?;
    let values = changes
        .get("values")
        .and_then(Value::as_object)
        .ok_or_else(|| error("設定修改內容必須是欄位和值的物件。"))?;
    for (key, value) in values {
        validate(key, value)?;
    }
    let (bytes, doc) = load(dir)?;
    if doc.revision != expected {
        return Err(conflict());
    }
    let next = render(&doc, values);
    if next.as_bytes() == bytes {
        return Ok(view(&doc));
    }
    let new_doc = parse(next.as_bytes())?;
    let path = dir.join(FILE);
    let backup = dir.join("server.properties.bak");
    no_links(&backup)?;
    if backup.exists() && !backup.is_file() {
        return Err(error("server.properties.bak 不是一般檔案，無法安全備份。"));
    }
    // 先完整備份，再替換正本；兩次重讀避免寫入準備期間的新修改被蓋掉。
    if load(dir)?.0 != bytes {
        return Err(conflict());
    }
    atomic(&backup, &bytes)?;
    let temp = write_temp(&path, next.as_bytes())?;
    let result = (|| {
        if load(dir)?.0 != bytes {
            return Err(conflict());
        }
        no_links(&path)?;
        no_links(&temp)?;
        fs::rename(&temp, &path)
            .map_err(|e| error(format!("無法更新 server.properties；原檔與備份仍保留：{e}")))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    Ok(view(&new_doc))
}

fn validate(key: &str, value: &Value) -> Result<()> {
    if key.is_empty() || key.len() > 256 || key.chars().any(char::is_control) {
        return Err(error("設定欄位名稱不可空白、過長或包含換行及控制字元。"));
    }
    let value = value
        .as_str()
        .ok_or_else(|| error(format!("設定「{key}」的值必須是文字。")))?;
    if value.len() > 32768 || value.chars().any(|c| c.is_control()) {
        return Err(error(format!(
            "設定「{key}」不可過長或包含換行及控制字元。"
        )));
    }
    if let Some(f) = FIELDS.iter().find(|f| f.key == key) {
        let valid = match f.kind {
            "boolean" => matches!(value, "true" | "false"),
            "enum" => f.options.contains(&value),
            "integer" => {
                !value.is_empty()
                    && value.bytes().all(|c| c.is_ascii_digit())
                    && value
                        .parse::<i64>()
                        .is_ok_and(|n| (f.min..=f.max).contains(&n))
            }
            _ => true,
        };
        if !valid {
            return Err(error(format!(
                "設定「{}」的值不正確，請檢查選項或允許範圍。",
                f.label
            )));
        }
    }
    Ok(())
}

fn write_temp(path: &Path, data: &[u8]) -> Result<PathBuf> {
    no_links(path)?;
    let name = format!(
        "{}.{}-{}-{}.tmp",
        path.file_name().unwrap().to_string_lossy(),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let tmp = path.with_file_name(name);
    no_links(&tmp)?;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| error(format!("無法建立設定檔暫存；未改正本：{e}")))?;
    if let Err(e) = f.write_all(data).and_then(|_| f.sync_all()) {
        drop(f);
        let _ = fs::remove_file(&tmp);
        return Err(error(format!("無法完整寫入設定檔；未改正本：{e}")));
    }
    Ok(tmp)
}

fn atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = write_temp(path, data)?;
    let result = no_links(path).and_then(|_| {
        fs::rename(&tmp, path)
            .map_err(|e| error(format!("無法備份 server.properties；未改正本：{e}")))
    });
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | 0x0c)
}

fn parse(bytes: &[u8]) -> Result<Document> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| malformed())?
        .to_owned();
    if text.contains('\0') {
        return Err(malformed());
    }
    let mut lines = Vec::new();
    let mut start = if bytes.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    let mut i = start;
    while i < bytes.len() {
        if matches!(bytes[i], b'\r' | b'\n') {
            let end = i;
            i += 1;
            if bytes[end] == b'\r' && bytes.get(i) == Some(&b'\n') {
                i += 1;
            }
            lines.push((start, end, i));
            start = i;
        } else {
            i += 1;
        }
    }
    if start < bytes.len() {
        lines.push((start, bytes.len(), bytes.len()));
    }
    let newline = lines
        .iter()
        .find_map(|(_, e, n)| (*e < *n).then(|| text[*e..*n].to_owned()))
        .unwrap_or_else(|| "\n".into());
    let mut entries = Vec::new();
    let mut values = Map::new();
    let mut row = 0;
    while row < lines.len() {
        let (begin, end, _) = lines[row];
        let first = &text[begin..end];
        let trimmed = first.trim_start_matches([' ', '\t', '\x0c']);
        if trimmed.is_empty() || trimmed.starts_with(['#', '!']) {
            row += 1;
            continue;
        }
        let mut logical = first.to_owned();
        let first_len = first.len();
        while logical.bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 1 {
            logical.pop();
            row += 1;
            let Some(&(b, e, _)) = lines.get(row) else {
                return Err(malformed());
            };
            logical.push_str(text[b..e].trim_start_matches([' ', '\t', '\x0c']));
        }
        let raw = logical.as_bytes();
        let mut key_start = 0;
        while key_start < raw.len() && whitespace(raw[key_start]) {
            key_start += 1;
        }
        let mut k = key_start;
        while k < raw.len() {
            if raw[k] == b'\\' {
                k += 2;
                continue;
            }
            if whitespace(raw[k]) || matches!(raw[k], b'=' | b':') {
                break;
            }
            k += 1;
        }
        let key_end = k.min(raw.len());
        let mut value_start = key_end;
        while value_start < raw.len() && whitespace(raw[value_start]) {
            value_start += 1;
        }
        if value_start < raw.len() && matches!(raw[value_start], b'=' | b':') {
            value_start += 1;
        }
        while value_start < raw.len() && whitespace(raw[value_start]) {
            value_start += 1;
        }
        let key = unescape(&logical[key_start..key_end])?;
        let value = unescape(&logical[value_start..])?;
        if key.is_empty() {
            return Err(malformed());
        }
        let prefix = if value_start <= first_len
            && first.get(..value_start) == logical.get(..value_start)
            && key_end < raw.len()
        {
            first[..value_start].to_owned()
        } else {
            format!("{}=", escape(&key, true))
        };
        let (_, last_content, last_end) = lines[row];
        entries.push(Entry {
            start: begin,
            end: last_end,
            key: key.clone(),
            prefix,
            ending: text[last_content..last_end].to_owned(),
        });
        values.insert(key, Value::String(value));
        row += 1;
    }
    Ok(Document {
        text,
        entries,
        values,
        revision: format!("{:x}", Sha256::digest(bytes)),
        newline,
    })
}

fn unescape(s: &str) -> Result<String> {
    let mut chars = s.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let c = chars.next().ok_or_else(malformed)?;
        match c {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'f' => out.push('\x0c'),
            'u' => {
                let mut unit = || -> Result<u16> {
                    let mut n = 0;
                    for _ in 0..4 {
                        n = n * 16
                            + chars
                                .next()
                                .and_then(|c| c.to_digit(16))
                                .ok_or_else(malformed)? as u16;
                    }
                    Ok(n)
                };
                let high = unit()?;
                let scalar = if (0xd800..=0xdbff).contains(&high) {
                    if chars.next() != Some('\\') || chars.next() != Some('u') {
                        return Err(malformed());
                    }
                    let mut low = 0u16;
                    for _ in 0..4 {
                        low = low * 16
                            + chars
                                .next()
                                .and_then(|c| c.to_digit(16))
                                .ok_or_else(malformed)? as u16;
                    }
                    if !(0xdc00..=0xdfff).contains(&low) {
                        return Err(malformed());
                    }
                    0x10000 + ((high as u32 - 0xd800) << 10) + (low as u32 - 0xdc00)
                } else {
                    high as u32
                };
                out.push(char::from_u32(scalar).ok_or_else(malformed)?);
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

fn escape(s: &str, key: bool) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c == '\\' || matches!(c, '=' | ':' | '#' | '!') || (c == ' ' && (key || i == 0)) {
            out.push('\\');
            out.push(c);
        } else if !c.is_ascii() {
            for unit in c.encode_utf16(&mut [0; 2]) {
                out.push_str(&format!("\\u{unit:04X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn render(doc: &Document, patch: &Map<String, Value>) -> String {
    let last: HashMap<&str, usize> = doc
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.key.as_str(), i))
        .collect();
    let mut out = String::new();
    let mut cursor = 0;
    for (i, entry) in doc.entries.iter().enumerate() {
        out.push_str(&doc.text[cursor..entry.start]);
        if let Some(value) = patch
            .get(&entry.key)
            .filter(|v| Some(*v) != doc.values.get(&entry.key))
            .filter(|_| last.get(entry.key.as_str()) == Some(&i))
        {
            out.push_str(&entry.prefix);
            out.push_str(&escape(value.as_str().unwrap(), false));
            out.push_str(&entry.ending);
        } else {
            out.push_str(&doc.text[entry.start..entry.end]);
        }
        cursor = entry.end;
    }
    out.push_str(&doc.text[cursor..]);
    for (key, value) in patch
        .iter()
        .filter(|(key, _)| !doc.values.contains_key(*key))
    {
        if !out.is_empty() && !out.ends_with(['\r', '\n', '\u{feff}']) {
            out.push_str(&doc.newline);
        }
        out.push_str(&escape(key, true));
        out.push('=');
        out.push_str(&escape(value.as_str().unwrap(), false));
        out.push_str(&doc.newline);
    }
    out
}
