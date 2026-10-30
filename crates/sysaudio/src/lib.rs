//! Windows audio plumbing: endpoints, default devices, and installing the
//! mic noise suppression APO on an endpoint.

pub mod apo;
mod policy;

use windows::core::PWSTR;
use windows::Win32::Devices::FunctionDiscovery::{PKEY_DeviceInterface_FriendlyName, PKEY_Device_FriendlyName};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eRender, EDataFlow, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};

pub use windows::core::Error as WinError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("windows: {0}")]
    Win(#[from] windows::core::Error),
    #[error("registry: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Msg(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flow {
    Output,
    Input,
}

impl Flow {
    fn data_flow(self) -> EDataFlow {
        match self {
            Flow::Output => eRender,
            Flow::Input => eCapture,
        }
    }

    /// Registry folder under `MMDevices\Audio`.
    pub fn registry_name(self) -> &'static str {
        match self {
            Flow::Output => "Render",
            Flow::Input => "Capture",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// e.g. `{0.0.1.00000000}.{08a58afb-...}`
    pub id: String,
    pub flow: Flow,
    /// e.g. "Headset Microphone (4- SteelSeries Arctis Nova 5)"
    pub name: String,
    /// The device the endpoint belongs to, e.g. "SteelSeries Arctis Nova 5".
    pub device: String,
}

impl Endpoint {
    /// The `{guid}` part of the id, which names the endpoint's registry key.
    pub fn guid(&self) -> &str {
        self.id.rsplit('.').next().unwrap_or(&self.id)
    }
}

/// COM for the calling thread (multithreaded apartment). Safe to call repeatedly.
pub fn com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn enumerator() -> Result<IMMDeviceEnumerator> {
    com_init();
    Ok(unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? })
}

fn string_prop(dev: &IMMDevice, key: &PROPERTYKEY) -> String {
    unsafe {
        let Ok(store) = dev.OpenPropertyStore(STGM_READ) else { return String::new() };
        store.GetValue(key).map(|v| v.to_string()).unwrap_or_default()
    }
}

fn take_pwstr(p: PWSTR) -> String {
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

fn endpoint_of(dev: &IMMDevice, flow: Flow) -> Result<Endpoint> {
    let id = take_pwstr(unsafe { dev.GetId()? });
    Ok(Endpoint {
        id,
        flow,
        name: string_prop(dev, &PKEY_Device_FriendlyName),
        device: string_prop(dev, &PKEY_DeviceInterface_FriendlyName),
    })
}

/// Active (plugged in, enabled) endpoints.
pub fn endpoints(flow: Flow) -> Result<Vec<Endpoint>> {
    let e = enumerator()?;
    let coll = unsafe { e.EnumAudioEndpoints(flow.data_flow(), DEVICE_STATE_ACTIVE)? };
    let n = unsafe { coll.GetCount()? };
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let dev = unsafe { coll.Item(i)? };
        out.push(endpoint_of(&dev, flow)?);
    }
    Ok(out)
}

/// The default endpoint for `flow`; `communications` selects the
/// "default communication device" instead of the main default.
pub fn default_endpoint(flow: Flow, communications: bool) -> Result<Option<Endpoint>> {
    let e = enumerator()?;
    let role = if communications { eCommunications } else { eConsole };
    match unsafe { e.GetDefaultAudioEndpoint(flow.data_flow(), role) } {
        Ok(dev) => Ok(Some(endpoint_of(&dev, flow)?)),
        Err(_) => Ok(None),
    }
}

/// Make `id` the default device and default communication device.
pub fn set_default(id: &str) -> Result<()> {
    com_init();
    Ok(policy::set_default(id)?)
}

/// The Nova 5's endpoints, matched by the USB device name.
pub fn find_arctis(flow: Flow) -> Result<Option<Endpoint>> {
    Ok(endpoints(flow)?.into_iter().find(|e| e.device.contains("Arctis Nova 5")))
}

/// Open a shared-mode capture stream on the endpoint `id` and run it
/// briefly, the way any app opening the mic would.
pub fn capture_opens(id: &str) -> Result<()> {
    use windows::Win32::Media::Audio::{IAudioClient, AUDCLNT_SHAREMODE_SHARED};
    let e = enumerator()?;
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let dev = e.GetDevice(windows::core::PCWSTR(wide.as_ptr()))?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let format = client.GetMixFormat()?;
        // 100 ms buffer, in 100 ns units.
        let r = client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 1_000_000, 0, format, None);
        CoTaskMemFree(Some(format as *const _));
        r?;
        client.Start()?;
        std::thread::sleep(std::time::Duration::from_millis(200));
        client.Stop()?;
    }
    Ok(())
}

/// How loud the default output device is playing, for lighting that follows
/// the sound. Opened on the default device at the time; reopen it to follow a
/// change of default.
pub struct PeakMeter(IAudioMeterInformation);

impl PeakMeter {
    pub fn open() -> Result<Self> {
        let e = enumerator()?;
        unsafe {
            let dev = e.GetDefaultAudioEndpoint(eRender, eConsole)?;
            Ok(PeakMeter(dev.Activate(CLSCTX_ALL, None)?))
        }
    }

    /// The current peak sample, 0-1, across channels.
    pub fn peak(&self) -> Result<f32> {
        Ok(unsafe { self.0.GetPeakValue()? })
    }
}
