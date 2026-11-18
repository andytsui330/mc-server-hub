# 從 1.x Python 版匯出「標準答案」：純邏輯函式的輸入與輸出 → tests/fixtures/*.json，2.0 Rust 版跑同一份。
# 跑法（Windows）：py -3 legacy\tests\export_fixtures.py      會覆寫 tests\fixtures\ 底下的 json
# 格式：{"source": ..., "cases": [{"fn": 函式名, "args": {...}, "out": 結果} 或 {..., "error": 白話錯誤}]}
import importlib.util, json, re, shutil, sys, tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
LEGACY = HERE.parent
OUT = LEGACY.parent / "tests" / "fixtures"
spec = importlib.util.spec_from_file_location("hub", LEGACY / "src" / "hub.py"); hub = importlib.util.module_from_spec(spec); spec.loader.exec_module(hub)
assert hub.OS == "win", "錯誤訊息裡的檔名（start.bat、記事本）以 Windows 為準，請在 Windows 匯出"
TMP = Path(tempfile.mkdtemp(prefix="mcfix_"))
DIR = TMP / "假 伺服器"   # 故意有中文與空白；輸出裡換成 <DIR>
DIR.mkdir()


def run(fn, args, call):
    try:
        out = call()
    except hub.UserError as e:
        return {"fn": fn, "args": args, "error": str(e)}
    return {"fn": fn, "args": args, "out": out}


def save(name, cases):
    OUT.mkdir(parents=True, exist_ok=True)
    data = {"source": f"legacy/src/hub.py {hub.VERSION}", "os": hub.OS, "cases": cases}
    (OUT / name).write_text(json.dumps(data, ensure_ascii=False, indent=1) + "\n", encoding="utf-8", newline="\n")
    print(f"{name}: {len(cases)} 筆")


# ---------- 啟動檔解析 ----------
REAL_NEO = ("@echo off\r\nREM Forge requires a configured set of both JVM and program arguments.\r\n"
            "java @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.266/win_args.txt %*\r\npause\r\n")
REAL_FORGE = ("@echo off\r\nREM Add custom JVM arguments (such as RAM allocation) to the user_jvm_args.txt\r\n\r\n"
              "java -jar forge-1.21.1-52.1.20-shim.jar --onlyCheckJava\r\nif %ERRORLEVEL% NEQ 0 (\r\n    echo.\r\n"
              "    echo If you're struggling to fix the error above, ask for help.\r\n    goto :exit\r\n)\r\n\r\n"
              "java @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.21.1-52.1.20/win_args.txt %*\r\n\r\n:exit\r\npause\r\n")
LAUNCH = [
    ("start.bat", "@echo off\njava -Xmx4G -jar server.jar nogui\npause\n"),
    ("start.bat", '@echo off\ncd /d "%~dp0"\nset RAM=6G\nset "J=C:\\Program Files\\Java\\jdk-21\\bin\\java.exe"\n"%J%" -Xmx%RAM% ^\n  -jar s.jar %*\npause\n'),
    ("start.bat", '"%~dp0jre\\bin\\java.exe" -jar server.jar nogui\n'),
    ("start.bat", "java @user_jvm_args.txt @libraries/net/x/win_args.txt %*\r\npause\r\n"),
    ("start.bat", ":top\njava -jar s.jar nogui\ntimeout /t 5\ngoto top\n"),
    ("start.bat", "title 我的伺服器\nchcp 65001\njavaw -Xms1G -Xmx2G -jar s.jar\n"),
    ("run.bat", REAL_NEO),
    ("run.bat", REAL_FORGE),
    ("run.bat", REAL_FORGE.replace("--onlyCheckJava", "--other")),
    ("run.bat", REAL_FORGE.replace("if %ERRORLEVEL% NEQ 0 (", "if exist x (")),
    ("start.bat", "if exist s.jar (\n java -jar s.jar\n)\n"),
    ("start.bat", 'java -jar s.jar | find "x"\n'),
    ("start.bat", "java -jar s.jar\njava -jar b.jar\n"),
    ("start.bat", "set /p X=?\njava -jar s.jar\n"),
    ("start.bat", "cd sub\njava -jar s.jar\n"),
    ("start.bat", "%UNKNOWN_VAR_X% -jar s.jar\n"),
    ("start.bat", "py -3 run.py\n"),
    ("start.bat", ":a\ngoto b\njava -jar s.jar\n:b\n"),
    ("start.bat", '"C:\\Java\\bin\\java.exe" -version >nul\nif errorlevel 1 exit\njava -jar s.jar\n'),
    ("start.sh", '#!/bin/sh\ncd "$(dirname "$0")"\nexec "/opt/jdk/bin/java" -Xmx2G -jar server.jar nogui\n'),
    ("start.sh", '#!/bin/sh\njava -Xmx4G -jar server.jar nogui "$@"\n'),
    ("start.sh", "#!/bin/sh\nwhile true; do java -jar s.jar; done\n"),
    ("run.sh", '#!/usr/bin/env sh\njava @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.265/unix_args.txt "$@"\n'),
    ("start.sh", "#!/bin/sh\necho hi\n"),
]


def launch_case(name, text):
    (DIR / name).write_bytes(text.encode("utf-8"))
    r = hub.parse_launch(DIR, name)
    return {"fn": "parse_launch", "args": {"file": name, "text": text},
            "out": {"cmd": r.cmd.replace(str(DIR), "<DIR>"), "java": r.java and r.java.replace(str(DIR), "<DIR>"), "mode": r.mode}}


launch = [launch_case(n, t) for n, t in LAUNCH]
for t in (REAL_NEO, REAL_FORGE, "java -jar server.jar\r\n", "REM java @user_jvm_args.txt @libraries/x/win_args.txt\r\n",
          "java @user_jvm_args.txt @libraries/x&calc/win_args.txt\r\n",
          '#!/usr/bin/env sh\njava @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.265/unix_args.txt "$@"\n'):
    launch.append(run("modloader_args", {"text": t}, lambda: hub.modloader_args(t)))
for os_name in ("win", "mac"):
    a = {"java": "C:\\Program Files\\Java\\bin\\java.exe" if os_name == "win" else "/opt/jdk/bin/java", "args": "-Xmx4G -jar server.jar nogui", "os": os_name}
    launch.append(run("start_script", a, lambda: hub.start_script(a["java"], a["args"], os_name).decode("utf-8")))
save("launch.json", launch)

# ---------- 版本挑選 ----------
PROMOS = {"promos": {"1.21.1-latest": "52.1.21", "1.21.1-recommended": "52.1.20", "1.21.4-latest": "54.1.22",
                     "1.20.1-recommended": "47.4.0", "1.16.5-recommended": "36.2.34", "1.21.3-latest": "53.0.0; rm"}}
NEO = {"versions": ["21.1.9", "21.1.265", "21.1.30", "21.1.300-beta", "21.4.0-beta", "21.4.1-beta",
                    "21.2.0-beta+snapshot", "26.3.0.5", "26.3.0.12", "26.3.0.13-alpha", "21.0.167"]}
ver = []
for v in ("1.21.1", "1.21", "26.3", "26.3.1", "1.21.11", "1", "", "1.2.x", "v1.2"):
    ver.append(run("mc_tuple", {"v": v}, lambda: hub.mc_tuple(v)))
for v in ("1.21.1", "1.20.5", "1.20.4", "1.18", "1.17.1", "1.16.5", "1.8.9", "26.1", "", "abc"):
    ver.append(run("java_for_mc", {"version": v}, lambda: hub.java_for_mc(v)))
for mc in ("1.21.1", "1.21.4", "1.20.1", "1.16.5", "1.20", "1.21.2", "1.21.3", "abc"):
    ver.append(run("forge_pick", {"promos": PROMOS, "mc": mc}, lambda: list(hub.forge_pick(PROMOS, mc))))
for v in ("21.1.265", "21.0.167", "21.4.161", "26.3.0.12", "26.3.1.4", "20.4", "1.2.3.4.5"):
    ver.append(run("neoforge_mc", {"v": v}, lambda: hub.neoforge_mc(v)))
for mc in ("1.21.1", "26.3", "1.21", "1.21.4", "1.19.2"):
    ver.append(run("neoforge_pick", {"data": NEO, "mc": mc}, lambda: list(hub.neoforge_pick(NEO, mc))))
for kind, mc, data in (("forge", "1.21.1", None), ("neoforge", "1.21.1", None), ("forge", "1.21.1", PROMOS), ("neoforge", "1.21.4", NEO)):
    ver.append(run("loader_choice", {"kind": kind, "mc": mc, "data": data}, lambda: hub.loader_choice(kind, mc, data)))
ver.append(run("forge_installer_url", {"mc": "1.21.1", "v": "52.1.20"}, lambda: hub.forge_installer_url("1.21.1", "52.1.20")))
ver.append(run("neoforge_installer_url", {"v": "21.1.265"}, lambda: hub.neoforge_installer_url("21.1.265")))
for label in ("1.21.1 · NeoForge", "1.21.11 · Fabric", "26.3", "版本未知", "", "Forge 1.20"):
    ver.append(run("mc_of", {"label": label}, lambda: hub.mc_of(label)))
for body in ("2.0.0\n", "1.2.0", "0.9.9", "1.2.1", "", "<html>moved</html>", "1.0", "1.0.0.1", "v2.0.0", "2.0.0 rm -rf", "99999.0.0", '{"v":"2.0.0"}'):
    ver.append(run("newer_version", {"body": body, "current": hub.VERSION}, lambda: hub.newer_version(lambda: body.encode())))
save("versions.json", ver)

# ---------- 開機失敗白話翻譯 ----------
FAILS = [
    (["x UnsupportedClassVersionError: y has been compiled ... class file version 65.0, this"], 1, False),
    (["Error: Unable to access jarfile server.jar"], 1, False),
    (["You need to agree to the EULA in order to run the server"], 0, False),
    (["**** FAILED TO BIND TO PORT!"], 1, True),
    (["Error occurred during initialization of VM", "Could not reserve enough space for 8388608KB object heap"], 1, False),
    (["'java' 不是內部或外部命令、可執行的程式或批次檔。"], 9009, False),
    (["sh: java: command not found"], 127, False),
    (["Incompatible mods found!"], 1, False),
    (["Missing or unsupported mandatory dependencies:"], 1, False),
    (["Done (3.2s)!", "Stopping server"], 0, True),
    (["Done (3.2s)!", "boom"], 1, True),
    (["ok"], 1, False),
]
save("failures.json", [run("explain_failure", {"lines": l, "code": c, "saw_done": d}, lambda: hub.explain_failure(l, c, d)) for l, c, d in FAILS])

# ---------- 常用指令 ----------
SAMPLE = {"player": "Steve_01", "target": ".BedrockGuy", "item": "diamond", "mob": "horse", "effect": "speed",
          "ench": "sharpness", "count": "64", "secs": "120", "amp": "2", "level": "5", "x": "10", "y": "-60", "z": "3.5",
          "msg": "晚上 9 點重開", "reason": "", "bool": "true", "pct": "50"}
CMD = {c["id"]: c for c in hub.COMMANDS}
cmds = [{"fn": "commands_count", "args": {}, "out": len(hub.COMMANDS)}]
for label in ("1.21.1 · NeoForge", "1.21.11", "26.3", "版本未知"):
    cmds.append(run("commands_for", {"label": label}, lambda: hub.commands_for(label)))
for c in hub.COMMANDS:
    if c.get("special"):
        continue
    for conf in ("strong", True, None):
        cmds.append(run("build_command", {"id": c["id"], "values": SAMPLE, "confirm": conf, "label": "1.21.1 · NeoForge"},
                        lambda: hub.build_command(c, SAMPLE, conf, "1.21.1 · NeoForge")))
EXTRA = [("keep_inv", {}, None, "1.21.11 · Fabric"), ("time_stop", {}, None, "26.3"), ("sleep_pct", {}, None, "26.3.1"),
         ("keep_inv", {}, None, "版本未知"), ("fire_tick", {}, None, "26.3"), ("mob_spawn", {"bool": "false"}, None, "1.21.1"),
         ("mob_spawn", {"bool": "false"}, True, "1.21.1"), ("gm_creative", {"player": "@a"}, None, "1.21.1"),
         ("gm_creative", {"player": "@a"}, True, "1.21.1")]
for cid, vals in (("give", {"player": "a b"}), ("give", {"player": "x" * 17}), ("give", {"count": "641"}), ("give", {"count": "0"}),
                  ("give", {"count": "abc"}), ("give", {"item": "Diamond"}), ("give", {"item": "diamond 64\nop me"}),
                  ("summon", {"mob": "player"}), ("effect", {"secs": "3601"}), ("tp_xyz", {"x": "~5"}), ("say", {"msg": "a\nstop"}),
                  ("say", {"msg": "x" * 201}), ("say", {"msg": ""}), ("enchant", {"ench": "mending", "level": "2"}),
                  ("keep_inv", {"bool": "yes"}), ("give", {"player": "@e"}), ("give", {"player": "Steve\r"}), ("kick", {"reason": "  吵  "})):
    EXTRA.append((cid, vals, "strong", "1.21.1"))
for cid, vals, conf, label in EXTRA:
    v = {**SAMPLE, **vals}
    cmds.append(run("build_command", {"id": cid, "values": v, "confirm": conf, "label": label}, lambda: hub.build_command(CMD[cid], v, conf, label)))
save("commands.json", cmds)

# ---------- 文字與欄位檢查 ----------
txt = []
for name in ("週末 生存服", "", "a/b", "a..", "CON", "con.txt", "COM1", "x" * 41, "x" * 40, "..\\x", "a:b", "結尾空白 "):
    txt.append(run("check_folder_name", {"name": name}, lambda: hub.check_folder_name(name)))
for items in ([{"name": "回家", "tpl": "/tp <player> 0 64 0", "id": "my_home"}, {"id": "my_abc", "name": "全體夜視", "tpl": "effect give @a minecraft:night_vision 600 0 true", "danger": "confirm"}],
              [{"name": "x", "tpl": "say a\nstop", "id": "my_a"}], [{"name": "x", "tpl": "say <密碼>", "id": "my_a"}], [{"name": "", "tpl": "list", "id": "my_a"}],
              [{"name": "x", "tpl": "list", "danger": "boom", "id": "my_a"}], [{"name": "x", "tpl": "list", "id": "../x"}], "nope", [{}] * 101):
    txt.append(run("clean_custom_commands", {"items": items}, lambda: hub.clean_custom_commands(items)))
for items in ([{"name": "Wiki", "url": "https://minecraft.wiki/"}], [{"name": "x", "url": "http://a"}], [{"name": "", "url": "https://a"}], "nope"):
    txt.append(run("clean_links", {"items": items}, lambda: hub.clean_links(items)))
for cfg in ({"version": "1.21.1 · NeoForge", "address": "abc.example.com"}, {"version": "1.21.1 · Fabric · 100 模組"},
            {"version": "1.20.1", "loader": "Forge"}, {"version": ""}, {"version": "版本未知"}):
    txt.append(run("join_text", {"template": hub.DEFAULT_JOIN, "server_cfg": cfg}, lambda: hub.join_text(hub.DEFAULT_JOIN, cfg)))
for text, gb in (('@echo off\r\n"java" -Xmx4G -jar s.jar nogui\r\n', 8), ('"java" -Xms6G -Xmx6G -jar s.jar\r\n', 4),
                 ('java -Xms512M -Xmx2048M -jar s.jar\n', 3), ("java -jar s.jar\n", 4), ("java -Xmx2G -Xmx4G -jar s.jar\n", 4)):
    script = DIR / "ram.bat"
    script.write_bytes(text.encode("utf-8"))

    def go():
        hub.set_ram(script, gb)
        return script.read_bytes().decode("utf-8")
    txt.append(run("set_ram", {"text": text, "gb": gb}, go))
for text in ("java -Xmx4G -jar s.jar", "java -Xmx2048M -jar s.jar", "java -Xmx1536m -jar s.jar", "java -jar s.jar"):
    (DIR / "ram.bat").write_bytes(text.encode("utf-8"))
    txt.append(run("read_ram_gb", {"text": text}, lambda: hub.read_ram_gb(DIR / "ram.bat")))
save("text.json", txt)

shutil.rmtree(TMP, ignore_errors=True)
print("匯出完成 →", OUT)
