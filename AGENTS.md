# WorkCentre 3119 開發規則

## 目標與入口

以 Rust 開發 Xerox WorkCentre 3119 的 Windows 11 x64 純驅動，優先完成掃描。完整交付包含實機掃描、Windows 掃描整合、安裝與解除安裝，接續完成列印。使用者透過 Windows 掃描等既有軟體操作，本專案不開發 GUI 或掃描 App。診斷工具、模擬測試或單次掃描成功，都不能當成完整驅動已完成。

- 接手先讀 `delivery-status.md`，確認目前成果、阻礙與下一個驗收項目。
- 修改架構、USB 存取、Windows 整合或測試策略前讀 `ENG.md`。
- 操作硬體前讀 `docs/hardware.md`，再重新偵測當下裝置。這份文件是一次觀測，不能取代即時檢查。
- 執行功能前讀 `docs/tickets/` 對應項目與必要前置條件，完成後更新驗收證據及 `delivery-status.md`。
- 修改影像轉換、亮度、對比或掃描預設值前，讀 `docs/tickets/07-scan-tones.md`，沿用偏亮偏白問題的證據與驗收條件。
- 2026-09-26 使用者要求暫停歷史偏白問題，不再將 07 列為目前交付阻礙，也不為偏白調查要求放置原稿。既有 WIA 軟體亮度／對比及中性值保持不變，後續優先掃描穩定性、取消復原、換孔與跨電腦驗收。
- 更動系統驅動前讀 `driver/README.md`。2026-09-13 使用者已核准本機 MI_00 配對內建 WinUSB、登錄裝置介面 GUID 及重新啟動該介面，限已備份並核對的目標。2026-09-19 使用者另核准在本開發機安裝 WDK、信任專案測試憑證並以 `driver/wc3119-setup.ps1` 安裝／更新／解除安裝 WIA 套件；每次更新須提高 INF `DriverVer`，腳本會重啟 stisvc。不花錢：不買 EV 憑證、不送 attestation。

## 開發與驗證

- 根目錄是 Rust Cargo 專案。`src/lib.rs` 負責公開 API、裝置識別與診斷分類，`src/windows.rs` 封裝 Windows 唯讀 API，`src/usb.rs` 封裝 WinUSB session（句柄以讀寫共用開啟，WinUSB 本身每台裝置只允許一個開啟中的句柄，驅動物件從第一次開啟保留到 Release），`src/protocol.rs` 驗證能力回覆，`src/scan.rs` 管理掃描工作及影像解碼，`src/bitmap.rs` 將影像塊編碼成有限記憶體的 BMP 串流。`src/main.rs` 提供 `wc3119 doctor` 與 `wc3119 inquiry`。
- Rust 核心提供硬體控制、影像資料傳輸與 Windows 驅動整合。預覽畫面、影像編輯、PDF 組頁及儲存操作由呼叫端軟體負責，CLI 僅作為開發、診斷與測試工具。
- 忠實重現掃描明暗及正確映射 WIA 亮度／對比屬於驅動責任。目前 WIA 亮度／對比（−1000..1000）由 `src/wia.rs` 的 `Tone` 在解碼後以單一查表套用，中性 0 不改任何像素；不得再加第二層轉換。使用者回報原廠驅動掃描偏亮偏白，原因尚未確認。不得預設壓暗整張影像、強制去背或套用固定 gamma 曲線充當修復。
- 原生 API 的 `unsafe` 必須限縮在封裝內，註明指標、長度、生命週期及資源釋放的依據。
- `src/com_stream.rs` 封裝原生 IStream 輸出及參考釋放，供 BMP 編碼使用。更動後執行 `cargo test --offline --test com_stream` 與 doc-tests，驗證 Windows 真實記憶體串流、錯誤及執行緒限制；這些測試不登錄 WIA 或操作 USB。
- `src/wia.rs` 驗證 WIA 純數值設定並轉成掃描要求，`scan_bmp` 會啟動真實 USB 掃描及輸出 BMP。`cargo test --offline --test wia` 只驗證設定與預先取消，不操作硬體。正式屬性同步與剩餘 WIA 服務整合由 05 接續實作。
- `src/wia_callback.rs` 封裝原生 WIA callback，`src/wia_transfer.rs` 接上同一 session 的 BMP、進度、取消與原始錯誤。更動後跑 `cargo test --offline --test wia_callback` 與 `cargo test --offline --lib wia_transfer::tests`。`tests/sti.rs` 的 `actual_callback_transfer_scans_cancels_and_rescans` 需當下 `WC3119_TEST_STI_PATH` 與不存在的 `WC3119_TEST_OUTPUT_DIR`，必須單獨指定 `--ignored --exact` 執行。它使用合成 callback 與真正 Windows IStream、實機 USB，不代表 WIA 服務或 Windows 掃描驗收。END_OF_STREAM／END_OF_TRANSFER 由 WIA 服務發送，驅動不可手動發送。
- `src/com_server.rs` 提供 DLL 載入入口、class factory 與 COM 物件生命週期，支援 COM aggregation（WIA 服務以 outer IUnknown 建立 USD，只接受 IID_IUnknown）。`src/com_server/trace.rs` 在 `catch_hresult` 攔到 panic 時寫一行到 `%SystemRoot%\debug\WIA\wc3119-driver.log`，cargo 測試程序不寫。服務端問題先看 `C:\Windows\debug\WIA\wiatrace.log`。更動後執行 `cargo test --offline --test com_server`，再依 `ENG.md` 的 DLL 驗證指令載入當次 release 建置。該動態載入測試預設 ignored，交付前必須另行執行，不能把預設測試通過當成 DLL 已驗證。這些測試不登錄 WIA 或操作 USB。
- 新核心功能採 TDD，先驗證測試會失敗，再實作。模擬封包須明示為合成資料，實機資料須記錄取得方式。
- `src/scan/hardware.rs` 的 `scan::hardware::actual_cancel_phase_then_same_session_rescan` 是精確階段取消對照，僅在測試建置啟用。指定當下 `WC3119_TEST_STI_PATH`、不存在的 `WC3119_TEST_OUTPUT_DIR` 及 `WC3119_TEST_CANCEL_PHASE=first_band`、`metadata_busy` 或 `metadata_ready_after_busy`，逐一以 `cargo test --offline --lib scan::hardware::actual_cancel_phase_then_same_session_rescan -- --ignored --exact` 執行。第三種階段在合法 Busy 記錄 pending，第一個可解析的 metadata Good 才設取消旗標，須確認零影像塊交付。它會啟動全平台 RGB75，取消後只有連線健康狀態允許才用同一 session 重掃；只保存數值診斷，不保存像素，不代表影像品質或 WIA 服務驗收。禁止與其他硬體測試平行執行；未產生 `complete.txt` 即未通過整組驗收。
- `src/com_server/minidrv.rs` 提供 IWiaMiniDrv 身分與多用戶端生命週期，`minidrv/tree.rs` 使用 Windows 原生根／平台項目。`locking.rs` 經服務的 IStiDevice 鎖定／解鎖及查詢同一 STI session 的能力；`properties.rs` 初始化／讀取真正服務 context，`properties/catalog.rs` 建立當次能力的初始值與範圍，`properties/native.rs` 封裝原生初始化／差異寫入，`properties/validation.rs` 處理相依設定，`properties/validation_entry.rs` 讀取服務的新舊值並驗證明確寫入集合，`properties/read_entry.rs` 在應用程式讀取根項目狀態時以同一 STI session 的 INQUIRY 更新 FLAT_READY，`errors.rs` 把驅動回報的裝置錯誤值轉成呼叫端釋放的說明字串，`capabilities.rs` 以程序生命週期的靜態表宣告 WIA_CMD_SYNCHRONIZE 與裝置連線／斷線事件並處理 drvDeviceCommand。`acquire.rs` 接上同一 STI 連線的串流傳輸，`cancel.rs` 通知相符的作用中工作，`formats.rs` 列舉 BMP。更動後跑 `cargo test --offline --lib com_server::minidrv::`、COM 介面測試及當次 release DLL 動態測試。一般測試不操作 USB、登錄系統或偽造 WIA context。屬性相依更新、讀取通知、錯誤字串、能力列舉與 STI_GENCAP_WIA 已接上；實際服務驗收仍未完成。初始化／相依寫入失敗會隔離整個 COM 物件，不能在同一物件重試初始化；須由服務釋放並建立新物件。項目樹 getter 的參考所有權須逐 API 核對，不能一律套用 QI 的 owned 規則。
- `actual_wia_lock_queries_live_capabilities_without_scan` 是額外的 ignored 硬體測試，以當下 `WC3119_TEST_STI_PATH` 驗證服務鎖定 → 同一 STI session 能力查詢 → 解鎖，不啟動掃描、不登錄系統；須單獨指定 `--ignored --exact`，不能當成原生屬性寫入的服務驗收。
- `minidrv/locking/hardware.rs` 的 `actual_wia_lock_routes_through_sti_and_releases` 只鎖定／INQUIRY；`actual_wia_dispatch_scans_cancels_and_rescans` 另啟動灰階、彩色回呼取消及重掃。`actual_wia_dispatch_cancels_from_parallel_thread_and_rescans` 從閒置裝置啟動彩色掃描，500 ms 後由純 Rust 旗標取消，再做彩色重掃與灰階對照；先讀 03／硬體紀錄，確認閒置前提，不能把前一張剛完成時的 RESERVE Busy 誤當首塊 metadata 取消。必須重新列舉 `WC3119_TEST_STI_PATH`，掃描另指定不存在的 `WC3119_TEST_OUTPUT_DIR`，所有硬體測試逐一以 `--ignored --exact` 執行。服務 helper、屬性快照與 callback 為合成測試資料，USB、Windows 項目與 IStream 為實際資源，不能當成 Windows 掃描驗收。
- `src/com_server/sti.rs` 提供 IStiUSD，`session.rs` 管理連線借用與異常隔離。更動後跑 `cargo test --offline --test sti`。硬體測試預設 ignored，必須個別指定名稱及 `--ignored --exact`，不可平行執行。`actual_sti_device_lock_presence_and_release` 使用重新列舉的 `WC3119_TEST_STI_PATH`，只做鎖定／INQUIRY。`actual_locked_object_scans_cancels_and_rescans_with_reentrant_output` 另須不存在的 `WC3119_TEST_OUTPUT_DIR`，會啟動灰階、彩色取消及彩色重掃。兩者皆不修改登錄，也不代表 WIA 服務的 port name、存取權或 COM 影像傳輸已驗證。
- 每次交付執行 `cargo fmt --all -- --check`、`cargo clippy --offline --all-targets -- -D warnings`、`cargo test --offline`、`cargo build --offline --release`。新增依賴後先完成抓取再離線驗證。
- 更動安裝流程另跑 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File driver/tests/setup-recovery.tests.ps1`；更動打包或 C 執行階段連結另跑 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File driver/tests/package-runtime.tests.ps1`。兩者只載入指定函式並使用合成輸入，不操作真實服務或驅動；實際套件仍須通過 staged DLL 匯入檢查、release DLL 動態載入及已授權的本機更新驗收。
- 掃描改動另跑 `cargo test --offline --example capture_scan` 與 `cargo build --offline --release --example capture_scan`。此範例帶新目錄、模式及 DPI 會啟動掃描，不帶參數只顯示說明。成功必須有 `complete.txt`，私人 USB 影像僅存 `artifacts/`。
- `examples/scan_stability.rs` 在同一程序連續測試 600 dpi 彩色與對照模式，不保存影像，任一錯誤即停止。修改後另跑 `cargo test --offline --example scan_stability` 與 `cargo build --offline --release --example scan_stability`；傳入新目錄才會操作硬體。20 次完成標記及外部記憶體量測只是可靠性證據，不能替代文件品質、WIA 或斷線復原驗收。
- `examples/winusb_setup.rs` 是開發機配對工具。更動後另跑 `cargo test --offline --example winusb_setup` 與 `cargo build --offline --release --example winusb_setup`。不帶參數僅預檢，帶 `--install-mi00` 才會修改系統，執行前須依 `driver/README.md` 核對授權、備份與復原範圍。
- 硬體存取改動另執行實機測試。掃描交付須保留匿名化測試紀錄與實際影像檢查結果，不把測試樣本、序號或使用者文件提交到 Git。
- 每次修改先讀取檔案，使用原文比對的補丁。整檔工具改寫前確認內容雜湊沒有變動，遇到其他人的變更先重新檢查。
- 紀錄完成、部分完成、未驗證與受阻狀態。功能完成後同步更新文件，不留下未實作卻宣稱可用的選項。

## 硬體與系統限制

- 硬體 USB ID 為 `0924:4265`。掃描介面為 `USB\VID_0924&PID_4265&MI_00`。列印為 `MI_01`，父裝置使用 `usbccgp`。
- 驅動配對必須精確包含 `MI_00`。不得用只有 VID/PID 的規則綁定掃描驅動，避免接管父裝置及列印。
- 所有 USB 端點、解析度、掃描範圍與影像行序都必須讀取實際描述與能力回應，不能從其他型號推定。
- 禁止為了讓測試通過而預設送 USB reset / CLEAR_HALT，或任意寫入 EEPROM、韌體與未查證的廠商控制命令。
- 取消、逾時、拔線、暖機、占用與重新掃描都需驗證。失敗時釋放資源，禁止無限重試。
- 安裝、移除或強制綁定系統驅動，登錄 COM/WIA，修改系統登錄與安全設定，必須先取得使用者針對具體動作的授權。
- 不得以停用 Secure Boot、簽章驗證、防毒或記憶體完整性作為一般安裝步驟。需要付費簽署或對外提交前另外確認。

## 來源與授權

- 原始專案 `LICENSE` 為 MIT。本次診斷程式為原創 Rust，維持原授權。
- SANE `xerox_mfp` 上游是 GPL + SANE exception。查閱協定事實不等於可以把上游程式逐行翻譯後改標 MIT。
- 複製、移植或連結第三方實作前，記錄來源、固定版本、授權與分發方式。若涉及改變既有授權，先提出具體方案供使用者確認。
- 技術結論附一手來源，硬體結論附實機證據，未知的相容性明確標示。

## 交付與溝通

以台灣繁體中文簡短回報成果、限制及待決策事項。列出實際使用的 Skills。使用者已授權本專案必要的 commit 與 push，可在驗證後自行提交及推送至既有遠端，不需逐次確認。不得強制推送或覆蓋他人變更。本機 MI_00 配對另已獲上述具體授權，不包含付費簽署或正式版本發布。架構與功能狀態存入上述專案文件，不寫入個人記憶。

提交訊息預設使用英文 Conventional Commits，例如 `feat(usb): add scanner inquiry transport`。

使用者指定子代理優先使用 `gpt-5.3-codex-spark`，Spark 無法妥善完成或當下因用量限制不可用時，才交給 `gpt-5.6-luna`。effort 設為該模型支援的最高值，目前分別為 `xhigh` 與 `max`。本專案已成功啟動這兩種子代理，不能只因工具清單未列出 Spark 就判定不可用。用量限制以當次工具結果為準，不能把一次受限當成永久不可用。
