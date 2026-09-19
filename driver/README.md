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
- `DeviceInterfaceGUIDs` 同時列出專案 GUID 與 `GUID_DEVINTERFACE_IMAGE`：WinUSB 會登錄兩個介面，後者讓 WIA 服務寫入 `DEVPKEY_WIA_DeviceType` 並讓 WinRT `Windows.Devices.Scanners`（Windows 掃描 App 用的 API）以介面類別找到裝置。代價是 WIA 服務會嘗試在該介面上開啟通知句柄，而 WinUSB 只允許一個句柄，所以驅動在 `IStiUSD::Initialize` 就開啟並保留 USB 句柄（見 ENG.md）。

尚未查證、必須以實測確認的前提：`Image` 類別搭配 WinUSB 函式驅動是否被類別安裝程式接受並建立 StillImage 裝置介面；WIA 服務帳號 `NT Authority\LocalService` 能否開啟 WinUSB 裝置介面；DLL 目前仍匯入 `VCRUNTIME140.dll` 與 UCRT，服務帳號載入時的 runtime 前置條件；`stisvc` 目前為 Stopped，實測時服務會由 PnP 事件啟動。

### 簽署（免費路線）

使用者已決定不花錢，因此不走 EV 憑證與 attestation。依 [PnP 裝置安裝簽署要求](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/pnp-device-installation-signing-requirements--windows-vista-and-later-)，開發與測試階段的套件可用測試憑證簽署 catalog；`bcdedit testsigning` 只影響核心模式二進位檔的載入，本套件沒有自己的核心驅動（函式驅動是 Microsoft 簽署的 WinUSB，本專案只有使用者模式 DLL），因此不啟用 testsigning，也不停用 Secure Boot。這個推論尚未以實際 `pnputil` 安裝驗證：若 PnP 仍拒絕，錯誤會停在 `/add-driver`，不會留下半套狀態。[Test-signing 說明](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/the-testsigning-boot-configuration-option)、[測試簽署套件](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/test-signing-driver-packages)

需要的免費工具：Windows SDK 的 `signtool`（本機已有）、WDK 的 `Inf2Cat`（本機尚未安裝，需另外下載 WDK），以及 PowerShell `New-SelfSignedCertificate` 產生的自簽程式碼簽署憑證。

代價是每台要安裝的電腦都必須先把這張測試憑證放進 LocalMachine 的 Trusted Root 與 Trusted Publishers，這是機器層級的安全設定變更，每台都要使用者明確授權。套件因此只適合自用或少數信任的電腦，不能公開分發。

### 套件、安裝、更新、解除安裝

兩支腳本都在本目錄，預設只做預檢，加 `-Apply` 才會改系統，且需要提升權限的 PowerShell。每次執行都在 Git 排除的 `artifacts/wia-setup-*` 留下日誌與備份。

| 步驟 | 指令 | 改動範圍 |
| --- | --- | --- |
| 打包簽署 | `driver/package.ps1 -NewTestCertificate`（之後用 `-CertificateThumbprint`） | 只寫 `artifacts/wia-package-<版本>-<時間>/`；憑證只進 CurrentUser\My |
| 查看狀態 | `driver/wc3119-setup.ps1 -Action Status` | 唯讀 |
| 信任憑證 | `driver/wc3119-setup.ps1 -Package <dir> -TrustCertificate -Apply` | LocalMachine Root＋TrustedPublisher 各加一張憑證，只接受本專案主體名稱 |
| 安裝 | `driver/wc3119-setup.ps1 -Package <dir> -Action Install -Apply` | 備份 → `pnputil /add-driver … /install` → 驗證 MI_00 為 Image 類別、服務仍 WINUSB、CLSID 已登錄、WIA 看得到裝置 |
| 更新 | 改 INF `DriverVer` 版本 → 重新打包 → `-Action Update -Apply` | 要求版本比已安裝新；安裝新套件後刪除舊的 `oem*.inf`；要求重開機時先停下回報 |
| 解除安裝 | `driver/wc3119-setup.ps1 -Action Uninstall -Apply` | `pnputil /delete-driver oemN.inf /uninstall /force` → 移除 HKCR CLSID 鍵（INF 的 HKCR AddReg 不會自動清） → 核對父裝置與 MI_01 未變 |
| 移除信任 | `-UntrustCertificate -Apply` | 只在沒有本專案套件時允許，移除兩個機器儲存區的測試憑證 |

安裝腳本的保護：只認 `wc3119-wia.inf` 且 Provider 為本專案的套件；MI_00 必須是唯一在線介面且目前為 WinUSB 或無驅動；父裝置與 MI_01 的 Service、INF、問題碼、ClassGuid、Parent 在每次操作後與備份比對，不同就報錯；`pnputil` 回 3010 時不自動重開機。腳本本身不會自動重試安裝。

解除安裝後 MI_00 會回到沒有驅動（問題碼 28），因為內建 `winusb.inf` 不匹配這個硬體 ID。若還要以開發工具存取 USB，需重新以 [配對工具](../examples/winusb_setup.rs) 綁定內建 WinUSB。這是與 WinUSB 開發配對並存的既有限制。

已實跑（2026-09-19，本開發機）：winget 安裝 WDK 10.0.26100 取得 Inf2Cat；`package.ps1 -NewTestCertificate` 產出簽署套件；`-TrustCertificate -Apply` 匯入兩個機器儲存區；`-Action Install -Apply` 安裝 0.2.0.0（pnputil 回 0、oem19.inf、MI_00 轉為 Image 類別且服務仍 WINUSB、CLSID 指向驅動存放區）；之後以 `-Action Update -Apply` 連續更新到 0.2.6.0，每次刪除前一版 oem inf。測試憑證簽署不需要 testsigning 的推論已由實際安裝證實。pnputil 回 3010 時腳本會停止 stisvc、`/remove-device` 該介面節點、`/scan-devices`，若 MI_00 沒回來就 `/restart-device` usbccgp 父裝置（MI_01 會一併重啟，之後比對未變），2026-09-19 實測不需重開機即完成更新。解除安裝已實跑一次：`-Action Uninstall -Apply` 時 WIA 服務仍載著 DLL 與 WinUSB 句柄，pnputil 回 3010（需重開機），裝置節點進入「等待重開機」狀態；此狀態下立即 `Install` 也回 3010，重啟 stisvc 後 `pnputil /restart-device` 被拒（pending reboot），WIA 連線失敗直到重開機。腳本已改為解除安裝前先停止 stisvc 並在之後 `/scan-devices`，但尚未在乾淨狀態重跑驗證；Install 預檢改為接受「無驅動」的暫態。重開機後 Status 與 WIA 掃描恢復正常，證明該循環可復原。移除信任尚未實跑。第一次 Install 曾因腳本用英文解析中文版 pnputil 輸出而誤報失敗，已改成掃描 `oem*.inf` 內容判斷；Update 另加入重啟 stisvc，否則服務仍持有舊 DLL。

### 授權後的執行與復原順序

1. 安裝 WDK（免費）取得 Inf2Cat，執行 `package.ps1 -NewTestCertificate`，記錄套件目錄、DLL／catalog SHA256 與憑證指紋。
2. `wc3119-setup.ps1 -Action Status` 確認 MI_00 仍是 WinUSB、問題碼 0。
3. 取得授權後 `-TrustCertificate -Apply`，再 `-Action Install`（先預檢，再 `-Apply`）。
4. 驗證：腳本內建檢查通過後，用 Windows 掃描做一次 75 dpi 掃描、一次取消、再掃一次；同時看 Windows Image Acquisition 事件紀錄有沒有載入錯誤。
5. 失敗時不重試安裝：先 `-Action Uninstall -Apply` 復原，再依日誌調整 INF 或 DLL，重新打包並提高 `DriverVer`。
6. 完全不需要時：`Uninstall` → `-UntrustCertificate -Apply` → 需要 USB 開發存取再重新配對 WinUSB。

## 正式安裝套件

### 程式化配對的已知限制

本機 `winusb.inf` 的一般模型匹配 `USB\MS_COMP_WINUSB`，沒有 3119 的硬體 ID，因此一般 `pnputil /add-driver /install` 不會自動完成這次配對。2026-09-13 已用 [Rust 配對工具](../examples/winusb_setup.rs) 實測：MI_00 的裝置專屬 CLASS 清單包含 3 個候選，只有 1 個符合內建 `winusb.inf`、`WINUSB` 區段、Microsoft 與 `USB\MS_COMP_WINUSB`。其他 BILLBOARD／ADB 候選均被排除。預檢前後 MI_00 維持無服務／INF、問題碼 28。

不帶參數執行 `cargo run --offline --example winusb_setup` 只列舉候選。已實作的 `--install-mi00` 路徑要求完整預期裝置 ID，再次核對唯一在線裝置、無既有驅動及專屬清單候選，才呼叫 [DiInstallDevice](https://learn.microsoft.com/en-us/windows/win32/api/newdev/nf-newdev-diinstalldevice)。它不負責備份、GUID 登錄或復原，不是可分發的完整安裝套件。執行前仍需完成本文件的授權與備份要求；回傳 3010 表示 Windows 要求重新開機，工具不會自動重開機。

不可改造 USB 裝置的硬體 ID 或相容 ID 來假冒 WinUSB 相容裝置。[Microsoft 屬性限制](https://learn.microsoft.com/en-us/windows/win32/api/setupapi/nf-setupapi-setupdisetdeviceregistrypropertyw)也不允許直接以該 API 寫入保留的 CLASSGUID、CLASS、SERVICE 屬性。

`DiInstallDevice` 的 `DIIDFLAG_INSTALLNULLDRIVER` 可解除指定裝置綁定，但會移除該裝置的設定，不能當成未備份時的完整還原。原本沒有驅動時也不能依賴 [DiRollbackDriver](https://learn.microsoft.com/en-us/windows/win32/api/newdev/nf-newdev-dirollbackdriver) 找到舊版本。執行前仍須完成上方的精確備份及復原檢查。

### 套件驗收

需使用 WDK 驗證 INF、產生 catalog 並完成適用的簽署程序。正式驗收包含一般使用者掃描、重新開機、USB 換孔、升級、解除安裝及 Windows 掃描整合。付費簽署、送審或對外發布需另外授權。
