# 掃描介面安裝方案

目前只有 INF 設計稿，沒有簽署完成的安裝套件。不要把此目錄當成可安裝版本。尚未取得系統驅動變更授權。

## 可核對的變更範圍

- 唯一目標：`USB\VID_0924&PID_4265&MI_00`，目前沒有驅動，問題碼 28。
- 建議綁定：Windows 已內建的 Microsoft WinUSB。
- 預定裝置介面 GUID：`{C4147E4A-9C41-4846-A53C-5E625C68021A}`，供 Rust 程式列舉與開啟。
- 父裝置 `usbccgp` 與 MI_01 `usbprint` 必須維持原狀。
- 這個變更只提供 USB 存取，不會自動新增 WIA、Windows 掃描或列印功能。

## 開發機驗證路徑

[Microsoft 文件](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/winusb-installation) 提供從裝置管理員選擇內建 WinUSB 的方法，並說明手動配對後需要登錄裝置介面 GUID。這台電腦已有 Microsoft 簽署的 `winusb.sys` 與 `winusb.inf`，但尚未驗證裝置管理員是否允許此次配對。

取得明確授權後：

1. 重新查詢所有 Xerox 介面，要求唯一 MI_00，核對當前服務與 INF。如狀態已變，不得沿用舊觀測強制覆蓋。
2. 在本機保存精確裝置 ID、原服務、INF、問題碼及將更動的 DeviceInterfaceGUIDs 原值。這些資料可能包含序號，不能提交到 Git。
3. 僅替 MI_00 選擇內建 WinUSB，登錄上述 GUID。若系統要求不可信簽章或無法提供內建配對，停止該動作，不能停用安全設定。
4. 重新連接後驗證 MI_00 使用 WinUSB、問題碼 0，並核對 MI_01 與父裝置未變。
5. 執行 `wc3119 inquiry` 驗證 VID/PID、介面號、bulk 端點與 INQUIRY。程式已加入，成功路徑仍待實機驗證；目前沒有啟動掃描命令。

配對期間掃描介面可能暫時失效，需要重新連接 USB。原本沒有掃描驅動可用，因此本次復原目標是回到原本未綁定的狀態。

## 復原條件

配對後出現錯誤、目標不符或使用者要求還原時，只移除這次對 MI_00 的綁定與新增 GUID，依備份恢復原值。不得刪除 Windows 共用的 WinUSB 套件、父裝置、MI_01 或其他 USB 裝置。重新偵測後核對基準狀態。此復原流程尚未實測，執行前必須完成精確命令與備份檢查。

## 正式安裝套件

### 程式化配對的已知限制

本機 `winusb.inf` 的一般模型匹配 `USB\MS_COMP_WINUSB`，沒有 3119 的硬體 ID，因此一般 `pnputil /add-driver /install` 不會自動完成這次配對。2026-09-13 已用 [Rust 配對工具](../examples/winusb_setup.rs) 實測：MI_00 的裝置專屬 CLASS 清單包含 3 個候選，只有 1 個符合內建 `winusb.inf`、`WINUSB` 區段、Microsoft 與 `USB\MS_COMP_WINUSB`。其他 BILLBOARD／ADB 候選均被排除。預檢前後 MI_00 維持無服務／INF、問題碼 28。

不帶參數執行 `cargo run --offline --example winusb_setup` 只列舉候選。已實作的 `--install-mi00` 路徑要求完整預期裝置 ID，再次核對唯一在線裝置、無既有驅動及專屬清單候選，才呼叫 [DiInstallDevice](https://learn.microsoft.com/en-us/windows/win32/api/newdev/nf-newdev-diinstalldevice)。它不負責備份、GUID 登錄或復原，不是可分發的完整安裝套件。執行前仍需完成本文件的授權與備份要求；回傳 3010 表示 Windows 要求重新開機，工具不會自動重開機。

不可改造 USB 裝置的硬體 ID 或相容 ID 來假冒 WinUSB 相容裝置。[Microsoft 屬性限制](https://learn.microsoft.com/en-us/windows/win32/api/setupapi/nf-setupapi-setupdisetdeviceregistrypropertyw)也不允許直接以該 API 寫入保留的 CLASSGUID、CLASS、SERVICE 屬性。

`DiInstallDevice` 的 `DIIDFLAG_INSTALLNULLDRIVER` 可解除指定裝置綁定，但會移除該裝置的設定，不能當成未備份時的完整還原。原本沒有驅動時也不能依賴 [DiRollbackDriver](https://learn.microsoft.com/en-us/windows/win32/api/newdev/nf-newdev-dirollbackdriver) 找到舊版本。執行前仍須完成上方的精確備份及復原檢查。

### 套件驗收

需使用 WDK 驗證 INF、產生 catalog 並完成適用的簽署程序。正式驗收包含一般使用者掃描、重新開機、USB 換孔、升級、解除安裝及 Windows 掃描整合。付費簽署、送審或對外發布需另外授權。
