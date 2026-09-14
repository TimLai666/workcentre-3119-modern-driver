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

原生 [IStream 輸出轉接](../../src/com_stream.rs) 及 [WIA 數值設定入口](../../src/wia.rs) 已連到實際掃描。Cargo 產生的 COM DLL 提供 class factory、IUnknown、IStiUSD 與 IWiaMiniDrv 共用生命週期，已通過動態載入測試。[IStiUSD](../../src/com_server/sti.rs) 已支援初始化、指定裝置鎖定及 INQUIRY 診斷。IWiaMiniDrv 項目樹、服務 IStiDevice 鎖定、服務屬性讀取與 drvAcquireItemData 已接上同一 USB session 的回呼傳輸。屬性初始化已接上當次能力查詢。相依更新、狀態及服務整合未完成，尚不能提供 Windows 掃描。WIA2 串流只保證 `Write`、`Seek`、`SetSize`，不得依賴呼叫端提供完整檔案功能。BMP 的 `finish` 成功後位置為 byte 2，WIA 服務端的定位及影像消費行為仍待驗證。[Microsoft WIA 介面](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-minidriver-interfaces)、[COM 識別契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/providing-a-com-interface)、[IStream 契約](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/istream-data-transfer-driver-changes)

目前 `FlatbedSettings` 驗證數值快照，支援六種對稱解析度、8-bit 灰階／24-bit 彩色、中性亮度／對比及無壓縮 BMP。位置按協定精確步進，範圍另由當次 INQUIRY 限制，詳見 [設定契約](../../ENG.md#wia-設定與掃描入口)。WIA 屬性初始化已建立初始有效值範圍、色深、通道及逐像素排列，相依更新仍未實作，不能宣稱已能切換 WIA 掃描設定。[Microsoft DATATYPE](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ipa-datatype) 實機指定 600×800 像素後回傳 600×801，必須釐清裝置幾何及 WIA 選取範圍的處理，不能把 BMP 成功當成範圍驗收。

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

### 屬性初始化與即時能力

2026-09-14：207 個 all-targets 測試、一般測試及 2 個 doc-tests、fmt、Clippy、全部 release targets 通過。當次 DLL 動態載入另以 ignored 模式通過，確認 slot 40 的初始化入口拒絕缺少服務 context 並正確填入錯誤，沒有以假 context 呼叫 SDK。SDK C11 編譯斷言與 Rust 測試核對 PROPSPEC、PROPVARIANT、WIA_PROPERTY_INFO 大小及 union／方法偏移。新實機鎖定／INQUIRY 測試另通過 0.02 秒，詳見 [硬體紀錄](../hardware.md#wia-屬性初始化的能力查詢)。私人 SDK 與測試輸出在 `artifacts/wia-properties-validation-20260914-a/`。

`drvInitItemProperties` 已接上根／平台初始屬性。服務鎖定、當次 INQUIRY、解鎖及屬性發佈共用一次 Connection 借用，發佈時仍阻擋重入。解鎖失敗先在 Mutex 外釋放連線，再保持 Failed，不重試也不繼續發佈；查詢及解鎖都失敗時保留查詢錯誤。未改動掃描命令、取消流程或 USB 復原行為。

初始模式優先灰階、解析度採實機與核心共同支援的最低值，範圍取實機平台上限。色深、通道、Y 解析度及可移動位置對應目前設定，明暗維持中性範圍，BMP 大小含標頭、灰階色盤與列填補。根項目宣告讀取權限與平台能力，狀態暫為 0，未把 INQUIRY 當成馬達已就緒。光學 600×2400 dpi 是 Xerox 本型號的資訊欄位，可選掃描設定仍限制為實機回報與已實作的對稱解析度。[Xerox 型號規格](https://www.office.xerox.com/latest/W31BR-01.PDF)、[Microsoft 屬性要求](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/required-flatbed-item-properties)

依 Windows 規定，原生發佈順序為名稱、數值、有效值描述。只接受 S_OK，任何失敗立即停止，BSTR／GUID 與列表的暫存持續存活至同步呼叫返回。保留服務建立的實際項目名稱，不覆寫服務從 INF 取得的 ICM 色彩設定。合成測試操作同一發佈流程，但不偽造服務 context。[Microsoft 發佈順序](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/adding-wia-properties-to-a-wia-item)、[服務維護的共通屬性](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/wia/-wia-wiaitempropcommonitem)

TDD：COM 入口先取得 E_NOTIMPL 而非 E_INVALIDARG 的失敗；初始尺寸、相依列表、讀取權限、光學資訊、中性 Range、根項目分類與 ICM 所有權的測試先失敗再修正。解鎖失敗的四個回歸情境亦先失敗再加入隔離。這些結果不代表服務能初始化／切換設定，`drvValidateItemProperties`、裝置狀態、WIA 服務實測與正式安裝仍待完成。選取高度與實際影像多一列的既有差異持續列入幾何驗收。

Diff Inspector：Scope CLEAN。根代理查核相對 `85f628f` 的核心、原生消費端、測試與文件，Luna 分別完成屬性 catalog／SDK、原生發佈／所有權及解鎖生命週期的對抗審查。初稿中的解鎖失敗仍回 Connected 已修正，沒有確認的未處理 P1／P2。成功發佈測試逐筆讀取實際 native payload 與 backing pointers，另驗證各階段失敗即停止。待服務驗收確認：最小掃描尺寸屬性的實際需求、`wiasReadPropStr` 失敗時非空 BSTR 的所有權，以及服務管理的名稱、狀態與屬性消費；未把缺少 SDK 明文當成已確認缺陷。

最終 SHA256：DLL `D79F95A8DC208A166890E4DA28E33992F7B0479BE6EF294E633DC23CE6F7AD66`；catalog `40EDC8E2C62ECC52AB5F31C12F77A06334A94F3E79D39284BE5647CC3755CC7E`；native `ACEF5B1EAAF0B7DEA3741CC031A8EE11E2ECDE8AE4A51EFE3F76D9DD7322D325`；locking `8EFA9192C437FFDFCC612D8640C0DE57D22F4532CE5C165C711672EA409C4A9E`。複查後的變更僅有格式及合成測試的資料順序／註解整理。

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

此項目樹版本沒有 USB 或系統設定操作；沿用前版掃描核心，沒有把歷史實掃當成本版 WIA 服務測試。DLL 新增 `wiaservc.dll` 匯入，仍依賴 VCRUNTIME140.dll，runtime exports 恰為兩個 COM 入口。後續已接上 acquire、鎖定、取消及屬性初始化，詳見前述驗收段落；仍須補上相依驗證，並以實際服務 context 驗收。準備具體可復原的登錄方案後再取得系統變更授權。

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
