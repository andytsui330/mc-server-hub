// 用真的 Java 跑假伺服器（legacy/tests/FakeServer.java）：開到 Done → 看狀態 → 下指令 → stop 存檔關機。
// 要本機有 JDK（javac、jar），平常不跑：cargo test -p mc-hub-core -- --ignored --nocapture
// 四種 start.bat 寫法和 1.x tests/test_hub.py 的 FORMS 一樣；Mac 的 start.sh 在第九節第 12 步實機測。
#![cfg(windows)]
use mc_hub_core::server::{Blocked, Server};
use mc_hub_core::{commands, java, platform};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn jdk_tool(name: &str) -> PathBuf {
    let found = java::installed_javas();
    found
        .iter()
        .map(|(_, p)| p.with_file_name(format!("{name}.exe")))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("找不到 {name}.exe，這個測試要裝 JDK"))
}

fn run(tool: &str, args: &[&str], cwd: &Path) {
    let ok = platform::command(jdk_tool(tool)).args(args).current_dir(cwd).status().unwrap().success();
    assert!(ok, "{tool} {args:?} 失敗");
}

/// 編好假伺服器 jar，回傳 jar 路徑。
fn fake_jar(base: &Path) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../legacy/tests/FakeServer.java");
    let classes = base.join("classes");
    fs::create_dir_all(&classes).unwrap();
    run("javac", &["-encoding", "UTF-8", "-d", classes.to_str().unwrap(), src.to_str().unwrap()], base);
    run("jar", &["cfe", "server.jar", "FakeServer", "-C", "classes", "."], base);
    base.join("server.jar")
}

fn make(base: &Path, jar: &Path, name: &str, bat: &str, port: u16) -> Server {
    let d = base.join(name);
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("start.bat"), platform::encode_text(bat).unwrap()).unwrap();
    fs::write(d.join("server.properties"), format!("server-port={port}\n")).unwrap();
    fs::write(d.join("eula.txt"), "eula=true\n").unwrap();
    fs::copy(jar, d.join("server.jar")).unwrap();
    Server::new(json!({"id": name, "name": name, "dir": d.display().to_string(), "bat": "start.bat", "version": "1.21.1"}))
}

/// 每 0.3 秒巡檢一次，直到條件成立（最多 30 秒）。
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
fn fake_server_start_to_done_and_stop() {
    let base = std::env::temp_dir().join(format!("mcsrv_{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let jar = fake_jar(&base);
    let java = jdk_tool("java").display().to_string();
    let forms = [
        ("A 單行", "@echo off\njava -Xmx256M -jar server.jar nogui\npause\n".to_string(), "direct", 25611),
        ("B 變數路徑", format!("@echo off\ntitle 測試\ncd /d \"%~dp0\"\nset \"JAVA_EXE={java}\"\nset RAM=256M\n\"%JAVA_EXE%\" -Xmx%RAM% ^\n  -Xms64M -jar server.jar nogui %*\npause\n"), "direct", 25612),
        ("C 自動重開", ":start\r\njava -Xmx256M -jar server.jar nogui\r\ntimeout /t 2\r\ngoto start\r\n".to_string(), "direct", 25613),
        ("D 看不懂", "@echo off\r\nif exist server.jar (\r\n  java -Xmx256M -jar server.jar nogui\r\n) else (\r\n  echo 找不到\r\n)\r\npause\r\n".to_string(), "bat", 25614),
    ];
    for (name, bat, mode, port) in &forms {
        let s = make(&base, &jar, name, bat, *port);
        assert_eq!(s.snapshot()["state"], "stopped");
        s.start(None).unwrap_or_else(|e| panic!("{name} 開不了機：{e:?}"));
        let snap = wait(&s, "開到 Done 且查得到 1 位玩家", |v| v["state"] == "running" && v["online"] == 1);
        assert_eq!(snap["names"], json!(["Tester"]));
        assert!(snap["mem_mb"].as_u64().is_some_and(|mb| mb > 20), "要量到真正的 Java，不是外殼程式：{snap}");
        assert_eq!(snap["port"], *port);
        assert!(s.force_kill().is_err(), "還沒 stop 就能強制結束");
        assert!(matches!(s.start(None), Err(Blocked::Error(e)) if e.msg == "已經在執行中了"));
        assert_eq!(s.send("say hi\nstop"), Err("指令不能換行".into()));
        s.send("/list").unwrap();
        if *name == "A 單行" {
            // 清敵對生物：讀回難度 → 切和平 → 2 秒後切回；危險指令沒確認要擋
            let cmd = commands::builtin().into_iter().find(|c| c["id"] == "op").unwrap();
            assert!(commands::build_command(&cmd, &json!({"player": "Tester"}), &json!(true), "1.21.1").unwrap_err().msg.contains("強確認"));
            s.clear_hostile().unwrap();
            std::thread::sleep(Duration::from_secs(3));
            let l = log(&s);
            assert!(l.contains("set to Peaceful") && l.rfind("set to Normal") > l.find("set to Peaceful"), "{l}");
            assert!(wait(&s, "記憶體折線有兩筆", |v| v["mem_hist"].as_array().unwrap().len() >= 2)["xmx_mb"] == 256);
        }
        std::thread::sleep(Duration::from_millis(800));
        s.stop().unwrap();
        let snap = wait(&s, "關機", |v| v["state"] == "stopped");
        let l = log(&s);
        assert!(l.contains("There are 1 of a max of 15") && l.contains("中文顯示正常") && l.contains("All dimensions are saved"), "{l}");
        assert!(l.contains("結束代碼 0") && snap["problem"].is_null(), "應該是自己存檔關掉：{l}");
        assert_eq!(l.matches("Loading fake server").count(), 1, "自動重開迴圈不該再開一次：{l}");
        assert_eq!(s.snapshot()["mem_hist"], json!([]));
        println!("開關機通過：{name}（{mode}）");
    }

    // 閃退：沒有 stop 就結束 → 白話說明原因
    let s = make(&base, &jar, "E 找不到 jar", "java -jar nope.jar nogui\r\n", 25615);
    s.start(None).unwrap();
    let snap = wait(&s, "閃退後停下", |v| v["state"] == "stopped" && !v["problem"].is_null());
    assert!(snap["problem"].as_str().unwrap().contains("找不到伺服器主程式"), "{snap}");

    // EULA 沒同意：擋下、不代為同意；連接埠被占用：開機前講白話
    let a = base.join("A 單行");
    fs::remove_file(a.join("eula.txt")).unwrap();
    let s = Server::new(json!({"id": "a", "name": "a", "dir": a.display().to_string()}));
    assert_eq!(s.start(None), Err(Blocked::NeedEula));
    assert!(!a.join("eula.txt").exists());
    mc_hub_core::server::launch::write_eula(&a).unwrap();
    let _busy = std::net::TcpListener::bind(("0.0.0.0", 25611)).unwrap();
    assert!(matches!(s.start(None), Err(Blocked::Error(e)) if e.msg.contains("連接埠 25611 已經被別的程式佔用")));

    // Java 不夠新：要 99 版 → 擋下並附下載網址
    let b = Server::new(json!({"id": "b", "name": "b", "dir": base.join("B 變數路徑").display().to_string()}));
    match b.start(Some(99)) {
        Err(Blocked::Error(e)) => assert!(e.msg.contains("需要 Java 99 以上") && e.help_url.as_ref().is_some_and(|u| u.contains("version=99")), "{e:?}"),
        other => panic!("應該擋下：{other:?}"),
    }
    let _ = fs::remove_dir_all(&base);
}

/// 假 NeoForge 安裝器（legacy/tests/FakeInstaller.java）：裝好 → 寫啟動檔 → 開到 Done → stop 後自動備份；另測失敗、寫檔出錯、逾時。
#[test]
#[ignore]
fn fake_installer_then_auto_backup() {
    use mc_hub_core::create::{inspect_server, install_modloader, INSTALL_TIMEOUT};
    let base = std::env::temp_dir().join(format!("mcinst_{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let classes = base.join("classes");
    fs::create_dir_all(&classes).unwrap();
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("../legacy/tests");
    let srcs = [tests.join("FakeServer.java"), tests.join("FakeInstaller.java")].map(|p| p.display().to_string());
    run("javac", &["-encoding", "UTF-8", "-d", classes.to_str().unwrap(), &srcs[0], &srcs[1]], &base);
    let name = "neoforge-21.1.999-installer.jar";
    run("jar", &["cfe", name, "FakeInstaller", "-C", "classes", "."], &base);
    let java = jdk_tool("java");
    let logs = base.join("平台資料");
    let setup = |dir: &str| {
        let d = base.join(dir);
        fs::create_dir_all(&d).unwrap();
        fs::copy(base.join(name), d.join(name)).unwrap();
        d
    };

    let neo = setup("N 新 NeoForge");
    let mut seen = Vec::new();
    let bat = install_modloader(&neo, &java, &neo.join(name), 1, &mut |l: &str| seen.push(l.to_string()), &logs, INSTALL_TIMEOUT).unwrap();
    assert_eq!(bat, "start.bat");
    assert!(seen.iter().any(|l| l.contains("installed successfully")), "{seen:?}");
    assert!(!neo.join(name).exists(), "安裝器 jar 應該清掉，不然會被誤認成伺服器本體");
    let want = platform::start_script(&java.display().to_string(), "-Xmx1G @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.999/win_args.txt nogui", platform::Os::Win).unwrap();
    assert_eq!(fs::read(neo.join("start.bat")).unwrap(), want);
    assert_eq!(inspect_server(&neo), json!({"version": "1.21.1", "java": 21, "loader": "NeoForge"}));

    // 裝好的伺服器開到 Done，關機後依設定自動備份（閃退不備份由開機測試的 E 案例涵蓋：沒 stop 就不走備份）
    fs::write(neo.join("server.properties"), "server-port=25616\n").unwrap();
    fs::write(neo.join("eula.txt"), "eula=true\n").unwrap();
    fs::create_dir_all(neo.join("world")).unwrap();
    fs::write(neo.join("world/level.dat"), "fake").unwrap();
    let s = Server::new(json!({"id": "n", "name": "n", "dir": neo.display().to_string(), "bat": "start.bat", "auto_backup": true}));
    s.start(Some(21)).unwrap_or_else(|e| panic!("開不了機：{e:?}"));
    wait(&s, "開到 Done", |v| v["state"] == "running");
    assert_eq!(s.backup(), Err("伺服器開著不能備份，請先關機".into()));
    s.stop().unwrap();
    let snap = wait(&s, "關機並備份完", |v| v["state"] == "stopped" && v["backup_busy"] == false && v["backup"]["ok"] == true);
    assert_eq!(snap["backup"]["files"], 1, "{snap}");
    assert!(log(&s).contains("依設定自動備份世界"));

    // 安裝器結束代碼 1：白話說明＋安裝紀錄留在平台資料夾
    let fail = setup("fail 安裝失敗");
    let e = install_modloader(&fail, &java, &fail.join(name), 1, &mut |_: &str| {}, &logs, INSTALL_TIMEOUT).unwrap_err();
    let kept: Vec<_> = fs::read_dir(logs.join("installer-logs")).unwrap().flatten().map(|e| e.path()).collect();
    assert!(e.msg.contains("結束代碼 1") && kept.len() == 1 && e.msg.contains(&kept[0].display().to_string()), "{e:?}");
    assert!(fs::read_to_string(&kept[0]).unwrap().contains("error during installation"));

    // 安裝器寫檔失敗（user_jvm_args.txt 被同名資料夾擋住）
    let blocked = setup("慢 安裝");
    fs::create_dir_all(blocked.join("user_jvm_args.txt")).unwrap();
    let e = install_modloader(&blocked, &java, &blocked.join(name), 1, &mut |_: &str| {}, &logs, INSTALL_TIMEOUT).unwrap_err();
    assert!(e.msg.contains("安裝紀錄已保留"), "{e:?}");

    // 逾時：強制結束安裝器並說明
    let slow = setup("逾時");
    let e = install_modloader(&slow, &java, &slow.join(name), 1, &mut |_: &str| {}, &logs, Duration::from_millis(1)).unwrap_err();
    assert!(e.msg.starts_with("安裝逾時") && e.msg.contains("安裝紀錄已保留"), "{e:?}");
    let _ = fs::remove_dir_all(&base);
}
