"""兩版同組假服對照，保留全部殘留；先 cargo build -p mc-hub-core --example parity。
Windows: 設 MCHUB_DATA_DIR 為 TEMP 下不存在的絕對路徑，再 py -3 tests/parity.py。
不跑正式下載，不碰真 APPDATA，不接受已存在的測試根目錄。
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import traceback
import zipfile

REPO = Path(__file__).resolve().parents[1]


def main():
    requested = os.environ.get("MCHUB_DATA_DIR", "")
    root = Path(requested).resolve()
    temp = Path(tempfile.gettempdir()).resolve()
    assert requested and Path(requested).is_absolute() and root != temp and root.is_relative_to(temp)
    assert not root.exists(), "MCHUB_DATA_DIR 必須是 TEMP 下全新資料夾；測試不刪檔"
    root.mkdir(parents=True)
    (root / "parity-marker").write_text("mc-hub parity test; preserve evidence", encoding="utf-8")
    appdata = Path(os.environ["APPDATA"]) / "MCServerHub"

    def fingerprints():
        return {n: hashlib.sha256((appdata / n).read_bytes()).hexdigest() if (appdata / n).is_file() else None
                for n in ("hub_config.json", "config.json", "hub_config.1x-backup.json")}

    before = fingerprints()
    report = {"root": str(root), "appdata_before": before, "checks": [], "differences": [], "limitations": [
        "未驗原生選夾、拖放、剪貼簿、外部連結、音效及兩版 UI；由 UI/桌面測試補充",
        "四載入器完整下載→建服工作未跑；沒有測試注入點，不更動正式官方網域檢查",
        "Mac 只驗 fixture 解析，未在 Mac 真機啟動；強制結束 120 秒等待分支未跑",
    ]}
    log = (root / "transcript.jsonl").open("w", encoding="utf-8")
    rust = None
    legacy = None
    external = None

    def record(name, out=None):
        report["checks"].append(name)
        print("PASS", name, flush=True)
        log.write(json.dumps({"check": name, "out": out}, ensure_ascii=False) + "\n")
        log.flush()

    def same(name, left, right):
        assert left == right, f"{name}: Python={left!r}; Rust={right!r}"
        record(name, left)

    def rpc(op, **kwargs):
        v = {"op": op, **kwargs}
        rust.stdin.write(json.dumps(v, ensure_ascii=False) + "\n")
        rust.stdin.flush()
        line = rust.stdout.readline()
        assert line, f"Rust runner ended: {rust.poll()}"
        out = json.loads(line)
        log.write(json.dumps({"request": v, "response": out}, ensure_ascii=False) + "\n")
        log.flush()
        return out

    def pycall(path, body):
        h = legacy
        if path == "/api/state":
            return {"servers": [s.snapshot() for s in h.all_servers()], "job": h.job, "update": h.update}
        if path == "/api/settings":
            return h.save_settings(body) if body else h.settings()
        if path == "/api/add":
            result = h.add_existing(body.get("dir"), body.get("name"))
        elif path == "/api/create":
            result = h.start_create(body)
        else:
            s = h.servers.get(body.get("id"))
            if path == "/api/commands":
                return {"fields": hub.FIELDS, "commands": hub.commands_for(s.cfg.get("version", "") if s else "", h.config.get("custom_commands", []))}
            assert s
            if path == "/api/log":
                seq, lines = s.logs_since(body.get("since", 0))
                return {"seq": seq, "lines": lines}
            if path == "/api/info":
                return h.server_info(s)
            if path in ("/api/start", "/api/stop", "/api/force", "/api/backup"):
                result = getattr(s, path.rsplit("/", 1)[1].replace("force", "force_kill"))()
            elif path == "/api/remove": result = h.remove(s)
            elif path == "/api/eula":
                if body.get("agree") is not True: result = "要勾選同意才能繼續"
                else: hub.write_eula(s.dir); result = None
            elif path == "/api/run": result = h.run_command(s, body)
            elif path == "/api/server-settings": result = h.server_settings(s, body)
            elif path == "/api/address":
                s.cfg["address"] = str(body.get("address", "")).strip()[:200]
                h.save_config(); result = None
            elif path == "/api/cmd":
                raw = str(body.get("cmd", "")); text = raw.strip()
                result = "指令不能換行" if "\n" in raw or "\r" in raw else s.send(text) if text else "指令是空的"
                if not result: s.add_log(f"> {text}")
            else: raise AssertionError(path)
        return {"ok": True} if result is None else result if isinstance(result, dict) else {"error": result}

    def poll():
        for s in legacy.all_servers(): s.poll_status()
        rpc("poll")

    def snap(side, sid):
        state = pycall("/api/state", {}) if side == "python" else rpc("call", path="/api/state", body={})
        return next(s for s in state["servers"] if s["id"] == sid)

    def wait(side, sid, predicate, seconds=40):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if side == "python": legacy.servers[sid].poll_status()
            else: rpc("poll")
            s = snap(side, sid)
            if predicate(s): return s
            time.sleep(.15)
        raise AssertionError(f"wait timeout {side}: {s}")

    def call(side, path, **body):
        out = pycall(path, body) if side == "python" else rpc("call", path=path, body=body)
        log.write(json.dumps({"side": side, "path": path, "body": body, "response": out}, ensure_ascii=False) + "\n")
        log.flush()
        return out

    def port():
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0)); return s.getsockname()[1]

    def directory(side, name, bat, jar, eula=True, chosen_port=None):
        d = root / side / name
        d.mkdir(parents=True)
        (d / "start.bat").write_bytes(bat.encode("mbcs"))
        (d / "server.properties").write_text(f"server-port={chosen_port or port()}\nlevel-seed=123456789\n", encoding="utf-8")
        if eula: (d / "eula.txt").write_text("eula=true\n", encoding="utf-8")
        if jar: shutil.copyfile(jar, d / "server.jar")
        return d

    try:
        spec = importlib.util.spec_from_file_location("parity_hub", REPO / "legacy/src/hub.py")
        hub = importlib.util.module_from_spec(spec); spec.loader.exec_module(hub)
        assert hub.OS == "win", "this live harness is Windows-only"
        for side in ("python", "rust"):
            (root / side / "config").mkdir(parents=True)
        imported_dir = root / "匯入 伺服器"; imported_dir.mkdir()
        (imported_dir / "run.bat").write_bytes(b"java -Xmx1G -jar server.jar nogui\r\n")
        (imported_dir / "server.properties").write_text(f"server-port={port()}\n", encoding="utf-8")
        imported = {"id": "imported", "name": "1.x 匯入的伺服器", "dir": str(imported_dir), "bat": "run.bat",
                    "version": "1.21.1 · Fabric", "loader": "Fabric", "java_need": 21, "address": "play.example.com",
                    "accent": "#5cc8ff", "icon": "pig", "color": "#5fd39a", "auto_backup": True,
                    "future_server": {"keep": "未知欄位也要保留"}}
        old_config = {"servers": [imported], "custom_commands": [{"id": "my_test", "name": "測試筆記", "tpl": "say <msg>"}],
                      "links": [{"name": "Wiki", "url": "https://minecraft.wiki/"}], "join_template": "加入 <版本> <連線網址>",
                      "update_check": False, "future": {"keep": [1, "中文", None]}}
        original = json.dumps(old_config, ensure_ascii=False, indent=1).encode("utf-8")
        for side in ("python", "rust"):
            (root / side / "config/hub_config.json").write_bytes(original)
        legacy = hub.Hub(root / "python/config/hub_config.json")
        rust_stderr = (root / "rust-stderr.log").open("w", encoding="utf-8")
        rust = subprocess.Popen([str(REPO / "target/debug/examples/parity.exe")], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=rust_stderr, text=True, encoding="utf-8", env=os.environ.copy(), creationflags=subprocess.CREATE_NO_WINDOW)
        same("完整 1.x 設定匯入與預設視圖", pycall("/api/settings", {}), rpc("call", path="/api/settings", body={}))
        same("匯入 JSON 完整含未知欄位", old_config, json.loads((root / "rust/config/config.json").read_text(encoding="utf-8")))
        assert (root / "rust/config/hub_config.json").read_bytes() == original
        assert (root / "rust/config/hub_config.1x-backup.json").read_bytes() == original
        record("1.x 原檔未動與逐 byte 備份")
        visible = ["id", "name", "version", "dir", "icon", "color", "address", "state"]
        states = [pycall("/api/state", {})["servers"][0], rpc("call", path="/api/state", body={})["servers"][0]]
        same("1.x 伺服器清單完整匯入並可見", *[{k: s[k] for k in visible} for s in states])
        # Windows 的 PATH 查找可能回傳 java.EXE；大小寫不同仍是同一個檔案。
        normalize_java = lambda found: sorted([[v, os.path.normcase(str(p))] for v, p in found])
        same("本機 Java 偵測（Windows 路徑不分大小寫）", normalize_java(hub.installed_javas()), normalize_java(rpc("java")))

        def pyfixture(c, d):
            a, fn = c["args"], c["fn"]
            try:
                if fn == "parse_launch":
                    (d / a["file"]).write_bytes(a["text"].encode("utf-8")); r = hub.parse_launch(d, a["file"])
                    out = {"cmd": r.cmd.replace(str(d), "<DIR>"), "java": r.java and r.java.replace(str(d), "<DIR>"), "mode": r.mode}
                elif fn == "start_script": out = hub.start_script(a["java"], a["args"], a["os"]).decode("utf-8")
                elif fn == "newer_version": out = hub.newer_version(lambda: a["body"].encode())
                elif fn == "commands_count": out = len(hub.COMMANDS)
                elif fn == "commands_for": out = hub.commands_for(a["label"])
                elif fn == "build_command": out = hub.build_command(next(x for x in hub.COMMANDS if x["id"] == a["id"]), a["values"], a.get("confirm"), a["label"])
                elif fn in ("set_ram", "read_ram_gb"):
                    script = d / "ram.bat"; script.write_bytes(a["text"].encode("utf-8"))
                    if fn == "set_ram":
                        hub.set_ram(script, a["gb"]); assert Path(str(script)+".bak").read_bytes() == a["text"].encode("utf-8")
                        out = script.read_bytes().decode("utf-8")
                    else: out = hub.read_ram_gb(script)
                else: out = getattr(hub, fn)(**a)
                return {"out": json.loads(json.dumps(out, ensure_ascii=False))}
            except hub.UserError as e: return {"error": str(e)}

        fixture_dir = root / "python/fixtures"; fixture_dir.mkdir()
        fixtures = 0
        for f in sorted((REPO / "tests/fixtures").glob("*.json")):
            for c in json.loads(f.read_text(encoding="utf-8"))["cases"]:
                left, right = pyfixture(c, fixture_dir), rpc("fixture", case=c)
                # 第5步已核准的文字差異，只替換這個精確字串，不改 fixture 正本。
                normalized = json.loads(json.dumps(left, ensure_ascii=False).replace("1.1.0 不支援", "目前不支援"))
                want = {"error": c["error"]} if "error" in c else {"out": c["out"]}
                assert normalized == right == want, f"fixture {c['fn']} {c['args']}: {left} / {right} / {want}"
                if left != right: report["differences"].append({"kind": "approved wording", "fn": c["fn"], "python": left, "rust": right})
                fixtures += 1
        record(f"{fixtures} 筆 fixture：現跑兩版與標準答案三方核對")

        # javac 只編一次；兩版拿完全相同 jar，所有 class 都留供查證。
        classes = root / "classes"; classes.mkdir()
        java = Path(shutil.which("java")); java_bin = java.parent
        props = subprocess.run([str(java), "-XshowSettings:properties", "-version"], capture_output=True, text=True, encoding="utf-8", errors="replace", creationflags=subprocess.CREATE_NO_WINDOW)
        import re
        java_bin = Path(re.search(r"java.home\s*=\s*(.+)", props.stderr).group(1).strip()) / "bin"
        subprocess.run([str(java_bin / "javac.exe"), "-encoding", "UTF-8", "-d", str(classes), str(REPO / "legacy/tests/FakeServer.java"), str(REPO / "legacy/tests/FakeInstaller.java")], check=True, creationflags=subprocess.CREATE_NO_WINDOW)
        def jar(name, main_class, version=None):
            target = root / name
            with zipfile.ZipFile(target, "w") as z:
                z.writestr("META-INF/MANIFEST.MF", f"Manifest-Version: 1.0\r\nMain-Class: {main_class}\r\n\r\n")
                for c in classes.glob("*.class"): z.write(c, c.name)
                if version: z.writestr("version.json", json.dumps(version))
            return target
        plain = jar("server.jar", "FakeServer")
        needs99 = jar("needs99.jar", "FakeServer", {"id": "9.9", "java_version": 99})
        installer = jar("neoforge-21.1.999-installer.jar", "FakeInstaller")
        java = java_bin / "java.exe"
        forms = [
            ("A 單行", "@echo off\njava -Xmx256M -jar server.jar nogui\npause\n"),
            ("B 變數路徑", f'@echo off\ncd /d "%~dp0"\nset "JAVA_EXE={java}"\nset RAM=256M\n"%JAVA_EXE%" -Xmx%RAM% ^\n -Xms64M -jar server.jar nogui %*\npause\n'),
            ("C 自動重開", ":start\r\njava -Xmx256M -jar server.jar nogui\r\ntimeout /t 2\r\ngoto start\r\n"),
            ("D 複雜bat", "@echo off\r\nif exist server.jar (\r\n java -Xmx256M -jar server.jar nogui\r\n) else (\r\n echo 找不到\r\n)\r\npause\r\n"),
        ]
        for name, bat in forms:
            outcomes = []
            for side in ("python", "rust"):
                d = directory(side, name, bat, plain)
                for world in ("world", "world_nether", "world_the_end"):
                    (d / world).mkdir(); (d / world / "level.dat").write_bytes(b"same fake world")
                sid = call(side, "/api/add", dir=str(d))["id"]
                assert "已經在清單" in call(side, "/api/add", dir=str(d))["error"]
                assert call(side, "/api/server-settings", id=sid, auto_backup=True) == {"ok": True}
                assert call(side, "/api/start", id=sid) == {"ok": True}
                s = wait(side, sid, lambda s: s["state"] == "running" and s["online"] == 1 and len(s["mem_hist"]) >= 2)
                assert s["names"] == ["Tester"] and s["max"] == 15 and s["mem_mb"] > 20
                assert s["xmx_mb"] == (None if name.startswith("D") else 256)
                assert "請先關機" in call(side, "/api/backup", id=sid)["error"]
                assert call(side, "/api/force", id=sid).get("error")
                assert call(side, "/api/cmd", id=sid, cmd="say hi\nstop")["error"] == "指令不能換行"
                assert call(side, "/api/cmd", id=sid, cmd="/list") == {"ok": True}
                if name.startswith("A"):
                    for confirm in (None, True):
                        assert "強確認" in call(side, "/api/run", id=sid, cmd="op", values={"player": "Tester"}, confirm=confirm)["error"]
                    assert call(side, "/api/run", id=sid, cmd="op", values={"player": " Tester "}, confirm="strong")["sent"] == "op Tester"
                    assert call(side, "/api/run", id=sid, cmd="kill_hostile", confirm="strong") == {"ok": True}
                    time.sleep(2.3)
                assert call(side, "/api/stop", id=sid) == {"ok": True}
                s = wait(side, sid, lambda s: s["state"] == "stopped" and not s["backup_busy"] and s["backup"] is not None)
                assert s["problem"] is None and s["mem_hist"] == []
                text = "\n".join(t for _, t in call(side, "/api/log", id=sid)["lines"])
                assert "中文顯示正常" in text and "All dimensions are saved" in text and "結束代碼 0" in text and text.count("Loading fake server") == 1
                assert "There are 1 of a max of 15" in text
                if name.startswith("A"): assert "set to Peaceful" in text and text.rfind("set to Normal") > text.find("set to Peaceful")
                backup = Path(s["backup"]["path"])
                for world in ("world", "world_nether", "world_the_end"):
                    assert (backup / world / "level.dat").read_bytes() == (d / world / "level.dat").read_bytes()
                outcomes.append({"state": s["state"], "problem": s["problem"], "backup_files": s["backup"]["files"], "backup_bytes": s["backup"]["bytes"], "names_running": ["Tester"], "saved": True})
                changed = call(side, "/api/server-settings", id=sid, ram_gb=1, icon="pig", color="#5fd39a")
                if name.startswith("B"):
                    # -Xmx%RAM% 不是可安全改寫的常數；兩版都應保留原檔並說明。
                    assert "找不到剛好一個 -Xmx" in changed["error"]
                    assert (d / "start.bat").read_bytes() == bat.encode("mbcs")
                    assert not (d / "start.bat.bak").exists()
                else:
                    assert changed == {"ok": True}, changed
                    assert (d / "start.bat.bak").read_bytes() == bat.encode("mbcs")
                outcomes[-1]["ram_update"] = changed
                call(side, "/api/address", id=sid, address=" play.example.com ")
                assert call(side, "/api/info", id=sid)["seed"] == "123456789"
                assert call(side, "/api/remove", id=sid) == {"ok": True} and (d / "server.jar").is_file()
            same(f"{name}：Done/玩家/記憶體/指令/stop/三世界自備份/修改/保留檔案移除", *outcomes)

        for side in ("python", "rust"):
            d = directory(side, "EULA 未同意", forms[0][1], plain, eula=False)
            sid = call(side, "/api/add", dir=str(d))["id"]
            assert call(side, "/api/start", id=sid) == {"need": "eula"}
            assert call(side, "/api/eula", id=sid, agree=False).get("error") and not (d / "eula.txt").exists()
            assert call(side, "/api/eula", id=sid, agree=True) == {"ok": True}
            with socket.socket() as busy:
                busy.bind(("0.0.0.0", hub.read_port(d)))
                assert "佔用" in call(side, "/api/start", id=sid)["error"]
            d = directory(side, "Java 不夠新", forms[0][1], needs99)
            sid = call(side, "/api/add", dir=str(d))["id"]
            result = call(side, "/api/start", id=sid)
            assert "Java 99 以上" in result["error"] and "version=99" in result["help_url"]
            d = directory(side, "閃退", "java -jar missing.jar\r\n", plain)
            sid = call(side, "/api/add", dir=str(d))["id"]
            assert call(side, "/api/start", id=sid) == {"ok": True}
            result = wait(side, sid, lambda s: s["state"] == "stopped" and s["problem"] is not None)
            assert "找不到伺服器主程式" in result["problem"]
            record(f"{side} EULA/占埠/Java99/閃退白話提示")

        # 同一個外部假服依序讓兩版探測；平台不得控制它或備份它。
        d = directory("external", "別處開服", forms[0][1], plain)
        external = subprocess.Popen([str(java), "-Xmx256M", "-jar", "server.jar"], cwd=d, stdin=subprocess.PIPE,
                                    stdout=(root / "external.log").open("wb"), stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW)
        for side in ("python", "rust"):
            sid = call(side, "/api/add", dir=str(d))["id"]
            s = wait(side, sid, lambda s: s["state"] == "external")
            assert s["online"] == 1 and s["names"] == ["Tester"]
            assert "不是從平台開的" in call(side, "/api/start", id=sid)["error"]
            assert call(side, "/api/backup", id=sid).get("error")
            assert call(side, "/api/cmd", id=sid, cmd="stop").get("error")
        external.stdin.write(b"stop\n"); external.stdin.flush(); external.wait(timeout=15); external = None
        poll(); record("外部開服：兩版偵測玩家、拒絕開機/備份/送指令")

        for scenario in ("installed", "fail", "write-error", "timeout"):
            outcomes = []
            for side in ("python", "rust"):
                d = root / side / f"installer-{scenario}"; d.mkdir()
                src = d / installer.name; shutil.copyfile(installer, src)
                if scenario == "write-error": (d / "user_jvm_args.txt").mkdir()
                lines = []
                if side == "python":
                    try: result = {"out": hub.install_modloader(d, str(java), src, 1, lines.append, root / side, timeout=.001 if scenario == "timeout" else 30)}
                    except hub.UserError as e: result = {"error": str(e)}
                else: result = rpc("install", dir=str(d), java=str(java), installer=str(src), timeout=0 if scenario == "timeout" else 30)
                if scenario == "installed":
                    assert result["out"] == "start.bat" and not src.exists()
                    assert (d / "start.bat").read_bytes() == hub.start_script(str(java), "-Xmx1G @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.999/win_args.txt nogui")
                    inspected = hub.inspect_server(d) if side == "python" else rpc("inspect", dir=str(d))
                    assert inspected == {"version": "1.21.1", "java": 21, "loader": "NeoForge"}
                    outcomes.append(inspected)
                else:
                    assert "安裝紀錄已保留" in result["error"]
                    assert "安裝逾時" in result["error"] if scenario == "timeout" else "結束代碼 1" in result["error"]
                    kept = list((root / side / "installer-logs").glob("*.log"))
                    assert kept
                    outcomes.append({"rejected": True, "logs_kept": True})
            same(f"假安裝器 {scenario}：輸出/結果/啟動檔/版本/失敗紀錄", *outcomes)

        same("55 指令與17欄位 schema API", call("python", "/api/commands"), call("rust", "/api/commands"))
        for side in ("python", "rust"):
            result = call(side, "/api/settings", join_template="更新 <版本> <連線網址>", update_check=False)
            assert result["update_check"] is False
        legacy = hub.Hub(root / "python/config/hub_config.json")
        rpc("reload")
        same("重開後設定與清單保留", call("python", "/api/settings"), call("rust", "/api/settings"))
        assert json.loads((root / "rust/config/config.json").read_text(encoding="utf-8"))["future"] == old_config["future"]
        record("多次儲存/重開仍保留未知欄位")
        report["status"] = "passed"
    except BaseException:
        report["status"] = "failed"
        report["failure"] = traceback.format_exc()
        raise
    finally:
        if external and external.poll() is None:
            external.stdin.write(b"stop\n"); external.stdin.flush(); external.wait(timeout=15)
        if legacy:
            for s in legacy.all_servers():
                if s.alive(): s.stop()
            end = time.monotonic() + 20
            while legacy.busy() and time.monotonic() < end: time.sleep(.1)
        if rust and rust.poll() is None:
            state = rpc("call", path="/api/state", body={})
            for s in state["servers"]:
                if s["state"] in ("starting", "running", "stopping"): rpc("call", path="/api/stop", body={"id": s["id"]})
            end = time.monotonic() + 20
            while rpc("busy") and time.monotonic() < end: time.sleep(.1)
            rpc_busy = rpc("busy")
            if not rpc_busy:
                rust.stdin.write('{"op":"quit"}\n'); rust.stdin.flush(); rust.wait(timeout=10)
            else: report["cleanup_warning"] = "Rust 假服仍忙碌，保留程序供人工檢查"
        report["appdata_after"] = fingerprints()
        report["appdata_unchanged"] = before == report["appdata_after"]
        (root / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        log.close()
        print("測試資料保留：", root, flush=True)
        assert report["appdata_unchanged"], "真 APPDATA 設定遭變動"


if __name__ == "__main__":
    main()
