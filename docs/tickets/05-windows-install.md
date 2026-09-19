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

Rust 已實作 [BMP 串流編碼](../../src/bitmap.rs)，沿用現有掃描 callback，部分寫入、錯誤、取消及工作釋放已有離線測試與部分實機證據。WIA 2.0 裝置的預設傳輸格式須為 BMP，但完成 BMP 編碼不等於完成 WIA 傳輸。[Microsoft WIA 格式屬性](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ipa-format)

原生 [IStream 輸出轉接](../../src/com_stream.rs) 及 [WIA 數值設定入口](../../src/wia.rs) 已連到實際掃描。Cargo 產生的 COM DLL 提供 class factory、IUnknown、IStiUSD 與 IWiaMiniDrv 共用生命週期，已通過動態載入測試。[IStiUSD](../../src/com_server/sti.rs) 已支援初始化、指定裝置鎖定及 INQUIRY 診斷。IWiaMiniDrv 項目樹、服務 IStiDevice 鎖定、服務屬性讀取與 drvAcquireItemData 已接上同一 USB session 的回呼傳輸。屬性初始化已接上當次能力查詢。相依更新已接上，狀態及服務整合未完成，尚不能提供 Windows 掃描。WIA2 串流只保證 `Write`、`Seek`、`SetSize`，不得依賴呼叫端提供完整檔案功能。BMP 的 `finish` 成功後位置為 byte 2，WIA 服務端的定位及影像消費行為仍待驗證。[Microsoft WIA 介面](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-minidriver-interfaces)、[COM 識別契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/providing-a-com-interface)、[IStream 契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/istream-data-transfer-driver-changes)

目前 `FlatbedSettings` 驗證數值快照，支援六種對稱解析度、8-bit 灰階／24-bit 彩色、中性亮度／對比及無壓縮 BMP。位置按協定精確步進，範圍另由當次 INQUIRY 限制，詳見 [設定契約](../../ENG.md#wia-設定與掃描入口)。WIA 屬性初始化已建立初始有效值範圍、色深、通道及逐像素排列，相依更新已接上，仍須由真正服務驗收設定切換。[Microsoft DATATYPE](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ipa-datatype) 實機指定 600×800 像素後回傳 600×801，必須釐清裝置幾何及 WIA 選取範圍的處理，不能把 BMP 成功當成範圍驗收。

本機 `sc.exe qc stisvc` 確認 WIA 服務帳號為 `NT Authority\LocalService`。一般使用者的 Rust 掃描成功不證明該服務帳號也能開啟 WinUSB。第一階段開發不登錄 COM、不修改服務或 USB 權限，也不把離線契約測試當成 Windows 掃描驗收。

原生 `GetNextStream` 已透過測試 callback 取得 Windows OLE 串流，尚未取得 WIA 服務提供的串流。服務端目的串流的 `Seek(0, END)`、完成後定位及影像消費仍要實測。`GetNextStream`／`SendMessage` 的 S_FALSE 與 WIA_STATUS_SKIP_ITEM 已分別處理，一般 IStream 的非 S_OK 回覆及 Interrupted 保留為錯誤。[GetNextStream](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wiamindr_lh/nf-wiamindr_lh-iwiaminidrvtransfercallback-getnextstream)、[SendMessage](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wiamindr_lh/nf-wiamindr_lh-iwiaminidrvtransfercallback-sendmessage)

### 原生回呼驗收矩陣

| 情境 | 行為與證據 |
| --- | --- |
| 空指標、QI 失敗、S_OK 卻無串流、無效設定、未鎖定 | 回報錯誤，不啟動掃描，callback 與 STI 前置檢查測試通過 |
| 開始前取消、SKIP、非空目的串流 | 不啟動掃描，保留既有內容，流程離線測試通過 |
| 正常影像及完成進度 | 逐塊 BMP，bytes 含色盤／填補，清理及 finish 成功後才 100，合成像素與實機驗證通過 |
| SendMessage 取消或錯誤 | 停止交付並清理，只有 Ready 可回取消；原始 HRESULT 與清理診斷保留，取消另有實機重掃證據 |
| 清理失敗、最終進度取消 | 不回報 Completed，清理失敗保持隔離，離線測試通過 |
| QI／GetNextStream／SendMessage／Release 重入 | 查詢可返回，掃描／解鎖回忙碌，實機測試通過 |
| 原生參考釋放 | callback QI 與 stream owned ref 各自釋放，HGLOBAL 原生計數回到測試保留的一個參考 |
| WIA 服務與等待期間取消 | 原生取消事件已接上；500 ms 提早取消可返回 S_FALSE，但立即重掃尚未通過。真實服務 context／排程未驗證 |

單張平台的 skip 發生於 RESERVE 前，沒有開始的頁面需要排空。驅動僅發 STATUS，END_OF_STREAM／END_OF_TRANSFER 由服務發送。[Microsoft 傳輸常數](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-transfer-constants)

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

### 全組合驗收與貼齊值寫回

2026-09-19 使用者要求測完所有選項組合。以桌面 WinRT `Windows.Devices.Scanners`（Windows 掃描 App 的同一條路徑）自動跑：6 種解析度 × 灰階／彩色整版，加上 App 選區會產生的英寸起點（0.0117 × 0.0233 英寸、1.5 × 1.0 英寸）在 6 種解析度。檔案格式不在驅動範圍：驅動只提供 DIB，WinRT `IsFormatSupported` 只回 DIB，PNG／JPEG／TIFF／XPS 都是 App 自己轉檔，App 端 PNG 已實掃過。

整版 12 組全部成功（套件 0.2.16.0）：75 dpi 灰階 7.3 秒到 600 dpi 彩色 105 秒，尺寸 648×871 … 5100×6961。選區 6 組第一輪全部失敗：wiatrace 顯示 App 寫 YPOS=1（75 dpi，步進 3），驅動已在內部貼齊成 0，但差異寫入以「舊值」為基準，貼齊後的 0 等於舊值就沒寫回，服務項目仍持有 1，`wiasValidateItemProperties` 依範圍屬性拒絕（0x80070057）。修正：[catalog.rs](../../src/com_server/minidrv/properties/catalog.rs) 新增 `with_service_values`，差異基準的值改用服務目前持有的值（屬性基準仍用舊狀態），先以「YPOS 1→0 仍須寫回」的失敗測試取得 RED 再實作。套件 0.2.17.0（DLL SHA256 `BF5274D9FC5D4C321F2BD8E7838D826B72D1A6147FC53790530724C915405446`）重跑：選區 6 組全部成功（7–12 秒），整版 75 dpi 灰階仍成功。

已知未處理：驗證失敗後驅動會隔離該 COM 物件（`drvUnLockWiaDevice` 回 E_UNEXPECTED），服務隨即把項目視為離線，同一個 App 連線之後的所有寫入都回 WIA_ERROR_OFFLINE，要重新連線才恢復；對 Microsoft 用戶端在修正後不會觸發，但仍列為待改善（失敗時回寫舊值而不隔離）。選區輸出寬度比要求多幾個像素（1.5 英寸在 75 dpi 要求 112、得到 120）屬 02 幾何驗收既有項目。

### Windows 掃描 App 600 dpi 彩色與起點貼齊

2026-09-19 使用者回報 App 選 PNG 600 dpi 會錯誤。wiatrace：App 寫入 600 dpi 後再寫 DATATYPE 3／DEPTH 24／XPOS 7／YPOS 0／XEXTENT 3729／YEXTENT 4015，`drvValidateItemProperties` 回 0x80070057；同一區域在 300 dpi 是 XPOS 3、75 dpi 是 XPOS 0 都成功。原因：XPOS 7 在 600 dpi 是 14 個 1/1200 英寸單位，不是協定要求的 1/100 英寸（12 單位）倍數，而驅動對明確寫入的起點採「無法表示就拒絕」。Windows 用戶端以英寸區域換算再四捨五入成像素，本來就不會對齊驅動的步進，因此 [validation.rs](../../src/com_server/minidrv/properties/validation.rs) 改為把明確起點貼齊最近的硬體步進（位移小於一個步進），貼齊後若明確範圍放不下就改用下一個較小步進，仍放不下才拒絕；範圍值本身不改。測試先以 XPOS 7@600→6、XPOS 1@75→0、邊界退位與放不下拒絕取得 RED 再實作；原本「明確不可表示位置直接拒絕」的測試前提已不成立，改寫為貼齊測試。

實機（套件 0.2.16.0，DLL SHA256 `D4C7D9854C1004388B2C5F6E27D7829707DD36642269BA8068E849A8D4E61D3D`，以套件 `Install` 自動從 0.2.15.0 更新）：App 彩色 600 dpi 整版掃描通過驗證，`drvAcquireItemData` 約 93 秒回 S_OK，產生 `掃描_20260919.png` 5100×6961 24 bpp（132776 bytes，空平台）。要求 7020 列只回 6961 列（11.6 英寸）是既有的平台長度差異，屬 02 幾何驗收，不在本次範圍。

### 一鍵安裝套件

2026-09-19 使用者要求「所有要做的事都包含在安裝程式裡，一裝即用」。[package.ps1](../../driver/package.ps1) 現在把 [wc3119-setup.ps1](../../driver/wc3119-setup.ps1)、[install.cmd](../../driver/install.cmd)、[uninstall.cmd](../../driver/uninstall.cmd) 與 [INSTALL.txt](../../driver/INSTALL.txt) 一起放進套件；`.cmd` 自行要求 UAC 提權後呼叫 `-Action Install -TrustCertificate -Apply`／`-Action Uninstall -UntrustCertificate -Apply`，結束時停在視窗顯示結果。安裝腳本改動：套件模式（旁邊有 manifest.json）自動以所在目錄為套件、日誌寫 `%ProgramData%\WorkCentre3119Driver\setup-logs`；憑證信任可與安裝同一次執行且已存在則略過；Install 對已裝舊版自動更新、同版只驗證、更新版已裝則拒絕；掃描器未接上時只暫存套件（pnputil 259 視為成功），接上後由 Windows 綁定；MI_00 被其他驅動綁定時明確提示；Uninstall 可一併移除信任。修正一個實跑發現的 bug：安裝函式的日誌輸出曾混進回傳值，改以 script 變數傳回結束碼。

開發機實跑：把套件複製到 `%TEMP%` 模擬新電腦，`uninstall.cmd` 移除套件、CLSID 與兩張憑證（Status 顯示未安裝、信任 0、WIA 0 台），`install.cmd` 匯入憑證→安裝→重啟 stisvc→WIA 1 台→提示 Windows 掃描 App／傳真和掃描皆已安裝，exit 0；再跑一次走「已安裝，只驗證」路徑。之後 Windows 掃描 App 直接連線並掃描成功（存到使用者設定的「掃描的文件」）。尚未在第二台乾淨電腦實跑；原廠驅動已綁定時的處理只提示不自動移除。

### Windows 掃描 App 實掃與 YRES 清單

2026-09-19：Windows 掃描 App（Microsoft.WindowsScan 6.3.9654）完成一次平台灰階 75 dpi 掃描（`掃描_20260919.png` 648×871）。此前 App 一直顯示「連線到掃描器時發生問題」，而 wiatrace、ETW WinRT-Error、Process Monitor 都沒有失敗證據。最後以 WDK cdb 附加載有 `Windows.Devices.Scanners.dll` 的 RuntimeBroker（`tasklist /m` 找出），用 `bm` 對 `Windows::Devices::Scanners::*Server::*` 記錄呼叫、在返回位址一次性中斷讀 HRESULT，並以停止 stisvc→`pnputil /remove-device`→`/scan-devices` 觸發 App 重新連線：

| 觀察 | 原因 | 修正 |
| --- | --- | --- |
| `FromIdAsync`、`get_FlatbedConfiguration`、`RegularInputSource::Initialize` 全部 S_OK；App 讀 Min／Max／Optical 後 `put_DesiredResolution(100,100)` 在 broker 內回 0x80070057 並 RoOriginateError，服務端沒有 WriteMultiple | WIA_IPS_YRES 有效清單只列目前值 [75]，WinRT 以初始化時快取的 X／Y 清單在客戶端驗證 DesiredResolution | [catalog.rs](../../src/com_server/minidrv/properties/catalog.rs) X／Y 都列完整清單，`with_settings` 只移動 nominal；[validation.rs](../../src/com_server/minidrv/properties/validation.rs) y-only 寫入改為兩軸跟隨，X／Y 同時寫入且不同仍拒絕 |

注意：服務持有 WinUSB 句柄時 `pnputil /disable-device` 會回 3010 並把 ConfigFlags 設為 DISABLED（下次重開機才生效），須用上述 remove／scan 流程復原。DLL SHA256 `9363EB564DD10CE0E094103A430E5F117A42D6249B9781379FB34F4EF7DDD041`，套件 0.2.15.0。App 的預覽、彩色與取消尚未驗收。

### Windows 傳真和掃描實掃

2026-09-19：啟用 Windows 選用功能 Print.Fax.Scan 後，以「Windows 傳真和掃描」完成一次平台彩色 75 dpi 掃描（`影像.jpg` 648×871 24 bpp）。這是第一個非本專案、非程式碼呼叫的既有掃描軟體驗收。逐步修正（皆有先失敗的 wiatrace／UI 證據）：

| 症狀 | 原因 | 修正 |
| --- | --- | --- |
| 「無法初始化選取的掃描器」，trace 在讀 Brightness 後中止 | WIA_IPS_BRIGHTNESS／CONTRAST 有效範圍 0..0，UI 無法建立滑桿 | 依 Microsoft 契約改為 −1000..1000、中性 0；[wia.rs](../../src/wia.rs) `Tone` 以單一查表實作，中性不改像素，`wire_data` 永不改寫 |
| 掃描設定檔提示、Document Handling Select／Show preview control／Segmentation 讀到 VT_EMPTY | 舊版 DPS 屬性與 UI 提示屬性缺少 | 補 3088（FLATBED，唯讀）、3103（DONT_SHOW）、6164（DONT_USE_SEGMENTATION_FILTER） |
| 「將設定套用到驅動程式時發生錯誤」，WriteMultiple 6157 回 E_INVALIDARG | UI 每次掃描前寫 WIA_IPS_ROTATION | 補 6157 清單 [PORTRAIT]，只接受 0 |

亮度／對比查表：`out = clamp(round((in−128)·(contrast+1000)/1000 + 128 + brightness·255/1000))`，單調不減，測試涵蓋中性為 None、±1000 極值、對比 0 收斂到 128、`wire_data` 不變。這是驅動端唯一的明暗轉換，07 的「不預設壓暗」維持不變，實際明暗品質仍待有原稿後驗收。DLL SHA256 `C18D711611811B7B09EC0A6852E7BCD7E4B56ECDDA35CC7BD37AE273C07838C7`，套件 0.2.14.0。Windows 掃描 App 仍失敗（見 delivery-status 阻礙）。

### 影像裝置介面、句柄保留與 Windows 掃描 App

2026-09-19：為了讓 WinRT `Windows.Devices.Scanners`（Windows 掃描 App 的 API）找到裝置，INF `DeviceInterfaceGUIDs` 加入 `GUID_DEVINTERFACE_IMAGE`，WIA 服務因此能寫入 `DEVPKEY_WIA_DeviceType`，WinRT 選擇器可列舉並連線。副作用：服務會在該介面開啟通知句柄，而提權探針（停止 stisvc 後以 P/Invoke 開啟）證實 WinUSB 每台裝置只允許一個開啟中的句柄，任何第二次開啟都回 ERROR_ACCESS_DENIED；因此 [sti.rs](../../src/com_server/sti.rs) 改為 Initialize 成功即開啟 USB 並保留到 Release，`UnLockDevice` 只清除 STI 鎖旗標；[usb.rs](../../src/usb.rs) 的 CreateFile 改為讀寫共用（對 WinUSB 沒有差別，但不再宣稱 OS 層獨占）。DLL SHA256 `C3DD8218E3B7901C0FDB2589CFD808C546A392272AEF418691E558FFE155ACEE`，套件 0.2.9.0。

結果：WIA automation 與桌面 WinRT（`FromIdAsync`、`ScanFilesToFolderAsync`）皆成功掃描；Windows 掃描 App 當時找到裝置但連線失敗，服務端呼叫序列與成功的 WinRT 完全相同；根因與修正見上方「Windows 掃描 App 實掃與 YRES 清單」。WinRT 回報只支援灰階、DIB 格式，彩色實際可掃（WIA automation 設 DATATYPE=3 成功），推測與 WIA_IPS_CUR_INTENT 有效旗標未含 COLOR 有關，待修。開發期限制：WIA 服務持有 WinUSB 句柄時，`wc3119 inquiry`、範例與硬體測試都會拒絕存取，須先停止 stisvc 或解除安裝套件。

### WIA 服務首次實掃（開發機）

2026-09-19：依 [安裝方案](../../driver/README.md#套件安裝更新解除安裝) 在開發機信任測試憑證並安裝套件，WIA 服務以 LocalService 載入 `workcentre_3119.dll`，鎖定 WinUSB 介面並完成灰階與彩色 75 dpi 全平台掃描；屬性驗證經服務拒絕 999 dpi。DLL SHA256 `2CBCB3B37E5CC495E1557F859443C78412E4A7C84EAC637CA778BF7BA3D19C1D`，套件 0.2.6.0。證據與時間見 [delivery-status](../../delivery-status.md#verified)。

服務行為與對應修正（皆有先失敗的 wiatrace 證據，再修正）：

| wiatrace 觀察 | 修正 |
| --- | --- |
| `CoCreateInstance … CLASS_E_NOAGGREGATION` | [com_server.rs](../../src/com_server.rs) 支援聚合：非委派 IUnknown 供 outer 持有，IStiUSD／IWiaMiniDrv 的 IUnknown 方法轉送 controlling unknown；outer 非空時只接受 IID_IUnknown |
| `IStiUSD::Initialize … 0x8007047E` | 服務傳 STI 版本 3；[sti.rs](../../src/com_server/sti.rs) 改接受版本 ≥ STI_VERSION_MIN_ALLOWED，不要求 Unicode 旗標（實測服務值仍被舊檢查拒絕），GetCapabilities 回 STI_VERSION_3 |
| `IStiUSD::LockDevice … 0x80004005` | `GetMyDevicePortName` 回 `AUTO`（INF `CreateFileName=AUTO`）；LockDevice 改以專案 GUID 列舉唯一 MI_00 |
| `drvReadItemProperties (16 properties) … 0x8000FFFF` 且服務記錄「Could not get the driver item flags for this generated item」 | 相容模式產生的應用程式項目不能 `wiasGetItemType`；[read_entry.rs](../../src/com_server/minidrv/properties/read_entry.rs) 對不含裝置狀態的讀取只確認連線即回 S_OK |
| 更新後仍載入舊行為 | COM 快取舊 DLL；[wc3119-setup.ps1](../../driver/wc3119-setup.ps1) 在 Install／Update 後重啟 stisvc |

另新增 [trace.rs](../../src/com_server/trace.rs)：`catch_hresult` 攔到 panic 時寫 `%SystemRoot%\debug\WIA\wc3119-driver.log`，本輪沒有 panic 記錄，E_UNEXPECTED 來自 `helper_failure` 對 wiasGetItemType 非 S_OK 的映射。`drvUnLockWiaDevice` 在 `drvUnInitializeWia` 之後被呼叫時回 E_UNEXPECTED，服務照常卸載，列為待改善。wiatrace 另警告 DLL 缺少版本資源（Driver version 0.0.0.0），不影響載入。

尚未驗收：Windows 掃描 App 預覽與取消、拔插／換孔／重開機後再掃、第二台乾淨電腦、解除安裝與移除信任、亮度／對比映射與文件品質、`WIA_DPS_DOCUMENT_HANDLING_STATUS` 目前應用程式讀到 0（服務未以該屬性觸發驅動讀取）。

### WIA 登錄方案設計

2026-09-19：新增 [WIA INF 設計稿](../../driver/wc3119-wia.inf)，Class 為 Image、函式驅動維持 WinUSB、以 sti_ci 類別安裝程式登錄 StillImage 與 USDClass／CLSID，事件表與 `drvGetCapabilities` 一致。完整前提、簽署門檻、備份與復原順序見 [安裝方案](../../driver/README.md#wia-登錄方案尚未執行待授權)。本輪只讀取系統狀態：`wc3119 doctor` 三介面問題碼 0、MI_00 服務 WINUSB；`stisvc` 為 Stopped；`bcdedit` 無 testsigning；本機只有 SDK signtool，沒有 WDK InfVerif／Inf2Cat。沒有修改綁定、登錄或服務。

2026-09-19 使用者決定不花錢：採測試憑證簽署 catalog、不啟用 testsigning（套件無自有核心驅動）、每台電腦需明確授權信任憑證。已新增 [package.ps1](../../driver/package.ps1)（打包＋Inf2Cat＋signtool，不改系統）與 [wc3119-setup.ps1](../../driver/wc3119-setup.ps1)（Status／Install／Update／Uninstall，預設預檢，`-Apply` 才改系統，含備份、父裝置／MI_01 比對、CLSID 清理、版本檢查與 3010 停止）。兩支腳本通過 PowerShell 語法解析；`Status` 與 `-SkipCatalog` 已實跑，其餘因本機尚無 WDK Inf2Cat 且未取得安裝授權而未實跑。完整流程見 [安裝方案](../../driver/README.md#套件安裝更新解除安裝)。

### 能力列舉、同步命令與 STI WIA 宣告

2026-09-19：176 個 lib 測試、全部整合測試與 2 個 doc-tests、fmt、Clippy（all targets，warnings 為錯誤）、全部 release targets 通過。另指定當次 release DLL 執行 ignored 動態載入測試通過，DLL SHA256 `A56CFCC5743FC12ACBC947421F02D0EB7C9338577C4967EB1F2C88D7F43D23F6`。沒有偽造 WIA context、沒有 USB 或系統登錄操作。

`drvGetCapabilities` 依 [Microsoft 契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wiamindr_lh/nf-wiamindr_lh-iwiaminidrv-drvgetcapabilities) 回傳程序生命週期的靜態 `WIA_DEV_CAP_DRV` 表（SDK 大小 40 bytes、flags 偏移 8），flags 1 為命令、2 為事件、3 為命令在前事件在後，其他值回 E_INVALIDARG；count 必填、list 可省略；context 允許 null，因為 [ProdScan](https://github.com/microsoft/Windows-driver-samples/blob/main/wia/ProdScan/MiniDrv.cpp) 指出服務可能在項目樹建立前呼叫。目前宣告 WIA_CMD_SYNCHRONIZE（icon `sti.dll,-2000`）及 WIA_EVENT_DEVICE_CONNECTED／DISCONNECTED（WIA_NOTIFICATION_EVENT，icon `sti.dll,-1001`）。驅動本身不發送任何事件，因此未宣告 SCAN_IMAGE、READY、COVER 等事件，也未接 STI 通知。[能力模組](../../src/com_server/minidrv/capabilities.rs)

`drvDeviceCommand` 只接受 WIA_CMD_SYNCHRONIZE 並回 S_OK：本驅動的平台項目樹在初始化時固定，沒有需要重建的內容，因此不像 ProdScan 刪除再重建項目樹或發 TREE_UPDATED。其他命令回 E_NOTIMPL，缺 context 回 E_INVALIDARG，item 輸出可省略且一律清空。`drvNotifyPnpEvent` 對已宣告的連線／斷線事件回 S_OK 且不影響取消狀態，未宣告事件維持 E_NOTIMPL。[命令契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wiamindr_lh/nf-wiamindr_lh-iwiaminidrv-drvdevicecommand)

`IStiUSD::GetCapabilities` 改為回報 STI_GENCAP_WIA（SDK sti.h 0x10）與 Unicode STI_VERSION，不宣告 STI_GENCAP_NOTIFICATIONS 或 POLLING。這是服務判斷 USD 是否附帶 IWiaMiniDrv 的依據；實際是否被載入須待 WIA 登錄後實測。[GetCapabilities 契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/stiusd/nf-stiusd-istiusd-getcapabilities)

TDD：`tests/com_server.rs` 先命名 command／capabilities slot 並斷言能力列舉成功，實跑取得 E_NOTIMPL 的 RED，再實作至 GREEN。單元測試核對結構佔位、命令先於事件、NUL 結尾名稱、flags 拒絕、null context／可省略 list、同步命令空操作與未知命令拒絕。`tests/sti.rs` 原斷言旗標為 0，其訊息明示前提是「IWiaMiniDrv 實作前」，前提已不成立故改為 0x10。待服務驗收：服務是否用同一 instance 派送連線事件、是否對本驅動發 SYNCHRONIZE，以及 STI_GENCAP_WIA 是否為載入 minidriver 的充分條件。

### 屬性讀取通知與裝置錯誤字串

2026-09-19：171 個 lib 測試、全部整合測試與 2 個 doc-tests、fmt、Clippy（all targets，warnings 為錯誤）、全部 release targets 通過。另指定當次 release DLL 執行 ignored 動態載入測試通過，DLL SHA256 `976033154F7B6CA1C72EC2664B1B0D296D90EF0B005FFC9C869E21B98D8CBA5C`。這些測試沒有偽造 WIA context、沒有操作 USB 或系統登錄。

`drvReadItemProperties` 依 [Microsoft 契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wiamindr_lh/nf-wiamindr_lh-iwiaminidrv-drvreaditemproperties) 只在讀取需要由裝置更新的屬性時存取硬體。[讀取入口](../../src/com_server/minidrv/properties/read_entry.rs) 界定 PROPSPEC 數量與名稱後，若要求的屬性不含 `WIA_DPS_DOCUMENT_HANDLING_STATUS`（ID 或本驅動登錄的名稱），只借用連線並以 `wiasGetItemType` 確認項目存在即回 S_OK，不碰 USB；若包含且項目為根，沿用初始化相同的服務鎖定 → 同一 STI session INQUIRY → 解鎖流程，成功才以單一 `wiasWriteMultiple` 寫入 `FLAT_READY`（SDK wiadef.h 0x02）。INQUIRY 失敗回傳原始 HRESULT、不改寫既有狀態；寫入失敗依既有政策隔離整個 COM 物件。平台項目沒有須由裝置更新的屬性，直接成功。[ProdScan 範例](https://github.com/microsoft/Windows-driver-samples/blob/main/wia/ProdScan/MiniDrv.cpp) 同樣只在根項目更新執行期狀態。INQUIRY 成功只代表裝置可通訊，協定沒有蓋板或紙張狀態欄位，因此不宣告 COVER_UP 等旗標。

`drvGetDeviceErrorStr` 依 [Microsoft 契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wiamindr_lh/nf-wiamindr_lh-iwiaminidrv-drvgetdeviceerrorstr)：本驅動的 `plDevErrVal` 一律是失敗 HRESULT，因此 [錯誤字串模組](../../src/com_server/minidrv/errors.rs) 把 0、FACILITY_WIA 1–16、常見 COM 錯誤對應成英文說明，`0x8007xxxx` 由 `FormatMessageW` 取得 Windows 本地化文字，無文字時回代碼；不認得的值回 E_INVALIDARG，保留 flags 非零亦拒絕。字串以 `CoTaskMemAlloc` 配置、由服務或應用程式釋放；`ppszDevErrStr` 可省略。尚未做的：WIA 錯誤說明只有英文，未依 Microsoft 建議放進資源檔做多語系。

TDD：`tests/com_server.rs` 先把 vtable 的 read／error_string slot 命名並斷言缺少 context 回 E_INVALIDARG，實跑取得 E_NOTIMPL 的 RED，再實作至 GREEN。單元測試涵蓋只有狀態屬性觸發裝置查詢、入口前置檢查、單值寫入僅一次 `values` 且非 S_OK 即失敗、錯誤字串配置／釋放往返與未知代碼拒絕。待服務驗收：服務實際傳入的 PROPSPEC 內容、每次應用程式讀取根屬性造成的 INQUIRY 頻率是否可接受、以及狀態值是否被 Windows 掃描消費。

### 屬性初始化與即時能力

2026-09-14：207 個 all-targets 測試、一般測試及 2 個 doc-tests、fmt、Clippy、全部 release targets 通過。當次 DLL 動態載入另以 ignored 模式通過，確認 slot 40 的初始化入口拒絕缺少服務 context 並正確填入錯誤，沒有以假 context 呼叫 SDK。SDK C11 編譯斷言與 Rust 測試核對 PROPSPEC、PROPVARIANT、WIA_PROPERTY_INFO 大小及 union／方法偏移。新實機鎖定／INQUIRY 測試另通過 0.02 秒，詳見 [硬體紀錄](../hardware.md#wia-屬性初始化的能力查詢)。私人 SDK 與測試輸出在 `artifacts/wia-properties-validation-20260914-a/`。

`drvInitItemProperties` 已接上根／平台初始屬性。服務鎖定、當次 INQUIRY、解鎖及屬性發佈共用一次 Connection 借用，發佈時仍阻擋重入。解鎖失敗先在 Mutex 外釋放連線，再保持 Failed，不重試也不繼續發佈；查詢及解鎖都失敗時保留查詢錯誤。未改動掃描命令、取消流程或 USB 復原行為。

初始模式優先灰階、解析度採實機與核心共同支援的最低值，範圍取實機平台上限。色深、通道、Y 解析度及可移動位置對應目前設定，明暗維持中性範圍，BMP 大小含標頭、灰階色盤與列填補。根項目宣告讀取權限與平台能力，狀態暫為 0，未把 INQUIRY 當成馬達已就緒。光學 600×2400 dpi 是 Xerox 本型號的資訊欄位，可選掃描設定仍限制為實機回報與已實作的對稱解析度。[Xerox 型號規格](https://www.office.xerox.com/latest/W31BR-01.PDF)、[Microsoft 屬性要求](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/required-flatbed-item-properties)

依 Windows 規定，原生發佈順序為名稱、數值、有效值描述。只接受 S_OK，任何失敗立即停止，BSTR／GUID 與列表的暫存持續存活至同步呼叫返回。保留服務建立的實際項目名稱，不覆寫服務從 INF 取得的 ICM 色彩設定。合成測試操作同一發佈流程，但不偽造服務 context。[Microsoft 發佈順序](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/adding-wia-properties-to-a-wia-item)、[服務維護的共通屬性](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/wia/-wia-wiaitempropcommonitem)

TDD：COM 入口先取得 E_NOTIMPL 而非 E_INVALIDARG 的失敗；初始尺寸、相依列表、讀取權限、光學資訊、中性 Range、根項目分類與 ICM 所有權的測試先失敗再修正。解鎖失敗的四個回歸情境亦先失敗再加入隔離。這些結果不代表服務能初始化／切換設定，`drvValidateItemProperties` 後續實作見下一節，裝置狀態、WIA 服務實測與正式安裝仍待完成。選取高度與實際影像多一列的既有差異持續列入幾何驗收。

Diff Inspector：Scope CLEAN。根代理查核相對 `85f628f` 的核心、原生消費端、測試與文件，Luna 分別完成屬性 catalog／SDK、原生發佈／所有權及解鎖生命週期的對抗審查。初稿中的解鎖失敗仍回 Connected 已修正，沒有確認的未處理 P1／P2。成功發佈測試逐筆讀取實際 native payload 與 backing pointers，另驗證各階段失敗即停止。待服務驗收確認：最小掃描尺寸屬性的實際需求、`wiasReadPropStr` 失敗時非空 BSTR 的所有權，以及服務管理的名稱、狀態與屬性消費；未把缺少 SDK 明文當成已確認缺陷。

最終 SHA256：DLL `D79F95A8DC208A166890E4DA28E33992F7B0479BE6EF294E633DC23CE6F7AD66`；catalog `40EDC8E2C62ECC52AB5F31C12F77A06334A94F3E79D39284BE5647CC3755CC7E`；native `ACEF5B1EAAF0B7DEA3741CC031A8EE11E2ECDE8AE4A51EFE3F76D9DD7322D325`；locking `8EFA9192C437FFDFCC612D8640C0DE57D22F4532CE5C165C711672EA409C4A9E`。複查後的變更僅有格式及合成測試的資料順序／註解整理。

### 屬性相依驗證與發佈失敗隔離

2026-09-14 最終驗證：236 個 all-targets 測試、一般測試與 2 個 doc-tests、fmt、Clippy（all targets，warnings 為錯誤）、全部 release targets 通過。另行指定當次 release DLL 執行 ignored 動態載入測試，slot 48 的驗證入口及缺少 context／錯誤輸出的 ABI 行為通過。日誌在 `artifacts/wia-property-validation-20260914-b/*-final.txt`。同版鎖定／能力查詢另以實機通過 0.02 秒，仍不代表服務屬性已驗收。

最終 SHA256：DLL `4684F5308B758FA895BC5EC76658A317F64AF3A2D3118DE89FEB5AD6126F87E2`；相依解析 `AF1725E38F9F0FFA4583F8857FEBB207B844840CCF27DE772192900C2AB3D3EA`；驗證入口 `3D677CC47F7F95CD07ED063C1F7DF88FD8854774B59F4A0F3894D0374E1C165F`；原生發佈 `2972DFAFF3A1158AFEF2BC43C705D05CE427F062082A254CBDBF9AB08EA76F42`。

Diff Inspector：Scope CLEAN。根代理及 Luna 對相對 `d8cb887` 的屬性解析、原生發佈、COM 借用／隔離及測試完成交叉審查。初始化發佈失敗可重新使用物件的缺口已修正，沒有新增確認的未處理 P1／P2。服務 old/current 行為、多用戶端排程及整個物件隔離後的重新載入仍須實測；下列測試只證明核心與原生邊界契約。

本輪補上 `drvValidateItemProperties` 的數值相依流程：[驗證入口](../../src/com_server/minidrv/properties/validation_entry.rs) 先界定 `PROPSPEC` 數量與 UTF-16 名稱，解析已註冊的 ID／名稱並拒絕未宣告或唯讀屬性；在 Windows 提供的 context 內讀取項目類型、項目名稱、設定 LONG／格式 GUID，對明確寫入欄位保留 old／current 快照，再由 `validation::resolve` 與 `PropertyCatalog::with_settings` 產生 DPI、位置、範圍、模式、色深及 BMP 大小的相依結果。[原生 delta 發佈](../../src/com_server/minidrv/properties/native.rs) 先完成所有暫存配置，只寫異動的相依值／有效值描述，最後呼叫 `wiasValidateItemProperties`；它不改名稱或無關唯讀預設，也不做未查證的 rollback，任何發佈／最終驗證錯誤都隔離整個 COM 物件，須由服務釋放並建立新物件。[Microsoft 驗證契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/writing-wia-item-properties-by-an-application)、[drvValidateItemProperties](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wiamindr_lh/nf-wiamindr_lh-iwiaminidrv-drvvalidateitemproperties)

TDD 先以缺少 native delta 入口／`PropSpec` accessor 的 RED 暴露接線缺口，再以灰階轉彩色、解析度／幾何相依更新、身份值不一致不發出 native call，以及數值／屬性／最終驗證失敗即停止的測試完成 GREEN。審查後補上初始化發佈失敗的隔離，並以同一 Connection 借用涵蓋能力查詢、服務鎖定／解鎖與屬性發佈。[生命週期 guard](../../src/com_server/minidrv/locking.rs)

這些測試沒有建立或偽造 WIA service context，也沒有呼叫真實服務的 `wiasReadProp*`／`wiasWriteMultiple`／`wiasValidateItemProperties`；DLL 載入與硬體項目／USB／IStream 測試不能替代 WIA service 驗收。取得具體登錄授權後，仍須以實際 WIA 服務／Windows 掃描流程確認 property context 的 old／current 行為、相依屬性的服務消費、服務帳號存取與整體影像傳輸。正式安裝、登錄及 Windows 掃描仍未完成。

### 取消事件與格式列舉

離線驗證：178 個 all-targets 測試、一般測試及 2 個 doc-tests、fmt、Clippy、全部 release targets 通過。補正文內說明後再建置，當次 release DLL 動態載入明確以 ignored 模式執行通過，包含真 BSTR 的取消方法呼叫，DLL SHA256：`27050B253D8C9DBA302DB5E947EEEC13B98488D5DF9544099ACE0BD639AFBE92`。另以 SDK 10.0.26100.0 C11 編譯斷言核對格式結構及方法偏移。capture_scan 範例測試／建置及 STI 一般測試通過；硬體結果獨立列於下文，不能用離線通過抵銷提早取消後重掃失敗。

`drvNotifyPnpEvent` 已處理 WIA_EVENT_CANCEL_IO，以初始化時的裝置識別限定目前工作。註冊、完成及取消有一致排序，閒置通知為成功的空操作，錯誤裝置不影響正在執行的掃描；未知事件回 E_NOTIMPL。跨執行緒測試只共享 Rust 取消狀態，原生 COM 留在原執行緒。服務實際是否並行派送此方法仍未驗證。[Microsoft 取消契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wiamindr_lh/nf-wiamindr_lh-iwiaminidrv-drvnotifypnpevent)

掃描 checkpoint 的明確取消以型別保留經過診斷包裝的起因。只有 Ready 且沒有輸出失敗才轉成取消；Interrupted、USB 或 callback 錯誤不因同時收到取消而被遮蓋。合成測試涵蓋起因保留、成功／失敗清理、舊旗標移除、錯誤裝置、重入與完成競態。新取消狀態、格式與取消起因功能均先取得缺少實作的 RED，再實作通過。

`drvGetWiaFormatInfo` 先透過真實 WIA context 的 `wiasGetItemType` 確認可傳輸影像，然後回傳程序生命週期的靜態 BMP／TYMED_FILE。根、資料夾、非影像或不可傳輸項目拒絕，count／device-error 必填、format-list 可省略。SDK 的 WIA_FORMAT_INFO 大小 20 bytes、tymed 偏移 16；取消方法位於 IWiaMiniDrv 的 slot 18／x64 偏移 144。[Microsoft 格式契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wiamindr_lh/nf-wiamindr_lh-iwiaminidrv-drvgetwiaformatinfo)

**實機驗收不完整**：灰階完成後約 500 ms 取消下一張 RGB，兩次在 RESERVE Busy 時被保守隔離。改由閒置狀態開始 RGB，約 720 ms 返回取消，沒有影像或 100% 訊息，但其後 RGB 重掃約 120 秒失敗。第一次重掃只留下 HRESULT 與零影像。補上原始診斷後重現：取消約 600 ms 返回 S_FALSE，重掃在 RESERVE 收到 800 次 Busy（0x08），120.049 秒到期。測試順序調整只區分取消情境，未修改核心隔離策略，也沒有把失敗的驗收放寬為通過。證據見 [硬體紀錄](../hardware.md#wia-取消事件與提早取消後重掃)，復原工作由 [03](03-recover-scan.md) 接續。

減法審查：取消沿用現有有限排空及清理流程，格式使用單一靜態 BMP 表。沒有增加背景 USB 執行緒或重試機制；屬性初始化以實際能力為前置，不加入固定機型能力假資料。

Diff Inspector：Scope CLEAN，根代理檢查相對 `8d2913d` 的完整核心／測試差異與文件，Luna 獨立審查取消併發、錯誤分類及協定限制。沒有確認的新增程式缺陷；已知提早取消實機失敗仍由 03 處理。完成與取消的先後以 `job.finish` 共用 Mutex 為準，Completed 結果在完成點前收到取消及錯誤優先的測試通過。待查證：服務是否使用同一 minidriver instance／裝置 ID 派送取消，以及原生屬性 context／格式列表的服務使用方式。既有 `exchange`／二次清理錯誤字串化仍不能保留所有底層 HRESULT，另列 03 後續，未把本輪的 callback／stream HRESULT 保留宣稱成全鏈完成。

### 原生鎖定與掃描 dispatch

最終離線驗證：168 個 all-targets 測試、一般測試及 2 個 doc-tests、格式、Clippy、全部 release targets 通過；補正文內說明後重新建置，當次 release DLL 的動態載入另行通過。DLL SHA256：`69562D5C698873D322707BCC6B83CCE7B823C3875667B7C689E9D7DD442D1FFF`。acquire／properties／locking 來源 SHA256 依序為 `8511C9A1F5EC381D9ADFA69858F5746532257CEEF1C751C86E06A9AB86F48434`、`CDAB5887E403ACCC1C429CC42ED53716357DD8EEC5868B61B88241CE6881A53A`、`D3497C9EF5F5B6FD6BF6FE5C995E790AA2C6933DA8D73FF10363656B5F180192`。

2026-09-14：WIA 原生 lock/unlock 經保留的 IStiDevice 轉送，acquire 接上真實屬性讀取及既有串流核心。屬性讀取與傳輸入口平行實作，合併後驗證。核心入口及借用順序先取得缺少實作的 RED；非預期正 HRESULT 的鎖定回歸先實際失敗（1 被當成功）再修為 E_UNEXPECTED。其他補充邊界測試不宣稱曾取得 RED。

| 情境 | 行為與證據 |
| --- | --- |
| 原生 ABI | SDK C11 及 Rust 斷言確認 IStiDevice lock/unlock、IWiaMiniDrv acquire/lock/unlock 偏移，transfer context 144 bytes、callback offset 104 |
| 屬性快照 | 先檢查項目類型，再讀 11 LONG、格式 GUID 與兩個 BSTR；拒絕缺值、不支援設定及非 S_OK，BSTR 16K 上限，沿用既有設定驗證 |
| 生命週期重入 | 保留整個 Connection 至讀取及傳輸返回，回呼期間解除初始化／再次鎖定／巢狀 acquire 回 Busy；正常錯誤歸還，panic 保持隔離 |
| 結果分類 | 僅明確取消回 S_FALSE，Completed／開始前 Skipped 回 S_OK，IStream Interrupted 保持錯誤；原生負 HRESULT 保留，異常正值拒絕 |
| 實機 dispatch | 合成屬性快照與 callback、真正原生項目／USB／IStream，灰階、彩色取消及彩色重掃 42.21 秒通過，詳見 [硬體紀錄](../hardware.md#wia-鎖定與-dispatch-實機驗證) |
| 真正服務 context | 尚未執行，測試不以假指標呼叫 wiasReadProp 系列，不能當作屬性儲存或 Windows 掃描驗收 |

由硬體能力限制的屬性初始值／有效範圍後續已建立，見本文件「屬性初始化與即時能力」；相依驗證與真正服務 context 的發佈／消費仍待驗收。格式列舉與 WIA_EVENT_CANCEL_IO 的後續成果見「取消事件與格式列舉」。測試編碼與實機掃描沒有改動 COM／WIA 登錄或系統權限。

Diff Inspector：Scope CLEAN。根代理追蹤 native acquire → 屬性快照 → STI session → 原生 callback／BMP 消費端，Luna 獨立審查 ABI、持有參考、重入與錯誤分類，沒有確認的未處理 P1／P2。平台未宣告 Folder／多項傳輸，拒絕 ACQUIRE_CHILDREN 與目前範圍一致。null STI helper 可建立項目樹但無法鎖定硬體，正式服務模式與 capability 宣告仍須驗證。

複核提出 transfer context 的四個大小欄位可能需額外填寫。根代理查核 [Microsoft ProdScan 的 ScanJobs.cpp](https://github.com/microsoft/Windows-driver-samples/blob/main/wia/ProdScan/ScanJobs.cpp)：WIA2 acquire／Download 使用 callback 與 format，沒有寫入上述四欄或呼叫 wiasGetImageInformation；[結構文件](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wiamindr_lh/ns-wiamindr_lh-_minidrv_transfer_context) 的前提是舊式 memory-callback／file transfer。因此尚不能確認這是本串流路徑的缺陷，保留為真實服務驗收項目，不重複計算 BMP 尺寸或加入未證實必要的舊式 helper。既有最終 Release／unlink 故障與服務排程的追蹤事項繼續保留。

屬性初始化依 [Microsoft 順序](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/adding-wia-properties-to-a-wia-item) 建立名稱、初始值、再設定有效範圍。必備項目分別核對 [root](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/required-root-item-properties-for-wia-scanners) 與 [flatbed](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/required-flatbed-item-properties)。初始化／讀取／驗證不能假定服務已鎖定 USB。初始能力後續已透過保留的 IStiDevice 鎖定，在同一 STI session 查詢並解鎖，合成測試及獨立 INQUIRY 硬體測試通過；真正 WIA 服務仍待驗證，沒有另開第二個獨占 handle。

### 原生項目樹與介面身分

2026-09-14：156 個 all-targets 測試、一般測試及 2 個 doc-tests、格式、Clippy、全部 release targets 通過，當次 release DLL 的動態載入另行通過。新 QI 測試先取得 E_NOINTERFACE 失敗再實作；項目樹測試先取得缺少實作的失敗，再使用真正 Windows 物件驗證。SDK 10.0.26100.0 C11 靜態斷言及 Rust 斷言核對 IWiaMiniDrv 的 20-slot、IWiaDrvItem 的 16-slot 大小／位置及根／平台旗標。

| 情境 | 行為與證據 |
| --- | --- |
| 三種介面互查及最終釋放 | QI 回到同一 IUnknown，次要介面持有期間 DLL 不可卸載；公開 ABI 與實際 DLL 測試通過 |
| 缺少服務 context／錯誤輸出 | 原生初始化拒絕，清空根／inner 輸出，不讀 BSTR 或碰觸 USB |
| 空名稱、超長完整名稱 | 建立前拒絕，下一次合法初始化仍可成功；內部初始化入口測試通過 |
| 原生根／平台內容 | 真正 wiasCreateDrvItem，逐一核對名稱、完整名稱與旗標，重複建立／解除三次通過 |
| 兩個用戶端共用 | 回傳同一根，第一個離開保留項目樹，最後離開清理；三輪通過，不同識別拒絕且不增加計數 |
| STI helper AddRef／Release 重入 | 同步重入回 Busy，沒有持有生命週期 Mutex；三輪各自保留／釋放一次 |
| 真實服務初始化及項目儲存 | 未驗證；測試使用合成名稱及內部入口，不以假 context 呼叫 WIA 服務 |
| 建立／unlink 系統錯誤及 panic | 實作保留 HRESULT、清理失敗隔離；原生資源耗盡與 unlink 故障未注入驗證 |

測試初稿誤將 `GetFirstChildItem` 的借用結果當成 owned 參考，造成測試程序 heap corruption／清理失敗。獨立 native probe 確認本機 `wiaservc.dll` 10.0.26100.8875 的 getter 不增加參考，測試改為先 AddRef 後 RAII，重複建立／清理通過。AddItemToFolder 則確實增加子項目參考；原生建立不增加 minidriver 參考。這是本機 Windows runtime 證據，不是跨版本或 WIA 服務驗收。私人 probe 位於 `artifacts/wia-tree-probe-20260914-b/`，SDK 編譯驗證位於 `artifacts/minidrv-validation-20260914-a/`，皆不提交。

此項目樹版本沒有 USB 或系統設定操作；沿用前版掃描核心，沒有把歷史實掃當成本版 WIA 服務測試。DLL 新增 `wiaservc.dll` 匯入，仍依賴 VCRUNTIME140.dll，runtime exports 恰為兩個 COM 入口。後續已接上 acquire、鎖定、取消及屬性初始化，詳見前述驗收段落；相依驗證亦已接上，仍須以實際服務 context 驗收。準備具體可復原的登錄方案後再取得系統變更授權。

Diff Inspector：Scope CLEAN。根代理查核全部差異及 STI／共用掃描消費端，Luna 完成身分、ABI、原生項目參考及重入的獨立對抗審查，沒有確認的未處理 P1／P2。仍需查證：服務是否保證最終 Release 前先 Uninitialize；尚未解除的物件直接析構時，helper Release 重入沒有測試；destructor 忽略 unlink 的 HRESULT，沒有原生故障注入證據確認其影響。這些事項列為服務生命週期整合的驗收條件，不能由正常程序內清理成功推論已完成。

最終來源 SHA256：`minidrv.rs` 為 `16332D8F05AB171139D63286BB20867E5FF5DA263C3575057A5F1BD215B260BE`，`tree.rs` 為 `4BE000A2C7A4855B5DEF88B9F1B563B37B974AEB81215878779458F4E016F11A`；最後補正文內說明後重新建置並另行載入的 release DLL 為 `6E29682A421654DB6FB4C890A23039CFC20F4AF8F2203B4405BCBF5AFD7C771C`。

### 先前版本證據

2026-09-14 原生傳輸回呼版本：153 個 all-targets 測試、一般測試及 2 個 doc-tests、格式、Clippy、全部 release targets、另行執行的當次 DLL 載入測試均通過。新回呼契約 15 個、傳輸流程 7 個，另增加 BMP 進度及 STI 入口前置檢查。原生轉接、進度及公開入口先取得缺少實作的失敗再實作，其他邊界補測未宣稱取得 RED。根代理及 Luna 對抗審查沒有確認的未處理 P1／P2。Windows SDK 10.0.26100.0 C11 靜態斷言核對 x64 callback 的 5-slot vtable、GetNextStream／SendMessage 偏移、24-byte WiaTransferParams 欄位偏移與 STATUS／SKIP 常數。

三個實機測試各自明確指定 `--ignored --exact`、序列執行，皆通過。新回呼路徑使用測試 COM callback 及真實 Windows HGLOBAL IStream／USB，同一鎖定物件完成灰階、彩色取消與彩色重掃；舊掃描路徑亦通過回歸。Pillow 全樣本解碼與新回呼影像實際檢視通過，仍為空平台，詳見 [硬體紀錄](../hardware.md#原生-wia-callback-實機傳輸)。沒有操作登錄、WIA 服務或 Windows 掃描，等待期間的服務取消尚未驗證。

最終 callback 原始碼 SHA256：`B9E9B280F84ABDEBD7FCDA03202E5F41BA5DCB04AB21DC9C37CA257A4CF7A4ED`；傳輸原始碼：`31B3C1AA3D7CA10B8D29C131E387FB2BE610FF32F33202203F80928C0AA15224`；當次另行載入的 release DLL：`2A3DAD4E77E36ABA133DDEA894F9AFFDBF42C6AA84BEB3C2CEC118EFA321FD4E`。實機後僅追加流程單元測試及格式整理，硬體測試執行檔識別保留在硬體紀錄，不將重新建置當成重跑實機。

2026-09-14：已補完上次連線借用版本的核心獨立複核及實機驗證。Luna 發現 RESERVE 前取消會錯誤隔離，根代理以回歸測試重現再修正，修正版複核沒有新增確認問題。129 個 all-targets、2 個 doc-tests、格式、Clippy、全部 release targets 與另行載入當次 DLL 的測試通過。兩個 STI 硬體測試逐一執行通過，含同一物件的 Gray75、RGB75 取消、RGB75 重掃、輸出 callback 重入及每次清理後診斷。BMP 經 Pillow 全部有效樣本解碼比對、GDI+ 開啟及實際檢視，為空平台。沒有 WIA 服務或 Windows 掃描驗收，實機與建置識別見 [硬體紀錄](../hardware.md#共用連線實掃與提早取消修正)。

2026-09-13 連線借用版本：128 個 all-targets、2 個 doc-tests、格式、Clippy、全部 release targets 與另行執行的 DLL 測試通過。`com_server::scan_locked_bmp` 已接到既有掃描及 BMP 實作，借用的是 IStiUSD 鎖定的 session；同步影像回呼不持有狀態 Mutex。核心以型別回報可歸還／需重連，未知消耗量及清理失敗不再由此物件重開。借用器的並行、重入、panic 與先關閉再開啟已有合成測試。使用者要求收尾，本版的兩個 ignored STI 硬體測試尚未執行。下次先執行有新輸出目錄的灰階／彩色取消／重掃測試並檢查 BMP，之後接上原生 IWiaMiniDrv。此 Rust 入口不能視為 Windows 掃描已可用。

2026-09-13，IStiUSD 版本完成 114 個 all-targets 測試、2 個 doc-tests、格式、Clippy 及全部 release targets。6 個離線 STI 測試驗證初始化、helper 參考、失敗重試、未初始化鎖定不死鎖、結構大小與錯誤資訊。明確啟用的實機測試另外通過指定裝置、排他鎖定、INQUIRY 及 Release 後重開；未啟動掃描或登錄 WIA。release DLL 的動態 QI／生命週期測試另行通過。SDK C11 靜態斷言核對 STI 結構與 19-slot ABI，私人證據在 `artifacts/sti-sdk-20260913-j/`。DLL 仍依賴 VCRUNTIME140.dll，新增 SETUPAPI／WINUSB 系統匯入，exports 仍只有兩個 COM 入口。雜湊與實機驗證界線見 [硬體紀錄](../hardware.md#istiusd-實機鎖定與能力診斷)。

2026-09-13，COM loader 版本通過 105 個 all-targets 測試及 2 個 doc-tests、格式、Clippy、全部 release targets 建置。另指定當次 release DLL 執行預設 ignored 的動態測試，1 個通過，實際完成 LoadLibraryExW、GetProcAddress、factory／物件參考釋放及 FreeLibrary。dumpbin 確認只有 DllGetClassObject／DllCanUnloadNow 兩個 runtime exports，建置沒有 LNK4104。DLL SHA256 為 `4DD6A9308662E92A35C1D55B120B75926184E0B30FE7AAE3D984A4666CC806F3`。原始碼 SHA256 為 `AF0F7836523072796585F727D851B04DDFA393E7004619162713561817C41C5A`。這輪沒有 USB、COM 登錄或 WIA 服務啟動。DLL 目前依賴 VCRUNTIME140.dll，跨電腦 runtime 與 COM 自動卸載排程仍待驗證，詳見 [DLL 契約](../../ENG.md#com-dll-載入與驗證)。

2026-09-13，WIA 數值設定先取得缺少模組的失敗，再通過 4 個公開契約測試與 1 個核心整合測試。涵蓋六種解析度的位置步進、模式／色深、格式、中性值、負值／溢位、預先取消不觸碰輸出，以及當次能力拒絕時不送 RESERVE。全部 98 個 all-targets 測試、一般測試含 2 個 doc-tests、格式、Clippy、核心及全部範例 release 建置通過。私人呼叫端經 `scan_bmp` 將 Gray75／RGB75 真實影像寫入 Windows 原生記憶體串流並成功讀回 BMP，獨立解碼及 GDI+ 開啟通過，詳見 [實機紀錄](../hardware.md#wia-數值設定與原生串流實掃)。這次未登錄 WIA，尚未驗證 Windows 掃描。

2026-09-13，原生 COM 輸出轉接先取得缺少公開模組的編譯失敗，再完成 5 個契約測試。合成邊界涵蓋短寫、零進度、超量、失敗 HRESULT 帶部分寫入量、Interrupted 不重試、Seek 及單次 Release。Windows 真實 `CreateStreamOnHGlobal` 物件完成 BMP 編碼、Seek 及 Read 回讀，確認標頭、行序與 RGB 樣本。2 個 doc-tests 證明物件不可跨執行緒移動或共用。最終 93 個 all-targets 測試、一般測試含上述 doc-tests、格式、Clippy、核心及全部範例 release 建置通過。沒有 USB、WIA 登錄或 Windows 掃描實測；驅動 DLL 與 WIA callback 尚未實作。

2026-09-13，BMP 元件 16 個測試與掃描整合 2 個測試通過，涵蓋灰階色盤、RGB 排列、行序／填補、尺寸及資料量、有限資源、部分寫入後失敗、Interrupted 不重試、取消與 RELEASE 失敗不完成影像。全部 88 個 all-targets 測試、一般測試含 doc-tests、Clippy、格式與 release 建置通過。Gray75、RGB300 與取消後 Gray75 的 USB／像素／BMP／PNM 獨立核對相同，Windows GDI+ 開啟前兩張成功；仍未驗證 Windows 掃描、原生 IStream 轉接或文件品質。詳見 [BMP 實機紀錄](../hardware.md#bmp-串流實機驗證)。

WIA 呼叫契約測試、第二台乾淨 Windows 11 x64 電腦的模型套件安裝、Windows 掃描實機驗證、換孔／拔插／重開機後掃描及復原測試。系統變更與測試副作用記錄於實機驗收資料；不得以開發機單一 devnode 的配對結果替代跨電腦驗收。

2026-09-13：開發機配對工具 7 個測試通過，涵蓋參數拒絕、完整目標 ID、候選條件、UTF-16 邊界、預檢失敗及 x64 原生結構。授權配對後 `doctor` 與 `inquiry` 均成功，真實能力回覆見 [硬體紀錄](../hardware.md)。這只驗證單一開發機／裝置實例的 WinUSB 與 INQUIRY；正式模型套件、catalog／簽署、WIA、第二台乾淨電腦、換孔／拔插／重開機及掃描影像仍未驗證。
