# 開發狀態

## Current Phase

2026-09-13：初始化與本機唯讀診斷已完成。使用者已確認專案為純驅動，不開發 GUI。完整驅動尚未完成，掃描核心、WIA、正式安裝與列印皆尚未實作。

## Stage Objective

先在已連接的 WorkCentre 3119 取得可驗證的真實掃描影像，再完成供既有掃描軟體使用的 Windows 驅動整合。

## Active Workstreams

- 裝置診斷已實作並實跑，尚缺拔線競態、多台裝置及權限拒絕的實機驗證。
- 已備妥限於 MI_00 的 WinUSB 安裝方案與 INF 設計稿，等待系統變更授權。
- 使用者回報 Windows 掃描搭配原廠驅動時彩色像過曝、灰階偏淡。已建立 07 的逐段比對與驗收條件，尚未重現或修復。

## Milestones

| id | target | owner | status | verification_signal |
| --- | --- | --- | --- | --- |
| init | Rust 專案與接手文件 | 開發者 | done | 五個測試、Clippy、格式檢查、release 建置通過 |
| 01 | 唯讀診斷完整情境 | 開發者 | in_progress | 本機問題碼 28 可重現，硬體異常情境未全部驗證 |
| 02 | 第一張實機掃描 | 開發者／使用者授權系統變更 | blocked | 沒有 USB 傳輸驅動及能力回覆 |
| 03 | 取消與復原 | 開發者 | not_started | 尚無 |
| 05 | Windows 掃描與安裝 | 開發者 | not_started | 尚無 |
| 06 | 列印 | 開發者 | not_started | 尚無 |
| 07 | 掃描明暗品質 | 開發者 | blocked | 缺少原始影像，彩色過曝感與灰階偏淡尚未重現 |

## Current Blockers

- MI_00 沒有驅動，Windows 問題碼 28。安裝或手動綁定系統 WinUSB 及新增裝置 GUID，尚未取得明確授權。
- WinUSB 開發綁定方式尚未在本機驗證。自訂 INF 沒有簽章 catalog，不能宣稱可安裝。
- 既有 MIT 授權保留。若選擇移植 SANE 的 GPL 實作，須先確認具體的授權與分發方案。本次沒有移植。
- 偏亮偏白問題沒有原始影像可比較，目前無法確定是取得影像、驅動轉換或呼叫端處理造成。07 的實機修正依賴 02，沒有預先加入固定壓暗處理。

## Next Verifiable Output

取得系統變更授權並完成可復原的 MI_00 配對後，Rust 讀取真正的 USB 介面、端點及 INQUIRY 能力回覆。保留匿名化回覆，核對型號、模式及解析度，再實作第一張掃描。

## Next Ticket

[02 — 第一張實機掃描](docs/tickets/02-first-scan.md)。其系統安裝部分等待授權，USB 開啟與能力查詢程式可接續開發。01 未驗證的硬體異常情境保留，不視為已完成。

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

## Verified

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

release 執行檔 SHA256：`14A779EDC22632988CB097553A58DCFA8AF1F30581DF8BC1B211DCFEF29D0907`。

工具在受限環境出現 home 路徑 canonicalize 與 Git 全域 ignore 存取警告，未使上述最終建置及測試失敗。沒有實際掃描、影像品質或 WIA 測試結果。

## Changed

| 檔案 | 變更摘要 |
| --- | --- |
| [AGENTS.md](AGENTS.md) | 專案規則、硬體限制與交付要求 |
| [CLAUDE.md](CLAUDE.md) | AGENTS.md 入口指標 |
| [Cargo.toml](Cargo.toml) | Rust 套件與檢查規則 |
| [Cargo.lock](Cargo.lock) | 可重現的套件鎖定檔，目前無第三方依賴 |
| [.gitignore](.gitignore) | 排除建置結果與本機實驗資料 |
| [src/lib.rs](src/lib.rs) | 精確裝置識別與診斷分類 |
| [src/windows.rs](src/windows.rs) | Windows 唯讀裝置與驅動查詢 |
| [src/main.rs](src/main.rs) | 繁體中文 doctor 指令 |
| [tests/device.rs](tests/device.rs) | 裝置與狀態測試 |
| [tests/cli.rs](tests/cli.rs) | CLI 行程測試 |
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

唯讀查詢 Windows 系統與 USB 裝置，執行 Rust 建置及診斷。沒有安裝／移除驅動、修改安全設定、對外發送、建立 commit、推送或發布。

## Source Links

- [工程設計](ENG.md)
- [硬體證據](docs/hardware.md)
- [工作項目](docs/tickets/)
- [可核對的系統安裝方案](driver/README.md)
- [SANE 1.4.0 裝置設定](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.conf.in#L236)
- [Microsoft WinUSB 安裝](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/winusb-installation)

## Handoff Notes

使用者要完整可用的 Rust 純驅動，不開發 GUI 或掃描 App，CLI 只作開發與診斷用途。不能把本次初始化當成原始需求完成。MI_00 掃描角色是依本機剩餘介面與 USB 類別推論，下一步以真實能力回覆確認。端點、影像行序與能力仍未知。系統驅動綁定授權仍待確認，本次範圍修正不構成安裝授權。
