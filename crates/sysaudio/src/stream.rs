//! Plain shared-mode streams on one endpoint.
//!
//! [`Capture`] and [`Render`] open like any app's, in float at [`RATE`] with
//! the audio engine converting to and from the endpoint's own format. A mic
//! capture here has been through the noise suppression APO. [`Loopback`]
//! records what an output endpoint is playing, in that endpoint's own mix
//! format (all its channels), for surround.

use std::time::Duration;

use windows::core::GUID;
use windows::Win32::Media::Audio::{
    IAudioCaptureClient, IAudioClient, IAudioRenderClient, AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
use windows::Win32::System::Com::CoTaskMemFree;

use crate::{audio_client, Error, Result};

/// Samples per second for [`Capture`] and [`Render`].
pub const RATE: u32 = 48000;
const SAMPLE_BYTES: u32 = size_of::<f32>() as u32;
/// Room for what arrives between two reads, however late the second one is.
const CAPTURE_BUFFER: Duration = Duration::from_millis(500);
pub(crate) const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
pub(crate) const SUBTYPE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

/// The shape of a stream's samples. Always 32-bit float, interleaved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub channels: usize,
    pub rate: u32,
    /// `dwChannelMask`, which speaker each channel is for; 0 if the format doesn't say.
    pub mask: u32,
}

fn open(id: &str, channels: u16, buffer: Duration) -> Result<IAudioClient> {
    let client = audio_client(id)?;
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
        nChannels: channels,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * SAMPLE_BYTES * channels as u32,
        nBlockAlign: (SAMPLE_BYTES * channels as u32) as u16,
        wBitsPerSample: SAMPLE_BYTES as u16 * 8,
        cbSize: 0,
    };
    let flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
    // The buffer is in 100 ns units.
    let buffer = buffer.as_nanos() as i64 / 100;
    unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, buffer, 0, &format, None)? };
    Ok(client)
}

/// Read a mix format the engine handed out; only float is expected in shared mode.
pub(crate) unsafe fn read_format(p: *const WAVEFORMATEX) -> Result<Format> {
    // WAVEFORMATEX is packed: copy fields out instead of referencing them.
    let f = unsafe { *p };
    let (tag, size, bits, channels, rate) = (f.wFormatTag, f.cbSize, f.wBitsPerSample, f.nChannels, f.nSamplesPerSec);
    let ext = (tag == WAVE_FORMAT_EXTENSIBLE && size >= 22).then(|| unsafe { *(p as *const WAVEFORMATEXTENSIBLE) });
    let (sub, mask) = match ext {
        Some(x) => (Some(x.SubFormat), x.dwChannelMask),
        None => (None, 0),
    };
    let float = tag == WAVE_FORMAT_IEEE_FLOAT as u16 || sub == Some(SUBTYPE_FLOAT);
    if !float || bits != 32 {
        return Err(Error::Msg(format!("unexpected mix format (tag {tag:#x}, {bits} bit)")));
    }
    Ok(Format { channels: channels as usize, rate, mask })
}

/// Move every packet waiting in `capture` to `out` (interleaved, `channels` wide).
/// Returns whether the engine reported a gap.
fn drain(capture: &IAudioCaptureClient, channels: usize, out: &mut Vec<f32>) -> Result<bool> {
    let mut gap = false;
    unsafe {
        while capture.GetNextPacketSize()? > 0 {
            let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0, 0);
            capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
            let samples = frames as usize * channels;
            // The buffer's contents are undefined when the engine marks it silent.
            if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                out.resize(out.len() + samples, 0.0);
            } else {
                out.extend_from_slice(std::slice::from_raw_parts(data as *const f32, samples));
            }
            gap |= flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0;
            capture.ReleaseBuffer(frames)?;
        }
    }
    Ok(gap)
}

/// A running mic stream (mono). COM objects: keep it on the thread that opened it.
pub struct Capture {
    client: IAudioClient,
    capture: IAudioCaptureClient,
}

impl Capture {
    /// Start capturing from the endpoint `id`.
    pub fn open(id: &str) -> Result<Self> {
        let client = open(id, 1, CAPTURE_BUFFER)?;
        unsafe {
            let capture = client.GetService()?;
            client.Start()?;
            Ok(Capture { client, capture })
        }
    }

    /// Append everything captured since the last read to `out`.
    pub fn read(&self, out: &mut Vec<f32>) -> Result<()> {
        drain(&self.capture, 1, out).map(|_| ())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}

/// What an output endpoint plays, in its own mix format. COM objects: keep it
/// on the thread that opened it.
pub struct Loopback {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    format: Format,
}

impl Loopback {
    /// Start recording what the output endpoint `id` plays.
    pub fn open(id: &str) -> Result<Self> {
        let client = audio_client(id)?;
        unsafe {
            let mix = client.GetMixFormat()?;
            let buffer = CAPTURE_BUFFER.as_nanos() as i64 / 100;
            let opened = (|| -> Result<Format> {
                let format = read_format(mix)?;
                client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, buffer, 0, mix, None)?;
                Ok(format)
            })();
            CoTaskMemFree(Some(mix as *const _));
            let format = opened?;
            let capture = client.GetService()?;
            client.Start()?;
            Ok(Loopback { client, capture, format })
        }
    }

    pub fn format(&self) -> Format {
        self.format
    }

    /// Append everything recorded since the last read to `out` (interleaved,
    /// [`Format::channels`] wide). Returns whether there was a gap. Nothing
    /// arrives while no app is playing to the endpoint.
    pub fn read(&self, out: &mut Vec<f32>) -> Result<bool> {
        drain(&self.capture, self.format.channels, out)
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}

/// A stream to an output. COM objects: keep it on the thread that opened it.
pub struct Render {
    client: IAudioClient,
    render: IAudioRenderClient,
    channels: usize,
    /// Frames it can hold.
    size: u32,
}

impl Render {
    /// A mono stream to the endpoint `id`, holding about `buffer` of audio.
    /// Nothing plays until [`Render::start`].
    pub fn open(id: &str, buffer: Duration) -> Result<Self> {
        Self::open_channels(id, 1, buffer)
    }

    /// Like [`Render::open`] with `channels` interleaved channels.
    pub fn open_channels(id: &str, channels: u16, buffer: Duration) -> Result<Self> {
        let client = open(id, channels, buffer)?;
        unsafe {
            let render = client.GetService()?;
            let size = client.GetBufferSize()?;
            Ok(Render { client, render, channels: channels as usize, size })
        }
    }

    pub fn start(&self) -> Result<()> {
        unsafe { self.client.Start()? };
        Ok(())
    }

    /// Frames it can hold.
    pub fn capacity(&self) -> usize {
        self.size as usize
    }

    /// Frames written and not played yet.
    pub fn queued(&self) -> Result<usize> {
        Ok(unsafe { self.client.GetCurrentPadding()? } as usize)
    }

    /// Queue as many whole frames of `samples` (interleaved) as fit and
    /// return how many samples that was.
    pub fn write(&self, samples: &[f32]) -> Result<usize> {
        let frames = (samples.len() / self.channels).min(self.size as usize - self.queued()?);
        if frames == 0 {
            return Ok(0);
        }
        let n = frames * self.channels;
        unsafe {
            let data = self.render.GetBuffer(frames as u32)?;
            std::ptr::copy_nonoverlapping(samples.as_ptr(), data as *mut f32, n);
            self.render.ReleaseBuffer(frames as u32, 0)?;
        }
        Ok(n)
    }
}

impl Drop for Render {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}
