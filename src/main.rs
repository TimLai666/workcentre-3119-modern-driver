use std::{
    io::{self, Write},
    process::ExitCode,
};
use workcentre_3119::{Interface, Readiness};

fn run() -> io::Result<u8> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut out = io::stdout().lock();
    if args.is_empty() || (args.len() == 1 && (args[0] == "--help" || args[0] == "-h")) {
        writeln!(
            out,
            "wc3119 doctor\n  唯讀檢查 Xerox WorkCentre 3119 的 USB 介面與驅動狀態。\nwc3119 inquiry\n  核對 USB 描述後查詢機器回報的能力，需要 MI_00 已配對 WinUSB。\n  本版本尚未提供掃描功能。\n\n結束碼：0 指令成功（不代表可以掃描）；2 裝置未就緒；1 檢查失敗；64 參數錯誤。"
        )?;
        return Ok(0);
    }
    if args.len() != 1 || (args[0] != "doctor" && args[0] != "inquiry") {
        writeln!(out, "不支援的參數。請執行 wc3119 --help。")?;
        return Ok(64);
    }
    if args[0] == "inquiry" {
        let caps = workcentre_3119::inquiry()?;
        writeln!(out, "裝置回報：{}", caps.identity)?;
        writeln!(out, "已辨識的回報解析度：{:?} dpi", caps.resolutions())?;
        writeln!(
            out,
            "解析度旗標：0x{:06x}；模式旗標：0x{:02x}",
            caps.resolution_mask, caps.mode_mask
        )?;
        writeln!(
            out,
            "回報範圍：寬 {}、最大長 {}、平台長 {}（單位 1/1200 英吋）",
            caps.width_units, caps.length_units, caps.flatbed_length_units
        )?;
        writeln!(
            out,
            "影像行序：0x{:02x}；壓縮旗標：0x{:02x}",
            caps.line_order, caps.compression_mask
        )?;
        writeln!(
            out,
            "以上是機器回報值，尚未驗證實際掃描、光學解析度或影像品質。未知旗標保留原值。"
        )?;
        return Ok(0);
    }
    let devices = workcentre_3119::discover()?;
    writeln!(out, "Xerox WorkCentre 3119 · USB 0924:4265")?;
    if devices.is_empty() {
        writeln!(
            out,
            "沒有找到已連線的裝置。請確認電源與 USB 連接線，再重新檢查。"
        )?;
        return Ok(2);
    }
    for device in &devices {
        let name = match device.interface {
            Interface::Composite => "複合裝置",
            Interface::Scanner => "掃描介面 MI_00",
            Interface::Printer => "列印介面 MI_01",
        };
        writeln!(
            out,
            "{name}: service={}, problem={}, started={}",
            device.service.as_deref().unwrap_or("未安裝"),
            device.problem_code,
            device.started
        )?;
    }
    let scanners: Vec<_> = devices
        .iter()
        .filter(|d| d.interface == Interface::Scanner)
        .collect();
    if scanners.len() != 1 {
        writeln!(
            out,
            "找到 {} 個掃描介面，無法判定唯一掃描目標。",
            scanners.len()
        )?;
        return Ok(2);
    }
    match scanners[0].readiness() {
        Readiness::DriverMissing => writeln!(
            out,
            "掃描介面缺少驅動（Windows 錯誤碼 28）。需要安裝掃描介面的 USB 驅動後才能繼續。"
        )?,
        Readiness::DriverStarted => {
            writeln!(
                out,
                "WinUSB 已啟動。這只確認 USB 驅動狀態，尚未驗證 USB 傳輸、掃描或 Windows 掃描整合。"
            )?;
            return Ok(0);
        }
        Readiness::OtherDriver => {
            writeln!(out, "掃描介面已使用其他驅動，請先確認相容性與復原方式。")?
        }
        Readiness::NotStarted => writeln!(out, "掃描介面尚未啟動，請檢查裝置管理員中的狀態。")?,
        Readiness::DeviceProblem(code) => writeln!(
            out,
            "掃描介面回報 Windows 錯誤碼 {code}，需要先處理裝置錯誤。"
        )?,
        Readiness::NotScanner => return Err(io::Error::other("Unexpected non-scanner selection")),
    }
    Ok(2)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            let _ = writeln!(io::stderr(), "檢查失敗：{e}");
            ExitCode::FAILURE
        }
    }
}
