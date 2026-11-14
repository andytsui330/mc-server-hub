# 實際連官方下載、從零建伺服器並開到 Done 再 stop（要網路，約 3–5 分鐘）。
# 跑法：py -3 tests\test_real_create.py <測試資料夾> [exe路徑]
# Mac：python3 tests/test_real_create.py <測試資料夾> [.app 路徑]
# 指定版本與載入器：環境變數 MC_REAL_VERSION=1.21.1 MC_REAL_LOADERS=fabric,neoforge（預設：最新可用版，Fabric＋原版）
import importlib.util, json, os, re, shutil, socket, subprocess, sys, tempfile, time
from pathlib import Path
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parent.parent
PORT = 47894
URL = f"http://127.0.0.1:{PORT}"
spec = importlib.util.spec_from_file_location("hub", ROOT / "src" / "hub.py"); hub = importlib.util.module_from_spec(spec); spec.loader.exec_module(hub)
parent = Path(sys.argv[1])
exe = sys.argv[2] if len(sys.argv) > 2 else None
config = Path(tempfile.mkdtemp(prefix="mcreal_")) / "c.json"
args = ["--config", str(config), "--port", str(PORT), "--no-window"]
if exe:
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith("PY")}
    if hub.OS == "mac":   # 跟從 Finder 雙擊一樣的精簡 PATH；平台要自己找到 JDK
        env["PATH"] = "/usr/bin:/bin:/usr/sbin:/sbin"
        exe = str(Path(exe) / "Contents" / "MacOS" / "MC開服平台") if exe.endswith(".app") else exe
    else:
        env["PATH"] = os.pathsep.join([os.environ["SystemRoot"] + r"\System32", os.environ["SystemRoot"], str(Path(shutil.which("java")).parent)])
    proc = subprocess.Popen([exe] + args, env=env)
else:
    proc = subprocess.Popen([sys.executable, str(ROOT / "src" / "hub.py")] + args)

# 把常用埠佔住，驗證建服會自動避開（電腦上原本就有伺服器開著時，本來就佔住了）
blockers = []
for p in (25565, 25566):
    s = socket.socket()
    try:
        s.bind(("0.0.0.0", p)); s.listen(); blockers.append(s)
    except OSError:
        s.close()
try:
    for _ in range(40):
        try:
            page = urlopen(URL + "/", timeout=1).read().decode(); break
        except OSError:
            time.sleep(0.5)
    token = re.search(r'TOKEN = "([^"]+)"', page).group(1)

    def call(path, body=None):
        return json.load(urlopen(Request(URL + path, data=json.dumps(body).encode() if body is not None else None,
                                         headers={"X-Hub-Token": token, "Content-Type": "application/json"}), timeout=60))

    def job_wait():
        last = None
        while True:
            j = call("/api/state")["job"]
            if j["msg"] != last:
                print("  ", j["msg"]); last = j["msg"]
            if j["state"] != "running":
                return j
            time.sleep(1)

    def server(sid):
        return next(s for s in call("/api/state")["servers"] if s["id"] == sid)

    def wait(sid, pred, secs):
        end = time.time() + secs
        while time.time() < end:
            s = server(sid)
            if pred(s):
                return s
            time.sleep(1)
        raise AssertionError(f"逾時：{server(sid)}\n" + "\n".join(t for _, t in call(f"/api/log?id={sid}&since=0")["lines"][-40:]))

    v = call("/api/mc/versions")
    assert v["versions"] and all(x["id"] for x in v["versions"]), v
    print("正式版清單", len(v["versions"]), "個，最新", v["versions"][0]["id"], "；電腦記憶體", v["ram_total_gb"], "GB")
    chosen = os.environ.get("MC_REAL_VERSION")
    loaders = [l for l in os.environ.get("MC_REAL_LOADERS", "fabric,vanilla").split(",") if l]
    for x in ([] if chosen else v["versions"]):
        st = call("/api/mc/java?version=" + x["id"])
        print("  ", x["id"], "需要 Java", st["need"], "可用" if st["ok"] else "不足")
        if not st["ok"]:
            # 真的走一次「Java 不夠」：要在下載前就停、不留下資料夾
            name = "Java不足測試"
            assert call("/api/create", {"name": name, "version": x["id"], "loader": "vanilla", "ram_gb": 2, "parent": str(parent), "eula": True}) == {"ok": True}
            j = job_wait()
            assert j["state"] == "error" and "Java" in j["error"] and "adoptium" in j["help_url"], j
            assert not (parent / name).exists()
            print("   Java 不足時擋下：", j["error"])
            continue
        if x["fabric"]:
            chosen = x["id"]; break
    assert chosen, "找不到可測的版本"

    if chosen:
        x = next(x for x in v["versions"] if x["id"] == chosen)
        for l in loaders:   # fabric 是 True／False；forge、neoforge 是 {"ok": …, "why": …}
            ok = x.get(l, True)
            assert ok is True or (isinstance(ok, dict) and ok["ok"]), (l, x)
        assert call("/api/mc/java?version=" + chosen)["ok"], "這台電腦的 Java 不夠開這個版本"
    for loader, name in ((l, "測試 " + {"fabric": "Fabric", "vanilla": "原版", "forge": "Forge", "neoforge": "NeoForge"}[l]) for l in loaders):
        print(f"== 建立 {name}（{chosen}）")
        assert call("/api/create", {"name": name, "version": chosen, "loader": loader, "ram_gb": 2, "parent": str(parent), "eula": True}) == {"ok": True}
        j = job_wait()
        assert j["state"] == "done" and not j["error"], j
        sid, d = j["id"], parent / name
        script = d / hub.start_script_name()
        print(start := script.read_text(encoding=hub.TEXT_ENC))
        if hub.OS != "win":
            assert os.access(script, os.X_OK), "start.sh 不能執行"
        assert hub.eula_agreed(d) and "-Xmx2G" in start
        port = hub.read_port(d)
        assert port >= 25567 or len(blockers) < 2, port
        print("   連接埠", port)
        t0 = time.time()
        s = wait(sid, lambda s: s["state"] == "running" and s["online"] == 0, 420)
        print(f"   開到 Done，{time.time() - t0:.0f} 秒；記憶體 {s['mem_mb']} MB／{s['xmx_mb']} MB；版本標籤 {s['version']}")
        assert s["xmx_mb"] == 2048 and s["mem_mb"] > 100, s
        if loader != "vanilla":
            assert s["modded"], s
        if loader == "fabric":
            assert (d / "mods").is_dir()
        assert "伺服器開著" in call("/api/backup", {"id": sid})["error"]
        assert call("/api/stop", {"id": sid}) == {"ok": True}
        s = wait(sid, lambda s: s["state"] == "stopped", 120)
        log = [t for _, t in call(f"/api/log?id={sid}&since=0")["lines"]]
        text = "\n".join(log)
        assert re.search(r"Done \(", text) and "結束代碼 0" in text and s["problem"] is None, text[-2000:]
        print("   ", next(l for l in log if "Done (" in l))
        print("   ", log[-1])
        assert call("/api/backup", {"id": sid}) == {"ok": True}
        s = wait(sid, lambda s: not s["backup_busy"], 300)
        assert s["backup"]["ok"], s["backup"]
        src = sum(hub.tree_stats(d / n)[0] for n in ("world",) if (d / n).is_dir())
        print("   ", s["backup"]["text"].replace("\n", " "))
        assert s["backup"]["files"] >= src

    assert call("/api/quit", {}) == {"ok": True}
    proc.wait(timeout=10)
    print("全部通過")
finally:
    for s in blockers:
        s.close()
    if proc.poll() is None:
        proc.kill()
