use mc_hub_core::hub::Hub;
use serde_json::json;

#[test]
fn preferences_are_validated_persisted_and_reset_without_losing_servers() {
    let dir = std::env::temp_dir().join(format!(
        "mchub-maturity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.json"),
        json!({"servers":[],"preferences":{"future":{"keep":true},"auto_start":"invalid old setting"}}).to_string(),
    )
    .unwrap();
    let hub = Hub::new(&dir).unwrap();
    assert_eq!(hub.call("/api/settings", &json!({}))["preferences"]["close_action"], "quit");
    let prefs = json!({"theme":"light","font_size":16,"sound":false,"volume":25,"stop_wait":180,"start_hidden":true});
    let r = hub.call("/api/settings", &json!({"preferences":prefs}));
    assert_eq!(r["preferences"]["theme"], "light", "{r}");
    let disk: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("config.json")).unwrap()).unwrap();
    assert_eq!(disk["preferences"]["future"], json!({"keep":true}));
    let before = std::fs::read(dir.join("config.json")).unwrap();
    assert!(hub.call("/api/settings", &json!({"preferences":{"volume":101,"theme":"dark"}}))["error"].is_string());
    assert_eq!(std::fs::read(dir.join("config.json")).unwrap(), before);
    assert_eq!(Hub::new(&dir).unwrap().call("/api/settings", &json!({}))["preferences"]["stop_wait"], 180);
    assert!(hub.begin_update().is_ok());
    assert!(hub.call("/api/create", &json!({}))["error"].as_str().unwrap().contains("更新"));
    hub.finish_update();
    assert!(hub.call("/api/reset-settings", &json!({"confirm":false}))["error"].is_string());
    let r = hub.call("/api/reset-settings", &json!({"confirm":true}));
    assert_eq!(r["preferences"]["theme"], "dark", "{r}");
    assert_eq!(std::fs::read(r["backup"].as_str().unwrap()).unwrap(), before);
    let disk: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("config.json")).unwrap()).unwrap();
    assert_eq!(disk["preferences"]["future"], json!({"keep":true}));
    let diagnostic = hub.call("/api/diagnostics", &json!({}));
    let text = std::fs::read_to_string(diagnostic["path"].as_str().unwrap()).unwrap();
    assert!(!text.contains(&dir.display().to_string()));
    assert!(!text.contains("servers\": ["));
}
