# WorkCentre 3119 Modern Driver

以 Rust 開發 Xerox WorkCentre 3119 的 Windows 11 x64 純驅動，優先支援掃描。使用 Windows 掃描等既有軟體操作，不另外開發 GUI 或掃描 App。

**目前是開發初期，尚不能掃描或列印。** 工具可檢查 USB 驅動狀態，另已加入 WinUSB 能力查詢程式。後者仍待掃描介面配對後完成實機驗證。

## 執行診斷

需要 Rust stable MSVC 工具鏈及 Visual Studio C++ 建置工具。診斷程式沒有第三方 Rust 依賴。

```powershell
cargo run --offline -- doctor
```

也可執行 `cargo build --offline --release` 後使用 `target\release\wc3119.exe doctor`。它不會安裝驅動、修改登錄或啟動掃描。

`doctor` 結束碼：0 表示唯一掃描介面的 WinUSB 驅動已啟動，2 表示裝置未就緒，1 表示查詢失敗，64 表示參數錯誤。0 不代表已驗證掃描功能。`--help` 成功也回傳 0。

## 開發用能力查詢

依 [安裝方案](driver/README.md) 完成 MI_00 的 WinUSB 配對及裝置介面登錄後，可執行：

```powershell
cargo run --offline -- inquiry
```

程式核對唯一裝置、USB 描述與端點後，只送出 INQUIRY 能力查詢，不啟動掃描。成功時列出機器回報的型號、解析度、模式及範圍，回傳 0。找不到已登錄介面、存取失敗、逾時或回覆不合法時回傳 1，不會安裝驅動或自動重試。能力回報不代表掃描及影像品質已驗證。

## 完整交付目標

- 真實平台掃描，依機器能力提供灰階、彩色與解析度設定。
- 調查並修正使用者回報的偏亮偏白問題，以保留淺色細節及正確的亮度／對比映射驗收，目前尚未確認原因或完成修復。
- 透過 Windows 掃描介面提供影像傳輸、取消、重掃、進度與錯誤回報。
- Windows 掃描整合與可安裝、更新、解除安裝的套件。
- 掃描穩定後完成列印協定與 Windows 列印整合。

所有項目需要實機驗證，進度見 [delivery-status.md](delivery-status.md)。本機已確認的裝置資訊見 [硬體紀錄](docs/hardware.md)，系統變更提案見 [掃描介面安裝方案](driver/README.md)。

預覽畫面、影像編輯、PDF 組頁與檔案儲存由呼叫端軟體負責。專案內的 CLI 僅用於開發、診斷及測試。

## 開發

先讀 [AGENTS.md](AGENTS.md) 與 [ENG.md](ENG.md)。

```powershell
cargo fmt --all -- --check
cargo clippy --offline --all-targets -- -D warnings
cargo test --offline
cargo build --offline --release
```

本專案目前的原創程式採 MIT。SANE 上游的授權不同，尚未把其實作移植進本專案。這是獨立開發專案，並非 Xerox 官方驅動。
