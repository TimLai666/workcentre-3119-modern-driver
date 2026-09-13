# WorkCentre 3119 工程設計

## 目標與現況

目標是在 Windows 11 x64 用 Rust 完成可以長期使用的純驅動，掃描優先，列印接續。不開發 GUI 或掃描 App，操作由 Windows 掃描等既有軟體提供。現有實作包含裝置診斷、能力查詢及分塊掃描；2026-09-13 已取得本機灰階／彩色空平台影像。這只代表單一開發機／裝置實例的部分掃描行為已驗證，文件品質、異常復原、跨電腦安裝、USB 換孔／拔插／重開機及 WIA 尚未驗收。功能的驗收條件與失敗情境由 [工作項目](docs/tickets/) 管理，進度由 [delivery-status.md](delivery-status.md) 管理。

## 使用流程與架構

```text
使用者執行診斷 → Rust 裝置辨識 → Windows Configuration Manager → 狀態與修復提示
既有掃描軟體 → Windows WIA → Rust 掃描工作 → USB 傳輸 → MI_00
      ↑                           ↓
      └──── 影像串流／狀態／錯誤 ────┘
使用者列印 → Windows 列印佇列 → 待查證的列印協定 → MI_01
```

裝置診斷與 Rust 掃描／USB 串流已實作，WIA 與列印仍是目標架構。USB 回覆與呼叫端參數均需檢查長度及數量。驅動負責回傳影像資料、能力、進度及錯誤，呼叫端軟體負責預覽畫面、編輯及檔案儲存。診斷預設不記錄序號及影像內容。

### 裝置識別

使用 Windows Configuration Manager 原生 API，不依賴 PowerShell 子行程。只查詢目前存在的裝置，精確識別父裝置、MI_00 與 MI_01。拔線造成列舉失敗時回報檢查失敗，不能顯示為「沒有裝置」。有多台時不得自動選第一台。

目前 `doctor` 只確認 PnP 驅動狀態。`DriverStarted` 表示 WinUSB 服務已啟動，不能當成已開啟 USB、端點可用或已支援 WIA。

`doctor` 透過 Configuration Manager 列舉目前裝置；`inquiry` 則由裝置介面 GUID 重新取得當下的裝置路徑，再核對精確 MI_00。實作見 [裝置辨識](src/lib.rs)、[診斷列舉](src/windows.rs)及 [USB 開啟](src/usb.rs)。WIA 與正式掃描流程須在換孔、拔插或重開機後重新發現，不得把開發機的完整實例 ID、序號、介面路徑或 USB 接孔位置寫死成安裝或執行條件。

### 掃描 USB 存取

初期建議用 Windows 內建 WinUSB 搭配 Rust 使用者模式程式。這讓硬體通訊與協定在一般程式內驗證，避免為探索封包新增核心程式碼。精確綁定 MI_00，保留父裝置與 MI_01。

`inquiry` 透過專案的裝置介面 GUID 列舉，再核對完整 MI_00 硬體識別及 USB VID/PID、介面號與類別。端點取自 USB 描述；`UsbSession` 提供獨占、有限長度的讀寫，`inquiry` 仍只送四位元組能力查詢。每次 USB 傳輸逾時 5 秒，禁止自動重送失敗命令或清除端點。`protocol::Capabilities` 檢查回覆框架、狀態、產品訊息種類、完整長度及非空能力；未知旗標保留供診斷，不能當成已支援的掃描設定。

能力欄位依 [SANE 1.4.0 INQUIRY](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.c#L775) 與 [解析度位元定義](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.c#L403) 獨立實作。能力位元與設定命令的解析度代碼不同，不可互換。幾何值保留 1/1200 英吋單位，尚未依未驗證的機型補償轉成有效掃描範圍。

[INF 設計稿](driver/wc3119-winusb.inf) 以型號／功能介面 `USB\VID_0924&PID_4265&MI_00` 配對並登錄固定 GUID，不含特定實例或孔位。它仍缺少 WDK 驗證與簽署 catalog，尚不能作為可分發的套件。

開發機的 [Rust 配對工具](examples/winusb_setup.rs) 要求完整實例 ID，只搜尋本機內建 `C:\Windows\INF\winusb.inf`，以 `DiInstallDevice` 綁定單一裝置。本機已獲授權並完成配對與真實 INQUIRY，結果見 [硬體紀錄](docs/hardware.md)。這次完整實例 ID 只限定被授權操作的目標，不是正式套件的匹配條件；正式套件仍需支援其他電腦的系統路徑、裝置實例及 USB 接孔。

真正的 Windows 掃描整合需要實作 WIA 驅動與安裝登錄。WinUSB 本身不會把裝置變成 Windows 掃描器。WIA 如何發現裝置、COM 生命週期、USB 句柄交接與一般使用者權限須先完成實機小範圍驗證，再確定正式安裝架構。

跨電腦安裝、換孔、拔插與重新開機的實機驗收集中於 [05](docs/tickets/05-windows-install.md)。單次工作可以使用目前取得的裝置路徑，重連後必須重新取得；多台候選不可任意選第一台。

### 掃描工作與影像

以實際 INQUIRY 回覆限定解析度、色彩模式及掃描範圍。先驗證無壓縮影像。不可把型錄的插值解析度當成光學解析度。

工作狀態規劃為「就緒 → 保留裝置 → 設定範圍 → 暖機／掃描 → 完成／取消／失敗 → 釋放」。同一台裝置只接受一個工作。呼叫端中斷或拒絕接收影像時，驅動須終止傳輸並釋放資源，不能回報成功。測試工具若保存影像，須保護既有檔案，並區分完整與中斷的輸出。

影像長度、列數、行寬、色彩通道與尾端填補必須彼此吻合。讀取採有限緩衝區，所有乘法、長度與配置量先驗證。取消後必須確認裝置可再掃描，必要時指示重新連接，不能無限等待。

`scan::scan_to` 以同一個獨占 USB session 重新查詢能力，再保留、設定、啟動與逐塊讀取。設定與分塊欄位依 [SANE 1.4.0 SET_WINDOW](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.c#L731)、[READ metadata](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.c#L1133)、[RGB 行序](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.c#L1206)的協定事實獨立實作，沒有移植上游程式。

目前僅處理 8-bit 灰階及 24-bit RGB、75–600 dpi、無壓縮平台影像。模式旗標按模式代碼位元檢查，壓縮 bit 0 作保守限制；上游沒有明確定義這些旗標的全部語義，仍須逐模式實機驗證。設定偏移必須能精確表示為百分之一英吋。RGB 行序 1 只重新排列通道，不改變像素值。回傳實際 READ 行寬／列數，尚未套用裁切、縮放或上游跨機型經驗補償，因此精確幾何仍待量測。

命令回覆除了框架與狀態外亦按用途限制訊息類型：INQUIRY 為 product，READ 為 scanner state 或 image band，SET_WINDOW 為空訊息／scanner state／scanning parameter，其餘控制命令為空訊息或 scanner state。本版本沒有 preview 設定流程，因此尚未接受 preview parameter 回覆；不能宣稱所有協定變體已支援。

每塊最多 16 MiB、每工作最多 256 MiB／4096 塊，讀取緩衝區 64 KiB。工作期限 120 秒；讀取期間取消、工作期限到達或第一次成功但零位元組的傳輸，會在剩餘長度仍明確時限時排空該塊。排空期限 10 秒，期間第二次空讀即停止，原始取消／錯誤原因保留。排空後才送 ABORT／RELEASE，不回傳已取消的像素、不重送 READ_IMAGE。這些期限另加正在執行的 5 秒 USB 呼叫與最多兩個清理命令，並非硬性 120 秒內涵蓋所有清理；callback 必須及時返回。

USB API 回傳錯誤可能已消耗未知長度，不能沿用舊的剩餘數量盲目排空。這種錯誤、短寫、超量、排空失敗或其他失同步仍回報需重插，不宣稱裝置已復原。呼叫端錯誤與可展開堆疊的 panic 會取消並釋放，合成測試已覆蓋。強制終止行程、panic=abort 及斷電仍無法依靠此清理路徑。

工作失敗診斷區分能力查詢、準備、保留、設定、啟動、資料描述、影像讀取、解碼、呼叫端及清理階段，保存工作耗時與成功交付的資料進度。影像讀取失敗另記已確認接收量及距上次有效資料的時間，保留原始錯誤種類與文字；清理錯誤另列，不遮蓋起因。失同步回覆不得把接收到的原文放入錯誤，因為其中可能是殘留影像。這是可觀測性補強，不改等待期限或代表歷史故障已修復。

控制命令等待另保存該次等待的 Busy 次數、最後 Busy 狀態與命令代碼；開始下一個命令時清除，避免把先前暖機狀態誤配到新命令。Busy 診斷只有在 CHECK（0x02）且訊息為 scanner state（0x20）時才解釋狀態位元，READ 位於 12／13、其他命令位於 4／5；單純 BUSY（0x08）的狀態欄位記為 unknown。[SANE 1.4.0 狀態定義](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.h#L173)

`examples/capture_scan.rs` 是開發驗證呼叫端，只寫入新建目錄，透過 `scan_with_evidence` 在同一個獨占 session 保存能力並產生設定，避免先查能力、重新開啟另一台裝置後才掃描的競態。`inquiry.txt` 保存描述與能力，`evidence.txt` 保存傳輸結果，另保存每塊 USB 原文及解碼後像素，最後用實際尺寸產生 PGM／PPM。只有完整掃描釋放及輸出同步成功才建立 `complete.txt`，沒有完成標記的目錄視為中斷資料，不當成成功影像。這不是 GUI 或正式使用者掃描程式。

`examples/scan_stability.rs` 用同一程序反覆呼叫核心，每次獨立取得當下能力及 USB session，藉此驗證多次工作後的釋放與模式切換。逐塊以獨立索引核對 wire／pixels，但只保存無影像的進度與結果，外部觀測程序量測記憶體。次數有上限，任何失敗即停止；不以新程序重啟或自動重試掩蓋累積狀態。可靠性驗收與仍未完成的復原條件由 [03](docs/tickets/03-recover-scan.md) 管理。

### Windows 影像串流

`bitmap::BmpEncoder` 將 `ImageBand` 逐列寫入新的空白 `Write + Seek` 串流。沿用掃描核心的影像 callback，輸出錯誤會經既有路徑取消及釋放裝置。只有掃描回傳成功的 `ScanSummary` 才可呼叫 `finish`，核對尺寸與資料量後完成標頭。呼叫端遇任何錯誤都不得交付成功影像，不能只看檔名或影像簽名認定成功。開發擷取範例保留中斷資料供診斷，以 `complete.txt` 區分是否完整。

使用無壓縮 BMP、40-byte BITMAPINFOHEADER、負高度的由上而下行序，避免為翻轉影像而暫存整張原稿。灰階採 8-bit 與中性灰色盤，RGB 採 24-bit BGR，每列補齊四位元組。保留裝置交付的列順序，只改通道排列與填補，不套亮度、gamma、色彩描述或壓縮。原稿的實體上下方向仍待有內容的文件驗證。額外暫存限一列，尺寸及累計資料量另設上限。[Microsoft DIB 行序](https://learn.microsoft.com/en-us/windows/win32/gdi/device-independent-bitmaps)、[BMP 儲存格式](https://learn.microsoft.com/en-us/windows/win32/gdi/bitmap-storage)

這是供 WIA 轉接元件使用的影像編碼，不是已完成的 WIA 驅動。`com_stream::ComOutputStream` 已提供 Windows 原生 IStream 的 `Write + Seek` 轉接，不依賴 `Read`、`Stat`、`Commit` 或 `SetSize`，`flush` 明確回報不支援。檔案同步仍由開發擷取範例負責。`finish` 成功後串流位置在 byte 2，後續定位及交付由呼叫端管理；不是已確認的 WIA 結束位置規則。WIA 裝置發現、屬性、minidriver COM 生命週期及 Windows 掃描的 top-down BMP 相容性仍須依 [05](docs/tickets/05-windows-install.md) 驗證。[Microsoft WIA 串流規則](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/istream-data-transfer-driver-changes)

`ComOutputStream::from_raw_owned` 接收一個已擁有的 IStream 參考，呼叫端負責 COM 執行緒初始化及指標有效性，轉接物件只在原執行緒使用並於 Drop 釋放一次。原生寫入每次最多 ULONG 長度，短寫按回報數量繼續，零進度及超量回報拒絕；任何非 S_OK 結果均回傳 `StreamError` 保留原始 HRESULT，包括已寫入部分資料後的失敗。`write_all` 遇 Interrupted 立即停止，不採標準函式的自動重試。S_FALSE 在此只是未支援的串流回覆，不推論為 WIA callback 的取消。絕對 Seek 保留完整 u64 位元，對應 COM 在 STREAM_SEEK_SET 時以無號解釋位移的規則。[Microsoft Write](https://learn.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-isequentialstream-write)、[Microsoft Seek](https://learn.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-istream-seek)

原生邊界以 Windows SDK 10.0.26100.0 `objidlbase.h` 的 IStream 方法順序核對，並使用 Windows `CreateStreamOnHGlobal` 真實物件完成 BMP 寫入及讀回測試。該測試只在測試執行緒初始化 COM，釋放串流後解除初始化，不登錄 DLL、不啟動掃描。這證明目前 Windows x64 的串流呼叫可運作，不替代 WIA 服務提供的實際串流驗收。[Microsoft OLE 記憶體串流](https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-createstreamonhglobal)

### WIA 設定與掃描入口

`wia::FlatbedSettings` 接受 WIA 純數值設定，`to_request` 不操作 USB，將像素位置／範圍精確轉成 1/1200 英吋。接受對稱 75、100、150、200、300、600 dpi，datatype/depth 僅灰階 2/8 與彩色 3/24，格式限 BMP、compression=0、brightness/contrast=0。不支援的組合明確拒絕。位置另須符合協定的 1/100 英吋步進，六種解析度依序每 3、1、3、2、3、6 像素一格，不能靜默取整。欄位與 GUID 依 Windows SDK 10.0.26100.0 `wiadef.h` 核對。[Microsoft XPOS](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-xpos)、[datatype](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ipa-datatype)

`wia::scan_bmp` 先驗證設定及預先取消，再沿用 `scan_to` 與 `BmpEncoder` 啟動真實掃描。當次能力與工作來自同一個 USB session，範圍及模式不符時在 RESERVE 前拒絕。輸出須為空白 `Write + Seek` 串流，可使用 `ComOutputStream`。任何錯誤均不交付成功影像，只有工作及清理成功後才完成 BMP 標頭。輸出尺寸保留實際 READ 結果，沒有新增裁切、縮放或明暗處理。

WIA 選取範圍 `XEXTENT/YEXTENT` 與輸出尺寸屬性用途不同。正式屬性模型須維護範圍、位置、解析度與頁面間的關係，不能因 READ 回傳不同就假定應覆寫選取範圍。Microsoft 建議應用程式以影像標頭取得實際尺寸。這個入口尚未實作屬性儲存或同步，也沒有 WIA callback／COM minidriver，實際消費與幾何契約由 05 驗收。[Microsoft XEXTENT](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-xextent)、[PIXELS_PER_LINE](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ipa-pixels-per-line)

### COM DLL 載入與驗證

Cargo 同時建置 Rust `rlib` 與原生 `cdylib`。Windows release 檔案為 `target/release/workcentre_3119.dll`，輸出 `DllGetClassObject` 與 `DllCanUnloadNow`，class ID 為 `{F71A8435-AA10-40A6-8334-49EEC8FE9C63}`。此 ID 識別專案的 COM 類別，不含裝置實例或 USB 孔位，也未登錄到系統。

`com_server` 的 factory 支援 `IUnknown`／`IClassFactory`，建立的物件支援 `IUnknown`／`IStiUSD`，兩者共享物件身分與參考計數。未知類別回傳 `CLASS_E_CLASSNOTAVAILABLE`，不支援的介面回傳 `E_NOINTERFACE`，無效輸出指標回傳 `E_POINTER`，失敗時清空有效的輸出欄位。尚未支援 COM aggregation，非空 outer 指標回傳 `CLASS_E_NOAGGREGATION`。`IWiaMiniDrv` 的初始化、屬性與掃描方法仍須接續實作，不能把載入成功當成 WIA 服務已接受此 DLL。[Microsoft COM 識別要求](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/providing-a-com-interface)、[QueryInterface 規則](https://learn.microsoft.com/en-us/windows/win32/com/rules-for-implementing-queryinterface)

`com_server::sti` 的 19 個方法位置依 SDK `stiusd.h` 宣告。Initialize 驗證 Unicode STI 2 版本，保留 helper 參考並取得有上限的 UTF-16 port name，借用的登錄句柄不使用也不關閉。helper 呼叫在狀態鎖外執行，失敗初始化可重試。LockDevice 重新列舉當下 MI_00，僅開啟與 helper 路徑相符的裝置，沒有找不到時改選其他裝置的行為。UnLockDevice 與最終 Release 釋放持有的 USB session。Diagnostic 僅在鎖定後執行既有 INQUIRY 並驗證能力回覆，不能用此結果宣稱掃描馬達已就緒。未實作的狀態、reset、raw、escape 與通知回傳不支援，GetCapabilities 目前不宣告 WIA／通知能力。[Microsoft IStiUSD](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/providing-an-istiusd-interface)

後續 WIA 影像傳輸必須重用 IStiUSD 已持有的 session；目前 `wia::scan_bmp` 會自行開啟 USB，不能直接在 LockDevice 期間呼叫它。WIA helper 實際回傳的 port name 不保證是本專案 WinUSB 路徑，必須完成服務端身分映射及存取權驗證，不能將合成 helper 的實機測試當成 WIA 發現成功。

本階段接續實作的流程為「IStiUSD 鎖定 → 暫借同一 session → 掃描／BMP 輸出 → 依核心回報歸還或隔離」。借用期間不持有狀態 Mutex，讓影像呼叫端重入查詢時能返回；再次掃描、解鎖及診斷則立即回報忙碌。成功、未送命令的取消、或經確認清理完成的失敗可以歸還連線；失同步、未知消耗量及清理失敗必須隔離，不能分析錯誤字串來判斷可重用。隔離後本物件不再開啟或送命令，重新連線的跨物件身分驗證仍由 03 完成。

這段驗證沿用核心合成封包測試及 COM 契約測試，另以具 Drop 計數的資源驗證同一借用器的重入、並行排他及 panic 清理。實機驗證須透過鎖定中的 COM 物件掃描並檢查 BMP，不能另開一個 session 代替。無效設定與預先取消須在碰觸串流前拒絕；USB／輸出失敗保留原錯誤，任何失敗不完成 BMP。這段不新增系統登錄或安裝步驟。

DLL 的動態測試獨立宣告 SDK ABI，使用 `LoadLibraryExW`／`GetProcAddress` 取得當次建置的實際輸出函式，建立及釋放物件後呼叫 `FreeLibrary`。它不使用登錄、`CoCreateInstance` 或 USB，也沒有新增掃描 App。呼叫端必須在所有介面參考釋放後才卸載 DLL，不能在其他執行緒仍呼叫 DLL 時強制卸載。[Microsoft DllCanUnloadNow](https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-dllcanunloadnow)

物件與 server lock 共用一個原子 hold 計數，避免分開讀取兩個計數時誤判可卸載。最後一個參考先釋放物件配置再減少 hold；參考計數飽和後永久保留，不能溢位釋放。LockServer 以 module 為範圍，跨 factory 解鎖的測試是額外容錯測試，一般呼叫端仍應使用原 factory 平衡 lock／unlock。[Microsoft ATL module lock](https://learn.microsoft.com/en-us/cpp/atl/reference/ccomclassfactory-class?view=msvc-170)

MSVC 建置由 `build.rs` 傳入 `driver/com-exports.def`，兩個 COM 入口標示 PRIVATE，保留 DLL export 並排除 import library 項目。2026-09-13 實際 exports 恰為上述兩個入口，沒有 LNK4104 警告。此建置仍依賴 `VCRUNTIME140.dll` 與 Windows 系統 runtime，正式套件須處理 runtime 前置條件，不能由開發機載入成功推論乾淨電腦可用。[Microsoft LNK4104](https://learn.microsoft.com/en-us/cpp/error-messages/tool-errors/linker-tools-warning-lnk4104?view=msvc-170)

正式整合還須驗證 COM 管理的 `CoGetClassObject`／`CoFreeUnusedLibraries` 並行載入排程，以及 WIA 是否要求 aggregation。現在的直接 DLL 測試由呼叫端持有 loader reference，不涵蓋上述排程。WIA 初始化應依 SDK 的 `IStiUSD::Initialize`、`GetCapabilities` 及 `IWiaMiniDrv::drvInitializeWia` 實作；保留 WIA 提供的 COM 物件需取得參考，借用的裝置參數登錄句柄不得關閉。[Microsoft 初始化](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/stiusd/nf-stiusd-istiusd-initialize)、[WIA 載入流程](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/loading-and-unloading-a-wia-minidriver)

更動 COM DLL 後，除了一般檢查，另執行：

```powershell
cargo build --offline --release
$env:WC3119_TEST_DLL = (Resolve-Path target/release/workcentre_3119.dll).Path
cargo test --offline --test com_server_dll -- --ignored
```

指定的 DLL 必須是絕對路徑且為本次建置。測試預設 ignored，以免一般測試誤載舊版 release；必須記錄額外執行結果及 DLL 雜湊。此驗證不取代 WIA 服務帳號、系統登錄或 Windows 掃描驗收。

### 效能與跨機型共用

使用者要求加速時維持解析度、色深、掃描範圍與品質，並讓資料緩衝、排程及影像處理能供其他機型沿用。機型命令、資料邊界及允許的並行程度仍由機型協定負責；尚未取得第二種機型作共用性驗證，不預先假定所有裝置都能並行讀取。

先分段量測再調整：階段壁鐘時間與 USB 呼叫時間是相互重疊的觀察值，不能相加。USB 呼叫包含等候裝置資料，不能解讀成純匯流排速度；僅由主機時間也無法拆出掃描頭運動與曝光時間。保留取消、期限、記憶體上限及輸出順序，優化同一批 USB 資料後須逐像素相同；實機品質另依 07 驗收。

初步離線測試重播已取得的完整 RGB600（117 塊、106503300 像素 bytes），以未修改的 `BandInfo::decode` release 最佳化編譯，預載資料後 2 次暖身、10 次量測為 31.641–32.526 ms，中位數 32.018 ms，結果與原像素完全相同。此測試含解碼配置，排除磁碟、USB、機器與呼叫端；不能當成完整掃描 profile，但沒有支持優先平行化 RGB 重排的證據。本機量測留於 `artifacts/performance-offline-20260913-d/`。

`scan_with_profile` 提供開發量測：沿用同一 session 的準備與影像 callback，成功及一般錯誤返回皆保留 `ScanProfile`。每次呼叫先清空舊資料，參數拒絕、預先取消及 USB 開啟失敗時沒有工作量測。階段時間自開啟及上限查詢後的 INQUIRY 起算，涵蓋呼叫端及清理，USB 次數與耗時包含失敗呼叫。最後參數接受 `Into<ScanTuning>`，既有 `Duration` 呼叫沿用 64 KiB 讀取，`ScanTuning` 可指定開發用緩衝區大小。

開發用 READ Busy 間隔限 1–1000 ms，只有 0x28 的 Busy 等待使用它；其他命令仍為 100 ms。較長間隔可能增加發現新資料或取消的延遲，最多另受當下睡眠及 USB 呼叫限制，不改工作／排空期限或傳輸政策。穩定性範例以 `--read-poll-ms` 暴露此實驗並保存 profile，即使掃描與診斷寫入同時失敗也保留原始掃描錯誤。此參數不屬於正式使用者的品質／速度選項。

影像讀取緩衝區接受 1 KiB–1 MiB、1024 bytes 的整數倍，預設 64 KiB。在 INQUIRY／RESERVE 前以 `WinUsb_GetPipePolicy` 讀取當下 bulk IN 的 `MAXIMUM_TRANSFER_SIZE`，超過者拒絕，不啟動掃描。回覆須為正確大小的 ULONG、非零且至少可容納一個端點封包，不假定回報上限本身能整除封包大小。較大緩衝區可能減少呼叫次數，也可能延長下一次取消檢查前的等待，需實測判斷。緩衝區每塊以 heap 配置，仍按剩餘資料限制每次要求長度並保留短讀、超量與排空處理。profile 記錄要求大小及已查得上限，開啟／上限查詢時間不計入階段耗時。[Microsoft GetPipePolicy](https://learn.microsoft.com/en-us/windows/win32/api/winusb/nf-winusb-winusb_getpipepolicy)、[pipe policy](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/winusb-functions-for-pipe-policy-modification)

此處採用回報上限作為開發實驗的保守限制。Microsoft 對要求長度與封包倍數的嚴格限制適用於 `RAW_IO`，本專案未啟用該政策，不能據此斷言一般 WinUSB 超過此長度必定失敗。所有公開掃描入口共用預檢，拒絕超限時 profile 保留要求值與回報上限。64／256 KiB 實機比較沒有縮短影像讀取時間，預設維持 64 KiB，見 [測試證據](docs/hardware.md#64-kib-與-256-kib-影像讀取對照)。

### 掃描明暗品質

使用者回報過去使用原廠驅動及 Windows 掃描時，彩色有過度曝光感，灰階偏淡。這是待重現的歷史症狀，不能由目前缺少 USB 驅動的問題碼 28 解釋。調查與修正的單一驗收來源是 [07 — 掃描明暗品質](docs/tickets/07-scan-tones.md)。

診斷時比較「USB 回傳的影像樣本 → Rust 解碼後影像 → 驅動轉換後串流 → 掃描軟體保存的檔案」，找出第一次出現明暗偏差的位置。USB 影像可能已受裝置內部處理，不得稱為未處理的感測器資料。分別記錄解碼與明暗轉換，避免同一個亮度設定被硬體、驅動或呼叫端重複套用。

WIA 的亮度與對比設定由驅動維護，標準正常值皆為 0。遵循 [Microsoft 亮度定義](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-brightness)及[對比定義](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-contrast)，實際硬體映射或必要的軟體轉換須依量測決定。目前沒有已確認的曝光或 gamma 控制命令。

[SANE 1.4.0 的模式處理](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.c#L674)僅在線稿與半色調模式啟用 threshold，不可用該參數修正灰階或彩色。原始樣本若已把不同的淺色合併成相同白值，單純壓暗輸出無法分辨原來的細節，必須往取得影像前的設定或硬體狀態查證。

### 列印傳輸

列印介面有 `usbprint` 只表示 USB 列印傳輸已啟動，本機目前沒有 Xerox 列印佇列。列印資料格式與 Windows 整合另行查證，不能假定支援 PCL、PostScript 或通用 IPP。

## 測試策略

| 測試位置 | 驗證內容 | 證據限制 |
| --- | --- | --- |
| 公開裝置辨識與診斷 API | 精確 ID、錯誤碼、不同既有驅動、父裝置隔離 | 不證明 USB 通訊 |
| CLI 行程 | 參數、說明、結束碼 | 不以模擬資料宣稱找到實機 |
| 掃描工作與 USB 傳輸交界 | 封包、短讀寫、取消、長度上限、錯誤復原 | 合成資料須另有真實硬體對照 |
| 實機端到端 | 實際文件、色彩、範圍、重掃、拔線、暖機、睡眠 | 需要已核准的驅動綁定 |
| 掃描明暗品質 | 同一原稿逐段比較、淺灰與近白細節、亮度方向與中性值 | 目前只有空平台影像，不能宣稱偏白已修復 |
| Windows 整合及可攜安裝 | 第二台乾淨支援 Windows 11 x64、模型套件安裝、一般使用者 WIA 掃描、換孔／拔插／重開機後重新發現、解除安裝及列印 | 開發機只驗證配對與 INQUIRY；尚無跨電腦、WIA 或換孔／拔插／重開機證據，且仍缺 catalog 與適用簽署 |

不為每個內部函式另建替身。掃描、影像輸出與取消優先從公開工作介面測試，保留真實底層呼叫的整合測試。

## 協定與來源決策

上游 SANE 1.4.0 支援表列此裝置為 `xerox_mfp`，評級 Good。這支持研究可行性，不能證明本 Rust 實作相容。

- [SANE 支援表](https://sane-project.gitlab.io/website/sane-backends.html)
- [固定版本 USB 裝置設定](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.conf.in#L236)
- [固定版本協定常數](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.h#L160)
- [固定版本命令與掃描流程](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.c#L243)
- [WIA 架構](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-architecture-overview)
- [Microsoft WinUSB 安裝方式](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/winusb-installation)

維持既有 MIT，尚未移植 SANE 程式碼。若採移植路線，需先確認元件授權、來源保留與分發安排。

## 減法審查

- 暫不加入雲端帳號、網路掃描分享、背景更新常駐服務，因為目前的目標是本機 USB 掃描。
- 建議移除自寫 USB 核心驅動的需求，先驗證 Windows 內建傳輸是否足夠。
- 移除自製 GUI、照片預設、影像編輯與 PDF 組頁工作，這些操作交由既有掃描軟體提供。
- 保留 WIA 能力與影像傳輸契約，讓既有軟體取得有效設定、進度及錯誤。掃描範圍與模式的硬體支援照常驗證。
