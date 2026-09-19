# WorkCentre 3119 Modern Driver

Xerox WorkCentre 3119 在 Windows 11 x64 上的開源掃描驅動，以 Rust 開發。本專案只做驅動本身，沒有附自己的掃描程式，裝好之後用 Windows 內建的掃描軟體（Windows 掃描 App、Windows 傳真和掃描）或其他 WIA 掃描軟體就能掃描。這是獨立開發的專案，不是 Xerox 官方驅動，採 MIT 授權。

## 目前能做什麼

- 平台掃描支援灰階 8 位元與彩色 24 位元，解析度有 75、100、150、200、300、600 dpi，可以掃整版，也可以自己框選區域。
- Windows 掃描 App（Microsoft Store）與 Windows 傳真和掃描都已經實機掃描成功，WinRT `Windows.Devices.Scanners` 與 WIA automation 也可以用。
- WIA 的亮度與對比（−1000 到 1000）由驅動套用，設成中性值 0 時不會更動任何像素。
- 安裝、更新、解除安裝都是按兩下就完成，不必購買程式碼簽章憑證。

還沒完成的部分：掃描到一半取消後立刻重掃的復原流程、拔插 USB 線、換 USB 孔、換到第二台電腦的實測、列印功能，還有掃描明暗的品質對照（要有內容的原稿才比得出來）。進度與證據見 [delivery-status.md](delivery-status.md)。

## 安裝（一般使用者）

1. 到 [Releases](https://github.com/TimLai666/workcentre-3119-modern-driver/releases) 下載最新的 `wc3119-scanner-driver-<版本>.zip`，解壓縮到任一資料夾。安裝時要保留整個資料夾，不要只把 `install.cmd` 拉出來。
2. 用 USB 線接上 WorkCentre 3119 並開機。還沒接上也可以先安裝。
3. 對 `install.cmd` 按兩下，在「使用者帳戶控制」按「是」。視窗顯示 `Done` 就可以開始掃描。

`install.cmd` 會信任套件的測試憑證、安裝或更新驅動、重啟 Windows 影像擷取服務，最後確認掃描器可以使用。同一個版本重跑只會做檢查，裝著舊版則會自動更新。驅動會複製進 Windows 驅動存放區，所以裝完之後可以把解壓縮出來的資料夾刪掉。之後想解除安裝，再下載一次套件並執行 `uninstall.cmd`，它會反向移除驅動並取消憑證信任。套件裡的 `INSTALL.txt` 有完整說明與疑難排解，執行紀錄寫在 `%ProgramData%\WorkCentre3119Driver\setup-logs`。

這個套件沒有買程式碼簽章憑證，所以每台電腦第一次安裝都要按一次 UAC，也要把測試憑證加進這台機器的信任清單。它因此只適合自用或少數幾台你信任的電腦，不適合公開散布。細節見 [driver/README.md](driver/README.md)。

## 怎麼運作

```text
Windows 掃描 App／傳真和掃描／WIA 軟體
        ↓ WIA 服務（stisvc）
workcentre_3119.dll   IStiUSD + IWiaMiniDrv（WIA 2.0 串流）
        ↓ WinUSB
USB\VID_0924&PID_4265&MI_00   掃描功能介面（MI_01 列印介面不動）
```

- INF 把 MI_00 登錄成 Image 類別，函式驅動維持 Microsoft 的 WinUSB，WIA 服務再以 COM 載入本專案的 DLL。
- 掃描協定是依 SANE `xerox_mfp` 已文件化的行為獨立實作，沒有複製 SANE 的程式碼。
- 驅動只輸出無壓縮的 BMP。PNG、JPEG、PDF 這些格式，還有預覽、編輯、存檔，都由呼叫端的軟體負責。
- 設計說明見 [ENG.md](ENG.md)，實機紀錄見 [docs/hardware.md](docs/hardware.md)。

## 打包（開發者）

需要 Rust stable 的 MSVC 工具鏈、Visual Studio C++ 建置工具、Windows SDK（提供 signtool）與 WDK（提供 Inf2Cat，可以用 winget 免費安裝）。

```powershell
cargo build --offline --release
./driver/package.ps1 -NewTestCertificate      # 第一次；之後用 -CertificateThumbprint <指紋>
```

套件會輸出到 `artifacts/wia-package-<版本>-<時間>/`，裡面有 INF、簽署過的 catalog、DLL、憑證、`wc3119-setup.ps1`、`install.cmd`、`uninstall.cmd` 與 `INSTALL.txt`。改版之前要先把 INF 的 `DriverVer` 提高。

## 開發

先讀 [AGENTS.md](AGENTS.md)（專案契約）與 [ENG.md](ENG.md)。每次交付前跑這四個指令：

```powershell
cargo fmt --all -- --check
cargo clippy --offline --all-targets -- -D warnings
cargo test --offline
cargo build --offline --release
```

改到 DLL 的時候另外跑 `cargo test --offline --test com_server_dll -- --ignored`，並把 `WC3119_TEST_DLL` 設成剛建好的 release DLL 路徑。

開發工具（不會安裝驅動，也不會改登錄）：

| 指令 | 用途 |
| --- | --- |
| `cargo run --offline -- doctor` | 檢查 MI_00 的驅動狀態，結束碼 0 表示 WinUSB 已啟動 |
| `cargo run --offline -- inquiry` | 送出 INQUIRY，列出機器回報的解析度、模式與範圍 |
| `cargo run --offline --release --example capture_scan -- <目錄> gray 75` | 不經過 WIA，直接以 WinUSB 掃描，保存 USB 原文、像素與 BMP |
| `cargo run --offline --release --example scan_stability -- <目錄> 20` | 連續掃描的穩定度測試，只記錄診斷數值 |

這些工具需要獨占 USB。WIA 服務載入驅動的時候會持有裝置，所以要先停止 `stisvc` 或解除安裝套件，才能用它們。開發機第一次配對 WinUSB 的工具與授權流程見 [driver/README.md](driver/README.md)。

`MI_00` 是複合式 USB 裝置的第 0 個功能介面，跟電腦上的 USB 接孔無關。套件是依型號與功能介面辨識裝置，不會綁定特定電腦或特定接孔。參考 [Microsoft 的 USB 識別碼定義](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/standard-usb-identifiers)。

## 已知限制

- 黑白線稿、半色調與非對稱的 600 × 2400 dpi 都還沒實作。[Xerox 規格](https://www.office.xerox.com/latest/W31BR-01.PDF)上標示的插值解析度，這個驅動不提供。
- 整版掃描實際回傳的長度約 11.6 英寸（要求的是 11.7），框選區域的寬度會被補齊到裝置的最小單位。幾何精度的驗收條件見 [02](docs/tickets/02-first-scan.md)。
- 掃描中途取消之後，機器可能要等約兩分鐘才會接受下一次掃描，見 [03](docs/tickets/03-recover-scan.md)。
- 掃描器的燈光與曝光沒辦法由驅動控制，明暗只能靠 WIA 的亮度與對比調整。
