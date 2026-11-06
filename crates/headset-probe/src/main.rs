//! Hardware probe for the Arctis Nova 5.
//!
//! - `headset-probe`: status and EQ
//! - `headset-probe list`: SteelSeries HID interfaces
//! - `headset-probe raw <hex bytes...>`: send one report and dump every reply
//! - `headset-probe devices`: audio endpoints and defaults
//! - `headset-probe default <endpoint id>`: make an endpoint the default
//! - `headset-probe install-apo <dll>` / `headset-probe uninstall-apo` (elevated): mic noise suppression
//! - `headset-probe install-eq <dll>` / `headset-probe uninstall-eq` (elevated): kit's EQ on the headset output
//! - `headset-probe mic <0|1> <strength%>`: noise suppression settings
//! - `headset-probe eq <0|1> [curve.txt]`: turn kit's EQ off or on (with an Equalizer APO format curve)
//! - `headset-probe flat`: set the headset's own EQ flat
//! - `headset-probe capture <dir> [game] [chat] [direct]`: measure Sonar (see `capture`)
//! - `headset-probe analyze <dir>`: analyze an earlier capture again
//! - `headset-probe kit-dll <kit_apo.dll> <dir> [curve.txt]`: run kit's EQ APO offline, compare with Sonar (see `dll`)
//! - `headset-probe ab <music> <curve.txt> [preamp dB]`: blind ABX of Sonar against kit (see `ab`)

mod ab;
mod analyze;
mod capture;
mod decode;
mod dll;
mod fit;
mod measure;
mod model;
mod signals;

use std::path::Path;
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
                    let effect = match flow {
                        sysaudio::Flow::Input => sysaudio::apo::Effect::MicDenoise,
                        sysaudio::Flow::Output => sysaudio::apo::Effect::HeadsetEq,
                    };
                    println!("  {effect:?}: {:?}", sysaudio::apo::status(&ep, effect));
                }
            }
        }
        Some("default") if args.len() == 2 => sysaudio::set_default(&args[1])?,
        Some("mute") if args.len() == 2 => {
            let ep = capture::nova()?;
            sysaudio::set_endpoint_mute(&ep.id, args[1] == "1")?;
            println!("{}: {:?}", ep.name, sysaudio::endpoint_volume(&ep.id)?);
        }
        Some("sessions") if args.len() == 2 => {
            let ep = capture::endpoint_named(&args[1])?;
            println!("mix format {:?}", sysaudio::mix_format(&ep.id));
            for s in sysaudio::sessions(&ep.id)? {
                println!("pid {:>6}  volume {:.3}  {}{}", s.pid, s.volume, if s.muted { "MUTED " } else { "" }, if s.active { "active" } else { "" });
            }
        }
        Some("install-apo") if args.len() == 2 => {
            let ep = sysaudio::find_arctis(sysaudio::Flow::Input)?.context("Nova 5 mic not found")?;
            sysaudio::apo::install(std::path::Path::new(&args[1]), &ep, sysaudio::apo::Effect::MicDenoise)?;
            println!("installed on {}", ep.name);
        }
        Some("uninstall-apo") => {
            let ep = sysaudio::find_arctis(sysaudio::Flow::Input)?;
            sysaudio::apo::uninstall(ep.as_ref(), sysaudio::apo::Effect::MicDenoise)?;
            println!("uninstalled");
        }
        Some("install-eq") if args.len() == 2 => {
            let ep = capture::nova()?;
            sysaudio::apo::install(std::path::Path::new(&args[1]), &ep, sysaudio::apo::Effect::HeadsetEq)?;
            println!("installed on {}", ep.name);
        }
        Some("uninstall-eq") => {
            let ep = capture::nova().ok();
            sysaudio::apo::uninstall(ep.as_ref(), sysaudio::apo::Effect::HeadsetEq)?;
            println!("uninstalled");
        }
        Some("eq") if args.len() == 2 || args.len() == 3 => {
            let text = match args.get(2) {
                Some(path) => std::fs::read_to_string(path).with_context(|| format!("read {path}"))?,
                None => String::new(),
            };
            kit_eq::Curve::parse(&text).map_err(anyhow::Error::msg)?;
            sysaudio::apo::set_headset_eq(args[1] == "1", &text)?;
        }
        Some("mic") if args.len() == 3 => {
            sysaudio::apo::set_mic_settings(args[1] == "1", args[2].parse()?)?;
        }
        Some("capture") if args.len() >= 2 => capture::run(Path::new(&args[1]), &args[2..])?,
        Some("analyze") if args.len() == 2 => capture::analyze_dir(Path::new(&args[1]))?,
        Some("kit-dll") if args.len() == 3 || args.len() == 4 => {
            dll::run_cmd(Path::new(&args[1]), Path::new(&args[2]), args.get(3).map(Path::new))?
        }
        Some("ab") if args.len() == 3 || args.len() == 4 => {
            let preamp = args.get(3).map(|p| p.parse()).transpose().context("preamp must be a number of dB")?;
            ab::run(Path::new(&args[1]), Path::new(&args[2]), preamp)?;
        }
        Some("flat") => {
            let h = arctis::Headset::open()?;
            let flat = [None; arctis::BANDS];
            h.set_eq(&flat, true)?;
            for (i, band) in h.settings()?.eq.iter().enumerate() {
                println!("band {:2}: {band:?}", i + 1);
            }
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
                bail!("usage: probe [list | raw <hex> | devices | default <id> | install-apo <dll> | uninstall-apo | install-eq <dll> | uninstall-eq | mic <0|1> <pct> | eq <0|1> [curve.txt] | flat | capture <dir> [game] [chat] [direct] | analyze <dir> | kit-dll <dll> <dir> [curve.txt] | ab <music> <curve.txt> [preamp dB]]");
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
