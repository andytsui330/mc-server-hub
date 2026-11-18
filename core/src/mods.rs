//! Modrinth：查詢與計畫不動檔；確認後由持有停服管理鎖的呼叫端套用。
use crate::{Result, UserError, net::Checksum};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub minecraft: String,
    pub loader: String,
}
pub struct Modrinth<F> {
    fetch: F,
}
#[derive(Default)]
struct Resolution {
    selected: HashMap<String, String>,
    visiting: HashSet<String>,
    files: Vec<Value>,
}
#[derive(Clone, Debug)]
pub struct InstallPlan {
    target: Target,
    files: Vec<Value>,
    base: Value,
    root: PathBuf,
}
impl InstallPlan {
    pub fn to_json(&self) -> Value {
        let old = self.base["managed"].as_array();
        let changes:Vec<Value>=self.files.iter().map(|f|json!({"project_id":f["project_id"],"filename":f["filename"],"version_id":f["version_id"],"action":if old.is_some_and(|a|a.iter().any(|e|e["project_id"]==f["project_id"])) {"replace"} else {"install"}})).collect();
        json!({"text":format!("將安裝或替換 {} 個模組（含必要相依）；確認後才會下載與換檔。",self.files.len()),"minecraft":self.target.minecraft,"loader":self.target.loader,"files":self.files,"changes":changes,"total_bytes":self.files.iter().filter_map(|f|f["size"].as_u64()).sum::<u64>(),"warning":"僅管理本工具安裝的模組；其他模組請人工確認相容性。套用前必須停服。"})
    }
}
impl<F: FnMut(&str) -> Result<Value>> Modrinth<F> {
    pub fn new(fetch: F) -> Self {
        Self { fetch }
    }
    pub fn search(&mut self, target: &Target, query: &str) -> Result<Vec<Value>> {
        check_target(target)?;
        if query.chars().count() > 160 || query.chars().any(char::is_control) {
            return Err(err("搜尋字太長或含不適用的字元。"));
        }
        let facets = json!([
            ["project_type:mod"],
            [format!("versions:{}", target.minecraft)],
            [format!("categories:{}", target.loader)],
            ["server_side:required", "server_side:optional"]
        ]);
        let value = (self.fetch)(&format!(
            "https://api.modrinth.com/v2/search?query={}&facets={}&limit=20",
            encode(query),
            encode(&facets.to_string())
        ))?;
        let hits = value["hits"]
            .as_array()
            .ok_or_else(|| err("Modrinth 搜尋結果格式不對。"))?;
        Ok(hits.iter().filter(|h|h["project_type"]=="mod"&&server_ok(h)).map(|h|json!({"project_id":h["project_id"],"title":h["title"],"description":h["description"],"downloads":h["downloads"],"server_side":h["server_side"]})).collect())
    }
    pub fn plan_install(
        &mut self,
        target: &Target,
        project: &str,
        dir: &Path,
    ) -> Result<InstallPlan> {
        self.plan(target, &[project.to_owned()], dir)
    }
    pub fn check_updates(&mut self, target: &Target, dir: &Path) -> Result<Vec<Value>> {
        check_target(target)?;
        let base = manifest(dir)?;
        check_manifest_target(&base, target)?;
        let mut updates = Vec::new();
        for installed in entries(&base)? {
            let project = text(installed, "project_id")?;
            self.project(project)?;
            let newest = self.latest(target, project)?;
            if newest["id"] != installed["version_id"]
                && newest["date_published"].as_str().unwrap_or("")
                    > installed["date_published"].as_str().unwrap_or("")
            {
                updates.push(json!({"project_id":project,"current_version_id":installed["version_id"],"version_id":newest["id"],"name":newest["name"]}));
            }
        }
        Ok(updates)
    }
    pub fn plan_updates(
        &mut self,
        target: &Target,
        projects: &[String],
        dir: &Path,
    ) -> Result<InstallPlan> {
        let base = manifest(dir)?;
        for project in projects {
            if !entries(&base)?.iter().any(|e| e["project_id"] == *project) {
                return Err(err("只能更新本工具已安裝的模組。"));
            }
        }
        self.plan(target, projects, dir)
    }
    fn project(&mut self, id: &str) -> Result<Value> {
        check_id(id)?;
        let p = (self.fetch)(&format!("https://api.modrinth.com/v2/project/{id}"))?;
        if p["project_type"] != "mod" || !server_ok(&p) {
            return Err(err("這個專案不是伺服器可用的模組，已停止。"));
        }
        check_id(text(&p, "id")?)?;
        Ok(p)
    }
    fn latest(&mut self, target: &Target, id: &str) -> Result<Value> {
        check_id(id)?;
        let url = format!(
            "https://api.modrinth.com/v2/project/{id}/version?loaders={}&game_versions={}&include_changelog=false",
            encode(&json!([target.loader]).to_string()),
            encode(&json!([target.minecraft]).to_string())
        );
        let value = (self.fetch)(&url)?;
        let mut versions: Vec<Value> = value
            .as_array()
            .ok_or_else(|| err("Modrinth 版本清單格式不對。"))?
            .iter()
            .filter(|v| compatible(v, target))
            .cloned()
            .collect();
        versions.sort_by(|a, b| {
            rank(b).cmp(&rank(a)).then_with(|| {
                b["date_published"]
                    .as_str()
                    .cmp(&a["date_published"].as_str())
            })
        });
        versions.into_iter().next().ok_or_else(|| {
            err("找不到符合目前 Minecraft 版本與載入器的伺服器模組；不會自動升級遊戲。")
        })
    }
    fn plan(&mut self, target: &Target, projects: &[String], dir: &Path) -> Result<InstallPlan> {
        check_target(target)?;
        let base = manifest(dir)?;
        check_manifest_target(&base, target)?;
        if projects.is_empty() || projects.len() > 100 {
            return Err(err("請選擇 1 到 100 個模組。"));
        }
        let mut state = Resolution::default();
        for project in projects {
            self.resolve(target, Some(project), None, false, &mut state)?;
        }
        let files = state.files;
        let mut names = HashSet::new();
        for f in &files {
            if !names.insert(text(f, "filename")?.to_lowercase()) {
                return Err(err("不同模組使用相同檔名，無法安全安裝。"));
            }
        }
        let final_entries = merge(&base, &files)?;
        validate_dependencies(&final_entries)?;
        check_files(dir, &base, &files)?;
        Ok(InstallPlan {
            target: target.clone(),
            files,
            base,
            root: fs::canonicalize(dir).map_err(|_| err("伺服器資料夾無法讀取。"))?,
        })
    }
    fn resolve(
        &mut self,
        target: &Target,
        project: Option<&str>,
        version: Option<&str>,
        dependency: bool,
        state: &mut Resolution,
    ) -> Result<()> {
        if state.selected.len() + state.visiting.len() >= 100 {
            return Err(err("模組相依過多，請人工確認。"));
        }
        let v = if let Some(id) = version {
            check_id(id)?;
            (self.fetch)(&format!("https://api.modrinth.com/v2/version/{id}"))?
        } else {
            let p = self.project(project.ok_or_else(|| err("相依缺少專案或版本編號。"))?)?;
            self.latest(target, text(&p, "id")?)?
        };
        let pid = text(&v, "project_id")?.to_owned();
        let vid = text(&v, "id")?.to_owned();
        check_id(&pid)?;
        check_id(&vid)?;
        if version.is_some_and(|id| id != vid) {
            return Err(err("Modrinth 回傳了不同的相依版本。"));
        }
        let p = self.project(&pid)?;
        if project.is_some_and(|id| id != pid && p["slug"].as_str() != Some(id)) {
            return Err(err("相依專案與指定版本不一致。"));
        }
        if !compatible(&v, target) {
            return Err(err("必要相依不支援目前 Minecraft 版本或載入器。"));
        }
        if state.visiting.contains(&pid) {
            return Err(err("模組有循環相依，請人工確認。"));
        }
        if let Some(chosen) = state.selected.get(&pid) {
            return if chosen == &vid {
                Ok(())
            } else {
                Err(err("同一模組需要互相衝突的版本，已停止。"))
            };
        }
        state.visiting.insert(pid.clone());
        let deps = v["dependencies"]
            .as_array()
            .ok_or_else(|| err("模組相依資料格式不對。"))?;
        for dep in deps {
            match dep["dependency_type"].as_str() {
                Some("required") => {
                    if dep["file_name"].as_str().is_some() {
                        return Err(err("模組需要外部檔案相依，請人工安裝並確認來源。"));
                    }
                    self.resolve(
                        target,
                        dep["project_id"].as_str(),
                        dep["version_id"].as_str(),
                        true,
                        state,
                    )?;
                }
                Some("optional" | "embedded" | "incompatible") => {}
                _ => return Err(err("Modrinth 回傳不認得的相依類型。")),
            }
        }
        let mut file = version_file(&v)?;
        file["dependency"] = json!(dependency);
        file["title"] = p["title"].clone();
        state.visiting.remove(&pid);
        state.selected.insert(pid, vid);
        state.files.push(file);
        Ok(())
    }
}

const MANIFEST: &str = ".mchub-mods.json";
const BACKUPS: &str = ".mchub-mod-backups";
const MARKER: &str = ".mchub-mod-transaction.json";
fn err(s: &str) -> UserError {
    UserError::new(s)
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| err("Modrinth 或安裝紀錄缺少必要欄位。"))
}
fn check_id(id: &str) -> Result<()> {
    if id.len() > 128
        || id.is_empty()
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
    {
        Err(err("模組編號格式不對。"))
    } else {
        Ok(())
    }
}
fn check_target(t: &Target) -> Result<()> {
    if !["fabric", "forge", "neoforge", "quilt"].contains(&t.loader.as_str()) {
        return Err(err("原版伺服器不能安裝模組，請先選用支援的模組載入器。"));
    }
    if t.minecraft.is_empty()
        || t.minecraft.len() > 40
        || !t
            .minecraft
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-+".contains(&c))
    {
        return Err(err("Minecraft 版本格式不對。"));
    }
    Ok(())
}
fn server_ok(v: &Value) -> bool {
    matches!(v["server_side"].as_str(), Some("required" | "optional"))
}
fn compatible(v: &Value, t: &Target) -> bool {
    v["game_versions"]
        .as_array()
        .is_some_and(|a| a.iter().any(|x| x == &t.minecraft))
        && v["loaders"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x == &t.loader))
        && matches!(
            v["version_type"].as_str(),
            Some("release" | "beta" | "alpha")
        )
        && v["status"]
            .as_str()
            .is_none_or(|s| matches!(s, "listed" | "unlisted" | "archived"))
}
fn rank(v: &Value) -> u8 {
    match v["version_type"].as_str() {
        Some("release") => 3,
        Some("beta") => 2,
        _ => 1,
    }
}
fn encode(s: &str) -> String {
    let mut out = String::new();
    for c in s.bytes() {
        if c.is_ascii_alphanumeric() || b"-._~".contains(&c) {
            out.push(c as char)
        } else {
            out.push_str(&format!("%{c:02X}"))
        }
    }
    out
}
fn check_filename(s: &str) -> Result<()> {
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if s.is_empty()
        || s.len() > 240
        || reserved
        || s.ends_with(['.', ' '])
        || s.chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || !s.to_ascii_lowercase().ends_with(".jar")
    {
        return Err(err("模組檔名不安全或不是 jar，已停止。"));
    }
    Ok(())
}
fn version_file(v: &Value) -> Result<Value> {
    let files = v["files"]
        .as_array()
        .ok_or_else(|| err("模組沒有檔案資料。"))?;
    let f = files
        .iter()
        .find(|f| f["primary"] == true)
        .or_else(|| {
            if files.len() == 1 {
                files.first()
            } else {
                None
            }
        })
        .ok_or_else(|| err("模組沒有明確的主要檔案。"))?;
    check_filename(text(f, "filename")?)?;
    let url = text(f, "url")?;
    crate::net::check_url_from(url, &crate::net::Source::Modrinth)?;
    if !url.starts_with("https://cdn.modrinth.com/data/") {
        return Err(err("模組檔案不是 Modrinth 官方下載來源。"));
    }
    let hash = text(&f["hashes"], "sha1")?;
    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(err("Modrinth 缺少正確的 SHA-1 檢查碼，已停止。"));
    }
    let size = f["size"]
        .as_u64()
        .filter(|s| *s > 0 && *s <= 2_147_483_648)
        .ok_or_else(|| err("模組檔案大小不合理。"))?;
    Ok(
        json!({"project_id":v["project_id"],"version_id":v["id"],"name":v["name"],"version_number":v["version_number"],"date_published":v["date_published"],"filename":f["filename"],"url":url,"sha1":hash.to_ascii_lowercase(),"size":size,"dependencies":v["dependencies"]}),
    )
}
fn no_links(path: &Path) -> Result<()> {
    let mut at = PathBuf::new();
    for part in path.components() {
        at.push(part);
        match fs::symlink_metadata(&at) {
            Ok(m) => {
                #[cfg(windows)]
                let link = {
                    use std::os::windows::fs::MetadataExt;
                    m.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let link = m.file_type().is_symlink();
                if link {
                    return Err(err("模組路徑包含連結，已停止以保護其他資料。"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(err("無法檢查模組檔案路徑。")),
        }
    }
    Ok(())
}
fn child(dir: &Path, rel: &Path) -> Result<PathBuf> {
    let path = crate::fs_safe::child(dir, rel)?;
    no_links(&path)?;
    Ok(path)
}
fn entries(base: &Value) -> Result<&Vec<Value>> {
    base["managed"]
        .as_array()
        .ok_or_else(|| err("模組安裝紀錄損壞；請先人工確認，避免蓋掉既有檔案。"))
}
fn manifest(dir: &Path) -> Result<Value> {
    ensure_safe(dir)?;
    no_links(dir)?;
    if !dir.is_dir() {
        return Err(err("伺服器資料夾不存在。"));
    }
    let path = child(dir, Path::new(MANIFEST))?;
    let base = match fs::read(&path) {
        Ok(bytes) => {
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(err("模組安裝紀錄過大。"));
            }
            serde_json::from_slice::<Value>(&bytes)
                .map_err(|_| err("模組安裝紀錄不是正確 JSON。"))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({"schema":1,"managed":[]}),
        Err(_) => return Err(err("無法讀取模組安裝紀錄。")),
    };
    if base["schema"] != 1 {
        return Err(err("模組安裝紀錄版本不支援。"));
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for e in entries(&base)? {
        check_id(text(e, "project_id")?)?;
        check_id(text(e, "version_id")?)?;
        check_filename(text(e, "filename")?)?;
        let hash = text(e, "sha1")?;
        if hash.len() != 40
            || !hash.bytes().all(|c| c.is_ascii_hexdigit())
            || e["size"].as_u64().is_none()
            || !ids.insert(text(e, "project_id")?)
            || !names.insert(text(e, "filename")?.to_lowercase())
        {
            return Err(err("模組安裝紀錄內容不完整或重複。"));
        }
    }
    Ok(base)
}
fn check_manifest_target(base: &Value, target: &Target) -> Result<()> {
    if !entries(base)?.is_empty()
        && (base["minecraft"] != target.minecraft || base["loader"] != target.loader)
    {
        return Err(err(
            "目前伺服器版本或載入器與安裝紀錄不同；請先人工確認模組相容性。",
        ));
    }
    Ok(())
}
fn merge(base: &Value, files: &[Value]) -> Result<Vec<Value>> {
    let mut final_entries: Vec<Value> = entries(base)?
        .iter()
        .filter(|old| !files.iter().any(|f| f["project_id"] == old["project_id"]))
        .cloned()
        .collect();
    final_entries.extend(files.iter().cloned());
    let mut names = HashSet::new();
    for f in &final_entries {
        if !names.insert(text(f, "filename")?.to_lowercase()) {
            return Err(err("模組檔名與既有管理模組衝突。"));
        }
    }
    Ok(final_entries)
}
fn validate_dependencies(files: &[Value]) -> Result<()> {
    for f in files {
        let deps = f["dependencies"]
            .as_array()
            .ok_or_else(|| err("安裝紀錄缺少相依資料。"))?;
        for d in deps {
            let pid = d["project_id"].as_str();
            let vid = d["version_id"].as_str();
            if d["dependency_type"] == "incompatible" && d["file_name"].as_str().is_some() {
                return Err(err("外部檔案的不相容相依無法自動判斷，請人工確認。"));
            }
            let matches = files.iter().any(|candidate| {
                pid.is_none_or(|id| candidate["project_id"] == id)
                    && vid.is_none_or(|id| candidate["version_id"] == id)
            });
            match d["dependency_type"].as_str() {
                Some("required")
                    if (pid.is_none() && vid.is_none())
                        || d["file_name"].as_str().is_some()
                        || !matches =>
                {
                    return Err(err("必要相依缺失或版本衝突，已停止。"));
                }
                Some("incompatible") if (pid.is_some() || vid.is_some()) && matches => {
                    return Err(err("模組宣告與另一個已安裝模組不相容，已停止。"));
                }
                Some("incompatible") if pid.is_none() && vid.is_none() => {
                    return Err(err("外部不相容相依無法自動判斷，請人工確認。"));
                }
                Some("required" | "incompatible" | "optional" | "embedded") => {}
                _ => return Err(err("安裝紀錄有不認得的相依類型。")),
            }
        }
    }
    Ok(())
}
fn verify(path: &Path, entry: &Value) -> Result<()> {
    no_links(path)?;
    let mut f =
        fs::File::open(path).map_err(|_| err("已管理的模組檔案遺失或無法讀取，請先人工確認。"))?;
    if !f
        .metadata()
        .map_err(|_| err("模組檔案無法讀取。"))?
        .is_file()
    {
        return Err(err("模組路徑不是一般檔案。"));
    }
    let mut sha = Sha1::new();
    let mut buf = [0u8; 65536];
    let mut size = 0;
    loop {
        let n = f.read(&mut buf).map_err(|_| err("模組檔案讀取失敗。"))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        sha.update(&buf[..n]);
    }
    let hash: String = sha.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if Some(size) != entry["size"].as_u64() || Some(hash.as_str()) != entry["sha1"].as_str() {
        return Err(err("模組檔案大小或 SHA-1 不符，已停止，沒有覆蓋既有模組。"));
    }
    Ok(())
}
fn check_files(dir: &Path, base: &Value, files: &[Value]) -> Result<()> {
    let mods = child(dir, Path::new("mods"))?;
    for old in entries(base)? {
        verify(
            &child(dir, &Path::new("mods").join(text(old, "filename")?))?,
            old,
        )?;
    }
    if mods.exists() {
        for item in fs::read_dir(&mods).map_err(|_| err("無法讀取 mods 資料夾。"))? {
            let item = item.map_err(|_| err("無法讀取 mods 裡的檔案。"))?;
            let name = item.file_name().to_string_lossy().to_string();
            no_links(&item.path())?;
            if files.iter().any(|f| {
                f["filename"]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&name))
            }) && !entries(base)?.iter().any(|old| {
                old["filename"].as_str() == Some(name.as_str())
                    && files.iter().any(|f| f["project_id"] == old["project_id"])
            }) {
                return Err(err("檔名與未管理或其他模組重複；不會覆蓋。"));
            }
        }
    }
    Ok(())
}
pub fn list_installed(dir: &Path) -> Result<Value> {
    let base = manifest(dir)?;
    let mut managed = Vec::new();
    let mut unknown = Vec::new();
    for old in entries(&base)? {
        let path = child(dir, &Path::new("mods").join(text(old, "filename")?))?;
        let mut row = old.clone();
        row.as_object_mut().unwrap().remove("dependencies");
        row.as_object_mut().unwrap().remove("url");
        row["missing"] = json!(!path.exists());
        row["modified"] = json!(path.exists() && verify(&path, old).is_err());
        managed.push(row);
    }
    let mods = child(dir, Path::new("mods"))?;
    if mods.exists() {
        for entry in fs::read_dir(&mods).map_err(|_| err("無法讀取 mods 資料夾。"))? {
            let entry = entry.map_err(|_| err("無法讀取模組檔案。"))?;
            no_links(&entry.path())?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.to_ascii_lowercase().ends_with(".jar")
                && !entries(&base)?.iter().any(|e| e["filename"] == name)
            {
                unknown.push(name);
            }
        }
    }
    unknown.sort();
    Ok(json!({"managed":managed,"unknown":unknown}))
}

fn manifest_digest(dir: &Path) -> Result<Option<String>> {
    let path = child(dir, Path::new(MANIFEST))?;
    match fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| err("模組紀錄無法讀取。"))?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(err("模組紀錄過大，請人工確認。"));
            }
            Ok(Some(
                Sha1::digest(&bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(err("模組紀錄無法核對，請先人工確認。")),
    }
}
/// 開服也必須先通過此檢查；中斷換檔時保守停住，由使用者依備份人工救援。
pub fn ensure_safe(dir: &Path) -> Result<()> {
    no_links(dir)?;
    let path = child(dir, Path::new(MARKER))?;
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => {
            return Err(err(
                "模組換檔狀態無法讀取；請先人工確認 .mchub-mod-backups 後再開服。",
            ));
        }
    };
    let marker: Value = serde_json::from_slice(&bytes)
        .map_err(|_| err("模組換檔狀態損壞，禁止開服；請人工核對備份。"))?;
    if marker["schema"] != 1
        || !matches!(marker["phase"].as_str(), Some("complete" | "rolled_back"))
    {
        return Err(err(
            "上次模組換檔尚未安全完成，禁止開服；請人工確認 .mchub-mod-backups 並還原。",
        ));
    }
    let digest = manifest_digest(dir)?;
    if marker.get("manifest_sha1").is_none() || marker["manifest_sha1"] != json!(digest) {
        return Err(err(
            "模組紀錄與換檔完成快照不符，禁止開服；請人工確認備份。",
        ));
    }
    Ok(())
}
fn write_marker(dir: &Path, tx: &Path, backup: &str, phase: &str) -> Result<()> {
    let path = tx.join(format!("marker-{phase}.json"));
    let value =
        json!({"schema":1,"phase":phase,"backup":backup,"manifest_sha1":manifest_digest(dir)?});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| err("無法建立模組換檔狀態檔，已停止。"))?;
    file.write_all(value.to_string().as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| err("模組換檔狀態無法存入硬碟，已停止。"))?;
    drop(file);
    let dest = child(dir, Path::new(MARKER))?;
    fs::rename(path, dest).map_err(|_| err("無法更新模組換檔狀態，請人工確認備份後再開服。"))
}
/// 呼叫端必須在整個呼叫期間持有伺服器管理鎖，並確認伺服器已停服。
pub fn apply_plan(
    dir: &Path,
    plan: &InstallPlan,
    mut download: impl FnMut(&str, &Path, &Checksum, u64) -> Result<()>,
) -> Result<Value> {
    let base = manifest(dir)?;
    if fs::canonicalize(dir).ok().as_ref() != Some(&plan.root) || base != plan.base {
        return Err(err("安裝計畫已過期或伺服器不同，請重新檢查與確認。"));
    }
    check_files(dir, &base, &plan.files)?;
    let mods = child(dir, Path::new("mods"))?;
    fs::create_dir_all(&mods).map_err(|_| err("無法建立 mods 資料夾。"))?;
    let backups = child(dir, Path::new(BACKUPS))?;
    fs::create_dir_all(&backups).map_err(|_| err("無法建立模組備份資料夾。"))?;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let name = format!(
        "{}-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let tx = child(dir, &Path::new(BACKUPS).join(&name))?;
    fs::create_dir(&tx).map_err(|_| err("無法建立模組交易備份。"))?;
    let stage = tx.join("stage");
    fs::create_dir(&stage).map_err(|_| err("無法建立模組暫存資料夾。"))?;
    for file in &plan.files {
        let dest = stage.join(text(file, "filename")?);
        let checksum = Checksum::Sha1(text(file, "sha1")?.into());
        download(
            text(file, "url")?,
            &dest,
            &checksum,
            file["size"].as_u64().unwrap(),
        )?;
        verify(&dest, file)?;
        fs::OpenOptions::new()
            .write(true)
            .open(&dest)
            .and_then(|f| f.sync_all())
            .map_err(|_| err("模組暫存檔無法確實存入硬碟。"))?;
    }
    // 下載期間仍可能被外部程式改檔，因此換檔前再核對一次。
    if manifest(dir)? != base {
        return Err(err("安裝紀錄已被修改，請重新建立計畫。"));
    }
    check_files(dir, &base, &plan.files)?;
    let final_entries = merge(&base, &plan.files)?;
    validate_dependencies(&final_entries)?;
    let next = json!({"schema":1,"minecraft":plan.target.minecraft,"loader":plan.target.loader,"managed":final_entries});
    let next_path = tx.join("manifest.next.json");
    fs::write(
        &next_path,
        serde_json::to_vec_pretty(&next).map_err(|_| err("無法整理模組紀錄。"))?,
    )
    .map_err(|_| err("無法暫存模組紀錄。"))?;
    fs::OpenOptions::new()
        .write(true)
        .open(&next_path)
        .and_then(|f| f.sync_all())
        .map_err(|_| err("模組紀錄無法確實存入硬碟。"))?;
    let manifest_path = child(dir, Path::new(MANIFEST))?;
    let backup = format!("{BACKUPS}/{name}");
    write_marker(dir, &tx, &backup, "swapping")?;
    let mut moved: Vec<String> = Vec::new();
    let mut installed: Vec<String> = Vec::new();
    let mut manifest_moved = false;
    let result = (|| -> Result<()> {
        for old in entries(&base)? {
            if plan
                .files
                .iter()
                .any(|f| f["project_id"] == old["project_id"])
            {
                let filename = text(old, "filename")?;
                fs::rename(mods.join(filename), tx.join(filename))
                    .map_err(|_| err("舊模組備份失敗，已停止。"))?;
                moved.push(filename.into());
            }
        }
        for file in &plan.files {
            let filename = text(file, "filename")?;
            let dest = child(dir, &Path::new("mods").join(filename))?;
            if dest.exists() {
                return Err(err("換檔前發現同名檔案，已停止。"));
            }
            fs::rename(stage.join(filename), dest).map_err(|_| err("模組替換失敗，正在還原。"))?;
            installed.push(filename.into());
        }
        if manifest_path.exists() {
            fs::rename(&manifest_path, tx.join(MANIFEST)).map_err(|_| err("安裝紀錄備份失敗。"))?;
            manifest_moved = true;
        }
        fs::rename(&next_path, &manifest_path).map_err(|_| err("安裝紀錄替換失敗，正在還原。"))?;
        Ok(())
    })();
    if let Err(e) = result {
        let mut failed = false;
        for filename in installed.iter().rev() {
            failed |= fs::rename(mods.join(filename), stage.join(filename)).is_err();
        }
        for filename in moved.iter().rev() {
            failed |= fs::rename(tx.join(filename), mods.join(filename)).is_err();
        }
        if manifest_moved {
            failed |= fs::rename(tx.join(MANIFEST), &manifest_path).is_err();
        }
        if failed {
            return Err(err(
                "模組替換失敗且部分還原失敗；舊檔仍保留在 .mchub-mod-backups，請先人工還原再開服。",
            ));
        }
        write_marker(dir, &tx, &backup, "rolled_back")?;
        return Err(e);
    }
    write_marker(dir, &tx, &backup, "complete")?;
    Ok(
        json!({"text":format!("已安裝或更新 {} 個模組，舊檔已保留在備份。",plan.files.len()),"installed":plan.files.len(),"backup":backup}),
    )
}
