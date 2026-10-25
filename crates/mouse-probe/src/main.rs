//! Hardware probe: `mouse-probe` prints device info, `mouse-probe dpi <value>` sets DPI.
//! `mouse-probe keyboard [...]` does the same for a Logitech keyboard.

use anyhow::{Context, Result};
use hidpp::{feature, Device, Kind};

fn main() -> Result<()> {
    let keyboard = std::env::args().nth(1).as_deref() == Some("keyboard");
    let kind = if keyboard { Kind::Keyboard } else { Kind::Mouse };
    let mut mouse = Device::open(kind).context("opening device")?;
    let (major, minor) = mouse.ping()?;
    println!("Interface : {}", mouse.product);
    println!("Index     : 0x{:02X}", mouse.device_index());
    println!("HID++     : {major}.{minor}");

    let args: Vec<String> = std::env::args().skip(if keyboard { 2 } else { 1 }).collect();
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

    if keyboard {
        match mouse.rgb_zones() {
            Ok(zones) => {
                for z in zones {
                    let ids: Vec<String> = z.effects.iter().map(|e| format!("{:02X}", e.id)).collect();
                    println!("Zone {}    : location {} effects {}", z.index, z.location, ids.join(" "));
                }
            }
            Err(e) => println!("Zones     : <{e}>"),
        }
        report("Per-key", mouse.per_key_zones().map(|z| format!("{} keys", z.len())));
    }

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
        feature::WIRELESS_STATUS => "  wireless status",
        feature::RGB_EFFECTS => "  RGB effects",
        feature::PER_KEY_LIGHTING_V2 => "  per-key lighting",
        _ => "",
    }
}
