# Mac 冒煙測試：只用假伺服器（tests/FakeServer.java）與暫存資料夾，不碰任何真世界、不連網。
# 驗：start.sh 開機到 Done → 記憶體有數字 → stop 正常關機；強制結束整組程序都不見；
#     「加入現有資料夾」認得 start.sh；改記憶體後 start.sh 仍可執行；畫面上不出現 Windows 用語。
# 跑法（要有 JDK 21，javac 要找得到）：python3 tests/test_mac.py                 測原始碼
#                                      python3 tests/test_mac.py <.app 路徑>    測打包好的 .app（環境變數拿掉 Python）
# 「選資料夾視窗」「開啟資料夾」會跳出真的視窗，要人看螢幕：python3 tests/test_mac.py --gui
import importlib.util, json, os, re, shutil, signal, stat, subprocess, sys, tempfile, time, zipfile
from pathlib import Path
from urllib.request import Request, urlopen

assert sys.platform == "darwin", "這支測試只在 Mac 跑；Windows 用 tests/test_hub.py"
HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
PORT = 47895
URL = f"http://127.0.0.1:{PORT}"
spec = importlib.util.spec_from_file_location("hub", ROOT / "src" / "hub.py"); hub = importlib.util.module_from_spec(spec); spec.loader.exec_module(hub)
assert hub.OS == "mac" and hub.SCRIPT == "start.sh" and hub.EDITOR == "「文字編輯」"

GUI = "--gui" in sys.argv
args = [a for a in sys.argv[1:] if a != "--gui"]
APP = Path(args[0]) if args else None

# ---------- 找 JDK（/usr/bin/java 只是轉接頭，沒裝 Java 時會跳安裝提示） ----------
javas = [(m, p) for m, p in hub.installed_javas() if m >= 21]
assert javas, "找不到 Java 21 以上，請先安裝 JDK 21"
JAVA = min(javas)[1]
JAVAC = str(Path(JAVA).parent / "javac")
assert os.path.isfile(JAVAC), f"{JAVA} 旁邊沒有 javac，要裝 JDK（不是只有 JRE）"
print("用", JAVA)

# ---------- 系統小工具：真的 ps／sysctl 讀得到 ----------
assert hub.total_ram_gb() >= 4
assert hub.mac_arch() in ("x64", "aarch64")
assert hub.mem_mb(os.getpid()) and hub.tree_mem_mb(os.getpid()) >= hub.mem_mb(os.getpid())

# ---------- 編一支假伺服器 jar ----------
work = Path(tempfile.mkdtemp(prefix="mcjar_"))
subprocess.run([JAVAC, "-encoding", "UTF-8", "-d", str(work), str(HERE / "FakeServer.java")], check=True)
jar = work / "server.jar"
with zipfile.ZipFile(jar, "w") as z:
    z.writestr("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\r\nMain-Class: FakeServer\r\n\r\n")
    for c in work.glob("*.class"):
        z.write(c, c.name)

base = Path(tempfile.mkdtemp(prefix="mcfake_")) / "假 伺服器"   # 故意有中文與空白
FORMS = {
    # 平台自己寫的 start.sh（建服精靈產生的就是這個樣子）
    "A 平台寫的": (hub.start_script(JAVA, "-Xms64M -Xmx256M -jar server.jar nogui"), "direct", 25611),
    # 使用者自己寫、java 不寫路徑
    "B 自己寫的": (b"#!/bin/sh\ncd \"$(dirname \"$0\")\"\njava -Xmx256M -jar server.jar nogui\n", "direct", 25612),
    # 自動重開迴圈：平台看不懂，交給 sh 執行；stop 後會再開一次，只能靠強制結束
    "C 自動重開": (f'#!/bin/sh\nwhile true; do\n  "{JAVA}" -Xmx256M -jar server.jar nogui\n  sleep 1\ndone\n'.encode(), "bat", 25613),
}
for name, (sh, mode, port) in FORMS.items():
    d = base / name
    d.mkdir(parents=True)
    (d / "start.sh").write_bytes(sh)
    (d / "start.sh").chmod(0o755)
    (d / "server.properties").write_text(f"server-port={port}\n", encoding="utf-8")
    (d / "eula.txt").write_text("eula=true\n", encoding="utf-8")
    shutil.copy(jar, d / "server.jar")
    assert hub.find_bat(d) == "start.sh", name
    assert hub.parse_launch(d, "start.sh").mode == mode, name


def java_pids(folder):
    """找還活著、在這個資料夾跑的 java（用 lsof 看程序的目前資料夾）。"""
    out = subprocess.run(["pgrep", "-f", "server.jar nogui"], capture_output=True, text=True).stdout.split()
    alive = []
    for pid in out:
        cwd = subprocess.run(["lsof", "-a", "-p", pid, "-d", "cwd", "-Fn"], capture_output=True, text=True).stdout
        if f"n{os.path.realpath(folder)}" in cwd.splitlines():   # /var 其實是 /private/var，lsof 回報真實路徑
            alive.append(int(pid))
    return alive


# ---------- 程序群組：直接開一台再整組殺掉（不經過平台，快速確認 killpg 行為） ----------
d = base / "C 自動重開"
p = hub.popen_server("/bin/sh ./start.sh", str(d), stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
for _ in range(50):
    if java_pids(d):
        break
    time.sleep(0.2)
assert java_pids(d), "sh 底下的 java 沒開起來"
assert os.getpgid(p.pid) == p.pid, "伺服器沒有開在自己的程序群組"
hub.kill_tree(p.pid)
p.wait(timeout=10)
time.sleep(0.5)
assert not java_pids(d), "強制結束後 java 還活著"
print("程序群組整組結束：通過")

# ---------- 啟動平台 ----------
config_dir = Path(tempfile.mkdtemp(prefix="mccfg_"))
if APP:
    exe = APP / "Contents" / "MacOS" / "MC開服平台"
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith("PY")}
    env["PATH"] = "/usr/bin:/bin:/usr/sbin:/sbin"   # 跟從 Finder 雙擊開的 .app 一樣
    cmd = [str(exe)]
else:
    env, cmd = None, [sys.executable, str(ROOT / "src" / "hub.py")]
proc = subprocess.Popen(cmd + ["--config", str(config_dir / "c.json"), "--port", str(PORT), "--no-window"], env=env)
try:
    for _ in range(60):
        try:
            page = urlopen(URL + "/", timeout=1).read().decode(); break
        except OSError:
            time.sleep(0.5)
    token = re.search(r'TOKEN = "([^"]+)"', page).group(1)
    assert f"v{hub.VERSION}" in page and "{{" not in page and "@font-face" in page
    for win_word in ("start.bat", "記事本", "MC開服平台.exe"):
        assert win_word not in page, f"Mac 畫面出現 Windows 用語：{win_word}"

    def call(path, body=None):
        h = {"X-Hub-Token": token, "Content-Type": "application/json"}
        return json.load(urlopen(Request(URL + path, data=json.dumps(body).encode() if body is not None else None, headers=h), timeout=15))

    def servers():
        return {s["id"]: s for s in call("/api/state")["servers"]}

    def log_of(sid):
        return "\n".join(t for _, t in call(f"/api/log?id={sid}&since=0")["lines"])

    def wait(sid, pred, secs=60):
        end = time.time() + secs
        while time.time() < end:
            s = servers()[sid]
            if pred(s):
                return s
            time.sleep(0.4)
        raise AssertionError(f"逾時：{servers()[sid]}\n" + log_of(sid))

    assert call("/api/state")["servers"] == []
    empty = base / "空資料夾"; empty.mkdir()
    assert "找不到 start.sh" in call("/api/add", {"dir": str(empty)})["error"]

    ids = {}
    # 開機到 Done → 有人在線、記憶體有數字 → stop 自己存檔關機（結束代碼 0）
    for name in ("A 平台寫的", "B 自己寫的"):
        sid = ids[name] = call("/api/add", {"dir": str(base / name)})["id"]
        s = servers()[sid]
        assert s["port"] == FORMS[name][2] and s["state"] == "stopped", s
        assert call("/api/start", {"id": sid}) == {"ok": True}, (name, log_of(sid))
        s = wait(sid, lambda s: s["state"] == "running" and s["online"] == 1)
        assert s["names"] == ["Tester"], s
        assert s["mem_mb"] and s["mem_mb"] > 20, s   # 要量到真正的 Java
        assert call("/api/cmd", {"id": sid, "cmd": "/list"}) == {"ok": True}
        time.sleep(0.8)
        assert call("/api/stop", {"id": sid}) == {"ok": True}
        s = wait(sid, lambda s: s["state"] == "stopped")
        log = log_of(sid)
        assert "There are 1 of a max of 15" in log and "中文顯示正常" in log and "All dimensions are saved" in log, log
        assert "結束代碼 0" in log and s["problem"] is None, log
        assert not java_pids(base / name)
        print("開關機通過：", name)

    # 改記憶體：-Xmx 換掉、-Xms 不動、start.sh 仍可執行，再開一次還是開得起來
    a = base / "A 平台寫的" / "start.sh"
    assert call("/api/server-settings", {"id": ids["A 平台寫的"], "ram_gb": 2}) == {"ok": True}
    assert b"-Xms64M -Xmx2G" in a.read_bytes() and os.stat(a).st_mode & stat.S_IXUSR, oct(os.stat(a).st_mode)
    assert call(f"/api/info?id={ids['A 平台寫的']}")["ram_gb"] == 2
    print("改記憶體保留可執行權限：通過")

    # 強制結束：自動重開迴圈 stop 後又開一次 → 等滿寬限時間 → 強制結束 → sh 和 java 都不見
    name = "C 自動重開"
    sid = ids[name] = call("/api/add", {"dir": str(base / name)})["id"]
    assert call("/api/start", {"id": sid}) == {"ok": True}
    wait(sid, lambda s: s["state"] == "running" and s["online"] == 1)
    assert call("/api/stop", {"id": sid}) == {"ok": True}
    assert "要等" in call("/api/force", {"id": sid})["error"]
    print(f"等 {hub.STOP_GRACE_S} 秒寬限時間才能強制結束…")
    time.sleep(hub.STOP_GRACE_S + 2)
    assert java_pids(base / name), "迴圈應該把伺服器又開起來了（不然測不到強制結束）"
    assert call("/api/force", {"id": sid}) == {"ok": True}
    wait(sid, lambda s: s["state"] == "stopped", 20)
    time.sleep(1)
    assert not java_pids(base / name), "強制結束後 java 還活著"
    print("平台強制結束整組關掉：通過")

    if GUI:   # 要人看螢幕：會跳出「選擇資料夾」視窗與 Finder
        print("等一下會跳出「選擇資料夾」視窗，請隨便選一個資料夾按「選擇」")
        r = call("/api/pick-folder", {})
        print("選到：", r)
        assert r.get("path", "").startswith("/"), r
        r = call("/api/open", {"id": ids["A 平台寫的"]})
        print("開啟資料夾：", r, "→ 請確認 Finder 有開出「A 平台寫的」")

    assert call("/api/quit", {}) == {"ok": True}
    proc.wait(timeout=10)
    assert (config_dir / "hub.log").exists()
    print("全部通過")
finally:
    if proc.poll() is None:
        proc.kill()
    for name in FORMS:   # 保險：不留任何假伺服器
        for pid in java_pids(base / name):
            os.kill(pid, signal.SIGKILL)
