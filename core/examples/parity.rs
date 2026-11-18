//! 測試專用 JSON-lines 入口；不開視窗、不連版本網站，不清理測試資料。
use mc_hub_core::{commands, create, fs_safe, hub::Hub, java, net, platform, settings};
use mc_hub_core::server::launch;
use serde_json::{json, Value};
use std::{fs, io::{self, BufRead, Write}, path::{Path, PathBuf}, time::Duration};

fn s(v: &Value) -> &str { v.as_str().unwrap_or("") }
fn pair(r: Result<String, &str>) -> Value { match r { Ok(v) => json!([v, null]), Err(e) => json!([null, e]) } }
fn fixture(c: &Value, dir: &Path) -> Result<Value, String> {
    let a = &c["args"];
    Ok(match s(&c["fn"]) {
        "parse_launch" => {
            fs::write(dir.join(s(&a["file"])), s(&a["text"])).map_err(|e| e.to_string())?;
            let l = launch::parse_launch(dir, s(&a["file"])).map_err(|e| e.to_string())?;
            let shown = dir.display().to_string();
            json!({"cmd": l.cmd.replace(&shown, "<DIR>"), "java": l.java.map(|j| j.replace(&shown, "<DIR>")), "mode": l.mode.as_str()})
        }
        "start_script" => json!(String::from_utf8(platform::start_script(s(&a["java"]), s(&a["args"]), if a["os"] == "mac" {platform::Os::Mac} else {platform::Os::Win}).map_err(|e| e.msg)?).unwrap()),
        "modloader_args" => json!(launch::modloader_args(s(&a["text"]))),
        "java_for_mc" => json!(java::java_for_mc(s(&a["version"]))),
        "mc_tuple" => json!(commands::mc_tuple(s(&a["v"]))),
        "mc_of" => json!(commands::mc_of(s(&a["label"]))),
        "newer_version" => json!(net::newer_version(s(&a["body"]).as_bytes(), s(&a["current"]))),
        "forge_pick" => pair(create::forge_pick(&a["promos"], s(&a["mc"]))),
        "neoforge_mc" => json!(create::neoforge_mc(s(&a["v"]))),
        "neoforge_pick" => pair(create::neoforge_pick(&a["data"], s(&a["mc"]))),
        "loader_choice" => create::loader_choice(s(&a["kind"]), s(&a["mc"]), Some(&a["data"]).filter(|d| !d.is_null())),
        "forge_installer_url" => json!(create::forge_installer_url(s(&a["mc"]), s(&a["v"]))),
        "neoforge_installer_url" => json!(create::neoforge_installer_url(s(&a["v"]))),
        "explain_failure" => json!(launch::explain_failure(&a["lines"].as_array().unwrap().iter().map(|v| s(v).to_string()).collect::<Vec<_>>(), a["code"].as_i64().unwrap(), a["saw_done"].as_bool().unwrap())),
        "commands_count" => json!(commands::builtin().len()),
        "commands_for" => json!(commands::commands_for(s(&a["label"]), &[])),
        "build_command" => {
            let c = commands::builtin().into_iter().find(|c| c["id"] == a["id"]).unwrap();
            json!(commands::build_command(&c, &a["values"], &a["confirm"], s(&a["label"])).map_err(|e| e.msg)?)
        }
        "clean_custom_commands" => json!(commands::clean_custom_commands(&a["items"]).map_err(|e| e.msg)?),
        "clean_links" => json!(settings::clean_links(&a["items"]).map_err(|e| e.msg)?),
        "check_folder_name" => json!(fs_safe::check_folder_name(s(&a["name"]))),
        "join_text" => json!(commands::join_text(s(&a["template"]), &a["server_cfg"])),
        "set_ram" | "read_ram_gb" => {
            let script = dir.join("ram.bat");
            fs::write(&script, s(&a["text"])).map_err(|e| e.to_string())?;
            if c["fn"] == "set_ram" {
                launch::set_ram(&script, a["gb"].as_u64().unwrap() as u32).map_err(|e| e.msg)?;
                assert_eq!(fs::read(script.with_extension("bat.bak")).unwrap(), s(&a["text"]).as_bytes());
                json!(fs::read_to_string(script).unwrap())
            } else { json!(launch::read_ram_gb(&script)) }
        }
        other => return Err(format!("unknown fixture {other}")),
    })
}

fn main() {
    let root = PathBuf::from(std::env::var_os("MCHUB_DATA_DIR").expect("MCHUB_DATA_DIR required")).canonicalize().unwrap();
    assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert!(root.join("parity-marker").is_file(), "only run from tests/parity.py");
    let data = root.join("rust/config");
    fs::create_dir_all(&data).unwrap();
    let scratch = root.join("rust/fixtures");
    fs::create_dir_all(&scratch).unwrap();
    let mut hub = Hub::new(&data).unwrap();
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let out = match s(&v["op"]) {
            "call" => hub.call(s(&v["path"]), &v["body"]),
            "poll" => { hub.poll(); json!({"ok": true}) },
            "busy" => json!(hub.busy()),
            "reload" => { assert!(!hub.busy()); hub = Hub::new(&data).unwrap(); json!({"ok": true}) },
            "fixture" => match fixture(&v["case"], &scratch) { Ok(v) => json!({"out": v}), Err(e) => json!({"error": e}) },
            "inspect" => create::inspect_server(Path::new(s(&v["dir"]))),
            "java" => json!(java::installed_javas().iter().map(|(n,p)| json!([n,p])).collect::<Vec<_>>()),
            "install" => {
                let mut lines = Vec::new();
                let result = create::install_modloader(Path::new(s(&v["dir"])), Path::new(s(&v["java"])), Path::new(s(&v["installer"])), 1, &mut |s| lines.push(s.to_string()), &root.join("rust"), Duration::from_secs(v["timeout"].as_u64().unwrap_or(30)));
                match result { Ok(bat) => json!({"out": bat,"lines": lines}), Err(e) => json!({"error": e.msg,"lines": lines}) }
            }
            "quit" => { assert!(!hub.busy()); break; },
            _ => panic!("unknown parity op"),
        };
        println!("{out}");
        io::stdout().flush().unwrap();
    }
}
