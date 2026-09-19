# WorkCentre 3119 Modern Driver

Xerox WorkCentre 3119 在 Windows 11 x64 上的開源掃描驅動，以 Rust 開發，純驅動、不附 GUI。裝好之後用 Windows 內建的掃描軟體（Windows 掃描 App、Windows 傳真和掃描）或任何 WIA 掃描軟體直接掃描。這是獨立開發專案，不是 Xerox 官方驅動，採 MIT 授權。

## 目前能做什麼

- 平台掃描：灰階 8 位元、彩色 24 位元；75／100／150／200／300／600 dpi；整版或任意選區。
- Windows 掃描 App（Microsoft Store）與 Windows 傳真和掃描實掃通過，WinRT `Windows.Devices.Scanners` 與 WIA automation 也可用。
- WIA 亮度／對比（−1000～1000，0 為中性）由驅動套用，中性時不改任何像素。
- 一鍵安裝／更新／解除安裝套件，不需付費簽章。

尚未完成：取消中途掃描後立即重掃的復原、拔插／換 USB 孔／第二台電腦的實測、列印、掃描明暗品質對照（需要有內容的原稿）。進度與證據見 [delivery-status.md](delivery-status.md)。

## 安裝（一般使用者）

1. 到 [Releases](https://github.com/TimLai666/workcentre-3119-modern-driver/releases) 下載最新的 `wc3119-scanner-driver-<版本>.zip`，解壓縮到任一資料夾（安裝時要保留整個資料夾，不要只拉出 `install.cmd`）。
2. 用 USB 線接上 WorkCentre 3119 並開機（沒接也可以先裝）。
3. 對 `install.cmd` 按兩下，在「使用者帳戶控制」按「是」。視窗顯示 `Done` 就可以掃描了。

`install.cmd` 會信任套件的測試憑證、安裝或更新驅動、重啟 Windows 影像擷取服務並確認掃描器可用；同版本重跑只做檢查，舊版自動更新。驅動會複製進 Windows 驅動存放區，裝完後可以刪掉解壓縮的資料夾；要解除安裝時再下載一次並執行 `uninstall.cmd`（反向移除驅動與憑證信任）。說明與疑難排解見套件內的 `INSTALL.txt`，紀錄在 `%ProgramData%\WorkCentre3119Driver\setup-logs`。

沒有花錢買程式碼簽章，所以每台電腦第一次安裝都要按一次 UAC，套件只適合自用或少數信任的電腦，不能公開散布為「免確認」安裝。細節見 [driver/README.md](driver/README.md)。

## 怎麼運作

```text
Windows 掃描 App／傳真和掃描／WIA 軟體
        ↓ WIA 服務（stisvc）
workcentre_3119.dll   IStiUSD + IWiaMiniDrv（WIA 2.0 串流）
        ↓ WinUSB
USB\VID_0924&PID_4265&MI_00   掃描功能介面（MI_01 列印介面不動）
```

- INF 把 MI_00 登錄為 Image 類別、函式驅動維持 Microsoft WinUSB，WIA 服務以 COM 載入本 DLL。
- 掃描協定依 SANE `xerox_mfp` 的文件化行為獨立實作，未複製 SANE 程式碼。
- 驅動只輸出無壓縮 BMP；PNG／JPEG／PDF 等格式與預覽、編輯、存檔都由呼叫端軟體負責。
- 設計說明見 [ENG.md](ENG.md)，實機紀錄見 [docs/hardware.md](docs/hardware.md)。

## 打包（開發者）

需要 Rust stable MSVC 工具鏈、Visual Studio C++ 建置工具、Windows SDK（signtool）與 WDK（Inf2Cat，可用 winget 免費安裝）。

```powershell
cargo build --offline --release
./driver/package.ps1 -NewTestCertificate      # 第一次；之後用 -CertificateThumbprint <指紋>
```

套件輸出在 `artifacts/wia-package-<版本>-<時間>/`，內含 INF、簽署 catalog、DLL、憑證、`wc3119-setup.ps1`、`install.cmd`、`uninstall.cmd` 與 `INSTALL.txt`。改版時先提高 INF 的 `DriverVer`。

## 開發

先讀 [AGENTS.md](AGENTS.md)（專案契約）與 [ENG.md](ENG.md)。每次交付前：

```powershell
cargo fmt --all -- --check
cargo clippy --offline --all-targets -- -D warnings
cargo test --offline
cargo build --offline --release
```

改到 DLL 時另跑 `cargo test --offline --test com_server_dll -- --ignored`，並設定 `WC3119_TEST_DLL` 指向剛建好的 release DLL。

開發工具（不安裝驅動、不改登錄）：

| 指令 | 用途 |
| --- | --- |
| `cargo run --offline -- doctor` | 檢查 MI_00 的驅動狀態，結束碼 0 表示 WinUSB 已啟動 |
| `cargo run --offline -- inquiry` | 送出 INQUIRY，列出機器回報的解析度、模式與範圍 |
| `cargo run --offline --release --example capture_scan -- <目錄> gray 75` | 直接經 WinUSB 掃描並保存 USB 原文、像素與 BMP |
| `cargo run --offline --release --example scan_stability -- <目錄> 20` | 連續掃描穩定度測試，只記錄診斷 |

這些工具需要獨占 USB：WIA 服務載入驅動時會持有裝置，先停止 `stisvc` 或解除安裝套件再用。開發機首次配對 WinUSB 的工具與授權流程見 [driver/README.md](driver/README.md)。

`MI_00` 是複合式 USB 裝置的第 0 個功能介面，與電腦上的 USB 接孔無關；套件依型號與功能介面辨識，不綁特定電腦或孔位。[Microsoft USB 識別碼定義](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/standard-usb-identifiers)

## 已知限制

- 黑白線稿／半色調與非對稱 600 × 2400 dpi 尚未實作；[Xerox 規格](https://www.office.xerox.com/latest/W31BR-01.PDF)標示的插值解析度不提供。
- 整版掃描實際回傳約 11.6 英寸（要求 11.7），選區寬度會補齊到裝置單位；精確幾何驗收見 [02](docs/tickets/02-first-scan.md)。
- 掃描中途取消後，裝置可能需要約兩分鐘才接受下一次掃描，見 [03](docs/tickets/03-recover-scan.md)。
- 掃描器的燈光與曝光無法由驅動控制；明暗只能透過 WIA 亮度／對比調整。
