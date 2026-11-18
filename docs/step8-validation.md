# 第 8 步驗證證據

唯一進度正本：AI-Agent 專案 `2026-11-14_2.0成熟軟體計畫書.md`「目前進度」。本檔只記證據與邊界。

## 實作

- 玩家：完整 `list` 回覆、OP／白名單／封鎖同頁；查詢失敗明示未知，不拿 SLP 樣本冒充完整名單。執行中只送經驗證單一指令，回覆看主控台；離線先 `.bak` 再原子寫 JSON，保留未知欄位。優先用既有 UUID，未知玩家須自行核對 UUID。外部執行、開關機過渡、備份還原與更新擋寫。
- properties：15 項為 motd、difficulty、gamemode、max-players、pvp、white-list、online-mode、server-port、view-distance、simulation-distance、spawn-protection、allow-flight、enable-command-block、force-gamemode、level-seed，其餘現有鍵收進進階區。保留註解、原行與未知值；Java escapes／續行／Unicode／冒號／重複鍵共用解析；原檔逐 byte 備份、SHA-256 revision 防一般外部編輯覆蓋。執行中修改明示重開。
- 備份：自訂位置依伺服器路徑雜湊分目錄，預設各台 backups。只清理驗證過的完整平台備份，預設保留 10；舊未知資料、未完成、快照與交易資料保留。inventory 含空目錄及逐檔 SHA-256。來源、stage、交換後均核對；固定先快照，I/O 失敗 rollback。交換前同步 journal，程序中斷時擋開機與新備份還原，保留交易資料供人工救援。
- 開機：主畫面／總覽開機先進檢查頁，確認才啟動；保留 Java／EULA／占埠，加磁碟不足 1 GiB 阻擋、不足 5 GiB／查詢失敗提醒。實際啟動再檢查。Windows 另連本機埠，避免 wildcard 綁定誤判；離線操作重讀新埠，執行中維持實際啟動埠。

設定頁接通位置、保留份數、排程時間與開關；還原快照為必要保護，固定開啟。排程依本機時間、平台開啟才執行；當天錯過時間會嘗試一次，執行中跳過並記原因，不補昨天、不自動關機。嘗試日先落盤，重開不重複。指定自動開服先執行，再開始排程巡檢，避免兩者搶同一台。

邏輯與安全工作鎖留 core，Tauri 只接桌面退出草稿事件；沒有新增套件、工具、權限或鎖版。

## 測試

- `cargo test --workspace`：36 核心單元＋11 備份整合＋16 fixture 組（339 標準答案）＋1 設定整合＋6 P1 邊界＋2 玩家＋9 properties＋5 外殼；原 6 項條件測試仍 ignored。
- clippy 全 targets 零警告；strict TypeScript／Vite／正式 exe 建置。
- `tests/p1-ui.cjs` 真 WebView2 十項：入口、離線名單／UUID／bak、六個全域導覽取消及 Ctrl K、真 CloseRequested 與共用退出取消、15 欄／進階／草稿／bak、備份還原／快照、開機檢查至真 Java FakeServer Done、執行中玩家指令與擋備份還原、執行中設定重開提示、四頁四尺寸／深淺主題／備份設定落盤。
- `tests/ui.cjs` 既有回歸通過（依新開機檢查入口調整兩個點擊）；`tests/maturity-ui.cjs` 13 項及 `tests/assets.cjs` 通過。不冒充第 6 步完整對等驗收重跑。
- Impeccable detector 四個 UI 目標零警告；已實看四桌面尺寸與深淺截圖。證據在忽略版控的 `target/step8-ui/`。

產品執行全用 TEMP 專用 MCHUB_DATA_DIR。完整 Rust suite 另把 TEMP／TMP 指到本輪新 sandbox，舊 helper 只清本輪 fixture；新 fixture 保留。正式 APPDATA 三檔存在狀態與 SHA-256 前後相同，未使用剪貼簿、真世界、真啟動檔或登入登錄。

獨立 Astra 審查發現改埠檢舊埠、中文世界名解析、全域草稿遺失三項實質問題，均補回歸；前兩項由獨立 TEMP probe 再驗通過。草稿再補加入入口、快捷鍵及外殼退出，主線真 UI 驗證通過。最後追加的獨立覆核因工作區額度中止，未假稱該回合通過；最後增量由主代理檢查與測試。另補玩家父目錄 junction、舊非 UTF-8 讀埠與啟動排程介面保護。

## 限制

- 交換中斷用 panic／持久 marker 模擬，沒有真切電源；跨程序中斷會阻擋，需按保留快照及交易資料人工救援，未自動跨程序修復。
- 快照及成功交易 previous 保留、額外占空間，需人工整理。改伺服器路徑或 level-name 後不自動跨身分還原舊備份。
- 編輯器只收 UTF-8／Java Unicode escapes，非 UTF-8 舊檔拒絕覆寫；既有讀取仍保留系統編碼相容。一般 revision／路徑重檢不保證抵抗惡意外部程序的極短競速窗口。
- 真 OS 登入啟動、原生匣選單滑鼠點擊、正式 NSIS 安裝／更新／解除、兩台真服 6 GB 長時間遊玩仍未驗。
- 沒有 push／Release／Cloudflare／私人 AI-Agent 備份，未讀更新器私鑰。
