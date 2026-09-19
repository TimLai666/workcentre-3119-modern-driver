# 掃描介面安裝方案

本目錄是可安裝套件的來源，包含 `wc3119-wia.inf`（Image 類別、WinUSB 函式驅動、WIA minidriver 登錄）、`package.ps1`（打包、Inf2Cat、測試憑證簽署）、`wc3119-setup.ps1` 與 `install.cmd`／`uninstall.cmd`（一鍵安裝）。一般使用流程見 [README](../README.md#安裝一般使用者)，這份文件記錄的是授權範圍、系統變更細節與實跑證據。`wc3119-winusb.inf` 與 `examples/winusb_setup.rs` 是開發機第一次配對 WinUSB 用的舊路徑，正式套件不需要它們。

2026-09-13 使用者已核准在本機把已備份的 MI_00 綁定內建 WinUSB、登錄下列裝置介面 GUID，並重新啟動該介面，用途是做實機通訊測試。

## 可核對的變更範圍

- 唯一目標是 `USB\VID_0924&PID_4265&MI_00`。配對前它沒有驅動、問題碼 28，配對後問題碼為 0。
- 本機已完成的綁定對象是 Windows 內建的 Microsoft WinUSB。
- 已登錄的裝置介面 GUID 是 `{C4147E4A-9C41-4846-A53C-5E625C68021A}`，供 Rust 程式列舉與開啟裝置。
- 父裝置 `usbccgp` 與 MI_01 的 `usbprint` 必須維持原狀。
- 這個變更只提供 USB 存取，不會自動新增 WIA、Windows 掃描或列印功能。

## 開發機驗證路徑

[Microsoft 文件](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/winusb-installation)說明怎麼從裝置管理員選擇內建的 WinUSB，也說明手動配對之後要登錄裝置介面 GUID。這台電腦已經有 Microsoft 簽署的 `winusb.sys` 與 `winusb.inf`，但還沒驗證裝置管理員會不會接受這次配對。

取得明確授權之後：

1. 重新查詢所有 Xerox 介面，要求結果只有一個 MI_00，並核對它目前的服務與 INF。狀態如果已經改變，不能沿用舊觀測強制覆蓋。
2. 在本機保存精確的裝置 ID、原本的服務、INF、問題碼，以及即將更動的 DeviceInterfaceGUIDs 原值。這些資料可能包含序號，不能提交到 Git。
3. 只替 MI_00 選擇內建的 WinUSB，並登錄上述 GUID。系統如果要求不可信的簽章，或無法提供內建配對，就停止這個動作，不能改去停用安全設定。
4. 重新連接之後，確認 MI_00 使用 WinUSB、問題碼為 0，再核對 MI_01 與父裝置沒有變動。
5. 執行 `wc3119 inquiry`，驗證 VID／PID、介面號、bulk 端點與 INQUIRY 回覆。本機這條路徑已經成功，詳見[硬體紀錄](../docs/hardware.md)。這一步不送出掃描命令。

配對期間掃描介面可能暫時失效，需要重新連接 USB。這台機器原本就沒有可用的掃描驅動，所以這次的復原目標是回到原本沒有綁定的狀態。

本機這次是經 UAC 用已審查的 Rust 工具完成配對，接著登錄 GUID 並只重新啟動 MI_00，全部回傳 0，也不需要重開機。父裝置與 MI_01 的屬性和備份相符。這是單一已備份裝置實例的開發驗證：同一台機器配對完成後，不能再跑一次那個只接受「無驅動」狀態的安裝工具。正式套件要依型號與功能介面配對，並依 [05 驗收](../docs/tickets/05-windows-install.md)在其他電腦與其他 USB 接孔實測。

## 復原條件

配對後如果出現錯誤、目標不符，或使用者要求還原，只移除這次對 MI_00 的綁定與新增的 GUID，並依備份恢復原值。不能刪除 Windows 共用的 WinUSB 套件、父裝置、MI_01 或其他 USB 裝置。重新偵測之後要和基準狀態核對。這套復原流程還沒有實測過，執行前必須先確認精確命令與備份都齊全。

## WIA 登錄方案（已於開發機實裝）

2026-09-19 依本機的 `C:\Windows\INF\sti.inf`、`winusb.inf` 與 [Microsoft WIA INF 規則](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/inf-files-for-wia-devices)完成 [WIA INF 設計稿](wc3119-wia.inf)。它和現有的 WinUSB 設計稿有以下差異：

- Class 改成 `Image`，讓 still-image 類別安裝程式（sti_ci）處理 `SubClass=StillImage`、`DeviceType=1`、`Capabilities=0x10`（STI_GENCAP_WIA）、`Events` 與 `DeviceData`。
- 函式驅動還是 `Include=winusb.inf` 的 WinUSB，並沿用同一個裝置介面 GUID。這裡不引用 `STI.USBSection`，因為它會加入 `usbscan.sys` 服務並改變傳輸方式。
- `AddReg` 寫入 `HardwareConfig=1,4`、`CreateFileName=AUTO`、`USDClass`，以及 HKCR `CLSID\{F71A8435-…}\InProcServer32` 指向驅動存放區（`%13%`）裡的 `workcentre_3119.dll`，ThreadingModel 為 Both。minidriver 不使用 port name，它自己以 GUID 列舉介面，所以 AUTO 和現有實作一致。
- 事件只宣告連線與斷線，和 `drvGetCapabilities` 一致。驅動不會自己發送事件。
- `DeviceInterfaceGUIDs` 同時列出專案 GUID 與 `GUID_DEVINTERFACE_IMAGE`。WinUSB 會登錄這兩個介面，後者讓 WIA 服務寫入 `DEVPKEY_WIA_DeviceType`，也讓 WinRT `Windows.Devices.Scanners`（Windows 掃描 App 用的 API）能以介面類別找到裝置。代價是 WIA 服務會試著在那個介面上開啟通知句柄，而 WinUSB 只允許一個句柄，所以驅動在 `IStiUSD::Initialize` 就先開啟並保留 USB 句柄（見 ENG.md）。

還沒查證、必須靠實測確認的前提有四項：`Image` 類別搭配 WinUSB 函式驅動，類別安裝程式會不會接受並建立 StillImage 裝置介面；WIA 服務帳號 `NT Authority\LocalService` 能不能開啟 WinUSB 裝置介面；DLL 目前還會匯入 `VCRUNTIME140.dll` 與 UCRT，服務帳號載入時的 runtime 前置條件是什麼；`stisvc` 目前是 Stopped，實測時服務會由 PnP 事件啟動。

### 簽署（免費路線）

使用者已經決定不花錢，所以不走 EV 憑證與 attestation。依 [PnP 裝置安裝簽署要求](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/pnp-device-installation-signing-requirements--windows-vista-and-later-)，開發與測試階段的套件可以用測試憑證簽署 catalog。`bcdedit testsigning` 只影響核心模式二進位檔的載入，而本套件沒有自己的核心驅動（函式驅動是 Microsoft 簽署的 WinUSB，本專案只有使用者模式的 DLL），所以不啟用 testsigning，也不停用 Secure Boot。這個推論當時還沒有用實際的 `pnputil` 安裝驗證過：PnP 如果還是拒絕，錯誤會停在 `/add-driver`，不會留下半套狀態。參考 [Test-signing 說明](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/the-testsigning-boot-configuration-option)與[測試簽署套件](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/test-signing-driver-packages)。

需要的免費工具有三樣：Windows SDK 的 `signtool`（本機已有）、WDK 的 `Inf2Cat`（本機當時還沒裝，要另外下載 WDK），以及 PowerShell `New-SelfSignedCertificate` 產生的自簽程式碼簽署憑證。

代價是每台要安裝的電腦都得先把這張測試憑證放進 LocalMachine 的 Trusted Root 與 Trusted Publishers。這是機器層級的安全設定變更，每台都要使用者明確授權。套件因此只適合自用或少數幾台信任的電腦，不能公開分發。

### 套件、安裝、更新、解除安裝

一般使用者只需要套件目錄。`package.ps1` 會把 `wc3119-setup.ps1`、`install.cmd`、`uninstall.cmd` 與 `INSTALL.txt` 一起放進 `artifacts/wia-package-<版本>-<時間>/`，整個目錄複製到目標電腦後，對 `install.cmd` 按兩下（UAC 提權）就會信任憑證、安裝或更新套件、重啟 WIA 服務並驗證，`uninstall.cmd` 則反向移除套件、CLSID 與憑證信任。套件模式下腳本以自己所在的目錄當作 `-Package`，日誌寫在 `%ProgramData%\WorkCentre3119Driver\setup-logs`。掃描器沒接上時只會先把驅動放進驅動存放區，接上之後 Windows 會自動綁定。同一個版本重跑只做檢查。MI_00 如果被別的掃描驅動綁走，腳本會停下來並提示先移除那個驅動。2026-09-19 在開發機以 `uninstall.cmd`（含移除信任）接 `install.cmd` 完整實跑過一次，之後 Windows 掃描 App 掃描成功。

以下是開發者用法。兩支腳本都在本目錄，預設只做預檢，加 `-Apply` 才會改系統，而且需要提升權限的 PowerShell。在儲存庫內執行時，每次都會在 Git 排除的 `artifacts/wia-setup-*` 留下日誌與備份。

| 步驟 | 指令 | 改動範圍 |
| --- | --- | --- |
| 打包簽署 | `driver/package.ps1 -NewTestCertificate`（之後用 `-CertificateThumbprint`） | 只寫入 `artifacts/wia-package-<版本>-<時間>/`，憑證只進 CurrentUser\My |
| 查看狀態 | `driver/wc3119-setup.ps1 -Action Status` | 唯讀 |
| 信任憑證 | `driver/wc3119-setup.ps1 -Package <dir> -TrustCertificate -Apply`（可以和 Install／Update 同一次執行） | LocalMachine Root 與 TrustedPublisher 各加一張憑證，只接受本專案的主體名稱，已存在就略過 |
| 安裝 | `driver/wc3119-setup.ps1 -Package <dir> -Action Install -Apply` | 先備份，再 `pnputil /add-driver … /install`，然後驗證 MI_00 是 Image 類別、服務還是 WINUSB、CLSID 已登錄、WIA 看得到裝置。已裝舊版會自動走更新，同版本只做驗證 |
| 更新 | 改 INF 的 `DriverVer` 版本，重新打包，再 `-Action Update -Apply` | 要求版本比已安裝的新。安裝新套件後刪除舊的 `oem*.inf`，系統要求重開機時先停下來回報 |
| 解除安裝 | `driver/wc3119-setup.ps1 -Action Uninstall -Apply`（加 `-UntrustCertificate` 一併移除信任） | `pnputil /delete-driver oemN.inf /uninstall /force`，移除 HKCR 的 CLSID 鍵（INF 的 HKCR AddReg 不會自動清掉），再核對父裝置與 MI_01 沒有變動 |
| 移除信任 | `-UntrustCertificate -Apply` | 只在機器上沒有本專案套件時允許，移除兩個機器儲存區裡的測試憑證 |

安裝腳本有幾道保護。它只認 `wc3119-wia.inf` 而且 Provider 是本專案的套件。MI_00 必須是唯一在線的介面，而且目前是 WinUSB 或沒有驅動。父裝置與 MI_01 的 Service、INF、問題碼、ClassGuid、Parent 在每次操作後都要和備份比對，不同就報錯。`pnputil` 回 3010 時不會自動重開機，腳本本身也不會自動重試安裝。

解除安裝之後 MI_00 會回到沒有驅動的狀態（問題碼 28），因為內建的 `winusb.inf` 不匹配這個硬體 ID。如果還要用開發工具存取 USB，需要重新用[配對工具](../examples/winusb_setup.rs)綁定內建 WinUSB。這是和 WinUSB 開發配對並存的既有限制。

已實跑的紀錄（2026-09-19，本開發機）：

- 用 winget 安裝 WDK 10.0.26100 取得 Inf2Cat，`package.ps1 -NewTestCertificate` 產出簽署套件，`-TrustCertificate -Apply` 匯入兩個機器儲存區。
- `-Action Install -Apply` 安裝 0.2.0.0 成功：pnputil 回 0、產生 oem19.inf、MI_00 轉為 Image 類別而服務還是 WINUSB、CLSID 指向驅動存放區。之後以 `-Action Update -Apply` 連續更新到 0.2.6.0，每次都刪除前一版的 oem inf。測試憑證簽署不需要 testsigning 的推論，已經由實際安裝證實。
- pnputil 回 3010 時，腳本會停止 stisvc、對該介面節點 `/remove-device`、再 `/scan-devices`，MI_00 如果沒回來就 `/restart-device` usbccgp 父裝置（MI_01 會一起重啟，之後比對沒有變動）。這天實測不需要重開機就完成更新。
- 解除安裝已實跑一次。`-Action Uninstall -Apply` 時 WIA 服務還載著 DLL 與 WinUSB 句柄，pnputil 回 3010（需重開機），裝置節點進入「等待重開機」狀態。這個狀態下立刻 Install 也回 3010，重啟 stisvc 後 `pnputil /restart-device` 被拒（pending reboot），WIA 連線一直失敗到重開機為止。腳本已經改成解除安裝前先停止 stisvc、之後再 `/scan-devices`，但還沒有在乾淨狀態下重跑驗證，Install 的預檢也改為接受「無驅動」這個暫態。重開機後 Status 與 WIA 掃描都恢復正常，可見這個循環可以復原。
- 移除信任還沒有實跑過。
- 第一次 Install 曾經誤報失敗，原因是腳本用英文去解析中文版 pnputil 的輸出，已改成掃描 `oem*.inf` 的內容來判斷。Update 另外加入重啟 stisvc，否則服務會一直持有舊的 DLL。

### 授權後的執行與復原順序

1. 安裝 WDK（免費）取得 Inf2Cat，執行 `package.ps1 -NewTestCertificate`，記錄套件目錄、DLL 與 catalog 的 SHA256，還有憑證指紋。
2. 執行 `wc3119-setup.ps1 -Action Status`，確認 MI_00 還是 WinUSB、問題碼 0。
3. 取得授權後執行 `-TrustCertificate -Apply`，再執行 `-Action Install`（先預檢，再加 `-Apply`）。
4. 驗證：腳本內建的檢查通過之後，用 Windows 掃描做一次 75 dpi 掃描、取消一次、再掃一次，同時看 Windows Image Acquisition 的事件紀錄有沒有載入錯誤。
5. 失敗時不要重試安裝。先用 `-Action Uninstall -Apply` 復原，再依日誌調整 INF 或 DLL，重新打包並提高 `DriverVer`。
6. 完全不需要時，依序執行 `Uninstall` 與 `-UntrustCertificate -Apply`。之後如果還需要 USB 開發存取，再重新配對 WinUSB。

## 正式安裝套件

### 程式化配對的已知限制

本機 `winusb.inf` 的一般模型匹配的是 `USB\MS_COMP_WINUSB`，裡面沒有 3119 的硬體 ID，所以一般的 `pnputil /add-driver /install` 不會自動完成這次配對。2026-09-13 已用 [Rust 配對工具](../examples/winusb_setup.rs)實測：MI_00 的裝置專屬 CLASS 清單有 3 個候選，只有 1 個符合內建的 `winusb.inf`、`WINUSB` 區段、Microsoft 與 `USB\MS_COMP_WINUSB`，其他 BILLBOARD 與 ADB 候選都被排除。預檢前後 MI_00 都維持沒有服務與 INF、問題碼 28。

不帶參數執行 `cargo run --offline --example winusb_setup` 只會列舉候選。已實作的 `--install-mi00` 路徑會要求完整的預期裝置 ID，再核對一次唯一在線裝置、沒有既有驅動，以及專屬清單候選，之後才呼叫 [DiInstallDevice](https://learn.microsoft.com/en-us/windows/win32/api/newdev/nf-newdev-diinstalldevice)。它不負責備份、GUID 登錄或復原，不是可分發的完整安裝套件。執行前還是要完成這份文件的授權與備份要求。它回傳 3010 表示 Windows 要求重新開機，工具不會自己重開機。

不可以改造 USB 裝置的硬體 ID 或相容 ID 來假冒 WinUSB 相容裝置。[Microsoft 的屬性限制](https://learn.microsoft.com/en-us/windows/win32/api/setupapi/nf-setupapi-setupdisetdeviceregistrypropertyw)也不允許用那個 API 直接寫入保留的 CLASSGUID、CLASS、SERVICE 屬性。

`DiInstallDevice` 的 `DIIDFLAG_INSTALLNULLDRIVER` 可以解除指定裝置的綁定，但它會移除該裝置的設定，不能當成沒有備份時的完整還原。裝置原本就沒有驅動時，也不能指望 [DiRollbackDriver](https://learn.microsoft.com/en-us/windows/win32/api/newdev/nf-newdev-dirollbackdriver) 找得到舊版本。執行前還是要完成上面的精確備份與復原檢查。

### 套件驗收

需要用 WDK 驗證 INF、產生 catalog，並完成適用的簽署程序。正式驗收包含一般使用者掃描、重新開機、USB 換孔、升級、解除安裝，以及 Windows 掃描整合。付費簽署、送審或對外發布要另外取得授權。
