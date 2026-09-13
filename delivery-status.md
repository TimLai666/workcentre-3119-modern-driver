# 開發狀態

## Current Phase

2026-09-13：灰階與彩色皆已完成 600 dpi 實機傳輸。新增已知資料長度的有限排空，早期／進行中定時取消及其後重掃成功。專案為純驅動，不開發 GUI；文件品質、完整異常復原、WIA、正式套件及列印仍未完成。

## Stage Objective

先在已連接的 WorkCentre 3119 取得可驗證的真實掃描影像，再完成供既有掃描軟體使用的 Windows 驅動整合。

## Active Workstreams

- 裝置診斷已實作並實跑，尚缺拔線競態、多台裝置及權限拒絕的實機驗證。
- Rust 已完成 Gray75、RGB75、RGB300、Gray600、RGB600 空平台傳輸。能力與影像來自同一個獨占 session。
- 本機 MI_00 綁定、GUID 登錄及介面重啟成功，不需重開機，父裝置與 MI_01 符合原始備份；復原未執行。
- 使用者回報 Windows 掃描搭配原廠驅動時彩色像過曝、灰階偏淡。已建立 07 的逐段比對與驗收條件，尚未重現或修復。

## Milestones

| id | target | owner | status | verification_signal |
| --- | --- | --- | --- | --- |
| init | Rust 專案與接手文件 | 開發者 | done | 五個測試、Clippy、格式檢查、release 建置通過 |
| 01 | 唯讀診斷完整情境 | 開發者 | in_progress | 本機問題碼 28 可重現，硬體異常情境未全部驗證 |
| 02 | 第一張實機掃描 | 開發者 | in_progress | 空平台灰階／彩色及獨立像素比對成功；文件、色彩及精確幾何待驗收 |
| 03 | 取消與復原 | 開發者 | in_progress | 已知長度有限排空通過合成測試；實機早期／進行中取消、重掃與互斥成功；失同步復原未完成 |
| 05 | Windows 掃描與安裝 | 開發者 | in_progress | 本機開發配對成功；完整套件、復原、WIA 與跨電腦／換孔未驗證 |
| 06 | 列印 | 開發者 | not_started | 尚無 |
| 07 | 掃描明暗品質 | 開發者 | blocked | 已有空平台影像，缺少可對照原稿；歷史偏白仍未重現 |

## Current Blockers

- 使用者確認平台沒有文件，無法完成文字、色彩、精確幾何與淺色細節驗收。取消／復原與 WIA 的獨立開發可繼續。
- 原始影像傳輸中斷若失去框架同步，目前僅釋放 OS 資源並回報需重插，沒有證明可直接重掃。正式整合前須完成重連狀態與異常復原，並驗證暖機、拔線、睡眠及 20 次連續掃描。
- 開發機配對不等於正式套件。自訂 INF 沒有簽章 catalog，跨電腦安裝、換孔與 WIA 尚未驗證。
- 既有 MIT 授權保留。若選擇移植 SANE 的 GPL 實作，須先確認具體的授權與分發方案。本次沒有移植。
- 偏亮偏白問題缺少有內容的原稿可比較，目前無法確定原因。USB 與解碼數值已比對相同，沒有加入固定壓暗處理。

## Next Verifiable Output

補完掃描中的取消及異常後重連狀態，驗證後續新工作不會讀到舊資料。平台有文件後補做 02／07 的文字、色彩、精確幾何及淺色細節對照；WIA 另依 05 接續。

## Next Ticket

[03 — 取消與復原](docs/tickets/03-recover-scan.md)。02 的文件驗收目前缺少原稿，保留進行中；已確認的取消與重掃不等於傳輸中斷可復原。01 未驗證的硬體異常情境亦保留。

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

## Verified

本次最終程式驗證：格式檢查、Clippy（all targets，warnings 為錯誤）、47 個 all-targets 測試、一般測試含 doc-tests 與核心／擷取範例 release 建置全部通過。擷取範例 release SHA256：`74848DAF2F002A4A3665DCD0DAE044B97018C2FEC963AD296057474F53863775`。新增測試先失敗再實作；範例 help 已實跑核對定時取消、預設值、錯誤及用法。開發配對工具未修改／重跑安裝。

最新實機證據：RGB600 5100×6961、117 塊、106503300 bytes，94.951 秒完成；獨立逐像素對照與 USB 資料一致。100 ms 早期取消及 8000 ms 進行中取消皆回傳失敗、不產生完成標記，隨後 Gray75 重掃成功。沒有注入實機 USB 錯誤，也沒有驗收文件品質。Luna 獨立審查本次有限排空未發現新增的傳輸重試缺陷；失同步後 API 尚未強制阻擋下一個工作是既有缺口，持續追蹤於 [03](docs/tickets/03-recover-scan.md)。

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
| [Cargo.lock](Cargo.lock) | 可重現的套件鎖定檔，目前無第三方依賴 |
| [.gitignore](.gitignore) | 排除建置結果與本機實驗資料 |
| [src/lib.rs](src/lib.rs) | 精確裝置識別、診斷分類與能力查詢入口 |
| [src/windows.rs](src/windows.rs) | Windows 唯讀裝置與驅動查詢 |
| [src/usb.rs](src/usb.rs) | WinUSB 裝置核對、端點查詢及單次 INQUIRY |
| [src/protocol.rs](src/protocol.rs) | 能力回覆框架驗證與欄位解析 |
| [src/scan.rs](src/scan.rs) | 掃描工作、影像解碼與已知長度的有限排空，保留取消原因 |
| [examples/capture_scan.rs](examples/capture_scan.rs) | 私人實機證據擷取、定時／塊後取消與完成標記 |
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

本次只執行掃描、取消、唯讀診斷及建置驗證，未新增系統安裝或登錄變更。原始影像與裝置證據保留在 Git 排除的 `artifacts/`。

前次掃描核心已提交並推送至 `origin/main`，commit `1bed8448b07175f59a630c8172dc8a79cc72f401`，當時已核對遠端相符。本次有限排空與定時取消的提交識別由 Git 記錄。

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
