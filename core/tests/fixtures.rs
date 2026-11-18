// 1.x Python 版匯出的標準答案（legacy/tests/export_fixtures.py → tests/fixtures/*.json）。
// 每個函式搬完就在這裡逐筆比對：同樣的輸入，Rust 版要給出和 Python 版一字不差的結果（含錯誤訊息）。
use mc_hub_core::server::launch;
use mc_hub_core::{commands, create, fs_safe, java, net, platform, settings};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}};

fn load(name: &str) -> Vec<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../tests/fixtures/{name}.json"));
    let data: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    data["cases"].as_array().unwrap().clone()
}

/// 標準答案在 Windows 匯出；Mac 版刻意改用 Mac 用語（1.2.0 定案），比對前換成同一套說法。
fn platform_wording(text: &str) -> String {
    if platform::OS == platform::Os::Win {
        return text.to_string();
    }
    text.replace("start.bat", platform::start_script_name(platform::OS))
        .replace("記事本", "「文字編輯」")
}

/// 跑某檔案裡某函式的全部案例；call 回傳 Ok(輸出) 或 Err(錯誤訊息)。回傳比對了幾筆，避免標準答案被改名後變成 0 筆也算過。
fn check(file: &str, fn_name: &str, call: impl Fn(&Value) -> Result<Value, String>) -> usize {
    let cases: Vec<Value> = load(file).into_iter().filter(|c| c["fn"] == fn_name).collect();
    assert!(!cases.is_empty(), "{file} 裡沒有 {fn_name} 的案例");
    for c in &cases {
        let want = match c.get("error") {
            Some(e) => Err(platform_wording(e.as_str().unwrap())),
            None => Ok(match &c["out"] {
                Value::String(text) => json!(platform_wording(text)),
                other => other.clone(),
            }),
        };
        assert_eq!(call(&c["args"]), want, "{fn_name} 輸入 {}", c["args"]);
    }
    cases.len()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

#[test]
fn fixtures_load() {
    let mut total = 0;
    for name in ["launch", "versions", "failures", "commands", "text"] {
        let cases = load(name);
        assert!(!cases.is_empty(), "{name} 沒有案例");
        for c in &cases {
            assert!(c["fn"].is_string() && (c.get("out").is_some() ^ c.get("error").is_some()), "{name} 格式不對：{c}");
        }
        total += cases.len();
    }
    assert!(total >= 300, "標準答案只有 {total} 筆");
}

#[test]
fn start_script() {
    let n = check("launch", "start_script", |a| {
        let os = if s(&a["os"]) == "win" { platform::Os::Win } else { platform::Os::Mac };
        let bytes = platform::start_script(s(&a["java"]), s(&a["args"]), os).map_err(|e| e.msg)?;
        Ok(json!(String::from_utf8(bytes).unwrap()))
    });
    assert_eq!(n, 2);
}

#[test]
fn java_for_mc() {
    assert_eq!(check("versions", "java_for_mc", |a| Ok(json!(java::java_for_mc(s(&a["version"]))))), 10);
}

#[test]
fn newer_version() {
    let n = check("versions", "newer_version", |a| Ok(json!(net::newer_version(s(&a["body"]).as_bytes(), s(&a["current"])))));
    assert_eq!(n, 12);
}

#[test]
fn clean_links() {
    assert_eq!(check("text", "clean_links", |a| settings::clean_links(&a["items"]).map(Value::from).map_err(|e| e.msg)), 4);
}

/// 每個測試自己的暫存資料夾，名稱故意有中文與空白（和 1.x 匯出時一樣）。
fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mcfix_{tag}_{}", std::process::id())).join("假 伺服器");
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn parse_launch() {
    let dir = temp_dir("launch");
    let shown = dir.display().to_string();
    let n = check("launch", "parse_launch", |a| {
        fs::write(dir.join(s(&a["file"])), s(&a["text"])).unwrap();
        let r = launch::parse_launch(&dir, s(&a["file"])).unwrap();
        Ok(json!({"cmd": r.cmd.replace(&shown, "<DIR>"), "java": r.java.map(|j| j.replace(&shown, "<DIR>")), "mode": r.mode.as_str()}))
    });
    assert_eq!(n, 24);
}

#[test]
fn modloader_args() {
    assert_eq!(check("launch", "modloader_args", |a| Ok(json!(launch::modloader_args(s(&a["text"]))))), 6);
}

#[test]
fn explain_failure() {
    let n = check("failures", "explain_failure", |a| {
        let lines: Vec<String> = a["lines"].as_array().unwrap().iter().map(|l| s(l).to_string()).collect();
        Ok(json!(launch::explain_failure(&lines, a["code"].as_i64().unwrap(), a["saw_done"].as_bool().unwrap())))
    });
    assert_eq!(n, 12);
}

#[test]
fn command_list() {
    assert_eq!(check("commands", "commands_count", |_| Ok(json!(commands::builtin().len()))), 1);
    assert_eq!(check("commands", "commands_for", |a| Ok(json!(commands::commands_for(s(&a["label"]), &[])))), 4);
}

#[test]
fn build_command() {
    let all = commands::builtin();
    let n = check("commands", "build_command", |a| {
        let cmd = all.iter().find(|c| c["id"] == a["id"]).unwrap();
        commands::build_command(cmd, &a["values"], &a["confirm"], s(&a["label"])).map(Value::from).map_err(|e| e.msg)
    });
    assert_eq!(n, 189);
}

#[test]
fn clean_custom_commands() {
    assert_eq!(check("text", "clean_custom_commands", |a| commands::clean_custom_commands(&a["items"]).map(Value::from).map_err(|e| e.msg)), 8);
}

#[test]
fn check_folder_name() {
    assert_eq!(check("text", "check_folder_name", |a| Ok(json!(fs_safe::check_folder_name(s(&a["name"]))))), 12);
}

#[test]
fn join_text() {
    assert_eq!(check("text", "join_text", |a| Ok(json!(commands::join_text(s(&a["template"]), &a["server_cfg"])))), 5);
}

#[test]
fn mc_tuple() {
    assert_eq!(check("versions", "mc_tuple", |a| Ok(json!(commands::mc_tuple(s(&a["v"]))))), 9);
    assert_eq!(check("versions", "mc_of", |a| Ok(json!(commands::mc_of(s(&a["label"]))))), 6);
}

#[test]
fn ram() {
    let dir = temp_dir("ram");
    let script = dir.join("ram.bat");
    let n = check("text", "set_ram", |a| {
        fs::write(&script, s(&a["text"])).unwrap();
        launch::set_ram(&script, a["gb"].as_u64().unwrap() as u32).map_err(|e| e.msg)?;
        assert_eq!(fs::read(dir.join("ram.bat.bak")).unwrap(), s(&a["text"]).as_bytes(), "改之前要留 .bak");
        Ok(json!(String::from_utf8(fs::read(&script).unwrap()).unwrap()))
    });
    assert_eq!(n, 5);
    let n = check("text", "read_ram_gb", |a| {
        fs::write(&script, s(&a["text"])).unwrap();
        // 1.x：寫 G 回整數、寫 M 回小數
        Ok(launch::read_ram_gb(&script).map_or(Value::Null, |gb| if s(&a["text"]).contains(['G', 'g']) { json!(gb as u64) } else { json!(gb) }))
    });
    assert_eq!(n, 4);
}

/// 挑版結果在 1.x 是 (版本, 不能選的原因) 兩格。
fn pair(r: Result<String, &str>) -> Value {
    match r {
        Ok(v) => json!([v, null]),
        Err(why) => json!([null, why]),
    }
}

#[test]
fn loader_versions() {
    assert_eq!(check("versions", "forge_pick", |a| Ok(pair(create::forge_pick(&a["promos"], s(&a["mc"]))))), 8);
    assert_eq!(check("versions", "neoforge_mc", |a| Ok(json!(create::neoforge_mc(s(&a["v"]))))), 7);
    assert_eq!(check("versions", "neoforge_pick", |a| Ok(pair(create::neoforge_pick(&a["data"], s(&a["mc"]))))), 5);
    let n = check("versions", "loader_choice", |a| Ok(create::loader_choice(s(&a["kind"]), s(&a["mc"]), Some(&a["data"]).filter(|d| !d.is_null()))));
    assert_eq!(n, 4);
    assert_eq!(check("versions", "forge_installer_url", |a| Ok(json!(create::forge_installer_url(s(&a["mc"]), s(&a["v"]))))), 1);
    assert_eq!(check("versions", "neoforge_installer_url", |a| Ok(json!(create::neoforge_installer_url(s(&a["v"]))))), 1);
}
