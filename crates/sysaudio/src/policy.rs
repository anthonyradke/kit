#![allow(non_snake_case)]

//! `IPolicyConfig`: the undocumented interface the Sound control panel uses
//! to change default devices. Stable since Windows 7 and used by every
//! audio switcher, since there is no public API for this.

use std::ffi::c_void;

use windows::core::{interface, GUID, HRESULT, IUnknown, IUnknown_Vtbl, PCWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows_core::BOOL;
use windows::Win32::Media::Audio::{eCommunications, eConsole, eMultimedia, ERole, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
use windows::Win32::System::Variant::VT_UI4;

use crate::stream::{SUBTYPE_FLOAT, WAVE_FORMAT_EXTENSIBLE};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
pub unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut WAVEFORMATEX) -> HRESULT;
    fn GetDeviceFormat(&self, device: PCWSTR, default: BOOL, format: *mut *mut WAVEFORMATEX) -> HRESULT;
    fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *mut WAVEFORMATEX, mix: *mut WAVEFORMATEX) -> HRESULT;
    fn GetProcessingPeriod(&self, device: PCWSTR, default: BOOL, default_period: *mut i64, min_period: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
    fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn GetPropertyValue(&self, device: PCWSTR, key: *const PROPERTYKEY, value: *mut PROPVARIANT) -> HRESULT;
    fn SetPropertyValue(&self, device: PCWSTR, key: *const PROPERTYKEY, value: *const PROPVARIANT) -> HRESULT;
    fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, device: PCWSTR, visible: BOOL) -> HRESULT;
}

/// Make `id` the default device for every role (including communications).
pub fn set_default(id: &str) -> windows::core::Result<()> {
    set_default_roles(id, &[eConsole, eMultimedia, eCommunications])
}

/// Make `id` the default device for `roles` only.
pub fn set_default_roles(id: &str, roles: &[ERole]) -> windows::core::Result<()> {
    let policy = policy()?;
    let wide = wide(id);
    for &role in roles {
        unsafe { policy.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role).ok()? };
    }
    Ok(())
}

fn policy() -> windows::core::Result<IPolicyConfig> {
    unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }
}

fn wide(id: &str) -> Vec<u16> {
    id.encode_utf16().chain(Some(0)).collect()
}

const SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
/// `PKEY_AudioEndpoint_PhysicalSpeakers`, what "Configure speakers" sets.
const PHYSICAL_SPEAKERS: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e), pid: 3 };
/// `PKEY_AudioEndpoint_FullRangeSpeakers`
const FULL_RANGE_SPEAKERS: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e), pid: 6 };

fn extensible(channels: u16, rate: u32, bits: u16, valid: u16, mask: u32, sub: GUID) -> WAVEFORMATEXTENSIBLE {
    let block = channels * bits / 8;
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_EXTENSIBLE,
            nChannels: channels,
            nSamplesPerSec: rate,
            nAvgBytesPerSec: rate * block as u32,
            nBlockAlign: block,
            wBitsPerSample: bits,
            cbSize: 22,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 { wValidBitsPerSample: valid },
        dwChannelMask: mask,
        SubFormat: sub,
    }
}

/// Set the output endpoint `id` up as speakers with `mask` (e.g. 7.1) at
/// `rate`, like Configure speakers plus the Advanced tab's default format.
/// Tries 24-bit, then 16-bit for the device side; the mix is always float.
pub fn set_speaker_format(id: &str, mask: u32, rate: u32) -> windows::core::Result<()> {
    let policy = policy()?;
    let wide = wide(id);
    let channels = mask.count_ones() as u16;
    let mut mix = extensible(channels, rate, 32, 32, mask, SUBTYPE_FLOAT);
    let mut result = Ok(());
    for (bits, valid) in [(32, 24), (24, 24), (16, 16)] {
        let mut device = extensible(channels, rate, bits, valid, mask, SUBTYPE_PCM);
        result = unsafe {
            policy
                .SetDeviceFormat(PCWSTR(wide.as_ptr()), &mut device as *mut _ as *mut WAVEFORMATEX, &mut mix as *mut _ as *mut WAVEFORMATEX)
                .ok()
        };
        if result.is_ok() {
            break;
        }
    }
    result?;
    // Games that ask for the speaker setup (rather than the mix format) read these.
    for key in [PHYSICAL_SPEAKERS, FULL_RANGE_SPEAKERS] {
        let value = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_UI4,
                    Anonymous: PROPVARIANT_0_0_0 { ulVal: mask },
                    ..Default::default()
                }),
            },
        };
        let _ = unsafe { policy.SetPropertyValue(PCWSTR(wide.as_ptr()), &key, &value) };
    }
    Ok(())
}
