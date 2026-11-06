//! Reading a music file (MP3, AAC/M4A, FLAC, WAV, ... whatever Windows
//! itself can play) as 48 kHz stereo float, with Media Foundation doing the
//! decoding and the sample rate conversion.

use std::path::Path;

use anyhow::{Context, Result};
use windows::core::PCWSTR;
use windows::Win32::Media::MediaFoundation::*;

use crate::signals::RATE;

pub fn decode(path: &Path) -> Result<Vec<[f32; 2]>> {
    let full = std::fs::canonicalize(path).with_context(|| format!("open {}", path.display()))?;
    let url = full.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let wide: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    sysaudio::com_init();
    unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)?;
        let out = read(&wide);
        let _ = MFShutdown();
        out.with_context(|| format!("Windows can't decode {}", path.display()))
    }
}

unsafe fn read(url: &[u16]) -> Result<Vec<[f32; 2]>> {
    unsafe {
        let reader = MFCreateSourceReaderFromURL(PCWSTR(url.as_ptr()), None)?;
        let stream = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(stream, true)?;
        let ty = MFCreateMediaType()?;
        ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        ty.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)?;
        ty.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
        ty.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE as u32)?;
        ty.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 32)?;
        ty.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 8)?;
        ty.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, RATE as u32 * 8)?;
        reader.SetCurrentMediaType(stream, None, &ty)?;
        let mut frames = Vec::new();
        loop {
            let mut flags = 0u32;
            let mut sample = None;
            reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))?;
            if let Some(sample) = sample {
                let buf = sample.ConvertToContiguousBuffer()?;
                let (mut data, mut len) = (std::ptr::null_mut(), 0u32);
                buf.Lock(&mut data, None, Some(&mut len))?;
                let floats = std::slice::from_raw_parts(data as *const f32, len as usize / 4);
                frames.extend_from_slice(floats.as_chunks::<2>().0);
                buf.Unlock()?;
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
        }
        Ok(frames)
    }
}
