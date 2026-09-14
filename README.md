# WorkCentre 3119 Modern Driver

以 Rust 開發 Xerox WorkCentre 3119 的 Windows 11 x64 純驅動，優先支援掃描。使用 Windows 掃描等既有軟體操作，不另外開發 GUI 或掃描 App。

**Rust 核心已取得實機灰階／彩色影像，目前仍不是完整可安裝的驅動。** 已驗證空平台掃描、取消後重掃及跨行程互斥；文字、色彩、精確幾何與偏白問題尚未驗收。Windows 掃描整合、正式安裝套件、跨電腦／換孔及列印仍未完成。

Windows 整合已具備 BMP 編碼、原生 COM 影像串流轉接及 WIA 數值設定映射。原生 DLL 已通過載入、建立物件及卸載測試，IStiUSD 已驗證初始化、實機獨占鎖定、能力查詢與釋放。原生 WIA callback 消費端已透過測試回呼取得 Windows 記憶體串流，完成同一鎖定物件的灰階掃描、彩色取消及彩色重掃。回呼期間再次掃描或解鎖會回報忙碌，完成進度只在影像及清理成功後發送。IWiaMiniDrv、屬性與服務整合仍未完成，Windows 掃描目前不能使用本核心，詳見 [整合進度](docs/tickets/05-windows-install.md)。

`MI_00` 表示複合式 USB 裝置的第 0 個功能介面，數字取自裝置描述，與電腦上的 USB 接孔編號無關。3119 的 MI_00 已回覆掃描能力，MI_01 使用列印傳輸服務。正式套件會依型號與功能介面辨識，不以開發機的孔位或完整實例路徑限定使用。[Microsoft USB 識別碼定義](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/standard-usb-identifiers)

## 解析度與模式

[Xerox 官方規格](https://www.office.xerox.com/latest/W31BR-01.PDF)標示光學最高 600 × 2400 dpi、插值最高 4800 dpi，模式含 1-bit 黑白線稿／半色調、8-bit 灰階及 24-bit 彩色。插值會增加輸出像素，不能當成新增的光學細節。

目前此機器的 INQUIRY 已辨識回報為 75、100、150、200、300、600 dpi，本核心以相同 X／Y 解析度設定，僅開放灰階與彩色。兩者皆已完成 600 dpi 空平台傳輸；黑白線稿、半色調與非對稱 600 × 2400 dpi 尚未實作／驗證。不能以當前協定回報推論整台機器的完整上限。

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

## 開發用影像擷取

本機已配對的 MI_00 可透過 Rust `scan::scan_to` 回傳無壓縮影像。以下範例會啟動完整平台掃描，輸出目錄必須不存在：

```powershell
cargo run --offline --release --example capture_scan -- artifacts/my-scan gray 75
```

可選 `gray`／`rgb`，解析度接受 75、100、150、200、300、600，並以機器當次回報再次限制。已實機驗證的組合見 [硬體紀錄](docs/hardware.md)，不能把可接受參數都當成已驗證。加 `--cancel-after-band` 可測第一塊傳輸後取消，或用 `--cancel-after-ms N` 在 1–120000 毫秒後提出取消；兩者不可同時使用。取消預期回傳失敗且不產生完成標記。詳見範例的 `--help`。

目錄保存 USB 原文、解碼像素、PGM／PPM 及分塊產生的 `image.bmp`，供獨立格式比對。只有掃描釋放與檔案同步成功才有 `complete.txt`，其餘目錄視為中斷資料。影像依 READ 實際尺寸保存，沒有自動提亮、gamma、裁切或幾何補償。BMP 只改 RGB 通道排列及每列補齊，不改樣本值。檔案可能包含私人文件，`artifacts/` 不提交至 Git。這是開發驗證範例，Windows 掃描尚不能使用此核心。

## 開發用連續掃描驗證

```powershell
cargo run --offline --release --example scan_stability -- artifacts/stability 20
```

會在同一程序依序重複「600 dpi 彩色、600 dpi 彩色、300 dpi 彩色、600 dpi 灰階」，每次重新取得裝置能力。次數預設 20，接受 1–20；輸出目錄必須不存在。逐塊核對 USB 資料與解碼像素，只保存進度及錯誤的 `diagnostics.log`，不保存影像。任一錯誤立即停止，所有工作成功才有 `complete.txt`。這是開發測試，不提供自動復原或 Windows 掃描整合；記憶體使用須另外從同一程序量測。詳見範例 `--help`。

可加 `--read-poll-ms N`，以 1–1000 毫秒測試 READ 忙碌回覆的詢問間隔，預設 100。例如 `scan_stability artifacts/poll500 1 --read-poll-ms 500`。`--read-buffer-kib N` 可測試每次影像讀取的緩衝區大小，接受 1–1024 KiB，預設 64，另以當下 WinUSB 回報上限限制。範例：`scan_stability artifacts/buffer256 1 --read-buffer-kib 256`。兩個選項接在目錄及可選次數之後，順序不限，各只能使用一次。

這些是開發用對照參數，建議一次只改一個。較長的詢問間隔或較大讀取量可能增加取消等待。其他命令、120 秒工作期限、USB 政策及影像設定不變，尚未證實的設定不作為正式加速預設。

每次成功或失敗都記錄階段耗時、USB 呼叫耗時及 Busy 等待。USB／Busy 時間已包含在階段時間中，不可相加；USB 呼叫包含等待機器產生資料，不能拿它當純 USB 頻寬。呼叫端耗時在此包含獨立像素核對與寫入診斷，沒有 WIA 或影像檔案輸出。

## 完整交付目標

- 真實平台掃描，依機器能力提供灰階、彩色與解析度設定。
- 調查並修正使用者回報的偏亮偏白問題，以保留淺色細節及正確的亮度／對比映射驗收，目前尚未確認原因或完成修復。
- 透過 Windows 掃描介面提供影像傳輸、取消、重掃、進度與錯誤回報。
- Windows 掃描整合與可在其他 Windows 11 x64 電腦安裝、更新、解除安裝的套件，支援 USB 換孔、拔插與重新開機。
- 掃描穩定後完成列印協定與 Windows 列印整合。

所有項目需要實機驗證，進度見 [delivery-status.md](delivery-status.md)。本機已確認的裝置資訊見 [硬體紀錄](docs/hardware.md)，系統變更提案見 [掃描介面安裝方案](driver/README.md)。

預覽畫面、影像編輯、PDF 組頁與檔案儲存由呼叫端軟體負責。專案內的 CLI 僅用於開發、診斷及測試。

## 開發

先讀 [AGENTS.md](AGENTS.md) 與 [ENG.md](ENG.md)。

```powershell
cargo fmt --all -- --check
cargo clippy --offline --all-targets -- -D warnings
cargo test --offline
cargo test --offline --example winusb_setup
cargo test --offline --example capture_scan
cargo test --offline --example scan_stability
cargo build --offline --release
cargo build --offline --release --example capture_scan
cargo build --offline --release --example scan_stability
```

開發機的 WinUSB 候選預檢可執行 `cargo run --offline --example winusb_setup`。不帶參數時不安裝驅動。實際配對需另外依 [安裝方案](driver/README.md)備份及取得系統變更授權，這個工具不是正式安裝套件。

本專案目前的原創程式採 MIT。SANE 上游的授權不同，尚未把其實作移植進本專案。這是獨立開發專案，並非 Xerox 官方驅動。
