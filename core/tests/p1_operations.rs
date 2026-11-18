use mc_hub_core::{hub::Hub, server::Server};
use serde_json::json;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn changed_port_is_rechecked_before_offline_mutations() {
    let d = std::env::temp_dir().join(format!(
        "mchub-p1-port-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let srv = d.join("srv");
    fs::create_dir_all(srv.join("world")).unwrap();
    fs::write(srv.join("world/level.dat"), "original").unwrap();
    fs::write(srv.join("start.bat"), "java -jar server.jar\n").unwrap();
    fs::write(srv.join("start.sh"), "java -jar server.jar\n").unwrap();
    let old = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let old_port = old.local_addr().unwrap().port();
    drop(old);
    fs::write(
        srv.join("server.properties"),
        format!("server-port={old_port}\n"),
    )
    .unwrap();
    let h = Hub::new(&d.join("data")).unwrap();
    let id = h.call("/api/add", &json!({"dir":srv}))["id"].clone();
    let new = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = new.local_addr().unwrap().port();
    drop(new);
    let props = h.call("/api/properties", &json!({"id":id}));
    assert!(h.call("/api/properties",&json!({"id":id,"expected_revision":props["revision"],"values":{"server-port":port.to_string()}}))["error"].is_null());
    let _occupied = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    assert!(h.call("/api/players/action",&json!({"id":id,"action":"op","name":"Andy","uuid":"12345678-1234-1234-1234-123456789abc","confirm":true}))["error"].is_string());
    assert!(!srv.join("ops.json").exists());
    assert!(h.call("/api/backup", &json!({"id":id}))["error"].is_string());
    assert!(
        h.call("/api/restore", &json!({"id":id,"name":"x","confirm":true}))["error"].is_string()
    );
}

#[test]
fn effective_properties_are_shared_by_backup_and_port_checks() {
    let d = std::env::temp_dir().join(format!(
        "mchub-p1-prop-read-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&d).unwrap();
    fs::write(
        d.join("server.properties"),
        "level-name=\\u670B\\u53CB\\u4E16\\u754C\nserver-port=25565\nserver-port : 27777\n",
    )
    .unwrap();
    assert_eq!(
        mc_hub_core::server::launch::read_prop(&d, "level-name").as_deref(),
        Some("朋友世界")
    );
    assert_eq!(mc_hub_core::server::launch::read_port(&d, 25565), 27777);
}

#[test]
fn legacy_non_utf8_properties_keep_their_port_without_being_rewritten() {
    let d = std::env::temp_dir().join(format!(
        "mchub-p1-encoding-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&d).unwrap();
    let bytes = b"motd=\xa4\xa4\nserver-port=27778\n";
    fs::write(d.join("server.properties"), bytes).unwrap();
    assert_eq!(mc_hub_core::server::launch::read_port(&d, 25565), 27778);
    assert!(mc_hub_core::properties::read(&d).is_err());
    assert_eq!(fs::read(d.join("server.properties")).unwrap(), bytes);
}
#[test]
fn backup_preferences_and_preflight_are_real_core_operations() {
    let d = std::env::temp_dir().join(format!(
        "mchub-p1-ops-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&d).unwrap();
    let h = Hub::new(&d.join("data")).unwrap();
    assert_eq!(h.preferences()["backup_keep"], 10);
    assert_eq!(h.call("/api/settings",&json!({"preferences":{"backup_keep":3,"backup_time":"04:15","backup_schedule":true,"backup_dir":d}}))["preferences"]["backup_keep"],3);
    assert!(
        h.call(
            "/api/settings",
            &json!({"preferences":{"backup_time":"25:00"}})
        )["error"]
            .is_string()
    );
    assert!(
        h.call(
            "/api/settings",
            &json!({"preferences":{"backup_snapshot":false}})
        )["error"]
            .is_string()
    );
    let s = Server::new(json!({"id":"x","dir":d,"bat":"missing.bat"}));
    let report = s.preflight_report(Some(21));
    assert_eq!(report["ready"], false);
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["key"] == "disk")
    );
}

#[test]
fn daily_backup_is_once_persisted_and_never_runs_on_busy_port() {
    let d = std::env::temp_dir().join(format!(
        "mchub-p1-schedule-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let srv = d.join("srv");
    fs::create_dir_all(srv.join("world")).unwrap();
    fs::write(srv.join("world/level.dat"), "original").unwrap();
    fs::write(srv.join("start.bat"), "java -jar server.jar\n").unwrap();
    fs::write(srv.join("start.sh"), "java -jar server.jar\n").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    fs::write(
        srv.join("server.properties"),
        format!("server-port={port}\n"),
    )
    .unwrap();
    let h = Hub::new(&d.join("data")).unwrap();
    let id = h.call("/api/add", &json!({"dir":srv}))["id"].clone();
    h.call(
        "/api/settings",
        &json!({"preferences":{"backup_schedule":true,"backup_time":"04:00","backup_dir":d}}),
    );
    h.run_backup_schedule("20261115_035959");
    assert!(!srv.join("backups").exists());
    h.run_backup_schedule("20261115_040000");
    let failed = h.call("/api/state", &json!({}));
    assert_eq!(failed["servers"][0]["backup_busy"], false);
    assert!(
        h.call("/api/backups", &json!({"id":id}))["backups"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    drop(listener);
    h.run_backup_schedule("20261115_140000");
    assert!(
        h.call("/api/backups", &json!({"id":id}))["backups"]
            .as_array()
            .unwrap()
            .is_empty(),
        "當天跳過不補做"
    );
    h.run_backup_schedule("20261116_040000");
    for _ in 0..100 {
        if h.call("/api/state", &json!({}))["servers"][0]["backup_busy"] == false {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        h.call("/api/backups", &json!({"id":id}))["backups"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(h);
    let h = Hub::new(&d.join("data")).unwrap();
    h.run_backup_schedule("20261116_120000");
    assert_eq!(
        h.call("/api/backups", &json!({"id":id}))["backups"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "重開平台不重複排程"
    );
}

#[test]
fn disk_thresholds_are_conservative_and_unknown_is_visible() {
    use mc_hub_core::server::disk_check;
    assert_eq!(disk_check(Some(100))["state"], "error");
    assert_eq!(disk_check(Some(2 * 1073741824))["state"], "warning");
    assert_eq!(disk_check(Some(8 * 1073741824))["state"], "ok");
    assert_eq!(disk_check(None)["state"], "warning");
}
