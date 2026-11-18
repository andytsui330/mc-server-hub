# 第11步：Windows安裝包、文件與CI證據

唯一進度在AI-Agent的2.0成熟軟體計畫書。本檔記實作與證據，不以設定檔存在就宣稱安裝完成。Andy已明確確認第10步通過；第11步NSIS候選包已編譯，目前帳號TEMP靜默測試及修正版8.3短路徑GUI安裝／解除回歸均通過。舊包GUI曾留捷徑，修正版已確認由解除程式自行移除；乾淨標準帳號已完成下述6組驗證，測試帳號已移除且3份資料副本逐檔核對保留；本輪兩台既有真服短測與本人登入已通過；TEMP解除立即核對異常保留，後續產品偏好清理另獲授權且最後基線核對通過，第11步本機驗收完成。第12步尚未開始，沒有對外發佈。

## 本機已備妥

- README改成2.0用途與操作，頂端固定Windows安裝檔入口 `MCServerHub-Setup-Windows.exe`，明示尚未發佈；不再引導下載2.0可攜ZIP。使用說明、升級／解除、網路與更新行為已對照實作修正，截圖使用已通過的隔離假服畫面。
- CHANGELOG以`2.0.0（未發佈）`單一段落維護「升級說明／新增功能／改善／修正／開發者備註」。包裝腳本從同一段擷取RELEASE-NOTES，不另外手寫更新說明，也不產生未簽的latest.json。
- NSIS限定currentUser，繁中／英文，沿用鎖定Tauri的模板並做局部修改：不建立桌面捷徑、不提供刪appdata選項、完成後開啟程式預設未勾。保留原安裝／升級流程及開始功能表入口；這些先經模板與設定檢查，後續TEMP實測見下節。
- 安裝資源附LICENSE、THIRD_PARTY_NOTICES.txt、使用說明、逐套件授權全文與CycloneDX1.6 SBOM。299個Rust建置／執行相依、4個npm正式相依與2個內嵌字型，主應用去重後304個components。它是pre-build依賴清單，不是二進位可達性或安全證明。
- 公開repo專用CI及手動Windows候選包workflow已在本機準備，Actions釘完整commit SHA。沒有push、工作流觸發、權限設定修改、Release或Cloudflare操作。Mac打包流程留後續階段，不把Windows設定冒稱雙平台驗證。

## 指令與防護

在repo根目錄執行，需使用已裝的Python3.11+、Cargo與Node：

```powershell
py -3 -B scripts/release_support.py prepare
py -3 -B -m unittest discover -s tests -p test_release_support.py -v
py -3 -B scripts/release_support.py check-tools
```

prepare只讀鎖定的本機相依，不下載工具。產物在`target/release-support/`：SBOM.cdx.json、THIRD_PARTY_LICENSES.txt、RELEASE-NOTES.md；缺完整授權或版本不一致就停止。來源補件與hash見`packaging/license-sources.json`及[UPSTREAM](../packaging/UPSTREAM.md)。

```powershell
py -3 -B scripts/release_support.py build-windows-test
```

這個入口預設先查NSIS快取與hash；缺工具就停止，**不呼叫Tauri builder**。已存在但不完整／被改動的快取一律保留，另行確認修復，不讓Tauri隱式遞迴刪除重建。只有明確批准取得工具後，才可用`--allow-tool-download`；人工刪檔、安裝成品或真服測試仍需各自授權。

本機候選包使用`tauri.local-test.conf.json`，關閉此次打包的updater artifact及WebView2安裝，並用`--no-sign`。正式config的`createUpdaterArtifacts=true`、`requireSignedVersion=true`與公鑰均保留；這不是停用正式更新驗簽。未讀更新器私鑰、未產生簽章或正式更新資訊。

成功打出候選包後，會另建`target/windows-candidate-<唯一編號>/`，複製成固定檔名並附SHA256SUMS及BUILD-STATUS。既有候選目錄不覆寫、不清除。這一步不執行安裝器；本機候選包只供隔離測試，不能拿去當正式版。

## 工具取得與授權範圍

已核對安裝的Tauri CLI2.12.4二進位字串與[同版官方打包程式](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.4/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs)：需要NSIS3.13 ZIP（2,363,925 bytes）和nsis-tauri-utils0.5.3 DLL（34,304 bytes），約2.4MB下載；解壓到`%LOCALAPPDATA%\tauri\NSIS\`。這台起始沒有該快取或NSIS程式。

來源為Tauri官方`binary-releases`與`nsis-tauri-utils` GitHub release，Tauri會核對內建hash；預檢額外核對NSIS.exe及helper DLL。工具本身不需改PATH或系統Java。**Andy已明確同意取得這兩項工具，本輪已下載、核hash並實際編譯。** `makensis /VERSION`回報v3.13，預檢problems為空；裝機總清單已登記，env-inventory回報未登記0。工具取得階段未安裝成品、WebView2、Java或其他工具；後續另經授權的成品測試見下節。

## 本輪候選包結果

- `target/windows-candidate-1794758204271433900/MCServerHub-Setup-Windows.exe`：4,436,677 bytes（4.23 MiB），SHA-256 `4ec20dea4b11b4de56df423c7fd00f751a1935e0dbff730b7c0053957870885e`。
- 同目錄附SHA256SUMS、BUILD-STATUS、更新說明、SBOM與完整授權附件；7份檔案全部逐byte核對SHA-256，固定檔名候選包與Tauri原始輸出相同。
- 已用既有Windows SDK的mt.exe讀取安裝包內嵌manifest，requestedExecutionLevel為asInvoker；PE憑證表為空，SDK signtool回報No signature found。這是未簽測試包，不是正式平台更新，不產生.sig或latest.json；未讀私鑰。
- 實際編譯的`target/release/nsis/x64/installer.nsi`已核對currentUser、法律資源、不建立桌面捷徑及不刪appdata。local-test的WebView2 skip在生成NSIS中呈空值，三個下載／安裝分支均不啟用；沒有取得或安裝WebView2。
- 證據為`target/step11-nsis-build.log`、`target/step11-inspection.json`、`target/step11-installer.manifest`與`target/step11-signature-check.log`。僅讀檔與使用編譯器／SDK工具；**此靜態檢查階段尚未執行安裝器或解除程式**；後續TEMP實測見下節，靜態asInvoker不冒充真機UAC測試。

安裝／解除測試已由Andy明確批准指定SHA及TEMP範圍，計畫見`target/step11-install-test-plan.json`（保留最初準備時的permission文字，另記後續批准）。只有新TEMP假服、獨立data及webview；安裝前重查相關HKCU、開始功能表捷徑及同名Run值無既有項。這台未裝Windows Sandbox，目前帳號隔離測試不能冒稱乾淨Windows帳號已驗證。

## TEMP安裝與解除實測（2026-11-16）

使用同一份SHA-256為`4ec20dea…7870885e`的候選包，沒有重打包或改安裝器。`tests/nsis-install.py`以明確`--execute`執行；沒有經由工具層放寬權限。安裝／解除使用`/S`，因此不是安裝精靈GUI逐頁點擊驗證。

- 目前Windows帳號的測試程序未提升管理員權限；安裝與解除exit code均0，程式只進預備TEMP的app資料夾。開始功能表捷徑指向該exe，沒有桌面捷徑；五份授權／使用說明／SBOM資源雜湊全符，相關HKLM登記未變。
- 安裝目錄中的exe實際啟動於獨立MCHUB_DATA_DIR／WebView資料夾，僅有指定假服；經開機前檢查後真Java假服到Done，主控台送指令並收到回覆，確認後正常stop。字級設定為16px、重啟仍保留，未自動開服或查正式更新。5項真WebView2流程通過。
- 正常退出安裝版後，先存測試data與假服全檔SHA-256，再解除安裝。安裝目錄、程式、解除程式及開始功能表捷徑均消失；測試設定、原啟動檔與假世界全檔雜湊保持一致。正式APPDATA三檔存在狀態與雜湊前後一致；沒有剪貼簿、真服或真啟動檔操作。
- 解除後只移除本次建立且內容仍符合安裝快照的兩個HKCU產品鍵，並核對Run值仍不存在。初次測試未保存發行者「父鍵」的原始基線，故保留空父鍵，不猜刪，也不稱整個父鍵恢復原狀。後續測試已改為先記父鍵基線；本輪留下的空父鍵沒有產品值或子鍵。
- 合計3組安裝／保留／清理檢查與5項UI流程通過。獨立唯讀覆核另確認整個TEMP app目錄與同名Run值均無殘留；測試腳本已補強這兩項assert，原實跑結果另以只讀後檢補證，不重複安裝或清理。

初次測試在安裝成功後的exe比對停下：Tauri把內含exe的唯一`__TAURI_BUNDLE_TYPE_VAR_UNK`改成`…_NSS`，打包後把建置目錄exe還原為UNK。實際差異僅3bytes；新增`verify_nsis_payload`要求唯一標記且所有其他bytes相同，任何其他變動或重複標記均拒絕。安裝版原始SHA為`1ce3837570dd1036a81c54abd6a1a9a986acb3e03f5dde2aba9f411ed70165ee`，不能寫成與未包裝exe的原始SHA相同。單元測試先紅後綠；包裝規則現11項通過。

續作先驗證同一journal、同一TEMP及登記的InstallLocation，從已安裝狀態繼續，沒有重裝；第一輪紀錄留在`target/step11-install/first-attempt.json`。主報告`target/step11-install/report.json`保留原通過文案並另附wording_correction，明說「只差NSIS標記」和「只恢復兩個產品鍵，未知父鍵保留」。UI報告與截圖在同目錄`ui/`，流程log為`target/step11-install-resume.log`。測試設定、假世界及證據均保留TEMP，沒有遞迴刪除fixture。

裝機總清單已記錄暫存安裝後解除、成品未保留，工具快取保留；env-inventory未登記0。此結果不涵蓋乾淨Windows帳號、安裝精靈GUI或真服長時間效果。

## GUI精靈補驗與短路徑問題（2026-11-16）

沿用原候選包SHA `4ec20dea…7870885e`及核准TEMP，透過Computer Use實際操作歡迎頁、安裝位置、安裝進度與完成頁；MIT授權頁由Andy本人推進，之後即時讀回確認已到安裝位置。位置仍為含8.3別名的 `C:\Users\ANDY20~1\AppData\Local\Temp\mchub-nsis-test-9lzcbz6h\app`。「執行MC開服平台」預設未勾，按完成後沒有啟動應用程式；重開精靈按取消後視窗關閉。

解除精靈只列指定TEMP程式位置，未提供刪設定／世界選項；顯示解除完成後，app整目錄消失，但開始功能表捷徑殘留。**此GUI路徑未通過，不能沿用先前靜默流程的通過結論。** 先前腳本會先以Path.resolve展開安裝位置，這輪GUI保留8.3字串，因而揭露差異。

已使用生成的原IsShortcutTarget宏做獨立NSIS探測：同一捷徑，短路徑回0、完整路徑回1；GetFullPathName沒有展開8.3，而GetLongPathNameW在目標仍存在時轉換後回1。因原解除流程先刪payload再比對捷徑，修正在Uninstall Section最前面轉換INSTDIR；API失敗或回傳長度超過緩衝區即中止解除，保留原捷徑目標所有權核對。獨立唯讀審查未發現新增具體缺陷，但不代替新安裝包回歸。

本次只在重新確認捷徑確實指向本次TEMP app後，按既有清理授權移除該殘留捷徑；兩個本次產品鍵也在內容仍等於安裝快照時移除。未知基線的空發行者父鍵照舊保留。同名Run值、程式與捷徑均無殘留，測試data／假服所有檔案集合及SHA與GUI前完全一致，正式APPDATA三檔仍符合原指紋。未碰真服、真啟動檔或剪貼簿。

修正版候選包 `target/windows-candidate-1794791107217900500/MCServerHub-Setup-Windows.exe` 已重新完整建置及NSIS編譯：4,433,285 bytes，SHA-256 `b688b38d6e89bd4b16439b7d879d88a5929eb762acfefc93dd7132c411ffb5c1`，7份附件檢查碼全符。舊包保留，新包沒有安裝、解除或GUI回歸；原授權明確指定舊包，須取得修正版候選包同範圍安裝／解除同意。沒有新增工具、帳號或永久PATH設定；首次重建因當前程序未載入Cargo PATH而停止，僅補當次程序的既有Cargo路徑後成功重建。

證據在`target/step11-gui/`：report.json、before.json、installed.json、01～07頁面截圖、shortcut-probe及normalization-probe原碼／結果、rebuild.log。原報告保留，GUI報告明記shortcut_removed_by_uninstaller=false與人工範圍清理，不把後檢不存在寫成解除程式自行移除。GUI仍待新包完成短路徑安裝→解除→捷徑自行消失；乾淨帳號與其他真機限制維持未驗。

## 修正版GUI短路徑回歸通過（2026-11-16）

Andy明確同意修正版同TEMP範圍安裝／解除後，使用SHA-256 `b688b38d6e89bd4b16439b7d879d88a5929eb762acfefc93dd7132c411ffb5c1` 原包，沒有重打包；開頭核對HEAD `af488f1`及乾淨工作區。安裝前確認app、兩個產品鍵、同名Run值及捷徑不存在，另保存完整測試檔案集合／SHA、正式APPDATA三檔、發行者父鍵及指定HKLM基線。

透過真GUI完成歡迎、相同MIT授權、位置、安裝與完成頁；沿用此前本人已確認的相同授權，沒有變更條款。位置頁與實際InstallLocation都保留 `ANDY20~1`，沒有先以Path.resolve避開短路徑。已核安裝payload唯一UNK→NSS差異、5份法律／說明／SBOM資源及捷徑target，相關HKLM未變。完成頁執行程式預設未勾；按完成後沒有平台程序。

真GUI解除顯示完成後，先做assert確認app整目錄、開始功能表捷徑與解除登記均已由解除程式移除，**沒有人工補刪捷徑**。之後只在偏好產品鍵仍完全符合本次安裝快照、無子鍵時清理該鍵；父鍵前後完全符合本輪基線，並保留其原有空容器。同名Run值仍無、相關HKLM未變；測試data及假服所有檔案集合與SHA完全相同，正式APPDATA指紋未變。

獨立唯讀覆核另外核對候選包hash、兩view短路徑InstallLocation、現場app／捷徑／產品鍵不存在、9個測試檔案集合與SHA及正式APPDATA，結果一致；此覆核未親自觀察解除瞬間，與操作紀錄分開陳述。

共6組核對通過；證據 `target/step11-gui-fixed/report.json`、before.json、installed.json、after.json與01～07七張GUI截圖。原 `target/step11-gui/` 失敗報告及探測證據原樣保留，不覆寫成通過。這輪補驗沒有再次開假服，先前安裝版5項WebView2流程仍以原實跑證據為準。

此次只完成目前帳號、指定TEMP下的GUI短路徑回歸，不代表乾淨Windows帳號、真JDK安裝、第三方模組相容、真OS登入啟動、原生系統匣選單滑鼠或兩台真服6GB長測已驗。真服未動；沒有新增帳號、工具、剪貼簿操作或推送，第11步整體仍未完成。

## 乾淨本機標準帳號驗證（2026-11-16）

Andy本人建立本機標準使用者MCHubTest；預檢確認其在Users、不在Administrators，無先前登入紀錄或profile。以Windows內建runas /profile載入新帳號，密碼由本人直接輸入Windows命令視窗，不進對話、日誌或檔案，未使用savecred。實際測試程序再次核對固定SID、非管理員token、新profile與APPDATA，並確認新HKCU無本產品、捷徑或正式設定。

使用同一份修正版SHA-256 `b688b38d…1ffb5c1`，在該帳號自己的全新TEMP建立獨立MCHUB_DATA_DIR及WebView資料夾。只預備空servers、關閉更新檢查／登入啟動／自動開服的測試設定，以及文字假世界；沒有真伺服器或Java執行。內建cmd／PowerShell入口不安裝工具、不改ExecutionPolicy或ACL；候選包GUI由本人完成安裝，後續首次空白UI、16px保存、退出重啟後仍16px與GUI解除由Computer Use核對。

6組檢查通過：實際新標準帳號與初始空狀態、同候選包exe／5份資源／捷徑、首次設定保存、重啟保留、解除程式自行移除app／捷徑／解除登記，以及測試設定／假世界檔案集合和SHA保留。只有內容仍等於安裝快照且無子鍵的本次偏好產品鍵被清理，沒有人工補刪捷徑。同名Run值與指定HKLM未變，新帳號正式APPDATA維持無設定；Andy原帳號正式設定指紋前後也一致。原不存在的發行者父鍵留下空容器，記錄parent_after，不冒稱整個HKCU完全還原。

中途測試入口停在首次退出後：PowerShell5.1 ConvertFrom-Json無法接受登錄預設值的空字串名稱。保留first-attempt.json與failure-first-exit.txt，改將快照名稱編碼為@default，並驗證JSON完整round-trip；在同SID、同TEMP、相同已安裝程式與已保存16px設定的記錄階段續跑，未重裝，也未重新起真服。這是測試入口解析問題，不能寫成產品曾無法重啟或沒有測試錯誤。

證據在`target/step11-clean-account/`：report.json、owner-before／owner-after.json、原失敗紀錄、run／resume.cmd及透明PowerShell片段、runner-integrity.json、首次空白UI／保存／重啟及解除截圖。該目錄為本機證據；含帳號SID與機器路徑的內容不加入公開repo。這輪沒有重跑無關核心測試，也不沿用11項包裝單元測試冒充新帳號實測。

依Andy最新指示，驗證後要移除MCHubTest帳號，但保留profile與測試資料。已準備cleanup-account.cmd：需本人手動管理員執行，先確認無測試帳號程序、固定SID與測試成功，核並複製data完整集合與假世界共3檔至preserved-data，再只Remove-LocalUser該SID，不刪profile或資料。尚未收到清理完成證據前，不記帳號已移除。當前原帳號工具無法讀取新profile私人檔案，未為此放寬ACL。

runas /profile是新本機帳號／新設定檔測試，不等於登出後的完整Windows桌面登入、全新OS或無WebView2電腦；真OS登入自動啟動、JDK安裝、第三方模組遊戲相容、原生系統匣選單滑鼠與兩台真服6GB長測仍未驗。沒有真服、正式設定／真啟動檔／剪貼簿或任何推送操作；第11步整體仍未完成，第12步未開始。

## 測試帳號清理與資料保留核對完成（2026-11-16）

Andy本人以管理員執行指定清理入口後，`target/step11-clean-account/cleanup-report.json`記錄phase=complete、account_removed=true、profile_deleted=false。另行即時查詢確認MCHubTest已不存在，Users群組也無原測試SID。清理入口先確認沒有該帳號的測試程序，再只Remove-LocalUser原固定SID；沒有刪除profile或測試資料。

已直接讀取`target/step11-clean-account/preserved-data/`三份副本並逐一重算SHA-256，實際檔案集合完整符合原runtime報告的data_after_uninstall與world_before：

- `data/config.json`：`38e9e9851ba5b9497d9358031e0567c5c34656e181991fbbc8cf9cca949f704e`
- `data/reports/session.json`：`fa896018af42fca4630e6ce0435edba8da709d863874b01d64734faddea0f1fc`
- `fake-world/level.dat`：`e9c0a5f79fc2a3959b6e94128d1571976f425026219691d0a041492c0062b5f3`

副本設定仍為16px、空伺服器清單且更新檢查關閉。管理員清理程式在刪帳號後再次核對原profile內三檔與副本的SHA均一致；目前非管理員Andy對原profile仍無直接讀權，因此這項原檔保留依管理員清理證據陳述，與本次可讀副本的獨立雜湊核對分開。未改ACL。Andy正式APPDATA三檔指紋也再次符合原基線。

此前一次未取得管理員權限的清理被guard擋下，`cleanup-failure.txt`保留為歷史，沒有覆寫或刪除；成功依新COMPLETE報告、現場帳號不存在及副本實檔核對判定。核對彙整另留`cleanup-verification.json`，不把失敗紀錄冒寫成全程無錯。

獨立唯讀覆核另以Get-LocalUser、net user與群組查詢確認帳號不存在，實際重算3份副本的完整集合與SHA，並核Andy正式設定，結果一致；未嘗試改ACL讀取原profile。

第11步本機NSIS、目前帳號GUI／假服、全新標準帳號安裝／設定重啟／解除，以及測試帳號移除與資料保留已有證據。唯一計畫書第九節仍要求Andy以真伺服器開一次；目前明確不准動真服，因此此完成條件維持未驗，第11步不記整體完成、不進第12步。遠端CI、Release、更新資訊與Cloudflare未執行；其他既列真機限制保持不變。

## CI邊界

`.github/workflows/ci.yml`做Windows鎖版建置、Rust測試／clippy、TypeScript及包裝規則檢查；`package-windows.yml`只允許公開repo的main分支手動產生未簽測試安裝包。兩者對私人repo都不執行，沒有contents寫入權限、Release或簽章秘密；手動包裝工作額外使用OIDC／attestations權限，供未來執行時產生[建置及SBOM證明](https://github.com/actions/attest)。這輪沒在GitHub執行，不能稱CI或attestation已通過。

workflow使用JSON形式的合法YAML，方便以現有Python標準庫離線解析，未為此安裝YAML工具。Actions來源版號（皆超過兩週）與SHA查自官方repo：checkout7.0.1、setup-node7.1.0、setup-python7.1.0、upload-artifact7.0.2、attest4.2.2；版本釘於workflow。Node24.18.0、Rust1.99.1、前後端套件鎖版不改。

## 已跑與未跑

本機包裝規則測試11項通過，包含缺工具時builder不會被呼叫、既有不完整快取保留、安裝模板資料保護、版本擷取、檢查碼、授權缺件停止、SBOM關聯完整、NSIS payload標記及CI公開repo／權限／SHA防護。SBOM已做結構、唯一ref與依賴指向檢查；未另安裝完整JSON Schema驗證器，不聲稱通過該工具。正式建置、候選包與隔離安裝結果收錄在`target/step11-index.json`，以實際結果為準。

獨立唯讀審查指出全新clone沒有`target/release-support`，原開發指令會缺資源；README已補Python3.11+、鎖版cargo fetch及prepare前置步驟。其餘NSIS資料保留、無桌面建立、工具cache預檢、未簽流程不讀私鑰與CI防護，審查未發現具體新增缺陷。這不是遠端CI或安裝器的執行證據。

完整NSIS編譯及先前目前帳號靜默流程已通過；GUI補驗揭露的8.3路徑捷徑殘留已由修正版完成同TEMP真GUI回歸，捷徑由解除程式自行移除。乾淨本機標準帳號亦完成6組新profile驗證；測試帳號已由本人管理員清理並經即時帳號清單與3份資料副本核對，真服仍未驗。真服測試需先取得當次同意：先備份世界並逐檔核對，保留原啟動檔與設定，正常開到Done、關機，再與私人計畫的ERROR／WARN基準比對；不把服名或私人路徑寫進公開repo。

真JDK安裝、第三方模組遊戲相容、真OS登入啟動、原生系統匣選單滑鼠點擊、兩台真服6GB長時間效果仍未驗。本輪安裝／解除及清理僅在明確同意的範圍，沒有新增其他工具，沒有真服／正式設定／真啟動檔／剪貼簿寫入或對外推送。

官方參考：[Windows安裝器](https://v2.tauri.app/distribute/windows-installer/)、[更新與簽章](https://v2.tauri.app/plugin/updater/)、[CycloneDX1.6](https://cyclonedx.org/docs/1.6/json/)。第三方授權附件與MPL來源提供方式，正式發佈前仍需人工核對。


## 兩台既有真服短測與解除後核對（2026-11-16）

Andy明確批准同SHA修正版候選包新TEMP安裝、兩服離線完整保全／逐檔SHA、依序開關服與可能的既有外部通知；後續再明確要求本人登入一併測試。私人路徑、玩家識別及原始log只留本機保全目錄，不加入公開repo。

完整兩服備份共3,885檔，含世界、模組、設定、libraries及舊備份，檔案集合／大小／SHA與來源穩定皆已核。安裝版以獨立data／WebView只登記兩服，更新、登入啟動、自動開服、重開及平台備份／排程均關閉。第一輪因玩家加入正常停服而中止，原報告保留；後續本人登入短測為另份證據，不覆寫原輪。兩服各Done、6GB、至少5分鐘觀察，本人確認體感正常並離線，list零玩家回覆後正常stop，程序退出與兩埠釋放已現場核對。

ERROR分別11與1，訊息及例外原因均屬歷史既有內容；WARN分別117與20。第一服增量逐項對應既有登入模組衝突、本人快速飛行／瞬移與舊世界高度資料警告；高度警告亦在保全的歷史log出現，未修改世界或模組。兩服均少掉舊12GB配置警告。這是短時本人遊戲及平台開關服驗證，不是完整第三方模組相容或6GB長期負載證明。

關機後全服manifest無檔案消失，原啟動檔、主程式、mods及libraries未變；世界、玩家資料、快取與log存檔留存。server.properties及模組設定的值未變，僅註記重寫；不還原、不回滾本人進度。

平台正常退出後，解除程式exit0，但緊接的app／捷徑／桌面不存在聯合檢查失敗。依「異常停止」要求，沒有重跑解除或人工補刪。稍後唯讀後檢已確認app、捷徑、桌面及解除登記均消失，正式APPDATA三檔、Run、HKLM與測試data完整SHA仍符基線；時間差是可能原因，尚未證明產品缺陷。初始失敗與後檢分開保留，不改寫全程通過。目前僅本次產品偏好登記仍在，完全符合安裝快照且無子鍵，待Andy確認續清；第11步整體仍未完成。

本輪無新增工具或帳號、無強制結束、無還原或刪測試資料，未碰剪貼簿、遠端CI、Release、Cloudflare或任何推送。裝機清單及本機盤點已同步，未登記0；真JDK安裝、全新OS／完整桌面登入、OS登入啟動、原生系統匣滑鼠操作及6GB長測仍未驗，第12步未開始。


## 第11步收尾核對完成（2026-11-16）

Andy另明確授權只清除本次產品偏好登記並作最後核對。執行前重核兩個登錄view的該產品鍵均完全符合安裝快照且無子鍵，之後僅刪該產品鍵，不刪發行者父鍵，不重跑解除或開服，不刪資料。清後產品鍵、發行者父鍵、Run與指定HKLM完整結構均符合原基線；正式APPDATA三檔與測試data完整集合／SHA一致，程式／捷徑／桌面／解除登記無殘留。

另重新完整核對兩服的檔案集合、大小、時間與SHA，均與正常關機後manifest全符。測前完整整服備份已於測後再次全檔核對保留；玩家進度及所有原始失敗／後檢證據留存，沒有還原。裝機盤點未登記0／新增0／消失0。

第11步的本機安裝／新標準帳號與兩服本人短時遊戲、Done／零玩家list／正常stop／存檔及授權收尾核對條件已滿足。初始解除立即assert仍是失敗，稍後後檢與另經授權清理的結果分開陳述，並未將其改寫成全程無錯；時間差仍僅列可能原因。這輪不改產品程式碼或重評既有設計／鎖版。

第12步尚未開始；真JDK安裝、全新OS／完整桌面登入、真OS登入啟動、原生系統匣滑鼠操作、完整第三方模組相容與兩服6GB長測仍未驗。遠端CI、供應鏈證明、正式更新簽章、Release、Cloudflare與任何推送均未執行；沒有新增工具／帳號、強制結束或剪貼簿操作。
