# 安裝模板與授權來源

NSIS模板沿用已鎖 Tauri CLI 2.12.4 的官方 `installer.nsi`（MIT OR Apache-2.0），本地檔 `app/src-tauri/windows/installer.nsi`。原始碼未執行；只把模板當資料取回並依下列差異修改。

- 來源：https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v2.12.4/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi
- 原始檔SHA-256：d00089d3b1a2c4a30d7b6e098079100cb8f25e980f84d4515aca4597b95e967d
- 保留上游安裝／升級／解除流程、開始功能表及目前使用者模式；刪除桌面捷徑建立選項與靜默建立，保留程式自動啟動為未勾選。
- 移除解除時刪app data的勾選與刪除分支，設定和世界不交由解除程式清除。解除程式仍須移除平台自身安裝檔與登記；目前帳號TEMP靜默與GUI解除已實測，完整限制見docs/step11-packaging.md。
- GUI實測發現8.3短路徑安裝的開始功能表捷徑殘留：解除Section在刪檔前用GetLongPathNameW轉成完整路徑，才交原IsShortcutTarget核對；轉換失敗即停止解除，不放寬捷徑所有權比對。
- 升級Tauri時需重新比對，不自動跟隨上游模板。模板授權見vendor-licenses/tauri-plugin/LICENSE-MIT。

`license-sources.json`記錄registry封裝未附完整授權文字的套件補件來源與SHA-256。alloc-stdlib、tauri-plugin、webview2-com家族從該版本`.cargo_vcs_info.json`的commit取官方repo全文；selectors 0.38.0的repo與crate均未附完整MPL全文，因此補Mozilla官方MPL2.0正文，套件來源仍保留crate版本下載連結。沒有以通用MIT文字蓋掉第三方作者聲明。

macOS（2026-11-16，第12步）：objc2家族15個crate（block2、dispatch2、objc2、objc2-encode、objc2-exception-helper及objc2-*框架crate）在crate與官方repo都只有根目錄`LICENSE.md`，該檔只聲明授權組合（objc2／block2／objc2-foundation／objc2-encode為MIT，其餘Zlib OR Apache-2.0 OR MIT）並提及衍生自Apple SDK，**未附授權條文全文、也沒有著作權人行**。因此原樣收錄各版本`.cargo_vcs_info.json` commit的`LICENSE.md`（5個commit內容SHA相同），另補SPDX license-list-data v3.29.0（2026-09-16）的MIT／Zlib／Apache-2.0官方條文；MIT條文保留官方範本的著作權人占位，未自行填寫或編造作者聲明。維護者已於2026-11-19決定以此補件方式發佈（2.0.0起）：上游所述Apple SDK衍生授權疑慮仍照實保留在此；若上游日後補上授權全文或著作權人行，後續版本會跟進更新。

`.gitattributes`只對`packaging/vendor-licenses/**`關閉換行轉換，避免Windows自動改行尾造成來源hash失配。已用全新TEMP、`core.autocrlf=true`從index匯出6份補件，SHA-256全數相符；未更動全域Git設定。

`scripts/release_support.py prepare`從鎖定的cargo metadata、npm production lock及既有內嵌字型聲明產生逐套件授權全文和CycloneDX1.6依賴清單；找不到全文就停止。清單涵蓋Rust建置與執行相依、npm正式相依，不宣稱每項都能在最終二進位中找到；`metadata.lifecycles`標為pre-build。

套件MPL來源以未修改的crate下載網址提供；正式對外發佈前仍需人工確認授權附件、原始碼取得方式與實際交付內容一致。
