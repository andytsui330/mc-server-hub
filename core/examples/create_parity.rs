//! 對等驗收專用：原封編譯正式 create/hub，只把網路換成兩版共用的離線回應。
//! 不放寬正式下載網域；正式程式不引用本入口。
#![cfg(not(test))]
pub use mc_hub_core::{automation,backup, commands, fs_safe, jar, java, java_install, mods, platform, server, Result, UserError};
#[allow(dead_code)]
#[path = "../src/settings.rs"]
mod settings;
#[allow(dead_code)]
#[path = "../src/create.rs"]
mod create;
#[allow(dead_code)]
#[path = "../src/hub.rs"]
mod hub;
use serde_json::{json, Value};
use std::{fs, io::{self, BufRead, Write}, path::{Path, PathBuf}, sync::{Mutex, OnceLock}};
static NETWORK: OnceLock<Mutex<Value>> = OnceLock::new();

mod net {
    pub use mc_hub_core::net::*;
    use super::*;
    fn response(url: &str) -> Result<Value> {
        check_url(url)?;
        NETWORK.get().unwrap().lock().unwrap().get(url).cloned()
            .ok_or_else(|| UserError::new(format!("測試未提供網址：{url}")))
    }
    pub fn fetch_json(url: &str, _sha1: Option<&str>) -> Result<Value> {
        let r = response(url)?;
        r.get("json").cloned().ok_or_else(|| UserError::new("測試模擬斷網"))
    }
    pub fn fetch_checksum(url: &str, algo: &str) -> Result<String> {
        parse_checksum(response(url)?["text"].as_str().unwrap_or("").as_bytes(), algo)
    }
    pub fn download(url: &str, dest: &Path, want: Option<&Checksum>, size: Option<u64>, progress: &mut dyn FnMut(f64)) -> Result<()> {
        let r = response(url)?;
        let file = PathBuf::from(r["file"].as_str().ok_or_else(|| UserError::new("測試模擬斷網"))?);
        let bytes = fs::read(&file).map_err(|e| UserError::new(e.to_string()))?;
        save_stream(bytes.as_slice(), dest, size.unwrap_or(bytes.len() as u64), want, progress)
    }
}

fn main() {
    let root = PathBuf::from(std::env::var_os("MCHUB_DATA_DIR").expect("MCHUB_DATA_DIR required")).canonicalize().unwrap();
    assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert!(root.join("create-parity-marker").is_file());
    NETWORK.set(Mutex::new(Value::Null)).unwrap();
    let app = hub::Hub::new(&root.join("rust/config")).unwrap();
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let out = match v["op"].as_str().unwrap() {
            "network" => { *NETWORK.get().unwrap().lock().unwrap() = v["responses"].clone(); json!({"ok":true}) },
            "call" => app.call(v["path"].as_str().unwrap(), &v["body"]),
            "poll" => { app.poll(); json!({"ok":true}) },
            "quit" => { assert!(!app.busy()); break },
            _ => panic!("unknown op"),
        };
        println!("{out}"); io::stdout().flush().unwrap();
    }
}
