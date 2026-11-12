//! Installing kit's APOs (mic noise suppression, the headset EQ) needs administrator rights. The app re-runs its own
//! exe elevated (one UAC prompt) with a helper flag; that process does the
//! work without starting the UI and reports back through a result file.

use std::path::{Path, PathBuf};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

pub const INSTALL_ARG: &str = "--install-apo";
pub const UNINSTALL_ARG: &str = "--uninstall-apo";
pub const INSTALL_EQ_ARG: &str = "--install-eq-apo";
pub const UNINSTALL_EQ_ARG: &str = "--uninstall-eq-apo";
/// kit's uninstaller: remove both effects, asking for elevation only when
/// something is installed.
pub const REMOVE_ALL_ARG: &str = "--remove-audio-effects";
/// The elevated half of [`REMOVE_ALL_ARG`].
const REMOVE_ALL_ELEVATED_ARG: &str = "--remove-audio-effects-elevated";
const RESULT_ARG: &str = "--result";
const HELPER_ARGS: [&str; 6] = [INSTALL_ARG, UNINSTALL_ARG, INSTALL_EQ_ARG, UNINSTALL_EQ_ARG, REMOVE_ALL_ARG, REMOVE_ALL_ELEVATED_ARG];

/// The DLL ships next to the exe (installer resources, or the cargo target dir in development).
pub fn apo_dll() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let dll = dir.join("kit_apo.dll");
    dll.exists().then_some(dll)
}

/// Run `exe <args> --result <file>` elevated and wait for it. One at a time:
/// installs restart the audio service and copy the same DLL, so a second
/// one waits for the first.
pub fn run_elevated(args: &[&str]) -> Result<(), String> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let result_file = std::env::temp_dir().join(format!("kit-setup-{}-{nanos:x}.txt", std::process::id()));
    let _ = std::fs::remove_file(&result_file);
    let mut params: Vec<String> = args.iter().map(|a| quote(a)).collect();
    params.push(RESULT_ARG.into());
    params.push(quote(&result_file.to_string_lossy()));
    let file = HSTRING::from(exe.as_os_str());
    let params = HSTRING::from(params.join(" "));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| {
            if e.code().0 as u32 == 0x800704C7 {
                "Cancelled at the Windows permission prompt.".to_string()
            } else {
                e.message()
            }
        })?;
        let mut code = 1u32;
        if WaitForSingleObject(info.hProcess, INFINITE) == WAIT_OBJECT_0 {
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
        }
        let _ = CloseHandle(info.hProcess);
        let message = std::fs::read_to_string(&result_file).unwrap_or_default();
        let _ = std::fs::remove_file(&result_file);
        if code == 0 {
            Ok(())
        } else if message.is_empty() {
            Err(format!("Setup failed (exit code {code})."))
        } else {
            Err(message)
        }
    }
}

fn quote(s: &str) -> String {
    format!("\"{s}\"")
}

/// Entry point for the elevated helper. Returns the process exit code, or
/// `None` when the arguments aren't a helper invocation.
pub fn helper_main(args: &[String]) -> Option<i32> {
    let action = args.get(1)?.as_str();
    if !HELPER_ARGS.contains(&action) {
        return None;
    }
    if action == REMOVE_ALL_ARG {
        // From the uninstaller, which runs as the user: elevate only if needed.
        if !sysaudio::apo::anything_installed() {
            return Some(0);
        }
        return Some(match run_elevated(&[REMOVE_ALL_ELEVATED_ARG]) {
            Ok(()) => 0,
            Err(_) => 1,
        });
    }
    let result_file = args.iter().position(|a| a == RESULT_ARG).and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let result = run_helper(action, args.get(2).filter(|a| *a != RESULT_ARG).map(Path::new));
    let code = match &result {
        Ok(()) => 0,
        Err(e) => {
            if let Some(f) = &result_file {
                let _ = std::fs::write(f, e);
            }
            1
        }
    };
    Some(code)
}

fn run_helper(action: &str, dll: Option<&Path>) -> Result<(), String> {
    use sysaudio::apo::Effect;
    sysaudio::com_init();
    if action == REMOVE_ALL_ELEVATED_ARG {
        return sysaudio::apo::uninstall_all().map_err(|e| e.to_string());
    }
    let (effect, flow, missing) = if action == INSTALL_EQ_ARG || action == UNINSTALL_EQ_ARG {
        (Effect::HeadsetEq, sysaudio::Flow::Output, "The Arctis Nova 5 isn't connected. Plug in the base station and try again.")
    } else {
        (Effect::MicDenoise, sysaudio::Flow::Input, "The Arctis Nova 5 microphone isn't connected. Plug in the base station and try again.")
    };
    let ep = sysaudio::find_arctis(flow).map_err(|e| e.to_string())?;
    if action == INSTALL_ARG || action == INSTALL_EQ_ARG {
        let dll = dll.ok_or("missing DLL path")?;
        let ep = ep.ok_or(missing)?;
        sysaudio::apo::install(dll, &ep, effect).map_err(|e| e.to_string())
    } else {
        // From every endpoint that lists it, the headset plugged in or not.
        sysaudio::apo::uninstall(effect).map_err(|e| e.to_string())
    }
}
