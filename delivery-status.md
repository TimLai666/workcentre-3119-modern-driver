# 開發狀態

## Current Phase

2026-09-13：Rust WinUSB 能力查詢已推送，新增開發機配對工具並通過唯讀候選預檢。成功通訊仍待系統配對後實測。專案為純驅動，不開發 GUI；影像取得、WIA、正式安裝與列印皆尚未實作。

## Stage Objective

先在已連接的 WorkCentre 3119 取得可驗證的真實掃描影像，再完成供既有掃描軟體使用的 Windows 驅動整合。

## Active Workstreams

- 裝置診斷已實作並實跑，尚缺拔線競態、多台裝置及權限拒絕的實機驗證。
- 已加入能力回覆解析與開發用 `inquiry` 指令，USB 成功通訊仍待實機驗證。
- 已備妥限於 MI_00 的 Rust 配對工具、系統基準備份及本機操作腳本。唯讀預檢通過，尚未執行安裝或復原。
- 使用者回報 Windows 掃描搭配原廠驅動時彩色像過曝、灰階偏淡。已建立 07 的逐段比對與驗收條件，尚未重現或修復。

## Milestones

| id | target | owner | status | verification_signal |
| --- | --- | --- | --- | --- |
| init | Rust 專案與接手文件 | 開發者 | done | 五個測試、Clippy、格式檢查、release 建置通過 |
| 01 | 唯讀診斷完整情境 | 開發者 | in_progress | 本機問題碼 28 可重現，硬體異常情境未全部驗證 |
| 02 | 第一張實機掃描 | 開發者／使用者授權系統變更 | in_progress | 能力查詢程式及合成測試已完成，實機通訊受阻，尚無能力回覆 |
| 03 | 取消與復原 | 開發者 | not_started | 尚無 |
| 05 | Windows 掃描與安裝 | 開發者 | in_progress | 開發機配對預檢通過；安裝、復原、WIA 皆未驗證 |
| 06 | 列印 | 開發者 | not_started | 尚無 |
| 07 | 掃描明暗品質 | 開發者 | blocked | 缺少原始影像，彩色過曝感與灰階偏淡尚未重現 |

## Current Blockers

- MI_00 沒有驅動，Windows 問題碼 28。安裝或手動綁定系統 WinUSB 及新增裝置 GUID，尚未取得明確授權。
- 已確認內建 WinUSB 的唯一裝置專屬候選，實際綁定仍未驗證。自訂 INF 沒有簽章 catalog，不能宣稱可安裝。
- 既有 MIT 授權保留。若選擇移植 SANE 的 GPL 實作，須先確認具體的授權與分發方案。本次沒有移植。
- 偏亮偏白問題沒有原始影像可比較，目前無法確定是取得影像、驅動轉換或呼叫端處理造成。07 的實機修正依賴 02，沒有預先加入固定壓暗處理。

## Next Verifiable Output

取得系統變更授權並完成可復原的 MI_00 配對後，Rust 讀取真正的 USB 介面、端點及 INQUIRY 能力回覆。保留匿名化回覆，核對型號、模式及解析度，再實作第一張掃描。

## Next Ticket

[02 — 第一張實機掃描](docs/tickets/02-first-scan.md)。其系統安裝部分等待授權，USB 開啟與能力查詢程式已備妥，下一步取得真實回覆。01 未驗證的硬體異常情境保留，不視為已完成。

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

開發機配對工具：24 個測試（既有 17 個及工具 7 個）、Clippy、格式檢查與 release 建置通過。Spark 實跑無效參數拒絕路徑；Luna 審查原生呼叫與本機操作腳本。已修正候選不唯一時預檢仍回傳成功的缺陷，並釐清安裝會持續修改目標綁定。原生 API 結構大小與欄位偏移通過 x64 SDK 對照測試。

實機預檢的裝置專屬與全域 CLASS 清單各有 3 個候選，均只有 1 個符合指定內建 WinUSB。備份包含父裝置／兩個介面屬性、MI_00 登錄匯出與原始 Device Parameters，存於 Git 排除的 `artifacts/winusb-baseline-20260913T043413Z-2a857f59/`。本機安裝協調腳本只執行不帶 `-Apply` 的預檢，沒有安裝紀錄。安裝成功、復原及 UAC 路徑尚未實測，不能當成正式安裝功能完成。

配對工具 release SHA256：`28A92A04E487A5F83E3A970C62483345FC8DE37E0F79A2FE47437ACD29928DE2`。

能力查詢版本先加入失敗測試，再完成解析、USB 驗證與 CLI。修正合成樣本的字串長度錯誤後，7 個能力解析測試通過；傳輸端另先重現錯誤 bulk 最大封包大小未被拒絕，再修正至測試通過。最終格式檢查、Clippy、17 個測試及 release 建置均通過。

本機 release 實跑 `doctor` 仍為 MI_00 問題碼 28、結束碼 2；`inquiry` 回報缺少已登錄 WinUSB 介面、結束碼 1，在開啟 USB 前停止。這只驗證目前缺少介面的失敗路徑，沒有取得端點、能力回覆或影像。成功通訊、實際逾時、拔線、占用及資源釋放的硬體情境仍未驗證。

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
| [Cargo.lock](Cargo.lock) | 可重現的套件鎖定檔，目前無第三方依賴 |
| [.gitignore](.gitignore) | 排除建置結果與本機實驗資料 |
| [src/lib.rs](src/lib.rs) | 精確裝置識別、診斷分類與能力查詢入口 |
| [src/windows.rs](src/windows.rs) | Windows 唯讀裝置與驅動查詢 |
| [src/usb.rs](src/usb.rs) | WinUSB 裝置核對、端點查詢及單次 INQUIRY |
| [src/protocol.rs](src/protocol.rs) | 能力回覆框架驗證與欄位解析 |
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

能力查詢版本已提交並推送至 `origin/main`，commit `4d3b8d05f7beeaa0a759a7a1d1610b5ca5ac7bc1`，本輪已核對遠端相符。

初始化已提交並推送至既有 `origin/main`，commit `c59a3ce006173fb80b886cdb1532311b93fce121`，遠端分支雜湊已核對相符。使用者授權後續必要的 commit 與 push。系統安裝、正式發布及付費簽署仍須依個別授權處理。

已執行唯讀系統查詢、Rust 建置及診斷，未安裝／移除驅動、修改安全設定或發布正式版本。

## Source Links

- [工程設計](ENG.md)
- [硬體證據](docs/hardware.md)
- [工作項目](docs/tickets/)
- [可核對的系統安裝方案](driver/README.md)
- [SANE 1.4.0 裝置設定](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.conf.in#L236)
- [Microsoft WinUSB 安裝](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/winusb-installation)

## Handoff Notes

使用者要完整可用的 Rust 純驅動，不開發 GUI 或掃描 App，CLI 只作開發與診斷用途。不能把本次初始化當成原始需求完成。MI_00 掃描角色是依本機剩餘介面與 USB 類別推論，下一步以真實能力回覆確認。端點、影像行序與能力仍未知。系統驅動綁定授權仍待確認，本次範圍修正不構成安裝授權。
