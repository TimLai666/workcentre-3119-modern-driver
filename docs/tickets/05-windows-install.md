# 05 — 使用者可以安裝驅動並透過 Windows 掃描

Epic：Windows 整合。User Story：一般掃描軟體能發現及使用 3119。
Blocked by：02 的掃描契約。Status：進行中，開發機 MI_00 WinUSB 配對及 INQUIRY 已驗證；正式模型套件、跨電腦換孔／拔插／重開機及 WIA 尚未驗證。

## 交付與流程

可分發模型套件（INF、catalog、適用簽署）→ 精確配對 USB MI_00 → WIA 裝置登錄 → Windows 掃描取得影像 → 更新／解除安裝。

開發機配對工具是單一 devnode 的驗證工具，不是上述模型套件或正式交付物。

## 已查證的整合限制

2026-09-13，查核本機 Windows SDK 10.0.26100.0 的 `stiusd.h`、`wiamindr_lh.h`、`wia_lh.h` 與 `C:\Windows\INF\sti.inf`。標準 `STI.USBSection.Services` 會指定 `usbscan.sys`；目前 MI_00 使用 WinUSB，因此不能直接把標準 STI USB 安裝段加入既有 INF 並假設傳輸方式不變。[Microsoft WIA INF 規則](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/inf-files-for-wia-devices)

WIA 2.0 的 IStream 傳輸路徑不呼叫 `drvWriteItemProperties`，硬體設定須在 `drvAcquireItemData` 中套用；同一時間只允許一條作用中串流。後續契約測試須涵蓋設定到實際掃描、串流寫入失敗、取消及重掃，不能只驗證 COM 介面可建立。[Microsoft IStream 傳輸契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/istream-data-transfer-driver-changes)

尚待驗證：保留 WinUSB 時 WIA 的裝置發現／minidriver 載入方式，以及 WIA 服務帳號能否存取該介面。先以不修改系統的契約測試及唯讀列舉縮小問題；需要 COM／INF 登錄或切換 MI_00 時，再提出具體可復原方案取得授權。本次未變更綁定、COM 或登錄。

下一個可獨立實作的項目是 Rust WIA 轉接元件的設定及串流契約。WIA minidriver 須支援 `IUnknown`、`IStiUSD`、`IWiaMiniDrv`，本機 SDK 標頭已有介面宣告，目前 Cargo 尚未產生 COM DLL。先驗證設定映射、串流部分寫入／錯誤、取消與工作釋放，再補 DLL 介面與生命週期測試。WIA2 串流只保證 `Write`、`Seek`、`SetSize`，不得依賴呼叫端提供完整檔案功能。[Microsoft WIA 介面](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-minidriver-interfaces)、[COM 識別契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/providing-a-com-interface)、[IStream 契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/istream-data-transfer-driver-changes)

本機 `sc.exe qc stisvc` 確認 WIA 服務帳號為 `NT Authority\LocalService`。一般使用者的 Rust 掃描成功不證明該服務帳號也能開啟 WinUSB。第一階段開發不登錄 COM、不修改服務或 USB 權限，也不把離線契約測試當成 Windows 掃描驗收。

## 驗收

- 先驗證 WIA COM、STI 登錄與 USB 存取權的整合方式，再固定正式 INF。不得只把 WinUSB 成功當作 WIA 完成。
- 正式交付物須是以 `USB\VID_0924&PID_4265&MI_00` 配對的型號／功能介面套件，並登錄固定裝置介面 GUID；套件與設定不得以開發機的完整實例 ID、序號、介面路徑或 USB 接孔位置限定使用。`driver/wc3119-winusb.inf` 目前仍是設計稿，沒有可分發的 catalog。
- [Rust 配對工具](../../examples/winusb_setup.rs)僅供開發機使用。它要求完整實例 ID，並以 `DiInstallDevice` 綁定單一 devnode，不能當成模型套件，也不能作為跨電腦可攜性證據。
- 在第二台未曾安裝本專案、維持 Windows 11 x64 預設安全設定的乾淨支援電腦上，以同一份可分發模型套件安裝 3119；不複製開發機的登錄或實例資料，並以一般使用者完成 Windows 掃描。
- Windows 掃描可發現裝置、選擇有效設定、取得影像、取消及再次掃描。
- 掃描操作使用既有 WIA 相容軟體，安裝後不需本專案的 GUI 或掃描 App。驅動回傳能力、進度及錯誤，預覽畫面、影像編輯與儲存由呼叫端負責。
- 按 [07 — 掃描明暗品質](07-scan-tones.md)驗證 WIA 亮度、對比與實際回傳影像的映射。完整掃描交付須通過該品質驗收，WIA 開發可先行。
- 非系統管理員能掃描，服務帳號的裝置及檔案權限遵循最小必要範圍。
- 安裝及更新驗證目標、簽章及版本。無裝置時提供明確結果，不修改其他機型。
- 安裝中斷、已有其他驅動、重複安裝及部分成功有可驗證的復原行為。
- 首次掃描後拔出裝置，改插另一個 USB 接孔，等待重新列舉後不重新安裝套件即可再次掃描；重開機後也可再次掃描。每次都由執行期重新發現當下裝置介面與路徑，不依賴保存的 instance ID、序號、USB 路徑或接孔位置；父裝置與 MI_01 維持原有服務及設定。
- 解除安裝僅移除本套件所有的登錄與檔案，保留其他 USB 裝置及使用者影像。
- 在安全設定維持啟用的 Windows 11 x64 驗證簽署套件。簽署、付費與對外送審先取得明確授權。

## 測試

WIA 呼叫契約測試、第二台乾淨 Windows 11 x64 電腦的模型套件安裝、Windows 掃描實機驗證、換孔／拔插／重開機後掃描及復原測試。系統變更與測試副作用記錄於實機驗收資料；不得以開發機單一 devnode 的配對結果替代跨電腦驗收。

2026-09-13：開發機配對工具 7 個測試通過，涵蓋參數拒絕、完整目標 ID、候選條件、UTF-16 邊界、預檢失敗及 x64 原生結構。授權配對後 `doctor` 與 `inquiry` 均成功，真實能力回覆見 [硬體紀錄](../hardware.md)。這只驗證單一開發機／裝置實例的 WinUSB 與 INQUIRY；正式模型套件、catalog／簽署、WIA、第二台乾淨電腦、換孔／拔插／重開機及掃描影像仍未驗證。
