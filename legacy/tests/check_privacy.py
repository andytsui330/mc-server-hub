# 發佈前檢查：exe 內部（含壓縮的程式碼）和所有要發出去的檔案都不能帶到個人資料。
# 禁用字樣清單放在原始碼以外的私人檔案（一行一個），找不到或是空的就失敗。
# 跑法：_build\venv\Scripts\python.exe tests\check_privacy.py <exe、檔案或資料夾…>
# 給資料夾就連檔名、資料夾名一起掃；.git、_build、dist、__pycache__ 不掃（版本歷史與打包暫存）。
# 只掃原始碼工作樹（不含 exe）時一般的 py -3 就能跑：py -3 tests\check_privacy.py D:\Git\mc-server-hub
import os, sys, zipfile
from pathlib import Path

SKIP_DIRS = {".git", "_build", "dist", "__pycache__"}

# 跟平台設定檔同一個資料夾：Windows %APPDATA%\MCServerHub、Mac ~/Library/Application Support/MCServerHub
LIST = (Path(os.environ["APPDATA"]) if sys.platform == "win32" else
        Path.home() / "Library" / "Application Support" if sys.platform == "darwin" else
        Path.home() / ".config") / "MCServerHub" / "release_banned.txt"
try:
    BANNED = [w.strip().lower().encode("utf-8") for w in LIST.read_text(encoding="utf-8-sig").splitlines() if w.strip() and not w.startswith("#")]
except OSError:
    sys.exit(f"找不到禁用字樣清單：{LIST}。沒有清單不能打包。")
if len(BANNED) < 5:
    sys.exit(f"禁用字樣清單只有 {len(BANNED)} 項，看起來不完整，停止打包。")
hits, count = [], 0


def scan(label, data):
    global count
    count += 1
    low = data.lower()
    # 不印出字樣本身，只印第幾項，避免清單內容跑進紀錄
    hits.extend((label, f"清單第 {i + 1} 項") for i, b in enumerate(BANNED) if b in low or b.decode().encode("utf-16-le") in low)


def scan_file(p):
    data = p.read_bytes()
    scan(str(p), data + p.name.encode("utf-8"))
    if p.suffix.lower() == ".zip":
        with zipfile.ZipFile(p) as z:
            for n in z.namelist():
                scan(f"{p.name}!{n}", z.read(n) + n.encode("utf-8"))
    # Windows 的 exe、Mac .app 裡 Contents/MacOS/ 的主程式：PyInstaller 把壓縮過的程式碼附在檔尾，要拆開掃
    if p.suffix.lower() == ".exe" or (p.parent.name == "MacOS" and p.parent.parent.name == "Contents"):
        from PyInstaller.archive.readers import CArchiveReader
        try:
            arch = CArchiveReader(str(p))
        except Exception:
            if p.suffix.lower() == ".exe":
                raise
            return   # .app 裡不是 PyInstaller 打包的其他執行檔，上面已經掃過原始位元組
        for name in arch.toc:
            if name.endswith(".pyz"):
                pyz = arch.open_embedded_archive(name)
                for mod in pyz.toc:
                    scan(f"pyz:{mod}", pyz.extract(mod, raw=True))
            else:
                scan(f"exe:{name}", arch.extract(name))


def walk(root):
    for dirpath, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIRS)
        for d in dirs:  # 資料夾名稱也要掃
            scan(str(Path(dirpath) / d), d.encode("utf-8"))
        for f in sorted(files):
            yield Path(dirpath) / f


if len(sys.argv) < 2:
    sys.exit("要指定要掃的 exe、檔案或資料夾")
for arg in sys.argv[1:]:
    root = Path(arg)
    if not root.exists():
        sys.exit(f"找不到要掃的項目：{root}")
    for p in ([root] if root.is_file() else walk(root)):
        scan_file(p)
print("用", len(BANNED), "項禁用字樣掃過", count, "個項目")
if hits:
    for h in hits[:40]:
        print("命中", h)
    sys.exit(1)
print("乾淨：沒有個人資料")
