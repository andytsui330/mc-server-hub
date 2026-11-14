# 用假伺服器走完整流程：3 種以上 start.bat 寫法的開關機、EULA、Java、連接埠、備份、新增移除。
# 跑法（要有 JDK）：py -3 tests\test_hub.py            測原始碼
#                  py -3 tests\test_hub.py <exe路徑>    測打包好的 exe（環境變數會拿掉 Python）
import importlib.util, json, os, re, shutil, socket, subprocess, sys, tempfile, time, zipfile
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
PORT = 47893
URL = f"http://127.0.0.1:{PORT}"
spec = importlib.util.spec_from_file_location("hub", ROOT / "src" / "hub.py"); hub = importlib.util.module_from_spec(spec); spec.loader.exec_module(hub)

JAVA = shutil.which("java")
FAKE_DIR = str(Path(tempfile.mkdtemp(prefix="mcfake_")) / "假 伺服器")   # 故意有中文與空白


def parse(text, name="p"):
    d = Path(FAKE_DIR) / name
    d.mkdir(parents=True, exist_ok=True)
    (d / "start.bat").write_bytes(text.encode("mbcs"))
    return hub.parse_launch(d)


# ---------- start.bat 解析 ----------
FLAGS = " -Dstdout.encoding=UTF-8 -Dstderr.encoding=UTF-8"
r = parse("@echo off\njava -Xmx4G -jar server.jar nogui\npause\n")
assert r == ("java" + FLAGS + " -Xmx4G -jar server.jar nogui", "java", "direct"), r
r = parse('@echo off\ncd /d "%~dp0"\nset RAM=6G\nset "J=C:\\Program Files\\Java\\jdk-21\\bin\\java.exe"\n"%J%" -Xmx%RAM% ^\n  -jar s.jar %*\npause\n')
assert r.cmd == '"C:\\Program Files\\Java\\jdk-21\\bin\\java.exe"' + FLAGS + " -Xmx6G  -jar s.jar ", r
assert r.java == "C:\\Program Files\\Java\\jdk-21\\bin\\java.exe" and r.mode == "direct", r
assert parse("java @user_jvm_args.txt @libraries/net/x/win_args.txt %*\r\npause\r\n").mode == "direct"   # Forge／NeoForge
assert parse(":top\njava -jar s.jar nogui\ntimeout /t 5\ngoto top\n").mode == "direct"                 # 自動重開迴圈
# 2026-11-13 真實安裝產生的 run.bat（原樣）：NeoForge 21.1.266 一行、Forge 52.1.20 先檢查 Java 再啟動的兩行寫法
REAL_NEO = ("@echo off\r\nREM Forge requires a configured set of both JVM and program arguments.\r\nREM Add custom JVM arguments to the user_jvm_args.txt\r\n"
            "REM Add custom program arguments {such as nogui} to this file in the next line before the %* or\r\nREM  pass them to this script directly\r\n"
            "java @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.266/win_args.txt %*\r\npause\r\n")
REAL_FORGE = ("@echo off\r\nREM Add custom JVM arguments (such as RAM allocation) to the user_jvm_args.txt\r\n\r\n"
              "java -jar forge-1.21.1-52.1.20-shim.jar --onlyCheckJava\r\nif %ERRORLEVEL% NEQ 0 (\r\n    echo.\r\n"
              "    echo If you're struggling to fix the error above, ask for help on the forums or Discord mentioned in the readme.\r\n"
              "    goto :exit\r\n)\r\n\r\nREM Add custom program arguments (such as nogui) to the next line before the %* or pass them to this script directly\r\n"
              "java @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.21.1-52.1.20/win_args.txt %*\r\n\r\n:exit\r\npause\r\n")
for real, lib in ((REAL_NEO, "net/neoforged/neoforge/21.1.266"), (REAL_FORGE, "net/minecraftforge/forge/1.21.1-52.1.20")):
    r = parse(real, "real")
    assert r == ("java" + FLAGS + f" @user_jvm_args.txt @libraries/{lib}/win_args.txt ", "java", "direct"), r
    assert hub.modloader_args(real) == f"@user_jvm_args.txt @libraries/{lib}/win_args.txt"
# 只拿掉「shim 檢查＋if 區塊」這個固定組合；其他 if 寫法照舊交給 bat 自己跑
assert parse(REAL_FORGE.replace("--onlyCheckJava", "--other"), "real").mode == "bat"
assert parse(REAL_FORGE.replace("if %ERRORLEVEL% NEQ 0 (", "if exist x ("), "real").mode == "bat"
for hard in ("if exist s.jar (\n java -jar s.jar\n)\n", "java -jar s.jar | find \"x\"\n", "java -jar s.jar\njava -jar b.jar\n",
             "set /p X=?\njava -jar s.jar\n", "cd sub\njava -jar s.jar\n", "%UNKNOWN_VAR_X% -jar s.jar\n", "py -3 run.py\n",
             ":a\ngoto b\njava -jar s.jar\n:b\n"):
    assert parse(hard).mode == "bat", hard
assert parse("if exist s.jar (\n java -jar s.jar\n)\n").java == "java"
assert parse("py -3 run.py\n").java is None
# ---------- 1.1.1 圖示與顏色：畫面與後端白名單一致、音效沒有音量／靜音 ----------
PAGE_SRC = (ROOT / "src" / "index.html").read_text(encoding="utf-8")
px = PAGE_SRC[PAGE_SRC.index("const PX = (() => {"):PAGE_SRC.index("return {ICONS, COLORS, draw};")]
page_ids = re.findall(r'\bid: "([a-z\-]+)"', px) + re.findall(r'\["(?:友善生物|敵對生物)", "([a-z]+)"', px)
assert len(page_ids) == 93 == len(set(page_ids)) and set(page_ids) == hub.ICON_IDS, set(page_ids) ^ hub.ICON_IDS
page_colors = re.findall(r'\["[^"]+", "(#[0-9a-f]{6})"\]', px[px.index("const COLORS"):])
assert tuple(page_colors) == hub.ICON_COLORS and len(hub.ICON_COLORS) == 8, page_colors
assert "data:image" not in PAGE_SRC and "snd-btn" not in PAGE_SRC and 'store.get("mute"' not in PAGE_SRC and 'store.get("vol"' not in PAGE_SRC
assert "SFX_VOL = 0.4" in PAGE_SRC and "K2(t)" in PAGE_SRC and "K3(t)" in PAGE_SRC
# ---------- 其他小邏輯 ----------
assert hub.java_for_mc("1.21.1") == 21 and hub.java_for_mc("1.20.4") == 17 and hub.java_for_mc("1.16.5") == 8 and hub.java_for_mc("26.1") is None
assert "Java 21" in hub.explain_failure(["x UnsupportedClassVersionError: y has been compiled ... class file version 65.0, this"], 1, False)
assert "EULA" in hub.explain_failure(["You need to agree to the EULA in order to run the server"], 0, False)
assert hub.explain_failure(["ok"], 0, True) is None and "還沒開好" in hub.explain_failure(["ok"], 1, False)
assert hub.check_folder_name("週末 生存服") is None
for bad in ("", "a/b", "a..", "CON", "x" * 41, "..\\x", "a:b"):
    assert hub.check_folder_name(bad), bad
try:
    hub._open("https://evil.example.com/x.jar"); raise AssertionError("非官方網址也能下載")
except hub.UserError:
    pass
try:
    hub._open("http://piston-meta.mojang.com/x"); raise AssertionError("沒加密也能下載")
except hub.UserError:
    pass
# ---------- 新版提醒：有新版、沒新版、內容錯誤、斷網 ----------
def boom():
    raise OSError("offline")
major = int(hub.VERSION.split(".")[0])
assert hub.newer_version(lambda: f"{major + 1}.0.0\n".encode()) == f"{major + 1}.0.0"
assert hub.newer_version(lambda: hub.VERSION.encode()) is None and hub.newer_version(lambda: b"0.9.9") is None
for junk in (b"", b"<html>moved</html>", b"1.0", b"1.0.0.1", b"v2.0.0", b"2.0.0 rm -rf", b"99999.0.0", b'{"v":"2.0.0"}'):
    assert hub.newer_version(lambda: junk) is None, junk
assert hub.newer_version(boom) is None
assert hub.VERSION_URL.startswith("https://") and re.fullmatch(r"\d+\.\d+\.\d+", hub.VERSION)

# ---------- Forge／NeoForge：挑版本、網址、檢查碼（寫死的假資料，不連網） ----------
PROMOS = {"promos": {"1.21.1-latest": "52.1.21", "1.21.1-recommended": "52.1.20", "1.21.4-latest": "54.1.22",
                     "1.20.1-recommended": "47.4.0", "1.16.5-recommended": "36.2.34", "1.21.3-latest": "53.0.0; rm"}}
assert hub.forge_pick(PROMOS, "1.21.1") == ("52.1.20", None)          # 有 recommended 就用它
assert hub.forge_pick(PROMOS, "1.21.4") == ("54.1.22", None)          # 沒有就用 latest
assert hub.forge_pick(PROMOS, "1.20.1") == ("47.4.0", None)
assert "1.1.0 不支援" in hub.forge_pick(PROMOS, "1.16.5")[1] and "1.1.0 不支援" in hub.forge_pick(PROMOS, "1.20")[1]
assert hub.forge_pick(PROMOS, "1.21.2")[0] is None and hub.forge_pick(PROMOS, "1.21.3")[0] is None   # 沒有／怪字元
assert hub.neoforge_mc("21.1.265") == "1.21.1" and hub.neoforge_mc("21.0.167") == "1.21" and hub.neoforge_mc("21.4.161") == "1.21.4"
assert hub.neoforge_mc("26.3.0.12") == "26.3" and hub.neoforge_mc("26.3.1.4") == "26.3.1"
NEO = {"isSnapshot": False, "versions": ["21.1.9", "21.1.265", "21.1.30", "21.1.300-beta", "21.4.0-beta", "21.4.1-beta",
                                         "21.2.0-beta+snapshot", "26.3.0.5", "26.3.0.12", "26.3.0.13-alpha", "21.0.167"]}
assert hub.neoforge_pick(NEO, "1.21.1") == ("21.1.265", None)         # 數字比大小，不是字串；排除 -beta
assert hub.neoforge_pick(NEO, "26.3") == ("26.3.0.12", None) and hub.neoforge_pick(NEO, "1.21") == ("21.0.167", None)
assert "只有測試版" in hub.neoforge_pick(NEO, "1.21.4")[1] and "還沒有" in hub.neoforge_pick(NEO, "1.19.2")[1]
assert hub.forge_installer_url("1.21.1", "52.1.20") == \
    "https://maven.minecraftforge.net/net/minecraftforge/forge/1.21.1-52.1.20/forge-1.21.1-52.1.20-installer.jar"
assert hub.neoforge_installer_url("21.1.265") == \
    "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.265/neoforge-21.1.265-installer.jar"
for u in (hub.FORGE_PROMOS, hub.NEOFORGE_VERSIONS, hub.forge_installer_url("1.21.1", "1"), hub.neoforge_installer_url("1")):
    assert hub.urlparse(u).hostname in hub.ALLOWED_HOSTS and u.startswith("https://"), u
assert hub.loader_choice("forge", "1.21.1", None) == {"ok": False, "version": None, "why": "連不上 Forge 官方網站，稍後再試"}
assert hub.loader_choice("neoforge", "1.21.1", NEO)["version"] == "21.1.265"
# run.bat 解析：NeoForge 一行、Forge 先檢查 Java 再啟動的兩行寫法、Mac 的 run.sh、看不懂的
neo_run = "@echo off\r\nREM Add custom JVM arguments to the user_jvm_args.txt\r\njava @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.265/win_args.txt %*\r\npause\r\n"
assert hub.modloader_args(neo_run) == "@user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.265/win_args.txt"
forge_run = ("java -jar forge-1.21.1-52.1.20-shim.jar --onlyCheckJava\r\nif %ERRORLEVEL% NEQ 0 goto end\r\n"
             "java @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.21.1-52.1.20/win_args.txt %*\r\n:end\r\npause\r\n")
assert hub.modloader_args(forge_run) == "@user_jvm_args.txt @libraries/net/minecraftforge/forge/1.21.1-52.1.20/win_args.txt"
assert hub.modloader_args('#!/usr/bin/env sh\njava @user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.265/unix_args.txt "$@"\n') \
    == "@user_jvm_args.txt @libraries/net/neoforged/neoforge/21.1.265/unix_args.txt"
for bad in ("java -jar server.jar\r\n", "REM java @user_jvm_args.txt @libraries/x/win_args.txt\r\n",
            "java @user_jvm_args.txt @libraries/x&calc/win_args.txt\r\n"):
    assert hub.modloader_args(bad) is None, bad
# 下載與檢查碼：在測試程式裡替換 hub._open（正式程式沒有後門）
class FakeResp:
    def __init__(self, data):
        self.data, self.pos, self.headers = data, 0, {"Content-Length": str(len(data))}
    def read(self, n=-1):
        n = len(self.data) - self.pos if n < 0 else n
        out = self.data[self.pos:self.pos + n]; self.pos += len(out); return out
    def __enter__(self): return self
    def __exit__(self, *a): return False
import hashlib
BLOB = b"installer-bytes" * 1000
SHA1, SHA256 = hashlib.sha1(BLOB).hexdigest(), hashlib.sha256(BLOB).hexdigest()
real_open, opened = hub._open, []
def fake_open(url):
    opened.append(url)
    return FakeResp({".sha1": SHA1.encode() + b"  forge-installer.jar\n", ".sha256": SHA256.encode(), ".bad": b"zz" * 32,
                     ".long": b"a" * 64 + b" " * 200}.get(url[url.rfind("."):], BLOB))
hub._open = fake_open
try:
    dl = Path(tempfile.mkdtemp(prefix="mcdl_"))
    assert hub.fetch_checksum("https://maven.minecraftforge.net/x.jar.sha1", "sha1") == SHA1
    assert hub.fetch_checksum("https://maven.neoforged.net/x.jar.sha256", "sha256") == SHA256
    for url, algo in (("https://x/a.bad", "sha256"), ("https://x/a.long", "sha256"), ("https://x/a.sha1", "sha256")):
        try:
            hub.fetch_checksum(url, algo); raise AssertionError("檢查碼格式不對卻收了：" + url)
        except hub.UserError:
            pass
    hub.download("https://x/i.jar", dl / "ok256.jar", sha256=SHA256); assert (dl / "ok256.jar").read_bytes() == BLOB
    hub.download("https://x/i.jar", dl / "ok1.jar", sha1=SHA1.upper()); assert (dl / "ok1.jar").exists()
    for kw in ({"sha256": SHA1 + "0" * 24}, {"sha1": "0" * 40}):
        try:
            hub.download("https://x/i.jar", dl / "bad.jar", **kw); raise AssertionError("檢查碼不對卻下載成功")
        except hub.UserError as e:
            assert "核對不符" in str(e) and not (dl / "bad.jar").exists() and not (dl / "bad.jar.part").exists()
finally:
    hub._open = real_open
# 埠上已有平台：同版叫回視窗、別版（含 1.0.0 沒回版本）請先關舊的、不是平台就照常報錯
assert hub.ping_action({"app": hub.APP_ID, "version": hub.VERSION}) == "reopen"
assert hub.ping_action({"app": hub.APP_ID}) == "old" and hub.ping_action({"app": hub.APP_ID, "version": "0.0.1"}) == "old"
assert hub.ping_action({"app": "other"}) is None and hub.ping_action(None) is None and hub.ping_action([1]) is None
# ---------- 指令清單：每一筆都組得出字串、危險的沒確認被拒、換行被拒、版本換規則名 ----------
SAMPLE = {"player": "Steve_01", "target": ".BedrockGuy", "item": "diamond", "mob": "horse", "effect": "speed",
          "ench": "sharpness", "count": "64", "secs": "120", "amp": "2", "level": "5", "x": "10", "y": "-60", "z": "3.5",
          "msg": "晚上 9 點重開", "reason": "", "bool": "true", "pct": "50"}
built = {}
for c in hub.COMMANDS:
    if c.get("special"):
        continue
    out = hub.build_command(c, SAMPLE, "strong", "1.21.1 · NeoForge")
    assert out and "<" not in out and "\n" not in out and not out.startswith("/"), (c["id"], out)
    built[c["id"]] = out
    if c["danger"]:
        try:
            hub.build_command(c, SAMPLE, None, "1.21.1"); raise AssertionError("危險指令沒確認卻組出來了：" + c["id"])
        except hub.UserError:
            pass
    if c["danger"] == "strong":
        try:
            hub.build_command(c, SAMPLE, True, "1.21.1"); raise AssertionError("強確認的指令只按一般確認就過了：" + c["id"])
        except hub.UserError:
            pass
assert len(hub.COMMANDS) == len(built) + 1 and len({c["id"] for c in hub.COMMANDS}) == len(hub.COMMANDS)
EXPECT = {
    "day": "time set day", "time_stop": "gamerule doDaylightCycle false", "weather_go": "gamerule doWeatherCycle true",
    "gm_creative": "gamemode creative Steve_01", "diff_peaceful": "difficulty peaceful",
    "tp_player": "tp Steve_01 .BedrockGuy", "tp_xyz": "execute at Steve_01 run tp Steve_01 10 -60 3.5",
    "spawnpoint": "execute at Steve_01 run spawnpoint Steve_01 ~ ~ ~", "worldspawn": "execute at Steve_01 run setworldspawn ~ ~ ~",
    "give": "give Steve_01 minecraft:diamond 64", "enchant": "enchant Steve_01 minecraft:sharpness 5",
    "xp": "xp add Steve_01 64 levels", "summon": "execute at Steve_01 run summon minecraft:horse ~ ~ ~",
    "effect": "effect give Steve_01 minecraft:speed 120 1 true", "effect_clear": "effect clear Steve_01",
    "keep_inv": "gamerule keepInventory true", "sleep_pct": "gamerule playersSleepingPercentage 50",
    "fire_tick": "gamerule doFireTick true", "say": "say 晚上 9 點重開", "kick": "kick Steve_01", "op": "op Steve_01",
    "ban": "ban Steve_01", "wl_on": "whitelist on", "banlist": "banlist", "seed": "seed",
    "kill_items": "kill @e[type=minecraft:item]", "kill_mob": "kill @e[type=minecraft:horse]", "clear_inv": "clear Steve_01",
}
for k, v in EXPECT.items():
    assert built[k] == v, (k, built[k], v)
CMD = {c["id"]: c for c in hub.COMMANDS}
B = lambda cid, vals, conf=None, ver="1.21.1": hub.build_command(CMD[cid], {**SAMPLE, **vals}, conf, ver)
assert B("keep_inv", {}, None, "1.21.11 · Fabric") == "gamerule keep_inventory true"
assert B("time_stop", {}, None, "26.3") == "gamerule advance_time false" and B("sleep_pct", {}, None, "26.3.1") == "gamerule players_sleeping_percentage 50"
assert B("keep_inv", {}, None, "版本未知") == "gamerule keepInventory true"           # 看不出版本 → 舊名
assert all(c["id"] != "fire_tick" for c in hub.commands_for("1.21.11"))                # 新版已移除的規則要隱藏
assert any(c["id"] == "fire_tick" for c in hub.commands_for("1.21.4"))
assert next(c for c in hub.commands_for("版本未知") if c["id"] == "keep_inv")["uncertain"] is True
try:
    B("fire_tick", {}, None, "26.3"); raise AssertionError("新版沒有的規則還送得出去")
except hub.UserError:
    pass
assert B("mob_spawn", {"bool": "true"}) == "gamerule doMobSpawning true"
for cid, vals in (("mob_spawn", {"bool": "false"}), ("mob_grief", {"bool": "false"}), ("gm_creative", {"player": "@a"})):
    try:
        B(cid, vals); raise AssertionError(f"{cid} {vals} 應該要確認")
    except hub.UserError:
        pass
    assert B(cid, vals, True)
for cid, vals in (("give", {"player": "a b"}), ("give", {"player": "x" * 17}), ("give", {"count": "641"}), ("give", {"count": "0"}),
                  ("give", {"item": "Diamond"}), ("give", {"item": "diamond 64\nop me"}), ("summon", {"mob": "player"}),
                  ("effect", {"secs": "3601"}), ("tp_xyz", {"x": "~5"}), ("say", {"msg": "a\nstop"}), ("say", {"msg": "x" * 201}),
                  ("say", {"msg": ""}), ("enchant", {"ench": "mending", "level": "2"}), ("keep_inv", {"bool": "yes"}),
                  ("give", {"player": "@e"}), ("give", {"player": "Steve\r"})):
    try:
        B(cid, vals, "strong"); raise AssertionError(f"不合法的欄位卻組出指令：{cid} {vals}")
    except hub.UserError:
        pass
assert hub.FIELDS["ench"]["picks"][0] == ["protection", "保護", 4] and all(re.fullmatch(r"[a-z_]+", e[0]) for e in hub.ENCHANTS)
# 我的筆記、實用連結
cc = hub.clean_custom_commands([{"name": "回家", "tpl": "/tp <player> 0 64 0"}, {"id": "my_abc", "name": "全體夜視", "tpl": "effect give @a minecraft:night_vision 600 0 true", "danger": "confirm"}])
assert cc[0]["tpl"] == "tp <player> 0 64 0" and cc[0]["id"].startswith("my_") and cc[0]["cat"] == "我的筆記" and cc[1]["id"] == "my_abc"
assert hub.build_command(cc[0], {"player": "Alex"}, None, "") == "tp Alex 0 64 0"
for bad in ([{"name": "x", "tpl": "say a\nstop"}], [{"name": "x", "tpl": "say <密碼>"}], [{"name": "", "tpl": "list"}],
            [{"name": "x", "tpl": "list", "danger": "boom"}], [{"name": "x", "tpl": "list", "id": "../x"}], "nope", [{}] * 101):
    try:
        hub.clean_custom_commands(bad); raise AssertionError(f"不合法的筆記卻收了：{str(bad)[:40]}")
    except hub.UserError:
        pass
assert hub.clean_links([{"name": "Wiki", "url": "https://minecraft.wiki/"}]) == [{"name": "Wiki", "url": "https://minecraft.wiki/"}]
for bad in ([{"name": "x", "url": "http://a.com"}], [{"name": "x", "url": "javascript:alert(1)"}], [{"name": "", "url": "https://a.com"}],
            [{"name": "x", "url": "https://a.com/\"><script>"}]):
    try:
        hub.clean_links(bad); raise AssertionError(f"不安全的連結卻收了：{bad}")
    except hub.UserError:
        pass
jt = hub.join_text(hub.DEFAULT_JOIN, {"version": "1.21.1 · NeoForge", "loader": "NeoForge", "address": "mc.example.com:25565"})
assert "mc.example.com:25565" in jt and "1.21.1" in jt and "NeoForge" in jt and "<" not in jt
assert "還沒設定連線網址" in hub.join_text(hub.DEFAULT_JOIN, {"version": "", "address": ""})
# 1.0.0 留下的設定沒有 loader：從版本字樣推回來，不能寫成原版
assert "（Fabric）" in hub.join_text(hub.DEFAULT_JOIN, {"version": "1.21.1 · Fabric · 100 模組", "address": "a"})
assert "（原版）" in hub.join_text(hub.DEFAULT_JOIN, {"version": "1.21.4", "address": "a"})
# 記憶體調整：只改唯一的 -Xmx、-Xms 太大一起降、留 .bak、編碼不動；看不懂的拒絕
rd = Path(tempfile.mkdtemp(prefix="mcram_"))
(rd / "start.bat").write_bytes('@echo off\r\nREM 中文註解\r\n"java" -Xms4G -Xmx6G -jar server.jar nogui\r\n'.encode("mbcs"))
hub.set_ram(rd / "start.bat", 2)
assert (rd / "start.bat").read_bytes() == '@echo off\r\nREM 中文註解\r\n"java" -Xms2G -Xmx2G -jar server.jar nogui\r\n'.encode("mbcs")
assert b"-Xmx6G" in (rd / "start.bat.bak").read_bytes() and hub.read_ram_gb(rd / "start.bat") == 2
for odd in (b"java -jar s.jar\r\n", b"java -Xmx2G -Xmx4G -jar s.jar\r\n"):
    (rd / "odd.bat").write_bytes(odd)
    try:
        hub.set_ram(rd / "odd.bat", 3); raise AssertionError("看不懂的啟動檔也被改了")
    except hub.UserError:
        assert (rd / "odd.bat").read_bytes() == odd
# 設定檔：未知欄位讀進再存不會消失
cfg_dir = Path(tempfile.mkdtemp(prefix="mccfgx_"))
(cfg_dir / "c.json").write_text(json.dumps({"servers": [{"id": "s1", "name": "舊", "version": "1.21.1", "dir": str(cfg_dir), "future_flag": 7}],
                                            "future_top": {"a": [1, 2]}}, ensure_ascii=False), encoding="utf-8")
h2 = hub.Hub(cfg_dir / "c.json")
assert h2.settings()["links"] == hub.DEFAULT_LINKS and h2.settings()["custom_commands"] == []
h2.save_settings({"links": [{"name": "Wiki", "url": "https://minecraft.wiki/"}], "custom_commands": cc})
again_cfg = json.loads((cfg_dir / "c.json").read_text(encoding="utf-8"))
assert again_cfg["future_top"] == {"a": [1, 2]} and again_cfg["servers"][0]["future_flag"] == 7 and len(again_cfg["custom_commands"]) == 2
assert hub.Hub(cfg_dir / "c.json").settings()["links"][0]["name"] == "Wiki"
assert "前置模組" in hub.explain_failure(["Missing or unsupported mandatory dependencies:", "\tMod ID: 'architectury'"], 1, False)

# ---------- 跨平台：假裝是 Mac（沒有 WinDLL）也要載得起來，純字串函式輸出正確 ----------
import ctypes, http.server   # 先載好標準模組，假裝期間只重新載入 hub 本身
real_plat, real_windll = sys.platform, getattr(ctypes, "WinDLL", None)
def _no_windll(*a, **k):
    raise OSError("假裝是 Mac：沒有 WinDLL")
sys.platform, ctypes.WinDLL = "darwin", _no_windll
try:
    mspec = importlib.util.spec_from_file_location("hub_mac", ROOT / "src" / "hub.py"); mac = importlib.util.module_from_spec(mspec); mspec.loader.exec_module(mac)
finally:
    sys.platform, ctypes.WinDLL = real_plat, real_windll
assert mac.OS == "mac" and mac.TEXT_ENC == "utf-8" and mac.NO_WINDOW == 0
assert mac.data_dir(home="/Users/x") == Path("/Users/x/Library/Application Support/MCServerHub")
assert mac.DATA_DIR == Path.home() / "Library" / "Application Support" / "MCServerHub"
assert mac.start_script_name() == "start.sh" and mac.args_file_name() == "unix_args.txt"
assert mac.start_script("/opt/jdk/bin/java", "-Xmx2G @user_jvm_args.txt @libraries/x/unix_args.txt nogui") == \
    b'#!/bin/sh\ncd "$(dirname "$0")"\nexec "/opt/jdk/bin/java" -Xmx2G @user_jvm_args.txt @libraries/x/unix_args.txt nogui\n'
assert "os=mac&arch=x64" in mac.java_help_url(21, arch="x64") and "arch=aarch64" in mac.java_help_url(21, arch="aarch64")
# 1.2.0 Mac 功能：用假的 ps／sysctl 輸出測
class _R:
    def __init__(self, out=b"", err=b""):
        self.stdout, self.stderr = out, err
ps_out = b"  1     0  1000\n 500     1  2048\n 501   500 409600\n 502   501  10240\n 900     1  99999\nbad line\n"
table = mac._ps_table(run=lambda *a, **k: _R(ps_out))
assert table[501] == (500, 409600) and len(table) == 5
assert mac.mem_mb(501, table) == 400 and mac.mem_mb(4242, table) is None
assert mac.tree_mem_mb(500, table) == round((2048 + 409600 + 10240) / 1024) and mac.tree_mem_mb(4242, table) is None
assert mac.total_ram_gb(run=lambda *a, **k: _R(b"17179869184\n")) == 16 and mac.total_ram_gb(run=lambda *a, **k: _R(b"??")) == 8
assert mac.mac_arch(run=lambda *a, **k: _R(b"1\n")) == "aarch64"
mac_home = Path(tempfile.mkdtemp(prefix="mcmachome_"))
jdk = mac_home / "Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home/bin"; jdk.mkdir(parents=True); (jdk / "java").write_bytes(b"")
jh = f"Matching Java Virtual Machines (1):\n    21.0.5 (x86_64) \"Eclipse Adoptium\" - \"OpenJDK 21\" {mac_home}/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home\n".encode()
found = mac.mac_java_paths(home=mac_home, run=lambda *a, **k: _R(err=jh))
assert found and all(Path(p) == jdk / "java" for p in found), found
mac_dir = Path(tempfile.mkdtemp(prefix="mcmac_"))
assert mac.write_start_script(mac_dir, "java", "-jar s.jar nogui") == "start.sh" and b"\r" not in (mac_dir / "start.sh").read_bytes()
# Mac 啟動檔解析：平台自己寫的、Forge 的 run.sh、看不懂的
FLAGS_SH = " -Dstdout.encoding=UTF-8 -Dstderr.encoding=UTF-8"
(mac_dir / "start.sh").write_bytes(mac.start_script("/opt/j dk/bin/java", "-Xms1G -Xmx2G -jar server.jar nogui"))
assert mac.parse_launch(mac_dir, "start.sh") == ('"/opt/j dk/bin/java"' + FLAGS_SH + " -Xms1G -Xmx2G -jar server.jar nogui", "/opt/j dk/bin/java", "direct")
(mac_dir / "run.sh").write_text("#!/usr/bin/env sh\n# Forge requires...\njava @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.21.1-52.1.20/unix_args.txt \"$@\"\n")
assert mac.parse_launch(mac_dir, "run.sh") == ("java" + FLAGS_SH + " @user_jvm_args.txt @libraries/net/minecraftforge/forge/1.21.1-52.1.20/unix_args.txt", "java", "direct")
(mac_dir / "loop.sh").write_text("#!/bin/sh\nwhile true; do\n  java -Xmx2G -jar server.jar nogui\ndone\n")
assert mac.parse_launch(mac_dir, "loop.sh") == ("/bin/sh ./loop.sh", "java", "bat")
(mac_dir / "var.sh").write_text("#!/bin/sh\nRAM=4G\n/usr/bin/java -Xmx$RAM -jar s.jar\n")
assert mac.parse_launch(mac_dir, "var.sh") == ("/bin/sh ./var.sh", "/usr/bin/java", "bat")
assert mac.find_bat(mac_dir) == "start.sh" and hub.find_bat(mac_dir, "mac") == "start.sh"
# 改記憶體不能把 start.sh 的可執行權限弄丟（Windows 上 chmod 只看唯讀位，這裡只確認權限有照抄）
before = os.stat(mac_dir / "start.sh").st_mode
mac.set_ram(mac_dir / "start.sh", 3)
assert b"-Xmx3G" in (mac_dir / "start.sh").read_bytes() and os.stat(mac_dir / "start.sh").st_mode == before
# 新版提醒可以關：關掉後完全不連線
upd_dir = Path(tempfile.mkdtemp(prefix="mcupd_"))
hu = hub.Hub(upd_dir / "c.json")
assert hu.settings()["update_check"] is True
hu.save_settings({"update_check": False})
assert json.loads((upd_dir / "c.json").read_text(encoding="utf-8"))["update_check"] is False
real_newer, hub.newer_version = hub.newer_version, lambda *a, **k: (_ for _ in ()).throw(AssertionError("關掉了還在連線"))
try:
    hu.update = "9.9.9"; hu.check_update(); assert hu.update is None
finally:
    hub.newer_version = real_newer
assert hub.Hub(upd_dir / "c.json").settings()["update_check"] is False
hu.save_settings({"update_check": "yes"}); assert hu.settings()["update_check"] is False   # 只認 true
# Windows 這邊內容要跟 1.0.0 一模一樣
assert hub.OS == "win" and hub.start_script_name() == "start.bat" and hub.args_file_name() == "win_args.txt"
assert hub.start_script("C:\\J\\java.exe", "-Xms1G -Xmx2G -jar server.jar nogui") == \
    '@echo off\r\ncd /d "%~dp0"\r\n"C:\\J\\java.exe" -Xms1G -Xmx2G -jar server.jar nogui\r\npause\r\n'.encode("mbcs")
assert hub.DATA_DIR == Path(os.environ["APPDATA"]) / "MCServerHub" and "os=windows" in hub.java_help_url(21)

# ---------- 用真的 java 編一支假伺服器 jar ----------
work = Path(tempfile.mkdtemp(prefix="mcjar_"))
subprocess.run(["javac", "-encoding", "UTF-8", "-d", str(work), str(HERE / "FakeServer.java"), str(HERE / "FakeInstaller.java")], check=True)
jar_plain, jar_java99 = work / "plain.jar", work / "needs99.jar"
with zipfile.ZipFile(jar_plain, "w") as z:
    z.writestr("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\r\nMain-Class: FakeServer\r\n\r\n")
    for c in work.glob("*.class"):
        z.write(c, c.name)
shutil.copy(jar_plain, jar_java99)
with zipfile.ZipFile(jar_java99, "a") as z:
    z.writestr("version.json", json.dumps({"id": "9.9", "java_version": 99}))

FORMS = {
    "A 單行":   ("@echo off\njava -Xmx256M -jar server.jar nogui\npause\n", "direct", 25601),
    "B 變數路徑": (f'@echo off\ntitle 測試\ncd /d "%~dp0"\nset "JAVA_EXE={JAVA}"\nset RAM=256M\n"%JAVA_EXE%" -Xmx%RAM% ^\n  -Xms64M -jar server.jar nogui %*\npause\n', "direct", 25602),
    "C 自動重開": (":start\r\njava -Xmx256M -jar server.jar nogui\r\ntimeout /t 2\r\ngoto start\r\n", "direct", 25603),
    "D 看不懂":  ("@echo off\r\nif exist server.jar (\r\n  java -Xmx256M -jar server.jar nogui\r\n) else (\r\n  echo 找不到\r\n)\r\npause\r\n", "bat", 25604),
}
base = Path(FAKE_DIR)
for name, (bat, mode, port) in FORMS.items():
    d = base / name
    d.mkdir(parents=True)
    (d / "start.bat").write_bytes(bat.encode("mbcs"))
    (d / "server.properties").write_text(f"server-port={port}\n", encoding="utf-8")
    (d / "eula.txt").write_text("eula=true\n", encoding="utf-8")
    shutil.copy(jar_plain, d / "server.jar")
    assert hub.parse_launch(d).mode == mode, name

# ---------- 假 NeoForge 安裝器：拿到安裝器之後的流程（成功與失敗） ----------
NEO_V = "21.1.999"
installer_jar = work / f"neoforge-{NEO_V}-installer.jar"
with zipfile.ZipFile(installer_jar, "w") as z:
    z.writestr("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\r\nMain-Class: FakeInstaller\r\n\r\n")
    for c in work.glob("*.class"):
        z.write(c, c.name)
inst_logs = Path(tempfile.mkdtemp(prefix="mcinstlog_"))
neo_dir = base / "N 新 NeoForge"
neo_dir.mkdir()
shutil.copy(installer_jar, neo_dir / installer_jar.name)
seen = []
assert hub.install_modloader(neo_dir, JAVA, neo_dir / installer_jar.name, 1, seen.append, inst_logs) == "start.bat"
assert any("installed successfully" in l for l in seen), seen
assert not (neo_dir / installer_jar.name).exists(), "安裝器 jar 應該清掉，不然會被誤認成伺服器本體"
assert (neo_dir / "start.bat").read_bytes() == hub.start_script(
    JAVA, f"-Xmx1G @user_jvm_args.txt @libraries/net/neoforged/neoforge/{NEO_V}/win_args.txt nogui")
assert hub.parse_launch(neo_dir).mode == "direct"
assert hub.inspect_server(neo_dir) == {"version": "1.21.1", "java": 21, "loader": "NeoForge"}
(neo_dir / "server.properties").write_text("server-port=25606\n", encoding="utf-8")
(neo_dir / "eula.txt").write_text("eula=true\n", encoding="utf-8")
fail_dir = base / "fail 安裝失敗"
fail_dir.mkdir()
shutil.copy(installer_jar, fail_dir / installer_jar.name)
try:
    hub.install_modloader(fail_dir, JAVA, fail_dir / installer_jar.name, 1, lambda l: None, inst_logs)
    raise AssertionError("安裝器失敗卻沒報錯")
except hub.UserError as e:
    kept = list((inst_logs / "installer-logs").glob("*.log"))
    assert "結束代碼 1" in str(e) and len(kept) == 1 and str(kept[0]) in str(e), e
    assert "error during installation" in kept[0].read_text(encoding="utf-8")
slow_dir = base / "慢 安裝"
slow_dir.mkdir()
shutil.copy(installer_jar, slow_dir / installer_jar.name)
(slow_dir / "user_jvm_args.txt").mkdir()   # 讓假安裝器寫檔失敗 → 結束代碼不是 0
try:
    hub.install_modloader(slow_dir, JAVA, slow_dir / installer_jar.name, 1, lambda l: None, inst_logs)
    raise AssertionError("安裝器出錯卻沒報錯")
except hub.UserError as e:
    assert "安裝紀錄已保留" in str(e), e
forge_dir = base / "F 舊 Forge"   # 加入現有資料夾：從 libraries 推出 Forge 與遊戲版本
(forge_dir / "libraries/net/minecraftforge/forge/1.20.1-47.4.0").mkdir(parents=True)
assert hub.inspect_server(forge_dir) == {"version": "1.20.1", "java": 17, "loader": "Forge"}

# ---------- 啟動平台 ----------
config_dir = Path(tempfile.mkdtemp(prefix="mccfg_"))
exe = sys.argv[1] if len(sys.argv) > 1 else None
if exe:
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith("PY")}
    env["PATH"] = os.pathsep.join([os.environ["SystemRoot"] + r"\System32", os.environ["SystemRoot"], str(Path(JAVA).parent)])
    proc = subprocess.Popen([exe, "--config", str(config_dir / "c.json"), "--port", str(PORT), "--no-window"], env=env)
else:
    proc = subprocess.Popen([sys.executable, str(ROOT / "src" / "hub.py"), "--config", str(config_dir / "c.json"), "--port", str(PORT), "--no-window"])
try:
    for _ in range(40):
        try:
            page = urlopen(URL + "/", timeout=1).read().decode(); break
        except OSError:
            time.sleep(0.5)
    token = re.search(r'TOKEN = "([^"]+)"', page).group(1)
    assert f"v{hub.VERSION}" in page and "{{" not in page and "@font-face" in page, "版本號或字型沒帶進頁面"
    # 頁面不載入任何外部資源，且有 CSP 標頭擋外連
    for bad in ('<link href="http', "<link rel=\"stylesheet\" href=\"http", 'src="http', "@import", "url(http"):
        assert bad not in page, f"頁面有外部資源：{bad}"
    csp = urlopen(URL + "/", timeout=5).headers.get("Content-Security-Policy", "")
    assert "default-src 'none'" in csp and "connect-src 'self'" in csp and "font-src data:" in csp, csp

    def call(path, body=None, tok=token, host=None):
        h = {"X-Hub-Token": tok, "Content-Type": "application/json"}
        if host:
            h["Host"] = host
        return json.load(urlopen(Request(URL + path, data=json.dumps(body).encode() if body is not None else None, headers=h), timeout=15))

    def servers():
        return {s["id"]: s for s in call("/api/state")["servers"]}

    def wait(sid, pred, secs=40):
        end = time.time() + secs
        while time.time() < end:
            s = servers()[sid]
            if pred(s):
                return s
            time.sleep(0.4)
        raise AssertionError(f"逾時：{servers()[sid]}\n" + "\n".join(t for _, t in call(f"/api/log?id={sid}&since=0")["lines"]))

    for kw in ({"tok": "wrong"}, {"host": "evil.example.com"}):
        try:
            call("/api/state", **kw); raise AssertionError(f"應該被擋：{kw}")
        except HTTPError as e:
            assert e.code == 403

    # 第一次開：沒有伺服器也不能壞，也不能帶別人的設定
    first = call("/api/state")
    assert first["servers"] == [] and first["job"] is None and "update" in first, first
    # 新增的防呆
    empty = base / "空資料夾"; empty.mkdir()
    assert "找不到 start.bat" in call("/api/add", {"dir": str(empty)})["error"]
    assert "找不到這個資料夾" in call("/api/add", {"dir": str(base / "不存在")})["error"]
    # 建服的防呆（沒勾 EULA、怪名稱、已有內容的資料夾，都不該動手）
    good = {"name": "x", "version": "1.21.1", "loader": "vanilla", "ram_gb": 2, "parent": str(base), "eula": True}
    assert "EULA" in call("/api/create", {**good, "eula": False})["error"]
    assert "EULA" in call("/api/create", {**good, "eula": "true"})["error"]
    assert "符號" in call("/api/create", {**good, "name": "..\\x"})["error"]
    assert "已經有東西" in call("/api/create", {**good, "name": "A 單行"})["error"]
    assert call("/api/state")["job"] is None

    # 四種 start.bat：開機 → 看狀態 → 下指令 → stop 存檔關機
    for name, (_, mode, port) in FORMS.items():
        sid = call("/api/add", {"dir": str(base / name)})["id"]
        s = servers()[sid]
        assert s["port"] == port and s["state"] == "stopped", s
        assert call("/api/start", {"id": sid}) == {"ok": True}, name
        s = wait(sid, lambda s: s["state"] == "running" and s["online"] == 1)
        assert s["names"] == ["Tester"], s
        assert s["mem_mb"] and s["mem_mb"] > 20, s   # 要量到真正的 Java，不是外殼程式
        assert "伺服器開著" in call("/api/backup", {"id": sid})["error"], "開機中不該能備份"
        assert "請先關機" in call("/api/remove", {"id": sid})["error"]
        assert call("/api/quit", {}).get("error"), "伺服器開著卻能關平台"
        assert call("/api/force", {"id": sid}).get("error"), "還沒 stop 就能強制結束"
        assert call("/api/cmd", {"id": sid, "cmd": "/list"}) == {"ok": True}
        time.sleep(0.8)
        assert call("/api/stop", {"id": sid}) == {"ok": True}
        s = wait(sid, lambda s: s["state"] == "stopped")
        log = "\n".join(t for _, t in call(f"/api/log?id={sid}&since=0")["lines"])
        assert "There are 1 of a max of 15" in log and "中文顯示正常" in log and "All dimensions are saved" in log, log
        assert "結束代碼 0" in log and s["problem"] is None, log   # 自己存檔關掉，不是被殺
        assert log.count("Loading fake server") == 1, "自動重開迴圈不該再開一次：" + log
        print("開關機通過：", name, mode)

    # 假 NeoForge 伺服器：加入 → 開到 Done → stop 關機 → 從清單拿掉
    nid = call("/api/add", {"dir": str(neo_dir)})["id"]
    assert servers()[nid]["version"] == "1.21.1 · NeoForge", servers()[nid]
    # 記憶體調整（關機時）、自動備份開關、詳細資訊
    assert call("/api/server-settings", {"id": nid, "ram_gb": 2, "auto_backup": True}) == {"ok": True}
    info = call(f"/api/info?id={nid}")
    assert info["ram_gb"] == 2 and info["auto_backup"] is True and info["seed"] == "" and info["seed_note"], info
    assert "1.21.1" in info["join_text"] and "NeoForge" in info["join_text"]
    assert "1 到" in call("/api/server-settings", {"id": nid, "ram_gb": 999})["error"]
    # 圖示與顏色：沒設過就是 None（畫面顯示預設），不自動寫進設定檔；白名單外的擋下；存了之後別的設定再存也不會洗掉
    cfg_file = config_dir / "c.json"
    entry = lambda: next(c for c in json.loads(cfg_file.read_text(encoding="utf-8"))["servers"] if c["id"] == nid)
    assert servers()[nid]["icon"] is None and servers()[nid]["color"] is None and "icon" not in entry() and "color" not in entry()
    assert "圖示" in call("/api/server-settings", {"id": nid, "icon": "../evil", "color": "#5fd39a"})["error"]
    assert "顏色" in call("/api/server-settings", {"id": nid, "icon": "pig", "color": "#123456"})["error"]
    assert "圖示" in call("/api/server-settings", {"id": nid, "color": "#5fd39a"})["error"]   # 沒設過圖示又只送顏色 → 擋
    assert "icon" not in entry()
    assert call("/api/server-settings", {"id": nid, "icon": "pig", "color": "#ff8ab8"}) == {"ok": True}
    assert (servers()[nid]["icon"], servers()[nid]["color"]) == ("pig", "#ff8ab8") and (entry()["icon"], entry()["color"]) == ("pig", "#ff8ab8")
    assert call("/api/address", {"id": nid, "address": "x.example.net"}) == {"ok": True}
    assert entry()["icon"] == "pig" and entry()["address"] == "x.example.net"
    assert call("/api/address", {"id": nid, "address": ""}) == {"ok": True}
    (neo_dir / "world").mkdir(); (neo_dir / "world" / "level.dat").write_bytes(b"w" * 100)
    assert call("/api/start", {"id": nid}) == {"ok": True}
    wait(nid, lambda s: s["state"] == "running" and s["online"] == 1)
    s = wait(nid, lambda s: len(s["mem_hist"]) >= 2, 15)   # 記憶體折線：巡檢有在記，且有上限
    assert all(isinstance(x, int) and x > 0 for x in s["mem_hist"]) and s["xmx_mb"] == 2048, s
    assert hub.Server({"id": "t", "dir": str(neo_dir)}).mem_hist.maxlen == hub.MEM_HISTORY == 60
    assert "先關機" in call("/api/server-settings", {"id": nid, "ram_gb": 3})["error"]
    assert call("/api/server-settings", {"id": nid, "icon": "creeper", "color": "#5fd39a"}) == {"ok": True}   # 開著也能換圖示
    assert servers()[nid]["icon"] == "creeper"
    # 指令 API：後端組字串、危險沒確認被拒、換行被拒、清敵對生物會讀回難度再切回
    cmds = call(f"/api/commands?id={nid}")
    assert any(c["id"] == "fire_tick" for c in cmds["commands"]) and "player" in cmds["fields"]
    assert call("/api/run", {"id": nid, "cmd": "give", "values": {"player": "Tester", "item": "diamond", "count": 3}}) == \
        {"ok": True, "sent": "give Tester minecraft:diamond 3"}
    assert "強確認" in call("/api/run", {"id": nid, "cmd": "op", "values": {"player": "Tester"}})["error"]
    assert "強確認" in call("/api/run", {"id": nid, "cmd": "op", "values": {"player": "Tester"}, "confirm": True})["error"]
    assert "強確認" in call("/api/run", {"id": nid, "cmd": "kill_hostile"})["error"]
    assert "換行" in call("/api/run", {"id": nid, "cmd": "say", "values": {"msg": "hi\nstop"}})["error"]
    assert "換行" in call("/api/cmd", {"id": nid, "cmd": "say hi\nstop"})["error"]
    assert "找不到" in call("/api/run", {"id": nid, "cmd": "nope"})["error"]
    assert call("/api/run", {"id": nid, "cmd": "kill_hostile", "confirm": "strong"}) == {"ok": True}
    time.sleep(3)
    log = "\n".join(t for _, t in call(f"/api/log?id={nid}&since=0")["lines"])
    assert "收到指令：give Tester minecraft:diamond 3" in log and "set to Peaceful" in log and log.rfind("set to Normal") > log.find("set to Peaceful"), log
    assert "收到指令：say hi" not in log and "Stopping" not in log, "換行的指令被送出去了"
    # 設定 API：我的筆記存檔後出現在指令清單、能執行
    st = call("/api/settings", {"custom_commands": [{"id": "my_home", "name": "回家", "tpl": "tp <player> 0 64 0"}]})
    assert st["custom_commands"][0]["id"] == "my_home" and st["links"] == hub.DEFAULT_LINKS
    assert "https://" in call("/api/settings", {"links": [{"name": "x", "url": "http://a"}]})["error"]
    assert any(c["id"] == "my_home" and c["cat"] == "我的筆記" for c in call(f"/api/commands?id={nid}")["commands"])
    assert call("/api/run", {"id": nid, "cmd": "my_home", "values": {"player": "Tester"}})["sent"] == "tp Tester 0 64 0"
    assert call("/api/stop", {"id": nid}) == {"ok": True}
    s = wait(nid, lambda s: s["state"] == "stopped" and not s["backup_busy"] and s["backup"])
    assert s["problem"] is None and "結束代碼 0" in "\n".join(t for _, t in call(f"/api/log?id={nid}&since=0")["lines"])
    assert s["backup"]["ok"] and s["backup"]["files"] == 1, s["backup"]   # 關機後自動備份
    assert b"-Xmx2G @user_jvm_args.txt" in (neo_dir / "start.bat").read_bytes()
    assert call("/api/remove", {"id": nid}) == {"ok": True}
    print("開關機通過：假 NeoForge")
    assert "驚嘆號" in call("/api/create", {**good, "name": "y!", "loader": "neoforge"})["error"]
    assert "類型只能選" in call("/api/create", {**good, "name": "y", "loader": "paper"})["error"]
    assert urlopen(URL + "/api/ping", timeout=5).read() and json.load(urlopen(URL + "/api/ping", timeout=5)) == {"app": hub.APP_ID, "version": hub.VERSION}

    ids = {s["name"]: s["id"] for s in servers().values()}
    assert "已經在清單" in call("/api/add", {"dir": str(base / "A 單行")})["error"]

    # EULA：沒同意就擋、不能代為同意；同意後才寫檔
    a = base / "A 單行"; (a / "eula.txt").unlink()
    assert call("/api/start", {"id": ids["A 單行"]}) == {"need": "eula"}
    assert not (a / "eula.txt").exists()
    assert call("/api/eula", {"id": ids["A 單行"], "agree": False}).get("error") and not (a / "eula.txt").exists()
    assert call("/api/eula", {"id": ids["A 單行"], "agree": True}) == {"ok": True} and hub.eula_agreed(a)

    # 連接埠被占用：開機前擋下並講白話
    with socket.socket() as blocker:
        blocker.bind(("0.0.0.0", 25601)); blocker.listen()
        assert "佔用" in call("/api/start", {"id": ids["A 單行"]})["error"]

    # Java 不夠新：開機前擋下並給下載連結（資料夾 E 的 jar 寫明要 Java 99）
    e = base / "E 要新Java"; e.mkdir()
    (e / "start.bat").write_text("java -jar server.jar nogui\n"); (e / "server.properties").write_text("server-port=25605\n"); (e / "eula.txt").write_text("eula=true\n")
    shutil.copy(jar_java99, e / "server.jar")
    r = call("/api/add", {"dir": str(e)}); sid = r["id"]
    assert servers()[sid]["version"] == "9.9"
    r = call("/api/start", {"id": sid})
    assert "Java 99" in r["error"] and "adoptium.net" in r["help_url"], r
    # 找不到 jar：閃退也要翻成白話
    (e / "server.jar").rename(e / "moved.bak")
    assert call("/api/start", {"id": sid}) == {"ok": True}
    s = wait(sid, lambda s: s["state"] == "stopped" and s["problem"])
    assert "找不到伺服器主程式" in s["problem"], s

    # 備份：對帳一致、顯示位置；移除只拿掉清單
    w = a / "world"; (w / "region").mkdir(parents=True)
    (w / "level.dat").write_bytes(b"x" * 1234); (w / "region" / "r.0.0.mca").write_bytes(b"y" * 8192)
    (a / "world_nether").mkdir(); (a / "world_nether" / "n.dat").write_bytes(b"z" * 10)
    assert call("/api/backup", {"id": ids["A 單行"]}) == {"ok": True}
    s = wait(ids["A 單行"], lambda s: not s["backup_busy"])
    b = s["backup"]
    assert b["ok"] and b["files"] == 3 and b["bytes"] == 1234 + 8192 + 10, b
    dest = Path(b["path"])
    assert hub.tree_stats(dest) == (3, 9436) and (dest / "world" / "region" / "r.0.0.mca").read_bytes() == b"y" * 8192, dest
    assert str(dest) in b["text"]
    no_world = call("/api/backup", {"id": ids["B 變數路徑"]}); assert no_world == {"ok": True}
    assert "找不到世界" in wait(ids["B 變數路徑"], lambda s: not s["backup_busy"])["backup"]["text"]

    assert call("/api/remove", {"id": ids["C 自動重開"]}) == {"ok": True}
    assert ids["C 自動重開"] not in servers() and (base / "C 自動重開" / "server.jar").exists()
    saved = json.loads((config_dir / "c.json").read_text(encoding="utf-8"))
    assert len(saved["servers"]) == 4 and all(c["name"] != "C 自動重開" for c in saved["servers"]), saved
    assert call("/api/address", {"id": ids["A 單行"], "address": "x.example.com"}) == {"ok": True}

    # 平台開著時再開一次：要馬上結束、原本的平台不受影響（同一把鑰匙還能用）
    again = [exe] if exe else [sys.executable, str(ROOT / "src" / "hub.py")]
    assert subprocess.run(again + ["--config", str(config_dir / "c.json"), "--port", str(PORT), "--no-window"], timeout=30).returncode == 0
    assert len(servers()) == 4

    assert call("/api/quit", {}) == {"ok": True}
    proc.wait(timeout=10)
    assert (config_dir / "hub.log").exists()
    print("全部通過")
finally:
    if proc.poll() is None:
        proc.kill()
