// 正式版不跳黑色命令列視窗
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod links;
mod reports;
mod update_transport;
mod updates;

use mc_hub_core::{hub::Hub, platform};
use serde_json::{Value, json};
use std::sync::{Arc,Mutex};
use std::time::Duration;
use tauri::{Emitter, Manager};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct DraftState(Mutex<(bool,bool)>);

fn request_exit(app:&tauri::AppHandle,discard:bool)->Value{
    let hub=app.state::<Arc<Hub>>();
    let (dirty,busy)=*app.state::<DraftState>().0.lock().unwrap_or_else(|e|e.into_inner());
    if hub.busy()||hub.updating()||busy{show(app);let _=app.emit("close-blocked",());return json!({"error":"請先關閉伺服器，並等建服、備份、儲存或更新完成"});}
    if dirty&&!discard{show(app);let _=app.emit("draft-close-requested",());return json!({"need":"draft"});}
    app.exit(0);json!({"ok":true})
}

/// 畫面呼叫後端的唯一入口（對應 app/src/api.ts）；在背景執行緒跑，不卡畫面。
#[tauri::command]
async fn api(app: tauri::AppHandle, hub: tauri::State<'_, Arc<Hub>>, path: String, body: Value) -> Result<Value, String> {
    if path=="/api/desktop/draft"{
        let state=app.state::<DraftState>();let mut guard=state.0.lock().unwrap_or_else(|e|e.into_inner());
        if body.get("dirty").is_some(){*guard=(body["dirty"]==true,body["busy"]==true);}
        return Ok(json!({"dirty":guard.0,"busy":guard.1}));
    }
    if path == "/api/updates/check" {
        return Ok(updates::check(&app, true).await.unwrap_or_else(|e| json!({"error":e})));
    }
    if path == "/api/updates/install" {
        if body["confirm"] != true {
            return Ok(json!({"error":"請先確認下載並更新"}));
        }
        return Ok(updates::install(&app).await.unwrap_or_else(|e| json!({"error":e})));
    }
    if path == "/api/updates/status" {
        return Ok(app.state::<updates::Updates>().status.lock().unwrap().clone());
    }
    if path == "/api/desktop/status" {
        let w = app.get_webview_window("main");
        let isolated = std::env::var_os("MCHUB_DATA_DIR").is_some();
        return Ok(
            json!({"visible":w.as_ref().is_some_and(|w|w.is_visible().unwrap_or(false)),"tray":app.tray_by_id("main-tray").is_some(),"isolated":isolated,
            "launch_at_login":if isolated{hub.preferences()["launch_at_login"].as_bool().unwrap_or(false)}else{app.autolaunch().is_enabled().unwrap_or(false)}}),
        );
    }
    if path == "/api/desktop/hide" {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
        return Ok(json!({"ok":true}));
    }
    if path == "/api/desktop/quit" {
        return Ok(request_exit(&app,body["discard"]==true));
    }
    if path == "/api/reset-settings" && body["confirm"] == true && std::env::var_os("MCHUB_DATA_DIR").is_none() {
        if hub.busy() || hub.updating() {
            return Ok(json!({"error":"請先關機，並等備份、建服與更新完成"}));
        }
        let previous = app.autolaunch().is_enabled().map_err(|_| "讀不到登入啟動狀態")?;
        if previous && app.autolaunch().disable().is_err() {
            return Ok(json!({"error":"無法關閉登入啟動，已取消重設"}));
        }
        let result = hub.call(&path, &body);
        if result["error"].is_string() && previous {
            let _ = app.autolaunch().enable();
        }
        return Ok(result);
    }
    if path == "/api/settings" && body.get("preferences").and_then(|v| v.get("launch_at_login")).is_some() {
        // TEMP 測試使用記憶體替身，絕不寫入正式 Run 登錄或登入項目。
        let checked = mc_hub_core::settings::clean_preferences(&hub.preferences(), &body["preferences"]);
        if let Err(e) = checked {
            return Ok(json!({"error":e.msg}));
        }
        if std::env::var_os("MCHUB_DATA_DIR").is_none() {
            let previous = app.autolaunch().is_enabled().map_err(|_| "讀不到登入啟動狀態")?;
            let wanted = body["preferences"]["launch_at_login"] == true;
            let result = if wanted { app.autolaunch().enable() } else { app.autolaunch().disable() };
            if result.is_err() {
                return Ok(json!({"error":"無法變更登入啟動，原設定保留"}));
            }
            let result = hub.call(&path, &body);
            if result["error"].is_string() {
                let _ = if previous { app.autolaunch().enable() } else { app.autolaunch().disable() };
            }
            return Ok(result);
        }
    }
    if path == "/api/desktop/open" {
        let target = match body["what"].as_str() {
            Some("log") => hub.data_dir().join("reports/session.json"),
            Some("reports") => hub.data_dir().join("reports"),
            _ => hub.data_dir().to_path_buf(),
        };
        let parent = if body["what"] == "log" { target.parent().unwrap() } else { &target };
        return Ok(match std::fs::create_dir_all(parent).and_then(|_| platform::open_path(&target)) {
            Ok(()) => json!({"ok":true}),
            Err(_) => json!({"error":"打不開設定資料夾或紀錄檔"}),
        });
    }
    if path == "/api/legal" {
        return Ok(
            json!({"license":include_str!("../../../LICENSE"),"notices":include_str!("../../../THIRD_PARTY_NOTICES.txt"),"changelog":include_str!("../../../CHANGELOG.md")}),
        );
    }
    // 網址是外殼工作，不讓 core 綁住桌面環境。
    if path == "/api/open-url" {
        return Ok(match links::open(body["url"].as_str().unwrap_or("")) {
            Ok(()) => serde_json::json!({"ok": true}),
            Err(error) => serde_json::json!({"error": error}),
        });
    }
    let hub = hub.inner().clone();
    let is_state = path == "/api/state";
    let mut result = tauri::async_runtime::spawn_blocking(move || hub.call(&path, &body))
        .await
        .map_err(|e| e.to_string())?;
    if is_state {
        let state = app.state::<updates::Updates>();
        let update = state.status.lock().unwrap();
        if update["state"] == "available" {
            result["update"] = update["version"].clone();
        }
    }
    Ok(result)
}

#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    // blocking 對話框必須在工作執行緒，否則會鎖住視窗事件。
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("選擇伺服器資料夾")
            .blocking_pick_folder()
            .map(|path| path.into_path().map(|p| p.to_string_lossy().into_owned()).map_err(|e| e.to_string()))
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn close_window(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

fn main() {
    let home = std::env::home_dir().unwrap_or_default();
    let appdata = std::env::var_os("APPDATA").map(std::path::PathBuf::from);
    // MCHUB_DATA_DIR：開發與對照測試用另一個設定資料夾，不碰真的設定（1.x 用 --config 做同一件事）
    let dir = std::env::var_os("MCHUB_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| platform::data_dir(platform::OS, appdata.as_deref(), &home));
    reports::hook(dir.join("reports"));
    let mut context = tauri::generate_context!();
    if std::env::var_os("MCHUB_DATA_DIR").is_some() {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        std::path::absolute(&dir).unwrap_or(dir.clone()).hash(&mut hash);
        context.config_mut().identifier = format!("io.github.mc-server-hub.test{:x}", hash.finish());
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show(app);
            let _ = app.emit("instance-focused", ());
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::Builder::new().args(["--autostart"]).build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::Updates::default())
        .manage(DraftState::default())
        .setup(move |app| {
            let hub = Hub::new(&dir)?;
            reports::session(&dir.join("reports"), false)?;
            let prefs = hub.preferences();
            app.manage(hub.clone());
            // Mac 選單列預設的 Quit（⌘Q）直接呼叫系統 terminate，不經過可攔截的事件；
            // 換成自訂項目，走和系統匣「結束平台」相同的保護（執行中／未存草稿不結束）。
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::MenuItemKind;
                let menu = Menu::default(app.handle())?;
                if let Some(MenuItemKind::Submenu(sub)) = menu.items()?.into_iter().next() {
                    for item in sub.items()? {
                        if let MenuItemKind::Predefined(p) = &item
                            && p.text()?.starts_with("Quit")
                        {
                            sub.remove(p)?;
                        }
                    }
                    sub.append(&MenuItem::with_id(app, "app-quit", "結束 MC開服平台", true, Some("CmdOrCtrl+Q"))?)?;
                }
                app.set_menu(menu)?;
                app.on_menu_event(|app, event| {
                    if event.id().as_ref() == "app-quit" {
                        request_exit(app, false);
                    }
                });
            }
            let open = MenuItem::with_id(app, "open", "顯示平台", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "結束平台", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("MC 開服平台（執行中）")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        show(tray.app_handle());
                    }
                })
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show(app),
                    "quit" => {
                        request_exit(app,false);
                    }
                    _ => {}
                })
                .build(app)?;
            if prefs["start_hidden"] == true
                && let Some(w) = app.get_webview_window("main")
            {
                let _ = w.hide();
            }
            let poller = hub.clone();
            let startup_dir=dir.clone();
            std::thread::spawn(move || {
                let result=poller.auto_start();
                if result.iter().any(|v|v["error"].is_string()||v["need"].is_string()){
                    let _=reports::record(&startup_dir.join("reports"),"auto-start-blocked");
                }
                loop {
                    poller.poll();
                    std::thread::sleep(Duration::from_secs(3));
                }
            });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = updates::check(&handle, false).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api, pick_folder, close_window])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let hub = window.state::<Arc<Hub>>();
                let (dirty,busy)=*window.state::<DraftState>().0.lock().unwrap_or_else(|e|e.into_inner());
                let prefs = hub.preferences();
                // 還沒選過就先問（Mac 關窗維持原行為，不跳）；選完由畫面呼叫 hide 或 quit。
                if !cfg!(target_os = "macos") && prefs["close_asked"] != true {
                    api.prevent_close();
                    let _ = window.emit("close-ask", ());
                } else if prefs["close_action"] == "tray" {
                    api.prevent_close();
                    let _ = window.hide();
                } else if hub.busy() || hub.updating() || busy {
                    api.prevent_close();
                    let _ = window.emit("close-blocked", ());
                } else if dirty {
                    api.prevent_close();
                    let _=window.emit("draft-close-requested",());
                }
            }
        })
        .build(context)
        .expect("MC 開服平台啟動失敗")
        .run(|app, event| {
            // Mac 的 ⌘Q／選單列 Quit 不經過關窗事件，直接要求結束：比照關窗與系統匣「結束平台」擋下。
            // app.exit(code) 由平台自己確認過才呼叫（code 有值），只攔使用者觸發的（code 為 None）。
            if let tauri::RunEvent::ExitRequested { api, code: None, .. } = &event {
                let hub = app.state::<Arc<Hub>>();
                let (dirty,busy)=*app.state::<DraftState>().0.lock().unwrap_or_else(|e|e.into_inner());
                if hub.busy() || hub.updating() || busy {
                    api.prevent_exit();
                    show(app);
                    let _ = app.emit("close-blocked", ());
                } else if dirty {
                    api.prevent_exit();
                    show(app);
                    let _ = app.emit("draft-close-requested", ());
                }
            }
            if let tauri::RunEvent::Exit = event {
                let hub = app.state::<Arc<Hub>>();
                let _ = reports::session(&hub.data_dir().join("reports"), true);
            }
        });
}

fn show(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
