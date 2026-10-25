//! Hardware probe: `mouse-probe` prints device info, `mouse-probe dpi <value>` sets DPI.

use anyhow::{Context, Result};
use hidpp::{feature, Mouse};

fn main() -> Result<()> {
    let mut mouse = Mouse::open().context("opening mouse")?;
    let (major, minor) = mouse.ping()?;
    println!("Interface : {}", mouse.product);
    println!("Index     : 0x{:02X}", mouse.device_index());
    println!("HID++     : {major}.{minor}");

    let args: Vec<String> = std::env::args().skip(1).collect();
    // `mouse-probe raw <feature id hex> <function> [param bytes hex...]`
    if args.first().map(String::as_str) == Some("raw") && args.len() >= 3 {
        let id = u16::from_str_radix(&args[1], 16)?;
        let function: u8 = args[2].parse()?;
        let params: Vec<u8> = args[3..].iter().map(|b| u8::from_str_radix(b, 16)).collect::<Result<_, _>>()?;
        let index = mouse.feature_index(id)?;
        let reply = mouse.request(index, function, &params)?;
        println!("Reply     : {}", reply.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "));
        return Ok(());
    }
    if let [cmd, value] = args.as_slice() {
        if cmd == "dpi" {
            let dpi: u16 = value.parse().context("DPI must be a number")?;
            println!("Set DPI   : {}", mouse.set_dpi(dpi)?);
            return Ok(());
        }
        if cmd == "rate" {
            let hz: u16 = value.parse().context("rate must be a number")?;
            mouse.ensure_host_mode()?;
            mouse.set_report_rate(hz)?;
            println!("Rate (Hz) : {}", mouse.report_rate()?);
            return Ok(());
        }
    }

    report("Name", mouse.name());
    report("Battery", mouse.battery().map(|(pct, chg)| {
        format!("{pct}%{}", if chg { " (charging)" } else { "" })
    }));
    report("DPI", mouse.dpi());
    report("Rate (Hz)", mouse.report_rate());

    println!("Features  :");
    for (index, id) in mouse.features()? {
        println!("  [{index:2}] 0x{id:04X}{}", label(id));
    }
    Ok(())
}

fn report<T: std::fmt::Display>(what: &str, r: hidpp::Result<T>) {
    match r {
        Ok(v) => println!("{what:<10}: {v}"),
        Err(e) => println!("{what:<10}: <{e}>"),
    }
}

fn label(id: u16) -> &'static str {
    match id {
        feature::ROOT => "  root",
        feature::FEATURE_SET => "  feature set",
        feature::DEVICE_NAME => "  device name",
        feature::BATTERY_STATUS => "  battery status",
        feature::UNIFIED_BATTERY => "  unified battery",
        feature::ADJUSTABLE_DPI => "  adjustable DPI",
        feature::REPORT_RATE => "  report rate",
        feature::ONBOARD_PROFILES => "  onboard profiles",
        _ => "",
    }
}
