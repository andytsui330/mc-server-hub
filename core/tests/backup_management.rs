use mc_hub_core::backup::{
    backup_world, backup_world_to, ensure_restore_safe, list_backups, restore_world,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
fn fixture() -> PathBuf {
    let base = PathBuf::from(
        std::env::var_os("MCHUB_DATA_DIR").expect("tests require isolated MCHUB_DATA_DIR"),
    );
    assert!(fs::canonicalize(&base)
        .unwrap()
        .starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
    let dir = base.join(format!(
        "backup-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(dir.join("world")).unwrap();
    fs::write(dir.join("world/level.dat"), b"original").unwrap();
    dir
}

#[test]
fn completed_backup_carries_verifiable_platform_manifest() {
    let dir = fixture();
    let result = backup_world(&dir).unwrap();
    let path = PathBuf::from(result["path"].as_str().unwrap());
    assert!(
        path.join("manifest.json").is_file(),
        "a completed backup must have a verifiable manifest"
    );
}

fn backup_name(value: &serde_json::Value) -> String {
    PathBuf::from(value["path"].as_str().unwrap())
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned()
}

#[test]
fn interrupted_or_unknown_restore_journal_blocks_start_backup_and_restore() {
    for state in [
        "swapping",
        "future-unknown",
        "bad-json",
        "unfinished-temp",
        "completed-missing-fields",
    ] {
        let dir = fixture();
        let root = dir.join("backups");
        let saved = backup_world_to(&dir, &root, 10).unwrap();
        let transaction = dir.join(".mchub-restore-interrupted");
        fs::create_dir_all(transaction.join("previous/world")).unwrap();
        let actual_manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(PathBuf::from(saved["path"].as_str().unwrap()).join("manifest.json"))
                .unwrap(),
        )
        .unwrap();
        let marker = if state == "bad-json" {
            b"bad json".to_vec()
        } else {
            serde_json::to_vec(&serde_json::json!({"format":"mc-hub-restore-v1", "state":if state == "completed-missing-fields" {"completed"} else {state},
                "server":actual_manifest["server"],"snapshot_path":root.join("_snapshots/recovery").to_string_lossy()})).unwrap()
        };
        let file = if state == "unfinished-temp" {
            "journal.json.tmp"
        } else {
            "journal.json"
        };
        fs::write(transaction.join(file), marker).unwrap();
        let error = ensure_restore_safe(&dir).unwrap_err();
        assert!(error.msg.contains(".mchub-restore-interrupted"));
        assert!(error.msg.contains("快照"));
        assert!(backup_world_to(&dir, &root, 10).is_err());
        assert!(restore_world(&dir, &root, &backup_name(&saved)).is_err());
        assert_eq!(fs::read(dir.join("world/level.dat")).unwrap(), b"original");
    }
}

#[test]
fn missing_journal_with_displaced_worlds_blocks_but_pre_stage_does_not() {
    let dir = fixture();
    let transaction = dir.join(".mchub-restore-preparing");
    fs::create_dir_all(transaction.join("stage/world")).unwrap();
    fs::create_dir(transaction.join("previous")).unwrap();
    assert!(ensure_restore_safe(&dir).is_ok());
    fs::create_dir(transaction.join("previous/world")).unwrap();
    assert!(ensure_restore_safe(&dir).is_err());
}

#[cfg(windows)]
#[test]
fn retention_failure_remains_visible_in_the_user_message() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = fixture();
    let root = dir.join("backups");
    let old = backup_world_to(&dir, &root, 1).unwrap();
    let file = PathBuf::from(old["path"].as_str().unwrap()).join("world/level.dat");
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&file)
        .unwrap();
    let saved = backup_world_to(&dir, &root, 1).unwrap();
    drop(lock);
    let warning = saved["retention_warning"]
        .as_str()
        .expect("locked old backup must report retention failure");
    assert!(saved["text"].as_str().unwrap().contains(warning));
    assert!(
        PathBuf::from(saved["path"].as_str().unwrap())
            .join("world/level.dat")
            .is_file()
    );
}

#[test]
fn custom_root_lists_complete_backups_and_retains_only_known_complete_ones() {
    let dir = fixture();
    let root = dir.join("custom-backups");
    fs::create_dir_all(root.join("unknown-old/world")).unwrap();
    fs::write(root.join("unknown-old/world/level.dat"), b"keep unknown").unwrap();
    fs::create_dir_all(root.join("broken")).unwrap();
    fs::write(root.join("broken/manifest.json"), b"not json").unwrap();
    let first = backup_world_to(&dir, &root, 2).unwrap();
    let second = backup_world_to(&dir, &root, 2).unwrap();
    let third = backup_world_to(&dir, &root, 2).unwrap();
    let items = list_backups(&dir, &root).unwrap();
    let items = items["backups"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["name"], backup_name(&third));
    assert_eq!(items[1]["name"], backup_name(&second));
    assert_eq!(items[0]["files"], 1);
    assert_eq!(items[0]["bytes"], 8);
    assert!(!PathBuf::from(first["path"].as_str().unwrap()).exists());
    assert!(root.join("unknown-old/world/level.dat").exists());
    assert!(root.join("broken/manifest.json").exists());
}

#[test]
fn legacy_entry_point_defaults_to_ten_backups() {
    let dir = fixture();
    let first = backup_world(&dir).unwrap();
    for _ in 0..10 {
        backup_world(&dir).unwrap();
    }
    assert_eq!(
        list_backups(&dir, &dir.join("backups")).unwrap()["backups"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    assert!(!PathBuf::from(first["path"].as_str().unwrap()).exists());
}

#[test]
fn restore_replaces_all_three_worlds_and_preserves_a_separate_snapshot() {
    let dir = fixture();
    let root = dir.join("backups");
    for world in ["world_nether", "world_the_end"] {
        fs::create_dir_all(dir.join(world).join("empty")).unwrap();
        fs::write(dir.join(world).join("level.dat"), world.as_bytes()).unwrap();
    }
    let saved = backup_world_to(&dir, &root, 1).unwrap();
    fs::write(dir.join("world/level.dat"), b"modified").unwrap();
    fs::write(dir.join("world/extra.dat"), b"extra").unwrap();
    fs::write(dir.join("world_nether/level.dat"), b"now nether").unwrap();
    let restored = restore_world(&dir, &root, &backup_name(&saved)).unwrap();
    let transaction = PathBuf::from(restored["transaction_path"].as_str().unwrap());
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(transaction.join("journal.json")).unwrap()).unwrap();
    assert_eq!(journal["state"], "completed");
    assert!(ensure_restore_safe(&dir).is_ok());
    assert_eq!(fs::read(dir.join("world/level.dat")).unwrap(), b"original");
    assert_eq!(
        fs::read(dir.join("world_nether/level.dat")).unwrap(),
        b"world_nether"
    );
    assert_eq!(
        fs::read(dir.join("world_the_end/level.dat")).unwrap(),
        b"world_the_end"
    );
    assert!(dir.join("world_the_end/empty").is_dir());
    assert!(!dir.join("world/extra.dat").exists());
    let snapshot = PathBuf::from(restored["snapshot_path"].as_str().unwrap());
    assert_eq!(
        fs::read(snapshot.join("world/level.dat")).unwrap(),
        b"modified"
    );
    assert_eq!(
        fs::read(snapshot.join("world_nether/level.dat")).unwrap(),
        b"now nether"
    );
    backup_world_to(&dir, &root, 1).unwrap();
    assert_eq!(
        list_backups(&dir, &root).unwrap()["backups"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        snapshot.join("world/extra.dat").is_file(),
        "retention must preserve snapshots"
    );
}

#[test]
fn restore_removes_worlds_absent_from_backup_without_touching_unrelated_folders() {
    let dir = fixture();
    let root = dir.join("backups");
    let saved = backup_world_to(&dir, &root, 10).unwrap();
    for name in ["world_nether", "world_the_end", "another_world"] {
        fs::create_dir_all(dir.join(name)).unwrap();
        fs::write(dir.join(name).join("level.dat"), b"new").unwrap();
    }
    let restored = restore_world(&dir, &root, &backup_name(&saved)).unwrap();
    assert!(!dir.join("world_nether").exists());
    assert!(!dir.join("world_the_end").exists());
    assert!(dir.join("another_world/level.dat").is_file());
    assert!(
        PathBuf::from(restored["snapshot_path"].as_str().unwrap())
            .join("world_nether/level.dat")
            .is_file()
    );
}

#[test]
fn rejects_corrupted_same_size_content_unknown_manifests_and_cross_server_restore() {
    let dir = fixture();
    let root = dir.join("backups");
    let saved = backup_world_to(&dir, &root, 10).unwrap();
    let path = PathBuf::from(saved["path"].as_str().unwrap());
    let other = fixture();
    assert!(restore_world(&other, &root, &backup_name(&saved)).is_err());
    fs::write(path.join("world/level.dat"), b"tampered").unwrap();
    assert!(restore_world(&dir, &root, &backup_name(&saved)).is_err());
    assert_eq!(
        list_backups(&dir, &root).unwrap()["backups"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(fs::read(dir.join("world/level.dat")).unwrap(), b"original");
    for (name, manifest) in [("unknown", "{}"), ("malformed", "bad json")] {
        fs::create_dir(root.join(name)).unwrap();
        fs::write(root.join(name).join("manifest.json"), manifest).unwrap();
        assert!(restore_world(&dir, &root, name).is_err());
    }
}

#[test]
fn rejects_unsafe_paths_zero_retention_and_manifest_world_escape() {
    let dir = fixture();
    for root in [
        dir.join("world/backups"),
        dir.join("world_nether/backups"),
        dir.join("world/../outside"),
    ] {
        assert!(backup_world_to(&dir, &root, 10).is_err());
    }
    assert!(backup_world_to(&dir, &dir.join("backups"), 0).is_err());
    let root = dir.join("backups");
    let saved = backup_world_to(&dir, &root, 10).unwrap();
    for name in ["../world", "", ".", "..", "C:\\Windows", "a/b", "a\\b"] {
        assert!(restore_world(&dir, &root, name).is_err());
    }
    let path = PathBuf::from(saved["path"].as_str().unwrap());
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
    manifest["worlds"] = serde_json::json!(["../outside"]);
    fs::write(
        path.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(restore_world(&dir, &root, &backup_name(&saved)).is_err());
    assert_eq!(fs::read(dir.join("world/level.dat")).unwrap(), b"original");
}

#[cfg(windows)]
fn junction(link: &std::path::Path, target: &std::path::Path) {
    let output = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction {} -> {}: {:?}",
        link.display(),
        target.display(),
        output
    );
}
#[cfg(unix)]
fn junction(link: &std::path::Path, target: &std::path::Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[test]
fn rejects_links_in_worlds_backup_root_and_restore_source() {
    let dir = fixture();
    let external = fixture();
    let root = dir.join("backups");
    let saved = backup_world_to(&dir, &root, 10).unwrap();
    junction(&dir.join("world/link"), &external);
    assert!(backup_world_to(&dir, &root, 10).is_err());
    assert!(
        restore_world(&dir, &root, &backup_name(&saved)).is_err(),
        "snapshot must refuse current linked worlds"
    );
    junction(&dir.join("linked-root"), &root);
    assert!(list_backups(&dir, &dir.join("linked-root")).is_err());
    let clean = fixture();
    let clean_root = clean.join("backups");
    let saved = backup_world_to(&clean, &clean_root, 10).unwrap();
    junction(
        &PathBuf::from(saved["path"].as_str().unwrap()).join("world/escape"),
        &external,
    );
    assert!(restore_world(&clean, &clean_root, &backup_name(&saved)).is_err());
    let top = fixture();
    junction(&top.join("world_nether"), &external);
    assert!(backup_world_to(&top, &top.join("backups"), 10).is_err());
}
