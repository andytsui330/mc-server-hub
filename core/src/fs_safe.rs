//! 安全檔案操作：先寫暫存再替換（寫到一半斷電不會留下半個檔）、路徑不准跑出指定資料夾、不跟隨捷徑連結。
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

/// 先寫到同資料夾的 .tmp、存到硬碟後再一次換掉正本。
pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut name = path.file_name().ok_or_else(|| io::Error::other("沒有檔名"))?.to_os_string();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let result = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// root 底下的 rel：不准絕對路徑、`..`，沿路已存在的每一層都不能是捷徑連結（含 Windows 連接點）。
pub fn child(root: &Path, rel: &Path) -> crate::Result<PathBuf> {
    let bad = || crate::UserError::new(format!("路徑「{}」超出伺服器資料夾，已停止。", rel.display()));
    if rel.as_os_str().is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(bad());
    }
    let mut path = root.to_path_buf();
    for part in rel.components() {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => return Err(bad()),
            Ok(_) => {}
            Err(_) => break, // 還不存在的部分不可能是連結
        }
    }
    Ok(root.join(rel))
}

/// 新伺服器的資料夾名稱：1–40 字、沒有 Windows 不准的符號、不以句點或空白結尾、不是 CON／COM1 這類保留名。
pub fn check_folder_name(name: &str) -> Option<&'static str> {
    let len = name.chars().count();
    if len == 0 || len > 40 {
        return Some("伺服器名稱要 1 到 40 個字");
    }
    let stem = name.split('.').next().unwrap_or("").to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if name.chars().any(|c| r#"<>:"/\|?*"#.contains(c) || c < ' ') || name.ends_with(['.', ' ']) || reserved {
        return Some(r#"伺服器名稱不能有 < > : " / \ | ? * 這些符號，也不能以句點或空白結尾"#);
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mchub-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn atomic_replaces_and_leaves_no_tmp() {
        let d = temp("atomic");
        let f = d.join("a.json");
        write_atomic(&f, b"one").unwrap();
        write_atomic(&f, b"two").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"two");
        assert!(!d.join("a.json.tmp").exists());
        assert!(write_atomic(&d.join("no/such/dir/x"), b"x").is_err());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn child_stays_inside() {
        let d = temp("child");
        assert_eq!(child(&d, Path::new("world/level.dat")).unwrap(), d.join("world/level.dat"));
        for bad in ["../x", "a/../../x", "", "/etc/passwd", "C:\\Windows"] {
            if bad.contains(':') && !cfg!(windows) {
                continue;
            }
            assert!(child(&d, Path::new(bad)).is_err(), "{bad} 應該被擋");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/", d.join("link")).unwrap();
            assert!(child(&d, Path::new("link/etc")).is_err());
        }
        #[cfg(windows)]
        {
            // 一般帳號不能建符號連結，用不需權限的「連接點」測
            let ok = std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"]).arg(d.join("link")).arg(std::env::temp_dir())
                .output().unwrap().status.success();
            assert!(ok);
            assert!(child(&d, Path::new("link/x")).is_err());
            fs::remove_dir(d.join("link")).unwrap();
        }
        fs::remove_dir_all(d).unwrap();
    }
}
