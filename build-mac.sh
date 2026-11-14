#!/bin/sh
# Mac 出版流程（在 Andy 的 2018 Intel Mac 跑）：打包 .app → 個人資料掃描 → 壓縮 → 記錄 SHA256。
# 任何一步失敗就停。跑法：在這個資料夾執行 sh build-mac.sh
# 成品放 ~/MCServerHub-releases/<版本>/（出過的版本不覆蓋）；要另存到雲端硬碟就設 MC_HUB_COPY_TO=<資料夾>。
# 成品是 universal2（同一個 .app 內含 Intel＋Apple 晶片兩份程式），所以一定要用 python.org 官方安裝檔的 Python
# （/Library/Frameworks/Python.framework，universal2 版）；Xcode 附的 python3、Homebrew 只有單一架構，會被擋下。
# 要指定別的 Python 就設 MC_HUB_PYTHON=<python3 路徑>。
set -eu
root="$(cd "$(dirname "$0")" && pwd)"
releases="${MC_HUB_RELEASES:-$HOME/MCServerHub-releases}"
banned="$HOME/Library/Application Support/MCServerHub/release_banned.txt"
app_name="MC開服平台"

[ -f "$banned" ] || { echo "找不到禁用字樣清單 $banned，沒有清單不能打包"; exit 1; }
version="$(sed -n 's/^VERSION = "\([0-9]*\.[0-9]*\.[0-9]*\)".*/\1/p' "$root/src/hub.py")"
[ -n "$version" ] || { echo "讀不到 hub.py 的 VERSION"; exit 1; }
final="$releases/$version"
[ ! -e "$final" ] || { echo "$final 已經存在。出過的版本不覆蓋，請先改 hub.py 的 VERSION"; exit 1; }

base_py="${MC_HUB_PYTHON:-}"
if [ -z "$base_py" ]; then
    for cand in /Library/Frameworks/Python.framework/Versions/3.*/bin/python3; do
        [ -x "$cand" ] && base_py="$cand"
    done
fi
[ -n "$base_py" ] && [ -x "$base_py" ] || { echo "找不到 python.org 官方安裝檔的 Python（universal2），請先安裝"; exit 1; }
base_bin="$("$base_py" -c 'import sys; print(sys.executable)')"
case "$(lipo -archs "$base_bin" 2>/dev/null)" in
    *x86_64*arm64*|*arm64*x86_64*) ;;
    *) echo "$base_bin 不是 universal2（只有 $(lipo -archs "$base_bin" 2>/dev/null)），打不出 Intel＋Apple 晶片通用版"; exit 1 ;;
esac

venv="$root/_build/venv-mac"
py="$venv/bin/python"
if [ ! -x "$py" ]; then
    echo "== 建立打包環境（$base_bin）"
    "$base_py" -m venv "$venv"
    "$py" -m pip install --quiet pyinstaller certifi
fi

echo "== 打包"
"$py" -m PyInstaller --noconfirm --clean --windowed --onedir --name "$app_name" \
    --icon "$root/src/icon.icns" --add-data "$root/src/index.html:." --add-data "$root/src/fonts.css:." \
    --hidden-import certifi --osx-bundle-identifier "io.github.mc-server-hub" --target-arch universal2 \
    --distpath "$root/dist-mac" --workpath "$root/_build/work-mac" --specpath "$root/_build" "$root/src/hub.py"

app="$root/dist-mac/$app_name.app"
# PyInstaller 預設把 .app 版本寫成 0.0.0（Finder「取得資訊」會看到），改成真的版本號、最低 macOS 11
# （python.org 的 universal2 Python 本身就要 macOS 11），改完 Info.plist 要重新做 ad-hoc 簽章
plist="$app/Contents/Info.plist"
plutil -replace CFBundleShortVersionString -string "$version" "$plist"
plutil -replace CFBundleVersion -string "$version" "$plist"
plutil -replace LSMinimumSystemVersion -string "11.0" "$plist"
plutil -replace NSHumanReadableCopyright -string "Copyright (c) 2026 Andy and MC Server Hub contributors. MIT License." "$plist"
codesign --force --deep --sign - "$app"
echo "== 檢查兩種晶片都在、簽章有效"
# 主程式與每個 .so／.dylib 都要同時有 x86_64 與 arm64；少一個就代表那台 Mac 開不起來
find "$app" -type f | while IFS= read -r f; do
    if file -b "$f" | grep -q "Mach-O"; then
        archs="$(lipo -archs "$f" 2>/dev/null || true)"
        case " $archs " in
            *" x86_64 "*) case " $archs " in *" arm64 "*) continue ;; esac ;;
        esac
        echo "缺架構：$f（$archs）"; echo x >> "$root/_build/arch-bad"
    fi
done
[ ! -s "$root/_build/arch-bad" ] || { rm -f "$root/_build/arch-bad"; echo "有檔案不是 universal2，停止"; exit 1; }
rm -f "$root/_build/arch-bad"
# Apple 晶片一定要有簽章才跑得動；PyInstaller 會自動做免費的 ad-hoc 簽章，這裡確認它完整
codesign --verify --deep --strict "$app" || { echo "簽章檢查沒過，停止"; exit 1; }
echo "主程式架構：$(lipo -archs "$app/Contents/MacOS/$app_name")"

stage="$root/_build/stage-mac/$version"
rm -rf "$stage"; mkdir -p "$stage"
cp -R "$root/dist-mac/$app_name.app" "$stage/"
cp "$root/docs/使用說明.txt" "$stage/使用說明.txt"
cp "$root/LICENSE" "$stage/LICENSE.txt"
cp "$root/THIRD_PARTY_NOTICES.txt" "$stage/THIRD_PARTY_NOTICES.txt"
zip="$root/_build/stage-mac/$app_name-$version-mac.zip"
rm -f "$zip"
(cd "$stage" && ditto -c -k --sequesterRsrc . "$zip")   # ditto 保留 .app 的權限與屬性

echo "== 個人資料掃描"
"$py" "$root/tests/check_privacy.py" "$stage" "$zip" "$root"

mkdir -p "$final"
cp -R "$stage/." "$final/"
cp "$zip" "$releases/"
(cd "$releases" && shasum -a 256 "$(basename "$zip")" > "SHA256-$version-mac.txt")
if [ -n "${MC_HUB_COPY_TO:-}" ]; then
    mkdir -p "$MC_HUB_COPY_TO"
    cp "$zip" "$releases/SHA256-$version-mac.txt" "$MC_HUB_COPY_TO/"
fi
echo "完成：$final"
cat "$releases/SHA256-$version-mac.txt"
