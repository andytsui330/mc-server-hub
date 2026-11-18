# shortcut: 2.0 起舊版移到 legacy/，本檔打包原始碼時的 docs、pages、README 等路徑沒跟著改, 真的要再發 1.x 時才修
# 出版流程：打包 → 個人資料掃描 → 測試 exe → 存入 releases\<版本>\ → 記錄 SHA256 →（有設定才）另存原始碼備份。
# 任何一步失敗就停，不會產生發佈檔。跑法：在這個資料夾用 PowerShell 跑 .\build.ps1
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
# 成品位置：預設放 repo 內 releases\（不進 Git）。要放別處就在同資料夾建 build.local.ps1（不進 Git），例如：
#   $releases = 'D:\某處\releases'; $copyTo = 'E:\備份\mc-server-hub'   # $copyTo：原始碼壓縮檔另存一份的位置，可省略
$releases = Join-Path $root 'releases'
$copyTo = $null
if (Test-Path "$root\build.local.ps1") { . "$root\build.local.ps1" }
New-Item -ItemType Directory -Force $releases | Out-Null
$banned = Join-Path $env:APPDATA 'MCServerHub\release_banned.txt'
$exeName = 'MC開服平台.exe'

function Step($name, [scriptblock]$run) {
    Write-Host "== $name"
    & $run
    if ($LASTEXITCODE -ne 0) { throw "$name 失敗（結束代碼 $LASTEXITCODE）" }
}

if (-not (Test-Path $banned)) { throw "找不到禁用字樣清單 $banned，沒有清單不能打包" }
$version = [regex]::Match((Get-Content "$root\src\hub.py" -Raw -Encoding UTF8), '(?m)^VERSION = "(\d+\.\d+\.\d+)"').Groups[1].Value
if (-not $version) { throw '讀不到 hub.py 的 VERSION' }
$final = Join-Path $releases $version
if (Test-Path $final) { throw "$final 已經存在。出過的版本不覆蓋，請先改 hub.py 的 VERSION" }

$venv = Join-Path $root '_build\venv'
$py = "$venv\Scripts\python.exe"
if (-not (Test-Path $py)) {
    Step '建立打包環境' { py -3.12 -m venv $venv }
    Step '安裝 PyInstaller' { & $py -m pip install --quiet pyinstaller certifi }
}
Step '打包' {
    & $py -m PyInstaller --noconfirm --clean --onefile --noconsole --name 'MC開服平台' `
        --icon "$root\src\icon.ico" --add-data "$root\src\index.html;." --add-data "$root\src\fonts.css;." --hidden-import certifi `
        --distpath "$root\dist" --workpath "$root\_build\work" --specpath "$root\_build" "$root\src\hub.py"
}

# 先在暫存區組好要發出去的東西，全部檢查通過才搬進 releases
$stage = Join-Path $root "_build\stage\$version"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "$root\dist\$exeName" $stage
# 使用說明存成記事本一定讀得懂的格式（UTF-8 含 BOM、CRLF）
$guide = (Get-Content "$root\docs\使用說明.txt" -Raw -Encoding UTF8) -replace "`r?`n", "`r`n"
[IO.File]::WriteAllText("$stage\使用說明.txt", $guide, (New-Object Text.UTF8Encoding($true)))
# 授權檔跟著 exe 一起發（字型內嵌在 exe 裡，OFL 要求附授權全文）
foreach ($n in 'LICENSE', 'THIRD_PARTY_NOTICES.txt') {
    $t = (Get-Content "$root\$n" -Raw -Encoding UTF8) -replace "`r?`n", "`r`n"
    [IO.File]::WriteAllText("$stage\$([IO.Path]::GetFileNameWithoutExtension($n)).txt", $t, (New-Object Text.UTF8Encoding($true)))
}
$zip = Join-Path $root "_build\stage\MCServerHub-$version-windows.zip"   # 英文檔名：GitHub Release 會吃掉中文附件檔名
$srcZip = Join-Path $root "_build\stage\mc-server-hub-$version-src.zip"
Remove-Item $zip, $srcZip -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage\*" -DestinationPath $zip
Compress-Archive -Path "$root\src", "$root\tests", "$root\docs", "$root\pages", "$root\build.ps1", "$root\build-mac.sh", "$root\README.md", "$root\.gitignore", "$root\LICENSE", "$root\THIRD_PARTY_NOTICES.txt", "$root\CHANGELOG.md" -DestinationPath $srcZip

# 連整個原始碼工作樹（含檔名）一起掃，不只發出去的檔案
Step '個人資料掃描' { & $py "$root\tests\check_privacy.py" "$stage\$exeName" $stage $zip $srcZip $root }
Step '測試 exe' { py -3 "$root\tests\test_hub.py" "$stage\$exeName" }

New-Item -ItemType Directory -Force $final | Out-Null
Copy-Item "$stage\*" $final
Copy-Item $zip $releases
$lines = foreach ($f in @("$final\$exeName", "$final\使用說明.txt", (Join-Path $releases (Split-Path $zip -Leaf)))) {
    '{0}  {1}' -f (Get-FileHash $f -Algorithm SHA256).Hash, (Split-Path $f -Leaf)
}
$lines | Set-Content (Join-Path $releases "SHA256-$version.txt") -Encoding UTF8
if ($copyTo) {
    New-Item -ItemType Directory -Force $copyTo | Out-Null
    Copy-Item $srcZip $copyTo
}
Write-Host "完成：$final"
$lines
