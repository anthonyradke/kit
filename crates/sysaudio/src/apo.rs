//! Installing kit's noise suppression APO on one capture endpoint.
//!
//! Everything here needs administrator rights except [`status`] and
//! [`set_mic_settings`]. Installing:
//! 1. copies the DLL to `Program Files\kit` (audiodg runs as LOCAL
//!    SERVICE and can't read a user profile),
//! 2. registers it as a COM class and as an audio processing object,
//! 3. lists it as the endpoint effect (EFX) in the endpoint's FxProperties,
//!    after backing up whatever values it replaces,
//! 4. restarts the audio service so audiodg picks it up.

use std::path::{Path, PathBuf};
use std::process::Command;

use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
use winreg::types::FromRegValue;
use winreg::RegKey;

use crate::{Endpoint, Error, Result};

const MMDEVICES: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio";
const SETTINGS_KEY: &str = r"SOFTWARE\kit";
const BACKUP_KEY: &str = r"SOFTWARE\kit\FxBackup";
const DLL_NAME: &str = "kit_apo.dll";
/// `PKEY_FX_EndpointEffectClsid`
const EFX: &str = "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},7";
/// `PKEY_CompositeFX_EndpointEffectClsid`
const EFX_COMPOSITE: &str = "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},15";
/// `PKEY_EFX_ProcessingModes_Supported_For_Streaming`
const EFX_MODES: &str = "{d3993a3f-99c2-4402-b5ec-a92a0367664b},7";
const MODE_DEFAULT: &str = "{C18E2F7E-933D-4965-B7D1-1EEF228D2AF3}";
const IID_AUDIO_PROCESSING_OBJECT: &str = "{FD7F2B29-24D0-4B5C-B177-592C39F9CA10}";
/// Values we write in FxProperties, so uninstall knows what to restore.
const TOUCHED: [&str; 3] = [EFX, EFX_COMPOSITE, EFX_MODES];

fn clsid() -> String {
    format!("{{{:?}}}", kit_apo::CLSID)
}

fn hklm() -> RegKey {
    RegKey::predef(HKEY_LOCAL_MACHINE)
}

pub fn install_dir() -> PathBuf {
    let base = std::env::var_os("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into());
    PathBuf::from(base).join("kit")
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("ProgramData").unwrap_or_else(|| r"C:\ProgramData".into());
    PathBuf::from(base).join("kit")
}

pub fn log_path() -> PathBuf {
    data_dir().join("apo.log")
}

fn fx_path(ep: &Endpoint) -> String {
    format!(r"{MMDEVICES}\{}\{}\FxProperties", ep.flow.registry_name(), ep.guid())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The COM class is registered and its DLL exists.
    pub registered: bool,
    /// The endpoint lists our APO in its FxProperties.
    pub on_endpoint: bool,
}

pub fn status(ep: &Endpoint) -> Status {
    let dll: Option<String> = hklm()
        .open_subkey(format!(r"SOFTWARE\Classes\CLSID\{}\InprocServer32", clsid()))
        .and_then(|k| k.get_value(""))
        .ok();
    let registered = dll.is_some_and(|p| Path::new(&p).exists());
    let on_endpoint = hklm()
        .open_subkey(fx_path(ep))
        .and_then(|k| k.get_value::<String, _>(EFX))
        .is_ok_and(|v| v.eq_ignore_ascii_case(&clsid()));
    Status { registered, on_endpoint }
}

/// Whether the installed DLL is the one at `bundled` (the copy next to the
/// app). Installing doesn't happen on app updates, since it needs an
/// administrator prompt, so after an update the installed one can be old.
/// `std::fs::copy` keeps the modification time, so size and time decide;
/// when only the times differ the contents are compared, once per change.
pub fn is_current(bundled: &Path) -> bool {
    use std::sync::Mutex;
    type Key = (u64, Option<std::time::SystemTime>, u64, Option<std::time::SystemTime>);
    static LAST: Mutex<Option<(Key, bool)>> = Mutex::new(None);
    let installed = install_dir().join(DLL_NAME);
    let (Ok(a), Ok(b)) = (std::fs::metadata(&installed), std::fs::metadata(bundled)) else { return true };
    let key = (a.len(), a.modified().ok(), b.len(), b.modified().ok());
    if a.len() != b.len() {
        return false;
    }
    if key.1.is_some() && key.1 == key.3 {
        return true;
    }
    let mut last = LAST.lock().unwrap();
    if let Some((k, current)) = *last {
        if k == key {
            return current;
        }
    }
    let current = matches!((std::fs::read(&installed), std::fs::read(bundled)), (Ok(x), Ok(y)) if x == y);
    *last = Some((key, current));
    current
}

/// Live settings read by the APO. Works unelevated once installed.
pub fn set_mic_settings(enabled: bool, strength_percent: u32) -> Result<()> {
    let key = hklm().open_subkey_with_flags(SETTINGS_KEY, KEY_WRITE)?;
    key.set_value("MicDenoise", &(enabled as u32))?;
    key.set_value("MicStrength", &strength_percent.min(100))?;
    Ok(())
}

pub fn install(dll_source: &Path, ep: &Endpoint) -> Result<()> {
    let dir = install_dir();
    std::fs::create_dir_all(&dir)?;
    let data = data_dir();
    std::fs::create_dir_all(&data)?;
    // LOCAL SERVICE (audiodg) may append to the log.
    run("icacls", &[data.to_str().unwrap(), "/grant", "*S-1-5-19:(OI)(CI)M", "/q"])?;
    let _ = std::fs::OpenOptions::new().create(true).append(true).open(log_path());

    // The DLL may be loaded by audiodg; stop the audio service to replace it.
    restart_audio(|| {
        std::fs::copy(dll_source, dir.join(DLL_NAME))?;
        register_com(&dir.join(DLL_NAME))?;
        register_settings_key()?;
        attach(ep)?;
        Ok(())
    })
}

pub fn uninstall(ep: Option<&Endpoint>) -> Result<()> {
    restart_audio(|| {
        if let Some(ep) = ep {
            detach(ep)?;
        }
        let classes = hklm().open_subkey_with_flags(r"SOFTWARE\Classes", KEY_WRITE)?;
        let _ = classes.delete_subkey_all(format!(r"CLSID\{}", clsid()));
        let _ = classes.delete_subkey_all(format!(r"AudioEngine\AudioProcessingObjects\{}", clsid()));
        let _ = std::fs::remove_file(install_dir().join(DLL_NAME));
        Ok(())
    })
}

fn register_com(dll: &Path) -> Result<()> {
    let id = clsid();
    let (server, _) = hklm().create_subkey(format!(r"SOFTWARE\Classes\CLSID\{id}\InprocServer32"))?;
    server.set_value("", &dll.to_string_lossy().to_string())?;
    server.set_value("ThreadingModel", &"Both")?;
    let (class, _) = hklm().create_subkey(format!(r"SOFTWARE\Classes\CLSID\{id}"))?;
    class.set_value("", &kit_apo::FRIENDLY_NAME)?;

    let props = kit_apo::registration_properties();
    let (apo, _) = hklm().create_subkey(format!(r"SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\{id}"))?;
    apo.set_value("FriendlyName", &kit_apo::FRIENDLY_NAME)?;
    apo.set_value("Copyright", &"MIT License")?;
    apo.set_value("MajorVersion", &props.u32MajorVersion)?;
    apo.set_value("MinorVersion", &props.u32MinorVersion)?;
    apo.set_value("Flags", &(props.Flags.0 as u32))?;
    apo.set_value("MinInputConnections", &props.u32MinInputConnections)?;
    apo.set_value("MaxInputConnections", &props.u32MaxInputConnections)?;
    apo.set_value("MinOutputConnections", &props.u32MinOutputConnections)?;
    apo.set_value("MaxOutputConnections", &props.u32MaxOutputConnections)?;
    apo.set_value("MaxInstances", &props.u32MaxInstances)?;
    apo.set_value("NumAPOInterfaces", &1u32)?;
    apo.set_value("APOInterface0", &IID_AUDIO_PROCESSING_OBJECT)?;
    Ok(())
}

/// Create the settings key, writable by normal users so the app needn't be elevated.
fn register_settings_key() -> Result<()> {
    let (key, _) = hklm().create_subkey(SETTINGS_KEY)?;
    if key.get_value::<u32, _>("MicDenoise").is_err() {
        key.set_value("MicDenoise", &1u32)?;
    }
    if key.get_value::<u32, _>("MicStrength").is_err() {
        key.set_value("MicStrength", &90u32)?;
    }
    // SYSTEM and admins: full. Users: read/write. LOCAL SERVICE (audiodg): read.
    set_key_sddl(&key, "D:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KRKW;;;BU)(A;OICI;KR;;;LS)")
}

fn set_key_sddl(key: &RegKey, sddl: &str) -> Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
    use windows::Win32::System::Registry::{RegSetKeySecurity, HKEY};

    let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(wide.as_ptr()), SDDL_REVISION_1, &mut sd, None)?;
        let r = RegSetKeySecurity(HKEY(key.raw_handle() as _), DACL_SECURITY_INFORMATION, sd);
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        r.ok()?;
    }
    Ok(())
}

fn attach(ep: &Endpoint) -> Result<()> {
    let fx = hklm().open_subkey_with_flags(fx_path(ep), KEY_READ | KEY_WRITE)?;
    let (backup, _) = hklm().create_subkey(format!(r"{BACKUP_KEY}\{}", ep.guid()))?;
    let already = status(ep).on_endpoint;
    for name in TOUCHED {
        // Back up the original values once; reinstalling must not back up our own.
        if already {
            break;
        }
        match fx.get_raw_value(name) {
            Ok(v) => backup.set_raw_value(name, &v)?,
            Err(_) => backup.set_value(name, &"<absent>")?,
        }
    }
    fx.set_value(EFX, &clsid())?;
    fx.set_value(EFX_COMPOSITE, &vec![clsid()])?;
    fx.set_value(EFX_MODES, &vec![MODE_DEFAULT.to_string()])?;
    Ok(())
}

fn detach(ep: &Endpoint) -> Result<()> {
    let Ok(fx) = hklm().open_subkey_with_flags(fx_path(ep), KEY_READ | KEY_WRITE) else { return Ok(()) };
    let backup = hklm().open_subkey(format!(r"{BACKUP_KEY}\{}", ep.guid())).ok();
    for name in TOUCHED {
        let saved = backup.as_ref().and_then(|b| b.get_raw_value(name).ok());
        match saved {
            Some(v) if String::from_reg_value(&v).ok().as_deref() != Some("<absent>") => fx.set_raw_value(name, &v)?,
            _ => {
                let _ = fx.delete_value(name);
            }
        }
    }
    if let Ok(k) = hklm().open_subkey_with_flags(BACKUP_KEY, KEY_WRITE) {
        let _ = k.delete_subkey_all(ep.guid());
    }
    Ok(())
}

/// Run `f` with the Windows audio service stopped, then start it again
/// (even if `f` failed). Audio drops out for about a second.
fn restart_audio(f: impl FnOnce() -> Result<()>) -> Result<()> {
    run("net", &["stop", "audiosrv", "/y"])?;
    let r = f();
    let started = run("net", &["start", "audiosrv"]);
    r.and(started)
}

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let out = Command::new(cmd).args(args).output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::Msg(format!(
            "{cmd} {} failed: {}{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}
