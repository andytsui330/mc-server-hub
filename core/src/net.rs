//! 下載：只連列名的官方網域、只走 HTTPS、轉址每一跳都重新檢查網域、核對檢查碼。
use crate::{Result, UserError};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;
use ureq::http::Uri;

pub const ALLOWED_HOSTS: [&str; 8] = [
    "piston-meta.mojang.com", "piston-data.mojang.com", "launchermeta.mojang.com", "launcher.mojang.com",
    "meta.fabricmc.net", "files.minecraftforge.net", "maven.minecraftforge.net", "maven.neoforged.net",
];
/// 新版提醒只讀這個固定網址上的版本號，不下載、不換檔。
pub const VERSION_URL: &str = "https://mc-hub-version.pages.dev/version.json";
const USER_AGENT: &str = concat!("MCServerHub/", env!("CARGO_PKG_VERSION"));

#[derive(Clone,Copy)]
pub enum Source { Existing,Modrinth,Temurin }

pub fn check_url_from(url:&str,source:&Source)->Result<()> {
    if matches!(source,Source::Existing){return check_url(url);}
    let ok=url.parse::<Uri>().is_ok_and(|u|{
        if u.scheme_str()!=Some("https")||u.port().is_some()||u.authority().is_none_or(|a|Some(a.as_str())!=u.host()){return false;}
        let host=u.host().unwrap_or("");let path=u.path();
        match source{
            Source::Modrinth=>(host=="api.modrinth.com"&&path.starts_with("/v2/"))||(host=="cdn.modrinth.com"&&path.starts_with("/data/")),
            Source::Temurin=>{
                if host=="api.adoptium.net"{return path.starts_with("/v3/");}
                if host=="release-assets.githubusercontent.com"{return true;}
                let lower=path.to_ascii_lowercase();
                if host!="github.com"||["%2f","%5c","%2e","\\"].iter().any(|s|lower.contains(s)) {return false;}
                let parts:Vec<_>=path.split('/').collect();
                parts.len()>=7&&parts[1]=="adoptium"&&parts[2].strip_prefix("temurin").and_then(|s|s.strip_suffix("-binaries")).is_some_and(|n|!n.is_empty()&&n.bytes().all(|b|b.is_ascii_digit()))&&parts[3]=="releases"&&parts[4]=="download"&&!parts.iter().any(|p|*p==".."||*p==".")
            },Source::Existing=>false
        }
    });
    if ok{Ok(())}else{Err(UserError::new("下載網址不在這項功能的官方來源白名單，已停止。"))}
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0) // 轉址自己處理，才能每一跳都檢查網域
        .http_status_as_error(false)
        .user_agent(USER_AGENT)
        .timeout_connect(Some(timeout))
        .timeout_recv_response(Some(timeout))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .timeout_global(Some(Duration::from_secs(900)))
        .build()
        .into()
}

pub fn check_url(url: &str) -> Result<()> {
    let ok = url.parse::<Uri>().is_ok_and(|u| {
        u.scheme_str() == Some("https") && u.host().is_some_and(|h| ALLOWED_HOSTS.contains(&h)) && u.port().is_none()
    });
    if ok { Ok(()) } else { Err(UserError::new("下載網址不是官方來源，已停止。")) }
}

/// 轉址目的地：完整網址或同網站的 /路徑；其他寫法一律不接受。
fn redirect_target(from: &str, location: &str) -> Option<String> {
    if location.starts_with("https://") || location.starts_with("http://") {
        return Some(location.to_string());
    }
    let u = from.parse::<Uri>().ok()?;
    if !location.starts_with('/') || location.starts_with("//") {
        return None;
    }
    Some(format!("{}://{}{location}", u.scheme_str()?, u.authority()?))
}

fn open(url: &str) -> Result<ureq::Body> {
    open_from(url,&Source::Existing)
}

fn open_from(url:&str,source:&Source)->Result<ureq::Body>{
    let agent = agent(Duration::from_secs(30));
    let mut url = url.to_string();
    for _ in 0..5 {
        check_url_from(&url,source)?;
        let resp = agent.get(&url).call().map_err(|_| UserError::new("連不上官方伺服器，請檢查網路後再試。"))?;
        let status = resp.status();
        if status.is_redirection() {
            let loc = resp.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("");
            url = redirect_target(&url, loc).ok_or_else(|| UserError::new("官方伺服器轉到看不懂的網址，已停止。"))?;
            continue;
        }
        if !status.is_success() {
            return Err(UserError::new(format!("官方伺服器回應錯誤（{}），請稍後再試。", status.as_u16())));
        }
        return Ok(resp.into_body());
    }
    Err(UserError::new("官方網址轉址太多次，已停止。"))
}

pub fn fetch_json_from(url:&str,source:&Source)->Result<serde_json::Value>{
    let data=open_from(url,source)?.with_config().limit(16<<20).read_to_vec().map_err(|_|UserError::new("官方資料下載不完整，請稍後再試"))?;
    serde_json::from_slice(&data).map_err(|_|UserError::new("官方資料格式不正確，請稍後再試"))
}

pub fn download_from(url:&str,dest:&Path,want:Option<&Checksum>,size:Option<u64>,progress:&mut dyn FnMut(f64),source:&Source)->Result<()>{
    let body=open_from(url,source)?;let total=size.or_else(||body.content_length()).unwrap_or(0);save_stream(body.into_reader(),dest,total,want,progress)
}

pub fn fetch_json(url: &str, sha1: Option<&str>) -> Result<serde_json::Value> {
    let data = open(url)?
        .with_config()
        .limit(50 << 20)
        .read_to_vec()
        .map_err(|_| UserError::new("官方版本資料下載不完整，請稍後再試。"))?;
    if sha1.is_some_and(|want| hex(&Sha1::digest(&data)) != want.to_ascii_lowercase()) {
        return Err(UserError::new("官方版本資料核對不符，已停止。請稍後再試。"));
    }
    serde_json::from_slice(&data).map_err(|_| UserError::new("官方版本資料格式不對，請稍後再試。"))
}

#[derive(Clone, Debug)]
pub enum Checksum {
    Sha1(String),
    Sha256(String),
}

/// 檢查碼只能防「下載壞掉」，防不了來源本身被換。
pub fn download(url: &str, dest: &Path, want: Option<&Checksum>, size: Option<u64>, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let body = open(url)?;
    let total = size.or_else(|| body.content_length()).unwrap_or(0);
    save_stream(body.into_reader(), dest, total, want, progress)
}

/// 邊收邊寫到 .part、邊算檢查碼；長度或檢查碼不對就刪掉 .part，對了才改成正式檔名。
pub fn save_stream(mut r: impl Read, dest: &Path, total: u64, want: Option<&Checksum>, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let mut part = dest.as_os_str().to_os_string();
    part.push(".part");
    let part = Path::new(&part);
    let (mut s1, mut s256, mut got) = (Sha1::new(), Sha256::new(), 0u64);
    let written = (|| -> std::io::Result<()> {
        let mut f = File::create(part)?;
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let n = r.read(&mut buf)?;
            if n == 0 {
                return f.sync_all();
            }
            if total>0&&got.saturating_add(n as u64)>total{return Err(std::io::Error::other("下載資料超過官方宣告大小"));}
            f.write_all(&buf[..n])?;
            match want {
                Some(Checksum::Sha256(_)) => s256.update(&buf[..n]),
                _ => s1.update(&buf[..n]),
            }
            got += n as u64;
            if total > 0 {
                progress(got as f64 / total as f64);
            }
        }
    })();
    let fail = |msg: String| {
        let _ = fs::remove_file(part);
        Err(UserError::new(msg))
    };
    if written.is_err() || (total > 0 && got != total) {
        return fail("下載不完整（網路可能中斷了），請再試一次。".into());
    }
    let (name, expect, actual) = match want {
        Some(Checksum::Sha1(w)) => ("SHA-1", w, hex(&s1.finalize())),
        Some(Checksum::Sha256(w)) => ("SHA-256", w, hex(&s256.finalize())),
        None => return fs::rename(part, dest).map_err(|e| UserError::new(format!("存不了下載的檔案：{e}"))),
    };
    if actual != expect.to_ascii_lowercase() {
        return fail(format!("下載的檔案核對不符（{name} 不一致），已刪除。請再試一次。"));
    }
    fs::rename(part, dest).map_err(|e| UserError::new(format!("存不了下載的檔案：{e}")))
}

/// 讀官方的 .sha1／.sha256 小檔（最多 200 bytes）。
pub fn fetch_checksum(url: &str, algo: &str) -> Result<String> {
    let mut data = Vec::new();
    open(url)?.into_reader().take(201).read_to_end(&mut data)
        .map_err(|_| UserError::new("連不上官方伺服器，請檢查網路後再試。"))?;
    parse_checksum(&data, algo)
}

/// 只收對的長度的十六進位，取第一個字（官方檔有時後面接檔名）。
pub fn parse_checksum(data: &[u8], algo: &str) -> Result<String> {
    let len = if algo == "sha1" { 40 } else { 64 };
    let text = String::from_utf8_lossy(data);
    match text.split_whitespace().next() {
        Some(w) if data.len() <= 200 && w.len() == len && w.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(w.to_ascii_lowercase()),
        _ => Err(UserError::new("官方的檢查碼檔案格式不對，已停止。請稍後再試。")),
    }
}

/// 有比 current 新的版本就回傳版本號；內容不是「數字.數字.數字」一律當沒有。
pub fn newer_version(body: &[u8], current: &str) -> Option<String> {
    let ws = |b: &u8| b" \t\n\r\x0b\x0c".contains(b);
    let start = body.iter().position(|b| !ws(b)).unwrap_or(body.len());
    let end = body.iter().rposition(|b| !ws(b)).map_or(start, |i| i + 1);
    let parse = |s: &[u8]| -> Option<Vec<u32>> {
        let parts: Vec<&[u8]> = s.split(|&b| b == b'.').collect();
        (parts.len() == 3 && parts.iter().all(|p| (1..=4).contains(&p.len()) && p.iter().all(u8::is_ascii_digit)))
            .then(|| parts.iter().map(|p| std::str::from_utf8(p).unwrap().parse().unwrap()).collect())
    };
    let latest = parse(&body[start..end])?;
    let now: Vec<u32> = current.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    (latest > now).then(|| latest.iter().map(u32::to_string).collect::<Vec<_>>().join("."))
}

/// 問固定網址有沒有新版；連不上、內容不對一律回 None，不影響開服。只讀 32 bytes、不跟隨轉址。
pub fn check_update() -> Option<String> {
    let resp = agent(Duration::from_secs(8)).get(VERSION_URL).call().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let mut data = Vec::new();
    resp.into_body().into_reader().take(32).read_to_end(&mut data).ok()?;
    newer_version(&data, env!("CARGO_PKG_VERSION"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_official_https() {
        assert!(check_url("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json").is_ok());
        for bad in ["http://piston-meta.mojang.com/x", "https://evil.com/x", "https://piston-meta.mojang.com.evil.com/x",
                    "https://piston-meta.mojang.com:8443/x", "ftp://maven.neoforged.net/x", "not a url"] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn redirects() {
        let from = "https://maven.neoforged.net/a/b";
        assert_eq!(redirect_target(from, "/c").as_deref(), Some("https://maven.neoforged.net/c"));
        assert_eq!(redirect_target(from, "https://evil.com/x").as_deref(), Some("https://evil.com/x")); // 交給下一輪 check_url 擋
        assert_eq!(redirect_target(from, "//evil.com/x"), None);
        assert_eq!(redirect_target(from, "c"), None);
    }

    #[test]
    fn checksum_file() {
        let h = "A".repeat(40);
        assert_eq!(parse_checksum(format!("{h}  forge.jar\n").as_bytes(), "sha1").unwrap(), "a".repeat(40));
        assert!(parse_checksum(h.as_bytes(), "sha256").is_err());
        assert!(parse_checksum(b"<html>", "sha1").is_err());
        assert!(parse_checksum(&[b'a'; 201], "sha1").is_err());
    }

    #[test]
    fn stream_checks_size_and_hash() {
        let d = std::env::temp_dir().join(format!("mchub-net-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let dest = d.join("server.jar");
        let sha1_abc = "a9993e364706816aba3e25717850c26c9cd0d89d";
        let mut seen = 0.0;
        save_stream(&b"abc"[..], &dest, 3, Some(&Checksum::Sha1(sha1_abc.to_uppercase())), &mut |p| seen = p).unwrap();
        assert_eq!((fs::read(&dest).unwrap(), seen), (b"abc".to_vec(), 1.0));
        let e = save_stream(&b"abc"[..], &dest, 4, None, &mut |_| {}).unwrap_err();
        assert!(e.msg.contains("不完整"));
        let e = save_stream(&b"abd"[..], &dest, 0, Some(&Checksum::Sha256("0".repeat(64))), &mut |_| {}).unwrap_err();
        assert!(e.msg.contains("SHA-256"));
        assert!(!d.join("server.jar.part").exists(), "失敗要清掉 .part");
        assert_eq!(fs::read(&dest).unwrap(), b"abc", "失敗不能動到舊檔");
        fs::remove_dir_all(d).unwrap();
    }

    /// 真的連一次 Mojang，確認 HTTPS 憑證與白名單能用。要連網，平常不跑：cargo test -- --ignored
    #[test]
    #[ignore]
    fn live_mojang_manifest() {
        let v = fetch_json("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json", None).unwrap();
        assert!(v["versions"].as_array().unwrap().len() > 100);
    }
}
