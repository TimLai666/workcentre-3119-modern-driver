# 本機硬體觀測

觀測日期：2026-09-13。此檔省略 USB 序號與完整實例路徑。

## 系統與工具

- Windows 11 專業版 x64，版本 `10.0.26200`。
- Rust `1.97.0`、Cargo `1.97.0`，`stable-x86_64-pc-windows-msvc`。
- Windows SDK `10.0.26100.0` 已存在。SDK 內找到 signtool，沒有找到 InfVerif / Inf2Cat，因此沒有完成 WDK 套件驗證。
- `C:\Windows\System32\drivers\winusb.sys` 的 Authenticode 檢查為 Valid，簽署者 Microsoft Windows。這不是自訂 INF 已簽署的證據。

## 已連接的 Xerox USB 裝置

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

加入能力查詢後，2026-09-13 再次以 release 執行 `doctor`，三個介面的服務、問題碼及啟動狀態與上表相同。`wc3119 inquiry` 回報 `Scanner WinUSB interface is unavailable; MI_00 must be paired and its interface GUID registered before inquiry`，結束碼 1。此次失敗發生在已登錄裝置介面的列舉階段，未開啟 USB 或送出 INQUIRY。

尚未讀取 USB 端點描述、INQUIRY 能力回覆或任何影像。沒有執行掃描、修改驅動綁定、建立 WIA 裝置、建立列印佇列或修改安全設定。
