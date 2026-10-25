//! Installing the mic APO needs administrator rights. The app re-runs its own
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
const RESULT_ARG: &str = "--result";

/// The DLL ships next to the exe (installer resources, or the cargo target dir in development).
pub fn apo_dll() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let dll = dir.join("kit_apo.dll");
    dll.exists().then_some(dll)
}

/// Run `exe <args> --result <file>` elevated and wait for it.
pub fn run_elevated(args: &[&str]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let result_file = std::env::temp_dir().join(format!("kit-setup-{}.txt", std::process::id()));
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
    if action != INSTALL_ARG && action != UNINSTALL_ARG {
        return None;
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
    sysaudio::com_init();
    let mic = sysaudio::find_arctis(sysaudio::Flow::Input).map_err(|e| e.to_string())?;
    if action == INSTALL_ARG {
        let dll = dll.ok_or("missing DLL path")?;
        let mic = mic.ok_or("The Arctis Nova 5 microphone isn't connected. Plug in the base station and try again.")?;
        sysaudio::apo::install(dll, &mic).map_err(|e| e.to_string())
    } else {
        sysaudio::apo::uninstall(mic.as_ref()).map_err(|e| e.to_string())
    }
}
