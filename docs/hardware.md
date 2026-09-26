# 本機硬體觀測

觀測日期：2026-09-13、2026-09-14、2026-09-26。此檔省略 USB 序號與完整實例路徑。

## 0.2.18 WIA 連續 20 次整版掃描

2026-09-26，使用已安裝的 0.2.18.0 與同一個 WIA automation 裝置／平台物件，循環 RGB600、RGB600、RGB300、Gray600 五輪。20 次全部完成，累計傳輸 1435.381 秒，沒有重試、服務重啟或重新連接。DLL SHA256 為 `5B227D444CCA40E6BA7C5F2BD2DD18177EA56F07368E15F67B6219A0FF970D11`。

| 模式 | 次數 | 每次耗時 | 實際影像尺寸 |
| --- | --- | --- | --- |
| 彩色 600 dpi | 10 | 92.441–105.317 秒 | 5100×6961、24 bpp |
| 彩色 300 dpi | 5 | 39.653–39.893 秒 | 2556×3476、24 bpp |
| 灰階 600 dpi | 5 | 42.041–42.309 秒 | 5100×6959、8 bpp |

範圍依當下 WIA 根項目回報的 8500×11700 千分之一英吋換算；600 dpi 寫入並讀回 5100×7020，300 dpi 為 2550×3510。輸出保留裝置實際尺寸，未裁切、補列或調整明暗。範圍單位依 [Microsoft WIA_DPS_HORIZONTAL_BED_SIZE](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-dps-horizontal-bed-size)。此輪只驗證整版傳輸、模式切換及有限記憶體，不保存像素，不能代替文件品質或精確幾何驗收。

同一 WIA 服務程序的 private bytes 從 3,653,632 降至 3,190,784 bytes。第一張進行中開始每 500 ms 外部取樣，共 2694 筆，觀測到的 private bytes 峰值 5,038,080 bytes（約 4.80 MiB）、working set 峰值 17,461,248 bytes。取樣未涵蓋第一張開頭，峰值只代表取樣範圍；20 次之間未見持續增長，不是永久無洩漏的證明。

正式證據在 `artifacts/wia-stability-fullbed-20260926/`：`run.ps1`、`results/runs.jsonl`、`results/complete.txt`、`memory-samples.csv` 與 `verified-summary.json`。先前 `artifacts/wia-stability-20260926/` 的 20 次是小選區：WIA automation 在切換 dpi 後仍回報初始 `SubTypeMax=637×877`，測試工具誤用該值。該組保留 `assessment.txt`，不計入整版驗收。600 dpi 歷史中斷此次未重現，原因仍未查明，不能宣稱已修復。

## 首塊 metadata Ready 後取消的對照

2026-09-26，新增僅在測試建置啟用的 `metadata_ready_after_busy`。第一次合法 READ Busy 在 502 ms 記錄 pending，但不設主機取消旗標；6195 ms 收到並驗證第一塊 metadata 後才設旗標，沿用既有 READ_IMAGE 將 921472 bytes 排空，未向影像消費端交付任何一塊。ABORT／RELEASE 都回 status=0、message=0，工作 7043 ms 以主機取消返回；同一 USB session 的 RGB75 重掃 19314 ms 完成 648×871、2 塊。整組 26.38 秒通過並有 `complete.txt`。

這與舊版 metadata Busy 當下取消後卡住、第一塊交付後取消可重掃的紀錄形成階段對照。它支持延後至首塊可讀並排空的處理方向，但不揭露韌體內部根因，也不代表正式驅動已套用修復。既有 120 秒工作期限、10 秒排空期限及命令順序不變。測試前暫停 stisvc，結束後恢復原本 Running；沒有重新安裝、重設 USB 或重插。

私人證據在 `artifacts/cancel-ready-20260926/`，包含服務前後狀態、測試輸出及 `results/diagnostics.log`。測試執行檔 SHA256：`A8679B2AB7D6F1A7CC08A844188AF0D07F03F9B2F3B998ED9F468E281796E162`。此為合成取消旗標加真實 USB，不是 Windows 取消事件驗收。

## 0.2.19 核心取消修復驗收

2026-09-26，正式核心在 START 成功後延後處理首塊前的主機取消，沿用 120 秒工作期限等候 metadata，讀完已知長度後才中止並釋放。若取消仍待處理、metadata 等待卻以逾時或裝置錯誤結束，即使清理回覆成功也隔離 session，要求重連；此異常分類有合成回歸測試，尚未製造實機逾時驗收。

| 精確階段 | 取消結果 | 同 USB session 重掃 | 整組結果 |
| --- | --- | --- | --- |
| metadata_busy | 245 ms 設取消；6796 ms 結束，921472 bytes 排空，零影像交付 | RGB75，19115 ms，648×871、2 塊 | 25.93 秒，complete.txt 存在 |
| first_band | 6662 ms 設取消；6749 ms 結束 | RGB75，19104 ms，648×871、2 塊 | 25.87 秒，complete.txt 存在 |

兩組均逐一重新列舉唯一 MI_00，ABORT／RELEASE 成功，未重插或重設 USB。測試期間暫停 WIA，結束均恢復原本 Running。私人證據分別在 `artifacts/cancel-core-hardware-20260926/` 與 `artifacts/cancel-core-first-band-20260926/`。測試執行檔 SHA256：`D4B990B540FFA26A95F0335D58AA3178F55C51DCC93B039EFFC1F0DBEA32F25B`；對應 release DLL SHA256：`7345169FEF9AA9647C648B0EB8EF3F6CE0DC4ED8B943E16CEF5E615EC792ACD0`。此處是核心 USB 驗收，安裝及 Windows API 驗收另列。

原本失敗的 `actual_wia_dispatch_cancels_from_parallel_thread_and_rescans` 也以同一測試執行檔從閒置裝置重跑通過。另一執行緒在 500 ms 通知取消，7223 ms 返回 S_FALSE，零影像 bytes；同一驅動物件接續彩色與灰階成功，整組 42.64 秒並建立 `complete.txt`。合成服務 helper／設定／callback 搭配真正 USB、Windows 項目與 IStream，不能替代真實 WIA 服務取消。私人證據：`artifacts/cancel-core-parallel-idle-20260926/`。

此前緊接 first_band 重掃便啟動的 `artifacts/cancel-core-parallel-20260926/` 未符合閒置前提：500 ms 取消發生在 RESERVE Busy，未取得保留權，610 ms 結束且沒有完成標記。保留該失敗，不把未取得保留權時的隔離行為當成已修復。這個分支沒有送 ABORT 取得別人的保留權，仍須依原始錯誤及重連要求處理。

## 0.2.18 Windows 掃描 API 取消與重掃

2026-09-26，以 Windows PowerShell 5.1 呼叫 WinRT `ImageScanner.ScanFilesToFolderAsync`，同一個 ImageScanner 物件依序執行 RGB75 取消、RGB75 重掃及 Gray75 對照。第一輪在 508 ms 呼叫原始非同步操作的 `Cancel`，575 ms 觀察到 `Canceled`；彩色重掃 19051 ms、灰階 17851 ms 完成，BMP 標頭均為 648×871，分別 24／8 bpp。沒有重啟服務或重插。這是 Windows 掃描所用 API 的實際取消結果，不是 App 按鈕互動，也不能以 API 返回時間推論機構停止時間。

此輪仍使用已安裝的 0.2.18.0，不包含之後的核心延後取消修正。私人證據：`artifacts/wia-cancel-20260926/run-d/` 的 `results.json`、`complete.txt` 與兩張 BMP。前面 run-a／run-b 是掃描前的診斷腳本相容性失敗；run-c 在啟動後因腳本型別錯誤退出，WIA trace 確認驅動其後以 `0x800706E5` 結束。三輪均不計成功驗收，保留失敗紀錄。

## 0.2.18 WIA 意圖切換與更新後掃描

2026-09-26，本機更新 0.2.18.0 成功後，以 WIA automation 同一連線寫入彩色／灰階意圖並重複寫入，DATATYPE／DEPTH 分別維持 3／24 與 2／8，舊版只改色深的錯誤未再發生。彩色 75 dpi 10.534 秒、灰階 75 dpi 17.924 秒，各完成 648×871 BMP；Pillow 完整解碼及目視檢查為空平台與少量細點，不能驗收偏白或文字品質。安裝後 DLL 雜湊與已測套件一致，父裝置及 MI_01 的服務、INF、問題碼、類別及 Parent 與備份相符。掃描後 stisvc Running、MI_00 問題碼 0、WIA 1 台。詳細更新與私人證據位置見 [05 審查修復](tickets/05-windows-install.md#2026-09-26-審查修復02180)。

## 精確階段取消與同連線重掃

2026-09-14，以僅在測試建置啟用的 `scan::hardware::actual_cancel_phase_then_same_session_rescan` 作兩次序列對照。每次重新列舉唯一啟用的 MI_00、建立新輸出目錄，一次開啟 USB 後沿用同一 session 取消及立即重掃。設定由當下 INQUIRY 建立全平台 RGB75；正式命令、64 KiB 緩衝區、100 ms Busy 等待及 120 秒期限未更動。

| 取消階段 | 取消與清理 | 同 session 重掃 | 驗收 |
| --- | --- | --- | --- |
| 第一塊影像後 | 7007 ms 觸發；工作 7103 ms 以 Interrupted 返回，已接收 1 塊／921456 像素 bytes；ABORT／RELEASE 均 status=0、message=0 | 19284 ms 完成，648×871、2 塊、1693224 像素 bytes | 26.41 秒通過，complete.txt 存在 |
| 首次 READ metadata Busy | 246 ms 觸發；工作 444 ms 以 Interrupted 返回，零影像；ABORT／RELEASE 均 status=0、message=0 | RESERVE 收到 800 次 Busy，120065 ms 達期限，零影像，NeedsReconnect | 120.52 秒失敗，沒有 complete.txt |

取消後核心均回報 SessionHealth::Ready；這是目前清理及串流同步判斷，不是裝置已能開始下一次掃描的證明。精確對照重現首塊前後差異，仍不能判定機器故障或修復策略。沒有增加重試、固定延遲、USB reset／CLEAR_HALT、重插或系統設定變更。測試後 doctor 三個介面問題碼均 0、started=true，不能據此認定掃描機構已復原。

此測試只累計影像尺寸、塊數及 bytes，保存命令、數值 status／message 和耗時，沒有保存或檢查影像內容。它不是文件品質、WIA 服務、600 dpi 穩定性或完整取消復原驗收。

私人診斷與完成標記分別在 `artifacts/cancel-phase-first-band-20260914-b/`、`artifacts/cancel-phase-metadata-busy-20260914-b/`。首塊後測試執行檔 SHA256：`58B2CCB6AA367B281E5BF3D223AB465FF3D6B8BE6A09B7835BD9DC3647B3DF38`；首塊前測試：`F7DCB775620E2751B47EA94B536C8D484142066EDA2E9B9FAEEA32B67F7E1150`。兩次之間只修正測試日誌的成功判定文字、補合成回歸測試及格式，不改正式協定與取消時機。首塊後日誌的 `status=good` 是當時由數值狀態推導的文字，不能單獨視為整個命令回覆有效；最終版僅記錄原始數值欄位。最終測試來源 SHA256：`FFD094F0B0B5F69EFB90ADA6F54F146827CA8D69C86429A6AE052B194841371A`。

## WIA 屬性初始化的能力查詢

屬性相依驗證完成後再次重新列舉介面，以最終版本單獨執行下述能力查詢測試，0.02 秒通過。測試執行檔 SHA256：`F7DCB775620E2751B47EA94B536C8D484142066EDA2E9B9FAEEA32B67F7E1150`；鎖定來源：`79FFC1E6D6DE4FCB6F192DA2E82720528BFA5FC027E64E0BCDC71194F7F21CD4`。證據為 `artifacts/wia-property-validation-20260914-b/live-capabilities-final.txt`。它只確認鎖定／能力查詢／解鎖，不驗證屬性發佈或前述取消後重掃。以下保留初始化版本的歷史結果。

2026-09-14，重新列舉唯一啟用的 MI_00 後，個別執行 `actual_wia_lock_queries_live_capabilities_without_scan --ignored --exact`，0.02 秒通過。測試使用原生 Windows 項目樹、合成 IStiDevice 服務轉送及真實 IStiUSD／WinUSB／INQUIRY，驗證一次鎖定、查詢、解鎖、解除連線與參考釋放。沒有掃描、影像或服務 property context，不代表 WIA 服務已接受初始化。

當次測試執行檔 SHA256：`812BE3AC87961283984CF41D77D407EC8381C5E5EF1DCC79868586A804F127CA`；鎖定來源：`2C0D389945342ADC5C462CD1D61C56F0808EE3FE8EC7FD63D59FC74B675FCD26`；STI 來源：`785D5E74AF7054EDFC566C6D65B9415CE38B8E8FFFDBC60A36F05013B262A359`。後續格式與離線測試修改另由一般驗證涵蓋，不把重建當成重新掃描。測試前後 doctor 顯示父裝置、MI_00、MI_01 均問題碼 0、started=true。沒有改綁定、系統登錄或權限。

## WIA 取消事件與提早取消後重掃

2026-09-14 補查取消階段：僅在測試建置暫加已驗證控制回覆的 status／message 與清理前錯誤紀錄，重新列舉 MI_00 後執行同一個 ignored 提早取消測試。SET_WINDOW（0x24）及 START（0x31）都回 status=0、message=0。取消發生在 READ metadata，elapsed=630 ms、3 次 Busy、零影像塊，控制串流仍同步。ABORT（0x06）與 RELEASE（0x17）同樣回 status=0、message=0，沒有 scanner-state 訊息可解讀。730 ms 返回 S_FALSE，下一次 RGB 重掃在 RESERVE 收到 800 次 Busy，120029 ms 到期，整體 120.86 秒失敗。這證明已發出並收到成功的清理回覆，尚未證明裝置內部狀態或再次保留的時序正確。

私人日誌在 `artifacts/wia-cancel-trace-log-20260914-a/output.txt`，中斷輸出在 `artifacts/wia-cancel-trace-20260914-a/`，沒有 complete.txt。診斷版核心 SHA256 `8EECE958027E0D825A87F3AB4C66F44992B47A6ABF11F85006C560C2A25F0928`，測試執行檔 `9E8917AA9DA83E5B52FBB8A10B111280F6F7C3932012B7A4841A750F654CEFDC`。暫時紀錄已移除，沒有改動正式等待或取消流程。測試後三個 PnP 介面均問題碼 0、started=true。

同一 USB session 對照：暫改測試為三輪共用一次服務鎖，取消後不關閉／重新開啟 USB，正式掃描核心與 `85f628f` 相同。取消 700 ms 回 S_FALSE，重掃仍在 RESERVE 收到 800 次 Busy、120069 ms 到期，整體 120.86 秒失敗。關閉／重開 USB 不是重現此故障的必要條件。測試變更已撤回。私人證據在 `artifacts/wia-cancel-same-session-log-20260914-a/` 及 `artifacts/wia-cancel-same-session-20260914-a/`，沒有 complete.txt。執行檔 SHA256 `75D887C34F676F833AF7D8DC4B7474BEE0103F9531DC8CD83903295E342EB5A5`，測試來源 `7E45AA988F3901B76AAA8BF7D3B3F7A0C98CED712ABB25D7CFC62E0877360517`。前述兩次失敗之間，已撤回的 SET_WINDOW Busy 拒絕草稿跑過一般回呼取消流程，42.08 秒完成三輪傳輸，影像未另做品質驗收，證據在 `artifacts/wia-window-busy-regression-20260914-a/`。該草稿及合成測試因上游容許此 Busy 狀態而撤回，不作為正式修復證據。

2026-09-14，重新偵測三個介面均問題碼 0、started=true，每次硬體測試重新列舉唯一啟用的 MI_00 路徑，全部序列執行。新增 `actual_wia_dispatch_cancels_from_parallel_thread_and_rescans` 使用原生 Windows 項目、真實 USB／HGLOBAL IStream 與合成服務 helper／屬性／回呼。另一執行緒只通知純 Rust 取消狀態，不跨執行緒呼叫未 marshal 的 COM。服務派送 WIA_EVENT_CANCEL_IO 本身仍未實測。

**新驗收失敗，未建立完成標記**：

| 情境 | 結果 |
| --- | --- |
| Gray75 完成後，下一張 RGB75 於註冊工作 500 ms 後取消 | 兩次約 610 ms 回 E_FAIL、零影像。第二次原始診斷為 RESERVE（0x16）收到 4 次 Busy（0x08）後取消，尚未確認保留權，維持隔離 |
| 閒置裝置開始 RGB75，500 ms 後取消，再立即重掃 | 取消 720 ms 回 S_FALSE、零影像、僅初始 0% 訊息；重掃約 120 秒 E_FAIL，整體 120.83 秒。第一次沒有記下原始錯誤階段 |
| 補入錯誤診斷後重現閒置情境 | 取消 600 ms 回 S_FALSE；重掃在 RESERVE 收到 800 次 Busy，status=0x08、state=unknown、120049 ms 到期。整體 120.75 秒，未進入灰階對照輪 |

這些結果不能解讀為 USB 斷線或已修復暖機取消。成功取消的精確階段及 ABORT／RELEASE 回覆未保留，下一步須取得這些資料，區分裝置端取消時序與保留權狀態。未確認所有權前不盲送 ABORT，沒有套用固定重掃延遲、延長期限、USB reset 或 CLEAR_HALT。

兩次閒置情境之間，既有 `actual_wia_dispatch_scans_cancels_and_rescans` 回歸通過 42.99 秒：Gray75 完成、RGB75 第一塊回呼取消、RGB75 重掃成功，各輪診斷與鎖定清理完成。灰階／彩色為 600×801，BMP 分別 481678／1441854 bytes。回呼取消留下 919854 bytes，不以 BM 開頭且沒有 100% 訊息。Pillow 獨立解析標頭、灰色盤、DPI、行序與全部有效樣本一致，實際查看無損 PNG 為白色空平台與少量細點。沒有原稿品質或精確幾何驗收。

私人證據集中在 `artifacts/wia-cancel-validation-20260914-a/`：`async-scan`／`async-scan-b` 為先灰階的兩次失敗，`idle-async-scan`／`idle-async-diagnostic` 為閒置取消與重掃失敗，只有 `callback-regression` 有 complete.txt 與 verification.json。回歸灰階 SHA256：`d1f8ac6c7e4c0c40654a9be20dfe776f2d5991d95555e2cc1e567ec6bd121bec`，彩色：`142271466cc4a9ed21299080025c249d972b86a8220bc21bad4b38af8a4135fa`。掃描核心 SHA256：`807D9CD7D2DA89311D61AE7A9C95A7BBF79B070CED4973BED79E17C1106B23DA`。

最後 doctor 三個介面正常，INQUIRY 解析度、範圍及行序與既有回報相符。此查詢不證明掃描馬達已恢復。沒有重插、驅動配對、COM／WIA 登錄或權限變更。

## WIA 鎖定與 dispatch 實機驗證

2026-09-14，重新偵測三個介面均問題碼 0、started=true，列舉唯一啟用的 MI_00 專案介面。新 `actual_wia_dispatch_scans_cancels_and_rescans` 以 `--ignored --exact` 序列執行，42.21 秒通過。使用真正 Windows 根／平台項目，透過合成 IStiDevice helper 轉送到 IStiUSD 鎖定；acquire dispatch 接受合成屬性快照，回呼提供真正 Windows HGLOBAL IStream，USB 為實機。沒有偽造 WIA property context，也沒有登錄服務。

三輪各自鎖定、查詢、傳輸及解鎖，依序為 Gray75、RGB75 第一塊後取消、RGB75 重掃。選取 600×800、75 dpi、中性明暗；灰階與彩色完成影像皆 600×801，BMP 分別 481678／1441854 bytes；取消留下 919854 bytes，沒有 BM 完成簽名或 100% 回報。各次原生回呼重入解除初始化、解鎖及巢狀 acquire 均回 Busy，清理後 INQUIRY 成功。callback／IStream 參考回到測試保留的一份，完成後釋放項目及 USB。

Pillow 獨立解析標頭、色盤、DPI、行序與全部有效樣本，與 BMP 位元組逐樣本一致。實際查看灰階／彩色無損 PNG，仍為白色空平台與少量細點，不是偏白或文件品質驗收。高度多一列的既有差異仍未修正。

私人影像與完成標記位於 `artifacts/wia-lock-validation-20260914-a/dispatch-scan/`。灰階 SHA256：`8ba4f4eef56ce98ba0f0df7fe124b91f6dc7031745568529b357488172597dc9`；彩色：`567e689f683f0cbe0abe9552229818a8a7c8d8b29d1711848ead45fe580bf8ec`。當次掃描測試執行檔 SHA256：`64F6BEA0A1EF9B65A04A1D78F606BA610725F524397421F021B45D3FFB14CD86`。後續只調整格式、測試及鎖定非預期正 HRESULT 的錯誤分類，另重跑鎖定／INQUIRY 測試通過（0.07 秒），未把重建執行檔當成重跑影像掃描。

最後 doctor 三個介面正常，INQUIRY 回覆六種 75–600 dpi，10200×14040 裝置單位、行序 1，與先前一致。沒有重插、USB reset、驅動配對、COM／WIA 登錄或權限變更。真正服務鎖、屬性 context、串流消費及等待期間的 WIA_EVENT_CANCEL_IO 仍待驗證。

## 原生 WIA callback 實機傳輸

2026-09-14，重新偵測父裝置、MI_00、MI_01 均問題碼 0、started=true，再列舉唯一啟用的專案介面。`actual_callback_transfer_scans_cancels_and_rescans` 明確以 `--ignored --exact` 執行通過（42.29 秒）。同一物件只鎖定一次，透過原生 QI／GetNextStream 取得 Windows HGLOBAL 串流，依序完成 Gray75、RGB75 第一塊後由 SendMessage 回 S_FALSE 取消、RGB75 重掃。callback 是測試物件，USB 與 IStream 是真實資源，沒有登錄 WIA。

選取區 600×800、75 dpi、零偏移、中性明暗。灰階輸出 600×801／1 塊／481678 BMP bytes；取消留下 919854 bytes 且沒有 BM 完成簽名；彩色重掃 600×801／2 塊／1441854 BMP bytes。灰階進度為 0→99→100，彩色重掃為 0→63→99→100，回報 bytes 與串流長度一致，取消沒有回報 100。所有訊息均為 flags=0 的 STATUS，沒有自行發送結束通知。

QI、GetNextStream、SendMessage 與 Release 內重入 GetLastError 均返回，再次掃描或解鎖回報忙碌，巢狀輸出保持空白。每次清理後同一 session 的 INQUIRY 診斷成功。callback 參考與 HGLOBAL 的實際原生參考計數都回到測試保留的一個參考。最後解鎖並釋放 helper，沒有重插、USB reset 或系統變更。

Pillow 逐樣本核對 BMP 標頭、色盤、行序、DPI 及解碼數值，實際查看灰階與彩色無損 PNG，仍是白色空平台與少量細點。沒有 USB 原文對照或文件品質驗收，選取高度多一列的既有差異保留。原有鎖定／診斷與 Rust 寫入介面的灰階／彩色取消／重掃測試另逐一通過，分別為 0.05／42.11 秒，沒有平行操作 USB。

私人證據位於 `artifacts/wia-callback-20260914-a/`，新回呼影像在 `scan/`，原有路徑回歸在 `legacy-scan/`，各有 complete.txt 與獨立 verification JSON。新回呼灰階 BMP SHA256：`a5f1b9b01135896e38dc0a7b6011ee9cbc92ed1aec10129c2c75510ef7b5309e`，彩色 BMP：`5c17790abbf5074ab5f2de358eb49a7e375c67fa419c53432bf5562c44d9e89b`。硬體測試執行檔 SHA256：`52C3C40F887EC52D4AEA5D181D265CABFCC93818FA5F922A030A8E43954B3776`。callback 原始碼 SHA256：`B9E9B280F84ABDEBD7FCDA03202E5F41BA5DCB04AB21DC9C37CA257A4CF7A4ED`。

## 共用連線實掃與提早取消修正

2026-09-14，重新列舉唯一啟用的 MI_00 WinUSB 介面，以當下路徑執行 `tests/sti.rs` 兩個 ignored 硬體測試，逐一指定 `--ignored --exact`，沒有平行操作 USB。版本 `705f0d6` 的鎖定／診斷測試通過（0.05 秒），同一物件的灰階／彩色取消／重掃測試通過（42.15 秒）。核心獨立複核另發現 RESERVE 前取消會錯誤隔離，此精確時機用合成回覆先重現再修正。修正版重新建置後，兩個實機測試再次通過，依序為 0.05 與 42.12 秒。

掃描測試只初始化與鎖定物件一次，依序執行 Gray75、RGB75 第一個影像寫入 callback 取消、RGB75 重掃。每個工作後都由仍持有的同一 session 完成 INQUIRY 診斷。輸出 callback 內的 GetLastError 正常返回，解鎖及巢狀掃描即時回報忙碌，巢狀掃描沒有觸碰輸出。取消回傳 Interrupted，輸出沒有完成的 BM 簽名。最後解鎖、釋放物件，helper 參考回到原始值。

修正版實際輸出如下。設定為零偏移、75 dpi、600×800 像素選取區、亮度／對比 0、無壓縮。

| 輸出 | 實際尺寸 | 塊數 | 像素 bytes | 檔案 bytes |
| --- | --- | --- | --- | --- |
| Gray75 BMP | 600×801 | 1 | 480600 | 481678 |
| RGB75 取消資料 | 未完成 | 未作成功計數 | 未作成功計數 | 919854 |
| RGB75 重掃 BMP | 600×801 | 2 | 1441800 | 1441854 |

Pillow 獨立檢查標頭、負高度、75 dpi、色盤、長度與全部 BMP 有效樣本解碼一致。Windows GDI+ 能開啟兩張 BMP，尺寸及中央樣本相符。實際檢視無損 PNG，影像為白色空平台與少量細點。比選取高度多 1 列的既有差異保留，沒有裁切、縮放或明暗轉換。測試沒有保存 USB 原文，不能宣稱本次又完成 wire 全樣本對照，也沒有驗收文件品質或 600 dpi 穩定性。

修正版測試執行檔 SHA256：`BEB38E96A655C3B7512C50AC8A2CCB57D15899814EE4B502772013EB85E598F6`，核心 `src/scan.rs`：`DB43C54B89CC4FC1A8343FA3F8378CCCCCE22843A5430611B5017566DBA784B1`。同批 DLL：`0C1C5D7117DEB79FF69692F7A5AB578122D5149E2399BA525746A17D03498042`，DLL 載入測試另行通過；掃描透過 Rust 函式庫及測試 helper，未經 WIA 服務。

修正版灰階 BMP SHA256：`9ab5abfcc1404673ef8f6437c08e00c0e954c35fdf5c9f0b04708d92c1bbdd16`，彩色 BMP：`3945c0ef0a0dde560d0bd2ece135dd967be35f5c369d0726a6dcd3540d9d6baf`。私人檔案為 `artifacts/shared-session-final-20260914-a/`，包含 `complete.txt`。測試紀錄與獨立驗證程式／結果在 `artifacts/shared-session-proof-20260914-a/`。修正前影像為 `artifacts/shared-session-20260914-a/`，與修正版分開保存。

測試後 release `inquiry` 成功，機器回報 SAMSUNG ORION 與原能力值。`doctor` 的父裝置、MI_00、MI_01 問題碼均為 0、started=true。沒有重插、USB reset、重新配對、COM／WIA 登錄或安全設定變更。WIA 服務端身分映射、LocalService 存取、Windows 掃描及失同步後跨物件隔離仍待完成。

## IStiUSD 實機鎖定與能力診斷

2026-09-13，重新以 PnP 列舉唯一啟用的專案 MI_00 介面，把當下路徑交給測試 helper，明確執行 release 的 `tests/sti.rs` ignored 測試，1 個通過（0.05 秒）。指定不存在的合成路徑時鎖定失敗，沒有改用已連接裝置。第一個物件成功鎖定後，第二個物件無法取得同一裝置；解鎖後第二個物件可取得並完成 INQUIRY。第二個物件在鎖定中最終 Release 後，第一個物件可重新取得。helper 最後保留原始的一個參考。

測試後 release `inquiry` 回報 SAMSUNG ORION、六種 75–600 dpi 解析度及原能力值，`doctor` 的父裝置／MI_00／MI_01 問題碼皆為 0。這次沒有啟動掃描、重設 USB、修改綁定或登錄。測試 helper 由測試程序提供，不能當成 WIA 服務實際裝置發現或 LocalService 存取權證據。

私人紀錄為 `artifacts/sti-sdk-20260913-j/hardware-test.log`。當次 STI 原始碼 SHA256：`5135711E1C68BC1CAA73619A572FCBC8F95EAD24B5C02965A441C02ED098E68A`。同批 release DLL SHA256：`59DA7C98A26DC1D0E5D28007804E769A1D1112824FD0E9A83952C8EB5FC96088`；DLL 的獨立動態測試另通過 IUnknown／IStiUSD 身分與生命週期，實機測試透過同版 Rust 函式庫執行，未經 WIA 服務載入。

## 系統與工具

- Windows 11 專業版 x64，版本 `10.0.26200`。
- Rust `1.97.0`、Cargo `1.97.0`，`stable-x86_64-pc-windows-msvc`。
- Windows SDK `10.0.26100.0` 已存在。SDK 內找到 signtool，沒有找到 InfVerif / Inf2Cat，因此沒有完成 WDK 套件驗證。
- `C:\Windows\System32\drivers\winusb.sys` 的 Authenticode 檢查為 Valid，簽署者 Microsoft Windows。這不是自訂 INF 已簽署的證據。

## 配對前的 Xerox USB 裝置

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

2026-09-13 開發機配對預檢：裝置專屬與全域 CLASS 清單各 3 個候選，均只有 1 個精確符合內建 WinUSB；其餘 BILLBOARD／ADB 均被拒絕。MI_00 前後維持未綁定、問題碼 28，父裝置與 MI_01 屬性符合原始備份。這次沒有執行安裝路徑。

加入能力查詢後，2026-09-13 再次以 release 執行 `doctor`，三個介面的服務、問題碼及啟動狀態與上表相同。`wc3119 inquiry` 回報 `Scanner WinUSB interface is unavailable; MI_00 must be paired and its interface GUID registered before inquiry`，結束碼 1。此次失敗發生在已登錄裝置介面的列舉階段，未開啟 USB 或送出 INQUIRY。

以上為配對前紀錄。當時沒有取得 USB 通訊或影像證據。

## 授權配對後的最新結果

2026-09-13 經使用者授權及 UAC，只對已備份的完整 MI_00 實例綁定內建 WinUSB、登錄專案 GUID 及重新啟動介面。安裝與介面重啟結束碼均為 0，無需重開機。原生協調腳本核對父裝置／MI_01 的服務、INF、問題碼、ClassGuid、Parent 均與備份一致。

| 目標 | 服務 | 問題碼 | 已啟動 |
| --- | --- | --- | --- |
| 父裝置 | usbccgp | 0 | 是 |
| MI_00 | WINUSB | 0 | 是 |
| MI_01 | usbprint | 0 | 是 |

一般權限執行 release `doctor` 與 `inquiry` 皆回傳 0。`inquiry` 已讀取並核對 USB 裝置／介面描述與唯一 bulk IN／OUT，再收到有效能力回覆：

| 回覆欄位 | 真實回報值 |
| --- | --- |
| 機器識別 | SAMSUNG ORION |
| 已辨識解析度 | 75、100、150、200、300、600 dpi |
| 解析度／模式旗標 | 0x00353f／0x29 |
| 寬／最大長／平台長 | 10200／14040／14040，單位 1/1200 英吋 |
| 行序／壓縮旗標 | 0x01／0x2f |

當時只有 INQUIRY；後續影像與端點證據見下節。回報解析度不等於光學解析度驗證。尚無 WIA、列印佇列、安裝復原、換孔或跨電腦驗證。未變更系統安全設定。

## 連續掃描中重現的自然失敗

2026-09-13，使用 `scan_stability` 同一程序預定執行 20 次空平台掃描。此工具逐塊核對 USB／解碼像素但不保存影像，任何錯誤立即停止。實際完成 4 次，第 5 次 600 dpi 彩色逾時，所以沒有 `complete.txt`，20 次驗收未通過。

| 次序 | 模式 | 秒數 | 結果 |
| --- | --- | --- | --- |
| 1 | RGB600 | 90.279 | 完整成功，117 塊，106503300 bytes |
| 2 | RGB600 | 101.734 | 完整成功，117 塊，106503300 bytes |
| 3 | RGB300 | 40.030 | 完整成功，29 塊，26653968 bytes |
| 4 | Gray600 | 42.208 | 完整成功，39 塊，35490900 bytes |
| 5 | RGB600 | 120.201 | 在等下一塊資料描述時達到工作期限；已交付 54 塊、49572000 bytes |

第 5 次最後一塊在 56.208 秒收到，接著約 64 秒沒有新影像。錯誤為 `stage=read-metadata ... Scan exceeded 120 second deadline`，沒有 USB 讀寫錯誤或清理失敗。錯誤發生在 `ready` 等候路徑；當時沒有記錄 Busy 回覆次數及最後狀態欄位，不能進一步歸因於暖機、硬體或驅動。清理後直接 Gray75 重掃成功（648×871，7.113 秒），不需重插；`doctor` 三個介面問題碼仍為 0。不能把這次自然逾時與先前刻意取消混為一談，也不能認定已找到歷史官方驅動故障的同一根因。

外部 PowerShell 每 500 ms 觀察同一程序，取得 769 個記憶體樣本。實際占用 RAM 峰值 7745536 bytes、程序私有配置峰值 2957312 bytes；本輪沒有觀察到記憶體暴增，但中途停止，不能宣稱長期無洩漏。證據在 `artifacts/stability-observation-20260913-c/`（`scans/diagnostics.log`、stdout／stderr、`memory.csv`、`analysis.json`），復原掃描在 `artifacts/scan-after-stability-timeout-20260913-c/`。測試執行檔 SHA256：`3FE5CE96D262EAEE1E7FE6971F5B587FA3B6C045B825CFE501AAAADAB18F1BDF`。

### 加入 Busy 診斷後再次重現

同日以新版本在同一程序預定執行 2 次 RGB600；第一個工作在 18.383 秒交付第 20 塊後停住，120.183 秒結束，第二次未啟動。已交付 18360000 bytes；READ（0x28）在該次等待收到 678 次有效 Busy 回覆，最後 status=0x08，state=unknown。這證明等候期間仍有控制回覆，沒有 USB API 讀寫錯誤；0x08 沒有足以辨識暖機或其他內部狀態的欄位，不能由此判定硬體損壞或驅動時序正確。

ABORT／RELEASE 沒有回報失敗，隨後 Gray75 又完整成功（648×871，7.221 秒），沒有重插。`doctor` 三個介面仍為問題碼 0、started=true。證據目錄為 `artifacts/stability-busy-observed-20260913-c/` 與 `artifacts/scan-after-busy-timeout-20260913-c/`；穩定性工具 SHA256 `3C600881525979BD72C2F45E84F1DE4579FBE649DCE01C9D61FA38D01C842FD5`。本次只增加診斷，沒有調整 100 ms 輪詢間隔、120 秒工作期限或 USB 傳輸政策；兩次自然失敗的根因仍未確認。

## 效能分段與 READ 詢問間隔對照

2026-09-13，新增開發 profile 後，保持全平台 RGB600、117 塊、5100×6961、106503300 像素 bytes 與 120 秒期限，以相同 release 執行檔比較 READ Busy 間隔。其他命令維持 100 ms，沒有重新配對或修改 USB 政策。平台沒有文件，工具逐塊核對每次 USB 資料與解碼像素，不保存影像。

| 順序／READ 間隔 | 工作秒數 | 影像讀取秒數 | 資料描述秒數 | 解碼毫秒 | 呼叫端毫秒 | 全工作 Busy 次數／睡眠秒數 |
| --- | --- | --- | --- | --- | --- | --- |
| 1／100 ms | 92.845 | 72.786 | 19.784 | 39.204 | 47.822 | 97／9.731 |
| 2／500 ms | 106.177 | 72.138 | 29.565 | 38.502 | 48.780 | 72／24.825 |
| 3／500 ms | 102.656 | 72.255 | 30.076 | 45.028 | 54.683 | 45／22.515 |
| 4／100 ms | 95.385 | 72.613 | 22.503 | 41.257 | 50.563 | 115／11.536 |

以上四次均成功且有 `complete.txt`。順序為 100、500、500、100 ms，每次為獨立程序，沒有控制機器內部暖機狀態。第二次 RESERVE 花 3.731 秒，其他約 0.030 秒，因此不能把整體時間差全部歸因於 READ 間隔。兩次 500 ms 均較慢，沒有支持改動預設的證據。解碼及呼叫端每次合計不到 0.1 秒，沒有支持優先平行化影像處理的證據。USB 呼叫包含等待裝置資料，且與階段時間重疊，不能把它當成純頻寬，也不能由主機 profile 分離機械運動與曝光耗時。

實機證據在 `artifacts/profile-read100-20260913-e/`、`artifacts/profile-read500-20260913-e/`、`artifacts/profile-read500-reverse-20260913-e/` 與 `artifacts/profile-read100-reverse-20260913-e/`，彙整於 `artifacts/profile-comparison-20260913-e.json`。測試 `scan_stability.exe` SHA256 為 `7E56EF81CEA7FB688ED0B1D4440F6F89838402394DA168D94E40002FA1F26981`，其來源後續僅做格式整理。測試後 `doctor` 父裝置、MI_00、MI_01 問題碼皆為 0、started=true。這四次沒有重現故障，不能取代同一程序 20 次驗收，也沒有高解析度故障修復或跨機型加速的證據。

## WIA 裝置列舉

2026-09-13，在目前 WinUSB 配對狀態下，PowerShell 建立 `WIA.DeviceManager` automation COM 物件，讀取 `DeviceInfos.Count` 為 0，查詢後釋放 COM 物件。這是 WIA automation 的唯讀觀察，尚未透過 Rust `IWiaDevMgr2` 或 Windows 掃描執行影像傳輸。沒有登錄自訂 COM 驅動、安裝套件或手動變更 WIA 服務。此前 `doctor` 的父裝置、MI_00、MI_01 均為問題碼 0、started=true。

## BMP 串流實機驗證

2026-09-13，開發擷取範例將同一次掃描的影像塊送入 Rust `BmpEncoder`，同步保留 USB 原文、解碼像素及 PGM／PPM 作獨立核對。平台仍沒有文件。

| 測試 | 實際像素 | 塊數 | 像素 bytes | 秒數 | 結果 |
| --- | --- | --- | --- | --- | --- |
| Gray75 | 648×871 | 1 | 564408 | 7.246 | BMP 及完成標記成功 |
| RGB300 | 2556×3476 | 29 | 26653968 | 29.931 | BMP 及完成標記成功，含尾塊 116 列 |
| RGB75 第一塊後取消 | 648×474 部分資料 | 1 | 921456 | 不作效能量測 | 結束碼 1，BMP 未完成，沒有完成標記 |
| 取消後最終版 Gray75 | 648×871 | 1 | 564408 | 7.200 | 未重插即重掃成功 |

以獨立 Python/Pillow 核對三張成功影像的全部 USB 有效樣本、解碼像素、BMP 解碼結果及 PGM／PPM 完全相同。另檢查標頭尺寸、負高度、每列四位元組填補及填補值為零。Windows GDI+ 成功開啟前兩張 BMP，核對尺寸及各九個位置的通道值相同；這不是 Windows 掃描的 WIA 傳輸驗收。最終灰階及 RGB300 另轉成 PNG 實際檢視，為白色空平台與少量細點，不能驗收文件明暗、色彩或實體上下方向。

前面三次使用 `capture_scan.exe` SHA256 `1B07D99533302B7A15A69B64209D94A83EB13243C208F2BADA26F8ADD53B58A9`；取消後重掃使用最終版 `A76A88E7AB32F2B998C6DEE48C5111C762330D373C04F923485D37B0B82D8E9E`。兩個建置的實機證據分開保留，不把重新建置當成重跑前三次測試。最終 `src/bitmap.rs` SHA256 為 `B61D1232AAA574215C3514EEE3FA330205844FAF27629F1D004BF1D4C875FF4E`。

私人證據依序存於 `artifacts/bmp-gray75-20260913-g/`、`artifacts/bmp-rgb300-20260913-g/`、`artifacts/bmp-cancel-rgb75-20260913-g/`、`artifacts/bmp-final-after-cancel-gray75-20260913-g/`，獨立核對程式為 `artifacts/verify-bitmap-20260913-g.py`。實機取消時已寫出部分 BMP，但簽名未完成，`complete.txt` 不存在。任何 API 錯誤仍須棄置結果，不能用簽名判斷是否成功。

測試後 `doctor` 父裝置、MI_00、MI_01 問題碼皆為 0、started=true。沒有重新安裝、實體重插或修改系統設定。這次沒有重跑 600 dpi 穩定性驗收，也沒有修復此前兩次自然逾時。

## WIA 數值設定與原生串流實掃

2026-09-13，私人測試呼叫端使用 `FlatbedSettings` → `scan_bmp` → `ComOutputStream`，將真實 USB 掃描寫入 Windows `CreateStreamOnHGlobal` 物件，成功釋放掃描工作後從同一物件讀回 BMP。兩次皆為 75 dpi、零偏移、600×800 像素選取範圍，精確對應 9600×12800 個 1/1200 英吋單位。亮度／對比為 0、無壓縮。平台沒有文件。

| 模式 | 實際尺寸 | 塊數 | 像素 bytes | BMP bytes | 掃描秒數 |
| --- | --- | --- | --- | --- | --- |
| 灰階 8-bit | 600×801 | 1 | 480600 | 481678 | 6.568 |
| RGB 24-bit | 600×801 | 2 | 1441800 | 1441854 | 13.263 |

兩次均完成並建立 `complete.txt`。實際比選取高度多 1 列，BMP 保留 READ 尺寸，沒有裁切或縮放，差異原因與正式 WIA 幾何尚未確認。Python/Pillow 核對 BMP 標頭、負高度、色盤、長度與全部 BMP 有效樣本的解碼一致。Windows GDI+ 開啟兩張成功，尺寸與各九個樣本符合 Pillow。實際檢視轉存 PNG，為白色空平台與少量細點。此測試沒有保存 USB 原文，不宣稱這兩次重新完成 USB 全樣本對照或文件品質驗收。

私人測試來源為 `artifacts/wia-scalar-smoke-20260913-h/smoke.rs`，SHA256 `E98BAD1EE48EE076FED01892E76628D7A0FC810F7B8D9B2989CD0FD8A82A4F86`。執行檔 SHA256 `246F6B483071AEADAC2C3279A0DAEEA89371588DDD9AB5ABFED6DCE8DC87FBA0`，`src/wia.rs` 為 `1AAD9DE923CEA3ABFB4463C032D2102FFB4474C59ED37E08BDCE08618001E33A`。設定、結果與 BMP 分別留在 `artifacts/wia-scalar-gray75-20260913-h/`、`artifacts/wia-scalar-rgb75-20260913-h/`。驗證程式及結果為 harness 目錄的 `verify.py`、`verification.json`。

灰階 BMP SHA256 `fa5d379c10c220158803860f8dce7f07a4bbae6fe9c8fe10111b46ba5296c9f5`，彩色為 `109493d52027f53af015d440ed4d9e9179e2ed6abbdfad76c17abafcf3d7ced8`。前後 `doctor` 三個介面均為問題碼 0、started=true。COM 在呼叫端執行緒初始化並隨串流釋放後解除，沒有 WIA 登錄、重新配對或系統設定變更。這是 WIA 前置掃描入口測試，沒有執行 Windows 掃描或修復 600 dpi 自然逾時。

## 64 KiB 與 256 KiB 影像讀取對照

2026-09-13，以同一個 release 執行檔各掃描一次全平台 RGB600，READ Busy 間隔固定 100 ms。WinUSB bulk IN 當下回報 `MAXIMUM_TRANSFER_SIZE=2097152` bytes，端點封包為 512 bytes。兩次都是 5100×6961、117 塊、106503300 像素 bytes，逐塊獨立核對 wire／pixels 成功，並產生 `complete.txt`。

| 讀取緩衝區 | 工作秒數 | 影像讀取秒數 | 資料描述秒數 | 全工作 USB 讀取呼叫 | Busy 次數 |
| --- | --- | --- | --- | --- | --- |
| 64 KiB | 97.010 | 72.549 | 24.153 | 1989 | 126 |
| 256 KiB | 93.736 | 72.784 | 20.658 | 689 | 102 |

較大緩衝區減少呼叫次數，但影像讀取階段沒有縮短。整體時間差主要出現在資料描述等候，只有一組比較，不能歸因於緩衝區或宣稱加速。維持 64 KiB 預設。兩次沒有重現自然故障，也不能取代同程序 20 次穩定性驗收。平台沒有文件，未驗收影像品質。

證據為 `artifacts/buffer64-20260913-f/` 與 `artifacts/buffer256-20260913-f/` 的 `diagnostics.log`、`complete.txt`。測試 `scan_stability.exe` SHA256：`1A22C30348B549318670F445CE03DB4B5348369F4816637786A476EC97E4DF88`。其後只補正傳輸上限回覆的非法封包檢查、共用公開掃描入口的上限預檢及拒絕設定的診斷，沒有改動資料讀取迴圈。

最終版本另經 `capture_scan` 完成 Gray75（648×871，564408 bytes，7.228 秒），保存於 `artifacts/buffer-final-gray75-20260913-f/`。PowerShell 獨立逐值核對全部 wire 有效樣本、像素及 PGM 相同，完成標記存在。實際檢視為空平台影像，不能用來驗收明暗品質。該擷取執行檔 SHA256 為 `50CC0C7F3F927775AD7FA2CB01C9BE022D4C974C50C849966B5D746B60E4517D`。測試後 `doctor` 父裝置、MI_00、MI_01 問題碼皆為 0、started=true。沒有重新安裝、重新插拔或修改系統設定。

## 首次實機影像傳輸

### 最新補充：600 dpi 彩色

定時取消驗證：`artifacts/scan-early-cancel-20260913-b` 設定 100 ms，結束碼 1，耗時約 0.38 秒；`scan-active-cancel-20260913-b` 設定 8000 ms、600RGB，收到 4 塊後以取消結束，總耗時約 8.71 秒。兩次都沒有 `complete.txt`，錯誤沒有清理失敗訊息。隨後 `scan-after-timed-cancel-20260913-b` 以 Gray75 得到 648 × 871 並正常完成，不需重插。定時器設定記於 `evidence.txt`；取消當下的精確 USB 階段未另追蹤。

2026-09-13 後續執行 `capture_scan artifacts/scan-rgb600-20260913-b rgb 600` 成功，回傳 5100 × 6961、117 塊、106503300 bytes，耗時 94.951 秒並正常釋放。逐塊移除 16-byte 尾端填補後，以獨立 Python/Pillow 核對平面 RGB 與 PPM 的所有像素相符。PPM SHA256：`d8746ed2e27214d64d1de203c37683e19727cf5ad86b25c775386e0d24fbee4b`，本機 `verification.json` 保留結果。這仍是空平台，不代表色彩或幾何品質已驗收。

此後灰階及彩色皆已驗證到 600 dpi。官方規格另標示光學 600 × 2400 dpi、插值 4800 dpi，與當前協定回報及相同 X／Y 設定的上限不同；非對稱設定尚未查明，見 [02](tickets/02-first-scan.md)。下方為較早的首次掃描紀錄。

2026-09-13，以一般權限執行原創 Rust `capture_scan` 範例，使用者確認平台沒有文件。以下皆為空平台，不能用來驗收文字、色彩、尺寸準確度或歷史偏白症狀。

端點由每次開啟時重新查詢：bulk IN `0x84`、OUT `0x03`，maximum packet 皆 512。裝置與介面描述如下，不含序號字串：

```text
device: 12 01 00 02 00 00 00 40 24 09 65 42 00 01 01 02 03 01
interface: 09 04 00 00 02 ff ff ff 00
INQUIRY (70 bytes):
a8 00 43 10 53 41 4d 53 55 4e 47 20 4f 52 49 4f
4e 20 20 20 20 20 20 20 20 20 20 20 20 20 20 20
20 20 20 20 35 3f 04 29 00 00 27 d8 00 00 36 d8
00 01 2f 00 00 02 00 00 00 00 36 d8 00 00 36 d8
05 02 05 05 00 00
```

全平台設定為零偏移、10200 × 14040（1/1200 英吋）、無壓縮。以下為 READ 回報並完整接收的尺寸，未裁切、縮放或修正明暗：

| 測試 | 實際像素 | 分塊 | 像素 bytes | 秒數 | 結果 |
| --- | --- | --- | --- | --- | --- |
| Gray 75 dpi | 648 × 871 | 1 | 564408 | 7.315 | 成功並釋放 |
| RGB 75 dpi | 648 × 871 | 2 | 1693224 | 14.608 | 成功並釋放 |
| RGB 75 dpi 第一塊後取消 | 648 × 474 部分資料 | 1 | 不標示完成 | 不作效能量測 | 回傳 Interrupted，ABORT／RELEASE 成功，無 complete.txt |
| 取消後 Gray 75 dpi | 648 × 871 | 1 | 564408 | 7.133 | 未重插即重新掃描成功 |
| RGB 300 dpi | 2556 × 3476 | 29 | 26653968 | 29.611 | 成功並釋放 |
| Gray 600 dpi | 5100 × 6959 | 39 | 35490900 | 31.757 | 成功並釋放 |

每塊 wire 比有效像素多 16 bytes。以獨立 Python/Pillow 讀取檔案，比對全部 wire 與 PGM／PPM 像素：灰階完全相同，RGB 逐行從平面通道重排後完全相同，沒有數值轉換。無損轉存 PNG 後再次逐位元組相等；75 dpi 灰階及彩色實際開啟檢視，為白色平台背景與少量細點，沒有文件可作色彩參照。

影像 SHA256（PGM／PPM）：Gray75 `c0670537dd8a294ce70275f4ac77638f77afc7e462cf8f697c9a833dfda6ffcf`；RGB75 `4be342fa8b2d817f589e18548715646669e86710618137658edfeb0440ec8ed6`；RGB300 `ce2fc27974795aa208c3ee566d4ca89388f84f338adcec8a97e04ad79d468edb`；Gray600 `607bb5dc7d9fe5ec926c4d0454c10cab3fbe89e82d976101d9704afb2e8599b1`。

300 dpi 掃描中，同時由另一行程執行 `wc3119 inquiry` 回傳存取被拒（OS error 5），原掃描仍成功完成。掃描後 `doctor` 核對父裝置、MI_00、MI_01 服務與問題碼仍正常。沒有安裝、重新配對、reset 或 CLEAR_HALT。

本機證據位於 Git 排除的 `artifacts/scan-*-20260913-a/`，包含 `evidence.txt`、每塊 `.bin`、解碼像素、完整 PGM／PPM 與 `verification.json`。取消那次沒有完成影像或標記。該批首次測試尚未涵蓋正式幾何、100／150／200 dpi、600 dpi 彩色、暖機中取消、傳輸中斷、拔線、睡眠與 20 次連續掃描；後續結果見本文件上方。

審查修正版另使用單一 session 取得能力及影像，能力改存同目錄 `inquiry.txt`。`scan-gray75-reviewed-20260913-a` 成功，接著 `scan-rgb75-reviewed-cancel-20260913-a` 在第一塊後取消，最後 `scan-rgb75-reviewed-after-cancel-20260913-a` 完整成功。兩張完整影像再次通過獨立像素比對，SHA256 分別為 `6b794ba3e5c88ada390266d64ca012704e95a5811cc56804eab8fef9d6959f5b` 及 `88c3c6828b4f511e03be33cd8c459cca9d0845a42b0d84ab4400f46809a22b98`。
