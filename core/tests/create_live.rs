// 真的連官方下載、從零建伺服器並開到 Done 再 stop（要網路與 JDK，約 3–5 分鐘；1.x 的 tests/test_real_create.py）。
// 跑法：cargo test -p mc-hub-core --test create_live -- --ignored --nocapture
// 指定版本與載入器：環境變數 MC_REAL_VERSION=1.21.1 MC_REAL_LOADERS=fabric,neoforge（預設：Fabric 支援的最新正式版，原版＋Fabric）
use mc_hub_core::create::{self, Catalog, Progress};
use mc_hub_core::server::Server;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[test]
#[ignore]
fn real_create_to_done() {
    let parent = std::env::temp_dir().join(format!("mcreal_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&parent);
    let cat = Catalog::default();
    let list = cat.mc_versions(&parent).unwrap();
    let version = std::env::var("MC_REAL_VERSION").unwrap_or_else(|_| {
        let v = list["versions"].as_array().unwrap().iter().find(|v| v["fabric"] == true).unwrap();
        v["id"].as_str().unwrap().to_string()
    });
    let loaders = std::env::var("MC_REAL_LOADERS").unwrap_or_else(|_| "vanilla,fabric".into());
    // 把常用埠佔住，驗證建服會自動避開
    let _held: Vec<_> = [25565, 25566].iter().filter_map(|p| std::net::TcpListener::bind(("0.0.0.0", *p)).ok()).collect();
    let mut used = Vec::new();
    for loader in loaders.split(',') {
        println!("== {version} {loader}");
        let req = json!({"eula": true, "name": format!("真 {loader}"), "loader": loader, "ram_gb": 2, "parent": parent.display().to_string(), "version": version});
        let plan = create::plan(&req).unwrap();
        let job = Mutex::new(Progress::default());
        let t = Instant::now();
        let made = create::create(&plan, &cat, &job, &parent.join("平台資料"), &used).unwrap_or_else(|e| panic!("{loader} 建立失敗：{e}"));
        println!("   建好（{} 秒）：{made:?}", t.elapsed().as_secs());
        assert!(made.port > 25566 && !used.contains(&made.port));
        used.push(made.port);
        let s = Server::new(json!({"id": loader, "name": plan.name, "dir": plan.target.display().to_string(), "bat": made.bat, "version": made.label, "loader": made.loader}));
        s.start(Some(made.java_need)).unwrap_or_else(|e| panic!("{loader} 開不了機：{e:?}"));
        let snap = wait(&s, |v| v["state"] == "running" || v["state"] == "stopped");
        assert_eq!(snap["state"], "running", "{loader} 沒開到 Done：{}", log(&s));
        println!("   開到 Done（{} 秒）", t.elapsed().as_secs());
        s.stop().unwrap();
        let snap = wait(&s, |v| v["state"] == "stopped");
        assert!(snap["problem"].is_null() && log(&s).contains("結束代碼 0"), "{}", log(&s));
        assert_eq!(mc_hub_core::create::inspect_server(&plan.target)["version"], json!(version));
    }
    let _ = std::fs::remove_dir_all(&parent);
}

fn wait(s: &Server, ok: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(600);
    loop {
        s.poll_status();
        let snap = s.snapshot();
        if ok(&snap) || Instant::now() > end {
            return snap;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn log(s: &Server) -> String {
    s.logs_since(0).1.into_iter().map(|l| l.1).collect::<Vec<_>>().join("\n")
}
