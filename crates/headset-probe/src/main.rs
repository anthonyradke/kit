//! Hardware probe for the Arctis Nova 5.
//!
//! - `headset-probe`: status and EQ
//! - `headset-probe list`: SteelSeries HID interfaces
//! - `headset-probe raw <hex bytes...>`: send one report and dump every reply
//! - `headset-probe devices`: audio endpoints and defaults
//! - `headset-probe default <endpoint id>`: make an endpoint the default
//! - `headset-probe install-apo <dll>` / `headset-probe uninstall-apo` (elevated)
//! - `headset-probe mic <0|1> <strength%>`: noise suppression settings

use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use hidapi::HidApi;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let api = HidApi::new()?;
    match args.first().map(String::as_str) {
        Some("list") => {
            for d in api.device_list().filter(|d| d.vendor_id() == arctis::STEELSERIES_VID) {
                println!(
                    "pid {:04X} if {} page {:04X} usage {:04X} {:?}",
                    d.product_id(),
                    d.interface_number(),
                    d.usage_page(),
                    d.usage(),
                    d.product_string()
                );
            }
        }
        Some("devices") => {
            for flow in [sysaudio::Flow::Output, sysaudio::Flow::Input] {
                let def = sysaudio::default_endpoint(flow, false)?.map(|e| e.id);
                let comm = sysaudio::default_endpoint(flow, true)?.map(|e| e.id);
                println!("{flow:?}:");
                for e in sysaudio::endpoints(flow)? {
                    let mark = match (def.as_ref() == Some(&e.id), comm.as_ref() == Some(&e.id)) {
                        (true, true) => "*C",
                        (true, false) => "* ",
                        (false, true) => " C",
                        _ => "  ",
                    };
                    println!("  {mark} {}  [{}]  {}", e.name, e.device, e.id);
                }
                if let Some(ep) = sysaudio::find_arctis(flow)? {
                    println!("  apo: {:?}", sysaudio::apo::status(&ep));
                }
            }
        }
        Some("default") if args.len() == 2 => sysaudio::set_default(&args[1])?,
        Some("install-apo") if args.len() == 2 => {
            let ep = sysaudio::find_arctis(sysaudio::Flow::Input)?.context("Nova 5 mic not found")?;
            sysaudio::apo::install(std::path::Path::new(&args[1]), &ep)?;
            println!("installed on {}", ep.name);
        }
        Some("uninstall-apo") => {
            let ep = sysaudio::find_arctis(sysaudio::Flow::Input)?;
            sysaudio::apo::uninstall(ep.as_ref())?;
            println!("uninstalled");
        }
        Some("mic") if args.len() == 3 => {
            sysaudio::apo::set_mic_settings(args[1] == "1", args[2].parse()?)?;
        }
        Some("raw") => {
            let bytes: Vec<u8> = args[1..]
                .iter()
                .map(|b| u8::from_str_radix(b, 16))
                .collect::<Result<_, _>>()
                .context("bytes must be hex")?;
            let info = api
                .device_list()
                .find(|d| {
                    d.vendor_id() == arctis::STEELSERIES_VID
                        && d.usage_page() == arctis::VENDOR_USAGE_PAGE
                })
                .context("no Nova vendor interface")?;
            let dev = api.open_path(info.path())?;
            let mut buf = [0u8; 65];
            buf[1..1 + bytes.len()].copy_from_slice(&bytes);
            dev.write(&buf)?;
            let until = Instant::now() + Duration::from_millis(500);
            let mut got = 0;
            while Instant::now() < until {
                let mut r = [0u8; 128];
                let n = dev.read_timeout(&mut r, 100)?;
                if n > 0 {
                    got += 1;
                    println!("{}", hex(&r[..n]));
                }
            }
            if got == 0 {
                println!("(no reply)");
            }
        }
        _ => {
            if !args.is_empty() {
                bail!("usage: probe [list | raw <hex> | devices | default <id> | install-apo <dll> | uninstall-apo | mic <0|1> <pct>]");
            }
            let h = arctis::Headset::open()?;
            println!("{:#?}", h.status()?);
            let st = h.settings()?;
            for (i, band) in st.eq.iter().enumerate() {
                println!("band {:2}: {band:?}", i + 1);
            }
            println!("mic volume {}, sidetone {}, volume limiter {}", st.mic_volume, st.sidetone, st.volume_limiter);
        }
    }
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}
