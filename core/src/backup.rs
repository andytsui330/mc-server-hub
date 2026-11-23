//! 離線世界備份：完整清單、保留份數及先快照再交換的交易式還原。
use crate::server::{launch, sys};
use crate::{Result, UserError, platform};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MANIFEST: &str = "manifest.json";
const FORMAT: &str = "mc-hub-world-backup-v1";
const JOURNAL_FORMAT: &str = "mc-hub-restore-v1";
static NEXT: AtomicU64 = AtomicU64::new(0);

/// 開機、備份及還原前檢查；中斷交易保留原物，交由人工使用快照／previous 復原。
pub fn ensure_restore_safe(server_dir: &Path) -> Result<()> {
    let server = checked_path(server_dir)?;
    for entry in fs::read_dir(&server).map_err(error)? {
        let entry = entry.map_err(error)?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(".mchub-restore-")
        {
            continue;
        }
        let transaction = entry.path();
        let blocked = |snapshot: &str| {
            UserError::new(format!(
                "發現未完成或無法確認的還原交易，已停止開機、備份與還原。交易資料：{}；快照：{}。請先人工確認並復原世界，再處理交易紀錄。",
                transaction.display(),
                snapshot
            ))
        };
        let unknown = "請查看交易的 journal.json／備份根目錄的 _snapshots";
        if !metadata(&transaction).is_ok_and(|m| m.is_dir()) {
            return Err(blocked(unknown));
        }
        let journal = transaction.join("journal.json");
        match fs::symlink_metadata(transaction.join("journal.json.tmp")) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(blocked(unknown)),
        }
        match fs::symlink_metadata(&journal) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let previous = transaction.join("previous");
                match fs::symlink_metadata(&previous) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Ok(_)
                        if metadata(&previous).is_ok_and(|m| m.is_dir())
                            && fs::read_dir(&previous)
                                .ok()
                                .is_some_and(|mut entries| entries.next().is_none()) =>
                    {
                        continue;
                    }
                    _ => {}
                }
                return Err(blocked(unknown));
            }
            Err(_) => return Err(blocked(unknown)),
            Ok(_) => {}
        }
        if metadata(&journal).is_err() {
            return Err(blocked(unknown));
        }
        let parsed = fs::read(&journal)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        let Some(m) = parsed else {
            return Err(blocked(unknown));
        };
        let snapshot = m["snapshot_path"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(unknown);
        let valid_path = |key: &str| {
            m[key]
                .as_str()
                .is_some_and(|s| !s.is_empty() && Path::new(s).is_absolute())
        };
        let valid_worlds = |key: &str| {
            m[key].as_array().is_some_and(|worlds| {
                worlds.len() <= 3
                    && worlds.iter().enumerate().all(|(i, world)| {
                        world.as_str().is_some_and(safe_name) && !worlds[..i].contains(world)
                    })
            })
        };
        if m["format"] != JOURNAL_FORMAT
            || m["server"].as_str() != server.to_str()
            || !valid_path("snapshot_path")
            || !valid_path("source")
            || !valid_worlds("worlds_before")
            || !valid_worlds("worlds_after")
            || m["worlds_after"].as_array().is_none_or(|w| w.is_empty())
            || !matches!(m["state"].as_str(), Some("completed" | "rolled_back"))
        {
            return Err(blocked(snapshot));
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

// Windows 以 WRITE_THROUGH 提交目錄交換；Unix 額外同步兩端目錄的中繼資料。
fn durable_rename(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(not(windows))]
    fs::rename(from, to)?;
    if let Some(parent) = from.parent() {
        sync_directory(parent)?;
    }
    if let Some(parent) = to.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::File::options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(error)?;
    file.write_all(bytes).map_err(error)?;
    file.sync_all().map_err(error)
}

fn write_journal(transaction: &Path, journal: &Value) -> Result<()> {
    let pending = transaction.join("journal.json.tmp");
    write_synced(
        &pending,
        &serde_json::to_vec_pretty(journal).map_err(error)?,
    )?;
    durable_rename(&pending, &transaction.join("journal.json")).map_err(error)
}

fn error(e: impl std::fmt::Display) -> UserError {
    UserError::new(format!("備份／還原未完成：{e}"))
}

fn no_links(p: &Path) -> UserError {
    UserError::new(format!(
        "資料夾裡有捷徑連結「{}」，平台不跟著複製。請移除後再操作。",
        p.display()
    ))
}

fn metadata(p: &Path) -> Result<fs::Metadata> {
    let m = fs::symlink_metadata(p).map_err(error)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(no_links(p));
        }
    }
    if m.file_type().is_symlink() {
        return Err(no_links(p));
    }
    Ok(m)
}

// 逐層檢查，包括已存在的父目錄；canonicalize 本身會跟隨連結，不能先做。
fn checked_path(p: &Path) -> Result<PathBuf> {
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(error("路徑不能包含 .."));
    }
    let absolute = if p.is_absolute() {
        p.to_owned()
    } else {
        std::env::current_dir().map_err(error)?.join(p)
    };
    let mut anchor = None;
    for ancestor in absolute.ancestors().collect::<Vec<_>>().into_iter().rev() {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                metadata(ancestor)?;
                anchor = Some(ancestor);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(error(e)),
        }
    }
    let anchor = anchor.ok_or_else(|| error("找不到路徑的根目錄"))?;
    let canonical = fs::canonicalize(anchor).map_err(error)?;
    #[cfg(windows)]
    let canonical = {
        let text = canonical.to_string_lossy();
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{unc}"))
        } else {
            PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
        }
    };
    Ok(canonical.join(absolute.strip_prefix(anchor).map_err(error)?))
}

fn within(path: &Path, parent: &Path) -> bool {
    #[cfg(windows)]
    {
        Path::new(&path.to_string_lossy().to_lowercase())
            .starts_with(parent.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        path.starts_with(parent)
    }
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.ends_with(['.', ' '])
        && !name.chars().any(|c| c < ' ' || r#"<>:"/\|?*"#.contains(c))
        && ![
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ]
        .contains(&name.split('.').next().unwrap_or("").to_uppercase().as_str())
}

fn world_level(server: &Path) -> Result<String> {
    if server.join("server.properties").exists() {
        metadata(&server.join("server.properties"))?;
    }
    // 保留 1.x 契約：舊 level-name 只取最後一段；不讓它逃出伺服器。
    let raw = launch::read_prop(server, "level-name")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "world".into());
    let level = Path::new(&raw)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("world")
        .to_owned();
    if !safe_name(&level) || level.starts_with(".mchub-") {
        return Err(error("世界名稱無效"));
    }
    Ok(level)
}

fn world_names(level: &str) -> Vec<String> {
    vec![
        level.to_owned(),
        format!("{level}_nether"),
        format!("{level}_the_end"),
    ]
}

fn locations(server: &Path, root: &Path) -> Result<(PathBuf, PathBuf, String)> {
    let server = checked_path(server)?;
    if !metadata(&server)?.is_dir() {
        return Err(error("伺服器資料夾不存在"));
    }
    let level = world_level(&server)?;
    let root = checked_path(root)?;
    if within(&server, &root)
        || world_names(&level)
            .iter()
            .any(|w| within(&root, &server.join(w)))
    {
        return Err(error("備份位置不能放在世界裡，也不能包含伺服器資料夾"));
    }
    Ok((server, root, level))
}

fn existing_worlds(server: &Path, level: &str) -> Result<Vec<String>> {
    let mut worlds = Vec::new();
    for name in world_names(level) {
        let path = server.join(&name);
        match fs::symlink_metadata(&path) {
            Ok(_) if metadata(&path)?.is_dir() => worlds.push(name),
            Ok(_) => return Err(error(format!("世界位置不是資料夾：{}", path.display()))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(error(e)),
        }
    }
    Ok(worlds)
}

fn hash_file(path: &Path) -> Result<(u64, String)> {
    metadata(path)?;
    let mut file = fs::File::open(path).map_err(error)?;
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(error)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        bytes += n as u64;
    }
    Ok((bytes, format!("{:x}", hash.finalize())))
}

fn walk(root: &Path, relative: &Path, inventory: &mut BTreeMap<String, Value>) -> Result<()> {
    let path = root.join(relative);
    let m = metadata(&path)?;
    let key = relative
        .to_str()
        .ok_or_else(|| error("檔名不是有效 UTF-8"))?
        .replace('\\', "/");
    if m.is_dir() {
        inventory.insert(key, json!({"kind": "directory"}));
        for entry in fs::read_dir(&path).map_err(error)? {
            let entry = entry.map_err(error)?;
            walk(root, &relative.join(entry.file_name()), inventory)?;
        }
    } else if m.is_file() {
        let (bytes, hash) = hash_file(&path)?;
        inventory.insert(key, json!({"kind": "file", "bytes": bytes, "sha256": hash}));
    } else {
        return Err(error(format!("不支援的檔案類型：{}", path.display())));
    }
    Ok(())
}

fn inventory(root: &Path, worlds: &[String]) -> Result<Value> {
    let mut items = BTreeMap::new();
    for world in worlds {
        walk(root, Path::new(world), &mut items)?;
    }
    serde_json::to_value(items).map_err(error)
}

fn stats(items: &Value) -> (u64, u64) {
    items
        .as_object()
        .unwrap()
        .values()
        .filter(|v| v["kind"] == "file")
        .fold((0, 0), |(f, b), v| {
            (f + 1, b + v["bytes"].as_u64().unwrap())
        })
}

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    let m = metadata(src)?;
    if !m.is_dir() {
        return Err(error("來源不是資料夾"));
    }
    fs::create_dir(dst).map_err(error)?;
    for entry in fs::read_dir(src).map_err(error)? {
        let entry = entry.map_err(error)?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let m = metadata(&from)?;
        if m.is_dir() {
            copy_tree(&from, &to)?;
        } else if m.is_file() {
            fs::copy(&from, &to).map_err(error)?;
            let file = fs::File::options().write(true).open(&to).map_err(error)?;
            file.set_modified(m.modified().map_err(error)?)
                .map_err(error)?;
            file.sync_all().map_err(error)?;
        } else {
            return Err(error("不支援的檔案類型"));
        }
    }
    sync_directory(dst).map_err(error)
}

fn unique() -> String {
    format!(
        "{}-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn make_backup(
    server: &Path,
    root: &Path,
    level: &str,
    kind: &str,
    allow_empty: bool,
) -> Result<Value> {
    let worlds = existing_worlds(server, level)?;
    if worlds.is_empty() && !allow_empty {
        return Err(UserError::new(format!(
            "找不到世界資料夾「{level}」，這台可能還沒開過機。"
        )));
    }
    let before = inventory(server, &worlds)?;
    let (files, bytes) = stats(&before);
    fs::create_dir_all(root).map_err(error)?;
    checked_path(root)?;
    if sys::disk_free(root).is_some_and(|free| (free as f64) < bytes as f64 * 1.05 + 104_857_600.0)
    {
        return Err(error("硬碟空間不夠"));
    }
    let token = unique();
    let name = format!("{level}_{}_{}", platform::local_stamp(), token);
    let pending = root.join(format!("{name}_未完成"));
    let dest = root.join(&name);
    fs::create_dir(&pending).map_err(error)?;
    for world in &worlds {
        copy_tree(&server.join(world), &pending.join(world))?;
    }
    if inventory(&pending, &worlds)? != before
        || inventory(server, &worlds)? != before
        || existing_worlds(server, level)? != worlds
    {
        return Err(error("備份逐檔 SHA-256 對帳不一致，已保留未完成資料"));
    }
    let manifest = json!({"format": FORMAT, "kind": kind, "server": server.to_string_lossy(), "level": level,
        "created_at": token, "worlds": worlds, "inventory": before, "files": files, "bytes": bytes});
    write_synced(
        &pending.join(MANIFEST),
        &serde_json::to_vec_pretty(&manifest).map_err(error)?,
    )?;
    sync_directory(&pending).map_err(error)?;
    durable_rename(&pending, &dest).map_err(error)?;
    Ok(
        json!({"ok": true, "name": name, "path": dest.to_string_lossy(), "files": files, "bytes": bytes,
        "created_at": manifest["created_at"], "text": format!("備份完成：{files} 個檔、{:.0} MB（對帳一致）\n位置：{}", bytes as f64 / 1_048_576.0, dest.display())}),
    )
}

fn validate_backup(server: &Path, path: &Path, level: &str, kind: &str) -> Result<Value> {
    if !metadata(path)?.is_dir() {
        return Err(error("備份位置不是資料夾"));
    }
    metadata(&path.join(MANIFEST))?;
    let m: Value =
        serde_json::from_slice(&fs::read(path.join(MANIFEST)).map_err(error)?).map_err(error)?;
    if m["format"] != FORMAT
        || m["kind"] != kind
        || m["server"].as_str() != server.to_str()
        || m["level"] != level
        || m["created_at"].as_str().is_none()
    {
        return Err(error("不是這台伺服器的完整平台備份"));
    }
    let allowed = world_names(level);
    let worlds: Vec<String> = m["worlds"]
        .as_array()
        .ok_or_else(|| error("備份世界清單無效"))?
        .iter()
        .map(|w| {
            w.as_str()
                .filter(|w| allowed.iter().any(|a| a == w))
                .map(str::to_owned)
                .ok_or_else(|| error("備份世界路徑無效"))
        })
        .collect::<Result<_>>()?;
    let expected: Vec<_> = allowed.into_iter().filter(|w| worlds.contains(w)).collect();
    if worlds != expected || (kind == "backup" && worlds.is_empty()) {
        return Err(error("備份世界清單不完整"));
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(path).map_err(error)? {
        let entry = entry.map_err(error)?;
        metadata(&entry.path())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| error("備份檔名無效"))?;
        if name != MANIFEST {
            entries.push(name);
        }
    }
    entries.sort();
    let mut sorted = worlds.clone();
    sorted.sort();
    if entries != sorted {
        return Err(error("備份含未登記的檔案或缺少世界"));
    }
    let actual = inventory(path, &worlds)?;
    let (files, bytes) = stats(&actual);
    if actual != m["inventory"]
        || m["files"].as_u64() != Some(files)
        || m["bytes"].as_u64() != Some(bytes)
    {
        return Err(error("備份逐檔 SHA-256 對帳不一致"));
    }
    Ok(m)
}

fn complete_backups(server: &Path, root: &Path, level: &str) -> Result<Vec<Value>> {
    let mut backups = Vec::new();
    if !root.exists() {
        return Ok(backups);
    }
    for entry in fs::read_dir(root).map_err(error)? {
        let entry = entry.map_err(error)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "_snapshots" || name.ends_with("_未完成") {
            continue;
        }
        if let Ok(m) = validate_backup(server, &entry.path(), level, "backup") {
            backups.push(json!({"name": name, "path": entry.path().to_string_lossy(),
                "created_at": m["created_at"], "files": m["files"], "bytes": m["bytes"]}));
        }
    }
    backups.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    Ok(backups)
}

/// 呼叫端須確認伺服器已關機。舊介面保留，預設保留十份完整備份。
pub fn backup_world(server_dir: &Path) -> Result<Value> {
    let root = server_dir.join("backups");
    let mut result = backup_world_to(server_dir, &root, 10)?;
    // 舊介面保留呼叫端原路徑拼法（Windows TEMP 可能使用 8.3 短路徑）。
    let path = root.join(result["name"].as_str().unwrap());
    result["path"] = json!(path.to_string_lossy());
    result["text"] = json!(format!(
        "備份完成：{} 個檔、{:.0} MB（對帳一致）\n位置：{}",
        result["files"].as_u64().unwrap(),
        result["bytes"].as_u64().unwrap() as f64 / 1_048_576.0,
        path.display()
    ));
    if let Some(warning) = result["retention_warning"].as_str() {
        result["text"] = json!(format!(
            "{}\n{warning}",
            result["text"].as_str().unwrap_or("")
        ));
    }
    Ok(result)
}

/// root 應由呼叫端按每台伺服器提供隔離目錄；未知檔案、失敗備份及快照不會清除。
pub fn backup_world_to(server_dir: &Path, root: &Path, keep: usize) -> Result<Value> {
    ensure_restore_safe(server_dir)?;
    if keep == 0 {
        return Err(error("至少保留一份備份"));
    }
    let (server, root, level) = locations(server_dir, root)?;
    let mut result = make_backup(&server, &root, &level, "backup", false)?;
    for old in complete_backups(&server, &root, &level)?
        .into_iter()
        .skip(keep)
    {
        let path = PathBuf::from(old["path"].as_str().unwrap());
        // 清除前再對帳，避免把不完整或後來被修改的資料當成一般備份刪掉。
        if validate_backup(&server, &path, &level, "backup").is_ok()
            && let Err(e) = fs::remove_dir_all(&path)
        {
            let warning = format!("備份完成，但舊備份清理失敗：{e}");
            result["text"] = json!(format!(
                "{}\n{warning}",
                result["text"].as_str().unwrap_or("")
            ));
            result["retention_warning"] = json!(warning);
        }
    }
    Ok(result)
}

pub fn list_backups(server_dir: &Path, root: &Path) -> Result<Value> {
    let (server, root, level) = locations(server_dir, root)?;
    Ok(json!({"ok": true, "backups": complete_backups(&server, &root, &level)?}))
}

/// 先驗證來源、快照現況、stage 對帳，再交換三個世界；交換出錯就按反序復原。
/// 快照與交易資料保留供人工復原，不列一般備份，也不參與保留清理。
pub fn restore_world(server_dir: &Path, root: &Path, name: &str) -> Result<Value> {
    restore_with_rename(server_dir, root, name, durable_rename)
}

fn restore_with_rename(
    server_dir: &Path,
    root: &Path,
    name: &str,
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<Value> {
    ensure_restore_safe(server_dir)?;
    if !safe_name(name) || name == "_snapshots" || name.ends_with("_未完成") {
        return Err(error("備份名稱無效"));
    }
    let (server, root, level) = locations(server_dir, root)?;
    let source = checked_path(&root.join(name))?;
    let manifest = validate_backup(&server, &source, &level, "backup")?;
    let worlds: Vec<String> = manifest["worlds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap().to_owned())
        .collect();
    let before_worlds = existing_worlds(&server, &level)?;
    let before = inventory(&server, &before_worlds)?;
    let snapshot_root = checked_path(&root.join("_snapshots"))?;
    let snapshot = make_backup(&server, &snapshot_root, &level, "snapshot", true)?;
    let snapshot_path = PathBuf::from(snapshot["path"].as_str().unwrap());
    let snapshot_manifest = validate_backup(&server, &snapshot_path, &level, "snapshot")?;
    if snapshot_manifest["inventory"] != before
        || existing_worlds(&server, &level)? != before_worlds
    {
        return Err(error("現況在建立快照時有變動，已保留快照，尚未交換世界"));
    }
    let transaction = server.join(format!(".mchub-restore-{}", unique()));
    fs::create_dir(&transaction).map_err(error)?;
    let stage = transaction.join("stage");
    let displaced = transaction.join("previous");
    fs::create_dir(&stage).map_err(error)?;
    fs::create_dir(&displaced).map_err(error)?;
    for world in &worlds {
        copy_tree(&source.join(world), &stage.join(world))?;
    }
    if inventory(&stage, &worlds)? != manifest["inventory"] {
        return Err(error("還原暫存逐檔 SHA-256 對帳不一致，尚未交換世界"));
    }
    validate_backup(&server, &source, &level, "backup")?;
    if inventory(&server, &before_worlds)? != before
        || existing_worlds(&server, &level)? != before_worlds
    {
        return Err(error("現有世界在準備期間有變動，尚未交換世界"));
    }
    sync_directory(&stage).map_err(error)?;
    sync_directory(&displaced).map_err(error)?;
    let mut moved = Vec::new();
    let mut installed = Vec::new();
    let mut journal = json!({"format": JOURNAL_FORMAT, "state": "swapping", "server": server.to_string_lossy(),
        "snapshot_path": snapshot_path.to_string_lossy(), "source": source.to_string_lossy(), "worlds_before": before_worlds, "worlds_after": worlds});
    write_journal(&transaction, &journal)?;
    sync_directory(&server).map_err(error)?;
    let swapped = (|| -> Result<()> {
        for world in world_names(&level) {
            let current = server.join(&world);
            if before_worlds.contains(&world) {
                metadata(&current)?;
                rename(&current, &displaced.join(&world)).map_err(error)?;
                moved.push(world.clone());
            }
            if worlds.contains(&world) {
                metadata(&stage.join(&world))?;
                rename(&stage.join(&world), &current).map_err(error)?;
                installed.push(world);
            }
        }
        if existing_worlds(&server, &level)? != worlds
            || inventory(&server, &worlds)? != manifest["inventory"]
        {
            return Err(error("交換後逐檔 SHA-256 對帳不一致"));
        }
        Ok(())
    })();
    if let Err(e) = swapped {
        let rollback = (|| -> Result<()> {
            for world in installed.iter().rev() {
                rename(&server.join(world), &stage.join(world)).map_err(error)?;
            }
            for world in moved.iter().rev() {
                rename(&displaced.join(world), &server.join(world)).map_err(error)?;
            }
            if existing_worlds(&server, &level)? != before_worlds
                || inventory(&server, &before_worlds)? != before
            {
                return Err(error("復原後對帳不一致"));
            }
            journal["state"] = json!("rolled_back");
            write_journal(&transaction, &journal)?;
            Ok(())
        })();
        return Err(UserError::new(match rollback {
            Ok(()) => format!(
                "還原失敗，原世界已復原：{e}。快照：{}",
                snapshot_path.display()
            ),
            Err(r) => format!(
                "還原失敗且自動復原未完成：{e}；{r}。請保留交易資料 {} 與快照 {}，人工復原。",
                transaction.display(),
                snapshot_path.display()
            ),
        }));
    }
    journal["state"] = json!("completed");
    write_journal(&transaction, &journal).map_err(|e| {
        UserError::new(format!(
            "世界已交換，但交易完成紀錄寫入失敗，已停止後續開機。交易資料：{}；快照：{}；{e}",
            transaction.display(),
            snapshot_path.display()
        ))
    })?;
    Ok(
        json!({"ok": true, "path": source.to_string_lossy(), "snapshot_path": snapshot_path.to_string_lossy(), "transaction_path": transaction.to_string_lossy(),
        "files": manifest["files"], "bytes": manifest["bytes"], "text": "還原完成（逐檔 SHA-256 對帳一致，已保留還原前快照）"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_safe::tests::temp;

    #[test]
    fn failed_second_world_install_rolls_back_every_moved_world() {
        let base = PathBuf::from(
            std::env::var_os("MCHUB_DATA_DIR").expect("isolated MCHUB_DATA_DIR required"),
        );
        assert!(fs::canonicalize(&base)
            .unwrap()
            .starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
        let server = base.join(format!("restore-rollback-{}", unique()));
        let root = server.join("backups");
        for world in ["world", "world_nether", "world_the_end"] {
            fs::create_dir_all(server.join(world)).unwrap();
            fs::write(server.join(world).join("level.dat"), b"original").unwrap();
        }
        let saved = backup_world_to(&server, &root, 10).unwrap();
        for world in ["world", "world_nether", "world_the_end"] {
            fs::write(server.join(world).join("level.dat"), b"modified").unwrap();
        }
        let mut injected = false;
        let result = restore_with_rename(
            &server,
            &root,
            saved["name"].as_str().unwrap(),
            |from, to| {
                if from.ends_with("stage/world_nether") {
                    injected = true;
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "injected second install failure",
                    ));
                }
                fs::rename(from, to)
            },
        );
        assert!(injected, "must exercise an actual mid-transaction failure");
        assert!(result.unwrap_err().msg.contains("原世界已復原"));
        for world in ["world", "world_nether", "world_the_end"] {
            assert_eq!(
                fs::read(server.join(world).join("level.dat")).unwrap(),
                b"modified"
            );
        }
        let snapshots: Vec<_> = fs::read_dir(root.join("_snapshots"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            fs::read(snapshots[0].join("world/level.dat")).unwrap(),
            b"modified"
        );
        assert_eq!(
            list_backups(&server, &root).unwrap()["backups"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let transactions: Vec<_> = fs::read_dir(&server)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".mchub-restore-")
            })
            .collect();
        let journal: Value =
            serde_json::from_slice(&fs::read(transactions[0].join("journal.json")).unwrap())
                .unwrap();
        assert_eq!(journal["state"], "rolled_back");
        assert!(ensure_restore_safe(&server).is_ok());
    }

    #[test]
    fn panic_mid_swap_leaves_durable_marker_and_blocks_new_operations() {
        let base = PathBuf::from(
            std::env::var_os("MCHUB_DATA_DIR").expect("isolated MCHUB_DATA_DIR required"),
        );
        assert!(fs::canonicalize(&base)
            .unwrap()
            .starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
        let server = base.join(format!("restore-interrupted-{}", unique()));
        let root = server.join("backups");
        for world in ["world", "world_nether"] {
            fs::create_dir_all(server.join(world)).unwrap();
            fs::write(server.join(world).join("level.dat"), b"original").unwrap();
        }
        let saved = backup_world_to(&server, &root, 10).unwrap();
        fs::write(server.join("world/level.dat"), b"modified").unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            restore_with_rename(
                &server,
                &root,
                saved["name"].as_str().unwrap(),
                |from, to| {
                    if from.ends_with("stage/world_nether") {
                        panic!("simulate process interruption after first world swap");
                    }
                    durable_rename(from, to)
                },
            )
        }));
        assert!(panic.is_err());
        assert_eq!(
            fs::read(server.join("world/level.dat")).unwrap(),
            b"original",
            "first world must already have swapped"
        );
        let blocked = ensure_restore_safe(&server).unwrap_err();
        assert!(blocked.msg.contains(".mchub-restore-"));
        assert!(blocked.msg.contains("_snapshots"));
        assert!(backup_world_to(&server, &root, 10).is_err());
        assert!(restore_world(&server, &root, saved["name"].as_str().unwrap()).is_err());
    }

    #[test]
    fn copies_all_three_worlds_and_counts() {
        let d = temp("backup");
        assert_eq!(
            backup_world(&d).unwrap_err().msg,
            "找不到世界資料夾「world」，這台可能還沒開過機。"
        );
        fs::write(d.join("server.properties"), "level-name=../我的 世界\n").unwrap();
        for (p, data) in [
            ("我的 世界/level.dat", "abc"),
            ("我的 世界/region/r.0.0.mca", "12345"),
            ("我的 世界_nether/DIM-1/x", "n"),
            ("其他/不要.txt", "x"),
        ] {
            fs::create_dir_all(d.join(p).parent().unwrap()).unwrap();
            fs::write(d.join(p), data).unwrap();
        }
        let r = backup_world(&d).unwrap();
        assert_eq!(
            (r["files"].as_u64(), r["bytes"].as_u64()),
            (Some(3), Some(9))
        );
        let dest = PathBuf::from(r["path"].as_str().unwrap());
        assert!(
            dest.starts_with(d.join("backups"))
                && dest
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("我的 世界_")
        );
        assert_eq!(
            fs::read_to_string(dest.join("我的 世界/region/r.0.0.mca")).unwrap(),
            "12345"
        );
        assert!(dest.join("我的 世界_nether/DIM-1/x").is_file() && !dest.join("其他").exists());
        let (a, b) = (
            fs::metadata(d.join("我的 世界/level.dat")).unwrap(),
            fs::metadata(dest.join("我的 世界/level.dat")).unwrap(),
        );
        assert_eq!(
            a.modified().unwrap(),
            b.modified().unwrap(),
            "要保留修改時間"
        );
        assert!(
            r["text"]
                .as_str()
                .unwrap()
                .starts_with("備份完成：3 個檔、0 MB（對帳一致）\n位置：")
        );
    }

    #[test]
    fn same_second_keeps_first_backup() {
        let d = temp("backup-twice");
        fs::create_dir_all(d.join("world")).unwrap();
        fs::write(d.join("world/level.dat"), "x").unwrap();
        let first = backup_world(&d).unwrap();
        let second = backup_world(&d);
        let path = PathBuf::from(first["path"].as_str().unwrap());
        assert!(
            path.join("world/level.dat").is_file(),
            "第一份不能被改名或弄壞"
        );
        if let Err(e) = second {
            assert_eq!(e.msg, "這一秒已經有一份備份了，請稍後再按一次。");
        }
    }
}
