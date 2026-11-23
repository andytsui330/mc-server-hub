use mc_hub_core::hub::Hub;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const STABLE: &str = "https://github.com/andytsui330/mc-server-hub/releases/latest/download/latest.json";
pub const BETA: &str = "https://github.com/andytsui330/mc-server-hub/releases/download/beta/latest.json";

#[derive(Default)]
pub struct Updates {
    pending: Mutex<Option<Update>>,
    pub status: Mutex<Value>,
    active: AtomicBool,
}

struct Active<'a>(&'a AtomicBool);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
struct Maintenance(Arc<Hub>);
impl Drop for Maintenance {
    fn drop(&mut self) {
        self.0.finish_update();
    }
}

fn manifest_view(update: &Update) -> Value {
    let size = update
        .raw_json
        .get("size")
        .cloned()
        .or_else(|| update.raw_json["platforms"][&update.target].get("size").cloned());
    json!({"state":"available","version":update.version,"notes":update.body,"date":update.date.map(|d|d.to_string()),"size":size})
}

/// 不帶 version 鍵：畫面看到 version 會再顯示「可更新至」與更新按鈕。
fn installed_view(version: &str) -> Value {
    json!({"state":"installed","message":format!("已更新到 {version}，平台即將重新開啟。若沒有自動開啟，請關閉平台後重新打開。")})
}

pub async fn check(app: &AppHandle, manual: bool) -> Result<Value, String> {
    let hub = app.state::<Arc<Hub>>();
    if !manual && !hub.update_enabled() {
        return Ok(json!({"state":"disabled"}));
    }
    // 隔離畫面測試不連正式更新端點；簽章流程另由真 updater 的本機測試驗證。
    if std::env::var_os("MCHUB_DATA_DIR").is_some() {
        return Ok(json!({"state":"isolated","message":"測試模式：不連線到正式更新服務"}));
    }
    let state = app.state::<Updates>();
    state
        .active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "正在檢查或下載更新".to_string())?;
    let _active = Active(&state.active);
    let channel = hub.preferences()["channel"].as_str().unwrap_or("stable").to_string();
    let url = if channel == "beta" { BETA } else { STABLE };
    let updater = app
        .updater_builder()
        .endpoints(vec![url.parse().map_err(|_| "更新網址不正確")?])
        .map_err(|e| e.to_string())?
        .configure_client(crate::update_transport::configure)
        .on_before_exit({
            let dir = hub.data_dir().join("reports");
            move || {
                let _ = crate::reports::session(&dir, true);
            }
        })
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let result = updater
        .check()
        .await
        .map_err(|_| "目前連不到更新服務，請稍後再試；原本的程式與設定都保留".to_string());
    let found = result?;
    if channel != hub.preferences()["channel"].as_str().unwrap_or("stable") {
        return Err("更新頻道已變更，請重新檢查".into());
    }
    let view = if let Some(ref update) = found {
        if !crate::update_transport::allowed(&update.download_url)
            || update.download_url.host_str() != Some("github.com")
            || !update.download_url.path().starts_with("/andytsui330/mc-server-hub/releases/download/")
        {
            return Err("更新檔來源不正確，已停止；原版保留".into());
        }
        let mut v = manifest_view(update);
        v["channel"] = json!(channel);
        v
    } else {
        json!({"state":"current"})
    };
    *state.pending.lock().unwrap() = found;
    *state.status.lock().unwrap() = view.clone();
    let _ = app.emit("update-status", &view);
    Ok(view)
}

pub async fn install(app: &AppHandle) -> Result<Value, String> {
    let hub = app.state::<Arc<Hub>>().inner().clone();
    let state = app.state::<Updates>();
    state
        .active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "正在檢查或下載更新".to_string())?;
    let _active = Active(&state.active);
    hub.begin_update()?;
    let maintenance = Maintenance(hub.clone());
    let mut update = state.pending.lock().unwrap().take().ok_or("請先檢查更新")?;
    update.timeout = Some(Duration::from_secs(15 * 60));
    let saved_channel = state.status.lock().unwrap()["channel"].clone();
    if saved_channel != hub.preferences()["channel"] {
        return Err("更新頻道已變更，請重新檢查".into());
    }
    let mut downloaded = 0u64;
    let downloaded_bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                let view = json!({"state":"downloading","downloaded":downloaded,"total":total});
                *state.status.lock().unwrap() = view.clone();
                let _ = app.emit("update-status", view);
            },
            || {},
        )
        .await;
    let result = match downloaded_bytes {
        Ok(bytes) => update.install(bytes),
        Err(e) => Err(e),
    };
    match result {
        Ok(()) => {
            // Windows 由安裝程式自己關閉／重開，走不到這裡；Mac 換完程式要自己重開，不然畫面會停在下載 100%。
            let view = installed_view(&update.version);
            *state.status.lock().unwrap() = view.clone();
            let _ = app.emit("update-status", &view);
            if !cfg!(windows) {
                let app = app.clone();
                // 留一點時間讓畫面顯示提示；維護狀態帶到重開為止，避免這段空檔被開伺服器。
                std::thread::spawn(move || {
                    let _maintenance = maintenance;
                    std::thread::sleep(Duration::from_millis(1500));
                    app.request_restart();
                });
            }
            Ok(view)
        }
        Err(_) => {
            let _ = crate::reports::resume(&hub.data_dir().join("reports"));
            let view = json!({"state":"error","message":"更新失敗或簽章不符，原本的程式與設定都保留。請重新檢查後再試。"});
            *state.status.lock().unwrap() = view.clone();
            let _ = app.emit("update-status", &view);
            Ok(view)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn installed_view_hides_update_button() {
        let v = super::installed_view("2.0.2");
        assert_eq!(v["state"], "installed");
        assert!(v.get("version").is_none());
        assert!(v["message"].as_str().unwrap().contains("2.0.2"));
    }
}
