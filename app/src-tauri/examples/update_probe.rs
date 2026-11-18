//! 僅供 tests/update.cjs；真 updater＋本機 HTTP＋TEMP 假安裝器，不連正式服務。
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::UpdaterExt;
fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("MCHUB_DATA_DIR").expect("隔離資料夾"));
    assert!(root.is_absolute() && root.starts_with(std::env::temp_dir()));
    let url = std::env::var("MCHUB_PROBE_URL").unwrap();
    assert!(url.starts_with("http://127.0.0.1:"));
    let key = std::fs::read_to_string(root.join("test.key.pub")).unwrap();
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({"pubkey":key.trim(),"requireSignedVersion":true,"dangerousInsecureTransportProtocol":true}),
    );
    let app = mock_builder().plugin(tauri_plugin_updater::Builder::new().build()).build(context).unwrap();
    let hook = root.join("before-exit");
    let marker = root.join("installed");
    let updater = app
        .updater_builder()
        .endpoints(vec![url.parse().unwrap()])
        .unwrap()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(5))
        .version_comparator(|_, _| true)
        .restart_after_install(false)
        .installer_args(["--marker".into(), marker.into_os_string()])
        .on_before_exit(move || {
            std::fs::write(&hook, "before-exit").unwrap();
        })
        .build()
        .unwrap();
    let result: Result<(), String> = tauri::async_runtime::block_on(async {
        let Some(mut update) = updater.check().await.map_err(|e| e.to_string())? else {
            println!("NO_UPDATE");
            return Ok(());
        };
        update.timeout = Some(std::time::Duration::from_millis(if std::env::var("MCHUB_PROBE_STALL").as_deref() == Ok("1") {
            200
        } else {
            900_000
        }));
        let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
        if std::env::var("MCHUB_PROBE_INSTALL").as_deref() == Ok("1") {
            update.install(bytes).map_err(|e| e.to_string())?;
        } else {
            println!("VERIFIED {}", bytes.len());
        }
        Ok(())
    });
    if result.is_err() {
        println!("UPDATE_REJECTED");
        std::process::exit(2);
    }
}
