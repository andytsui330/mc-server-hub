# 第10步設計審查與驗證

唯一進度來源仍是 AI-Agent 專案 `2026-11-14_2.0成熟軟體計畫書.md`「目前進度」。本檔只記審查、修正與證據；第10步須由 Andy 看圖點頭，不能以測試通過替代確認，也未進入第11步。

## 沿用的設計

用途是屋主在本機開關服、看在線與主控台、管理設定；第一眼先辨認哪台執行中，再找到開關機。沿用 `design/設計簡報-1.1.md`、已核准像素圖示、字型、深色底階與冰藍焦點，保留淺色主題。Impeccable 以既有操作介面的 audit／polish 審查，ui-ux-pro-max補操作與無障礙準則；沒有重新抽風格、換框架或改鎖版。

## 實際修正

| 問題與影響 | 修正及驗證 |
| --- | --- |
| 平台設定未儲存就切頁，草稿直接消失 | 設定頁接入現有 dirty／busy 保護；總覽、伺服器、新增、加入、關閉、Ctrl K、原生關窗均驗取消後保留草稿 |
| 自動重開還沒讀到既有設定，就可修改畫面上的預設值 | 讀取完成前鎖欄位與儲存；讀取中提示、錯誤與重試入口；以延遲及失敗回應驗證，不寫掉未知設定 |
| P2查詢時仍可返回主控台，回應與錯誤被丟掉 | 返回按鈕及處理函式同樣檢查子頁 busy；分類與全域導覽保護保留 |
| 加入／建立請求送出中，取消看似成功但工作仍繼續 | 加入對話框與建服精靈停用欄位、取消與 Esc；精靈測試攔截 create 回應，沒有建立或下載真伺服器 |
| 指令表單取消／Esc後焦點落空；Modal busy切換重跑焦點生命週期 | 表單收起後回到原指令格；Modal用ref讀最新busy，保留一次開關的觸發焦點。實測取消、Esc、Ctrl K及詳細資訊返回 |
| 舊欄位樣式蓋掉全域鍵盤焦點框 | 針對對話框／指令欄位補2px冰藍focus-visible，複合指令輸入列補2px底線；實測可見實線框 |
| 尚未取得在線人數，畫面卻稱「目前沒人／0人在線」 | null資料顯示「在線名單待確認／在線人數待確認」；只有確認online為0才寫沒人。真WebView以明確標記的未知回應fixture驗證 |
| 重開／平台設定草稿提示誤稱遊戲設定；設定頁仍指向舊入口 | 草稿提示改成目前頁面尚未儲存的設定；補清楚管理伺服器與詳細資訊的分工，不變更產品能力 |

## 驗證與證據

- 正式 `tauri build --no-bundle` 通過，包含 TypeScript strict與Vite建置。未打NSIS安裝包、未改Rust核心或相依鎖版；未重做第0～9步。
- `tests/design-ui.cjs` 真WebView2操作16項通過；正式APPDATA三檔存在狀態與SHA-256前後一致，沒有操作剪貼簿。新增兩台假服與全部設定只在當次全新TEMP `MCHUB_DATA_DIR`。
- 最終畫面證據 `target/step10-ui/delivery/`：24種畫面×4尺寸，共96張PNG。1360×900、1366×768、1280×650及最小桌面900×600；含主畫面、總覽、管理七頁、設定、深淺色、20px字級、精靈、詳細資訊、圖示、指令搜尋與確認框。全部尺寸無整頁／main水平溢出，主控台至少160px。
- 全套行為綠燈證據 `target/step10-ui/behavior/report.json`；這次以 `MCHUB_DESIGN_SKIP_CAPTURES=1` 復驗，不重新產圖。delivery與behavior使用相同正式exe，證據索引及exe SHA-256在 `target/step10-index.json`。
- 既有P1與成熟配備UI回歸結果見證據索引。獨立代理做唯讀程式與指定尺寸截圖覆核，追加兩項問題已修；最終三項增量覆核無新增缺陷。這不冒充該代理親自操作過UI。

### 三尺寸同畫面

以下圖片為本機證據，`target/`不進Git，clone後須依下方命令重產。

1360×900：

![主畫面1360×900](../target/step10-ui/delivery/running-1360x900.png)

1366×768：

![主畫面1366×768](../target/step10-ui/delivery/running-1366x768.png)

1280×650：

![主畫面1280×650](../target/step10-ui/delivery/running-1280x650.png)

900×600最小桌面：[主畫面](../target/step10-ui/delivery/running-900x600.png)、[20px遊戲設定](../target/step10-ui/delivery/font20-properties-900x600.png)。精靈及長管理表單在矮視窗採內部捲動；不增加手機版。

### Impeccable與工具邊界

全 `app/src` 靜態detector有3個warning：`fonts.css`既定Inter／Geist各1個，以及`style.css`危險指令2px紅色標記1個。三者皆是已核准設計，保留並記為適配例外，不稱全樹零警告。detector只提供靜態線索；視覺、焦點與操作另以實際畫面驗證，沒有宣稱完成讀屏軟體實機驗證或FPS量測。

測試中的延遲／錯誤只攔本輪WebView的fetch回應；Tauri invoke是不可寫屬性，早期覆寫invoke的嘗試未生效，不作通過證據。對話框CSS仍為2px，這台WebView回報計算值1.6px；原本要求整數2的測試曾誤判，已改驗可見實線至少1.5px並保留原始記錄。delivery的單一焦點門檻失敗不是被改寫成綠燈，而由behavior完整16項成功復驗。

另一次測試在開圖示時失去WebView/CDP目標，原生視窗仍回應，正常關窗後新TEMP復驗可完成；沒有當機報告可確認原因，不把它寫成已知根因或全平台無此風險。

## 重跑

使用已裝Playwright路徑設定 `MCHUB_PLAYWRIGHT`；不執行安裝或下載瀏覽器。正式exe須先在`app/`執行 `npm run tauri -- build --no-bundle`（Windows終端機須已有Cargo路徑）。

```powershell
# repo根目錄；MCHUB_PLAYWRIGHT指向當台既有套件
node tests/design-ui.cjs
```

預設會重新建立TEMP、重產四尺寸截圖、測完正常停假服並退出，不清掉舊TEMP。`MCHUB_DESIGN_PHASE`可另選證據子目錄；`MCHUB_DESIGN_SKIP_CAPTURES=1`只跑同一組行為；`MCHUB_DESIGN_HOLD=1`是本機人工審查用，需在對應證據目錄寫`continue`才會收尾，正常重跑不要設定。

## 沿用限制

未安裝真JDK、未用第三方模組開遊戲驗相容、未驗真OS登入啟動／原生系統匣選單滑鼠點擊／正式NSIS／兩台真服6GB長時間效果。未知模組相容與中斷交易救援仍須人工確認。沒有推送（含私人AI-Agent備份）、Release、Cloudflare、工具安裝、人工刪檔或讀更新器私鑰。第10步等待Andy確認，第11步未開始。
