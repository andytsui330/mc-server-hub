use mc_hub_core::hub::Hub;
use serde_json::json;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
#[test]
fn player_files_reject_parent_junctions() {
    let root = std::env::temp_dir().join(format!(
        "mchub-p1-player-link-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let real = root.join("actual");
    fs::create_dir_all(&real).unwrap();
    let link = root.join("linked");
    assert!(
        std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .output()
            .unwrap()
            .status
            .success()
    );
    let r = mc_hub_core::players::change(
        &link,
        &json!({"action":"op","name":"Andy","uuid":"12345678-1234-1234-1234-123456789abc","confirm":true}),
    );
    assert!(r.is_err());
    assert!(!real.join("ops.json").exists());
}

#[test]
fn offline_management_backs_up_preserves_and_validates() {
    let root = std::env::temp_dir().join(format!(
        "mchub-p1-players-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let srv = root.join("server");
    fs::create_dir_all(&srv).unwrap();
    fs::write(srv.join("start.bat"), "java -jar server.jar nogui\n").unwrap();
    fs::write(srv.join("start.sh"), "java -jar server.jar nogui\n").unwrap();
    fs::write(srv.join("server.properties"), "server-port=48998\n").unwrap();
    let hub = Hub::new(&root.join("data")).unwrap();
    let id = hub.call("/api/add", &json!({"dir":srv}))["id"].clone();
    let uuid = "12345678-1234-1234-1234-123456789abc";
    let original = format!(
        r#"[{{"uuid":"{uuid}","name":"Andy","level":2,"bypassesPlayerLimit":true,"future":42}}]"#
    );
    fs::write(srv.join("ops.json"), &original).unwrap();
    let list = hub.call("/api/players", &json!({"id":id}));
    assert_eq!(list["ops"][0]["name"], "Andy");
    let action = |action: &str, name: &str| {
        hub.call(
            "/api/players/action",
            &json!({"id":id,"action":action,"name":name,"uuid":uuid,"confirm":true}),
        )
    };
    assert_eq!(action("deop", "Andy")["ok"], true);
    assert_eq!(
        fs::read_to_string(srv.join("ops.json.bak")).unwrap(),
        original
    );
    assert_eq!(action("whitelist-add", "Andy")["ok"], true);
    assert_eq!(action("ban", "Andy")["ok"], true);
    assert_eq!(action("pardon", "Andy")["ok"], true);
    assert!(action("op", "@a")["error"].is_string());
    assert!(action("op", "Andy\nstop")["error"].is_string());
    assert!(action("kick", "Andy")["error"].is_string());
    fs::write(srv.join("ops.json"), "{broken").unwrap();
    assert!(action("op", "Andy")["error"].is_string());
    assert_eq!(fs::read_to_string(srv.join("ops.json")).unwrap(), "{broken");
    assert!(hub.begin_update().is_ok());
    assert!(action("op", "Andy")["error"].is_string());
    hub.finish_update();
}
