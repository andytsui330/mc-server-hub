"""一次改好所有版本號（維護手冊第二節）：py -3 scripts/bump_version.py 2.0.1 [--date 2026-11-30]

改 tauri.conf.json、兩個 Cargo.toml、package.json 與兩個 lock 檔；給 --date 時順便把 CHANGELOG 該版標題的日期填好。
不改 pages/version.json（Cloudflare 版本號最後才改）。
"""
import argparse
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
# (檔案, 版本號所在的樣式)；每個樣式必須剛好命中預期次數，否則整批不寫。
TARGETS = [
    ("app/src-tauri/tauri.conf.json", r'(\n  "version": ")[^"]+(")', 1),
    ("core/Cargo.toml", r'(\[package\]\nname = "mc-hub-core"\nversion = ")[^"]+(")', 1),
    ("app/src-tauri/Cargo.toml", r'(\[package\]\nname = "mc-server-hub"\nversion = ")[^"]+(")', 1),
    ("Cargo.lock", r'(name = "(?:mc-hub-core|mc-server-hub)"\nversion = ")[^"]+(")', 2),
    ("app/package.json", r'(\n  "version": ")[^"]+(")', 1),
    ("app/package-lock.json", r'("name": "mc-server-hub-app",\n\s*"version": ")[^"]+(")', 2),
]


def bump(root, version, date=None):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("版本號格式要像 2.0.1")
    if date and not re.fullmatch(r"\d{4}-\d{2}-\d{2}", date):
        raise ValueError("日期格式要像 2026-11-30")
    changes = {}
    for name, pattern, expected in TARGETS:
        file = root / name
        text = file.read_bytes().decode("utf-8")
        # 保留原本的換行（CRLF/LF），比對時統一成 LF
        crlf = "\r\n" in text
        body = text.replace("\r\n", "\n")
        body, count = re.subn(pattern, lambda m: m.group(1) + version + m.group(2), body)
        if count != expected:
            raise ValueError(f"{name}：預期改 {expected} 處，實際找到 {count} 處，全部不寫")
        changes[file] = body.replace("\n", "\r\n") if crlf else body
    if date:
        file = root / "CHANGELOG.md"
        text = file.read_bytes().decode("utf-8")
        text, count = re.subn(r"^## " + re.escape(version) + r"(?:[（(][^）)\r\n]*[）)])?[ \t]*(?=\r?$)",
                              f"## {version}（{date}）", text, count=1, flags=re.M)
        if count != 1:
            raise ValueError(f"CHANGELOG 找不到「## {version}」標題，全部不寫")
        changes[file] = text
    for file, text in changes.items():
        file.write_bytes(text.encode("utf-8"))
    return sorted(str(f.relative_to(root)) for f in changes)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("--date", help="發佈日 YYYY-MM-DD，填進 CHANGELOG 該版標題")
    args = parser.parse_args()
    try:
        for name in bump(ROOT, args.version, args.date):
            print("已改", name)
    except ValueError as error:
        print("停止：", error, file=sys.stderr)
        return 1
    print("記得跑 py -3 scripts/release_support.py prepare 確認一致（含文件把關）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
