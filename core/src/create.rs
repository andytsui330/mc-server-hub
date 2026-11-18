//! 建服：原版／Fabric／Forge／NeoForge 的版本挑選、下載與安裝；加入現有資料夾時判斷版本與類型。
//! 對應 1.x 的 forge_pick、neoforge_pick、loader_choice、install_modloader、inspect_server、Hub._create。
//! 平台本體（同時只准建一台、登記到設定檔、第一次開機）在接 Tauri 外殼時做（第 5 步）。
use crate::commands::mc_tuple;
use crate::jar::Jar;
use crate::net::{self, Checksum};
use crate::server::{launch, sys};
use crate::{fs_safe, java, platform, Result, UserError};
use regex::Regex;
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{mpsc, LazyLock, Mutex};
use std::time::{Duration, Instant};

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const FABRIC_META: &str = "https://meta.fabricmc.net/v2/versions";
pub const FORGE_PROMOS: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
pub const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
pub const NEOFORGE_VERSIONS: &str = "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
pub const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
/// Forge／NeoForge 安裝器最多跑 15 分鐘。
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(900);
/// 官方版本清單 10 分鐘內共用。
const CACHE_FOR: Duration = Duration::from_secs(600);

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

/// JSON 值當字串用（1.x 的 str(v)）。
fn text_of(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_string)
}

/// 回傳 Forge 版本，或不能選的原因。
pub fn forge_pick(promos: &Value, mc: &str) -> std::result::Result<String, &'static str> {
    if !mc_tuple(mc).is_some_and(|t| t >= vec![1, 20, 1]) {
        return Err("這版 Forge 啟動方式不同，目前不支援");
    }
    let p = &promos["promos"];
    let truthy = |v: &&Value| !v.is_null() && v.as_str() != Some("");
    let v = p.get(format!("{mc}-recommended")).filter(truthy).or_else(|| p.get(format!("{mc}-latest")).filter(truthy));
    match v.map(text_of) {
        Some(v) if mc_tuple(&v).is_some() => Ok(v), // 版本號會進網址和檔名，只收數字與點
        _ => Err("Forge 還沒有這個遊戲版本"),
    }
}

/// NeoForge 版本號 → 遊戲版本：21.1.x→1.21.1、21.0.x→1.21、26.3.0.x→26.3（第三段 0 省略）。
pub fn neoforge_mc(v: &str) -> Option<String> {
    let p: Vec<&str> = v.split('.').collect();
    let tail = |s: &str| if s == "0" { String::new() } else { format!(".{s}") };
    if p.len() >= 4 && p[0].parse::<u64>().ok()? >= 26 {
        return Some(format!("{}.{}{}", p[0], p[1], tail(p[2])));
    }
    (p.len() == 3).then(|| format!("1.{}{}", p[0], tail(p[1])))
}

/// 只收「純數字與點」的正式版，取最大；只有測試版就說明。
pub fn neoforge_pick(data: &Value, mc: &str) -> std::result::Result<String, &'static str> {
    let all: Vec<String> = data["versions"].as_array().into_iter().flatten().map(text_of).collect();
    let best = all.iter().filter(|v| mc_tuple(v).is_some() && neoforge_mc(v).as_deref() == Some(mc)).max_by_key(|v| mc_tuple(v));
    if let Some(v) = best {
        return Ok(v.clone());
    }
    static PREFIX: LazyLock<Regex> = LazyLock::new(|| re(r"^\d+(?:\.\d+)+"));
    if all.iter().any(|v| PREFIX.find(v).is_some_and(|m| neoforge_mc(m.as_str()).as_deref() == Some(mc))) {
        return Err("NeoForge 這個遊戲版本只有測試版");
    }
    Err("NeoForge 還沒有這個遊戲版本")
}

pub fn forge_installer_url(mc: &str, v: &str) -> String {
    format!("{FORGE_MAVEN}/{mc}-{v}/forge-{mc}-{v}-installer.jar")
}

pub fn neoforge_installer_url(v: &str) -> String {
    format!("{NEOFORGE_MAVEN}/{v}/neoforge-{v}-installer.jar")
}

/// 給建服精靈：某遊戲版本能不能選這個載入器、會裝哪版、不能的原因。data 為 None＝官方網站連不上。
pub fn loader_choice(kind: &str, mc: &str, data: Option<&Value>) -> Value {
    let label = if kind == "forge" { "Forge" } else { "NeoForge" };
    let Some(data) = data else {
        return json!({"ok": false, "version": null, "why": format!("連不上 {label} 官方網站，稍後再試")});
    };
    match if kind == "forge" { forge_pick(data, mc) } else { neoforge_pick(data, mc) } {
        Ok(v) => json!({"ok": true, "version": v, "why": null}),
        Err(why) => json!({"ok": false, "version": null, "why": why}),
    }
}

/// 資料夾名稱裡的數字依序取出，排序用（1.x 的 _num_key）。
fn num_key(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse().unwrap_or(u64::MAX)).collect()
}

/// 子資料夾依名稱裡的數字由大到小。
fn subdirs_desc(d: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort_by_key(|n| std::cmp::Reverse(num_key(n)));
    names
}

/// 從 libraries 資料夾推 Forge／NeoForge 對應的遊戲版本：(遊戲版本, 載入器)。
pub fn modloader_version(d: &Path) -> Option<(String, &'static str)> {
    for name in subdirs_desc(&d.join("libraries/net/neoforged/neoforge")) {
        if mc_tuple(&name).is_some()
            && let Some(mc) = neoforge_mc(&name)
        {
            return Some((mc, "NeoForge"));
        }
    }
    static FORGE: LazyLock<Regex> = LazyLock::new(|| re(r"^(\d+(?:\.\d+)+)-[\d.]+$"));
    subdirs_desc(&d.join("libraries/net/minecraftforge/forge")).iter().find_map(|n| FORGE.captures(n).map(|m| (m[1].to_string(), "Forge")))
}

fn jars_in(d: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("jar")) && p.is_file())
        .collect();
    v.sort();
    v
}

/// 加入現有伺服器時，從 jar 或紀錄檔判斷遊戲版本、類型與需要的 Java；判斷不出來就留空。
pub fn inspect_server(d: &Path) -> Value {
    if let Some((mc, loader)) = modloader_version(d) {
        // Forge／NeoForge：資料夾裡的 jar 不是伺服器本體，看 libraries 就夠
        return json!({"version": mc, "java": java::java_for_mc(&mc), "loader": loader});
    }
    let truthy = |v: &Value| !matches!(v, Value::Null | Value::Bool(false)) && v != &json!(0) && v != &json!("");
    let (mut version, mut java_v) = (Value::Null, Value::Null);
    let mut jars = jars_in(d);
    jars.extend(jars_in(&d.join(".fabric/server")));
    for sub in subdirs_desc(&d.join("versions")) {
        jars.extend(jars_in(&d.join("versions").join(sub)));
    }
    static GAME: LazyLock<regex::bytes::Regex> = LazyLock::new(|| regex::bytes::Regex::new(r"game-version=(\S+)").unwrap());
    for jar in jars {
        let Ok(mut z) = Jar::open(&jar) else { continue };
        let has_version = z.names().any(|x| x == "version.json");
        let has_install = z.names().any(|x| x == "install.properties");
        if has_version {
            let Some(v) = z.read("version.json").ok().flatten().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
            if truthy(&v["id"]) {
                version = v["id"].clone();
            }
            java_v = v.get("java_version").cloned().unwrap_or(Value::Null);
        } else if has_install && version.is_null() {
            let Some(b) = z.read("install.properties").ok().flatten() else { continue };
            version = GAME.captures(&b).map_or(Value::Null, |m| json!(String::from_utf8_lossy(&m[1])));
        }
        if truthy(&java_v) {
            break;
        }
    }
    if version.is_null() {
        static LOG: LazyLock<Regex> = LazyLock::new(|| re(r"Starting minecraft server version (\S+)"));
        if let Ok(text) = launch::read_text_any(&d.join("logs/latest.log")) {
            version = LOG.captures(&text).map_or(Value::Null, |m| json!(m[1].to_string()));
        }
    }
    if !truthy(&java_v) {
        java_v = json!(version.as_str().and_then(java::java_for_mc));
    }
    let names = jars_in(d).iter().map(|p| p.file_name().unwrap().to_string_lossy().to_lowercase()).collect::<Vec<_>>().join(" ");
    let loader = if names.contains("fabric") || d.join(".fabric").is_dir() {
        "Fabric"
    } else if d.join("libraries/net/neoforged").is_dir() {
        "NeoForge"
    } else if d.join("libraries/net/minecraftforge").is_dir() {
        "Forge"
    } else if names.contains("paper") {
        "Paper"
    } else {
        ""
    };
    json!({"version": version, "java": java_v, "loader": loader})
}

/// 安裝失敗時把安裝紀錄複製到 log_dir\installer-logs\，回傳存放位置。
fn keep_installer_log(target: &Path, installer: &str, captured: &[String], log_dir: &Path) -> io::Result<PathBuf> {
    let dest = log_dir.join("installer-logs").join(format!("{}_{installer}.log", platform::local_stamp()));
    fs::create_dir_all(dest.parent().unwrap())?;
    let mut src = target.join(format!("{installer}.log"));
    if !src.is_file() {
        src = target.join("installer.log"); // 2026-11-13 實測：Forge 52.1.20、NeoForge 21.1.266 都寫成 installer.log
    }
    if src.is_file() {
        fs::copy(&src, &dest)?;
    } else {
        fs::write(&dest, captured.join("\n"))?;
    }
    Ok(dest)
}

/// 在伺服器資料夾裡跑 Forge／NeoForge 安裝器，成功後依它產生的 run 檔寫平台自己的啟動檔，回傳檔名。
/// 失敗時把安裝紀錄留在 log_dir\installer-logs\ 並回白話錯誤（清資料夾由呼叫端負責）。
pub fn install_modloader(
    target: &Path, java: &Path, installer: &Path, ram: u32, on_line: &mut dyn FnMut(&str), log_dir: &Path, timeout: Duration,
) -> Result<&'static str> {
    let name = installer.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let (mut reader, writer) = io::pipe().map_err(|e| UserError::new(format!("安裝器跑不起來：{e}")))?;
    let mut cmd = platform::command(java);
    cmd.args(["-Dstdout.encoding=UTF-8", "-Dstderr.encoding=UTF-8", "-jar", &name, "--installServer"]).current_dir(target);
    let err_writer = writer.try_clone().map_err(|e| UserError::new(format!("安裝器跑不起來：{e}")))?;
    cmd.stdout(Stdio::from(writer)).stderr(Stdio::from(err_writer));
    let mut child = cmd.spawn().map_err(|e| UserError::new(format!("安裝器跑不起來：{e}")))?;
    drop(cmd); // 放掉這邊手上的管線寫入端，安裝器結束時讀取端才收得到結尾
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut buf, mut chunk) = (Vec::new(), vec![0u8; 16384]);
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            while let Some(i) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=i).collect();
                let _ = tx.send(platform::decode_text(&line).trim_end_matches(['\r', '\n']).to_string());
            }
        }
        if !buf.is_empty() {
            let _ = tx.send(platform::decode_text(&buf).trim_end_matches(['\r', '\n']).to_string());
        }
    });
    let deadline = Instant::now() + timeout;
    let (mut captured, mut killed, mut code, mut exited) = (Vec::<String>::new(), false, -1, None::<Instant>);
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                on_line(&line);
                captured.push(line);
                if captured.len() > 5000 {
                    captured.drain(..captured.len() - 5000);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) if exited.is_some() => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        // 安裝器結束後最多再等 3 秒收完輸出（萬一它另開的程式還握著管線）
        if exited.is_some_and(|t| t.elapsed() > Duration::from_secs(3)) {
            break;
        }
        if exited.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => (code, exited) = (status.code().unwrap_or(-1), Some(Instant::now())),
                Ok(None) if Instant::now() > deadline && !killed => {
                    killed = true;
                    let _ = child.kill();
                }
                Ok(None) => {}
                Err(_) => exited = Some(Instant::now()),
            }
        }
    }
    let run = target.join(if platform::OS == platform::Os::Win { "run.bat" } else { "run.sh" });
    let args = if code != 0 || killed || !run.is_file() { None } else { launch::read_text_any(&run).ok().and_then(|t| launch::modloader_args(&t)) };
    let Some(args) = args else {
        let why = if killed {
            format!("安裝逾時（超過 {} 分鐘）", timeout.as_secs() / 60)
        } else if code != 0 {
            format!("安裝失敗（結束代碼 {code}）")
        } else {
            "安裝完成了，但看不懂它產生的啟動方式，目前不支援這個版本".to_string()
        };
        let kept = match keep_installer_log(target, &name, &captured, log_dir) {
            Ok(p) => format!("安裝紀錄已保留在：{}", p.display()),
            Err(e) => format!("安裝紀錄存不下來（{e}）"),
        };
        return Err(UserError::new(format!("{why}。{kept}")));
    };
    let _ = fs::remove_file(installer); // 安裝器 jar 留著會被誤認成伺服器本體
    platform::write_start_script(target, &java.display().to_string(), &format!("-Xmx{ram}G {args} nogui"))
}

/// 建服精靈送來、檢查過的需求。
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub name: String,
    pub version: String,
    pub loader: &'static str,
    pub ram: u32,
    pub parent: PathBuf,
    pub target: PathBuf,
    pub java_path: Option<PathBuf>,
}

/// 檢查建服需求（1.x 的 start_create 前半）；不合格回白話原因。
pub fn plan(req: &Value) -> std::result::Result<Plan, String> {
    if req["eula"] != json!(true) {
        return Err("要先勾選同意 Minecraft 授權合約（EULA）才能建立".into()); // 一定要使用者自己勾選，程式不代為同意
    }
    // 非字串一律當沒填（1.x 會把 null 變成名稱「None」）
    let name = req["name"].as_str().unwrap_or("").trim().to_string();
    if let Some(e) = fs_safe::check_folder_name(&name) {
        return Err(e.into());
    }
    let loader = ["vanilla", "fabric", "forge", "neoforge"].into_iter().find(|l| req["loader"] == *l).ok_or("類型只能選原版、Fabric、Forge 或 NeoForge")?;
    let ram = match &req["ram_gb"] {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
    .ok_or("記憶體要選一個數字")?;
    if !(1..=64).contains(&ram) {
        return Err("記憶體要在 1 到 64 GB 之間".into());
    }
    let parent = PathBuf::from(req["parent"].as_str().unwrap_or("").trim().trim_matches('"'));
    if !parent.is_absolute() {
        return Err("存放位置要選一個完整的資料夾路徑".into());
    }
    let target = parent.join(&name);
    if matches!(loader, "forge" | "neoforge") && target.to_string_lossy().contains('!') {
        return Err("Forge／NeoForge 的存放路徑不能有驚嘆號 !，請換一個位置或名稱".into()); // Java 的 @參數檔與 jar 路徑遇到 ! 會出錯
    }
    if target.exists() && (!target.is_dir() || fs::read_dir(&target).map_or(true, |mut d| d.next().is_some())) {
        return Err(format!("「{}」已經有東西了，請換名稱或換位置，平台不會覆蓋既有資料", target.display()));
    }
    let version = req["version"].as_str().unwrap_or("").to_string();
    let java_path = req["java_path"].as_str().filter(|p| !p.is_empty()).map(PathBuf::from);
    if java_path.as_ref().is_some_and(|p| !p.is_absolute() || !p.is_file()) { return Err("Java 路徑不存在".into()); }
    Ok(Plan { name, version, loader, ram: ram as u32, parent, target, java_path })
}

/// 建服過程給畫面看的進度。
#[derive(Debug, Default, Clone)]
pub struct Progress {
    pub msg: String,
    pub pct: Option<u32>,
    /// Forge／NeoForge 安裝器最近 200 行輸出
    pub lines: Vec<String>,
}

/// 建好、還沒登記到平台的伺服器。
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    pub label: String,
    pub loader: &'static str,
    pub java_need: u32,
    pub bat: &'static str,
    pub port: u16,
}

/// 官方版本清單（10 分鐘內共用）。
#[derive(Default)]
pub struct Catalog {
    manifest: Mutex<Option<(Instant, Value)>>,
    loaders: Mutex<Option<(Instant, Value)>>,
}

impl Catalog {
    pub fn manifest(&self) -> Result<Value> {
        let mut m = self.manifest.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, v)) = m.as_ref().filter(|(t, _)| t.elapsed() < CACHE_FOR) {
            return Ok(v.clone());
        }
        let v = net::fetch_json(MANIFEST_URL, None)?;
        *m = Some((Instant::now(), v.clone()));
        Ok(v)
    }

    /// Forge／NeoForge 版本清單；連不上的那個記成 null，不讓整份版本清單失敗，下次再試。
    pub fn loader_lists(&self) -> Value {
        let mut l = self.loaders.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, v)) = l.as_ref().filter(|(t, _)| t.elapsed() < CACHE_FOR) {
            return v.clone();
        }
        let get = |url| net::fetch_json(url, None).unwrap_or(Value::Null);
        let v = json!({"forge": get(FORGE_PROMOS), "neoforge": get(NEOFORGE_VERSIONS)});
        *l = (!v["forge"].is_null() && !v["neoforge"].is_null()).then(|| (Instant::now(), v.clone()));
        v
    }

    /// 建服精靈的版本清單：每個正式版能不能選 Fabric／Forge／NeoForge。
    pub fn mc_versions(&self, default_parent: &Path) -> Result<Value> {
        let manifest = self.manifest()?;
        let fabric: Vec<String> = net::fetch_json(&format!("{FABRIC_META}/game"), None)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .into_iter()
            .flatten()
            .filter(|g| g["stable"] == json!(true))
            .map(|g| text_of(&g["version"]))
            .collect();
        let lists = self.loader_lists();
        let pick = |kind: &str, v: &str| loader_choice(kind, v, Some(&lists[kind]).filter(|x| !x.is_null()));
        let versions: Vec<Value> = releases(&manifest)
            .map(|v| json!({"id": v, "fabric": fabric.iter().any(|f| f == v), "forge": pick("forge", v), "neoforge": pick("neoforge", v)}))
            .collect();
        Ok(json!({"versions": versions, "ram_total_gb": sys::total_ram_gb(), "default_parent": default_parent.display().to_string()}))
    }

    /// 某個正式版的官方資料（含伺服器檔網址與需要的 Java）。
    pub fn version_meta(&self, version: &str) -> Result<Value> {
        let manifest = self.manifest()?;
        let entry = manifest["versions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|v| v["id"] == version && v["type"] == "release")
            .ok_or_else(|| UserError::new(format!("官方正式版清單裡沒有 {version}")))?;
        net::fetch_json(entry["url"].as_str().unwrap_or_default(), entry["sha1"].as_str())
    }

    /// 建服精靈選了版本後，告訴使用者電腦上的 Java 夠不夠。
    pub fn java_status(&self, version: &str, found: &[(u32, PathBuf)]) -> Result<Value> {
        let need = java_need(&self.version_meta(version)?, version);
        let best = java::best_java(Some(need), found);
        Ok(json!({"need": need, "ok": best.is_some(), "have": best.as_ref().map(|b| b.0),
                  "path": best.as_ref().map(|b| b.1.display().to_string()), "help_url": help_url(need)}))
    }
}

fn releases(manifest: &Value) -> impl Iterator<Item = &str> {
    manifest["versions"].as_array().into_iter().flatten().filter(|v| v["type"] == "release").filter_map(|v| v["id"].as_str())
}

fn java_need(meta: &Value, version: &str) -> u32 {
    meta["javaVersion"]["majorVersion"].as_u64().filter(|&n| n > 0).map(|n| n as u32).or_else(|| java::java_for_mc(version)).unwrap_or(8)
}

fn help_url(need: u32) -> String {
    let arch = if platform::OS == platform::Os::Mac { platform::mac_arch() } else { "x64" };
    platform::java_help_url(Some(need), platform::OS, arch)
}

/// 從 25565 起找一個沒被其他伺服器登記、也沒被佔用的連接埠。
pub fn free_port(used: &[u16]) -> u16 {
    (25565..25700).find(|p| !used.contains(p) && launch::port_free(*p)).unwrap_or(25565)
}

/// 網址路徑用的百分比編碼（1.x 的 urllib quote）。
fn quote(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// 建一台新伺服器：下載、安裝、寫啟動檔、server.properties 與 EULA（使用者已在 plan 勾選）。
/// 失敗時只清掉這次自己新建、原本不存在的資料夾。成功後由平台登記並第一次開機。
pub fn create(plan: &Plan, cat: &Catalog, job: &Mutex<Progress>, log_dir: &Path, used_ports: &[u16]) -> Result<Made> {
    let made = !plan.target.exists();
    let r = create_inner(plan, cat, job, log_dir, used_ports);
    if r.is_err() && made {
        let _ = fs::remove_dir_all(&plan.target);
    }
    r
}

fn create_inner(plan: &Plan, cat: &Catalog, job: &Mutex<Progress>, log_dir: &Path, used_ports: &[u16]) -> Result<Made> {
    let set = |msg: &str, pct: Option<u32>| {
        let mut j = job.lock().unwrap_or_else(|e| e.into_inner());
        (j.msg, j.pct) = (msg.to_string(), pct);
    };
    let io_err = |e: io::Error| UserError::new(format!("建立失敗：連不上官方網站或資料寫不進去（{e}）。請確認網路後再試一次。"));
    let bar = |label: &'static str| move |f: f64| set(label, Some((f * 100.0).round() as u32));
    let (version, target) = (plan.version.as_str(), plan.target.as_path());
    set("讀取官方版本資料…", None);
    let meta = cat.version_meta(version)?;
    let need = java_need(&meta, version);
    let found = plan.java_path.as_ref().map(|p| java::java_major(p).map(|v| vec![(v,p.clone())]).unwrap_or_default()).unwrap_or_else(java::installed_javas);
    let Some((_, java)) = java::best_java(Some(need), &found) else {
        return Err(UserError {
            msg: format!("Minecraft {version} 需要 Java {need} 以上，電腦上沒有找到。請點下方按鈕下載安裝，裝好後再建立一次。"),
            help_url: Some(help_url(need)),
        });
    };
    let server = &meta["downloads"]["server"];
    if !server.is_object() {
        return Err(UserError::new(format!("Minecraft {version} 官方沒有提供伺服器檔案，請選其他版本。")));
    }
    fs::create_dir_all(&plan.parent).map_err(io_err)?;
    if sys::disk_free(&plan.parent).is_some_and(|b| b < 2 << 30) {
        return Err(UserError::new("硬碟空間不足 2 GB，請換一個位置。"));
    }
    fs::create_dir_all(target).map_err(io_err)?;
    let (jar, label, loader, mut bat) = match plan.loader {
        "vanilla" => {
            let want = server["sha1"].as_str().map(|s| Checksum::Sha1(s.to_string()));
            net::download(server["url"].as_str().unwrap_or_default(), &target.join("server.jar"), want.as_ref(), server["size"].as_u64(), &mut bar("下載官方伺服器檔案…"))?;
            (Some("server.jar"), format!("{version} · 原版"), "", "")
        }
        kind @ ("forge" | "neoforge") => {
            let name = if kind == "forge" { "Forge" } else { "NeoForge" };
            set(&format!("查詢 {name} 版本…"), None);
            let lists = cat.loader_lists();
            let choice = loader_choice(kind, version, Some(&lists[kind]).filter(|x| !x.is_null()));
            let Some(v) = choice["version"].as_str() else {
                return Err(UserError::new(format!("{name}：{}，請換版本或換類型。", text_of(&choice["why"]))));
            };
            let (url, algo) = if kind == "forge" { (forge_installer_url(version, v), "sha1") } else { (neoforge_installer_url(v), "sha256") };
            let installer = target.join(url.rsplit('/').next().unwrap());
            let sum = net::fetch_checksum(&format!("{url}.{algo}"), algo)?;
            let want = if algo == "sha1" { Checksum::Sha1(sum) } else { Checksum::Sha256(sum) };
            let label = if kind == "forge" { "下載 Forge 安裝器…" } else { "下載 NeoForge 安裝器…" };
            net::download(&url, &installer, Some(&want), None, &mut bar(label))?;
            set(&format!("安裝 {name} {v}（安裝器會自己連官方網站下載約數百 MB，約 2–10 分鐘）…"), None);
            let mut on_line = |line: &str| {
                let mut j = job.lock().unwrap_or_else(|e| e.into_inner());
                j.lines.push(line.to_string());
                if j.lines.len() > 200 {
                    j.lines.remove(0);
                }
            };
            let bat = install_modloader(target, &java, &installer, plan.ram, &mut on_line, log_dir, INSTALL_TIMEOUT)?;
            fs::create_dir_all(target.join("mods")).map_err(io_err)?;
            (None, format!("{version} · {name}"), name, bat)
        }
        _ => {
            set("查詢 Fabric 最新穩定版…", None);
            let stable = |list: Value, key: fn(&Value) -> &Value| -> Vec<String> {
                list.as_array().into_iter().flatten().filter(|x| key(x)["stable"] == json!(true)).map(|x| text_of(&key(x)["version"])).collect()
            };
            let loaders = stable(net::fetch_json(&format!("{FABRIC_META}/loader/{}", quote(version)), None)?, |x| &x["loader"]);
            let installers = stable(net::fetch_json(&format!("{FABRIC_META}/installer"), None)?, |x| x);
            let (Some(l), Some(i)) = (loaders.first(), installers.first()) else {
                return Err(UserError::new(format!("Fabric 目前不支援 {version}，請換版本或選原版。")));
            };
            let jar = target.join("fabric-server.jar");
            let url = format!("{FABRIC_META}/loader/{}/{}/{}/server/jar", quote(version), quote(l), quote(i));
            net::download(&url, &jar, None, None, &mut bar("下載 Fabric 伺服器檔案…"))?;
            // Fabric 官方沒有公布這個 jar 的雜湊值，改驗檔案完整、內容版本相符。
            let ok = Jar::open(&jar).and_then(|mut z| {
                z.test()?;
                Ok(z.read("install.properties")?.unwrap_or_default())
            });
            let want = format!("game-version={version}");
            if !ok.is_ok_and(|p| p.windows(want.len()).any(|w| w == want.as_bytes())) {
                return Err(UserError::new("下載的 Fabric 檔案核對不符，請再試一次。"));
            }
            fs::create_dir_all(target.join("mods")).map_err(io_err)?;
            (Some("fabric-server.jar"), format!("{version} · Fabric"), "Fabric", "")
        }
    };
    set("寫入設定…", None);
    let port = free_port(used_ports);
    if let Some(jar) = jar {
        bat = platform::write_start_script(target, &java.display().to_string(), &format!("-Xms1G -Xmx{}G -jar {jar} nogui", plan.ram))?;
    }
    fs::write(target.join("server.properties"), format!("server-port={port}\n")).map_err(io_err)?;
    launch::write_eula(target).map_err(io_err)?;
    Ok(Made { label, loader, java_need: need, bat, port })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_safe::tests::temp;
    use crate::jar::write_zip;

    #[test]
    fn modloader_folders() {
        let d = temp("inspect-forge");
        fs::create_dir_all(d.join("libraries/net/minecraftforge/forge/1.20.1-47.4.0")).unwrap();
        fs::create_dir_all(d.join("libraries/net/minecraftforge/forge/1.19.2-43.1.1")).unwrap();
        assert_eq!(inspect_server(&d), json!({"version": "1.20.1", "java": 17, "loader": "Forge"}));
        let d = temp("inspect-neo");
        for v in ["21.1.9", "21.1.265", "21.1.300-beta"] {
            fs::create_dir_all(d.join("libraries/net/neoforged/neoforge").join(v)).unwrap();
        }
        assert_eq!(modloader_version(&d), Some(("1.21.1".into(), "NeoForge")));
        assert_eq!(inspect_server(&d)["java"], 21);
    }

    #[test]
    fn jars_and_log() {
        let d = temp("inspect-jar");
        assert_eq!(inspect_server(&d), json!({"version": null, "java": null, "loader": ""}));
        fs::create_dir_all(d.join("logs")).unwrap();
        fs::write(d.join("logs/latest.log"), "[Server thread/INFO]: Starting minecraft server version 1.20.4\n").unwrap();
        assert_eq!(inspect_server(&d), json!({"version": "1.20.4", "java": 17, "loader": ""}));
        write_zip(&d.join("fabric-server-launch.jar"), &[("install.properties", b"fabric-loader-version=0.16\ngame-version=1.21.1\n", true)]);
        assert_eq!(inspect_server(&d), json!({"version": "1.21.1", "java": 21, "loader": "Fabric"}));
        fs::write(d.join("broken.jar"), b"not a zip").unwrap();
        write_zip(&d.join("server.jar"), &[("version.json", br#"{"id": "9.9", "java_version": 99}"#, false)]);
        assert_eq!(inspect_server(&d), json!({"version": "9.9", "java": 99, "loader": "Fabric"}));
    }

    #[test]
    fn plan_checks() {
        let parent = temp("plan");
        let p = parent.display().to_string();
        let ok = json!({"eula": true, "name": " 新世界 ", "loader": "fabric", "ram_gb": "4", "parent": format!("\"{p}\""), "version": "1.21.1"});
        let got = plan(&ok).unwrap();
        assert_eq!((got.name.as_str(), got.ram, got.loader, got.target.clone()), ("新世界", 4, "fabric", parent.join("新世界")));
        let with = |k: &str, v: Value| {
            let mut r = ok.clone();
            r[k] = v;
            plan(&r).unwrap_err()
        };
        assert!(with("eula", json!("true")).contains("EULA"), "只認使用者本人勾的 true");
        assert_eq!(with("loader", json!("paper")), "類型只能選原版、Fabric、Forge 或 NeoForge");
        assert_eq!(with("ram_gb", json!(65)), "記憶體要在 1 到 64 GB 之間");
        assert_eq!(with("ram_gb", json!("四")), "記憶體要選一個數字");
        assert_eq!(with("parent", json!("相對路徑")), "存放位置要選一個完整的資料夾路徑");
        assert_eq!(with("name", json!("a/b")), fs_safe::check_folder_name("a/b").unwrap());
        let mut r = ok.clone();
        r["loader"] = json!("forge");
        r["name"] = json!("驚嘆號!");
        assert!(plan(&r).unwrap_err().contains("驚嘆號"));
        fs::create_dir_all(parent.join("新世界")).unwrap();
        assert!(plan(&ok).is_ok(), "空資料夾可以用");
        fs::write(parent.join("新世界/x.txt"), "x").unwrap();
        assert!(plan(&ok).unwrap_err().contains("不會覆蓋既有資料"));
    }

    #[test]
    fn small_helpers() {
        assert_eq!(quote("1.21.1"), "1.21.1");
        assert_eq!(quote("a b/ç"), "a%20b/%C3%A7");
        assert!(free_port(&[25565, 25566]) > 25566, "登記過的連接埠要跳過");
    }
}
