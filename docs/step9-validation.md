# 第9步驗證與邊界

唯一進度來源仍是 AI-Agent 專案 `2026-11-14_2.0成熟軟體計畫書.md`「目前進度」。本檔是證據索引，沒有另一份進度表。

## 功能

- Modrinth：搜尋相同 Minecraft／載入器且伺服器可用的模組、解析 required project/version 相依、衝突／循環檢查、已裝清單、更新檢查與更新計畫。先看完整變更，再確認套用；不自動升遊戲。Hub持有計畫、前端只傳token，避免篡改下載網址。SHA-1／大小自行再驗；stage後交易換檔，原檔移到 `.mchub-mod-backups`，失敗rollback。交換中斷marker擋開機，保留資料供人工救援。未知mods保留、只自動管理平台安裝的模組。
- 重開：預設關閉。當機等待5–600秒、最多1–10次（預設30秒／3次），達上限停下；手動開機重設次數，手動stop／force／console stop取消待重開。每日排程平台開啟才做、只重開就緒的本平台伺服器、當天嘗試先落盤，不補昨天。先廣播倒數，再正常stop；正常退出、備份完成且磁碟／Java／EULA／占埠檢查通過才重開，失敗不強殺或無限重試。
- Java：官方Temurin GA HotSpot JDK，按major／OS／arch挑版本。先列版本／大小／SHA-256，再由使用者確認下載到平台 runtimes；來源與repo/tag/archive版本綁定，大小／SHA-256自行再驗。串流解zip/tar，拒絕穿越、絕對路徑、連結、重複覆蓋、加密、分卷、炸彈；驗release與bin/java後原子發布，真產品再跑version核對。沒有修改PATH／JAVA_HOME／系統設定／既有bat。一般java命令會挑平台管理的合適runtime，自訂Java路徑保持原意；新建服可選用managed runtime。

Java具有全域入口，沒有伺服器也可在建服精靈下載。背景工作阻擋開服／更新／移除，伺服器與全域忙碌狀態分開；正常關機與取消待重開仍可執行。沒有新套件／工具／權限／鎖版。

## 來源與傳輸

[Modrinth API](https://docs.modrinth.com/api/)／[Version schema](https://docs.modrinth.com/api/operations/getversion/)；[Adoptium官方API](https://adoptium.net/installation/ci-scripts)／[archive說明](https://adoptium.net/installation/archives)。本輪真查官方metadata，並下載Fabric API模組到TEMP逐byte雜湊核對、完成背景交易安裝；沒有執行第三方模組或下載／安裝真JDK。

各功能獨立HTTPS精確白名單，不擴大舊建服來源。Modrinth只API v2與官方CDN data；Temurin只官方API、指定adoptium/temurinN-binaries release及官方release-assets轉址。每一跳重檢；讀取閒置30秒、每次請求總上限15分鐘；串流超過已知大小立即停止寫入。

## 驗證

- 全workspace測試、clippy全targets、strict/Vite、正式exe建置；結果見 `target/step9-rust-tests.log` 與唯一計畫書。
- 新增Modrinth 12組：篩選、相依、更新、壞hash／大小、未知保留、Windows真換檔失敗rollback、中斷阻擋。Java 7組：metadata／tag／archive綁定、8與21版本、SHA／CRC、zipSlip／tar links／hardlinks／duplicates／bomb、65MiB成員串流與失敗不覆蓋。沒有執行fake exe。
- 重開狀態機可控時鐘測試、維護期間不能移除與更新、精確來源政策、超量串流、長token回歸。
- `tests/p2-ui.cjs` 真WebView2：真Java假服連續exit23，cap兩次後停止；手動stop不重開；每日真廣播倒數→stop→備份→重開；真官方Modrinth搜尋與相依計畫／取消、確認安裝到TEMP、更新檢查；真官方Adoptium metadata／取消、無id全域入口；三個P2頁四桌面尺寸截圖。完整run不得設 `MCHUB_P2_METADATA_ONLY`；該變數只供查詢失敗定位，不是完整驗收。
- `tests/p1-ui.cjs`、`tests/maturity-ui.cjs`為受影響流程回歸；原第0～8步不重新評估或重做全套對等驗收。UI detector零警告並實看截圖；證據 `target/step9-ui/`。

所有產品測試TEMP專用MCHUB_DATA_DIR；完整Rust suite另隔離TEMP／TMP，保留新fixture。正式APPDATA三檔存在狀態與SHA-256前後相同，不碰真世界、真啟動檔或剪貼簿。

獨立審查修正：維護中移除造成工作脫離Hub、手動stop／自動Start競態、備份入隊前重開競態、下載超量仍寫入、自訂Java檢查與實際選用不一致。另由真API流程抓到24字token超過舊helper上限，先紅後綠補測。

## 限制

模組只管理平台安裝的jar，既有未知模組相容性需人工確認；中斷交易保守阻擋、人工救援。沒有以真第三方模組開遊戲驗相容、沒有真JDK安裝或Mac實機解壓；Mac只有封裝fixture，若官方包使用被拒絕的連結／特殊格式會安全失敗。沒有執行正式OS登入／匣選單滑鼠／NSIS／真服6GB長測，沒有push／Release／Cloudflare／私人備份，未讀更新器私鑰。
