# MC Server Hub（MC 開服平台）

> **English summary** — A small desktop tool for running Minecraft: Java Edition servers on your own computer (Windows and macOS). One click to start and stop, a live console, a command menu with confirmations for risky commands, world backups, and a wizard that creates Vanilla, Fabric, Forge or NeoForge servers from the official download sites. It only listens on `127.0.0.1`, so nobody can control it from the network. The interface is Traditional Chinese only. Not an official Minecraft product; not approved by or associated with Mojang or Microsoft. MIT licensed.

在自己的電腦上開 Minecraft（Java 版）伺服器的小工具：按一下就開機、關機，看主控台、點選常用指令、備份世界，也能從官方網站建立新的伺服器。

![畫面](docs/screenshot.png)

## 能做什麼

- **開關機與主控台**：每台伺服器一個按鈕開機、關機，主控台即時顯示；當機時把常見錯誤翻成白話。
- **建立新伺服器**：原版、Fabric、Forge、NeoForge 四種，從 Mojang／Fabric／Forge／NeoForge 官方網站下載，下載檔會核對檢查碼。
- **加入現有伺服器**：選資料夾就好，連接埠、版本、載入器自動讀取。
- **常用指令選單**：分類點選、原地填欄位；危險指令要確認，影響全部玩家或世界的要打字確認。`Ctrl K`（Mac 是 `⌘ K`）搜尋指令。
- **備份世界**：手動備份，或設定「正常關機後自動備份」。
- **其他**：記憶體調整、給朋友的加入說明（一鍵複製）、我的筆記、實用連結、伺服器圖示與顏色、開關機音效。

## 下載與第一次使用

到本 repo 的 **Releases** 頁面下載最新版壓縮檔：

| 系統 | 檔案 | 第一次打開 |
| :-- | :-- | :-- |
| Windows 10／11 | `MCServerHub-<版本>-windows.zip` | 解壓縮後雙擊 `MC開服平台.exe`。若出現「Windows 已保護您的電腦」，按「其他資訊」→「仍要執行」。 |
| macOS 11 以上（Intel＋Apple 晶片通用） | `MCServerHub-<版本>-mac.zip` | 解壓縮後雙擊，跳出「Apple 無法驗證」按「完成」→ 到「系統設定 → 隱私權與安全性」按「強制打開」→ 輸入密碼 →「打開」。只要做一次，之後直接雙擊。（macOS 15 起「右鍵 → 打開」已經沒用。） |

會跳警告是因為這個工具沒有付費購買微軟／Apple 的數位簽章，不是偵測到病毒。

**Mac 晶片支援**：Mac 版是 universal2，同一個 app 內含 Intel 與 Apple 晶片（M 系列）兩份原生程式，不靠 Rosetta。1.2.0 在 2018 Intel MacBook Pro（macOS 15.8.1）實機測過；**Apple 晶片版本已內含、尚未在 M 系列 Mac 實機測試**，遇到問題歡迎回報。每個版本都附 `SHA256SUMS-<版本>.txt` 檢查碼，可以自己核對下載的檔案。

需要 Java：開服前平台會檢查電腦上的 Java 夠不夠新，不夠會給你官方（Eclipse Temurin）下載連結，裝好不用重開平台。

詳細步驟見 [docs/使用說明.txt](docs/使用說明.txt)。

## 安全與隱私

- **只在你的電腦上運作**：平台只聽 `127.0.0.1`，每次啟動產生一把隨機鑰匙，網路上的其他人無法控制它。
- **不會自己更新**，也不會偷偷下載程式。
- **對外連線只有兩種**：
  1. 建立伺服器時，從上面列的官方網站下載伺服器檔案與版本清單（只接受這幾個網域）。
  2. **新版提醒**：啟動時讀一個固定網址上的版本號（例如 `1.2.0`）。只送一個 GET 請求、最多讀 32 個字元、只接受數字格式、不跟隨轉址、不傳送任何你的資料，有新版也只會在畫面上提示。不想要的話，在「實用連結」選單裡取消「啟動時檢查新版」，之後就完全不連線。
- **Minecraft 授權合約（EULA）** 必須由你本人在畫面上勾選同意，工具不會代勾。
- 伺服器 jar 檔不包含在本工具裡，一律由你的電腦從官方網站下載。

## 從原始碼執行

後端只用 Python 標準函式庫，畫面是一個不載入任何外部資源的網頁（`src/index.html`），由 Edge／Chrome 的 App 模式顯示。

```bash
python3 src/hub.py                 # Windows 用 py -3
python3 tests/test_hub.py          # Windows 自動測試（需要 JDK；只用假伺服器與暫存資料夾）
python3 tests/test_mac.py          # Mac 自動測試（同上）
```

打包用 PyInstaller：Windows 的步驟寫在 `build.ps1`；Mac 用 `sh build-mac.sh`，要先裝 python.org 官方安裝檔的 Python（universal2 版），才能打出 Intel＋Apple 晶片通用的 `.app`。

## 授權與聲明

- 程式碼：[MIT License](LICENSE)。
- 內嵌字型 Inter Tight、Geist Mono：SIL Open Font License 1.1，全文見 [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt)。
- 介面裡的像素圖示是本專案自己畫的，不含任何 Mojang 的材質、字型、音效或標誌。
- **NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.** 本工具不是 Minecraft 官方產品，與 Mojang、Microsoft 無關。「Minecraft」是 Mojang Synergies AB 的商標。

更新紀錄見 [CHANGELOG.md](CHANGELOG.md)。
