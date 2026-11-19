# 第12步：Mac 驗收與發佈（已結案）

2.0.0 已於 2026-11-19 正式發佈：[Release v2.0.0](https://github.com/andytsui330/mc-server-hub/releases/tag/v2.0.0)。本檔保留第12步已驗範圍，並照實列出發佈時仍未驗的項目；後續版本的變更見 [CHANGELOG](../CHANGELOG.md)。私人測試路徑、原始 log、設定與來源交接 bundle 不屬於公開附件。

## 已有證據

| 項目 | 範圍與限制 |
| :-- | :-- |
| Windows 重核 | Mac 修正接回後，Rust workspace 111 passed／6 ignored、clippy、前端建置與包裝規則 11 項通過。 |
| Windows 開機前檢查 | 既有清單的隔離副本，兩台主頁與總覽共 3 個入口皆先顯示檢查頁；未按確認開機，不擴稱正式模式已驗。 |
| Windows 更新檢查失敗 | 正式設定模式，啟動檢查及兩次手動檢查都由本機拒絕代理擋下，未連 GitHub；錯誤提示、可重試、兩服關閉與正常退出通過。不是更新安裝測試。 |
| Windows 正式下載版 | 從 Release 下載的安裝檔（SHA256 與 `SHA256SUMS.txt` 相符、`gh attestation verify` 通過）實走下載、安裝、開啟；另以同一安裝檔做過隔離安裝與解除安裝。 |
| Mac Intel | macOS 15.8.1；原生拖放／選夾、系統匣、快捷鍵、設定保存、單一實例、執行中退出保護及指定尺寸已有證據。原版 1.21.11 真實建服到 Done、list 與正常 stop 代碼 0 通過。 |
| Mac universal | DMG 內程式含 x86_64 與 arm64，授權資源及 SBOM 已核；ad-hoc 簽章核對通過。這不能代替 arm64 實機驗收。 |
| 發佈 | Release 11 個檔案下載回來 SHA 全部相符；README 兩個下載按鈕與 `latest.json` 皆可下載。 |

Windows 的本機假更新測試另有無更新、HTTP 錯誤、簽章錯誤、檔案遭改、版本不符、下載逾時、壞安裝檔與測試假安裝器等證據；測試簽章及只寫 TEMP 的假程式不能代替正式更新安裝。

## 發佈時仍未驗（照實標示）

- 缺少 WebView2 的乾淨 Windows 電腦自動補裝。
- 從 2.0.0 實際更新到下一版（要等下一版發佈才能走）。
- Mac：網路下載後的 Gatekeeper 首次開啟、OS 登入啟動、正式更新的下載／驗簽／替換。Mac 版的自動更新檔尚未提供，提供前請到 Releases 手動下載新版。
- Apple 晶片實機（暫緩，不能以 Intel 結果代替）。
- 測試更新頻道：目前沒有測試版 Release，選了會顯示連不到更新服務。

## 已知差異與注意事項

- Windows 安裝檔沒有程式碼簽章；Mac 為 ad-hoc 簽章、未經 Apple 公證。
- Mac DMG 由本機建置，沒有 GitHub 供應鏈證明，請以 SHA256 核對。
- Apple SDK 衍生套件的授權補件疑慮見 [UPSTREAM](../packaging/UPSTREAM.md)。
- 1.x 平台跳出的新版提示寫「下載壓縮檔、蓋掉 exe」，不適用 2.0；請改從 Releases 下載安裝檔（Windows）或 DMG（Mac）。
