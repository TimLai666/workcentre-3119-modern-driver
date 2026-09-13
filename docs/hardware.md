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

## 連續掃描中重現的自然失敗

2026-09-13，使用 `scan_stability` 同一程序預定執行 20 次空平台掃描。此工具逐塊核對 USB／解碼像素但不保存影像，任何錯誤立即停止。實際完成 4 次，第 5 次 600 dpi 彩色逾時，所以沒有 `complete.txt`，20 次驗收未通過。

| 次序 | 模式 | 秒數 | 結果 |
| --- | --- | --- | --- |
| 1 | RGB600 | 90.279 | 完整成功，117 塊，106503300 bytes |
| 2 | RGB600 | 101.734 | 完整成功，117 塊，106503300 bytes |
| 3 | RGB300 | 40.030 | 完整成功，29 塊，26653968 bytes |
| 4 | Gray600 | 42.208 | 完整成功，39 塊，35490900 bytes |
| 5 | RGB600 | 120.201 | 在等下一塊資料描述時達到工作期限；已交付 54 塊、49572000 bytes |

第 5 次最後一塊在 56.208 秒收到，接著約 64 秒沒有新影像。錯誤為 `stage=read-metadata ... Scan exceeded 120 second deadline`，沒有 USB 讀寫錯誤或清理失敗。錯誤發生在 `ready` 等候路徑；當時沒有記錄 Busy 回覆次數及最後狀態欄位，不能進一步歸因於暖機、硬體或驅動。清理後直接 Gray75 重掃成功（648×871，7.113 秒），不需重插；`doctor` 三個介面問題碼仍為 0。不能把這次自然逾時與先前刻意取消混為一談，也不能認定已找到歷史官方驅動故障的同一根因。

外部 PowerShell 每 500 ms 觀察同一程序，取得 769 個記憶體樣本。實際占用 RAM 峰值 7745536 bytes、程序私有配置峰值 2957312 bytes；本輪沒有觀察到記憶體暴增，但中途停止，不能宣稱長期無洩漏。證據在 `artifacts/stability-observation-20260913-c/`（`scans/diagnostics.log`、stdout／stderr、`memory.csv`、`analysis.json`），復原掃描在 `artifacts/scan-after-stability-timeout-20260913-c/`。測試執行檔 SHA256：`3FE5CE96D262EAEE1E7FE6971F5B587FA3B6C045B825CFE501AAAADAB18F1BDF`。

### 加入 Busy 診斷後再次重現

同日以新版本在同一程序預定執行 2 次 RGB600；第一個工作在 18.383 秒交付第 20 塊後停住，120.183 秒結束，第二次未啟動。已交付 18360000 bytes；READ（0x28）在該次等待收到 678 次有效 Busy 回覆，最後 status=0x08，state=unknown。這證明等候期間仍有控制回覆，沒有 USB API 讀寫錯誤；0x08 沒有足以辨識暖機或其他內部狀態的欄位，不能由此判定硬體損壞或驅動時序正確。

ABORT／RELEASE 沒有回報失敗，隨後 Gray75 又完整成功（648×871，7.221 秒），沒有重插。`doctor` 三個介面仍為問題碼 0、started=true。證據目錄為 `artifacts/stability-busy-observed-20260913-c/` 與 `artifacts/scan-after-busy-timeout-20260913-c/`；穩定性工具 SHA256 `3C600881525979BD72C2F45E84F1DE4579FBE649DCE01C9D61FA38D01C842FD5`。本次只增加診斷，沒有調整 100 ms 輪詢間隔、120 秒工作期限或 USB 傳輸政策；兩次自然失敗的根因仍未確認。

## 效能分段與 READ 詢問間隔對照

2026-09-13，新增開發 profile 後，保持全平台 RGB600、117 塊、5100×6961、106503300 像素 bytes 與 120 秒期限，以相同 release 執行檔比較 READ Busy 間隔。其他命令維持 100 ms，沒有重新配對或修改 USB 政策。平台沒有文件，工具逐塊核對每次 USB 資料與解碼像素，不保存影像。

| 順序／READ 間隔 | 工作秒數 | 影像讀取秒數 | 資料描述秒數 | 解碼毫秒 | 呼叫端毫秒 | 全工作 Busy 次數／睡眠秒數 |
| --- | --- | --- | --- | --- | --- | --- |
| 1／100 ms | 92.845 | 72.786 | 19.784 | 39.204 | 47.822 | 97／9.731 |
| 2／500 ms | 106.177 | 72.138 | 29.565 | 38.502 | 48.780 | 72／24.825 |
| 3／500 ms | 102.656 | 72.255 | 30.076 | 45.028 | 54.683 | 45／22.515 |
| 4／100 ms | 95.385 | 72.613 | 22.503 | 41.257 | 50.563 | 115／11.536 |

以上四次均成功且有 `complete.txt`。順序為 100、500、500、100 ms，每次為獨立程序，沒有控制機器內部暖機狀態。第二次 RESERVE 花 3.731 秒，其他約 0.030 秒，因此不能把整體時間差全部歸因於 READ 間隔。兩次 500 ms 均較慢，沒有支持改動預設的證據。解碼及呼叫端每次合計不到 0.1 秒，沒有支持優先平行化影像處理的證據。USB 呼叫包含等待裝置資料，且與階段時間重疊，不能把它當成純頻寬，也不能由主機 profile 分離機械運動與曝光耗時。

實機證據在 `artifacts/profile-read100-20260913-e/`、`artifacts/profile-read500-20260913-e/`、`artifacts/profile-read500-reverse-20260913-e/` 與 `artifacts/profile-read100-reverse-20260913-e/`，彙整於 `artifacts/profile-comparison-20260913-e.json`。測試 `scan_stability.exe` SHA256 為 `7E56EF81CEA7FB688ED0B1D4440F6F89838402394DA168D94E40002FA1F26981`，其來源後續僅做格式整理。測試後 `doctor` 父裝置、MI_00、MI_01 問題碼皆為 0、started=true。這四次沒有重現故障，不能取代同一程序 20 次驗收，也沒有高解析度故障修復或跨機型加速的證據。

## 64 KiB 與 256 KiB 影像讀取對照

2026-09-13，以同一個 release 執行檔各掃描一次全平台 RGB600，READ Busy 間隔固定 100 ms。WinUSB bulk IN 當下回報 `MAXIMUM_TRANSFER_SIZE=2097152` bytes，端點封包為 512 bytes。兩次都是 5100×6961、117 塊、106503300 像素 bytes，逐塊獨立核對 wire／pixels 成功，並產生 `complete.txt`。

| 讀取緩衝區 | 工作秒數 | 影像讀取秒數 | 資料描述秒數 | 全工作 USB 讀取呼叫 | Busy 次數 |
| --- | --- | --- | --- | --- | --- |
| 64 KiB | 97.010 | 72.549 | 24.153 | 1989 | 126 |
| 256 KiB | 93.736 | 72.784 | 20.658 | 689 | 102 |

較大緩衝區減少呼叫次數，但影像讀取階段沒有縮短。整體時間差主要出現在資料描述等候，只有一組比較，不能歸因於緩衝區或宣稱加速。維持 64 KiB 預設。兩次沒有重現自然故障，也不能取代同程序 20 次穩定性驗收。平台沒有文件，未驗收影像品質。

證據為 `artifacts/buffer64-20260913-f/` 與 `artifacts/buffer256-20260913-f/` 的 `diagnostics.log`、`complete.txt`。測試 `scan_stability.exe` SHA256：`1A22C30348B549318670F445CE03DB4B5348369F4816637786A476EC97E4DF88`。其後只補正傳輸上限回覆的非法封包檢查、共用公開掃描入口的上限預檢及拒絕設定的診斷，沒有改動資料讀取迴圈。

最終版本另經 `capture_scan` 完成 Gray75（648×871，564408 bytes，7.228 秒），保存於 `artifacts/buffer-final-gray75-20260913-f/`。PowerShell 獨立逐值核對全部 wire 有效樣本、像素及 PGM 相同，完成標記存在。實際檢視為空平台影像，不能用來驗收明暗品質。該擷取執行檔 SHA256 為 `50CC0C7F3F927775AD7FA2CB01C9BE022D4C974C50C849966B5D746B60E4517D`。測試後 `doctor` 父裝置、MI_00、MI_01 問題碼皆為 0、started=true。沒有重新安裝、重新插拔或修改系統設定。

## 首次實機影像傳輸

### 最新補充：600 dpi 彩色

定時取消驗證：`artifacts/scan-early-cancel-20260913-b` 設定 100 ms，結束碼 1，耗時約 0.38 秒；`scan-active-cancel-20260913-b` 設定 8000 ms、600RGB，收到 4 塊後以取消結束，總耗時約 8.71 秒。兩次都沒有 `complete.txt`，錯誤沒有清理失敗訊息。隨後 `scan-after-timed-cancel-20260913-b` 以 Gray75 得到 648 × 871 並正常完成，不需重插。定時器設定記於 `evidence.txt`；取消當下的精確 USB 階段未另追蹤。

2026-09-13 後續執行 `capture_scan artifacts/scan-rgb600-20260913-b rgb 600` 成功，回傳 5100 × 6961、117 塊、106503300 bytes，耗時 94.951 秒並正常釋放。逐塊移除 16-byte 尾端填補後，以獨立 Python/Pillow 核對平面 RGB 與 PPM 的所有像素相符。PPM SHA256：`d8746ed2e27214d64d1de203c37683e19727cf5ad86b25c775386e0d24fbee4b`，本機 `verification.json` 保留結果。這仍是空平台，不代表色彩或幾何品質已驗收。

此後灰階及彩色皆已驗證到 600 dpi。官方規格另標示光學 600 × 2400 dpi、插值 4800 dpi，與當前協定回報及相同 X／Y 設定的上限不同；非對稱設定尚未查明，見 [02](tickets/02-first-scan.md)。下方為較早的首次掃描紀錄。

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

本機證據位於 Git 排除的 `artifacts/scan-*-20260913-a/`，包含 `evidence.txt`、每塊 `.bin`、解碼像素、完整 PGM／PPM 與 `verification.json`。取消那次沒有完成影像或標記。該批首次測試尚未涵蓋正式幾何、100／150／200 dpi、600 dpi 彩色、暖機中取消、傳輸中斷、拔線、睡眠與 20 次連續掃描；後續結果見本文件上方。

審查修正版另使用單一 session 取得能力及影像，能力改存同目錄 `inquiry.txt`。`scan-gray75-reviewed-20260913-a` 成功，接著 `scan-rgb75-reviewed-cancel-20260913-a` 在第一塊後取消，最後 `scan-rgb75-reviewed-after-cancel-20260913-a` 完整成功。兩張完整影像再次通過獨立像素比對，SHA256 分別為 `6b794ba3e5c88ada390266d64ca012704e95a5811cc56804eab8fef9d6959f5b` 及 `88c3c6828b4f511e03be33cd8c459cca9d0845a42b0d84ab4400f46809a22b98`。
