//! Plain shared-mode streams on one endpoint, for hearing the mic inside kit.
//!
//! A capture stream here is opened like any app's, in the default processing
//! mode, so what it reads has been through the noise suppression APO. Both
//! directions are mono floats at [`RATE`]; the audio engine converts to and
//! from the endpoint's own format.

use std::time::Duration;

use windows::Win32::Media::Audio::{
    IAudioCaptureClient, IAudioClient, IAudioRenderClient, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX,
};
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;

use crate::{audio_client, Result};

/// Samples per second in both directions.
pub const RATE: u32 = 48000;
const SAMPLE_BYTES: u32 = size_of::<f32>() as u32;
/// Room for what arrives between two reads, however late the second one is.
const CAPTURE_BUFFER: Duration = Duration::from_millis(500);

fn open(id: &str, buffer: Duration) -> Result<IAudioClient> {
    let client = audio_client(id)?;
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
        nChannels: 1,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * SAMPLE_BYTES,
        nBlockAlign: SAMPLE_BYTES as u16,
        wBitsPerSample: SAMPLE_BYTES as u16 * 8,
        cbSize: 0,
    };
    let flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
    // The buffer is in 100 ns units.
    let buffer = buffer.as_nanos() as i64 / 100;
    unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, buffer, 0, &format, None)? };
    Ok(client)
}

/// A running mic stream. COM objects: keep it on the thread that opened it.
pub struct Capture {
    client: IAudioClient,
    capture: IAudioCaptureClient,
}

impl Capture {
    /// Start capturing from the endpoint `id`.
    pub fn open(id: &str) -> Result<Self> {
        let client = open(id, CAPTURE_BUFFER)?;
        unsafe {
            let capture = client.GetService()?;
            client.Start()?;
            Ok(Capture { client, capture })
        }
    }

    /// Append everything captured since the last read to `out`.
    pub fn read(&self, out: &mut Vec<f32>) -> Result<()> {
        unsafe {
            while self.capture.GetNextPacketSize()? > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0, 0);
                self.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                // The buffer's contents are undefined when the engine marks it silent.
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                    out.resize(out.len() + frames as usize, 0.0);
                } else {
                    out.extend_from_slice(std::slice::from_raw_parts(data as *const f32, frames as usize));
                }
                self.capture.ReleaseBuffer(frames)?;
            }
        }
        Ok(())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}

/// A stream to an output. COM objects: keep it on the thread that opened it.
pub struct Render {
    client: IAudioClient,
    render: IAudioRenderClient,
    /// Frames it can hold.
    size: u32,
}

impl Render {
    /// Open the endpoint `id`, holding about `buffer` of audio. Nothing plays until [`Render::start`].
    pub fn open(id: &str, buffer: Duration) -> Result<Self> {
        let client = open(id, buffer)?;
        unsafe {
            let render = client.GetService()?;
            let size = client.GetBufferSize()?;
            Ok(Render { client, render, size })
        }
    }

    pub fn start(&self) -> Result<()> {
        unsafe { self.client.Start()? };
        Ok(())
    }

    /// Samples written and not played yet.
    pub fn queued(&self) -> Result<usize> {
        Ok(unsafe { self.client.GetCurrentPadding()? } as usize)
    }

    /// Queue as much of `samples` as fits and return how much that was.
    pub fn write(&self, samples: &[f32]) -> Result<usize> {
        let n = samples.len().min(self.size as usize - self.queued()?);
        if n == 0 {
            return Ok(0);
        }
        unsafe {
            let data = self.render.GetBuffer(n as u32)?;
            std::ptr::copy_nonoverlapping(samples.as_ptr(), data as *mut f32, n);
            self.render.ReleaseBuffer(n as u32, 0)?;
        }
        Ok(n)
    }
}

impl Drop for Render {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}
