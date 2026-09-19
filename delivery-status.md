# 開發狀態

## Current Phase

2026-09-19：開發機已用測試憑證簽署的 WIA 套件安裝本驅動，Windows 的 WIA 服務首次成功載入、鎖定並透過既有 WIA 用戶端（WIA automation）完成灰階與彩色 75 dpi 全平台掃描，屬性驗證也經服務拒絕無效 dpi。修正過程確認服務要求 COM aggregation、傳 STI 版本 3、port name 為 AUTO、相容模式項目不能查型別。WinRT `Windows.Devices.Scanners` 桌面程序可列舉、連線並完成掃描（Windows 掃描 App 使用的 API）；Windows 掃描 App 本身在 AppContainer 內連線失敗，服務端呼叫序列與成功的桌面 WinRT 完全相同，原因尚未取得客戶端證據。取消、拔插、第二台電腦與解除安裝驗收仍未完成。精確階段取消對照已加入測試建置，第一塊影像後取消並以同一 USB session 重掃通過；首塊前取消的復原缺口仍由 03 追蹤。

## Stage Objective

先在已連接的 WorkCentre 3119 取得可驗證的真實掃描影像，再完成供既有掃描軟體使用的 Windows 驅動整合。

## Active Workstreams

- 裝置診斷已實作並實跑，尚缺拔線競態、多台裝置及權限拒絕的實機驗證。
- Rust 已完成 Gray75、RGB75、RGB300、Gray600、RGB600 空平台傳輸。能力與影像來自同一個獨占 session。
- 使用者確認是歷史官方驅動經常在 600 dpi 中途失敗，尤其彩色，其他解析度較少。本輪新核心另重現兩次自然 RGB600 逾時，不能認定與歷史故障同因；先前兩次定時中止則是刻意取消。
- 本機 MI_00 綁定、GUID 登錄及介面重啟成功，不需重開機，父裝置與 MI_01 符合原始備份；復原未執行。
- 使用者回報 Windows 掃描搭配原廠驅動時彩色像過曝、灰階偏淡。已建立 07 的逐段比對與驗收條件，尚未重現或修復。

## Milestones

| id | target | owner | status | verification_signal |
| --- | --- | --- | --- | --- |
| init | Rust 專案與接手文件 | 開發者 | done | 五個測試、Clippy、格式檢查、release 建置通過 |
| 01 | 唯讀診斷完整情境 | 開發者 | in_progress | 本機問題碼 28 可重現，硬體異常情境未全部驗證 |
| 02 | 第一張實機掃描 | 開發者 | in_progress | 空平台灰階／彩色及獨立像素比對成功；文件、色彩及精確幾何待驗收 |
| 03 | 取消與復原 | 開發者 | in_progress | 連續工作前 4 次成功，第 5 次 RGB600 自然逾時，清理後 Gray75 成功；20 次驗收與失同步復原未完成 |
| 05 | Windows 掃描與安裝 | 開發者 | in_progress | 開發機測試憑證套件安裝成功，WIA 服務載入驅動並完成灰階／彩色掃描與屬性驗證；Windows 掃描 App、取消、拔插、第二台電腦、解除安裝與明暗品質未驗收 |
| 06 | 列印 | 開發者 | not_started | 尚無 |
| 07 | 掃描明暗品質 | 開發者 | blocked | 已有空平台影像，缺少可對照原稿；歷史偏白仍未重現 |

## Current Blockers

- Windows 掃描 App（Microsoft.WindowsScan）能找到裝置但顯示「連線到掃描器時發生問題」。同一台電腦以桌面 PowerShell 呼叫 WinRT `ImageScanner.FromIdAsync`／`ScanFilesToFolderAsync` 成功掃描，wiatrace 顯示兩者對驅動的呼叫序列與回傳值完全相同（drvInitializeWia、drvInitItemProperties×2、drvReadItemProperties 1／1／1／12 全部 S_OK），差異在 App 的 AppContainer 客戶端。嘗試以 TraceLogging 名稱推導 GUID 擷取 `Microsoft.Windows.Scan.Runtime` 沒有事件，PrintScanBrokerService 未被啟動。待查：AppContainer 對 WinUSB 裝置介面的存取政策、WinRT 只回報灰階（`IsColorModeSupported(Color)` 為 false，可能與 WIA_IPS_CUR_INTENT 有效旗標有關）。

- 免費路線的限制：每台要用的電腦都得先信任專案測試憑證，不能公開分發。目前只在開發機驗證，第二台乾淨電腦、換孔、拔插、重開機與解除安裝尚未實測。

- 提早取消情境：已確認取消發生於首塊前的 READ metadata Busy，ABORT／RELEASE 都回成功，但立即重掃 RESERVE 收到 800 次 Busy、約 120 秒到期。保持同一 USB session 也失敗，關閉／重開連線不是必要條件。須查明首塊前取消的裝置時序，不能由成功清理回覆推論已可重掃。詳見 03 及硬體紀錄。

- 使用者確認平台沒有文件，無法完成文字、色彩、精確幾何與淺色細節驗收。取消／復原與 WIA 的獨立開發可繼續。
- RGB600 兩次停止影像進度後達到 120 秒工作期限，沒有 USB API 讀寫或清理錯誤。第二次第 20 塊後約 102 秒收到 678 次 Busy（0x08）回覆，未包含可解釋的 scanner state。仍需區分命令時序與裝置內部停止進度的原因，不能只延長期限當成修復。
- 原始影像傳輸中斷若失去框架同步，目前僅釋放 OS 資源並回報需重插，沒有證明可直接重掃。正式整合前須完成重連狀態與異常復原，並驗證暖機、拔線、睡眠及 20 次連續掃描。
- 開發機配對不等於正式套件。自訂 INF 沒有簽章 catalog，跨電腦安裝、換孔與 WIA 尚未驗證。
- 既有 MIT 授權保留。若選擇移植 SANE 的 GPL 實作，須先確認具體的授權與分發方案。本次沒有移植。
- 偏亮偏白問題缺少有內容的原稿可比較，目前無法確定原因。USB 與解碼數值已比對相同，沒有加入固定壓暗處理。

## Next Verifiable Output

兩條可平行工作：05 的 WIA INF 設計稿與備份／復原方案已寫成，簽署路線已定為免費測試憑證，安裝／更新／解除安裝腳本已寫好；接下來安裝 WDK、打包，取得授權後依 driver/README.md 順序信任憑證、安裝並實測 Windows 掃描；03 依精確階段取消日誌查明首塊前取消的裝置時序，再修正及重跑失敗驗收。屬性初始化、相依驗證、讀取通知、錯誤字串、格式列舉、能力列舉、同步命令與取消事件入口已接上，不重做。硬體能力須來自實際裝置，服務 context 必須由 Windows 提供，不可用假指標替代。硬體測試仍序列執行，重新列舉裝置並使用新輸出目錄。

IWiaMiniDrv 的 `drvWriteItemProperties`、`drvAnalyzeItem` 與 `drvDeleteItem` 仍回不支援；WIA 2.0 串流路徑不呼叫前者，後兩者不適用單一平台項目。STI 已宣告 STI_GENCAP_WIA，但服務是否據此載入 minidriver 仍待登錄後實測。COM aggregation 的實際需求、服務管理的並行載入／卸載排程、項目參考所有權與 runtime 前置條件仍須驗證。取消通知是否派送到同一 instance 與相同裝置 ID 也要由服務實測。600×800 選取區仍回傳 600×801，不可將輸出尺寸任意當成 WIA 選取範圍。WinUSB 共存、服務帳號存取及 Windows 掃描消費 BMP 仍需整合驗證。跨工作隔離依 03 補完，不以介面到達時間戳記當成實體重插證據。準備具體安裝、備份及復原方案後，才提出必要的系統變更授權。平台有文件後補做 02／07 品質對照。

## Next Ticket

[05 — Windows 掃描與安裝](docs/tickets/05-windows-install.md)，先做不需系統登錄的實作與測試。[03 — 取消與復原](docs/tickets/03-recover-scan.md) 的跨工作隔離及 20 次驗收持續追蹤。02／07 缺少原稿，01 的硬體異常情境亦保留。

## Decision Log

| decision | rationale | timestamp | impacted_ticket_ids |
| --- | --- | --- | --- |
| Rust、Windows 11 x64、掃描優先 | 使用者要求與本機系統 | 2026-09-13 | 01–06 |
| 純驅動，不開發 GUI 或掃描 App | 使用者明確修正範圍，刪除原 04 工作，操作與儲存由既有軟體負責 | 2026-09-13 | 02、03、05 |
| 初期建議 WinUSB + Rust 使用者模式掃描 | Windows 提供底層 USB，SANE 有真實協定依據 | 2026-09-13 | 02、05 |
| WIA 安裝架構待驗證 | WinUSB 不會自動提供 Windows 掃描相容性 | 2026-09-13 | 05 |
| 維持 MIT、未引入 SANE 程式碼 | 保存原始授權，避免未確認的移植授權變更 | 2026-09-13 | 02 |
| 將彩色過曝感與灰階偏淡列入驅動品質驗收 | 使用者回報 Windows 掃描的歷史症狀，需先對照資料定位原因 | 2026-09-13 | 02、05、07 |
| 驗證後自行 commit 與 push | 使用者授權本專案必要的提交與推送，系統安裝授權分開處理 | 2026-09-13 | 01、02、03、05、06、07 |
| 核准本機 MI_00 配對、GUID 登錄與介面重啟 | 使用者回答「好」同意具體安裝範圍，限本機已備份目標 | 2026-09-13 | 02、05 |
| 完整套件須支援別台 Windows 電腦與不同 USB 接孔 | 使用者明確補充可攜性要求；目前僅驗證開發機配對與能力查詢 | 2026-09-13 | 05 |
| 加速保持解析度、色深、範圍與像素品質，資料處理可供其他機型沿用 | 使用者補充效能及共用需求，先量測耗時再選擇改善位置，機型協定各自驗證 | 2026-09-13 | 02、03、07 |
| 建立持續完成完整驅動的目標 | 使用者要求逐步完成實作、驗證與推送，需要使用者介入時提出具體需求，可獨立工作繼續推進 | 2026-09-13 | 01、02、03、05、06、07 |

## Verified

2026-09-19 重開機後：使用者重開機，`wc3119-setup.ps1 -Action Status` 顯示 MI_00 服務 WINUSB、oem19.inf 0.2.9.0、Image 類別、問題碼 0，兩個裝置介面 Enabled，stisvc Running，WIA 1 台；WIA automation 連線 75 ms、灰階 75 dpi 傳輸 7.4 秒成功。這證明「解除安裝→重新安裝→重開機」的循環可以復原，也是第一次重開機後的自動載入驗收。

2026-09-19 影像介面與句柄保留版本：178 個 lib 測試、全部整合測試及 2 個 doc-tests、fmt、Clippy、release targets、當次 release DLL 動態載入通過；DLL SHA256 `C3DD8218E3B7901C0FDB2589CFD808C546A392272AEF418691E558FFE155ACEE`，套件 0.2.9.0 以 Update 流程安裝。實機：INF 加入 `GUID_DEVINTERFACE_IMAGE` 後 `pnputil /enum-interfaces` 列出兩個介面，WinRT 選擇器（InterfaceClassGuid＋WiaDeviceType=1）找到裝置。提權探針證實 WinUSB 每台裝置只接受一個開啟中的句柄（第二次任何存取／共用模式的開啟皆回 ERROR_ACCESS_DENIED，零存取權的句柄也會阻擋），因此服務的通知句柄曾使 LockDevice 回 0x80070005；改為 Initialize 即開啟並保留句柄後，WIA automation 灰階 75 dpi 掃描 7.3 秒成功，WinRT `ScanFilesToFolderAsync` 7.3 秒產生 565486 bytes BMP。硬體測試 `actual_sti_device_lock_presence_and_release` 已改為「第一個物件 Release 前第二個物件不能鎖定」，本輪未在實機重跑（WIA 服務持有裝置時無法執行硬體測試，須先停止 stisvc）。

2026-09-19 WIA 服務實掃版本：178 個 lib 測試、全部整合測試及 2 個 doc-tests、fmt、Clippy、全部 release targets、當次 release DLL 動態載入通過；DLL SHA256 `2CBCB3B37E5CC495E1557F859443C78412E4A7C84EAC637CA778BF7BA3D19C1D`。實機：套件 0.2.6.0 經 `wc3119-setup.ps1 -Action Update -Apply` 安裝，WIA automation `Connect` 70 ms，根屬性 Manufacturer／Description／Name 由服務提供，平台項目回報 75 dpi、637×877 選取、8 bpp、Item Size 562358；灰階 75 dpi 傳輸 7.3 秒得 648×871 8 bpp BMP（565486 bytes，抽樣灰階平均 255，空平台）；同一連線改彩色後 Item Size 更新為 1676878、無效 999 dpi 被服務以 0x80070057 拒絕、彩色傳輸 10.9 秒得 648×871 24 bpp BMP。wiatrace 顯示 drvGetCapabilities、drvInitializeWia、drvInitItemProperties×2、drvReadItemProperties、drvGetWiaFormatInfo、drvAcquireItemData 全部回 S_OK，END_OF_STREAM／END_OF_TRANSFER 由服務發送。逐步排除的服務端失敗：CLASS_E_NOAGGREGATION（改支援聚合）、STIERR_OLD_VERSION（接受版本 ≥2）、LockDevice E_FAIL（port name AUTO 改 GUID 列舉）、相容模式項目讀取 E_UNEXPECTED（不再對非狀態讀取查項目類型）。私人證據在 `artifacts/wia-scan-20260919T050522Z/` 與 `artifacts/wia-setup-*`。沒有 Windows 掃描 App、取消、拔插、第二台電腦或文件品質驗收。

2026-09-19 能力列舉版本：176 個 lib 測試、全部整合測試及 2 個 doc-tests、fmt、Clippy（all targets，warnings 為錯誤）、全部 release targets 通過；當次 release DLL 動態載入以 ignored 模式另行通過，slot 14／x64 偏移 112 的能力列舉以 null context 回傳 2 個事件。TDD：公開 COM 測試先取得能力列舉回 E_NOTIMPL 的 RED。`tests/sti.rs` 原本斷言 GetCapabilities 旗標為 0 並註明「實作前不宣告」，前提已不成立，改為斷言 STI_GENCAP_WIA。沒有硬體、系統登錄或 Windows 掃描驗收，詳見 [05](docs/tickets/05-windows-install.md#能力列舉同步命令與-sti-wia-宣告)。

2026-09-19 屬性讀取／錯誤字串版本：171 個 lib 測試、全部整合測試及 2 個 doc-tests、fmt、Clippy（all targets，warnings 為錯誤）、全部 release targets 通過；當次 release DLL 動態載入以 ignored 模式另行通過，slot 8／x64 偏移 64 的讀取入口拒絕缺少服務 context，slot 12／偏移 96 的錯誤字串入口回傳呼叫端釋放的 OLE 字串。TDD：公開 COM 測試先取得讀取入口回 E_NOTIMPL 而非 E_INVALIDARG 的 RED。沒有硬體、系統登錄或 Windows 掃描驗收，詳見 [05](docs/tickets/05-windows-install.md#屬性讀取通知與裝置錯誤字串)。

2026-09-14 屬性相依驗證版本：236 個 all-targets 測試、一般測試及 2 個 doc-tests、fmt、Clippy、全部 release targets 與另行啟用的當次 DLL 動態載入通過。模式／解析度／範圍相依、原生差異寫入、唯讀／未知 ID 與名稱拒絕、部分發佈失敗隔離已有測試。獨立審查找到的初始化失敗隔離缺口與硬體日誌誤報成功已修正；最後複查沒有新增確認的未處理 P1／P2。真正服務 old/current 儲存、失敗後重新載入及 Windows 掃描仍未驗收，詳見 [05](docs/tickets/05-windows-install.md#屬性相依驗證與發佈失敗隔離)。

同版實機能力查詢 0.02 秒通過。精確階段取消對照：首塊後取消並沿用同一 session 重掃 26.41 秒通過；首次 READ Busy 取消後，立即重掃 RESERVE 800 次 Busy、120.065 秒到期，整組 120.52 秒失敗且沒有完成標記。兩次的建置差異、數值日誌與界線見 [硬體紀錄](docs/hardware.md#精確階段取消與同連線重掃)。此測試不保存像素，沒有影像品質驗收。前後三個 PnP 介面正常，沒有系統登錄或 USB 重設。

2026-09-14 屬性初始化版本：207 個 all-targets 測試、一般測試及 2 個 doc-tests、fmt、Clippy、全部 release targets、另行啟用的當次 DLL 動態載入通過。SDK C11 與 Rust 斷言核對屬性 ABI，合成接收端逐筆檢查原生發佈資料與失敗即停止，解鎖失敗的隔離及重入測試通過。實機共用 session 能力查詢另通過 0.02 秒，測試前後三個介面正常，沒有掃描或系統變更。根代理與 Luna 複查沒有確認的未處理 P1／P2。細節、雜湊及服務驗收限制見 [05](docs/tickets/05-windows-install.md#屬性初始化與即時能力)。Windows 掃描、取消復原與文件品質仍未完成驗收。

2026-09-14 取消事件／格式版本：178 個 all-targets 測試、一般測試及 2 個 doc-tests、fmt、Clippy、全部 release targets、明確啟用的當次 DLL 動態載入通過。SDK C11 格式結構與方法偏移斷言通過。回呼取消回歸實掃 42.99 秒通過，BMP 全樣本與影像檢視通過。新提早取消測試四次失敗，已保留實際結果，詳見 [05](docs/tickets/05-windows-install.md#取消事件與格式列舉) 及 [硬體紀錄](docs/hardware.md#wia-取消事件與提早取消後重掃)。沒有 Windows 掃描或文件品質驗收。

2026-09-14 原生鎖定／acquire 版本：168 個 all-targets 測試、一般測試與 2 個 doc-tests、格式、Clippy、全部 release targets 及明確啟用的當次 DLL 動態載入通過。合成屬性 dispatch 的灰階、彩色取消與重掃實機測試 42.21 秒通過，鎖定／INQUIRY 修後另通過 0.07 秒。Pillow 全樣本比對及實際影像檢視通過，仍是空平台，沒有服務 context 或 Windows 掃描驗收。證據與剩餘差異見 [05](docs/tickets/05-windows-install.md#原生鎖定與掃描-dispatch) 及 [實機紀錄](docs/hardware.md#wia-鎖定與-dispatch-實機驗證)。

2026-09-14 原生項目樹版本：156 個 all-targets 測試、一般測試與 2 個 doc-tests、格式、Clippy、全部 release targets 及另行執行的當次 DLL 動態載入通過。Windows 原生根／平台項目重複建立、雙用戶端共用、名稱拒絕、helper 參考與重入清理通過。SDK C11 與 Rust 斷言核對介面大小／偏移。這些測試沒有 WIA 服務 context、USB 或系統設定操作，完整證據及下一步見 [05](docs/tickets/05-windows-install.md#原生項目樹與介面身分)。

2026-09-14 原生傳輸回呼版本：153 個 all-targets 測試、一般測試及 2 個 doc-tests、格式、Clippy、全部 release targets 通過；當次 release DLL 的動態載入測試另行通過。callback 轉接、BMP 進度與公開入口均先取得缺少實作的失敗，再實作通過。7 個傳輸流程測試涵蓋原生記憶體串流、合成像素、開始前取消／SKIP、非空串流、錯誤保留、清理失敗及最後進度取消。SDK C11 靜態斷言確認 callback vtable、結構偏移與常數。

新回呼實機測試 42.29 秒完成灰階、彩色取消及重掃，QI／GetNextStream／SendMessage／Release 內重入均維持排他且可查詢。原有兩個硬體測試另逐一通過（0.05／42.11 秒）。Pillow 獨立檢查 BMP 標頭與全部有效樣本，實際檢視新回呼的灰階／彩色影像仍是空平台。測試後 doctor 三個介面正常，INQUIRY 成功，沒有系統變更。完整證據見 [實機紀錄](docs/hardware.md#原生-wia-callback-實機傳輸)。

Diff Inspector：Scope CLEAN，根代理查核全部程式／文件差異與共用掃描的消費端，Luna 完成原生 ABI、參考生命週期、回呼重入及錯誤分類的獨立對抗審查，沒有確認的未處理 P1／P2。依 Microsoft 契約移除草稿中的手動結束通知，僅由服務發送 END_OF_STREAM／END_OF_TRANSFER；沿用既有掃描及 session 借用流程。服務串流定位、暖機／USB 等待期間取消及真實 WIA context 列為後續整合驗證。最終來源與 DLL 雜湊見 [05](docs/tickets/05-windows-install.md#測試)。

2026-09-14 修正版：129 個 all-targets、2 個 doc-tests、格式、Clippy、全部 release targets 通過，當次 release DLL 的動態載入測試另行通過。兩個 STI ignored 硬體測試逐一通過，鎖定／診斷 0.05 秒，同物件灰階／彩色取消／重掃 42.12 秒。Pillow 檢查全部 BMP 有效樣本解碼一致，GDI+ 開啟及實際影像檢視通過，內容為空平台。沒有重插或系統變更，詳見 [實機證據](docs/hardware.md#共用連線實掃與提早取消修正)。

Diff Inspector：根代理與 Luna 補完 `6fbc383..705f0d6` 的核心及消費端複核，發現有效 INQUIRY 後、未嘗試 RESERVE 就取消會錯誤隔離的 P2。新增回歸測試先取得 `NeedsReconnect != Ready` 失敗，再修正為以實際命令嘗試旗標分類；已送 RESERVE 的 Busy 後取消維持隔離。根代理審查本輪全部差異，Luna 另複核修正版，沒有新增確認問題。最終核心 SHA256：`DB43C54B89CC4FC1A8343FA3F8378CCCCCE22843A5430611B5017566DBA784B1`。這個精確取消時機僅以合成回覆控制，實機驗證的是影像 callback 取消及其後重掃。

2026-09-13 連線借用版本：128 個 all-targets 測試、2 個 doc-tests、格式、Clippy 及全部 release targets 通過；另行載入當次 DLL 的 1 個測試通過。新增 5 個資源生命週期／重入／並行測試、8 個核心健康狀態測試及 1 個 COM 物件的未鎖定／預先取消測試。借用器與公開入口先取得缺少實作的失敗，關閉完成前可被重開的競態先重現再修正。當時依使用者要求收尾，兩個 STI 硬體測試沒有執行，已於上述 2026-09-14 補驗。

該前版由根代理檢查全部核心差異與呼叫關係，補回共用 WinUSB 讀取上限預檢。當時 Luna 完成借用器、STI、BMP 與 COM 邊界審查，核心凍結版的獨立複核留待下輪，結果已記於上述 2026-09-14 修正。前版核心 SHA256 `D8B0E05E46AC4BCE1446902432D2A668E5645A3C95205C398E3A342AF3125A43`，借用器為 `2EF501E2717ACF1B3AB1D99E95C333DF0298F7F6FED9283582E9D67314ACA2C4`。

IStiUSD 版本：114 個 all-targets 測試、2 個 doc-tests、格式、Clippy、全部 release targets 通過。兩個預設 ignored 測試分別明確執行並通過：release DLL 的 IStiUSD 身分／生命週期，以及真實 MI_00 的指定路徑、互斥、能力診斷和最終釋放。初始 IStiUSD 與 DLL QI 測試曾對舊版失敗，USB 未指定目標的多候選回歸亦先取得失敗再修正。鎖定死鎖及狀態結構大小由審查發現並修正，補測首次執行即通過，沒有宣稱它們取得 RED。SDK C11 靜態斷言核對 19-slot vtable、helper port slot、結構大小／偏移及版本／錯誤常數。詳見 [05](docs/tickets/05-windows-install.md#測試) 與 [實機紀錄](docs/hardware.md#istiusd-實機鎖定與能力診斷)。

本版 Diff Inspector：範圍符合 05，根代理完整差異及 Luna 跨元件／並行邊界對抗審查沒有確認的未處理 P1／P2。USB 開啟本體與前版比對，差異只有移動作用域、型別名稱及格式，沿用單一開啟實作。待查證包含真實 STI helper 的執行緒／COM apartment 生命週期、無效 helper 回傳未終止字串，以及同步 Diagnostic 期間其他方法等待 USB 逾時的服務端行為。現在沒有在狀態鎖內呼叫 helper，未確認存在重入死鎖；後續 WIA callback 必須重新審查鎖的範圍。

COM loader 版本：105 個 all-targets 測試、2 個 doc-tests、格式、Clippy、全部 release targets 通過；預設 ignored 的 release DLL 測試已另行執行，1 個通過。實際 exports 恰為 DllGetClassObject／DllCanUnloadNow，建置未再出現 LNK4104。新測試先驗證缺少模組／DLL 的失敗；審查發現的雙計數卸載競態先取得失敗證據，再以單一 module hold 修正。最終含 production ModuleState 測試與公開並行交接測試。根代理與 Luna 查核沒有確認的未處理 P1／P2，COM 服務載入排程仍列為整合待驗證。來源與 DLL 雜湊見 [05 測試](docs/tickets/05-windows-install.md#測試)。沒有系統登錄或實機掃描，不能視為 WIA minidriver 已完成。

WIA 數值設定版本：98 個 all-targets 測試、一般測試含 2 個 doc-tests、格式、Clippy（warnings 為錯誤）、核心及全部範例 release 建置通過。新模組先取得缺少公開模組的失敗，再完成 4 個設定測試及 1 個當次能力／命令整合測試。Gray75 與 RGB75 使用同一測試建置完成真實掃描、原生 COM 串流輸出及讀回，獨立 BMP 解碼和 GDI+ 開啟通過。平台沒有文件，尺寸與品質限制見 [實機紀錄](docs/hardware.md#wia-數值設定與原生串流實掃)。尚未驗證 WIA 服務提供的串流或 Windows 掃描。

Diff Inspector：根代理查核本輪完整程式／文件及呼叫關係，Luna 對抗審查沒有確認的 P1／P2。`src/wia.rs` 最終 SHA256 `1AAD9DE923CEA3ABFB4463C032D2102FFB4474C59ED37E08BDCE08618001E33A`。正式 WIA 色彩屬性及選取區同步由 05 接續驗證；公開入口到 COM 的完整實機測試目前保留於私人 harness，尚未列為可自動執行的 repository 硬體測試。

原生 COM 串流版本：93 個 all-targets 測試、一般測試含 2 個 doc-tests、格式、Clippy（warnings 為錯誤）、核心及全部範例 release 建置通過。原生新功能先取得缺少模組的失敗測試，再實作。Windows `CreateStreamOnHGlobal` 真實物件完成 BMP 寫入、定位及回讀，合成 COM 邊界另測部分寫入、錯誤 HRESULT、取消不重試及只釋放一次。測試執行緒的 COM 初始化已於串流釋放後解除，不登錄 DLL、不操作 USB。仍未驗證 WIA 服務串流、裝置登錄及 minidriver 載入，見 [05](docs/tickets/05-windows-install.md)。

根代理完整差異查核與 Luna 原生邊界對抗審查未發現新增 confirmed P1／P2。SDK 方法順序及型別與 Windows x64 實測相符，未外推到其他架構。最終 `src/com_stream.rs` SHA256 `740E530C1A5341A660491BF1C144D6CB0CFC271EB5F449EC33A5E5EB3366B3B4`，`tests/com_stream.rs` 為 `9B7381E61F1EC6293C870AE7A439B85A5B714598D2C6FF94D6BFE0F2400570CE`。

BMP 串流版本：88 個 all-targets 測試、一般測試含 doc-tests、格式、Clippy（warnings 為錯誤）、核心及全部範例 release 建置通過。新增核心先取得失敗測試再實作，補測部分寫入後 Interrupted 不重試、尺寸／資源上限、取消與釋放失敗不完成影像。Luna 獨立對抗審查及根代理完整差異檢查未發現新增 confirmed P1／P2；最終 `src/bitmap.rs` SHA256 `B61D1232AAA574215C3514EEE3FA330205844FAF27629F1D004BF1D4C875FF4E`。

實機 Gray75、RGB300 及取消後 Gray75 全部通過 USB／像素／BMP／PGM 或 PPM 的獨立全樣本比對。RGB75 第一塊後取消回傳失敗，不產生成功標記。Windows GDI+ 能開啟前兩張 BMP，另核對各九個樣本；灰階及彩色影像實際檢視仍為空平台，未驗收文件品質。這些結果沒有證明 Windows 掃描可用、偏白已修復或 600 dpi 穩定性通過。不同測試建置的雜湊與證據見 [BMP 實機紀錄](docs/hardware.md#bmp-串流實機驗證)。WIA automation 唯讀列舉為 0 台，沒有登錄 COM 或變更系統綁定。

傳輸緩衝區版本：70 個 all-targets 測試、一般測試含 doc-tests、格式、Clippy（warnings 為錯誤）、核心及全部範例 release 建置通過。新增上限、短讀、尾塊、取消排空及參數測試，依 TDD 先失敗再實作核心功能。獨立審查後將公開掃描入口合併至同一預檢路徑，超限失敗亦保留要求大小及回報上限。實機 64／256 KiB 各一次 RGB600 成功，最終版 Gray75 重掃及獨立逐像素對照成功，見 [硬體紀錄](docs/hardware.md#64-kib-與-256-kib-影像讀取對照)。沒有調整掃描預設值或宣稱故障／品質已修復。

Diff Inspector：根代理已查核本輪完整程式及文件差異，Luna 對傳輸關鍵流程複查，兩個預檢／診斷問題已修正，最終未發現新增 confirmed P1／P2。複查 `src/scan.rs` SHA256 為 `E20CB8A9434210AACA605A79F8CFEB212BCFE1B2F51C7FCDB464F10CF8365B6C`。上限預檢針對掃描入口，獨立 INQUIRY 維持既有 1024-byte 讀取，不把 RAW_IO 的限制外推成一般 WinUSB 的已確認故障。WIA 介面及服務帳號另完成唯讀查核，下一段實作與未確認的 WinUSB 共存條件記於 [05](docs/tickets/05-windows-install.md)。

四次空平台 RGB600 按 100、500、500、100 ms 間隔對照皆成功，耗時依序 92.845、106.177、102.656、95.385 秒，尺寸與像素 bytes 相同且每次完整通過 wire／pixels 核對。影像讀取約 72 秒，解碼與呼叫端合計每次不到 0.1 秒，沒有支持優先平行化影像處理或改用 500 ms 的證據。這四次是各自獨立的程序，沒有重現先前故障，也沒有驗收文件品質或 20 次穩定性。量測與限制見 [硬體紀錄](docs/hardware.md#效能分段與-read-詢問間隔對照)。

最終核心及兩個掃描範例 release 建置通過，`scan_stability --help` 已核對參數、預設值與 profile 說明。格式整理後 `scan_stability.exe` SHA256 `FCBFDFD281E1EBE3626AA7C527A81C45647A3720BC69AF7D1E1C5640D643147F`，`capture_scan.exe` SHA256 `7D3ABD09B59BAEAC5C123FC35F17102436A228A5B8DC1EF54D040B0D390756C6`。四次實機測試使用格式整理前但程式行為相同的 release，識別另存於硬體紀錄，沒有把重新建置當成重跑掃描。

效能量測版本：格式檢查、Clippy（all targets，warnings 為錯誤）、64 個 all-targets 測試、一般測試含 doc-tests 通過。新 profile／間隔驗證及範例參數／寫入錯誤測試先取得失敗再實作，取消期間 Busy profile 另有回歸測試。Luna 獨立審查核心與範例 diff，沒有新增 confirmed P1／P2。根代理核對 API 使用點、命令與像素路徑，以及文件一致性。核心 SHA256 `0EFF43CCD626BF7041020E591ABAB41B26CA6E2A159728F708AFA9E95767B035`，範例 SHA256 `E47E319F7329E0DD800F514E9B45BF65BA97841B189EC6F154F81E55593EF978`。原有失同步隔離與 CHECK 語義問題繼續由 03 追蹤。

診斷與連續掃描工具版本：格式檢查、Clippy（all targets，warnings 為錯誤）、59 個 all-targets 測試、一般測試含 doc-tests，以及核心／兩個掃描範例 release 建置通過。階段／原始錯誤／影像進度與 Busy 歷史隔離均先取得失敗測試再實作；短框架防 panic 與 CHECK 狀態偏移另有通過的回歸測試。`scan_stability --help` 已實跑，確認次數、模式輪替、輸出及失敗行為與實作相符。最終 `scan_stability.exe` SHA256 `3C600881525979BD72C2F45E84F1DE4579FBE649DCE01C9D61FA38D01C842FD5`，`capture_scan.exe` SHA256 `C06BB5FE70370BC43515AA92C4A2BE7D28B6C2988E5FD4BBB9DE65DCF6BAED0A`。

本輪連續掃描：同一程序前 4 次成功，第 5 次 RGB600 在 54 塊後自然逾時，沒有完成標記，20 次驗收未通過。記憶體峰值約 7.39 MiB，沒有觀察到暴增。逾時清理後 Gray75 成功，全部 564408 個像素再次以獨立 Python 核對 USB／PGM 相同；PGM SHA256 `7ac754eb2d4e29605cb1771180aa20505e06e5557b2e833c4d584466f2178e81`。這是空平台傳輸證據，沒有驗收文件品質或證明歷史故障同因。詳見 [硬體紀錄](docs/hardware.md)。

最終診斷版本預定 2 次 RGB600，第一掃在 20 塊後自然逾時即停止；READ Busy 678 次、最後 status=0x08、state=unknown。清理後 Gray75 成功，7.221 秒；沒有重插或安裝動作。這次直接驗證新增診斷可捕捉卡住的命令與 Busy 狀態，沒有證明中斷已修復。

Diff Inspector：本輪範圍符合診斷及可靠性調查，根代理已審查完整 diff，Luna 對掃描關鍵流程完成獨立對抗審查。最終 `src/scan.rs` SHA256 `477EE89D8DBAF1AE83CBF4945853B9CEE4F099810E6107B34F75949E6718363A` 未發現新增 confirmed P1／P2；既有非 MSG20 CHECK 處理語義與失同步跨工作隔離仍列於 03，不能視為完整驅動已通過審查。

前次有限排空版本驗證：格式檢查、Clippy（all targets，warnings 為錯誤）、47 個 all-targets 測試、一般測試含 doc-tests 與核心／擷取範例 release 建置全部通過。擷取範例 release SHA256：`74848DAF2F002A4A3665DCD0DAE044B97018C2FEC963AD296057474F53863775`。新增測試先失敗再實作；範例 help 已實跑核對定時取消、預設值、錯誤及用法。開發配對工具未修改／重跑安裝。

前次實機證據：RGB600 5100×6961、117 塊、106503300 bytes，94.951 秒完成；獨立逐像素對照與 USB 資料一致。100 ms 早期取消及 8000 ms 進行中取消皆回傳失敗、不產生完成標記，隨後 Gray75 重掃成功。沒有注入實機 USB 錯誤，也沒有驗收文件品質。Luna 獨立審查有限排空未發現新增的傳輸重試缺陷；失同步後 API 尚未強制阻擋下一個工作是既有缺口，持續追蹤於 [03](docs/tickets/03-recover-scan.md)。

首次影像傳輸：Gray75 648×871、RGB75 648×871、RGB300 2556×3476、Gray600 5100×6959，皆正常完成並釋放。RGB75 第一塊後取消回傳 Interrupted、沒有完成標記，立即重掃成功。300RGB 工作中另一行程被拒（OS error 5）且原工作完成。獨立 Python/Pillow 對照所有成功初始掃描的 wire 與像素相同，RGB 僅重新排列；75 dpi 影像實際檢視為空平台，不能驗收偏白修復。詳見 [硬體證據](docs/hardware.md)。

新增掃描核心先取得測試失敗，再實作。Luna 審查指出跨 session 證據競態與回覆訊息類型檢查缺口，已修正並通過合成測試；修正版又完成 Gray75、RGB75 取消及其後 RGB75 重掃。原始傳輸失同步不能自動復原的限制保留於 03，不當成已解決。callback panic 另有先失敗後成功的取消／釋放測試。

本機授權配對後：`DiInstallDevice`、精確 MI_00 重啟皆回傳 0，協調腳本結果 `Paired`、`NeedsReboot=false`、MI_00 問題碼 0。GUID 回讀符合單一 REG_MULTI_SZ 值。父裝置／MI_01 的服務、INF、問題碼、ClassGuid 與 Parent 均符合原始備份。一般權限 Rust `doctor` 與 `inquiry` 實跑均回傳 0。

真實機器回報：`SAMSUNG ORION`；已辨識解析度 `[75, 100, 150, 200, 300, 600]`；解析度旗標 `0x00353f`、模式 `0x29`、行序 `0x01`、壓縮 `0x2f`；寬 `10200`、最大長／平台長 `14040`（1/1200 英吋）。首次 INQUIRY 當時尚無影像；後續已保存原始能力與端點並取得上述像素尺寸，光學解析度與明暗品質仍未驗證。

開發機配對工具：24 個測試（既有 17 個及工具 7 個）、Clippy、格式檢查與 release 建置通過。Spark 實跑無效參數拒絕路徑；Luna 審查原生呼叫與本機操作腳本。已修正候選不唯一時預檢仍回傳成功的缺陷，並釐清安裝會持續修改目標綁定。原生 API 結構大小與欄位偏移通過 x64 SDK 對照測試。

實機預檢的裝置專屬與全域 CLASS 清單各有 3 個候選，均只有 1 個符合指定內建 WinUSB。備份包含父裝置／兩個介面屬性、MI_00 登錄匯出與原始 Device Parameters，存於 Git 排除的 `artifacts/winusb-baseline-20260913T043413Z-2a857f59/`。取得授權後經 UAC 執行同目錄 `install.ps1 -Apply`，結果及安裝／重啟紀錄保留於此。復原未執行，本機成功不能當成正式安裝功能完成。

配對工具 release SHA256：`28A92A04E487A5F83E3A970C62483345FC8DE37E0F79A2FE47437ACD29928DE2`。

能力查詢版本先加入失敗測試，再完成解析、USB 驗證與 CLI。修正合成樣本的字串長度錯誤後，7 個能力解析測試通過；傳輸端另先重現錯誤 bulk 最大封包大小未被拒絕，再修正至測試通過。最終格式檢查、Clippy、17 個測試及 release 建置均通過。

配對前的 release 實跑 `doctor` 為 MI_00 問題碼 28、結束碼 2；`inquiry` 回報缺少已登錄 WinUSB 介面、結束碼 1，在開啟 USB 前停止。當時只驗證缺少介面的失敗路徑。配對後成功通訊見上方最新紀錄；實際逾時、拔線、占用及異常資源釋放仍未驗證。

能力查詢版本 release SHA256：`51E689AB154B51907AE9F233E3F075BBD6312DE103E8663B23939A0CD954BA63`。

明暗品質調查已查核 SANE 的 threshold 適用模式與 Microsoft WIA 亮度／對比定義。再次實跑 doctor，問題碼仍為 28，結束碼 2。本輪文件連結、格式、Clippy、既有 5 個測試及 release 建置通過。尚未取得任何掃描影像，因此沒有修正前後品質測試結果。

純驅動範圍修正後，已確認自製 GUI 工作項目刪除、相關文件改為由既有掃描軟體負責操作與儲存、本機文件連結有效。重新執行格式檢查、Clippy、5 個測試與 release 建置均通過。本次只修改文件，沒有新增掃描能力或變更系統驅動。

初始化時先加入公開診斷 API 測試，確認未實作時編譯失敗，再完成實作。結果：

- `cargo fmt --all -- --check`：通過。
- `cargo clippy --offline --all-targets -- -D warnings`：通過。
- `cargo test --offline`：5 個測試通過，涵蓋精確裝置 ID、診斷分類、介面隔離、說明及參數拒絕。
- `cargo build --offline --release`：通過。
- `target\release\wc3119.exe doctor`：實機辨識父裝置、MI_00、MI_01。MI_00 service 未安裝、問題碼 28、started=false，結束碼為 2。與 PowerShell 系統查詢一致。
- INF 文字範圍檢查：只有 MI_00 型號配對。未執行 InfVerif、Inf2Cat 或實際安裝。
- `git diff --check`：已追蹤變更通過。文件連結另以本機檔案存在性檢查。

初始化版本的 release 執行檔 SHA256：`14A779EDC22632988CB097553A58DCFA8AF1F30581DF8BC1B211DCFEF29D0907`。這不是後續能力查詢版本的雜湊。

工具在受限環境出現 home 路徑 canonicalize 與 Git 全域 ignore 存取警告，未使上述最終建置及測試失敗。沒有實際掃描、影像品質或 WIA 測試結果。

## Changed

| 檔案 | 變更摘要 |
| --- | --- |
| [AGENTS.md](AGENTS.md) | 專案規則、硬體限制與交付要求 |
| [CLAUDE.md](CLAUDE.md) | AGENTS.md 入口指標 |
| [Cargo.toml](Cargo.toml) | Rust 套件與檢查規則 |
| [build.rs](build.rs) | MSVC cdylib 的 COM export 定義參數 |
| [driver/com-exports.def](driver/com-exports.def) | 兩個 runtime COM exports，排除 import library 項目 |
| [src/com_server.rs](src/com_server.rs) | DLL 入口、factory、IUnknown／module lock 生命週期及鎖定物件的原生回呼傳輸入口 |
| [src/com_server/minidrv.rs](src/com_server/minidrv.rs) | IWiaMiniDrv 共用身分、原生初始化與多用戶端生命週期 |
| [src/com_server/minidrv/tree.rs](src/com_server/minidrv/tree.rs) | Windows 根／平台項目、BSTR、連結及釋放 |
| [src/com_server/minidrv/locking.rs](src/com_server/minidrv/locking.rs) | 經服務 IStiDevice 鎖定／解鎖、項目連線借用 |
| [src/com_server/minidrv/properties.rs](src/com_server/minidrv/properties.rs) | 真實服務屬性快照、原生 GUID／BSTR 讀取 |
| [src/com_server/minidrv/acquire.rs](src/com_server/minidrv/acquire.rs) | 原生 DOWNLOAD 入口、共用掃描與 HRESULT 映射 |
| [src/com_server/minidrv/locking/hardware.rs](src/com_server/minidrv/locking/hardware.rs) | 明確啟用的 WIA 鎖定、合成屬性 dispatch 真實 USB 測試 |
| [src/com_server/sti.rs](src/com_server/sti.rs) | IStiUSD 初始化、指定裝置獨占、能力診斷及錯誤回報，共用 BMP／原生回呼的連線借用 |
| [src/com_server/session.rs](src/com_server/session.rs) | 同一資源借用、回呼期間排他、關閉時序及異常隔離 |
| [tests/sti.rs](tests/sti.rs) | SDK 契約、helper 參考及明確啟用的實機互斥／釋放、原生回呼取消與重掃驗證 |
| [tests/com_server.rs](tests/com_server.rs) | ABI、失敗、參考釋放及並行 module hold 交接 |
| [tests/com_server_dll.rs](tests/com_server_dll.rs) | 明確指定 release DLL 的實際動態載入與卸載 |
| [Cargo.lock](Cargo.lock) | 可重現的套件鎖定檔，目前無第三方依賴 |
| [.gitignore](.gitignore) | 排除建置結果與本機實驗資料 |
| [src/lib.rs](src/lib.rs) | 精確裝置識別、診斷分類與能力查詢入口 |
| [src/windows.rs](src/windows.rs) | Windows 唯讀裝置與驅動查詢 |
| [src/usb.rs](src/usb.rs) | WinUSB 裝置核對、精確路徑選擇、端點／讀取上限查詢及單次 INQUIRY |
| [src/protocol.rs](src/protocol.rs) | 能力回覆框架驗證與欄位解析 |
| [src/bitmap.rs](src/bitmap.rs) | 逐列 BMP 編碼、格式／尺寸驗證、部分寫入與失敗處理，回報完整列數及編碼進度 |
| [src/com_stream.rs](src/com_stream.rs) | Windows IStream 輸出、原始 HRESULT、單次參考釋放及執行緒限制 |
| [tests/com_stream.rs](tests/com_stream.rs) | 合成 COM 錯誤邊界與 Windows 真實記憶體串流 BMP 回讀 |
| [src/wia.rs](src/wia.rs) | WIA 數值設定驗證、精確範圍換算及真實掃描 BMP 入口 |
| [tests/wia.rs](tests/wia.rs) | 六種解析度、模式／色深、無效設定與預先取消 |
| [src/wia_callback.rs](src/wia_callback.rs) | 原生 callback QI、BSTR、GetNextStream、進度與 HRESULT／參考管理 |
| [src/wia_transfer.rs](src/wia_transfer.rs) | 同一連線的回呼掃描、BMP 進度、取消及輸出／清理錯誤保留 |
| [src/com_server/minidrv/cancel.rs](src/com_server/minidrv/cancel.rs) | 每工作取消旗標與原生取消事件，完成／取消排序及裝置識別 |
| [src/com_server/minidrv/formats.rs](src/com_server/minidrv/formats.rs) | 由真實項目型別列舉靜態 BMP／TYMED_FILE 格式 |
| [tests/wia_callback.rs](tests/wia_callback.rs) | 15 個原生回呼契約與 Windows 記憶體串流測試 |
| [tests/support/wia_callback.rs](tests/support/wia_callback.rs) | 共用的獨立 COM ABI 測試物件及真實 HGLOBAL 串流 |
| [src/scan.rs](src/scan.rs) | 掃描工作、影像解碼、有限排空、階段／Busy 診斷與效能量測，共用上限預檢與連線健康狀態，RESERVE 前取消保留可用連線 |
| [examples/capture_scan.rs](examples/capture_scan.rs) | 私人實機證據擷取、定時／塊後取消、BMP 串流與完成標記 |
| [examples/scan_stability.rs](examples/scan_stability.rs) | 同程序連續掃描、獨立像素核對、成功／失敗 profile 及有限範圍的 READ 間隔／緩衝區參數，任一失敗即停止 |
| [src/main.rs](src/main.rs) | 繁體中文 doctor 與 inquiry 指令 |
| [tests/device.rs](tests/device.rs) | 裝置與狀態測試 |
| [tests/cli.rs](tests/cli.rs) | CLI 行程測試 |
| [tests/inquiry.rs](tests/inquiry.rs) | 合成能力回覆、未知旗標、截斷與異常資料測試 |
| [examples/winusb_setup.rs](examples/winusb_setup.rs) | 唯讀 WinUSB 候選預檢與限完整 MI_00 ID 的開發配對路徑 |
| [README.md](README.md) | 真實功能狀態、執行與驗證說明 |
| [ENG.md](ENG.md) | 掃描架構、驗證策略與待確認假設 |
| [delivery-status.md](delivery-status.md) | 本文件，成果、限制與接手點 |
| [docs/hardware.md](docs/hardware.md) | 本機硬體觀測與實機診斷輸出 |
| [docs/tickets/01-diagnose.md](docs/tickets/01-diagnose.md) | 診斷驗收 |
| [docs/tickets/02-first-scan.md](docs/tickets/02-first-scan.md) | 真實掃描驗收 |
| [docs/tickets/03-recover-scan.md](docs/tickets/03-recover-scan.md) | 取消與復原驗收 |
| `docs/tickets/04-scan-app.md`（已刪除） | 依使用者指示移除自製掃描 App 工作 |
| [docs/tickets/05-windows-install.md](docs/tickets/05-windows-install.md) | Windows 整合與安裝驗收 |
| [docs/tickets/06-print.md](docs/tickets/06-print.md) | 列印驗收 |
| [docs/tickets/07-scan-tones.md](docs/tickets/07-scan-tones.md) | 彩色過曝感與灰階偏淡的調查、來源及修正驗收 |
| [driver/wc3119-winusb.inf](driver/wc3119-winusb.inf) | 僅配對 MI_00 的未簽署 INF 設計稿 |
| [driver/README.md](driver/README.md) | 安裝範圍、風險與復原要求 |

## Actions

2026-09-14 屬性相依驗證與精確取消對照依使用者要求平行實作、交叉審查，硬體操作序列執行。測試及診斷留於 Git 排除的 `artifacts/`，沒有登錄 COM／WIA、重新配對、安全設定或權限變更。必要提交及推送依既有授權，版本識別以 Git 紀錄為準。

2026-09-14 取消事件／格式版本依使用者要求平行實作與審查，沿用當次 Spark 用量限制後的 Luna 最高 effort。USB 測試全部序列執行，私有影像與失敗診斷保留在 `artifacts/`。沒有重插、配對、COM／WIA 登錄或安全設定變更。必要提交／推送依既有授權，版本識別由 Git 記錄。

2026-09-14 原生項目樹版本執行程序內 Windows COM／WIA 項目 API、離線測試、SDK C11 編譯及當次 release DLL 載入／卸載。私人 probe 僅存 `artifacts/`，沒有 USB、系統登錄、配對或服務設定變更。

2026-09-14 執行真實 USB 鎖定／診斷、灰階掃描、彩色取消與重掃，修正前後分開保存影像及匿名化驗證結果。所有私人影像只存於 Git 排除的 `artifacts/`，完成後釋放資源並確認能力查詢及三個介面狀態正常。另執行離線／DLL 測試及建置，沒有重新配對、COM／WIA 登錄或安全設定變更。Spark 當次回報用量上限後改用 Luna 最高思考強度。前版 `705f0d6` 已推送 origin/main，本輪必要提交與推送依既有授權執行，版本識別以 Git 紀錄為準。

IStiUSD 版本執行測試程序內 DLL 載入／卸載、真實 USB 獨占與 INQUIRY，完成後釋放句柄及 helper 參考。未新增系統登錄、安裝、安全設定或掃描影像。上一版本 `d75777a2c0156a93b55a1c51a10fadc5cab8daeb` 已推送 origin/main，當輪 Spark 兩次啟動後遇到用量限制，才由 Luna 接續實作／審查；後續仍依使用者要求優先 Spark。必要提交與推送依既有授權，版本識別以 Git 紀錄為準。

COM loader 版本只執行建置、離線契約測試及測試程序內的 DLL 載入／卸載，沒有操作 USB 或新增系統登錄。上一版本 `37efd782a9b638bf3dfbf89ca69f9ca1d5eb07db` 已推送 origin/main。本輪必要提交與推送依既有授權執行，版本識別以 Git 紀錄為準。

本輪數值設定版本完成 Gray75／RGB75 真實 USB 掃描與 Windows 記憶體串流輸出、讀回、釋放，私人影像只存於 Git 排除的 `artifacts/`。前後 doctor 正常，沒有系統登錄、重新配對或安全設定變更。上輪原生 COM 版本已推送 `origin/main`，commit `32b2a75a8a97d54cd57a4f3994399aca83a09de4`，當時遠端雜湊相符。本輪必要提交與推送依既有授權執行，版本識別以 Git 紀錄為準。

本輪原生 COM 轉接僅執行離線測試、測試執行緒的 COM 初始化／解除初始化、Windows 記憶體串流建立／釋放及 Rust 建置，沒有掃描或系統登錄。上輪 BMP 版本已推送 `origin/main`，commit `acb2ef66fe5c5f8864ed73493bddae40821fa0fd`，當時遠端雜湊核對相同。

BMP 串流版本當時執行掃描、取消、取消後重掃、唯讀診斷與 WIA automation 列舉，以及建置驗證，未新增系統安裝或登錄變更。原始影像、BMP 及裝置證據保留在 Git 排除的 `artifacts/`。必要提交依既有授權推送 `origin/main`，BMP 版本識別以 Git 紀錄為準。

前次掃描核心已提交並推送至 `origin/main`，commit `1bed8448b07175f59a630c8172dc8a79cc72f401`，當時已核對遠端相符。後續有限排空版本為 `8092ee9eba01c8c2bd542c46da6c28ecf590761f`；本輪診斷與連續掃描驗證的提交識別由 Git 記錄。

使用者核准後，經 Windows UAC 執行精確 MI_00 的內建 WinUSB 配對、GUID 登錄及介面重啟，全部成功。沒有重開機、執行復原或變更安全設定。備份及含私人裝置 ID 的安裝紀錄只存於 Git 排除目錄。

開發配對工具版本已推送至 `origin/main`，commit `b48330aa037b1e088ca8b6366d184d3872795abf`，推送後遠端雜湊相符。

能力查詢版本已提交並推送至 `origin/main`，commit `4d3b8d05f7beeaa0a759a7a1d1610b5ca5ac7bc1`，本輪已核對遠端相符。

初始化已提交並推送至既有 `origin/main`，commit `c59a3ce006173fb80b886cdb1532311b93fce121`，遠端分支雜湊已核對相符。使用者授權後續必要的 commit 與 push。系統安裝、正式發布及付費簽署仍須依個別授權處理。

配對之前的歷史動作僅有唯讀系統查詢、Rust 建置及診斷。本次已核准的安裝結果記於本節最上方；尚未發布正式版本。

## Source Links

- [工程設計](ENG.md)
- [硬體證據](docs/hardware.md)
- [工作項目](docs/tickets/)
- [可核對的系統安裝方案](driver/README.md)
- [SANE 1.4.0 裝置設定](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.conf.in#L236)
- [Microsoft WinUSB 安裝](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/winusb-installation)

## Handoff Notes

使用者要完整可用的 Rust 純驅動，不開發 GUI 或掃描 App，CLI 只作開發與診斷用途。不能把本機通訊成功當成原始需求完成。MI_00 已回覆有效掃描能力，機器識別為 SAMSUNG ORION，不能因不是 Xerox 字串而擅改辨識條件。本機精確配對已完成，不要重跑只適用於未綁定狀態的安裝腳本。正式套件必須另完成別台 Windows 11 x64 電腦、USB 換孔、拔插與重開機驗收。
