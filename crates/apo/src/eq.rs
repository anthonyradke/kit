//! kit's headset EQ, as the endpoint effect (EFX) on the Nova 5's output.
//!
//! Sonar did its EQ in a virtual "Game" device and sent the result on to the
//! headset. This runs the same curve on the headset's own endpoint instead,
//! after Windows has mixed every app together, so there is one output device
//! and nothing to route. The curve comes from the kit app through the
//! registry (`settings`), is designed the way Sonar runs it (`kit_eq`'s
//! matched design, off the audio thread), and runs in 64-bit floating point
//! with a preamp and a lookahead limiter.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use kit_eq::{Curve, Design, Processor};
use windows::core::{implement, Ref, GUID};
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Media::Audio::Apo::*;
use windows::Win32::System::Com::CoTaskMemAlloc;

use crate::{check_format, format_of, guard, log, passthrough, settings};

pub const CLSID: GUID = GUID::from_u128(0x5f4b3c0e_8d7a_4b1e_9c62_3a91e0d4b7f5);
pub const FRIENDLY_NAME: &str = "kit headset EQ";

/// Set if processing ever panicked; the EQ then passes audio through until
/// audiodg restarts.
static FAILED: AtomicBool = AtomicBool::new(false);

/// The EQ panicked and passes audio through until audiodg restarts.
pub(crate) fn failed() -> bool {
    FAILED.load(Ordering::Relaxed)
}

/// What the app asked for, as the settings thread last read it.
pub(crate) struct Wanted {
    pub on: bool,
    pub curve: Curve,
}

static WANTED: Mutex<Option<Wanted>> = Mutex::new(None);
/// Every locked instance, so a new curve reaches them all.
static SLOTS: Mutex<Vec<Arc<Slot>>> = Mutex::new(Vec::new());

/// One locked stream's mailbox: the settings thread designs for its rate and
/// leaves the result here; the audio thread picks it up with `try_lock`.
struct Slot {
    rate: f64,
    pending: Mutex<Option<Option<Design>>>,
}

/// The design for `rate`, or `None` for passthrough.
fn design_for(rate: f64) -> Option<Design> {
    let wanted = WANTED.lock().ok()?;
    let w = wanted.as_ref().filter(|w| w.on)?;
    Some(w.curve.design(rate))
}

/// Called by the settings thread when the registry values change.
pub(crate) fn set_wanted(wanted: Wanted) {
    if let Ok(mut w) = WANTED.lock() {
        *w = Some(wanted);
    }
    let slots: Vec<Arc<Slot>> = SLOTS.lock().map(|s| s.clone()).unwrap_or_default();
    for slot in slots {
        let design = design_for(slot.rate);
        if let Ok(mut p) = slot.pending.lock() {
            *p = Some(design);
        }
    }
}

struct Locked {
    channels: usize,
    rate: u32,
    slot: Arc<Slot>,
    processor: Processor,
    /// Whether the processor runs (false: passthrough).
    on: bool,
}

#[implement(IAudioProcessingObject, IAudioProcessingObjectRT, IAudioProcessingObjectConfiguration, IAudioSystemEffects)]
pub(crate) struct EqApo {
    channels: AtomicU32,
    /// 100 ns units, for `GetLatency`.
    latency: AtomicU64,
    locked: Mutex<Option<Locked>>,
}

impl EqApo {
    pub(crate) fn new() -> Self {
        settings::start();
        log("headset EQ: instance created");
        EqApo { channels: AtomicU32::new(0), latency: AtomicU64::new(0), locked: Mutex::new(None) }
    }
}

impl IAudioProcessingObject_Impl for EqApo_Impl {
    fn Reset(&self) -> windows::core::Result<()> {
        guard("headset EQ: Reset", || {
            if let Ok(mut l) = self.locked.lock() {
                if let Some(l) = l.as_mut() {
                    l.processor.reset();
                }
            }
            Ok(())
        })
    }

    fn GetLatency(&self) -> windows::core::Result<i64> {
        Ok(self.latency.load(Ordering::Relaxed) as i64)
    }

    fn GetRegistrationProperties(&self) -> windows::core::Result<*mut APO_REG_PROPERTIES> {
        unsafe {
            let p = CoTaskMemAlloc(size_of::<APO_REG_PROPERTIES>()) as *mut APO_REG_PROPERTIES;
            if p.is_null() {
                return Err(windows::Win32::Foundation::E_OUTOFMEMORY.into());
            }
            p.write(crate::registration_properties(CLSID, FRIENDLY_NAME));
            Ok(p)
        }
    }

    fn Initialize(&self, cbdatasize: u32, pbydata: *const u8) -> windows::core::Result<()> {
        guard("headset EQ: Initialize", || {
            if cbdatasize as usize >= size_of::<APOInitSystemEffects2>() && !pbydata.is_null() {
                let init = unsafe { &*(pbydata as *const APOInitSystemEffects2) };
                log(&format!(
                    "headset EQ: initialize, mode {:?} discovery-only {}",
                    init.AudioProcessingMode,
                    init.InitializeForDiscoveryOnly.as_bool()
                ));
            } else {
                log(&format!("headset EQ: initialize, {cbdatasize} bytes"));
            }
            Ok(())
        })
    }

    fn IsInputFormatSupported(
        &self,
        poppositeformat: Ref<'_, IAudioMediaType>,
        prequestedinputformat: Ref<'_, IAudioMediaType>,
    ) -> windows::core::Result<IAudioMediaType> {
        guard("headset EQ: IsInputFormatSupported", || {
            check_format(poppositeformat, prequestedinputformat)
        })
    }

    fn IsOutputFormatSupported(
        &self,
        poppositeformat: Ref<'_, IAudioMediaType>,
        prequestedoutputformat: Ref<'_, IAudioMediaType>,
    ) -> windows::core::Result<IAudioMediaType> {
        guard("headset EQ: IsOutputFormatSupported", || {
            check_format(poppositeformat, prequestedoutputformat)
        })
    }

    fn GetInputChannelCount(&self) -> windows::core::Result<u32> {
        Ok(self.channels.load(Ordering::Relaxed))
    }
}

impl IAudioProcessingObjectConfiguration_Impl for EqApo_Impl {
    fn LockForProcess(
        &self,
        u32numinputconnections: u32,
        ppinputconnections: *const *const APO_CONNECTION_DESCRIPTOR,
        u32numoutputconnections: u32,
        ppoutputconnections: *const *const APO_CONNECTION_DESCRIPTOR,
    ) -> windows::core::Result<()> {
        guard("headset EQ: LockForProcess", || {
            if u32numinputconnections != 1 || u32numoutputconnections != 1 {
                return Err(APOERR_NUM_CONNECTIONS_INVALID.into());
            }
            let mut locked = self.locked.lock().unwrap_or_else(|e| e.into_inner());
            if locked.is_some() {
                return Err(APOERR_APO_LOCKED.into());
            }
            let input = unsafe { &**ppinputconnections };
            let _ = ppoutputconnections;
            let format = input.pFormat.as_ref().ok_or(windows::core::Error::from(E_INVALIDARG))?;
            let f = format_of(format)?;
            let channels = f.dwSamplesPerFrame.max(1) as usize;
            let rate = f.fFramesPerSecond.round().max(1.0) as u32;
            // Listed before the design is made, so a curve that changes in
            // between is still sent here.
            let slot = Arc::new(Slot { rate: rate as f64, pending: Mutex::new(None) });
            if let Ok(mut slots) = SLOTS.lock() {
                slots.push(slot.clone());
            }
            let design = design_for(rate as f64);
            let mut processor = Processor::new(&design.unwrap_or_default(), rate, channels, true);
            processor.reset();
            let latency = processor.latency() as u64 * 10_000_000 / rate as u64;
            self.latency.store(latency, Ordering::Relaxed);
            log(&format!("headset EQ: lock, {channels} ch, {rate} Hz, EQ {}", if design.is_some() { "on" } else { "off" }));
            self.channels.store(channels as u32, Ordering::Relaxed);
            crate::health::EQ_STREAMS.fetch_add(1, Ordering::Relaxed);
            crate::health::EQ_RATE.store(rate, Ordering::Relaxed);
            *locked = Some(Locked { channels, rate, slot, processor, on: design.is_some() });
            Ok(())
        })
    }

    fn UnlockForProcess(&self) -> windows::core::Result<()> {
        guard("headset EQ: UnlockForProcess", || {
            let mut locked = self.locked.lock().unwrap_or_else(|e| e.into_inner());
            let Some(l) = locked.take() else {
                return Err(APOERR_ALREADY_UNLOCKED.into());
            };
            if let Ok(mut slots) = SLOTS.lock() {
                slots.retain(|s| !Arc::ptr_eq(s, &l.slot));
            }
            crate::health::EQ_STREAMS.fetch_sub(1, Ordering::Relaxed);
            log(&format!("headset EQ: unlock ({} Hz)", l.rate));
            Ok(())
        })
    }
}

impl IAudioProcessingObjectRT_Impl for EqApo_Impl {
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

impl EqApo {
    /// EQ one period. Returns false when the input should pass through untouched.
    fn process(&self, input: &APO_CONNECTION_PROPERTY, output: &mut APO_CONNECTION_PROPERTY, len: usize) -> bool {
        // Lock/Unlock never overlap processing, so this never actually waits.
        let Ok(mut guard) = self.locked.try_lock() else { return false };
        let Some(l) = guard.as_mut() else { return false };
        if let Ok(mut pending) = l.slot.pending.try_lock() {
            if let Some(next) = pending.take() {
                match next {
                    Some(design) => {
                        if !l.on {
                            l.processor.reset();
                        }
                        l.processor.set_design(&design);
                        l.on = true;
                    }
                    None => l.on = false,
                }
            }
        }
        if !l.on || len == 0 || !len.is_multiple_of(l.channels) {
            return false;
        }
        let out = unsafe { std::slice::from_raw_parts_mut(output.pBuffer as *mut f32, len) };
        if input.u32BufferFlags == BUFFER_SILENT {
            // Filter tails and the limiter's delay still have audio to give.
            out.fill(0.0);
        } else if output.pBuffer != input.pBuffer {
            out.copy_from_slice(unsafe { std::slice::from_raw_parts(input.pBuffer as *const f32, len) });
        }
        l.processor.process(out);
        output.u32BufferFlags = BUFFER_VALID;
        true
    }
}

impl IAudioSystemEffects_Impl for EqApo_Impl {}

#[cfg(test)]
mod tests {
    use super::*;
    use kit_eq::{Filter, Kind};
    use windows::core::Interface;
    use windows::Win32::Media::Audio::WAVEFORMATEX;
    use windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
    use windows_core::BOOL;

    /// The little of IAudioMediaType the APO reads.
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

    fn media_type(guid: GUID, bytes: u32, channels: u32, rate: f32) -> IAudioMediaType {
        Format(UNCOMPRESSEDAUDIOFORMAT {
            guidFormatType: guid,
            dwSamplesPerFrame: channels,
            dwBytesPerSampleContainer: bytes,
            dwValidBitsPerSample: bytes * 8,
            fFramesPerSecond: rate,
            dwChannelMask: 3,
        })
        .into()
    }

    /// Run `frames` of stereo through a locked instance, `period` at a time.
    fn run(rt: &IAudioProcessingObjectRT, frames: &[f32], period: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for chunk in frames.chunks(period * 2) {
            let mut buf = chunk.to_vec();
            let inp = APO_CONNECTION_PROPERTY {
                pBuffer: buf.as_mut_ptr() as usize,
                u32ValidFrameCount: (chunk.len() / 2) as u32,
                u32BufferFlags: BUFFER_VALID,
                u32Signature: 0,
            };
            let mut o = inp;
            let ip: *const APO_CONNECTION_PROPERTY = &inp;
            let mut op: *mut APO_CONNECTION_PROPERTY = &mut o;
            unsafe { rt.APOProcess(1, &ip, 1, &mut op) };
            out.extend_from_slice(&buf);
        }
        out
    }

    #[test]
    fn runs_the_curve_through_com_and_follows_changes() {
        // Through the class factory, as audiodg makes it (aggregation is
        // covered by `aggregate`'s test).
        let factory: windows::Win32::System::Com::IClassFactory = crate::Factory(CLSID).into();
        let apo: IAudioProcessingObject = unsafe { factory.CreateInstance(None) }.unwrap();
        let props = unsafe { &*apo.GetRegistrationProperties().unwrap() };
        assert_eq!(props.clsid, CLSID);

        let curve = Curve { preamp_db: -6.0, filters: vec![Filter::new(Kind::Peaking, 1000.0, 6.0, 1.0)] };
        set_wanted(Wanted { on: true, curve: curve.clone() });

        let config: IAudioProcessingObjectConfiguration = apo.cast().unwrap();
        let ty = media_type(KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, 4, 2, 48000.0);
        assert!(unsafe { apo.IsInputFormatSupported(None, &ty) }.is_ok());
        let desc = APO_CONNECTION_DESCRIPTOR {
            Type: APO_CONNECTION_BUFFER_TYPE_ALLOCATED,
            pBuffer: 0,
            u32MaxFrameCount: 480,
            pFormat: std::mem::ManuallyDrop::new(Some(ty.clone())),
            u32Signature: 0,
        };
        let d: *const APO_CONNECTION_DESCRIPTOR = &desc;
        unsafe { config.LockForProcess(&[d], &[d]) }.unwrap();
        // The limiter's lookahead, reported to the engine.
        assert_eq!(unsafe { apo.GetLatency() }.unwrap(), 47 * 10_000_000 / 48000);

        let rt: IAudioProcessingObjectRT = apo.cast().unwrap();
        let tone = |f: f64, n: usize| -> Vec<f32> {
            (0..n).flat_map(|i| [(2.0 * std::f64::consts::PI * f * i as f64 / 48000.0).sin() as f32 * 0.1; 2]).collect()
        };
        let level = |x: &[f32]| {
            let tail = &x[x.len() / 2..];
            20.0 * ((tail.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / tail.len() as f64).sqrt() / (0.1 / 2f64.sqrt())).log10()
        };
        // +6 dB filter, -6 dB preamp: 0 dB at 1 kHz, -6 dB far from it.
        let at_1k = level(&run(&rt, &tone(1000.0, 48000), 480));
        assert!(at_1k.abs() < 0.05, "{at_1k}");
        let at_50 = level(&run(&rt, &tone(50.0, 48000), 480));
        assert!((at_50 + 6.0).abs() < 0.05, "{at_50}");

        // Turning it off passes audio through bit for bit.
        set_wanted(Wanted { on: false, curve: curve.clone() });
        let x = tone(300.0, 4800);
        assert_eq!(run(&rt, &x, 480), x);

        // On again with a new curve.
        let flat = Curve { preamp_db: 3.0, filters: vec![] };
        set_wanted(Wanted { on: true, curve: flat });
        let up = level(&run(&rt, &tone(1000.0, 48000), 480));
        assert!((up - 3.0).abs() < 0.05, "{up}");

        unsafe { config.UnlockForProcess() }.unwrap();
        assert!(SLOTS.lock().unwrap().is_empty());
    }

    #[test]
    fn refuses_integer_formats() {
        let apo: IAudioProcessingObject = EqApo::new().into();
        let pcm = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
        let ty = media_type(pcm, 2, 2, 48000.0);
        assert!(unsafe { apo.IsInputFormatSupported(None, &ty) }.is_err());
    }
}
