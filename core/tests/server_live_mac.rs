// Mac 版假伺服器實測（第九節第 12 步）：用真的 Java 跑 legacy/tests/FakeServer.java，
// 以 start.sh 開到 Done → 看狀態／記憶體 → 下指令 → stop 存檔關機。
// 要本機有 JDK（javac、jar），平常不跑：cargo test -p mc-hub-core --test server_live_mac -- --ignored --nocapture
// 測試資料夾放在 TMPDIR 底下、跑完不自動刪（留證據，由人決定何時清）。
// 不測「無限重開迴圈」寫法：stop 後若迴圈又拉起 Java，會留下要強制結束的殘留程序。
#![cfg(target_os = "macos")]
use mc_hub_core::server::{Blocked, Server};
use mc_hub_core::{java, platform};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn jdk_tool(name: &str) -> PathBuf {
    java::installed_javas()
        .iter()
        .map(|(_, p)| p.with_file_name(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("找不到 {name}，這個測試要裝 JDK"))
}

fn run(tool: &str, args: &[&str], cwd: &Path) {
    let ok = platform::command(jdk_tool(tool)).args(args).current_dir(cwd).status().unwrap().success();
    assert!(ok, "{tool} {args:?} 失敗");
}

fn fake_jar(base: &Path) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../legacy/tests/FakeServer.java");
    let classes = base.join("classes");
    fs::create_dir_all(&classes).unwrap();
    run("javac", &["-encoding", "UTF-8", "-d", classes.to_str().unwrap(), src.to_str().unwrap()], base);
    run("jar", &["cfe", "server.jar", "FakeServer", "-C", "classes", "."], base);
    base.join("server.jar")
}

fn make(base: &Path, jar: &Path, name: &str, sh: &[u8], port: u16) -> Server {
    let d = base.join(name);
    fs::create_dir_all(&d).unwrap();
    let script = d.join("start.sh");
    fs::write(&script, sh).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(d.join("server.properties"), format!("server-port={port}\n")).unwrap();
    fs::write(d.join("eula.txt"), "eula=true\n").unwrap();
    fs::copy(jar, d.join("server.jar")).unwrap();
    Server::new(json!({"id": name, "name": name, "dir": d.display().to_string(), "bat": "start.sh", "version": "1.21.1"}))
}

fn wait(s: &Server, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        s.poll_status();
        let snap = s.snapshot();
        if ok(&snap) {
            return snap;
        }
        assert!(Instant::now() < end, "等不到「{what}」：{snap}\n{}", log(s));
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn log(s: &Server) -> String {
    s.logs_since(0).1.into_iter().map(|l| l.1).collect::<Vec<_>>().join("\n")
}

#[test]
#[ignore]
fn fake_server_start_sh_to_done_and_stop() {
    let base = std::env::temp_dir().join(format!("mcsrv_mac_{}", std::process::id()));
    assert!(!base.exists(), "測試資料夾已存在，不覆蓋：{}", base.display());
    fs::create_dir_all(&base).unwrap();
    println!("測試資料夾：{}", base.display());
    let jar = fake_jar(&base);
    let java = jdk_tool("java").display().to_string();
    let forms: [(&str, Vec<u8>, u16); 3] = [
        ("A 單行 空白", b"#!/bin/sh\njava -Xmx256M -jar server.jar nogui\n".to_vec(), 25621),
        ("B 平台產生", platform::start_script(&java, "-Xmx256M -Xms64M -jar server.jar nogui", platform::Os::Mac).unwrap(), 25622),
        ("C 看不懂", b"#!/bin/sh\nif [ -f server.jar ]; then\n  java -Xmx256M -jar server.jar nogui\nelse\n  echo missing\nfi\n".to_vec(), 25623),
    ];
    for (name, sh, port) in &forms {
        let s = make(&base, &jar, name, sh, *port);
        assert_eq!(s.snapshot()["state"], "stopped");
        s.start(None).unwrap_or_else(|e| panic!("{name} 開不了機：{e:?}"));
        let snap = wait(&s, "開到 Done 且查得到 1 位玩家", |v| v["state"] == "running" && v["online"] == 1);
        assert_eq!(snap["names"], json!(["Tester"]));
        let mem = wait(&s, "量到 Java 記憶體", |v| v["mem_mb"].as_u64().is_some_and(|mb| mb > 20));
        assert_eq!(snap["port"], *port);
        assert!(s.force_kill().is_err(), "還沒 stop 就能強制結束");
        assert!(matches!(s.start(None), Err(Blocked::Error(e)) if e.msg == "已經在執行中了"));
        assert_eq!(s.send("say hi\nstop"), Err("指令不能換行".into()));
        s.send("/list").unwrap();
        std::thread::sleep(Duration::from_millis(800));
        s.stop().unwrap();
        let snap = wait(&s, "關機", |v| v["state"] == "stopped");
        let l = log(&s);
        assert!(l.contains("There are 1 of a max of 15") && l.contains("中文顯示正常") && l.contains("All dimensions are saved"), "{l}");
        assert!(l.contains("結束代碼 0") && snap["problem"].is_null(), "應該是自己存檔關掉：{l}");
        assert_eq!(l.matches("Loading fake server").count(), 1, "{l}");
        println!("開關機通過：{name}（記憶體 {} MB）", mem["mem_mb"]);
    }

    // EULA 沒同意：擋下、不代為同意
    let a = base.join("A 單行 空白");
    fs::remove_file(a.join("eula.txt")).unwrap();
    let s = Server::new(json!({"id": "a", "name": "a", "dir": a.display().to_string(), "bat": "start.sh"}));
    assert_eq!(s.start(None), Err(Blocked::NeedEula));
    assert!(!a.join("eula.txt").exists());
    println!("EULA 未同意擋下通過");
}
