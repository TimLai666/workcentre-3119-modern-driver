# 掃描介面安裝方案

目前只有 INF 設計稿及開發機配對工具，沒有簽署完成的安裝套件。不要把此目錄當成可安裝版本。2026-09-13 使用者已核准本機已備份 MI_00 綁定內建 WinUSB、登錄下列裝置介面 GUID 及重新啟動該介面，以進行實機通訊測試。

## 可核對的變更範圍

- 唯一目標：`USB\VID_0924&PID_4265&MI_00`，配對前沒有驅動、問題碼 28，配對後為 0。
- 已完成本機綁定：Windows 已內建的 Microsoft WinUSB。
- 已登錄裝置介面 GUID：`{C4147E4A-9C41-4846-A53C-5E625C68021A}`，供 Rust 程式列舉與開啟。
- 父裝置 `usbccgp` 與 MI_01 `usbprint` 必須維持原狀。
- 這個變更只提供 USB 存取，不會自動新增 WIA、Windows 掃描或列印功能。

## 開發機驗證路徑

[Microsoft 文件](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/winusb-installation) 提供從裝置管理員選擇內建 WinUSB 的方法，並說明手動配對後需要登錄裝置介面 GUID。這台電腦已有 Microsoft 簽署的 `winusb.sys` 與 `winusb.inf`，但尚未驗證裝置管理員是否允許此次配對。

取得明確授權後：

1. 重新查詢所有 Xerox 介面，要求唯一 MI_00，核對當前服務與 INF。如狀態已變，不得沿用舊觀測強制覆蓋。
2. 在本機保存精確裝置 ID、原服務、INF、問題碼及將更動的 DeviceInterfaceGUIDs 原值。這些資料可能包含序號，不能提交到 Git。
3. 僅替 MI_00 選擇內建 WinUSB，登錄上述 GUID。若系統要求不可信簽章或無法提供內建配對，停止該動作，不能停用安全設定。
4. 重新連接後驗證 MI_00 使用 WinUSB、問題碼 0，並核對 MI_01 與父裝置未變。
5. 執行 `wc3119 inquiry` 驗證 VID/PID、介面號、bulk 端點與 INQUIRY。本機此路徑已成功，詳見 [硬體紀錄](../docs/hardware.md)；目前沒有啟動掃描命令。

配對期間掃描介面可能暫時失效，需要重新連接 USB。原本沒有掃描驅動可用，因此本次復原目標是回到原本未綁定的狀態。

本機此次經 UAC 使用已審查的 Rust 工具配對，再登錄 GUID 及僅重新啟動 MI_00，全部回傳 0、無需重開機。父裝置與 MI_01 屬性符合備份。這是單一已備份裝置實例的開發驗證；同機已配對後不得重跑只接受無驅動狀態的安裝工具。正式套件須依型號及功能介面配對，並依 [05 驗收](../docs/tickets/05-windows-install.md)在其他電腦及 USB 接孔實測。

## 復原條件

配對後出現錯誤、目標不符或使用者要求還原時，只移除這次對 MI_00 的綁定與新增 GUID，依備份恢復原值。不得刪除 Windows 共用的 WinUSB 套件、父裝置、MI_01 或其他 USB 裝置。重新偵測後核對基準狀態。此復原流程尚未實測，執行前必須完成精確命令與備份檢查。

## WIA 登錄方案（尚未執行，待授權）

2026-09-19 依本機 `C:\Windows\INF\sti.inf`、`winusb.inf` 與 [Microsoft WIA INF 規則](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/inf-files-for-wia-devices) 完成 [WIA INF 設計稿](wc3119-wia.inf)。它與現有 WinUSB 設計稿的差異：

- Class 改為 `Image`，讓 still-image 類別安裝程式（sti_ci）處理 `SubClass=StillImage`、`DeviceType=1`、`Capabilities=0x10`（STI_GENCAP_WIA）、`Events` 與 `DeviceData`。
- 函式驅動仍是 `Include=winusb.inf` 的 WinUSB，並沿用同一裝置介面 GUID。不引用 `STI.USBSection`，因為它會加入 `usbscan.sys` 服務並改變傳輸方式。
- `AddReg` 寫入 `HardwareConfig=1,4`、`CreateFileName=AUTO`、`USDClass` 與 HKCR `CLSID\{F71A8435-…}\InProcServer32` 指向驅動存放區（`%13%`）內的 `workcentre_3119.dll`，ThreadingModel 為 Both。minidriver 不使用 port name，自行以 GUID 列舉介面，因此 AUTO 與現有實作一致。
- 事件只宣告連線／斷線，與 `drvGetCapabilities` 相同；驅動不自行發送事件。

尚未查證、必須以實測確認的前提：`Image` 類別搭配 WinUSB 函式驅動是否被類別安裝程式接受並建立 StillImage 裝置介面；WIA 服務帳號 `NT Authority\LocalService` 能否開啟 WinUSB 裝置介面；DLL 目前仍匯入 `VCRUNTIME140.dll` 與 UCRT，服務帳號載入時的 runtime 前置條件；`stisvc` 目前為 Stopped，實測時服務會由 PnP 事件啟動。

### 簽署門檻

Windows 11 x64 安裝第三方 INF 需要簽署的 catalog。本機只有 Windows SDK 的 signtool，沒有 WDK 的 InfVerif／Inf2Cat，`bcdedit` 也未開啟 testsigning。可行路徑只有兩條，皆須使用者另行決定與授權：

1. 開發機測試簽署：安裝 WDK 工具，產生 catalog 並以自簽測試憑證簽署，把測試憑證放入本機受信任的根與 Trusted Publishers，並啟用 `bcdedit /set testsigning on` 後重開機。這是系統安全設定變更，只限開發機，不能寫進一般安裝步驟。
2. 正式簽署：經 Hardware Dev Center 取得 attestation 簽署。需要 EV 憑證與付費，須另行授權。

沒有這兩者之一，`pnputil /add-driver … /install` 會因缺少簽章而失敗；本專案不會以停用簽章驗證、Secure Boot 或記憶體完整性作為替代。

### 授權後的執行與復原順序

1. 重新列舉唯一 MI_00，核對目前仍是 WinUSB、問題碼 0，父裝置與 MI_01 未變。狀態不同就停止。
2. 備份到 Git 排除的 `artifacts/` 新目錄：MI_00 devnode 屬性與 `Device Parameters`、目前 `oem*.inf` 清單（`pnputil /enum-drivers`）、HKCR `CLSID\{F71A8435-…}` 是否存在、`HKLM\SYSTEM\CurrentControlSet\Control\Class\{6BDD1FC6-…}` 子鍵清單、`stisvc` 狀態。備份可能含序號，不提交。
3. 建置 release DLL，記錄 SHA256，與 INF、catalog 放入同一套件目錄；以 InfVerif 檢查 INF。
4. `pnputil /add-driver wc3119-wia.inf /install`，記錄回傳碼與新增的 `oem*.inf` 名稱。要求重開機時不自動重開，先回報。
5. 驗證：`wc3119 doctor` 三個介面問題碼 0；MI_00 的 Class 為 Image、服務仍為 WINUSB；`Get-CimInstance Win32_PnPEntity` 出現 Image 類別裝置；WIA automation `DeviceInfos.Count` 由 0 變 1；Windows 掃描可列出裝置並完成一次 75 dpi 掃描與取消。任何一步失敗就停在該步，依既有日誌診斷，不重試安裝。
6. 復原：`pnputil /delete-driver oem<N>.inf /uninstall /force` 只移除本套件，再以備份核對 MI_00 回到 WinUSB＋USBDevice 類別、CLSID 鍵消失、父裝置與 MI_01 未變。若 pnputil 移除後 devnode 仍綁 Image 類別，改用開發機配對工具重新綁定內建 WinUSB。測試簽署模式與測試憑證另有獨立的關閉／移除步驟，不併入套件復原。

## 正式安裝套件

### 程式化配對的已知限制

本機 `winusb.inf` 的一般模型匹配 `USB\MS_COMP_WINUSB`，沒有 3119 的硬體 ID，因此一般 `pnputil /add-driver /install` 不會自動完成這次配對。2026-09-13 已用 [Rust 配對工具](../examples/winusb_setup.rs) 實測：MI_00 的裝置專屬 CLASS 清單包含 3 個候選，只有 1 個符合內建 `winusb.inf`、`WINUSB` 區段、Microsoft 與 `USB\MS_COMP_WINUSB`。其他 BILLBOARD／ADB 候選均被排除。預檢前後 MI_00 維持無服務／INF、問題碼 28。

不帶參數執行 `cargo run --offline --example winusb_setup` 只列舉候選。已實作的 `--install-mi00` 路徑要求完整預期裝置 ID，再次核對唯一在線裝置、無既有驅動及專屬清單候選，才呼叫 [DiInstallDevice](https://learn.microsoft.com/en-us/windows/win32/api/newdev/nf-newdev-diinstalldevice)。它不負責備份、GUID 登錄或復原，不是可分發的完整安裝套件。執行前仍需完成本文件的授權與備份要求；回傳 3010 表示 Windows 要求重新開機，工具不會自動重開機。

不可改造 USB 裝置的硬體 ID 或相容 ID 來假冒 WinUSB 相容裝置。[Microsoft 屬性限制](https://learn.microsoft.com/en-us/windows/win32/api/setupapi/nf-setupapi-setupdisetdeviceregistrypropertyw)也不允許直接以該 API 寫入保留的 CLASSGUID、CLASS、SERVICE 屬性。

`DiInstallDevice` 的 `DIIDFLAG_INSTALLNULLDRIVER` 可解除指定裝置綁定，但會移除該裝置的設定，不能當成未備份時的完整還原。原本沒有驅動時也不能依賴 [DiRollbackDriver](https://learn.microsoft.com/en-us/windows/win32/api/newdev/nf-newdev-dirollbackdriver) 找到舊版本。執行前仍須完成上方的精確備份及復原檢查。

### 套件驗收

需使用 WDK 驗證 INF、產生 catalog 並完成適用的簽署程序。正式驗收包含一般使用者掃描、重新開機、USB 換孔、升級、解除安裝及 Windows 掃描整合。付費簽署、送審或對外發布需另外授權。
