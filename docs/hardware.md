# 本機硬體觀測

觀測日期：2026-09-13。此檔省略 USB 序號與完整實例路徑。

## 系統與工具

- Windows 11 專業版 x64，版本 `10.0.26200`。
- Rust `1.97.0`、Cargo `1.97.0`，`stable-x86_64-pc-windows-msvc`。
- Windows SDK `10.0.26100.0` 已存在。SDK 內找到 signtool，沒有找到 InfVerif / Inf2Cat，因此沒有完成 WDK 套件驗證。
- `C:\Windows\System32\drivers\winusb.sys` 的 Authenticode 檢查為 Valid，簽署者 Microsoft Windows。這不是自訂 INF 已簽署的證據。

## 配對前的 Xerox USB 裝置

| 目標 | 硬體 ID | 服務 | 問題碼 | 已啟動 |
| --- | --- | --- | --- | --- |
| 父裝置 | `USB\VID_0924&PID_4265` | usbccgp | 0 | 是 |
| 掃描介面 | `USB\VID_0924&PID_4265&MI_00` | 無 | 28 | 否 |
| 列印介面 | `USB\VID_0924&PID_4265&MI_01` | usbprint | 0 | 是 |

MI_00 相容 ID 為 vendor-specific `Class_ff&SubClass_ff&Prot_ff`。MI_01 為 `Class_07&SubClass_01&Prot_02`。掃描介面目前沒有 DriverInfPath，列印使用 `usbprint.inf`，父裝置使用 `usb.inf`。

MI_01 的列印角色有 USB 類別與服務佐證。MI_00 是唯一剩下的廠商專用介面，因此暫定為掃描目標。這是依本機介面組合的推論，仍需正確的 INQUIRY 回覆確認，不能把角色名稱當作通訊已驗證。

本機列印佇列只有 OneNote (Desktop) 與 Microsoft Print to PDF，沒有 Xerox 佇列。

## 驗證方法

初次觀測使用唯讀 `Get-PnpDevice -PresentOnly`、`Get-PnpDeviceProperty`、`Get-CimInstance Win32_OperatingSystem`、`Get-CimInstance Win32_Printer`。Rust 程式隨後使用 Configuration Manager API 得到相同的介面、服務與問題碼。

重現 Rust 診斷：

```powershell
cargo run --offline -- doctor
```

當次 Rust 輸出：

```text
Xerox WorkCentre 3119 · USB 0924:4265
複合裝置: service=usbccgp, problem=0, started=true
掃描介面 MI_00: service=未安裝, problem=28, started=false
列印介面 MI_01: service=usbprint, problem=0, started=true
掃描介面缺少驅動（Windows 錯誤碼 28）。需要安裝掃描介面的 USB 驅動後才能繼續。
```

## 未取得的證據

2026-09-13 開發機配對預檢：裝置專屬與全域 CLASS 清單各 3 個候選，均只有 1 個精確符合內建 WinUSB；其餘 BILLBOARD／ADB 均被拒絕。MI_00 前後維持未綁定、問題碼 28，父裝置與 MI_01 屬性符合原始備份。這次沒有執行安裝路徑。

加入能力查詢後，2026-09-13 再次以 release 執行 `doctor`，三個介面的服務、問題碼及啟動狀態與上表相同。`wc3119 inquiry` 回報 `Scanner WinUSB interface is unavailable; MI_00 must be paired and its interface GUID registered before inquiry`，結束碼 1。此次失敗發生在已登錄裝置介面的列舉階段，未開啟 USB 或送出 INQUIRY。

以上為配對前紀錄。當時沒有取得 USB 通訊或影像證據。

## 授權配對後的最新結果

2026-09-13 經使用者授權及 UAC，只對已備份的完整 MI_00 實例綁定內建 WinUSB、登錄專案 GUID 及重新啟動介面。安裝與介面重啟結束碼均為 0，無需重開機。原生協調腳本核對父裝置／MI_01 的服務、INF、問題碼、ClassGuid、Parent 均與備份一致。

| 目標 | 服務 | 問題碼 | 已啟動 |
| --- | --- | --- | --- |
| 父裝置 | usbccgp | 0 | 是 |
| MI_00 | WINUSB | 0 | 是 |
| MI_01 | usbprint | 0 | 是 |

一般權限執行 release `doctor` 與 `inquiry` 皆回傳 0。`inquiry` 已讀取並核對 USB 裝置／介面描述與唯一 bulk IN／OUT，再收到有效能力回覆：

| 回覆欄位 | 真實回報值 |
| --- | --- |
| 機器識別 | SAMSUNG ORION |
| 已辨識解析度 | 75、100、150、200、300、600 dpi |
| 解析度／模式旗標 | 0x00353f／0x29 |
| 寬／最大長／平台長 | 10200／14040／14040，單位 1/1200 英吋 |
| 行序／壓縮旗標 | 0x01／0x2f |

當時只有 INQUIRY；後續影像與端點證據見下節。回報解析度不等於光學解析度驗證。尚無 WIA、列印佇列、安裝復原、換孔或跨電腦驗證。未變更系統安全設定。

## 首次實機影像傳輸

2026-09-13，以一般權限執行原創 Rust `capture_scan` 範例，使用者確認平台沒有文件。以下皆為空平台，不能用來驗收文字、色彩、尺寸準確度或歷史偏白症狀。

端點由每次開啟時重新查詢：bulk IN `0x84`、OUT `0x03`，maximum packet 皆 512。裝置與介面描述如下，不含序號字串：

```text
device: 12 01 00 02 00 00 00 40 24 09 65 42 00 01 01 02 03 01
interface: 09 04 00 00 02 ff ff ff 00
INQUIRY (70 bytes):
a8 00 43 10 53 41 4d 53 55 4e 47 20 4f 52 49 4f
4e 20 20 20 20 20 20 20 20 20 20 20 20 20 20 20
20 20 20 20 35 3f 04 29 00 00 27 d8 00 00 36 d8
00 01 2f 00 00 02 00 00 00 00 36 d8 00 00 36 d8
05 02 05 05 00 00
```

全平台設定為零偏移、10200 × 14040（1/1200 英吋）、無壓縮。以下為 READ 回報並完整接收的尺寸，未裁切、縮放或修正明暗：

| 測試 | 實際像素 | 分塊 | 像素 bytes | 秒數 | 結果 |
| --- | --- | --- | --- | --- | --- |
| Gray 75 dpi | 648 × 871 | 1 | 564408 | 7.315 | 成功並釋放 |
| RGB 75 dpi | 648 × 871 | 2 | 1693224 | 14.608 | 成功並釋放 |
| RGB 75 dpi 第一塊後取消 | 648 × 474 部分資料 | 1 | 不標示完成 | 不作效能量測 | 回傳 Interrupted，ABORT／RELEASE 成功，無 complete.txt |
| 取消後 Gray 75 dpi | 648 × 871 | 1 | 564408 | 7.133 | 未重插即重新掃描成功 |
| RGB 300 dpi | 2556 × 3476 | 29 | 26653968 | 29.611 | 成功並釋放 |
| Gray 600 dpi | 5100 × 6959 | 39 | 35490900 | 31.757 | 成功並釋放 |

每塊 wire 比有效像素多 16 bytes。以獨立 Python/Pillow 讀取檔案，比對全部 wire 與 PGM／PPM 像素：灰階完全相同，RGB 逐行從平面通道重排後完全相同，沒有數值轉換。無損轉存 PNG 後再次逐位元組相等；75 dpi 灰階及彩色實際開啟檢視，為白色平台背景與少量細點，沒有文件可作色彩參照。

影像 SHA256（PGM／PPM）：Gray75 `c0670537dd8a294ce70275f4ac77638f77afc7e462cf8f697c9a833dfda6ffcf`；RGB75 `4be342fa8b2d817f589e18548715646669e86710618137658edfeb0440ec8ed6`；RGB300 `ce2fc27974795aa208c3ee566d4ca89388f84f338adcec8a97e04ad79d468edb`；Gray600 `607bb5dc7d9fe5ec926c4d0454c10cab3fbe89e82d976101d9704afb2e8599b1`。

300 dpi 掃描中，同時由另一行程執行 `wc3119 inquiry` 回傳存取被拒（OS error 5），原掃描仍成功完成。掃描後 `doctor` 核對父裝置、MI_00、MI_01 服務與問題碼仍正常。沒有安裝、重新配對、reset 或 CLEAR_HALT。

本機證據位於 Git 排除的 `artifacts/scan-*-20260913-a/`，包含 `evidence.txt`、每塊 `.bin`、解碼像素、完整 PGM／PPM 與 `verification.json`。取消那次沒有完成影像或標記。正式幾何、100／150／200 dpi、600 dpi 彩色、暖機中取消、傳輸中斷、拔線、睡眠與 20 次連續掃描仍未驗證。

審查修正版另使用單一 session 取得能力及影像，能力改存同目錄 `inquiry.txt`。`scan-gray75-reviewed-20260913-a` 成功，接著 `scan-rgb75-reviewed-cancel-20260913-a` 在第一塊後取消，最後 `scan-rgb75-reviewed-after-cancel-20260913-a` 完整成功。兩張完整影像再次通過獨立像素比對，SHA256 分別為 `6b794ba3e5c88ada390266d64ca012704e95a5811cc56804eab8fef9d6959f5b` 及 `88c3c6828b4f511e03be33cd8c459cca9d0845a42b0d84ab4400f46809a22b98`。
