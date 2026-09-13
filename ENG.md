# WorkCentre 3119 工程設計

## 目標與現況

目標是在 Windows 11 x64 用 Rust 完成可以長期使用的純驅動，掃描優先，列印接續。不開發 GUI 或掃描 App，操作由 Windows 掃描等既有軟體提供。現有實作包含裝置診斷與待實機驗證的 USB 能力查詢。功能的驗收條件與失敗情境由 [工作項目](docs/tickets/) 管理，進度由 [delivery-status.md](delivery-status.md) 管理。

## 使用流程與架構

```text
使用者執行診斷 → Rust 裝置辨識 → Windows Configuration Manager → 狀態與修復提示
既有掃描軟體 → Windows WIA → Rust 掃描工作 → USB 傳輸 → MI_00
      ↑                           ↓
      └──── 影像串流／狀態／錯誤 ────┘
使用者列印 → Windows 列印佇列 → 待查證的列印協定 → MI_01
```

第一行已實作，其餘是目標架構。USB 回覆與呼叫端參數均需檢查長度及數量。驅動負責回傳影像資料、能力、進度及錯誤，呼叫端軟體負責預覽畫面、編輯及檔案儲存。診斷預設不記錄序號及影像內容。

### 裝置識別

使用 Windows Configuration Manager 原生 API，不依賴 PowerShell 子行程。只查詢目前存在的裝置，精確識別父裝置、MI_00 與 MI_01。拔線造成列舉失敗時回報檢查失敗，不能顯示為「沒有裝置」。有多台時不得自動選第一台。

目前 `doctor` 只確認 PnP 驅動狀態。`DriverStarted` 表示 WinUSB 服務已啟動，不能當成已開啟 USB、端點可用或已支援 WIA。

### 掃描 USB 存取

初期建議用 Windows 內建 WinUSB 搭配 Rust 使用者模式程式。這讓硬體通訊與協定在一般程式內驗證，避免為探索封包新增核心程式碼。精確綁定 MI_00，保留父裝置與 MI_01。

`inquiry` 透過專案的裝置介面 GUID 列舉，再核對完整 MI_00 硬體識別及 USB VID/PID、介面號與類別。端點取自 USB 描述，命令僅限四位元組 INQUIRY。傳輸具有限逾時，失敗直接釋放資源，不重試或清除端點。`protocol::Capabilities` 檢查回覆框架、狀態、產品訊息種類、完整長度及非空能力；未知旗標保留供診斷，不能當成已支援的掃描設定。

能力欄位依 [SANE 1.4.0 INQUIRY](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.c#L775) 與 [解析度位元定義](https://gitlab.com/sane-project/backends/-/blob/1.4.0/backend/xerox_mfp.c#L403) 獨立實作。能力位元與設定命令的解析度代碼不同，不可互換。幾何值保留 1/1200 英吋單位，尚未依未驗證的機型補償轉成有效掃描範圍。

綁定及正式簽署方式尚未核准。自訂 INF 需要簽章目錄檔，目前只有 [INF 設計稿](driver/wc3119-winusb.inf)，不是可分發的套件。開發機可評估 Microsoft 文件中的內建 WinUSB 手動配對方式，但本機尚未驗證該流程。

真正的 Windows 掃描整合需要實作 WIA 驅動與安裝登錄。WinUSB 本身不會把裝置變成 Windows 掃描器。WIA 如何發現裝置、COM 生命週期、USB 句柄交接與一般使用者權限須先完成實機小範圍驗證，再確定正式安裝架構。

### 掃描工作與影像

以實際 INQUIRY 回覆限定解析度、色彩模式及掃描範圍。先驗證無壓縮影像。不可把型錄的插值解析度當成光學解析度。

工作狀態規劃為「就緒 → 保留裝置 → 設定範圍 → 暖機／掃描 → 完成／取消／失敗 → 釋放」。同一台裝置只接受一個工作。呼叫端中斷或拒絕接收影像時，驅動須終止傳輸並釋放資源，不能回報成功。測試工具若保存影像，須保護既有檔案，並區分完整與中斷的輸出。

影像長度、列數、行寬、色彩通道與尾端填補必須彼此吻合。讀取採有限緩衝區，所有乘法、長度與配置量先驗證。取消後必須確認裝置可再掃描，必要時指示重新連接，不能無限等待。

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
| 未來掃描工作與 USB 傳輸交界 | 封包、短讀寫、取消、長度上限、錯誤復原 | 合成資料須另有真實硬體對照 |
| 實機端到端 | 實際文件、色彩、範圍、重掃、拔線、暖機、睡眠 | 需要已核准的驅動綁定 |
| 掃描明暗品質 | 同一原稿逐段比較、淺灰與近白細節、亮度方向與中性值 | 目前無影像，不能宣稱偏白已修復 |
| Windows 整合及乾淨安裝 | Windows 掃描、一般使用者、重開機、解除安裝、列印 | 需簽署及安裝驗證環境 |

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
