# WorkCentre 3119 開發規則

## 目標與入口

以 Rust 開發 Xerox WorkCentre 3119 的 Windows 11 x64 純驅動，優先完成掃描。完整交付包含實機掃描、Windows 掃描整合、安裝與解除安裝，接續完成列印。使用者透過 Windows 掃描等既有軟體操作，本專案不開發 GUI 或掃描 App。診斷工具、模擬測試或單次掃描成功，都不能當成完整驅動已完成。

- 接手先讀 `delivery-status.md`，確認目前成果、阻礙與下一個驗收項目。
- 修改架構、USB 存取、Windows 整合或測試策略前讀 `ENG.md`。
- 操作硬體前讀 `docs/hardware.md`，再重新偵測當下裝置。這份文件是一次觀測，不能取代即時檢查。
- 執行功能前讀 `docs/tickets/` 對應項目與必要前置條件，完成後更新驗收證據及 `delivery-status.md`。
- 修改影像轉換、亮度、對比或掃描預設值前，讀 `docs/tickets/07-scan-tones.md`，沿用偏亮偏白問題的證據與驗收條件。
- 更動系統驅動前讀 `driver/README.md`。2026-09-13 使用者已核准本機 MI_00 配對內建 WinUSB、登錄裝置介面 GUID 及重新啟動該介面，限已備份並核對的目標。

## 開發與驗證

- 根目錄是 Rust Cargo 專案。`src/lib.rs` 負責公開 API、裝置識別與診斷分類，`src/windows.rs` 封裝 Windows 唯讀 API，`src/usb.rs` 封裝獨占 WinUSB session，`src/protocol.rs` 驗證能力回覆，`src/scan.rs` 管理掃描工作及影像解碼。`src/main.rs` 提供 `wc3119 doctor` 與 `wc3119 inquiry`。
- Rust 核心提供硬體控制、影像資料傳輸與 Windows 驅動整合。預覽畫面、影像編輯、PDF 組頁及儲存操作由呼叫端軟體負責，CLI 僅作為開發、診斷與測試工具。
- 忠實重現掃描明暗及正確映射 WIA 亮度／對比屬於驅動責任。使用者回報原廠驅動掃描偏亮偏白，原因尚未確認。不得預設壓暗整張影像、強制去背或套用固定 gamma 曲線充當修復。
- 原生 API 的 `unsafe` 必須限縮在封裝內，註明指標、長度、生命週期及資源釋放的依據。
- 新核心功能採 TDD，先驗證測試會失敗，再實作。模擬封包須明示為合成資料，實機資料須記錄取得方式。
- 每次交付執行 `cargo fmt --all -- --check`、`cargo clippy --offline --all-targets -- -D warnings`、`cargo test --offline`、`cargo build --offline --release`。新增依賴後先完成抓取再離線驗證。
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

使用者指定子代理分工：簡單驗證用 `gpt-5.3-codex-spark`，較複雜的實作與審查用 `gpt-5.6-luna`，effort 設為該模型支援的最高值，目前分別為 `xhigh` 與 `max`。本專案已成功啟動這兩種子代理，不能只因工具清單未列出 Spark 就判定不可用。
