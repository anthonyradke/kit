//! `headset-probe kit-dll <kit_apo.dll> <dir>`: run the measurement program
//! through kit's EQ APO offline, the way audiodg would run it, and analyze
//! the result next to the Sonar takes in `dir`.
//!
//! The DLL is loaded with `LoadLibrary` and driven through its own class
//! factory and COM interfaces, and it gets its curve the way it does when
//! installed: from the registry, written with `sysaudio::apo::set_headset_eq`
//! (the key is writable without elevation once kit's mic APO is installed).
//! So this checks the shipped DLL end to end without installing anything.

use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use windows::core::{implement, Interface, Ref, GUID, HRESULT, PCSTR};
use windows::Win32::Media::Audio::Apo::*;
use windows::Win32::Media::Audio::WAVEFORMATEX;
use windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_core::BOOL;

use crate::measure;
use crate::signals::{Program, RATE};

/// The engine's period at 48 kHz.
const PERIOD: usize = 480;

#[implement(IAudioMediaType)]
struct Format(UNCOMPRESSEDAUDIOFORMAT);

impl IAudioMediaType_Impl for Format_Impl {
    fn IsCompressedFormat(&self) -> windows::core::Result<BOOL> {
        Ok(false.into())
    }
    fn IsEqual(&self, _other: Ref<'_, IAudioMediaType>) -> windows::core::Result<u32> {
        Ok(0)
    }
    fn GetAudioFormat(&self) -> *mut WAVEFORMATEX {
        std::ptr::null_mut()
    }
    fn GetUncompressedAudioFormat(&self, out: *mut UNCOMPRESSEDAUDIOFORMAT) -> windows::core::Result<()> {
        unsafe { *out = self.0 };
        Ok(())
    }
}

type GetClassObject = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut std::ffi::c_void) -> HRESULT;

/// The program through the DLL's EQ with `curve` (Equalizer APO text).
pub fn process(dll: &Path, curve: &str, prog: &[[f32; 2]]) -> Result<Vec<[f32; 2]>> {
    kit_eq::Curve::parse(curve).map_err(anyhow::Error::msg)?;
    sysaudio::com_init();
    sysaudio::apo::set_headset_eq(true, curve).context("write the curve to HKLM\\SOFTWARE\\kit (is kit's mic APO installed?)")?;
    let out = run(dll, prog);
    // Back off: nothing should find the EQ on later.
    let _ = sysaudio::apo::set_headset_eq(false, curve);
    out
}

fn run(dll: &Path, prog: &[[f32; 2]]) -> Result<Vec<[f32; 2]>> {
    let wide: Vec<u16> = dll.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
    unsafe {
        let module = LoadLibraryW(windows::core::PCWSTR(wide.as_ptr())).with_context(|| format!("load {}", dll.display()))?;
        let entry = GetProcAddress(module, PCSTR(c"DllGetClassObject".as_ptr() as *const u8)).context("no DllGetClassObject")?;
        let get: GetClassObject = std::mem::transmute(entry);
        let mut factory = std::ptr::null_mut();
        get(&kit_apo::eq::CLSID, &IClassFactory::IID, &mut factory).ok().context("the DLL has no headset EQ class")?;
        let factory = IClassFactory::from_raw(factory);
        let apo: IAudioProcessingObject = factory.CreateInstance(None)?;
        // Its settings thread reads the registry as it starts.
        std::thread::sleep(Duration::from_millis(500));
        let ty: IAudioMediaType = Format(UNCOMPRESSEDAUDIOFORMAT {
            guidFormatType: KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
            dwSamplesPerFrame: 2,
            dwBytesPerSampleContainer: 4,
            dwValidBitsPerSample: 32,
            fFramesPerSecond: RATE as f32,
            dwChannelMask: 3,
        })
        .into();
        apo.IsInputFormatSupported(None, &ty).context("the EQ refuses 48 kHz stereo float")?;
        let desc = APO_CONNECTION_DESCRIPTOR {
            Type: APO_CONNECTION_BUFFER_TYPE_ALLOCATED,
            pBuffer: 0,
            u32MaxFrameCount: PERIOD as u32,
            pFormat: std::mem::ManuallyDrop::new(Some(ty.clone())),
            u32Signature: 0,
        };
        let d: *const APO_CONNECTION_DESCRIPTOR = &desc;
        let config: IAudioProcessingObjectConfiguration = apo.cast()?;
        config.LockForProcess(&[d], &[d])?;
        let latency = apo.GetLatency()?;
        eprintln!("  locked; latency {:.2} ms", latency as f64 / 10_000.0);
        let rt: IAudioProcessingObjectRT = apo.cast()?;
        let mut out = Vec::with_capacity(prog.len());
        let mut ran = false;
        for chunk in prog.chunks(PERIOD) {
            let mut buf: Vec<f32> = chunk.iter().flatten().copied().collect();
            let silent = buf.iter().all(|&x| x == 0.0);
            let inp = APO_CONNECTION_PROPERTY {
                pBuffer: buf.as_mut_ptr() as usize,
                u32ValidFrameCount: chunk.len() as u32,
                u32BufferFlags: if silent { BUFFER_SILENT } else { BUFFER_VALID },
                u32Signature: 0,
            };
            let mut o = inp;
            let ip: *const APO_CONNECTION_PROPERTY = &inp;
            let mut op: *mut APO_CONNECTION_PROPERTY = &mut o;
            rt.APOProcess(1, &ip, 1, &mut op);
            if o.u32BufferFlags == BUFFER_SILENT {
                buf.fill(0.0);
            }
            ran |= buf.as_chunks::<2>().0.iter().zip(chunk).any(|(a, b)| a[0] != b[0]);
            out.extend_from_slice(buf.as_chunks::<2>().0);
        }
        config.UnlockForProcess()?;
        if !ran {
            bail!("the EQ passed everything through unchanged; it never got the curve");
        }
        Ok(out)
    }
}

/// `kit-dll`: process, save as `kit.wav` next to the Sonar takes, analyze.
pub fn run_cmd(dll: &Path, dir: &Path, curve: Option<&Path>) -> Result<()> {
    let text = match curve {
        Some(p) => std::fs::read_to_string(p).with_context(|| format!("read {}", p.display()))?,
        None => crate::model::sonar_main().to_string(),
    };
    std::fs::create_dir_all(dir)?;
    let prog = Program::standard();
    // Silence on both sides, like a recording: the analysis looks for the start.
    let mut frames = vec![[0.0f32; 2]; RATE / 4];
    frames.extend_from_slice(&prog.frames);
    frames.extend(std::iter::repeat_n([0.0f32; 2], RATE));
    eprintln!("kit: running {:.0} s through {} ...", frames.len() as f64 / RATE as f64, dll.display());
    let out = process(dll, &text, &frames)?;
    measure::write_wav(&dir.join("kit.wav"), &out)?;
    std::fs::write(dir.join("kit.txt"), format!("the measurement program\nkit's EQ APO ({}), offline\n", dll.display()))?;
    std::fs::write(dir.join("kit-curve.txt"), &text)?;
    crate::capture::analyze_dir(dir)
}
