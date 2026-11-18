use std::path::{Path, PathBuf};

pub fn hook(dir: PathBuf) {
    std::panic::set_hook(Box::new(move |_| {
        let _ = record(&dir, "panic");
    }));
}
pub fn session(dir: &Path, clean: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let marker = dir.join("session.json");
    if !clean
        && let Ok(bytes) = std::fs::read(&marker)
        && serde_json::from_slice::<serde_json::Value>(&bytes).ok().is_some_and(|v| v["clean"] == false)
    {
        record(dir, "unclean-exit")?;
    }
    mark(dir, clean)
}
fn mark(dir: &Path, clean: bool) -> std::io::Result<()> {
    mc_hub_core::fs_safe::write_atomic(
        &dir.join("session.json"),
        serde_json::json!({"clean":clean,"version":env!("CARGO_PKG_VERSION")}).to_string().as_bytes(),
    )
}
pub fn resume(dir: &Path) -> std::io::Result<()> {
    mark(dir, false)
}

/// 不記 panic 原文／路徑／堆疊，避免把玩家聊天、伺服器目錄或機敏值寫進可分享報告。
pub fn record(dir: &Path, kind: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = dir.join(format!("crash-{stamp}.json"));
    let v = serde_json::json!({"app_version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"kind":kind,"timestamp":stamp.to_string()});
    mc_hub_core::fs_safe::write_atomic(&path, serde_json::to_string_pretty(&v)?.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn report_contains_only_safe_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "mchub-report-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let p = super::record(&dir, "panic").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
        assert_eq!(v["kind"], "panic");
        assert_eq!(v.as_object().unwrap().len(), 5);
        super::session(&dir, false).unwrap();
        super::session(&dir, true).unwrap();
        super::session(&dir, false).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        super::resume(&dir).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        super::session(&dir, false).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
    }
}
