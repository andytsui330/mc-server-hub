use mc_hub_core::{
    Result, UserError,
    mods::{self, Modrinth, Target},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn temp() -> PathBuf {
    // macOS 時鐘只到微秒，平行測試可能拿到同一個時間；加流水號避免撞名。
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let base = std::env::var_os("MCHUB_DATA_DIR").expect("tests require isolated MCHUB_DATA_DIR");
    let dir = PathBuf::from(base).join(format!(
        "mods-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
fn target() -> Target {
    Target {
        minecraft: "1.21.1".into(),
        loader: "fabric".into(),
    }
}
fn project(id: &str) -> Value {
    json!({"id":id,"title":id,"project_type":"mod","server_side":"required","client_side":"optional"})
}
fn version(project: &str, id: &str, file: &str) -> Value {
    json!({"id":id,"project_id":project,"name":id,"version_number":id,"version_type":"release","date_published":"2026-01-01T00:00:00Z","game_versions":["1.21.1"],"loaders":["fabric"],"dependencies":[],"files":[{"filename":file,"url":format!("https://cdn.modrinth.com/data/{project}/versions/{id}/{file}"),"primary":true,"size":3,"hashes":{"sha1":"a9993e364706816aba3e25717850c26c9cd0d89d","sha512":"unused"}}]})
}
fn fixtures(versions: Vec<Value>) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    for v in versions {
        let pid = v["project_id"].as_str().unwrap();
        map.insert(format!("/project/{pid}"), project(pid));
        map.insert(format!("/version/{}", v["id"].as_str().unwrap()), v.clone());
        let list = map
            .entry(format!("/project/{pid}/version"))
            .or_insert(json!([]));
        list.as_array_mut().unwrap().push(v);
    }
    map
}
fn client(map: HashMap<String, Value>) -> Modrinth<impl FnMut(&str) -> Result<Value>> {
    Modrinth::new(move |url| {
        let key = url
            .strip_prefix("https://api.modrinth.com/v2")
            .expect("only API origin")
            .split('?')
            .next()
            .unwrap();
        map.get(key)
            .cloned()
            .ok_or_else(|| UserError::new(format!("missing fixture {url}")))
    })
}
fn download(_: &str, dest: &Path, _: &mc_hub_core::net::Checksum, _: u64) -> Result<()> {
    fs::write(dest, b"abc").map_err(|e| UserError::new(e.to_string()))
}

#[test]
fn plans_required_dependencies_then_installs_and_lists_unknown_without_touching_it() {
    let dir = temp();
    fs::create_dir(dir.join("mods")).unwrap();
    fs::write(dir.join("mods/private.jar"), b"keep").unwrap();
    let mut root = version("root", "r1", "root.jar");
    root["dependencies"] = json!([
        {"project_id":"dep","version_id":null,"file_name":null,"dependency_type":"required"},
        {"project_id":"skip","dependency_type":"optional"}
    ]);
    let mut api = client(fixtures(vec![root, version("dep", "d1", "dep.jar")]));
    let plan = api.plan_install(&target(), "root", &dir).unwrap();
    assert!(
        !dir.join("mods/root.jar").exists(),
        "planning must never install"
    );
    let view = plan.to_json();
    assert_eq!(view["files"].as_array().unwrap().len(), 2);
    assert_eq!(view["files"][0]["project_id"], "dep");
    assert_eq!(view["files"][0]["size"], 3);
    assert_eq!(
        view["files"][0]["sha1"],
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    mods::apply_plan(&dir, &plan, download).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(
            &fs::read(dir.join(".mchub-mod-transaction.json")).unwrap()
        )
        .unwrap()["phase"],
        "complete"
    );
    assert_eq!(fs::read(dir.join("mods/root.jar")).unwrap(), b"abc");
    assert_eq!(fs::read(dir.join("mods/private.jar")).unwrap(), b"keep");
    let installed = mods::list_installed(&dir).unwrap();
    assert_eq!(installed["managed"].as_array().unwrap().len(), 2);
    assert_eq!(installed["unknown"], json!(["private.jar"]));
}

#[test]
fn selection_filters_exact_target_and_prefers_release_and_rejects_client_only() {
    let dir = temp();
    let release = version("root", "r1", "release.jar");
    let mut beta = version("root", "b2", "beta.jar");
    beta["version_type"] = json!("beta");
    beta["date_published"] = json!("2026-02-01T00:00:00Z");
    let mut wrong = version("root", "r3", "wrong.jar");
    wrong["game_versions"] = json!(["1.21.2"]);
    let mut api = client(fixtures(vec![wrong, beta, release]));
    assert_eq!(
        api.plan_install(&target(), "root", &dir).unwrap().to_json()["files"][0]["version_id"],
        "r1"
    );
    let mut map = fixtures(vec![version("root", "r1", "root.jar")]);
    map.get_mut("/project/root").unwrap()["server_side"] = json!("unsupported");
    assert!(client(map).plan_install(&target(), "root", &dir).is_err());
    let vanilla = Target {
        loader: "vanilla".into(),
        ..target()
    };
    assert!(api.plan_install(&vanilla, "root", &dir).is_err());
}

#[test]
fn rejects_cycles_external_dependencies_conflicting_versions_and_unsafe_files() {
    let dir = temp();
    for kind in [
        "cycle",
        "external",
        "incompatible",
        "conflict",
        "traversal",
        "url",
        "wrong_loader",
    ] {
        let mut root = version("root", "r1", "root.jar");
        let mut dep = version("dep", "d1", "dep.jar");
        root["dependencies"] =
            json!([{"project_id":"dep","version_id":"d1","dependency_type":"required"}]);
        match kind {
            "cycle" => {
                dep["dependencies"] = json!([{"project_id":"root","dependency_type":"required"}])
            }
            "external" => {
                root["dependencies"] =
                    json!([{"file_name":"external.jar","dependency_type":"required"}])
            }
            "incompatible" => {
                dep["dependencies"] =
                    json!([{"project_id":"root","dependency_type":"incompatible"}])
            }
            "conflict" => {
                root["dependencies"] = json!([{"project_id":"dep","version_id":"d1","dependency_type":"required"},{"project_id":"dep","version_id":"d2","dependency_type":"required"}])
            }
            "traversal" => dep["files"][0]["filename"] = json!("../outside.jar"),
            "url" => dep["files"][0]["url"] = json!("https://cdn.modrinth.com.evil.test/mod.jar"),
            "wrong_loader" => dep["loaders"] = json!(["forge"]),
            _ => unreachable!(),
        }
        let mut api = client(fixtures(vec![root, dep, version("dep", "d2", "dep2.jar")]));
        assert!(
            api.plan_install(&target(), "root", &dir).is_err(),
            "must reject {kind}"
        );
    }
}

#[test]
fn update_keeps_old_jar_in_backup_and_stale_plan_cannot_overwrite_local_edits() {
    let dir = temp();
    let mut api = client(fixtures(vec![version("root", "r1", "old.jar")]));
    let plan = api.plan_install(&target(), "root", &dir).unwrap();
    mods::apply_plan(&dir, &plan, download).unwrap();
    let mut v2 = version("root", "r2", "new.jar");
    v2["date_published"] = json!("2026-02-01T00:00:00Z");
    let mut api = client(fixtures(vec![version("root", "r1", "old.jar"), v2]));
    assert_eq!(
        api.check_updates(&target(), &dir).unwrap()[0]["version_id"],
        "r2"
    );
    let update = api.plan_updates(&target(), &["root".into()], &dir).unwrap();
    fs::write(dir.join("mods/old.jar"), b"edited").unwrap();
    assert!(mods::apply_plan(&dir, &update, download).is_err());
    assert_eq!(fs::read(dir.join("mods/old.jar")).unwrap(), b"edited");
    fs::write(dir.join("mods/old.jar"), b"abc").unwrap();
    let result = mods::apply_plan(&dir, &update, download).unwrap();
    assert_eq!(fs::read(dir.join("mods/new.jar")).unwrap(), b"abc");
    assert!(!dir.join("mods/old.jar").exists());
    let backup = dir.join(result["backup"].as_str().unwrap());
    assert_eq!(fs::read(backup.join("old.jar")).unwrap(), b"abc");
    assert_eq!(
        mods::list_installed(&dir).unwrap()["managed"][0]["version_id"],
        "r2"
    );
    assert!(
        mods::apply_plan(&dir, &update, download).is_err(),
        "manifest changed since plan"
    );
}

#[test]
fn bad_download_hash_or_unknown_filename_collision_preserves_every_existing_file() {
    let dir = temp();
    let mut api = client(fixtures(vec![version("root", "r1", "root.jar")]));
    let plan = api.plan_install(&target(), "root", &dir).unwrap();
    assert!(
        mods::apply_plan(&dir, &plan, |_, p, _, _| {
            fs::write(p, b"bad").unwrap();
            Ok(())
        })
        .is_err()
    );
    assert!(!dir.join("mods/root.jar").exists());
    fs::write(dir.join("mods/root.jar"), b"private").unwrap();
    assert!(api.plan_install(&target(), "root", &dir).is_err());
    assert!(mods::apply_plan(&dir, &plan, download).is_err());
    assert_eq!(fs::read(dir.join("mods/root.jar")).unwrap(), b"private");
    assert!(
        mods::list_installed(&dir).unwrap()["managed"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn search_encodes_query_and_filters_server_compatibility() {
    let mut api = Modrinth::new(|url: &str| {
        assert!(url.starts_with("https://api.modrinth.com/v2/search?"));
        assert!(url.contains("query=a%26b%20%E4%B8%AD"));
        assert!(url.contains("facets="));
        Ok(
            json!({"hits":[{"project_id":"a","title":"A","description":"ok","server_side":"optional","project_type":"mod"},{"project_id":"b","server_side":"unsupported","project_type":"mod"}]}),
        )
    });
    let hits = api.search(&target(), "a&b 中").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["project_id"], "a");
}

#[test]
fn version_only_dependency_installs_and_existing_reverse_dependency_blocks_incompatible_update() {
    let dir = temp();
    let mut root = version("root", "r1", "root.jar");
    root["dependencies"] = json!([{"project_id":null,"version_id":"d1","file_name":null,"dependency_type":"required"}]);
    let mut api = client(fixtures(vec![root, version("dep", "d1", "dep.jar")]));
    let plan = api.plan_install(&target(), "root", &dir).unwrap();
    mods::apply_plan(&dir, &plan, download).unwrap();
    let mut next = version("dep", "d2", "dep2.jar");
    next["date_published"] = json!("2026-02-01T00:00:00Z");
    let mut api = client(fixtures(vec![next]));
    assert!(
        api.plan_updates(&target(), &["dep".into()], &dir).is_err(),
        "root still requires exact d1"
    );
    assert_eq!(fs::read(dir.join("mods/dep.jar")).unwrap(), b"abc");
}

#[test]
fn rejects_api_project_identity_mismatch_instead_of_installing_another_project() {
    let dir = temp();
    let mut data = fixtures(vec![version("root", "r1", "root.jar")]);
    data.insert("/project/root".into(), project("other"));
    data.insert(
        "/project/other/version".into(),
        json!([version("other", "o1", "other.jar")]),
    );
    data.insert("/project/other".into(), project("other"));
    assert!(client(data).plan_install(&target(), "root", &dir).is_err());
}

#[test]
fn external_incompatible_file_requires_manual_review_even_when_project_is_not_installed() {
    let dir = temp();
    let mut root = version("root", "r1", "root.jar");
    root["dependencies"] = json!([{"file_name":"external.jar","project_id":"external","dependency_type":"incompatible"}]);
    assert!(
        client(fixtures(vec![root]))
            .plan_install(&target(), "root", &dir)
            .is_err()
    );
}

#[test]
fn interrupted_or_unknown_durable_transaction_blocks_plan_apply_list_and_start_gate() {
    let dir = temp();
    let mut api = client(fixtures(vec![version("root", "r1", "root.jar")]));
    let plan = api.plan_install(&target(), "root", &dir).unwrap();
    for phase in ["swapping", "unexpected"] {
        fs::write(
            dir.join(".mchub-mod-transaction.json"),
            json!({"schema":1,"phase":phase,"backup":".mchub-mod-backups/interrupted"}).to_string(),
        )
        .unwrap();
        assert!(mods::ensure_safe(&dir).is_err());
        assert!(mods::list_installed(&dir).is_err());
        assert!(api.plan_install(&target(), "root", &dir).is_err());
        assert!(mods::apply_plan(&dir, &plan, download).is_err());
    }
    assert!(!dir.join("mods/root.jar").exists());
}

#[test]
fn completed_snapshot_rejects_manifest_corruption_and_panic_left_swapping_marker() {
    let dir = temp();
    let mut api = client(fixtures(vec![version("root", "r1", "root.jar")]));
    let plan = api.plan_install(&target(), "root", &dir).unwrap();
    mods::apply_plan(&dir, &plan, download).unwrap();
    fs::write(dir.join(".mchub-mods.json"), b"{}").unwrap();
    assert!(
        mods::ensure_safe(&dir).is_err(),
        "complete marker cannot hide damaged manifest"
    );
    let _ = std::panic::catch_unwind(|| {
        fs::write(
            dir.join(".mchub-mod-transaction.json"),
            json!({"schema":1,"phase":"swapping","backup":".mchub-mod-backups/interrupted"})
                .to_string(),
        )
        .unwrap();
        panic!("simulate process stopping during swapping");
    });
    assert!(mods::ensure_safe(&dir).is_err());
    assert_eq!(fs::read(dir.join("mods/root.jar")).unwrap(), b"abc");
}

#[cfg(windows)]
#[test]
fn locked_staged_jar_rolls_back_already_replaced_jars_and_manifest() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = temp();
    let mut root = version("root", "r1", "root.jar");
    root["dependencies"] = json!([{"project_id":"dep","dependency_type":"required"}]);
    let mut api = client(fixtures(vec![
        root.clone(),
        version("dep", "d1", "dep.jar"),
    ]));
    mods::apply_plan(
        &dir,
        &api.plan_install(&target(), "root", &dir).unwrap(),
        download,
    )
    .unwrap();
    let old_manifest = fs::read(dir.join(".mchub-mods.json")).unwrap();
    root["id"] = json!("r2");
    root["files"][0]["filename"] = json!("root2.jar");
    root["date_published"] = json!("2026-02-01T00:00:00Z");
    let mut api = client(fixtures(vec![root, version("dep", "d1", "dep.jar")]));
    let plan = api.plan_updates(&target(), &["root".into()], &dir).unwrap();
    let mut lock = None;
    let outcome = mods::apply_plan(&dir, &plan, |u, p, h, s| {
        download(u, p, h, s)?;
        if p.file_name().unwrap() == "root2.jar" {
            lock = Some(
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(3)
                    .open(p)
                    .unwrap(),
            );
        }
        Ok(())
    });
    assert!(outcome.is_err());
    assert_eq!(fs::read(dir.join("mods/root.jar")).unwrap(), b"abc");
    assert_eq!(fs::read(dir.join("mods/dep.jar")).unwrap(), b"abc");
    assert!(!dir.join("mods/root2.jar").exists());
    assert_eq!(
        fs::read(dir.join(".mchub-mods.json")).unwrap(),
        old_manifest
    );
    assert_eq!(
        serde_json::from_slice::<Value>(
            &fs::read(dir.join(".mchub-mod-transaction.json")).unwrap()
        )
        .unwrap()["phase"],
        "rolled_back"
    );
    mods::ensure_safe(&dir).unwrap();
    drop(lock);
}
