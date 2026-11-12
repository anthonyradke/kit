//! Installing kit's APOs: mic noise suppression on the headset's capture
//! endpoint and the headset EQ on its render endpoint. Both classes live in
//! one DLL.
//!
//! Everything here needs administrator rights except [`status`],
//! [`set_mic_settings`] and [`set_headset_eq`]. Installing:
//! 1. copies the DLL to `Program Files\kit` (audiodg runs as LOCAL
//!    SERVICE and can't read a user profile),
//! 2. registers it as a COM class and as an audio processing object,
//! 3. lists it as the endpoint effect (EFX) in the endpoint's FxProperties,
//!    after backing up whatever values it replaces,
//! 4. restarts the audio service so audiodg picks it up.

use std::path::{Path, PathBuf};
use std::process::Command;

use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WRITE};
use winreg::types::FromRegValue;
use winreg::RegKey;

use crate::{Endpoint, Error, Flow, Result};

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

/// One of kit's effects and the endpoint it goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// On the headset's mic.
    MicDenoise,
    /// On the headset's output.
    HeadsetEq,
}

impl Effect {
    fn guid(self) -> windows::core::GUID {
        match self {
            Effect::MicDenoise => kit_apo::CLSID,
            Effect::HeadsetEq => kit_apo::eq::CLSID,
        }
    }

    fn clsid(self) -> String {
        format!("{{{:?}}}", self.guid())
    }

    fn name(self) -> &'static str {
        match self {
            Effect::MicDenoise => kit_apo::FRIENDLY_NAME,
            Effect::HeadsetEq => kit_apo::eq::FRIENDLY_NAME,
        }
    }

    fn other(self) -> Effect {
        match self {
            Effect::MicDenoise => Effect::HeadsetEq,
            Effect::HeadsetEq => Effect::MicDenoise,
        }
    }

    /// Whether the class is in the registry at all, DLL or not.
    fn listed(self) -> bool {
        hklm().open_subkey(format!(r"SOFTWARE\Classes\CLSID\{}", self.clsid())).is_ok()
    }

    /// Whether the class is registered with its DLL in place.
    fn registered(self) -> bool {
        let dll: Option<String> = hklm()
            .open_subkey(format!(r"SOFTWARE\Classes\CLSID\{}\InprocServer32", self.clsid()))
            .and_then(|k| k.get_value(""))
            .ok();
        dll.is_some_and(|p| Path::new(&p).exists())
    }
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

/// Access for an endpoint's FxProperties key. Windows only grants
/// administrators query and set-value there; asking for `KEY_WRITE` (which
/// includes creating subkeys) is denied.
const FX_ACCESS: u32 = KEY_QUERY_VALUE | KEY_SET_VALUE;

fn fx_path(ep: &Endpoint) -> String {
    fx_key(ep.flow, ep.guid())
}

fn fx_key(flow: Flow, guid: &str) -> String {
    format!(r"{MMDEVICES}\{}\{guid}\FxProperties", flow.registry_name())
}

/// An endpoint whose FxProperties lists one of kit's effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    pub flow: Flow,
    /// The endpoint's registry key name, `{guid}`.
    pub guid: String,
    pub effect: Effect,
}

/// Every endpoint, connected or not, that lists one of kit's effects. An
/// unplugged headset and the old endpoints a driver update leaves behind
/// count too: their settings stay in the registry. Readable without
/// administrator rights.
pub fn attached() -> Vec<Attached> {
    let mut out = Vec::new();
    for flow in [Flow::Output, Flow::Input] {
        let Ok(root) = hklm().open_subkey(format!(r"{MMDEVICES}\{}", flow.registry_name())) else { continue };
        for guid in root.enum_keys().filter_map(|k| k.ok()) {
            let Ok(efx) = hklm().open_subkey(fx_key(flow, &guid)).and_then(|k| k.get_value::<String, _>(EFX)) else { continue };
            for effect in [Effect::MicDenoise, Effect::HeadsetEq] {
                if efx.eq_ignore_ascii_case(&effect.clsid()) {
                    out.push(Attached { flow, guid: guid.clone(), effect });
                }
            }
        }
    }
    out
}

/// Whether any of kit's effects is registered or listed on an endpoint.
pub fn anything_installed() -> bool {
    Effect::MicDenoise.listed() || Effect::HeadsetEq.listed() || !attached().is_empty()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The COM class is registered and its DLL exists.
    pub registered: bool,
    /// The endpoint lists our APO in its FxProperties.
    pub on_endpoint: bool,
}

pub fn status(ep: &Endpoint, effect: Effect) -> Status {
    let on_endpoint = hklm()
        .open_subkey(fx_path(ep))
        .and_then(|k| k.get_value::<String, _>(EFX))
        .is_ok_and(|v| v.eq_ignore_ascii_case(&effect.clsid()));
    Status { registered: effect.registered(), on_endpoint }
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

/// The headset EQ the APO runs: `curve` in Equalizer APO's text format,
/// or passthrough when `on` is false. Works unelevated once installed.
pub fn set_headset_eq(on: bool, curve: &str) -> Result<()> {
    let key = hklm().open_subkey_with_flags(SETTINGS_KEY, KEY_WRITE)?;
    // The curve first, so the APO never runs the old curve after "on".
    key.set_value("HeadsetEq", &curve)?;
    key.set_value("HeadsetEqOn", &(on as u32))?;
    Ok(())
}

pub fn install(dll_source: &Path, ep: &Endpoint, effect: Effect) -> Result<()> {
    let dir = install_dir();
    std::fs::create_dir_all(&dir)?;
    let data = data_dir();
    std::fs::create_dir_all(&data)?;
    // LOCAL SERVICE (audiodg) may append to the log.
    run("icacls", &[data.to_str().unwrap(), "/grant", "*S-1-5-19:(OI)(CI)M", "/q"])?;
    let _ = std::fs::OpenOptions::new().create(true).append(true).open(log_path());

    // Only an endpoint that opens now says anything about the APO afterwards
    // (a mic also fails when Windows' privacy settings block desktop apps).
    let opened_before = crate::stream_opens(&ep.id).is_ok();
    // The DLL may be loaded by audiodg; stop the audio service to replace it.
    restart_audio(|| {
        std::fs::copy(dll_source, dir.join(DLL_NAME))?;
        register_com(&dir.join(DLL_NAME), effect)?;
        register_settings_key()?;
        attach(ep, effect)?;
        Ok(())
    })?;
    if !opened_before {
        return Ok(());
    }
    // A broken APO doesn't crash anything: apps just can't open the device.
    // Check, giving the restarted audio service a moment, and undo it if so.
    let mut last = Ok(());
    for _ in 0..10 {
        last = crate::stream_opens(&ep.id);
        if last.is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let err = last.unwrap_err();
    uninstall(effect)?;
    Err(Error::Msg(match effect {
        Effect::MicDenoise => format!(
            "Noise suppression stopped the mic from opening ({err}), so it was removed again. The mic works as before."
        ),
        Effect::HeadsetEq => format!(
            "kit's EQ stopped the headset from playing ({err}), so it was removed again. The headset works as before."
        ),
    }))
}

/// Remove `effect`: from every endpoint that lists it, connected or not (an
/// endpoint left naming a class that's gone can't be opened), then the
/// class, then the DLL once neither effect uses it.
pub fn uninstall(effect: Effect) -> Result<()> {
    restart_audio(|| remove(effect))
}

/// Both effects and everything kit keeps outside its own folder (the
/// settings key, the DLL's folder, the APO log), for kit's uninstaller.
pub fn uninstall_all() -> Result<()> {
    restart_audio(|| {
        remove(Effect::MicDenoise)?;
        remove(Effect::HeadsetEq)?;
        if let Ok(software) = hklm().open_subkey_with_flags("SOFTWARE", KEY_WRITE) {
            let _ = software.delete_subkey_all("kit");
        }
        let _ = std::fs::remove_dir_all(install_dir());
        let _ = std::fs::remove_dir_all(data_dir());
        Ok(())
    })
}

/// Put kit's effects back in working order, saying what was done:
/// - an endpoint left naming an effect that's gone, or one that isn't the
///   headset's (a driver update recreates endpoints under new ids), drops it;
/// - a missing or older DLL is replaced by `dll_source` (this kit's), or the
///   effect is removed when there's no DLL to put back;
/// - an installed effect missing from the headset's endpoint goes back on;
/// - the settings key and the backups get their permissions back.
///
/// The audio service restarts either way, which also starts afresh an APO
/// that fell back to passthrough after an error.
pub fn repair(dll_source: Option<&Path>, output: Option<&Endpoint>, input: Option<&Endpoint>) -> Result<Vec<String>> {
    let mut done = Vec::new();
    restart_audio(|| {
        if !anything_installed() {
            return Ok(());
        }
        register_settings_key()?;
        let dll = install_dir().join(DLL_NAME);
        let mut dll_updated = false;
        for effect in [Effect::MicDenoise, Effect::HeadsetEq] {
            let ep = match effect {
                Effect::MicDenoise => input,
                Effect::HeadsetEq => output,
            };
            let listed = effect.listed();
            for a in attached().into_iter().filter(|a| a.effect == effect) {
                let current = ep.is_some_and(|e| e.flow == a.flow && e.guid().eq_ignore_ascii_case(&a.guid));
                if !listed || (ep.is_some() && !current) {
                    detach(a.flow, &a.guid)?;
                    done.push(format!("Took {} off an audio device it no longer belongs on.", effect.name()));
                }
            }
            if !listed {
                continue;
            }
            match dll_source {
                Some(src) if !dll.exists() || !is_current(src) => {
                    if !dll_updated {
                        std::fs::create_dir_all(install_dir())?;
                        std::fs::copy(src, &dll)?;
                        dll_updated = true;
                        done.push("Put this version of kit's audio effects in place.".into());
                    }
                }
                None if !dll.exists() => {
                    remove(effect)?;
                    done.push(format!("Removed {}: its DLL was missing. Install it again from kit.", effect.name()));
                    continue;
                }
                _ => {}
            }
            register_com(&dll, effect)?;
            if let Some(ep) = ep {
                if !status(ep, effect).on_endpoint {
                    attach(ep, effect)?;
                    done.push(format!("Put {} back on {}.", effect.name(), ep.name));
                }
            }
        }
        Ok(())
    })?;
    Ok(done)
}

fn remove(effect: Effect) -> Result<()> {
    for a in attached().into_iter().filter(|a| a.effect == effect) {
        detach(a.flow, &a.guid)?;
    }
    let classes = hklm().open_subkey_with_flags(r"SOFTWARE\Classes", KEY_WRITE)?;
    let _ = classes.delete_subkey_all(format!(r"CLSID\{}", effect.clsid()));
    let _ = classes.delete_subkey_all(format!(r"AudioEngine\AudioProcessingObjects\{}", effect.clsid()));
    // The other effect shares the DLL.
    if !effect.other().registered() {
        let _ = std::fs::remove_file(install_dir().join(DLL_NAME));
    }
    Ok(())
}

fn register_com(dll: &Path, effect: Effect) -> Result<()> {
    let id = effect.clsid();
    let (server, _) = hklm().create_subkey(format!(r"SOFTWARE\Classes\CLSID\{id}\InprocServer32"))?;
    server.set_value("", &dll.to_string_lossy().to_string())?;
    server.set_value("ThreadingModel", &"Both")?;
    let (class, _) = hklm().create_subkey(format!(r"SOFTWARE\Classes\CLSID\{id}"))?;
    class.set_value("", &effect.name())?;

    let props = kit_apo::registration_properties(effect.guid(), effect.name());
    let (apo, _) = hklm().create_subkey(format!(r"SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\{id}"))?;
    apo.set_value("FriendlyName", &effect.name())?;
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
    // Off until the app sends a curve.
    if key.get_value::<u32, _>("HeadsetEqOn").is_err() {
        key.set_value("HeadsetEqOn", &0u32)?;
    }
    // SYSTEM and admins: full. Users: read/write. LOCAL SERVICE (audiodg): read.
    set_key_sddl(&key, "D:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KRKW;;;BU)(A;OICI;KR;;;LS)")?;
    // Where the APOs report what they're doing: LOCAL SERVICE writes, users read.
    let (status, _) = hklm().create_subkey(kit_apo::health::KEY)?;
    set_key_sddl(&status, "D:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KR;;;BU)(A;OICI;KRKW;;;LS)")?;
    protect_backups()
}

/// What the APOs in the audio engine last reported about themselves (JSON,
/// see `kit_apo::health`), if they have. Readable without administrator rights.
pub fn health() -> Option<String> {
    hklm().open_subkey(kit_apo::health::KEY).ok()?.get_value(kit_apo::health::VALUE).ok()
}

/// What the uninstaller writes back into an endpoint's FxProperties comes
/// from the backups, so only administrators may change them (they used to
/// inherit the settings key's write access for users).
const BACKUP_SDDL: &str = "D:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KR;;;BU)";

fn protect_backups() -> Result<()> {
    let (backups, _) = hklm().create_subkey(BACKUP_KEY)?;
    set_key_sddl(&backups, BACKUP_SDDL)?;
    for guid in backups.enum_keys().filter_map(|k| k.ok()) {
        if let Ok(k) = hklm().open_subkey_with_flags(format!(r"{BACKUP_KEY}\{guid}"), KEY_ALL_ACCESS) {
            set_key_sddl(&k, BACKUP_SDDL)?;
        }
    }
    Ok(())
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

fn attach(ep: &Endpoint, effect: Effect) -> Result<()> {
    let fx = hklm().open_subkey_with_flags(fx_path(ep), FX_ACCESS)?;
    protect_backups()?;
    let (backup, _) = hklm().create_subkey(format!(r"{BACKUP_KEY}\{}", ep.guid()))?;
    set_key_sddl(&backup, BACKUP_SDDL)?;
    let already = status(ep, effect).on_endpoint;
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
    fx.set_value(EFX, &effect.clsid())?;
    fx.set_value(EFX_COMPOSITE, &vec![effect.clsid()])?;
    fx.set_value(EFX_MODES, &vec![MODE_DEFAULT.to_string()])?;
    Ok(())
}

/// Put back what an endpoint's FxProperties had before kit (or clear kit's
/// values when there's no backup).
fn detach(flow: Flow, guid: &str) -> Result<()> {
    let Ok(fx) = hklm().open_subkey_with_flags(fx_key(flow, guid), FX_ACCESS) else { return Ok(()) };
    let backup = hklm().open_subkey(format!(r"{BACKUP_KEY}\{guid}")).ok();
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
        let _ = k.delete_subkey_all(guid);
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
