//! headroom's mic noise suppression, as a Windows Audio Processing Object.
//!
//! The audio engine (audiodg.exe) loads this DLL for the endpoints whose
//! FxProperties list our CLSID, and calls `APOProcess` on its real-time
//! thread for every period of captured audio. That makes the clean mic the
//! real "Headset Microphone" device: no virtual devices, nothing for apps to
//! pick wrong.
//!
//! Rules for the real-time path: no allocation, no locks that can block, no
//! I/O. Buffers are sized in `LockForProcess`; settings arrive as atomics.

#![allow(non_snake_case)]
// COM entry points are exported without PRIVATE; harmless, and only the system loads them.
#![allow(linker_messages)]

pub mod denoise;
pub mod settings;

use std::ffi::c_void;
use std::io::Write;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use windows::core::{implement, Interface, Ref, GUID, HRESULT, IUnknown};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_INVALIDARG, E_POINTER, E_UNEXPECTED, S_FALSE,
};
use windows::Win32::Media::Audio::Apo::*;
use windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
use windows::Win32::System::Com::{CoTaskMemAlloc, IClassFactory, IClassFactory_Impl};
use windows_core::BOOL;

use denoise::Denoiser;

pub const CLSID: GUID = GUID::from_u128(0xcae0eb59_a9d9_438f_99d2_5695189c2b06);
pub const FRIENDLY_NAME: &str = "headroom mic noise suppression";
const COPYRIGHT: &str = "MIT License";
/// One RNNoise frame, in 100 ns units.
const LATENCY_HNS: i64 = 100_000;
const LOG_PATH: &str = r"C:\ProgramData\headroom\apo.log";

static INSTANCES: AtomicU32 = AtomicU32::new(0);
static LOGGED_FORMAT_REJECT: AtomicBool = AtomicBool::new(false);
/// Set if processing ever panicked.
static FAILED: AtomicBool = AtomicBool::new(false);

/// Best-effort diagnostics. Never called from `APOProcess`.
fn log(msg: &str) {
    let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(LOG_PATH) else { return };
    if f.metadata().map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        return;
    }
    let pid = std::process::id();
    let _ = writeln!(f, "[{pid}] {msg}");
}

struct Locked {
    channels: usize,
    denoiser: Denoiser,
    /// Copy of the input, so input and output may be the same buffer.
    scratch: Vec<f32>,
    zeros: Vec<f32>,
    /// Whether the previous period ran the denoiser (a fresh start resets it).
    was_enabled: bool,
}

#[implement(IAudioProcessingObject, IAudioProcessingObjectRT, IAudioProcessingObjectConfiguration, IAudioSystemEffects)]
struct Apo {
    channels: AtomicU32,
    locked: Mutex<Option<Locked>>,
}

impl Apo {
    fn new() -> Self {
        settings::start();
        let n = INSTANCES.fetch_add(1, Ordering::Relaxed) + 1;
        log(&format!("instance created ({n} total)"));
        Apo { channels: AtomicU32::new(0), locked: Mutex::new(None) }
    }
}

impl Drop for Apo {
    fn drop(&mut self) {
        INSTANCES.fetch_sub(1, Ordering::Relaxed);
    }
}

fn format_of(t: &IAudioMediaType) -> windows::core::Result<UNCOMPRESSEDAUDIOFORMAT> {
    let mut f = UNCOMPRESSEDAUDIOFORMAT::default();
    unsafe { t.GetUncompressedAudioFormat(&mut f)? };
    Ok(f)
}

/// We process 32-bit float only, with the same layout on both sides.
fn check_format(opposite: Ref<'_, IAudioMediaType>, requested: Ref<'_, IAudioMediaType>) -> windows::core::Result<IAudioMediaType> {
    let requested = requested.ok().map_err(|_| windows::core::Error::from(E_POINTER))?;
    let req = format_of(requested)?;
    let mut ok = req.guidFormatType == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT && req.dwBytesPerSampleContainer == 4;
    if let Ok(opposite) = opposite.ok() {
        let opp = format_of(opposite)?;
        ok &= opp.dwSamplesPerFrame == req.dwSamplesPerFrame && opp.fFramesPerSecond == req.fFramesPerSecond;
    }
    if ok {
        Ok(requested.clone())
    } else {
        if !LOGGED_FORMAT_REJECT.swap(true, Ordering::Relaxed) {
            log(&format!(
                "rejected format: {:?} {} ch {} Hz {} bytes",
                req.guidFormatType, req.dwSamplesPerFrame, req.fFramesPerSecond, req.dwBytesPerSampleContainer
            ));
        }
        Err(APOERR_FORMAT_NOT_SUPPORTED.into())
    }
}

impl IAudioProcessingObject_Impl for Apo_Impl {
    fn Reset(&self) -> windows::core::Result<()> {
        if let Ok(mut l) = self.locked.lock() {
            if let Some(l) = l.as_mut() {
                l.denoiser.reset();
            }
        }
        Ok(())
    }

    fn GetLatency(&self) -> windows::core::Result<i64> {
        Ok(LATENCY_HNS)
    }

    fn GetRegistrationProperties(&self) -> windows::core::Result<*mut APO_REG_PROPERTIES> {
        unsafe {
            let p = CoTaskMemAlloc(size_of::<APO_REG_PROPERTIES>()) as *mut APO_REG_PROPERTIES;
            if p.is_null() {
                return Err(windows::Win32::Foundation::E_OUTOFMEMORY.into());
            }
            p.write(registration_properties());
            Ok(p)
        }
    }

    fn Initialize(&self, cbdatasize: u32, pbydata: *const u8) -> windows::core::Result<()> {
        if cbdatasize as usize >= size_of::<APOInitSystemEffects2>() && !pbydata.is_null() {
            let init = unsafe { &*(pbydata as *const APOInitSystemEffects2) };
            log(&format!(
                "initialize: mode {:?} discovery-only {}",
                init.AudioProcessingMode,
                init.InitializeForDiscoveryOnly.as_bool()
            ));
        } else {
            log(&format!("initialize: {cbdatasize} bytes"));
        }
        Ok(())
    }

    fn IsInputFormatSupported(
        &self,
        poppositeformat: Ref<'_, IAudioMediaType>,
        prequestedinputformat: Ref<'_, IAudioMediaType>,
    ) -> windows::core::Result<IAudioMediaType> {
        check_format(poppositeformat, prequestedinputformat)
    }

    fn IsOutputFormatSupported(
        &self,
        poppositeformat: Ref<'_, IAudioMediaType>,
        prequestedoutputformat: Ref<'_, IAudioMediaType>,
    ) -> windows::core::Result<IAudioMediaType> {
        check_format(poppositeformat, prequestedoutputformat)
    }

    fn GetInputChannelCount(&self) -> windows::core::Result<u32> {
        Ok(self.channels.load(Ordering::Relaxed))
    }
}

impl IAudioProcessingObjectConfiguration_Impl for Apo_Impl {
    fn LockForProcess(
        &self,
        u32numinputconnections: u32,
        ppinputconnections: *const *const APO_CONNECTION_DESCRIPTOR,
        u32numoutputconnections: u32,
        ppoutputconnections: *const *const APO_CONNECTION_DESCRIPTOR,
    ) -> windows::core::Result<()> {
        if u32numinputconnections != 1 || u32numoutputconnections != 1 {
            return Err(APOERR_NUM_CONNECTIONS_INVALID.into());
        }
        let mut locked = self.locked.lock().map_err(|_| windows::core::Error::from(E_UNEXPECTED))?;
        if locked.is_some() {
            return Err(APOERR_APO_LOCKED.into());
        }
        let (input, output) = unsafe { (&**ppinputconnections, &**ppoutputconnections) };
        let format = input.pFormat.as_ref().ok_or(windows::core::Error::from(E_INVALIDARG))?;
        let f = format_of(format)?;
        let channels = f.dwSamplesPerFrame.max(1) as usize;
        let max_frames = input.u32MaxFrameCount.max(output.u32MaxFrameCount) as usize;
        log(&format!(
            "lock: {} ch, {} Hz, max {} frames, denoise {} at {:.0}%",
            channels,
            f.fFramesPerSecond,
            max_frames,
            settings::enabled(),
            settings::strength() * 100.0
        ));
        self.channels.store(channels as u32, Ordering::Relaxed);
        *locked = Some(Locked {
            channels,
            denoiser: Denoiser::new(channels),
            scratch: vec![0.0; max_frames * channels],
            zeros: vec![0.0; max_frames * channels],
            was_enabled: false,
        });
        Ok(())
    }

    fn UnlockForProcess(&self) -> windows::core::Result<()> {
        let mut locked = self.locked.lock().map_err(|_| windows::core::Error::from(E_UNEXPECTED))?;
        if locked.take().is_none() {
            return Err(APOERR_ALREADY_UNLOCKED.into());
        }
        Ok(())
    }
}

impl IAudioProcessingObjectRT_Impl for Apo_Impl {
    fn APOProcess(
        &self,
        u32numinputconnections: u32,
        ppinputconnections: *const *const APO_CONNECTION_PROPERTY,
        u32numoutputconnections: u32,
        ppoutputconnections: *mut *mut APO_CONNECTION_PROPERTY,
    ) {
        if u32numinputconnections < 1 || u32numoutputconnections < 1 {
            return;
        }
        let (input, output) = unsafe { (&**ppinputconnections, &mut **ppoutputconnections) };
        output.u32ValidFrameCount = input.u32ValidFrameCount;
        output.u32BufferFlags = input.u32BufferFlags;
        let len = input.u32ValidFrameCount as usize * self.channels.load(Ordering::Relaxed) as usize;

        let processed = !FAILED.load(Ordering::Relaxed)
            && std::panic::catch_unwind(AssertUnwindSafe(|| self.process(input, output, len))).unwrap_or_else(|_| {
                // Never retry: stay a plain passthrough until audiodg restarts.
                FAILED.store(true, Ordering::Relaxed);
                false
            });
        if !processed {
            output.u32BufferFlags = input.u32BufferFlags;
            passthrough(input, output, len);
        }
    }

    fn CalcInputFrames(&self, u32outputframecount: u32) -> u32 {
        u32outputframecount
    }

    fn CalcOutputFrames(&self, u32inputframecount: u32) -> u32 {
        u32inputframecount
    }
}

impl Apo {
    /// Denoise one period. Returns false when the input should pass through untouched.
    fn process(&self, input: &APO_CONNECTION_PROPERTY, output: &mut APO_CONNECTION_PROPERTY, len: usize) -> bool {
        if !settings::enabled() {
            if let Ok(mut guard) = self.locked.try_lock() {
                if let Some(l) = guard.as_mut() {
                    l.was_enabled = false;
                }
            }
            return false;
        }
        // Lock/Unlock never overlap processing, so this never actually waits.
        let Ok(mut guard) = self.locked.try_lock() else { return false };
        let Some(l) = guard.as_mut() else { return false };
        if len > l.scratch.len() || len == 0 {
            return false;
        }
        if !l.was_enabled {
            l.denoiser.reset();
            l.was_enabled = true;
        }
        let src: &[f32] = if input.u32BufferFlags == BUFFER_SILENT {
            &l.zeros[..len]
        } else {
            let src = unsafe { std::slice::from_raw_parts(input.pBuffer as *const f32, len) };
            l.scratch[..len].copy_from_slice(src);
            &l.scratch[..len]
        };
        let out = unsafe { std::slice::from_raw_parts_mut(output.pBuffer as *mut f32, len) };
        l.denoiser.process(src, out, len / l.channels, settings::strength());
        // The denoiser's FIFO can hold speech from earlier periods, so the
        // output is real audio even when this period's input was silent.
        output.u32BufferFlags = BUFFER_VALID;
        true
    }
}

fn passthrough(input: &APO_CONNECTION_PROPERTY, output: &mut APO_CONNECTION_PROPERTY, len: usize) {
    if output.pBuffer == input.pBuffer || len == 0 {
        return;
    }
    let out = unsafe { std::slice::from_raw_parts_mut(output.pBuffer as *mut f32, len) };
    if input.u32BufferFlags == BUFFER_SILENT {
        out.fill(0.0);
    } else {
        out.copy_from_slice(unsafe { std::slice::from_raw_parts(input.pBuffer as *const f32, len) });
    }
}

impl IAudioSystemEffects_Impl for Apo_Impl {}

pub fn registration_properties() -> APO_REG_PROPERTIES {
    let mut p = APO_REG_PROPERTIES {
        clsid: CLSID,
        Flags: APO_FLAG_DEFAULT,
        u32MajorVersion: 1,
        u32MinorVersion: 0,
        u32MinInputConnections: 1,
        u32MaxInputConnections: 1,
        u32MinOutputConnections: 1,
        u32MaxOutputConnections: 1,
        u32MaxInstances: u32::MAX,
        u32NumAPOInterfaces: 1,
        iidAPOInterfaceList: [IAudioProcessingObject::IID],
        ..Default::default()
    };
    copy_wide(&mut p.szFriendlyName, FRIENDLY_NAME);
    copy_wide(&mut p.szCopyrightInfo, COPYRIGHT);
    p
}

fn copy_wide(dst: &mut [u16], s: &str) {
    let max = dst.len() - 1;
    for (d, c) in dst.iter_mut().zip(s.encode_utf16().take(max)) {
        *d = c;
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Ref<'_, IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if ppvobject.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppvobject = std::ptr::null_mut() };
        if punkouter.is_some() {
            log("CreateInstance refused: aggregation requested");
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let unknown: IUnknown = Apo::new().into();
        unsafe { unknown.query(&*riid, ppvobject).ok() }
    }

    fn LockServer(&self, _flock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    unsafe {
        *ppv = std::ptr::null_mut();
        if *rclsid != CLSID {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = Factory.into();
        factory.query(&*riid, ppv)
    }
}

/// audiodg keeps the DLL for the life of the process; the settings thread lives in it.
#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}
