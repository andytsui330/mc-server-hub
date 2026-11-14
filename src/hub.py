# MC 開服平台後端：只用 Python 內建模組，打包成 exe 後對方電腦不用裝 Python。
# 視窗由 Edge 的「App 模式」顯示（index.html），本程式負責開關伺服器、建新伺服器與轉送主控台。
import argparse, collections, ctypes, glob, hashlib, json, logging, os, re, secrets, shutil, socket, ssl, struct
import subprocess, sys, threading, time, zipfile
from collections import namedtuple
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.error import URLError
from urllib.parse import parse_qs, quote, urlparse
from urllib.request import HTTPRedirectHandler, HTTPSHandler, Request, build_opener, urlopen

FROZEN = getattr(sys, "frozen", False)
RES_DIR = Path(sys._MEIPASS) if FROZEN else Path(__file__).resolve().parent
VERSION = "1.2.0"           # 版本號全專案只寫這一處
APP_ID = "mc-server-hub"
# 新版提醒只讀這個固定網址上的版本號，不下載、不換檔。
VERSION_URL = "https://mc-hub-version.pages.dev/version.json"
STOP_GRACE_S = 120          # 送出 stop 超過這麼久還沒關，才允許「強制結束」
IDLE_EXIT_S = 180           # 沒有伺服器在跑、視窗也關了這麼久，平台自動收工
MEM_HISTORY = 60            # 記憶體折線每台保留最近幾筆（巡檢每 3 秒一次＝約 3 分鐘）
# 頁面只准用自己內嵌的樣式、程式、字型，只連回平台自己；不載入任何外部資源。
CSP = ("default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; font-src data:; img-src data:; "
       "connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'")
MANIFEST_URL = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
FABRIC_META = "https://meta.fabricmc.net/v2/versions"
# 只從這些官方網域下載；回傳內容裡的網址也要符合。
ALLOWED_HOSTS = {"piston-meta.mojang.com", "piston-data.mojang.com", "launchermeta.mojang.com",
                 "launcher.mojang.com", "meta.fabricmc.net",
                 "files.minecraftforge.net", "maven.minecraftforge.net", "maven.neoforged.net"}
FORGE_PROMOS = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json"
FORGE_MAVEN = "https://maven.minecraftforge.net/net/minecraftforge/forge"
NEOFORGE_VERSIONS = "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge"
NEOFORGE_MAVEN = "https://maven.neoforged.net/releases/net/neoforged/neoforge"
INSTALL_TIMEOUT_S = 900     # Forge／NeoForge 安裝器最多跑 15 分鐘
EULA_URL = "https://aka.ms/MinecraftEULA"
ACCENTS = ["#5cc8ff", "#ff7ab6", "#3ddc97", "#ffb547", "#a98bff", "#ff8a4c"]
# 1.1.1 伺服器圖示與顏色：畫面（index.html 的 ICONS／COLORS）自己畫，這裡只放白名單，存檔前檢查。
# 兩邊要一致，tests/test_hub.py 會比對。
ICON_IDS = frozenset((
    "grass", "dirt", "stone", "cobble", "sand", "snow", "ice", "water", "lava", "planks", "log", "leaves", "bricks",
    "glass", "pumpkin", "obsidian", "nether", "endstone", "amethyst", "ore-diamond", "ore-gold", "ore-emerald",
    "ore-redstone", "ore-lapis", "ore-iron", "ore-coal", "diamond", "emerald", "ruby", "gold", "iron", "crown",
    "star", "heart", "sword", "pickaxe", "axe", "shield", "bomb", "torch", "potion", "chest", "book", "map",
    "compass", "pad", "apple", "carrot", "cake", "fish", "mushroom", "tree", "flower", "fire", "island", "castle",
    "house", "portal", "moon", "sun", "pig", "cow", "sheep", "chicken", "horse", "rabbit", "wolf", "cat", "fox",
    "panda", "bee", "parrot", "villager", "golem", "axolotl", "frog", "turtle", "squid", "dolphin", "creeper",
    "zombie", "skeleton", "spider", "enderman", "slime", "witch", "pillager", "blaze", "ghast", "piglin", "wither",
    "dragon", "warden",
))
ICON_COLORS = ("#5fd39a", "#57c1ff", "#b48cff", "#ff8ab8", "#ff6b6b", "#ffa45c", "#f2d16b", "#f4f4f6")


class UserError(Exception):
    """要直接顯示給使用者看的白話錯誤。"""
    def __init__(self, msg, help_url=None):
        super().__init__(msg)
        self.help_url = help_url


# ========== 系統差異：只有這一區看作業系統，其他程式一律呼叫這裡 ==========
# Windows 是現況；Mac 先只做純字串部分，實際功能 1.2.0 才做。
OS = "win" if sys.platform == "win32" else "mac" if sys.platform == "darwin" else "other"
TEXT_ENC = "mbcs" if OS == "win" else "utf-8"   # Windows 記事本存的中文 bat 多半是 Big5
NO_WINDOW = 0x08000000 if OS == "win" else 0    # CREATE_NO_WINDOW：子程序不跳黑窗


class NotYet(UserError):
    def __init__(self, what):
        super().__init__(f"{what}：Mac 版要到 1.2.0 才支援。")


def data_dir(os_name=OS, env=os.environ, home=None):
    """設定檔放固定位置：exe 每次解壓到暫存資料夾，放旁邊會不見。"""
    home = Path(home or Path.home())
    if os_name == "win":
        return Path(env.get("APPDATA") or home) / "MCServerHub"
    if os_name == "mac":
        return home / "Library" / "Application Support" / "MCServerHub"
    return home / ".config" / "MCServerHub"


def start_script_name(os_name=OS):
    return "start.bat" if os_name == "win" else "start.sh"


# 給人看的字：訊息與畫面依系統換成對的檔名與工具名
SCRIPT = start_script_name()
EDITOR = "記事本" if OS == "win" else "「文字編輯」"
APP_FILE = "MC開服平台.exe" if OS == "win" else "MC開服平台"


def args_file_name(os_name=OS):
    """Forge／NeoForge 安裝後的參數檔。"""
    return "win_args.txt" if os_name == "win" else "unix_args.txt"


def start_script(java, args, os_name=OS):
    """平台自己產生的啟動檔內容（bytes）。Windows：CRLF＋系統編碼；Mac：LF＋UTF-8。"""
    if os_name == "win":
        return f'@echo off\r\ncd /d "%~dp0"\r\n"{java}" {args}\r\npause\r\n'.encode(TEXT_ENC)
    return f'#!/bin/sh\ncd "$(dirname "$0")"\nexec "{java}" {args}\n'.encode("utf-8")


def write_start_script(folder, java, args):
    name = start_script_name()
    path = Path(folder) / name
    path.write_bytes(start_script(java, args))
    if OS != "win":
        path.chmod(0o755)
    return name


def java_help_url(need, os_name=OS, arch=None):
    if os_name == "win":
        return f"https://adoptium.net/temurin/releases/?version={need or 21}&os=windows&arch=x64"
    return f"https://adoptium.net/temurin/releases/?version={need or 21}&os=mac&arch={arch or mac_arch()}"


if OS == "win":
    from ctypes import wintypes

    class _PMC(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
            (n, ctypes.c_size_t) for n in ("PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
                                           "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
                                           "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]

    class _PE32(ctypes.Structure):
        _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD), ("th32ProcessID", wintypes.DWORD),
                    ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", wintypes.DWORD), ("cntThreads", wintypes.DWORD),
                    ("th32ParentProcessID", wintypes.DWORD), ("pcPriClassBase", wintypes.LONG), ("dwFlags", wintypes.DWORD),
                    ("szExeFile", ctypes.c_wchar * 260)]

    _k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    _k32.OpenProcess.restype = wintypes.HANDLE
    _k32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    _k32.K32GetProcessMemoryInfo.argtypes = (wintypes.HANDLE, ctypes.POINTER(_PMC), wintypes.DWORD)
    _k32.CloseHandle.argtypes = (wintypes.HANDLE,)
    _k32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    _k32.CreateToolhelp32Snapshot.argtypes = (wintypes.DWORD, wintypes.DWORD)
    _k32.Process32FirstW.argtypes = _k32.Process32NextW.argtypes = (wintypes.HANDLE, ctypes.POINTER(_PE32))

    def mem_mb(pid):
        h = _k32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
        if not h:
            return None
        try:
            pmc = _PMC(); pmc.cb = ctypes.sizeof(pmc)
            return round(pmc.WorkingSetSize / 1048576) if _k32.K32GetProcessMemoryInfo(h, ctypes.byref(pmc), pmc.cb) else None
        finally:
            _k32.CloseHandle(h)

    def tree_mem_mb(pid):
        """連子程序一起算：Oracle 的 java.exe 和 cmd.exe 都只是外殼，真正吃記憶體的 Java 在底下。"""
        snap = _k32.CreateToolhelp32Snapshot(2, 0)   # TH32CS_SNAPPROCESS
        children = collections.defaultdict(list)
        try:
            pe = _PE32(); pe.dwSize = ctypes.sizeof(pe)
            ok = _k32.Process32FirstW(snap, ctypes.byref(pe))
            while ok:
                children[pe.th32ParentProcessID].append(pe.th32ProcessID)
                ok = _k32.Process32NextW(snap, ctypes.byref(pe))
        finally:
            _k32.CloseHandle(snap)
        total, todo = 0, [pid]
        while todo:
            p = todo.pop()
            total += mem_mb(p) or 0
            todo += [c for c in children[p] if c != p]
        return total or None

    def total_ram_gb():
        kb = ctypes.c_ulonglong(0)
        return round(kb.value / 1048576) if _k32.GetPhysicallyInstalledSystemMemory(ctypes.byref(kb)) else 8

    def kill_tree(pid):
        subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"], stdin=subprocess.DEVNULL,
                       capture_output=True, creationflags=NO_WINDOW)

    def open_path(path):
        os.startfile(path)

    def fatal_box(msg):
        ctypes.windll.user32.MessageBoxW(None, msg, "MC 開服平台", 0x10)

    def reset_dll_dir():
        if FROZEN:   # 打包版會把 DLL 搜尋路徑指到暫存資料夾，別讓 Java 子程序繼承
            _k32.SetDllDirectoryW(None)
else:
    # Mac（1.2.0）：記憶體看 ps、總記憶體看 sysctl、伺服器開在自己的程序群組裡，強制結束時整組一起關。
    def _ps_table(run=subprocess.run):
        """回傳 {pid: (ppid, rss_kb)}；ps 失敗回空表。"""
        try:
            out = run(["ps", "-A", "-o", "pid=,ppid=,rss="], stdin=subprocess.DEVNULL, capture_output=True, timeout=5).stdout
        except (OSError, subprocess.SubprocessError):
            return {}
        table = {}
        for line in out.decode("ascii", "replace").splitlines():
            parts = line.split()
            if len(parts) == 3 and all(p.isdigit() for p in parts):
                table[int(parts[0])] = (int(parts[1]), int(parts[2]))
        return table

    def mem_mb(pid, table=None):
        row = (table if table is not None else _ps_table()).get(pid)
        return round(row[1] / 1024) if row else None

    def tree_mem_mb(pid, table=None):
        """連子程序一起算：start.sh 只是外殼，真正吃記憶體的 Java 在底下。"""
        table = table if table is not None else _ps_table()
        children = collections.defaultdict(list)
        for p, (pp, _) in table.items():
            children[pp].append(p)
        total, todo, seen = 0, [pid], set()
        while todo:
            p = todo.pop()
            if p in seen:
                continue
            seen.add(p)
            total += table[p][1] if p in table else 0
            todo += children[p]
        return round(total / 1024) or None

    def total_ram_gb(run=subprocess.run):
        try:
            out = run(["sysctl", "-n", "hw.memsize"], stdin=subprocess.DEVNULL, capture_output=True, timeout=5).stdout
            return round(int(out.strip()) / 1073741824) or 8
        except (OSError, subprocess.SubprocessError, ValueError):
            return 8

    def kill_tree(pid):
        import signal
        try:
            os.killpg(os.getpgid(pid), signal.SIGKILL)   # 開機時 start_new_session=True，整組就是這台伺服器
        except (ProcessLookupError, PermissionError):
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass

    def open_path(path):
        subprocess.Popen(["open" if OS == "mac" else "xdg-open", str(path)], stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def fatal_box(msg):
        print(msg, file=sys.stderr)
        if OS == "mac":
            script = f'display alert "MC 開服平台" message {json.dumps(msg)} as critical'
            subprocess.run(["osascript", "-e", script], stdin=subprocess.DEVNULL, capture_output=True, timeout=600)

    def reset_dll_dir():
        pass


def popen_server(cmd, cwd, **kw):
    """開伺服器子程序。Windows 直接給整串指令；Mac 拆成清單，並開新程序群組讓強制結束能整組關掉。"""
    if OS == "win":
        return subprocess.Popen(cmd, cwd=cwd, creationflags=NO_WINDOW, **kw)
    import shlex
    return subprocess.Popen(shlex.split(cmd), cwd=cwd, start_new_session=True, **kw)


def mac_arch(run=subprocess.run):
    """Apple 晶片上跑 Intel 版程式（Rosetta）時 platform 會說 x86_64，要另外問系統。"""
    try:
        r = run(["sysctl", "-n", "sysctl.proc_translated"], stdin=subprocess.DEVNULL, capture_output=True, timeout=5)
        if r.stdout.strip() == b"1":
            return "aarch64"
    except (OSError, subprocess.SubprocessError):
        pass
    import platform
    return "aarch64" if platform.machine() in ("arm64", "aarch64") else "x64"


PICK_FOLDER_PS = r"""
[Console]::OutputEncoding = [Text.Encoding]::UTF8
Add-Type -AssemblyName System.Windows.Forms
$owner = New-Object System.Windows.Forms.Form
$owner.TopMost = $true; $owner.ShowInTaskbar = $false; $owner.Opacity = 0; $owner.Show()
$d = New-Object System.Windows.Forms.FolderBrowserDialog
$d.ShowNewFolderButton = $true
if ($d.ShowDialog($owner) -eq 'OK') { $d.SelectedPath }
$owner.Close()
"""


PICK_FOLDER_OSA = 'POSIX path of (choose folder with prompt "選擇資料夾")'


def pick_folder():
    if OS == "mac":
        try:
            r = subprocess.run(["osascript", "-e", PICK_FOLDER_OSA], stdin=subprocess.DEVNULL, capture_output=True, timeout=600)
        except (OSError, subprocess.SubprocessError) as e:
            return {"error": f"資料夾選擇視窗打不開（{e}），請直接把路徑貼到欄位裡"}
        path = r.stdout.decode("utf-8", "replace").strip()   # 按取消：結束代碼 1、沒有輸出
        return {"path": path.rstrip("/") if len(path) > 1 else path}
    if OS != "win":
        return {"error": "這個系統沒有資料夾選擇視窗，請直接把路徑貼到欄位裡"}
    try:
        r = subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", PICK_FOLDER_PS], stdin=subprocess.DEVNULL,
                           capture_output=True, timeout=600, creationflags=NO_WINDOW)
    except (OSError, subprocess.SubprocessError) as e:
        return {"error": f"資料夾選擇視窗打不開（{e}），請直接把路徑貼到欄位裡"}
    return {"path": r.stdout.decode("utf-8", "replace").strip().lstrip("﻿")}


def open_window(url):
    if OS == "win":
        for base in ("ProgramFiles(x86)", "ProgramFiles"):
            edge = Path(os.environ.get(base, "")) / "Microsoft/Edge/Application/msedge.exe"
            if edge.exists():
                subprocess.Popen([str(edge), f"--app={url}", "--window-size=1360,900"], stdin=subprocess.DEVNULL,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                return
    if OS == "mac":   # 有 Chrome／Edge 就用 App 模式（沒有網址列，像一般程式視窗），否則交給預設瀏覽器
        # 用 open -na 交給系統啟動，瀏覽器才是獨立的程式。直接執行瀏覽器主程式的話，它會變成平台的子程序，
        # 之後瀏覽器更新自己時 macOS 會算在平台頭上，跳出「已防止 MC開服平台 修改 App」（2026-11-14 實測）。
        for app in ("Google Chrome", "Microsoft Edge"):
            for root in ("/Applications", str(Path.home() / "Applications")):
                bundle = Path(root) / f"{app}.app"
                if (bundle / "Contents" / "MacOS" / app).exists():
                    subprocess.Popen(["open", "-na", str(bundle), "--args", f"--app={url}", "--window-size=1360,900"],
                                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    return
    import webbrowser
    webbrowser.open(url)


DATA_DIR = data_dir()
# ========== 系統差異區塊結束 ==========


# ---------- Minecraft 狀態查詢（Server List Ping，遊戲列表看到的那份資料） ----------
def _varint(n):
    out = b""
    while True:
        b, n = n & 0x7F, n >> 7
        out += bytes([b | (0x80 if n else 0)])
        if not n:
            return out


def _read_varint(sock):
    num = shift = 0
    while True:
        c = sock.recv(1)
        if not c:
            raise ConnectionError("eof")
        num |= (c[0] & 0x7F) << shift
        if not c[0] & 0x80:
            return num
        shift += 7


def slp_ping(port, timeout=1.5):
    """回傳伺服器狀態 dict；連不上就丟例外。"""
    with socket.create_connection(("127.0.0.1", port), timeout=timeout) as s:
        host = b"127.0.0.1"
        hs = _varint(0) + _varint(767) + _varint(len(host)) + host + struct.pack(">H", port) + _varint(1)
        s.sendall(_varint(len(hs)) + hs + b"\x01\x00")
        _read_varint(s); _read_varint(s)
        size, buf = _read_varint(s), b""
        while len(buf) < size:
            chunk = s.recv(size - len(buf))
            if not chunk:
                raise ConnectionError("eof")
            buf += chunk
        return json.loads(buf.decode("utf-8", "replace"))


# ---------- 看懂 start.bat ----------
Launch = namedtuple("Launch", "cmd java mode")   # mode：direct＝平台直接跑 java；bat＝看不懂，執行 bat 本身
_SKIP_WORDS = {"rem", "title", "chcp", "color", "cls", "setlocal", "endlocal", "pause", "timeout", "exit", "mode", "popd"}


def read_text_any(path):
    data = Path(path).read_bytes()
    try:
        return data.decode("utf-8-sig")
    except UnicodeDecodeError:
        return data.decode(TEXT_ENC, "replace")


def _expand(s, env, server_dir):
    """把 bat 變數換成實際值；有不認識的變數就回 None（交給 bat 自己跑）。"""
    s = re.sub(r"%~dp0", lambda _: str(Path(server_dir)) + "\\", s, flags=re.I)
    s = re.sub(r"%[*\d]", "", s)
    missing = []

    def rep(m):
        v = env.get(m[1].upper())
        if v is None:
            missing.append(m[1])
            return ""
        return v
    s = re.sub(r"%([^%\s]+)%", rep, s)
    return None if missing or "%" in s else s


def _first_token(s):
    """回傳 (執行檔, 指令中執行檔結束的位置)。"""
    if s.startswith('"'):
        end = s.find('"', 1)
        return (s[1:end], end + 1) if end > 0 else (s[1:], len(s))
    parts = s.split(None, 1)
    return parts[0], len(parts[0])


def _java_mentioned(text):
    """bat 看不懂時，盡量找出它會用哪個 Java，開機前才查得了版本。"""
    for raw in text.splitlines():
        line = raw.strip().lstrip("@").strip()
        if not line or re.match(r"(?i)(rem\b|echo\b|::)", line):
            continue
        m = re.search(r'"([^"%\r\n]*\\javaw?(?:\.exe)?)"', line, re.I)
        if m:
            return m[1]
        if re.search(r"(?i)(^|[\s(])javaw?(\.exe)?\s+-", line):
            return "java"
    return None


_SH_CD_OK = {'cd "$(dirname "$0")"', 'cd "$(dirname -- "$0")"', "cd \"$(dirname \"$0\")\" || exit 1",
             'cd "$(dirname "$0")" || exit', "cd .", "cd ./", "set -e"}


def parse_launch_sh(server_dir, script="start.sh"):
    """Mac 的 start.sh／run.sh：只有一行 java（可加 exec、結尾 "$@"）就直接跑；其他寫法交給 sh 執行。"""
    text = read_text_any(Path(server_dir) / script)
    java = re.search(r'(?m)^\s*(?:exec\s+)?"?([^\s"]*/java|java)"?\s+', text)
    fallback = Launch(f"/bin/sh ./{script}", java[1] if java else None, "bat")
    java_line = None
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or line in _SH_CD_OK:
            continue
        if java_line is not None:
            return fallback
        line = re.sub(r"^exec\s+", "", line)
        line = re.sub(r'\s+"\$@"\s*$', "", line)
        if re.search(r"[$`|<>&;()\\]", line):
            return fallback
        import shlex
        try:
            parts = shlex.split(line)
        except ValueError:
            return fallback
        if not parts or Path(parts[0]).name != "java":
            return fallback
        java_line = line
    if java_line is None:
        return fallback
    exe, end = _first_token(java_line)
    return Launch(java_line[:end] + " -Dstdout.encoding=UTF-8 -Dstderr.encoding=UTF-8" + java_line[end:], exe, "direct")


def parse_launch(server_dir, bat="start.bat"):
    """看得懂就抽出 java 指令直接跑（多加 UTF-8 參數防亂碼）；有分支、管線等複雜寫法就執行 bat 本身。"""
    if bat.lower().endswith(".sh"):
        return parse_launch_sh(server_dir, bat)
    text = read_text_any(Path(server_dir) / bat)
    # Forge 1.21 的 run.bat：先跑 shim.jar --onlyCheckJava 檢查 Java、失敗就 goto 結尾，再跑真正那行。
    # 平台開機前自己會檢查 Java，所以把這段拿掉，改直接跑那行（格式依 2026-11-13 真實安裝 52.1.20 的產物）。
    text = re.sub(r"(?im)^[ \t]*java[ \t]+-jar[ \t]+\S+-shim\.jar[ \t]+--onlyCheckJava[ \t]*\r?\n"
                  r"[ \t]*if[ \t]+%ERRORLEVEL%[ \t]+NEQ[ \t]+0[ \t]+\([ \t]*\r?\n(?:(?![ \t]*\)).*\r?\n)*?[ \t]*\)[ \t]*\r?$", "", text)
    lines = re.sub(r"\^[ \t]*\r?\n[ \t]*", " ", text).splitlines()
    # 要寫 .\：有些環境（NoDefaultCurrentDirectoryInExePath）cmd 不會找目前資料夾。
    fallback = Launch(f'cmd.exe /d /c ".\\{bat}"', _java_mentioned(text), "bat")
    env = {k.upper(): v for k, v in os.environ.items()}
    env["CD"] = str(Path(server_dir))
    java_line, java_idx, labels, gotos = None, -1, {}, []
    for i, raw in enumerate(lines):
        line = raw.strip().lstrip("@").strip()
        if not line or line.startswith("::"):
            continue
        if line.startswith(":"):
            labels[(line[1:].split() or [""])[0].lower()] = i
            continue
        word = line.split(None, 1)[0].lower()
        if word in _SKIP_WORDS or re.match(r"echo\b|echo[.:,]", word):
            continue
        if word in ("cd", "chdir", "pushd"):
            arg = re.sub(r"(?i)^/d\s+", "", line.split(None, 1)[1] if " " in line else "").strip().strip('"').rstrip("\\/")
            if arg.lower() not in ("%~dp0", ".", "%cd%"):
                return fallback
            continue
        if word == "set":
            m = re.match(r'(?i)set\s+"([^=]+)=(.*)"\s*$', line) or re.match(r'(?i)set\s+([^=\s/"][^=]*)=(.*)$', line)
            value = _expand(m[2], env, server_dir) if m else None
            if value is None:
                return fallback
            env[m[1].strip().upper()] = value
            continue
        if word == "goto":
            gotos.append((i, line.split(None, 1)[1].strip().lstrip(":").lower() if " " in line else ""))
            continue
        expanded = _expand(line, env, server_dir)
        if expanded is None:
            return fallback
        exe, _ = _first_token(expanded)
        if Path(exe).stem.lower() not in ("java", "javaw") or java_line is not None:
            return fallback
        if re.search(r"[|<>&]", re.sub(r'"[^"]*"', "", expanded)):
            return fallback
        java_line, java_idx = expanded, i
    if java_line is None:
        return fallback
    # 自動重開的迴圈（java 之後 goto 回 java 之前）平台不需要；其他 goto 都交給 bat 自己跑。
    if any(idx < java_idx or labels.get(target, java_idx + 1) > java_idx for idx, target in gotos):
        return fallback
    exe, end = _first_token(java_line)
    return Launch(java_line[:end] + " -Dstdout.encoding=UTF-8 -Dstderr.encoding=UTF-8" + java_line[end:], exe, "direct")


def read_prop(server_dir, key):
    try:
        for line in read_text_any(Path(server_dir) / "server.properties").splitlines():
            if line.startswith(key + "="):
                return line.split("=", 1)[1].strip()
    except OSError:
        pass
    return None


def read_port(server_dir, fallback=25565):
    try:
        return int(read_prop(server_dir, "server-port"))
    except (TypeError, ValueError):
        return fallback


def port_free(port):
    with socket.socket() as s:
        try:
            s.bind(("0.0.0.0", port))
            return True
        except OSError:
            return False


def eula_agreed(server_dir):
    try:
        return bool(re.search(r"(?im)^\s*eula\s*=\s*true\s*$", read_text_any(Path(server_dir) / "eula.txt")))
    except OSError:
        return False


def write_eula(server_dir):
    (Path(server_dir) / "eula.txt").write_text(f"# {EULA_URL}\n# Agreed by the user in MC Server Hub.\neula=true\n", encoding="utf-8")


def find_bat(d, os_name=OS):
    """找啟動檔：Windows 找 .bat，Mac 找 .sh（名稱沿用 bat 是 1.0.0 設定檔的欄位名）。"""
    name, ext = start_script_name(os_name), "*.bat" if os_name == "win" else "*.sh"
    if (d / name).is_file():
        return name
    with_java = [p.name for p in d.glob(ext) if "java" in read_text_any(p).lower()]
    return with_java[0] if len(with_java) == 1 else None


# ---------- Java ----------
def java_major(exe):
    try:
        r = subprocess.run([exe, "-version"], stdin=subprocess.DEVNULL, capture_output=True, timeout=20, creationflags=NO_WINDOW)
    except (OSError, subprocess.SubprocessError):
        return None
    m = re.search(rb'version "(\d+)(?:\.(\d+))?', r.stderr + r.stdout)
    if not m:
        return None
    return int(m[2] or 0) if m[1] == b"1" else int(m[1])


def installed_javas():
    """掃 PATH 和常見安裝位置：剛裝好的 Java，平台不用重開也找得到。"""
    paths = [shutil.which("java")]
    if OS == "win":
        for root in filter(None, (os.environ.get("ProgramW6432"), os.environ.get("ProgramFiles"),
                                  os.path.join(os.environ.get("LOCALAPPDATA", ""), "Programs"))):
            for vendor in ("Java", "Eclipse Adoptium", "Eclipse Foundation", "AdoptOpenJDK", "Microsoft", "Zulu", "BellSoft", "Amazon Corretto", "Semeru"):
                paths += glob.glob(os.path.join(root, vendor, "*", "bin", "java.exe"))
    else:
        paths = mac_java_paths()
    found, seen = [], set()
    for p in filter(None, paths):
        key = os.path.normcase(p)
        if key not in seen:
            seen.add(key)
            major = java_major(p)
            if major:
                found.append((major, p))
    return found


def mac_java_paths(home=None, run=subprocess.run):
    """Mac 的 /usr/bin/java 只是轉接頭（沒裝 Java 時會跳安裝提示），所以直接找各 JDK 的實際位置。"""
    home = Path(home or Path.home())
    paths = []
    for jvm in ("/Library/Java/JavaVirtualMachines", str(home / "Library/Java/JavaVirtualMachines")):
        paths += glob.glob(os.path.join(jvm, "*", "Contents", "Home", "bin", "java"))
    for brew in ("/opt/homebrew/opt", "/usr/local/opt"):
        paths += glob.glob(os.path.join(brew, "openjdk*", "bin", "java"))
    try:   # java_home -V 把清單印在 stderr，每行最後是 JDK 的 Home 路徑
        r = run(["/usr/libexec/java_home", "-V"], stdin=subprocess.DEVNULL, capture_output=True, timeout=10)
        for line in r.stderr.decode("utf-8", "replace").splitlines():
            m = re.search(r"(/\S.*?/Contents/Home)\s*$", line)
            if m:
                paths.append(m[1] + "/bin/java")
    except (OSError, subprocess.SubprocessError):
        pass
    return [p for p in paths if os.path.isfile(p)]


def best_java(need):
    """挑夠用裡最接近需求的版本（太新的 Java 舊模組容易出問題）。"""
    ok = [j for j in installed_javas() if j[0] >= (need or 8)]
    return min(ok, key=lambda j: j[0]) if ok else None


def java_for_mc(version):
    m = re.fullmatch(r"1\.(\d+)(?:\.(\d+))?", version or "")
    if not m:
        return None   # 新版號不在表內；版本資料有寫就用資料，沒寫靠開機錯誤翻譯
    v = (int(m[1]), int(m[2] or 0))
    return 21 if v >= (20, 5) else 17 if v >= (18, 0) else 16 if v >= (17, 0) else 8


def _num_key(p):
    return [int(x) for x in re.findall(r"\d+", p.name)]


def modloader_version(d):
    """從 libraries 資料夾推 Forge／NeoForge 對應的遊戲版本；回傳 (遊戲版本, 載入器) 或 (None, None)。"""
    d = Path(d)
    for p in sorted((d / "libraries/net/neoforged/neoforge").glob("*"), key=_num_key, reverse=True):
        if p.is_dir() and re.fullmatch(r"\d+(\.\d+)+", p.name) and neoforge_mc(p.name):
            return neoforge_mc(p.name), "NeoForge"
    for p in sorted((d / "libraries/net/minecraftforge/forge").glob("*"), key=_num_key, reverse=True):
        m = re.fullmatch(r"(\d+(?:\.\d+)+)-[\d.]+", p.name)
        if p.is_dir() and m:
            return m[1], "Forge"
    return None, None


def inspect_server(d):
    """從 jar 或紀錄檔判斷遊戲版本、類型與需要的 Java，判斷不出來就留空。"""
    d, info = Path(d), {"version": None, "java": None, "loader": ""}
    mc, loader = modloader_version(d)
    if mc:   # Forge／NeoForge：資料夾裡的 jar 不是伺服器本體，看 libraries 就夠
        return {"version": mc, "java": java_for_mc(mc), "loader": loader}
    for jar in list(d.glob("*.jar")) + list(d.glob(".fabric/server/*.jar")) + list(d.glob("versions/*/*.jar")):
        try:
            with zipfile.ZipFile(jar) as z:
                names = set(z.namelist())
                if "version.json" in names:
                    v = json.loads(z.read("version.json"))
                    info["version"], info["java"] = v.get("id") or info["version"], v.get("java_version")
                elif "install.properties" in names and not info["version"]:
                    m = re.search(rb"game-version=(\S+)", z.read("install.properties"))
                    info["version"] = m[1].decode() if m else None
        except (OSError, zipfile.BadZipFile, ValueError, KeyError):
            continue
        if info["java"]:
            break
    if not info["version"]:
        try:
            m = re.search(r"Starting minecraft server version (\S+)", read_text_any(d / "logs" / "latest.log"))
            info["version"] = m[1] if m else None
        except OSError:
            pass
    info["java"] = info["java"] or java_for_mc(info["version"])
    names = " ".join(p.name.lower() for p in d.glob("*.jar"))
    info["loader"] = ("Fabric" if "fabric" in names or (d / ".fabric").is_dir() else
                      "NeoForge" if (d / "libraries/net/neoforged").is_dir() else
                      "Forge" if (d / "libraries/net/minecraftforge").is_dir() else
                      "Paper" if "paper" in names else "")
    return info


# ---------- 把常見的開機失敗翻成白話 ----------
_FAILURES = [
    (r"UnsupportedClassVersionError.*?class file version (\d+)",
     lambda m: f"Java 版本太舊：這台伺服器需要 Java {int(m[1]) - 44} 以上，請安裝後再開機。"),
    (r"Unable to access jarfile", lambda m: "找不到伺服器主程式（jar 檔）。請確認 " + SCRIPT + " 寫的檔名和資料夾裡的檔案一樣。"),
    (r"You need to agree to the EULA", lambda m: "還沒同意 Minecraft 授權合約（EULA）。再按一次開機就會跳出同意畫面。"),
    (r"FAILED TO BIND TO PORT|Address already in use", lambda m: "連接埠被別的程式佔用了。請關掉另一台伺服器，或改 server.properties 的 server-port。"),
    (r"Could not reserve enough space|Invalid (?:maximum|initial) heap size|Initial heap size set to a larger value|insufficient memory",
     lambda m: "記憶體不夠：" + SCRIPT + " 的 -Xmx 設得比電腦能給的還多。請改小一點，或先關掉其他程式。"),
    (r"'[^'\n]*java[^'\n]*'\s*(?:is not recognized|不是內部或外部命令)|java: (?:command )?not found|No Java runtime present|Unable to locate a Java Runtime",
     lambda m: "找不到 Java。請先安裝 Java，或確認 " + SCRIPT + " 裡的路徑沒打錯。"),
    (r"Incompatible mods found|Some of your mods are incompatible|Mod resolution encountered", lambda m: "模組有問題：有模組缺少必要的前置模組，或和這個遊戲版本不合。請看主控台最後幾行寫的是哪一個模組。"),
    # Forge／NeoForge（FML）缺前置模組或版本不合時的訊息
    (r"Missing or unsupported mandatory dependencies",
     lambda m: "模組少了前置模組，或前置模組版本不合（Forge／NeoForge）。主控台會寫哪個模組需要什麼，請到該模組的下載頁補上對應版本，放進 mods 資料夾。"),
]


def explain_failure(lines, code, saw_done):
    text = "\n".join(lines[-200:])
    for pattern, message in _FAILURES:
        m = re.search(pattern, text)
        if m:
            return message(m)
    if saw_done:
        return f"伺服器意外關閉（結束代碼 {code}）。請看主控台最後幾行，或資料夾裡的 crash-reports。" if code else None
    return f"伺服器還沒開好就結束了（結束代碼 {code}）。請看下方主控台最後幾行的錯誤訊息。"


def tree_stats(root):
    files = size = 0
    for folder, _, names in os.walk(root):
        for n in names:
            files += 1
            size += os.path.getsize(os.path.join(folder, n))
    return files, size


# ---------- 單台伺服器 ----------
class Server:
    def __init__(self, cfg):
        self.cfg = cfg
        self.id, self.dir = cfg["id"], cfg["dir"]
        self.port = read_port(self.dir)
        self.proc = self.launch = self.problem = self.backup_info = None
        self.state = "stopped"
        self.started_at = self.stop_sent_at = None
        self.ping = None
        self.external = self.saw_done = self.backup_busy = False
        self.lock = threading.Lock()
        self.log = collections.deque(maxlen=3000)
        self.seq = 0
        self.mem_hist = collections.deque(maxlen=MEM_HISTORY)   # 記憶體折線：每次巡檢記一筆（MB）

    def add_log(self, text):
        with self.lock:
            self.seq += 1
            self.log.append((self.seq, text))

    def logs_since(self, since):
        with self.lock:
            return self.seq, [l for l in self.log if l[0] > since]

    def alive(self):
        return self.proc is not None and self.proc.poll() is None

    def preflight(self):
        """開機前檢查；回傳 (launch, 問題)，有問題時 launch 是 None。"""
        bat = self.cfg.get("bat", "start.bat")
        try:
            launch = parse_launch(self.dir, bat)
        except OSError:
            return None, f"在「{self.dir}」找不到 {bat}。資料夾可能被移動或刪除了，請從清單移除後重新加入。"
        need = self.cfg.get("java_need") or inspect_server(self.dir)["java"]
        if launch.java:
            have = java_major(launch.java)
            if have is None or (need and have < need):
                better = best_java(need) if launch.mode == "direct" and launch.java.lower() in ("java", "java.exe") else None
                if better:   # PATH 上的 Java 不夠新，但電腦裡有裝夠新的，就直接用它
                    self.add_log(f"» 平台：改用電腦裡的 Java {better[0]}（{better[1]}）")
                    _, end = _first_token(launch.cmd)
                    launch = Launch(f'"{better[1]}"' + launch.cmd[end:], better[1], "direct")
                else:
                    wanted = f"Java {need} 以上" if need else "Java"
                    now = "找不到 Java" if have is None else f"目前用的是 Java {have}"
                    return None, {"error": f"這台伺服器需要 {wanted}，但電腦上{now}。請點下方按鈕下載安裝，裝好再按一次開機，平台不用重開。"
                                           + (f"（已經裝了的話，請檢查 {SCRIPT} 裡寫的 Java 路徑。）" if launch.mode == "bat" or launch.java.lower() not in ("java", "java.exe") else ""),
                                  "help_url": java_help_url(need)}
        if not eula_agreed(self.dir):
            return None, {"need": "eula"}
        if not port_free(self.port):
            return None, f"連接埠 {self.port} 已經被別的程式佔用。請先關掉另一台伺服器，或改這台 server.properties 的 server-port。"
        return launch, None

    def start(self):
        if self.alive():
            return "已經在執行中了"
        if self.backup_busy:
            return "正在備份世界，等備份完成再開機"
        if self.external:
            return f"連接埠 {self.port} 已經有伺服器在跑（不是從平台開的），請先在那邊關掉"
        self.port = read_port(self.dir, self.port)
        launch, blocked = self.preflight()
        if blocked:
            return blocked
        self.problem, self.saw_done, self.launch = None, False, launch
        self.add_log(f"» 平台：開機中…（{launch.cmd}）")
        if launch.mode == "bat":
            self.add_log(f"» 平台：這份 {SCRIPT} 寫法比較複雜，平台直接執行它")
        try:
            self.proc = popen_server(launch.cmd, self.dir, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.STDOUT)
        except (OSError, ValueError) as e:
            self.add_log(f"» 平台：開機失敗 {e}")
            return f"開機失敗：{e}"
        self.state, self.started_at, self.stop_sent_at = "starting", time.time(), None
        self.mem_hist.clear()
        logging.info("start %s pid=%s mode=%s", self.id, self.proc.pid, launch.mode)
        threading.Thread(target=self._reader, args=(self.proc,), daemon=True).start()
        return None

    def _line(self, raw):
        try:
            line = raw.decode("utf-8")
        except UnicodeDecodeError:
            line = raw.decode(TEXT_ENC, "replace")
        line = line.rstrip("\r\n")
        self.add_log(line)
        if "Done (" in line and "help" in line:
            self.saw_done = True
            if self.state == "starting":
                self.state = "running"

    def _reader(self, proc):
        buf = b""
        while True:
            chunk = proc.stdout.read1(65536)
            if not chunk:
                break
            buf += chunk
            *lines, buf = buf.split(b"\n")
            for raw in lines:
                self._line(raw)
            # bat 裡的 pause 會卡住等按鍵（提示沒有換行），替使用者按一下才能結束。
            if self.launch.mode == "bat" and re.search(rb"\. \. \.\s*$", buf):
                self._line(buf)
                buf = b""
                try:
                    proc.stdin.write(b"\n"); proc.stdin.flush()
                except OSError:
                    pass
        if buf:
            self._line(buf)
        code = proc.wait()
        self.add_log(f"» 平台：伺服器已關閉（結束代碼 {code}）")
        logging.info("exit %s code=%s", self.id, code)
        if self.stop_sent_at is None:
            with self.lock:
                recent = [t for _, t in self.log]
            self.problem = explain_failure(recent, code, self.saw_done)
            if self.problem:
                self.add_log("» 平台：" + self.problem)
        self.state, self.ping, self.started_at = "stopped", None, None
        if self.stop_sent_at is not None and self.cfg.get("auto_backup"):   # 只在正常關機後備份，閃退不備份
            self.add_log("» 平台：已關機，依設定自動備份世界…")
            err = self.backup()
            if err:
                self.add_log("» 平台：自動備份沒做：" + err)

    def clear_hostile(self):
        """清敵對生物：讀回目前難度 → 切和平 → 2 秒後切回。讀不到難度就什麼都不改。"""
        if not self.alive():
            return "伺服器沒有在執行"
        since = self.seq
        err = self.send("difficulty")
        if err:
            return err
        cur, end = None, time.time() + 3
        while cur is None and time.time() < end:
            for _, t in self.logs_since(since)[1]:
                m = re.search(r"The difficulty is (\w+)", t)
                cur = m[1].lower() if m else cur
            time.sleep(0.1)
        if cur not in ("peaceful", "easy", "normal", "hard"):
            return "讀不到目前的難度（3 秒內沒回應），什麼都沒改"
        if cur == "peaceful":
            return "現在已經是和平難度，本來就沒有敵對生物"
        self.add_log(f"» 平台：清敵對生物：暫時切到和平，2 秒後切回 {cur}")
        err = self.send("difficulty peaceful")
        if not err:
            threading.Timer(2, lambda: self.send(f"difficulty {cur}") and self.add_log(f"» 平台：難度沒切回去，請手動送 difficulty {cur}")).start()
        return err

    def send(self, text):
        if not self.alive():
            return "伺服器沒有在執行"
        if "\n" in text.strip() or "\r" in text.strip():
            return "指令不能換行"
        try:
            self.proc.stdin.write((text.strip().lstrip("/") + "\n").encode("utf-8"))
            self.proc.stdin.flush()
        except OSError as e:
            return f"指令送不出去：{e}"
        return None

    def stop(self):
        """一律走遊戲內 stop 指令，讓伺服器自己存檔後關閉。"""
        if not self.alive():
            return "伺服器沒有在執行"
        self.add_log("» 平台：送出 stop，伺服器會先存檔再關閉…")
        err = self.send("stop")
        if not err:
            self.state, self.stop_sent_at = "stopping", time.time()
        return err

    def can_force(self):
        return self.state == "stopping" and self.stop_sent_at and time.time() - self.stop_sent_at > STOP_GRACE_S

    def force_kill(self):
        # shortcut: bat 模式裡看不懂的自動重開迴圈，stop 後會再開一次，只能靠這裡強制結束；真有人用到再解析。
        if not self.can_force():
            return f"送出 stop 後要等 {STOP_GRACE_S} 秒還沒關，才能強制結束"
        self.add_log("» 平台：強制結束（最後手段）")
        logging.warning("force kill %s", self.id)
        try:
            kill_tree(self.proc.pid)
        except UserError as e:
            return str(e)
        return None

    def backup(self):
        if self.alive() or self.external:
            return "伺服器開著不能備份，請先關機"
        if self.backup_busy:
            return "已經在備份了"
        self.backup_busy, self.backup_info = True, {"ok": None, "text": "備份中…"}
        threading.Thread(target=self._backup, daemon=True).start()
        return None

    def _backup(self):
        try:
            d = Path(self.dir)
            level = Path(read_prop(d, "level-name") or "world").name
            sources = [d / n for n in (level, level + "_nether", level + "_the_end") if (d / n).is_dir()]
            if not sources:
                raise UserError(f"找不到世界資料夾「{level}」，這台可能還沒開過機。")
            need = sum(tree_stats(s)[1] for s in sources)
            if shutil.disk_usage(d).free < need * 1.05 + 104857600:
                raise UserError(f"硬碟空間不夠，備份需要約 {need / 1073741824:.1f} GB。")
            dest = d / "backups" / f"{level}_{time.strftime('%Y%m%d_%H%M%S')}"
            files = size = 0
            try:
                for s in sources:
                    shutil.copytree(s, dest / s.name)
                    a, b = tree_stats(s), tree_stats(dest / s.name)
                    if a != b:
                        raise UserError(f"對帳不一致：{s.name} 原本 {a[0]} 個檔、{a[1]} bytes，備份只有 {b[0]} 個檔、{b[1]} bytes。")
                    files, size = files + a[0], size + a[1]
            except (UserError, OSError, shutil.Error):
                if dest.exists():   # 留著但標清楚，不讓人誤以為是完整備份
                    dest.rename(dest.with_name(dest.name + "_未完成"))
                raise
            self.backup_info = {"ok": True, "path": str(dest), "files": files, "bytes": size,
                                "text": f"備份完成：{files} 個檔、{size / 1048576:.0f} MB（對帳一致）\n位置：{dest}"}
            logging.info("backup %s files=%s bytes=%s", self.id, files, size)
        except UserError as e:
            self.backup_info = {"ok": False, "text": f"備份失敗：{e}"}
        except OSError as e:
            logging.exception("backup failed")
            self.backup_info = {"ok": False, "text": f"備份失敗：有檔案讀不到或硬碟寫不進去（{e}）。如果伺服器還開著，請先關機。"}
        finally:
            self.backup_busy = False
        self.add_log("» 平台：" + self.backup_info["text"])

    def poll_status(self):
        try:
            self.ping = slp_ping(self.port)
        except (OSError, ValueError):
            self.ping = None
        mine = self.alive()
        self.external = bool(self.ping) and not mine
        if mine and self.ping and self.state == "starting":
            self.state = "running"
        if mine:
            mb = tree_mem_mb(self.proc.pid)
            if mb is not None:
                self.mem_hist.append(round(mb))

    def snapshot(self):
        p = self.ping or {}
        players = p.get("players") or {}
        alive, xmx = self.alive(), None
        if alive and self.launch.mode == "direct":
            m = re.search(r"-Xmx(\d+)([GgMm])", self.launch.cmd)
            xmx = int(m.group(1)) * (1024 if m.group(2) in "Gg" else 1) if m else None
        state = "external" if self.external else self.state
        return {
            "id": self.id, "name": self.cfg["name"], "version": self.cfg.get("version", ""),
            "accent": self.cfg.get("accent", "#7c9cff"), "address": self.cfg.get("address", ""),
            "icon": self.cfg.get("icon") if self.cfg.get("icon") in ICON_IDS else None,
            "color": self.cfg.get("color") if self.cfg.get("color") in ICON_COLORS else None,
            "port": self.port, "state": state, "dir": self.dir, "problem": self.problem,
            "backup": self.backup_info, "backup_busy": self.backup_busy,
            "modded": self.cfg.get("loader") == "Fabric" or "Fabric" in self.cfg.get("version", "") or (Path(self.dir) / "mods").is_dir(),
            "online": players.get("online") if self.ping else None, "max": players.get("max"),
            "names": [x.get("name", "") for x in players.get("sample") or []],
            "mem_mb": tree_mem_mb(self.proc.pid) if alive else None, "xmx_mb": xmx,
            "mem_hist": list(self.mem_hist) if alive else [],
            "uptime_s": int(time.time() - self.started_at) if self.started_at and alive else None,
            "can_force": bool(self.can_force()),
            "stopping_s": int(time.time() - self.stop_sent_at) if self.state == "stopping" and self.stop_sent_at else None,
        }


# ---------- 官方來源下載 ----------
def _ssl_context():
    ctx = ssl.create_default_context()
    try:   # 剛重灌的 Windows 憑證不一定齊全，打包時多帶一份
        import certifi
        ctx.load_verify_locations(certifi.where())
    except (ImportError, OSError):
        pass
    return ctx


def _open(url):
    u = urlparse(url)
    if u.scheme != "https" or u.hostname not in ALLOWED_HOSTS:
        raise UserError("下載網址不是官方來源，已停止。")
    # shortcut: urlopen 會跟著轉址走、不再檢查目的網域；等真實安裝看過 Forge／NeoForge 實際會不會轉址再決定要不要加檢查
    return urlopen(Request(url, headers={"User-Agent": f"MCServerHub/{VERSION}"}), timeout=30, context=_ssl_context())


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *a, **k):
        return None   # 不跟隨轉址：urllib 會把 3xx 當錯誤丟出


def _fetch_version():
    """只連寫死的 HTTPS 網址，最多讀 32 bytes。"""
    opener = build_opener(_NoRedirect, HTTPSHandler(context=_ssl_context()))
    with opener.open(Request(VERSION_URL, headers={"User-Agent": f"MCServerHub/{VERSION}"}), timeout=8) as r:
        return r.read(32)


def newer_version(fetch=_fetch_version):
    """有比現在新的版本就回傳版本號字串；沒有、內容不對、連不上一律回 None，不影響開服。"""
    try:
        m = re.fullmatch(rb"\s*(\d{1,4})\.(\d{1,4})\.(\d{1,4})\s*", fetch())
    except Exception:
        return None
    if not m:
        return None
    latest = tuple(int(x) for x in m.groups())
    return ".".join(map(str, latest)) if latest > tuple(int(x) for x in VERSION.split(".")) else None


def fetch_json(url, sha1=None):
    with _open(url) as r:
        data = r.read()
    if sha1 and hashlib.sha1(data).hexdigest() != sha1:
        raise UserError("官方版本資料核對不符，已停止。請稍後再試。")
    return json.loads(data)


def download(url, dest, sha1=None, size=None, progress=None, sha256=None):
    """檢查碼只能防「下載壞掉」，防不了來源本身被換。"""
    dest, part = Path(dest), Path(str(dest) + ".part")
    digest, got = (hashlib.sha256() if sha256 else hashlib.sha1()), 0
    with _open(url) as r, open(part, "wb") as f:
        total = size or int(r.headers.get("Content-Length") or 0)
        while True:
            chunk = r.read(262144)
            if not chunk:
                break
            f.write(chunk); digest.update(chunk); got += len(chunk)
            if progress and total:
                progress(got / total)
    if total and got != total:
        part.unlink(missing_ok=True)
        raise UserError("下載不完整（網路可能中斷了），請再試一次。")
    want = sha256 or sha1
    if want and digest.hexdigest() != want.lower():
        part.unlink(missing_ok=True)
        raise UserError(f"下載的檔案核對不符（{'SHA-256' if sha256 else 'SHA-1'} 不一致），已刪除。請再試一次。")
    os.replace(part, dest)


def fetch_checksum(url, algo):
    """讀官方的 .sha1／.sha256 小檔（最多 200 bytes），只收對的長度的十六進位。"""
    with _open(url) as r:
        data = r.read(201)
    words = data.decode("ascii", "replace").split()
    if len(data) > 200 or not words or not re.fullmatch(r"[0-9a-fA-F]{%d}" % (40 if algo == "sha1" else 64), words[0]):
        raise UserError("官方的檢查碼檔案格式不對，已停止。請稍後再試。")
    return words[0].lower()


# ---------- Forge／NeoForge：挑版本、組網址（純函式，測試用寫死的資料） ----------
def mc_tuple(v):
    return tuple(int(x) for x in v.split(".")) if re.fullmatch(r"\d+(\.\d+)+", v or "") else None


def forge_pick(promos, mc):
    """回傳 (Forge 版本, None) 或 (None, 不能選的原因)。"""
    t = mc_tuple(mc)
    if not t or t < (1, 20, 1):
        return None, "這版 Forge 啟動方式不同，1.1.0 不支援"
    p = (promos or {}).get("promos") or {}
    v = p.get(f"{mc}-recommended") or p.get(f"{mc}-latest")
    if not v or not re.fullmatch(r"\d+(\.\d+)+", str(v)):   # 版本號會進網址和檔名，只收數字與點
        return None, "Forge 還沒有這個遊戲版本"
    return v, None


def neoforge_mc(v):
    """NeoForge 版本號 → 遊戲版本：21.1.x→1.21.1、21.0.x→1.21、26.3.0.x→26.3（第三段 0 省略）。"""
    p = v.split(".")
    if len(p) >= 4 and int(p[0]) >= 26:
        return f"{p[0]}.{p[1]}" + (f".{p[2]}" if p[2] != "0" else "")
    if len(p) == 3:
        return f"1.{p[0]}" + (f".{p[1]}" if p[1] != "0" else "")
    return None


def neoforge_pick(data, mc):
    """只收「純數字與點」的正式版，取最大；只有測試版就說明。"""
    all_v = [str(v) for v in (data or {}).get("versions") or []]
    stable = [v for v in all_v if re.fullmatch(r"\d+(\.\d+)+", v) and neoforge_mc(v) == mc]
    if stable:
        return max(stable, key=mc_tuple), None
    if any((m := re.match(r"\d+(?:\.\d+)+", v)) and neoforge_mc(m[0]) == mc for v in all_v):
        return None, "NeoForge 這個遊戲版本只有測試版"
    return None, "NeoForge 還沒有這個遊戲版本"


def forge_installer_url(mc, v):
    return f"{FORGE_MAVEN}/{mc}-{v}/forge-{mc}-{v}-installer.jar"


def neoforge_installer_url(v):
    return f"{NEOFORGE_MAVEN}/{v}/neoforge-{v}-installer.jar"


def loader_choice(kind, mc, data):
    """給建服精靈：某遊戲版本能不能選這個載入器、會裝哪版、不能的原因。"""
    label = "Forge" if kind == "forge" else "NeoForge"
    if data is None:
        return {"ok": False, "version": None, "why": f"連不上 {label} 官方網站，稍後再試"}
    v, why = (forge_pick if kind == "forge" else neoforge_pick)(data, mc)
    return {"ok": bool(v), "version": v, "why": why}


def modloader_args(text):
    """從安裝器產生的 run.bat／run.sh 抽出 `@user_jvm_args.txt @libraries/…args.txt`；找不到回 None。"""
    for raw in text.splitlines():
        line = raw.strip()
        if re.match(r"(?i)(rem\b|::|#)", line) or "@user_jvm_args.txt" not in line:
            continue
        args = re.findall(r"(?<!\S)@(?:user_jvm_args\.txt|libraries/[\w./+-]+_args\.txt)(?!\S)", line)
        if len(args) >= 2:
            return " ".join(args)
    return None


def _keep_installer_log(target, installer, captured, log_dir):
    dest = Path(log_dir) / "installer-logs" / f"{time.strftime('%Y%m%d_%H%M%S')}_{installer.name}.log"
    dest.parent.mkdir(parents=True, exist_ok=True)
    src = Path(target) / (installer.name + ".log")
    if not src.is_file():   # 2026-11-13 實測：Forge 52.1.20、NeoForge 21.1.266 都寫成 installer.log
        src = Path(target) / "installer.log"
    if src.is_file():
        shutil.copyfile(src, dest)
    else:
        dest.write_text("\n".join(captured), encoding="utf-8")
    return dest


def install_modloader(target, java, installer, ram, on_line, log_dir, timeout=INSTALL_TIMEOUT_S):
    """在伺服器資料夾裡跑 Forge／NeoForge 安裝器，成功後依它產生的 run 檔寫平台自己的啟動檔，回傳檔名。
    失敗時把安裝紀錄複製到 log_dir\\installer-logs\\ 並丟出白話錯誤（清資料夾由呼叫端負責）。"""
    target, installer = Path(target), Path(installer)
    captured, killed = [], []
    try:
        proc = subprocess.Popen([java, "-Dstdout.encoding=UTF-8", "-Dstderr.encoding=UTF-8", "-jar", installer.name, "--installServer"], cwd=target, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, creationflags=NO_WINDOW)
    except OSError as e:
        raise UserError(f"安裝器跑不起來：{e}")
    timer = threading.Timer(timeout, lambda: (killed.append(1), proc.kill()))
    timer.start()
    try:
        for raw in proc.stdout:
            try:
                line = raw.decode("utf-8")
            except UnicodeDecodeError:
                line = raw.decode(TEXT_ENC, "replace")
            line = line.rstrip("\r\n")
            captured.append(line)
            del captured[:-5000]
            on_line(line)
        code = proc.wait()
    finally:
        timer.cancel()
    run = target / ("run.bat" if OS == "win" else "run.sh")
    args = None if code or killed or not run.is_file() else modloader_args(read_text_any(run))
    if not args:
        where = _keep_installer_log(target, installer, captured, log_dir)
        why = (f"安裝逾時（超過 {timeout // 60} 分鐘）" if killed else f"安裝失敗（結束代碼 {code}）" if code else
               "安裝完成了，但看不懂它產生的啟動方式，1.1.0 不支援這個版本")
        raise UserError(f"{why}。安裝紀錄已保留在：{where}")
    installer.unlink(missing_ok=True)   # 安裝器 jar 留著會被誤認成伺服器本體
    return write_start_script(target, java, f"-Xmx{ram}G {args} nogui")


def check_folder_name(name):
    """回傳錯誤訊息；沒問題回 None。"""
    if not name or len(name) > 40:
        return "伺服器名稱要 1 到 40 個字"
    if re.search(r'[<>:"/\\|?*\x00-\x1f]', name) or name[-1] in ". " or name.split(".")[0].upper() in (
            {"CON", "PRN", "AUX", "NUL"} | {f"{p}{i}" for p in ("COM", "LPT") for i in range(1, 10)}):
        return "伺服器名稱不能有 < > : \" / \\ | ? * 這些符號，也不能以句點或空白結尾"
    return None


# ---------- 常用指令選單（清單只改這裡；範本用 <欄位> 占位，後端組字串並驗證） ----------
# danger：""＝直接送、"confirm"＝要確認、"strong"＝強確認（畫面要再打一次對象或按住 1 秒）。
# rule：遊戲規則的舊名；1.21.11 起改名（對過 Minecraft Wiki「Game rule」頁），新名 None＝新版已移除、隱藏。
GAMERULE_NEW = {"keepInventory": "keep_inventory", "doMobSpawning": "spawn_mobs", "mobGriefing": "mob_griefing",
                "playersSleepingPercentage": "players_sleeping_percentage", "doInsomnia": "spawn_phantoms",
                "doImmediateRespawn": "immediate_respawn", "doDaylightCycle": "advance_time",
                "doWeatherCycle": "advance_weather", "doFireTick": None}
RULE_RENAME_FROM = (1, 21, 11)
ITEMS = [["diamond", "鑽石"], ["iron_ingot", "鐵錠"], ["gold_ingot", "金錠"], ["bread", "麵包"], ["cooked_beef", "牛排"],
         ["torch", "火把"], ["oak_log", "橡木原木"], ["elytra", "鞘翅"], ["totem_of_undying", "不死圖騰"]]
MOBS = [["horse", "馬"], ["wolf", "狼"], ["cat", "貓"], ["villager", "村民"], ["iron_golem", "鐵魔像"],
        ["chicken", "雞"], ["cow", "牛"], ["sheep", "羊"]]
EFFECTS = [["night_vision", "夜視"], ["speed", "加速"], ["haste", "挖掘加速"], ["fire_resistance", "抗火"],
           ["water_breathing", "水下呼吸"], ["regeneration", "回復"]]
ENCHANTS = [[k, n, lv] for k, n, lv in (
    ("protection", "保護", 4), ("fire_protection", "火焰保護", 4), ("blast_protection", "爆炸保護", 4),
    ("projectile_protection", "投射物保護", 4), ("feather_falling", "輕盈", 4), ("respiration", "水中呼吸", 3),
    ("aqua_affinity", "親水性", 1), ("thorns", "尖刺", 3), ("depth_strider", "深海漫遊", 3), ("frost_walker", "冰霜行者", 2),
    ("soul_speed", "靈魂疾走", 3), ("swift_sneak", "迅捷潛行", 3), ("sharpness", "鋒利", 5), ("smite", "不死剋星", 5),
    ("bane_of_arthropods", "節肢剋星", 5), ("knockback", "擊退", 2), ("fire_aspect", "燃燒", 2), ("looting", "掠奪", 3),
    ("sweeping_edge", "橫掃之刃", 3), ("efficiency", "效率", 5), ("silk_touch", "絲綢之觸", 1), ("unbreaking", "耐久", 3),
    ("fortune", "幸運", 3), ("power", "強力", 5), ("punch", "衝擊", 2), ("flame", "火焰", 1), ("infinity", "無限", 1),
    ("luck_of_the_sea", "海洋的祝福", 3), ("lure", "魚餌", 3), ("loyalty", "忠誠", 3), ("impaling", "魚叉", 5),
    ("riptide", "波濤", 3), ("channeling", "喚雷", 1), ("multishot", "分裂箭", 1), ("quick_charge", "快速上弦", 3),
    ("piercing", "貫穿", 4), ("density", "密度", 5), ("breach", "破甲", 4), ("wind_burst", "風爆", 3), ("mending", "修補", 1))]
FIELDS = {
    "player": {"label": "玩家", "kind": "player"}, "target": {"label": "目標玩家", "kind": "player"},
    "item": {"label": "物品", "kind": "id", "picks": ITEMS}, "mob": {"label": "生物", "kind": "id", "picks": MOBS},
    "effect": {"label": "效果", "kind": "id", "picks": EFFECTS}, "ench": {"label": "附魔", "kind": "ench", "picks": ENCHANTS},
    "count": {"label": "數量", "kind": "int", "min": 1, "max": 640, "default": 1},
    "secs": {"label": "秒數", "kind": "int", "min": 1, "max": 3600, "default": 60},
    "amp": {"label": "等級", "kind": "int", "min": 1, "max": 10, "default": 1},      # 畫面 1＝I 級，送出減 1
    "level": {"label": "附魔等級", "kind": "int", "min": 1, "max": 5, "default": 1},
    "x": {"label": "X", "kind": "num"}, "y": {"label": "Y", "kind": "num"}, "z": {"label": "Z", "kind": "num"},
    "msg": {"label": "訊息", "kind": "text"}, "reason": {"label": "原因（可空白）", "kind": "text", "optional": True},
    "bool": {"label": "開／關", "kind": "bool"}, "pct": {"label": "百分比", "kind": "int", "min": 0, "max": 100, "default": 100},
}


def _cmd(cid, cat, name, tpl, danger="", **extra):
    return {"id": cid, "cat": cat, "name": name, "tpl": tpl, "danger": danger, **extra}


COMMANDS = [
    _cmd("day", "時間天氣", "白天", "time set day"), _cmd("noon", "時間天氣", "正午", "time set noon"),
    _cmd("night", "時間天氣", "晚上", "time set night"), _cmd("midnight", "時間天氣", "午夜", "time set midnight"),
    _cmd("clear", "時間天氣", "放晴", "weather clear"), _cmd("rain", "時間天氣", "下雨", "weather rain"),
    _cmd("thunder", "時間天氣", "打雷", "weather thunder"),
    _cmd("time_stop", "時間天氣", "停在現在時間", "gamerule <rule> false", rule="doDaylightCycle"),
    _cmd("time_go", "時間天氣", "恢復時間流動", "gamerule <rule> true", rule="doDaylightCycle"),
    _cmd("weather_stop", "時間天氣", "停止天氣變化", "gamerule <rule> false", rule="doWeatherCycle"),
    _cmd("weather_go", "時間天氣", "恢復天氣變化", "gamerule <rule> true", rule="doWeatherCycle"),
    _cmd("gm_survival", "遊戲模式", "生存", "gamemode survival <player>"),
    _cmd("gm_creative", "遊戲模式", "創造", "gamemode creative <player>"),
    _cmd("gm_adventure", "遊戲模式", "冒險", "gamemode adventure <player>"),
    _cmd("gm_spectator", "遊戲模式", "旁觀", "gamemode spectator <player>"),
    _cmd("diff_peaceful", "遊戲模式", "難度：和平", "difficulty peaceful", "confirm"),
    _cmd("diff_easy", "遊戲模式", "難度：簡單", "difficulty easy"),
    _cmd("diff_normal", "遊戲模式", "難度：普通", "difficulty normal"),
    _cmd("diff_hard", "遊戲模式", "難度：困難", "difficulty hard"),
    _cmd("tp_player", "傳送", "傳到玩家旁邊", "tp <player> <target>"),
    _cmd("tp_xyz", "傳送", "傳到座標", "execute at <player> run tp <player> <x> <y> <z>"),
    _cmd("spawnpoint", "傳送", "把玩家重生點設在他現在的位置", "execute at <player> run spawnpoint <player> ~ ~ ~"),
    _cmd("worldspawn", "傳送", "把世界重生點設在玩家位置", "execute at <player> run setworldspawn ~ ~ ~", note="玩家要在主世界"),
    _cmd("give", "給物品", "給物品", "give <player> minecraft:<item> <count>"),
    _cmd("enchant", "給物品", "附魔主手物品", "enchant <player> minecraft:<ench> <level>", note="手上要拿著能附魔的東西"),
    _cmd("xp", "給物品", "給經驗等級", "xp add <player> <count> levels"),
    _cmd("summon", "召喚生物", "在玩家旁邊召喚", "execute at <player> run summon minecraft:<mob> ~ ~ ~"),
    _cmd("effect", "效果", "給效果", "effect give <player> minecraft:<effect> <secs> <amp> true"),
    _cmd("effect_clear", "效果", "清除效果", "effect clear <player>"),
    _cmd("keep_inv", "規則", "死亡不掉落", "gamerule <rule> <bool>", rule="keepInventory"),
    _cmd("mob_spawn", "規則", "生物自然生成", "gamerule <rule> <bool>", rule="doMobSpawning", confirm_if_false=True),
    _cmd("mob_grief", "規則", "生物破壞方塊（苦力怕炸地形等）", "gamerule <rule> <bool>", rule="mobGriefing", confirm_if_false=True),
    _cmd("fire_tick", "規則", "火焰蔓延", "gamerule <rule> <bool>", rule="doFireTick"),
    _cmd("sleep_pct", "規則", "幾成玩家睡覺就天亮", "gamerule <rule> <pct>", rule="playersSleepingPercentage"),
    _cmd("insomnia", "規則", "幻翼出現", "gamerule <rule> <bool>", rule="doInsomnia"),
    _cmd("instant_respawn", "規則", "死亡立即重生", "gamerule <rule> <bool>", rule="doImmediateRespawn"),
    _cmd("list", "伺服器", "誰在線上", "list"), _cmd("save", "伺服器", "立刻存檔", "save-all"),
    _cmd("say", "伺服器", "廣播訊息", "say <msg>"), _cmd("seed", "伺服器", "查種子碼", "seed"),
    _cmd("wl_add", "伺服器", "加入白名單", "whitelist add <player>"),
    _cmd("wl_remove", "伺服器", "移出白名單", "whitelist remove <player>"),
    _cmd("wl_on", "伺服器", "開啟白名單", "whitelist on", "confirm", note="不在白名單的人會進不來"),
    _cmd("wl_off", "伺服器", "關閉白名單", "whitelist off"), _cmd("wl_list", "伺服器", "看白名單", "whitelist list"),
    _cmd("kick", "伺服器", "踢出玩家", "kick <player> <reason>", "confirm"),
    _cmd("op", "伺服器", "給管理員", "op <player>", "strong", note="管理員能下所有指令"),
    _cmd("deop", "伺服器", "拿掉管理員", "deop <player>"),
    _cmd("ban", "伺服器", "封鎖玩家", "ban <player>", "strong"),
    _cmd("pardon", "伺服器", "解除封鎖", "pardon <player>"), _cmd("banlist", "伺服器", "看封鎖名單", "banlist"),
    _cmd("kill_items", "清除", "清掉地上掉落物", "kill @e[type=minecraft:item]", "strong", note="地上所有掉落物會消失，不能復原"),
    _cmd("kill_mob", "清除", "清掉某種生物", "kill @e[type=minecraft:<mob>]", "strong", note="這種生物全部消失（含有取名的），不能復原"),
    _cmd("kill_hostile", "清除", "清掉所有敵對生物", "", "strong", special="clear_hostile",
         note="做法是暫時切到和平再切回來；清不掉豬布林類、疣豬獸、終界龍"),
    _cmd("clear_inv", "清除", "清空玩家背包", "clear <player>", "strong", note="背包裡的東西全部消失，不能復原"),
]
_DANGER_RANK = {"": 0, "confirm": 1, "strong": 2}


def mc_of(label):
    """從清單上的版本字（如「1.21.1 · NeoForge」）取出遊戲版本 tuple；看不出來回 None。"""
    m = re.search(r"\d+\.\d+(?:\.\d+)?", label or "")
    return mc_tuple(m[0]) if m else None


def rule_name(old, version):
    """回傳 (規則名, 是否不確定)；新版已移除回 (None, False)。"""
    if version is None:
        return old, True
    return (GAMERULE_NEW[old], False) if version >= RULE_RENAME_FROM else (old, False)


def tpl_fields(tpl):
    return list(dict.fromkeys(re.findall(r"<(\w+)>", tpl)))


def commands_for(label, custom=()):
    """給畫面的清單：依伺服器版本換規則名、隱藏新版沒有的；版本看不出來時標 uncertain。"""
    v, out = mc_of(label), []
    for c in COMMANDS + [dict(c, cat=c.get("cat") or "我的筆記") for c in custom]:
        c = dict(c, fields=[f for f in tpl_fields(c["tpl"]) if f != "rule"])
        if c.get("rule"):
            name, unsure = rule_name(c["rule"], v)
            if name is None:
                continue
            c["preview"], c["uncertain"] = c["tpl"].replace("<rule>", name), unsure
        out.append(c)
    return out


def _check_value(key, raw, ench=None):
    spec, text = FIELDS[key], str("" if raw is None else raw)
    if "\r" in text or "\n" in text:   # 先檢查再去空白：結尾的換行也要擋
        raise UserError(f"{spec['label']}不能換行")
    text = text.strip()
    kind = spec["kind"]
    if kind == "player":
        if text == "@a" or re.fullmatch(r"\.?[A-Za-z0-9_]{1,16}", text):
            return text
        raise UserError(f"{spec['label']}只能是 1–16 個英數字或底線（基岩版玩家前面可有一個句點），或 @a 代表全部")
    if kind in ("id", "ench"):
        if not re.fullmatch(r"[a-z0-9_]{1,64}", text) or (key == "mob" and text == "player"):
            raise UserError(f"{spec['label']}名稱只能是小寫英文、數字、底線（例如 diamond）")
        return text
    if kind == "int":
        try:
            n = int(text)
        except ValueError:
            raise UserError(f"{spec['label']}要填整數")
        top = next((e[2] for e in ENCHANTS if e[0] == ench), spec["max"]) if key == "level" else spec["max"]
        if not spec["min"] <= n <= top:
            raise UserError(f"{spec['label']}要在 {spec['min']} 到 {top} 之間")
        return str(n - 1 if key == "amp" else n)
    if kind == "num":
        if not re.fullmatch(r"-?\d{1,8}(\.\d{1,3})?", text):
            raise UserError(f"座標 {spec['label']} 要填數字")
        return text
    if kind == "bool":
        if text.lower() not in ("true", "false"):
            raise UserError(f"{spec['label']}只能選開或關")
        return text.lower()
    if not text and not spec.get("optional"):   # text
        raise UserError(f"{spec['label']}不能空白")
    if len(text) > 200:
        raise UserError(f"{spec['label']}最多 200 字")
    return text


def build_command(cmd, values, confirm, label):
    """回傳要送進主控台的字串；欄位不合法、危險指令沒確認、新版沒有的規則都丟 UserError。"""
    values = values if isinstance(values, dict) else {}
    out, danger = cmd["tpl"], _DANGER_RANK[cmd.get("danger", "")]
    if cmd.get("rule"):
        name, _ = rule_name(cmd["rule"], mc_of(label))
        if name is None:
            raise UserError(f"「{cmd['name']}」這條規則在這個遊戲版本已經沒有了")
        out = out.replace("<rule>", name)
    for key in tpl_fields(out):
        if key not in FIELDS:
            raise UserError(f"指令範本裡有不認識的欄位 <{key}>")
        value = _check_value(key, values.get(key), values.get("ench"))
        if value == "@a" or (cmd.get("confirm_if_false") and value == "false"):
            danger = max(danger, 1)
        out = out.replace(f"<{key}>", value)
    out = out.strip()
    if danger == 2 and confirm != "strong":
        raise UserError("這是會刪東西或給權限的指令，要先在畫面上強確認")
    if danger == 1 and confirm not in (True, "strong"):
        raise UserError("這個指令要先按確認")
    if not out or "\n" in out or "\r" in out:
        raise UserError("指令是空的或含有換行")
    return out


def clean_custom_commands(items):
    """我的筆記：驗證使用者自訂指令；沒有編號的給一個。"""
    if not isinstance(items, list) or len(items) > 100:
        raise UserError("我的筆記最多 100 筆")
    out = []
    for c in items:
        if not isinstance(c, dict):
            raise UserError("我的筆記格式不對")
        name, cat = str(c.get("name", "")).strip(), str(c.get("cat") or "我的筆記").strip()
        tpl = str(c.get("tpl", "")).strip().lstrip("/")
        cid = str(c.get("id") or "my_" + secrets.token_hex(3))
        if not 1 <= len(name) <= 40 or len(cat) > 20 or not 1 <= len(tpl) <= 300 or "\n" in tpl or "\r" in tpl:
            raise UserError(f"「{name or '（沒名稱）'}」：名稱 1–40 字、指令 1–300 字且不能換行")
        if not re.fullmatch(r"my_[0-9a-z_]{1,20}", cid):
            raise UserError("我的筆記編號格式不對")
        bad = [f for f in tpl_fields(tpl) if f not in FIELDS or f == "rule"]
        if bad:
            raise UserError(f"「{name}」用了不認識的欄位 <{bad[0]}>，可用：" + "、".join(f"<{k}>" for k in FIELDS))
        danger = c.get("danger") or ""
        if danger not in _DANGER_RANK:
            raise UserError("危險等級只能是無、確認、強確認")
        out.append({"id": cid, "cat": cat, "name": name, "tpl": tpl, "danger": danger, "note": str(c.get("note", ""))[:200]})
    return out


DEFAULT_LINKS = [{"name": "Minecraft Wiki", "url": "https://minecraft.wiki/"},
                 {"name": "Chunkbase（找村莊、要塞）", "url": "https://www.chunkbase.com/apps/"}]
DEFAULT_JOIN = ("【加入伺服器】\n1. 用 Minecraft Java 版（或 PrismLauncher）開 <版本>。\n"
                "2. 有裝模組的伺服器（<載入器>）：先裝同一個載入器，把伺服器給你的模組放進 mods 資料夾。"
                "Forge 和 NeoForge 的模組不能互通。\n3. 多人遊戲 → 新增伺服器 → 位址填：<連線網址>\n")


def clean_links(items):
    if not isinstance(items, list) or len(items) > 20:
        raise UserError("實用連結最多 20 個")
    out = []
    for l in items:
        name, url = str((l or {}).get("name", "")).strip(), str((l or {}).get("url", "")).strip()
        if not 1 <= len(name) <= 40 or not re.fullmatch(r"https://[^\s<>\"']{1,500}", url):
            raise UserError(f"連結「{name or url}」：名稱 1–40 字，網址要以 https:// 開頭")
        out.append({"name": name, "url": url})
    return out


def join_text(template, server_cfg):
    label = server_cfg.get("version", "")
    # 1.0.0 加入的伺服器設定裡沒有 loader，從版本字樣（如「1.21.1 · Fabric · 100 模組」）推回來
    guess = next((k for k in ("NeoForge", "Forge", "Fabric") if re.search(rf"\b{k}\b", label)), "")
    loader = server_cfg.get("loader") or guess or ("原版" if label else "")
    return (template.replace("<連線網址>", server_cfg.get("address") or "（屋主還沒設定連線網址）")
            .replace("<版本>", mc_of(label) and ".".join(map(str, mc_of(label))) or "伺服器指定的版本")
            .replace("<載入器>", loader or "原版"))


def set_ram(script, gb):
    """改啟動檔裡唯一一個 -Xmx（逐位元組改，不動原本編碼）；-Xms 比新值大就一起降。看不懂就丟錯。"""
    data = Path(script).read_bytes()
    if len(re.findall(rb"-Xmx\d+[GgMm]\b", data)) != 1:
        raise UserError("這份啟動檔不是平台產生的格式（找不到剛好一個 -Xmx），請用" + EDITOR + "手動改。")
    data = re.sub(rb"-Xmx\d+[GgMm]\b", b"-Xmx%dG" % gb, data)
    m = re.search(rb"-Xms(\d+)([GgMm])\b", data)
    if m and int(m[1]) * (1024 if m[2] in b"Gg" else 1) > gb * 1024:
        data = data[:m.start()] + b"-Xms%dG" % gb + data[m.end():]
    shutil.copyfile(script, str(script) + ".bak")
    tmp = Path(str(script) + ".tmp")
    tmp.write_bytes(data)
    shutil.copymode(script, tmp)   # Mac 的 start.sh 要保留「可執行」權限
    os.replace(tmp, script)


def read_ram_gb(script):
    try:
        m = re.search(rb"-Xmx(\d+)([GgMm])\b", Path(script).read_bytes())
    except OSError:
        return None
    return (int(m[1]) if m[2] in b"Gg" else round(int(m[1]) / 1024, 1)) if m else None


# ---------- 平台本體 ----------
class Hub:
    def __init__(self, config_path):
        self.config_path = config_path
        self.cfg_lock = threading.Lock()
        self.config = self._load()
        self.servers = {c["id"]: Server(c) for c in self.config["servers"]}
        self.token = secrets.token_urlsafe(24)
        self.last_seen = self.booted = time.time()
        self.httpd = None
        self.hosts = set()
        self.job = None
        self.update = None
        self._manifest = (0, None)
        self._loaders = (0, None)

    def loader_lists(self):
        """Forge／NeoForge 版本清單，10 分鐘內共用；連不上的那個記成 None，不讓整份版本清單失敗。"""
        if time.time() - self._loaders[0] > 600:
            out = {}
            for kind, url in (("forge", FORGE_PROMOS), ("neoforge", NEOFORGE_VERSIONS)):
                try:
                    out[kind] = fetch_json(url)
                except (UserError, URLError, OSError, ValueError):
                    logging.exception("loader list %s", kind)
                    out[kind] = None
            self._loaders = (time.time() if all(out.values()) else 0, out)   # 失敗的下次再試
        return self._loaders[1]

    def _load(self):
        try:
            return json.loads(self.config_path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            return {"servers": []}
        except (OSError, ValueError):   # 壞掉的設定留著，不直接蓋掉
            logging.exception("bad config")
            os.replace(self.config_path, self.config_path.with_name(f"hub_config.broken-{int(time.time())}.json"))
            return {"servers": []}

    def save_config(self):
        with self.cfg_lock:
            tmp = self.config_path.with_suffix(".tmp")
            tmp.write_text(json.dumps(self.config, ensure_ascii=False, indent=2), encoding="utf-8")
            os.replace(tmp, self.config_path)

    def check_update(self):
        if self.config.get("update_check") is False:   # 使用者關掉新版提醒：完全不連線
            self.update = None
            logging.info("update check: off")
            return
        self.update = newer_version()
        logging.info("update check: %s", self.update or "none")

    def all_servers(self):
        return list(self.servers.values())

    def any_alive(self):
        return any(s.alive() for s in self.all_servers())

    def busy(self):
        return self.any_alive() or any(s.backup_busy for s in self.all_servers()) or bool(self.job and self.job["state"] == "running")

    def monitor(self):
        while True:
            for s in self.all_servers():
                s.poll_status()
            idle = time.time() - max(self.last_seen, self.booted)
            if not self.busy() and idle > IDLE_EXIT_S:
                logging.info("idle exit")
                threading.Thread(target=self.httpd.shutdown, daemon=True).start()
                return
            time.sleep(3)

    def register(self, path, name, version, loader="", java_need=None, bat="start.bat"):
        cfg = {"id": "s" + secrets.token_hex(4), "name": name, "version": version, "dir": str(path), "bat": bat, "loader": loader,
               "java_need": java_need, "accent": ACCENTS[len(self.config["servers"]) % len(ACCENTS)], "address": ""}
        self.config["servers"].append(cfg)
        self.save_config()
        self.servers[cfg["id"]] = s = Server(cfg)
        return s

    def add_existing(self, folder, name):
        folder = str(folder or "").strip().strip('"')
        d = Path(folder)
        if not folder or not d.is_absolute() or not d.is_dir():
            return "找不到這個資料夾，請重新選擇"
        key = os.path.normcase(os.path.abspath(d))
        if any(os.path.normcase(os.path.abspath(s.dir)) == key for s in self.all_servers()):
            return "這個資料夾已經在清單裡了"
        bat = find_bat(d)
        if not bat:
            return f"這個資料夾裡找不到 {SCRIPT}（或其他含 java 指令的 {SCRIPT.split('.')[1]} 檔）。沒有伺服器的話，可以改用「建立新伺服器」。"
        info = inspect_server(d)
        label = " · ".join(filter(None, (info["version"], info["loader"]))) or "版本未知"
        name = str(name or "").strip()[:40] or d.name[:40]
        s = self.register(d, name, label, info["loader"], None, bat)
        return {"ok": True, "id": s.id}

    def remove(self, s):
        if s.alive() or s.backup_busy:
            return "這台還在執行或備份中，請先關機"
        self.config["servers"] = [c for c in self.config["servers"] if c["id"] != s.id]
        self.save_config()
        del self.servers[s.id]
        return None

    def settings(self):
        c = self.config
        return {"custom_commands": c.get("custom_commands", []), "links": c.get("links", DEFAULT_LINKS),
                "join_template": c.get("join_template", DEFAULT_JOIN), "update_check": c.get("update_check") is not False}

    def save_settings(self, body):
        """只改有帶的那幾項；設定檔裡其他（包含新版才有的）欄位原樣保留。"""
        changes = {}
        if "custom_commands" in body:
            changes["custom_commands"] = clean_custom_commands(body["custom_commands"])
        if "links" in body:
            changes["links"] = clean_links(body["links"])
        if "join_template" in body:
            t = str(body["join_template"] or "")
            if len(t) > 2000:
                raise UserError("加入說明最多 2000 字")
            changes["join_template"] = t.replace("\r\n", "\n")
        if "update_check" in body:
            changes["update_check"] = body["update_check"] is True
            if not changes["update_check"]:
                self.update = None
        self.config.update(changes)
        self.save_config()
        return self.settings()

    def run_command(self, s, body):
        """指令選單：由編號＋欄位值在後端組字串，驗證、確認都過了才送。"""
        cid = str(body.get("cmd", ""))
        cmd = next((c for c in COMMANDS + self.config.get("custom_commands", []) if c["id"] == cid), None)
        if not cmd:
            return "找不到這個指令"
        try:
            if cmd.get("special") == "clear_hostile":
                if body.get("confirm") != "strong":
                    raise UserError("這是會刪東西或給權限的指令，要先在畫面上強確認")
                return s.clear_hostile()
            text = build_command(cmd, body.get("values"), body.get("confirm"), s.cfg.get("version", ""))
        except UserError as e:
            return str(e)
        err = s.send(text)
        if err:
            return err
        s.add_log(f"> {text}")
        return {"ok": True, "sent": text}

    def server_settings(self, s, body):
        """每台的小設定：關機後自動備份、記憶體（改啟動檔的 -Xmx，要先關機）、圖示與顏色（開著也能改）。"""
        if "icon" in body or "color" in body:
            icon, color = body.get("icon", s.cfg.get("icon")), body.get("color", s.cfg.get("color"))
            if icon not in ICON_IDS:
                return "沒有這個圖示，請重新選一個"
            if color not in ICON_COLORS:
                return "沒有這個顏色，請重新選一個"
            s.cfg["icon"], s.cfg["color"] = icon, color
            self.save_config()
        if "ram_gb" in body:
            if s.alive() or s.external or s.backup_busy:
                return "要先關機才能改記憶體"
            try:
                gb = int(body["ram_gb"])
            except (TypeError, ValueError):
                return "記憶體要選一個數字"
            top = min(64, total_ram_gb())
            if not 1 <= gb <= top:
                return f"記憶體要在 1 到 {top} GB 之間"
            try:
                set_ram(Path(s.dir) / s.cfg.get("bat", "start.bat"), gb)
            except UserError as e:
                return str(e)
            except OSError as e:
                return f"啟動檔寫不進去：{e}"
        if "auto_backup" in body:
            s.cfg["auto_backup"] = body["auto_backup"] is True
            self.save_config()
        return None

    def server_info(self, s):
        seed = read_prop(s.dir, "level-seed")
        return {"seed": seed or "", "seed_note": None if seed else "server.properties 沒寫種子碼（隨機產生）；伺服器開著時可用「查種子碼」指令看",
                "ram_gb": read_ram_gb(Path(s.dir) / s.cfg.get("bat", "start.bat")), "ram_total_gb": total_ram_gb(),
                "auto_backup": bool(s.cfg.get("auto_backup")), "pid": s.proc.pid if s.alive() else None,
                "java": s.launch.java if s.launch else None, "dir": s.dir,
                "join_text": join_text(self.settings()["join_template"], s.cfg)}

    def manifest(self):
        if time.time() - self._manifest[0] > 600:
            self._manifest = (time.time(), fetch_json(MANIFEST_URL))
        return self._manifest[1]

    def mc_versions(self):
        releases = [v["id"] for v in self.manifest()["versions"] if v["type"] == "release"]
        try:
            fabric = {g["version"] for g in fetch_json(FABRIC_META + "/game") if g.get("stable")}
        except (URLError, OSError, ValueError):
            fabric = set()
        default_parent = Path.home() / "MCServers"
        lists = self.loader_lists()
        return {"versions": [{"id": v, "fabric": v in fabric, "forge": loader_choice("forge", v, lists["forge"]),
                              "neoforge": loader_choice("neoforge", v, lists["neoforge"])} for v in releases],
                "ram_total_gb": total_ram_gb(), "default_parent": str(default_parent)}

    def version_meta(self, version):
        entry = next((v for v in self.manifest()["versions"] if v["id"] == version and v["type"] == "release"), None)
        if not entry:
            raise UserError(f"官方正式版清單裡沒有 {version}")
        return fetch_json(entry["url"], entry.get("sha1"))

    def java_status(self, version):
        meta = self.version_meta(version)
        need = (meta.get("javaVersion") or {}).get("majorVersion") or java_for_mc(version) or 8
        best = best_java(need)
        return {"need": need, "ok": bool(best), "have": best[0] if best else None,
                "path": best[1] if best else None, "help_url": java_help_url(need)}

    def free_port(self):
        used = {s.port for s in self.all_servers()}
        return next((p for p in range(25565, 25700) if p not in used and port_free(p)), 25565)

    def start_create(self, req):
        if self.job and self.job["state"] == "running":
            return "已經在建立另一台了，請等它完成"
        if req.get("eula") is not True:   # 一定要使用者自己勾選，程式不代為同意
            return "要先勾選同意 Minecraft 授權合約（EULA）才能建立"
        name = str(req.get("name", "")).strip()
        err = check_folder_name(name)
        if err:
            return err
        loader = req.get("loader")
        if loader not in ("vanilla", "fabric", "forge", "neoforge"):
            return "類型只能選原版、Fabric、Forge 或 NeoForge"
        try:
            ram = int(req.get("ram_gb"))
        except (TypeError, ValueError):
            return "記憶體要選一個數字"
        if not 1 <= ram <= 64:
            return "記憶體要在 1 到 64 GB 之間"
        parent = Path(str(req.get("parent", "")).strip().strip('"'))
        if not parent.is_absolute():
            return "存放位置要選一個完整的資料夾路徑"
        target = parent / name
        if loader in ("forge", "neoforge") and "!" in str(target):   # Java 的 @參數檔與 jar 路徑遇到 ! 會出錯
            return "Forge／NeoForge 的存放路徑不能有驚嘆號 !，請換一個位置或名稱"
        if target.exists() and (not target.is_dir() or any(target.iterdir())):
            return f"「{target}」已經有東西了，請換名稱或換位置，平台不會覆蓋既有資料"
        self.job = {"state": "running", "msg": "準備中…", "pct": None, "id": None, "error": None, "help_url": None}
        threading.Thread(target=self._create, args=(name, str(req.get("version", "")), loader, ram, parent, target), daemon=True).start()
        return None

    def _create(self, name, version, loader, ram, parent, target):
        job, made = self.job, False

        def bar(label):
            def update(frac):
                job["msg"], job["pct"] = label, round(frac * 100)
            return update
        try:
            job["msg"] = "讀取官方版本資料…"
            meta = self.version_meta(version)
            need = (meta.get("javaVersion") or {}).get("majorVersion") or java_for_mc(version) or 8
            java = best_java(need)
            if not java:
                raise UserError(f"Minecraft {version} 需要 Java {need} 以上，電腦上沒有找到。請點下方按鈕下載安裝，裝好後再建立一次。", java_help_url(need))
            server = (meta.get("downloads") or {}).get("server")
            if not server:
                raise UserError(f"Minecraft {version} 官方沒有提供伺服器檔案，請選其他版本。")
            parent.mkdir(parents=True, exist_ok=True)
            if shutil.disk_usage(parent).free < 2147483648:
                raise UserError("硬碟空間不足 2 GB，請換一個位置。")
            made = not target.exists()
            target.mkdir(exist_ok=True)
            if loader == "vanilla":
                jar, label = "server.jar", f"{version} · 原版"
                download(server["url"], target / jar, server["sha1"], server.get("size"), bar("下載官方伺服器檔案…"))
            elif loader in ("forge", "neoforge"):
                jar, kind = None, ("Forge" if loader == "forge" else "NeoForge")
                label = f"{version} · {kind}"
                job["msg"] = f"查詢 {kind} 版本…"
                choice = loader_choice(loader, version, self.loader_lists()[loader])
                if not choice["ok"]:
                    raise UserError(f"{kind}：{choice['why']}，請換版本或換類型。")
                v = choice["version"]
                url, algo = ((forge_installer_url(version, v), "sha1") if loader == "forge" else
                             (neoforge_installer_url(v), "sha256"))
                installer = target / url.rsplit("/", 1)[1]
                download(url, installer, progress=bar(f"下載 {kind} 安裝器…"), **{algo: fetch_checksum(f"{url}.{algo}", algo)})
                job["msg"], job["pct"] = f"安裝 {kind} {v}（安裝器會自己連官方網站下載約數百 MB，約 2–10 分鐘）…", None
                job["lines"] = lines = []

                def on_line(line):
                    lines.append(line)
                    del lines[:-200]
                bat = install_modloader(target, java[1], installer, ram, on_line, self.config_path.parent)
                (target / "mods").mkdir(exist_ok=True)
            else:
                jar, label = "fabric-server.jar", f"{version} · Fabric"
                job["msg"] = "查詢 Fabric 最新穩定版…"
                loaders = [l["loader"]["version"] for l in fetch_json(f"{FABRIC_META}/loader/{quote(version)}") if l["loader"].get("stable")]
                installers = [i["version"] for i in fetch_json(f"{FABRIC_META}/installer") if i.get("stable")]
                if not loaders or not installers:
                    raise UserError(f"Fabric 目前不支援 {version}，請換版本或選原版。")
                download(f"{FABRIC_META}/loader/{quote(version)}/{quote(loaders[0])}/{quote(installers[0])}/server/jar",
                         target / jar, None, None, bar("下載 Fabric 伺服器檔案…"))
                # Fabric 官方沒有公布這個 jar 的雜湊值，改驗檔案完整、內容版本相符。
                with zipfile.ZipFile(target / jar) as z:
                    if z.testzip() is not None or f"game-version={version}".encode() not in z.read("install.properties"):
                        raise UserError("下載的 Fabric 檔案核對不符，請再試一次。")
                (target / "mods").mkdir(exist_ok=True)
            job["msg"], job["pct"] = "寫入設定…", None
            port = self.free_port()
            if jar:
                bat = write_start_script(target, java[1], f"-Xms1G -Xmx{ram}G -jar {jar} nogui")
            (target / "server.properties").write_text(f"server-port={port}\n", encoding="utf-8")
            write_eula(target)
            s = self.register(target, name, label, {"fabric": "Fabric", "forge": "Forge", "neoforge": "NeoForge"}.get(loader, ""), need, bat)
            job["id"] = s.id
            job["msg"] = "第一次開機（要產生世界，約 1–3 分鐘）…"
            err = s.start()
            job.update(state="done", error=(err.get("error") if isinstance(err, dict) else err))
            logging.info("created %s %s", s.id, label)
        except UserError as e:
            job.update(state="error", error=str(e), help_url=e.help_url)
        except (URLError, OSError, ValueError, KeyError, zipfile.BadZipFile) as e:
            logging.exception("create failed")
            job.update(state="error", error=f"建立失敗：連不上官方網站或資料寫不進去（{e}）。請確認網路後再試一次。")
        if job["state"] == "error" and made:
            shutil.rmtree(target, ignore_errors=True)   # 只清這次自己新建、原本不存在的資料夾


def make_handler(hub):
    page = ((RES_DIR / "index.html").read_text(encoding="utf-8")
            .replace("{{FONTS}}", (RES_DIR / "fonts.css").read_text(encoding="utf-8")).replace("{{VERSION}}", VERSION)
            .replace("{{SCRIPT}}", SCRIPT).replace("{{EDITOR}}", EDITOR).replace("{{APP_FILE}}", APP_FILE))

    class H(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def _send(self, code, body, ctype="application/json; charset=utf-8"):
            data = body.encode("utf-8") if isinstance(body, str) else json.dumps(body, ensure_ascii=False).encode("utf-8")
            self.send_response(code)
            self.send_header("Content-Type", ctype)
            self.send_header("Cache-Control", "no-store")
            if ctype.startswith("text/html"):
                self.send_header("Content-Security-Policy", CSP)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def _host_ok(self):
            # 擋「把網域指到 127.0.0.1」騙過瀏覽器同源限制的攻擊。
            if self.headers.get("Host") in hub.hosts:
                return True
            self._send(403, {"error": "forbidden"})
            return False

        def _authed(self):
            # 防止其他網頁偷偷對本機平台下指令：只有平台自己的頁面拿得到這把鑰匙。
            if self.headers.get("X-Hub-Token") == hub.token:
                return True
            self._send(403, {"error": "forbidden"})
            return False

        def _reply(self, result):
            if result is None:
                return self._send(200, {"ok": True})
            self._send(200, result if isinstance(result, dict) else {"error": result})

        def _network(self, fn):
            try:
                self._send(200, fn())
            except UserError as e:
                self._send(200, {"error": str(e), "help_url": e.help_url})
            except (URLError, OSError, ValueError, KeyError):
                logging.exception("network")
                self._send(200, {"error": "連不上 Minecraft 官方網站，請確認網路後再試一次。"})

        def do_GET(self):
            if not self._host_ok():
                return
            u = urlparse(self.path)
            if u.path == "/":
                return self._send(200, page.replace("{{TOKEN}}", hub.token), "text/html; charset=utf-8")
            if u.path == "/api/ping":
                return self._send(200, {"app": APP_ID, "version": VERSION})
            if not self._authed():
                return
            q = parse_qs(u.query)
            if u.path == "/api/state":
                hub.last_seen = time.time()
                return self._send(200, {"servers": [s.snapshot() for s in hub.all_servers()], "job": hub.job, "update": hub.update})
            if u.path == "/api/log":
                s = hub.servers.get(q.get("id", [""])[0])
                if not s:
                    return self._send(404, {"error": "no server"})
                seq, lines = s.logs_since(int(q.get("since", ["0"])[0] or 0))
                return self._send(200, {"seq": seq, "lines": lines})
            if u.path == "/api/settings":
                return self._send(200, hub.settings())
            if u.path in ("/api/commands", "/api/info"):
                s = hub.servers.get(q.get("id", [""])[0])
                if u.path == "/api/info":
                    return self._send(200, hub.server_info(s)) if s else self._send(404, {"error": "no server"})
                return self._send(200, {"fields": FIELDS, "commands": commands_for(
                    s.cfg.get("version", "") if s else "", hub.config.get("custom_commands", []))})
            if u.path == "/api/mc/versions":
                return self._network(hub.mc_versions)
            if u.path == "/api/mc/java":
                return self._network(lambda: hub.java_status(q.get("version", [""])[0]))
            self._send(404, {"error": "not found"})

        def do_POST(self):
            if not self._host_ok() or not self._authed():
                return
            try:
                body = json.loads(self.rfile.read(int(self.headers.get("Content-Length") or 0)) or b"{}")
            except ValueError:
                return self._send(400, {"error": "bad json"})
            path = urlparse(self.path).path
            if path == "/api/quit":
                if hub.busy():
                    return self._send(200, {"error": "還有伺服器在執行或正在處理資料，請先關機再關閉平台"})
                self._send(200, {"ok": True})
                threading.Thread(target=hub.httpd.shutdown, daemon=True).start()
                return
            if path == "/api/pick-folder":
                return self._send(200, pick_folder())
            if path == "/api/add":
                return self._reply(hub.add_existing(body.get("dir"), body.get("name")))
            if path == "/api/create":
                return self._reply(hub.start_create(body))
            if path == "/api/settings":
                try:
                    return self._send(200, hub.save_settings(body))
                except UserError as e:
                    return self._reply(str(e))
            s = hub.servers.get(body.get("id"))
            if not s:
                return self._send(404, {"error": "找不到這台伺服器"})
            if path == "/api/start":
                return self._reply(s.start())
            if path == "/api/stop":
                return self._reply(s.stop())
            if path == "/api/force":
                return self._reply(s.force_kill())
            if path == "/api/backup":
                return self._reply(s.backup())
            if path == "/api/remove":
                return self._reply(hub.remove(s))
            if path == "/api/eula":
                if body.get("agree") is not True:
                    return self._reply("要勾選同意才能繼續")
                try:
                    write_eula(s.dir)
                except OSError as e:
                    return self._reply(f"寫不進 eula.txt：{e}")
                return self._reply(None)
            if path == "/api/open":
                target = Path(s.dir) / {"mods": "mods", "backups": "backups"}.get(body.get("what"), "")
                try:
                    target.mkdir(exist_ok=True)
                    open_path(target)
                except UserError as e:
                    return self._reply(str(e))
                except OSError as e:
                    return self._reply(f"打不開資料夾：{e}")
                return self._reply(None)
            if path == "/api/run":
                return self._reply(hub.run_command(s, body))
            if path == "/api/server-settings":
                return self._reply(hub.server_settings(s, body))
            if path == "/api/cmd":
                raw = str(body.get("cmd", ""))
                text = raw.strip()
                err = "指令不能換行" if "\n" in raw or "\r" in raw else s.send(text) if text else "指令是空的"
                if not err:
                    s.add_log(f"> {text}")
                return self._reply(err)
            if path == "/api/address":
                s.cfg["address"] = str(body.get("address", "")).strip()[:200]
                hub.save_config()
                return self._reply(None)
            self._send(404, {"error": "not found"})

    return H


class HubHTTPServer(ThreadingHTTPServer):
    # Windows 上預設可重複綁同一個埠，第二次雙擊會變成兩個平台互搶；關掉才會走「叫回視窗」。
    allow_reuse_address = False


def ping_action(resp):
    """埠被占時看對方是誰：同版平台→reopen；別版平台（1.0.0 沒回版本）→old；不是平台→None。"""
    if not isinstance(resp, dict) or resp.get("app") != APP_ID:
        return None
    return "reopen" if resp.get("version") == VERSION else "old"


def main():
    reset_dll_dir()
    ap = argparse.ArgumentParser()
    ap.add_argument("--config", default=str(DATA_DIR / "hub_config.json"))
    ap.add_argument("--port", type=int, default=47892)
    ap.add_argument("--no-window", action="store_true")
    args = ap.parse_args()
    config = Path(args.config)
    config.parent.mkdir(parents=True, exist_ok=True)
    logging.basicConfig(filename=config.parent / "hub.log", level=logging.INFO, encoding="utf-8",
                        format="%(asctime)s %(levelname)s %(message)s")
    url = f"http://127.0.0.1:{args.port}/"
    hub = Hub(config)
    hub.hosts = {f"127.0.0.1:{args.port}", f"localhost:{args.port}"}
    try:
        hub.httpd = HubHTTPServer(("127.0.0.1", args.port), make_handler(hub))
    except OSError:
        # 平台已經開著（例如關了視窗但伺服器還在跑）→ 同版就把視窗叫回來；不同版請使用者先關舊的
        try:
            other = json.load(urlopen(Request(url + "api/ping"), timeout=2))
        except (OSError, ValueError):
            other = None
        action = ping_action(other)
        if action == "reopen":
            if not args.no_window:
                open_window(url)
            return
        if action == "old":
            fatal_box(f"另一個版本的平台（v{other.get('version') or '1.0.0'}）還開著，請先在那邊按「關閉平台」再開這一版。")
            return
        raise
    logging.info("hub up on %s", url)
    threading.Thread(target=hub.monitor, daemon=True).start()
    threading.Thread(target=hub.check_update, daemon=True).start()
    if not args.no_window:
        open_window(url)
    hub.httpd.serve_forever()
    logging.info("hub down")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        logging.exception("crash")
        fatal_box(f"平台開不起來：{e}")
        raise
