use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::{Duration, Instant};

/// Returns `true` if at least one process named `steam.exe` is running.
pub fn is_steam_running() -> bool {
    #[cfg(windows)]
    {
        !find_steam_pids().is_empty()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Search well-known locations for `Steam.exe`, returning the first found.
/// Logs every candidate path and whether it exists on disk.
pub fn find_steam_exe() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![
        PathBuf::from(r"C:\Program Files (x86)\Steam\Steam.exe"),
        PathBuf::from(r"C:\Program Files\Steam\Steam.exe"),
    ];

    if let Ok(pf) = std::env::var("ProgramFiles") {
        candidates.push(PathBuf::from(&pf).join("Steam").join("Steam.exe"));
    }
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        candidates.push(PathBuf::from(&pf86).join("Steam").join("Steam.exe"));
    }

    println!(
        "[steam] Searching for Steam.exe in {} candidate location(s):",
        candidates.len()
    );
    for path in &candidates {
        let exists = path.exists();
        println!(
            "[steam]   {} -> {}",
            path.display(),
            if exists { "FOUND" } else { "not found" }
        );
        if exists {
            println!("[steam] Using Steam.exe at: {}", path.display());
            return Some(path.clone());
        }
    }

    println!("[steam] Not found in well-known locations; falling back to PATH search ...");
    let result = which_steam();
    match &result {
        Some(p) => println!("[steam] Found via PATH: {}", p.display()),
        None => println!("[steam] Steam.exe not found anywhere on PATH either."),
    }
    result
}

/// Gracefully shuts down Steam.
///
/// On Windows: sends `WM_CLOSE` to all windows owned by each Steam PID, then
/// waits up to 5 seconds for the processes to exit.  Any that remain after the
/// timeout are force-killed.
/// On non-Windows: no-op.
pub fn shutdown_steam(_steam_exe: Option<&PathBuf>) {
    #[cfg(windows)]
    shutdown_steam_windows();
}

/// Launch Steam from the given executable path.
/// Prints the full executable path, checks it exists on disk, reports the
/// spawn result (PID on success, OS error on failure), and confirms completion.
pub fn launch_steam(steam_exe: &Path, login: &str, password: &str) {
    println!("[steam] -- Relaunch sequence -------------------------------------");
    println!("[steam] Executable  : {}", steam_exe.display());
    println!("[steam] Exists on disk: {}", steam_exe.exists());
    println!("[steam] Arguments   : -login {} ****", login);
    println!("[steam] Spawning process ...");

    match Command::new(steam_exe)
        .args(["-login", login, password])
        .spawn()
    {
        Ok(child) => {
            println!(
                "[steam] Spawn succeeded -- Steam process started with PID {}.",
                child.id()
            );
            println!("[steam] Steam is now running in the background.");
        }
        Err(e) => {
            eprintln!("[steam] ERROR: Failed to spawn Steam process!");
            eprintln!("[steam]   Path   : {}", steam_exe.display());
            eprintln!("[steam]   Reason : {}", e);
            eprintln!(
                "[steam]   Hint   : Check that the path exists and you have execute permission."
            );
        }
    }

    println!("[steam] -- End of relaunch sequence ------------------------------");
}

// ── private helpers ──────────────────────────────────────────────────────────

fn which_steam() -> Option<PathBuf> {
    // On Windows PATH entries are separated by ';'
    let separator = if cfg!(windows) { ';' } else { ':' };
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(separator) {
            let candidate = PathBuf::from(dir).join("Steam.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Sends `WM_CLOSE` to every window owned by a Steam PID, then polls until
/// all Steam processes have exited (up to 5 seconds).  Remaining processes are
/// force-killed.
#[cfg(windows)]
fn shutdown_steam_windows() {
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows;

    let steam_pids = find_steam_pids();
    if steam_pids.is_empty() {
        println!("[steam] No Steam processes found to shut down.");
        return;
    }

    println!("[steam] Sending WM_CLOSE to all Steam windows ...");

    // Pass the Vec<u32> via LPARAM (a pointer-sized integer).
    let raw: *mut Vec<u32> = Box::into_raw(Box::new(steam_pids));
    unsafe {
        EnumWindows(Some(enum_steam_windows_proc), raw as isize);
        drop(Box::from_raw(raw));
    }

    // Poll up to 5 s for all Steam PIDs to disappear.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = find_steam_pids();
        if remaining.is_empty() {
            break;
        }
        if Instant::now() >= deadline {
            eprintln!("[steam] Timed out waiting for Steam to exit; force-killing ...");
            for pid in remaining {
                force_kill(pid);
            }
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }

    println!("[steam] Steam stopped.");
}

/// `EnumWindows` callback: posts `WM_CLOSE` to each top-level window whose
/// owning process is in the Steam PID list passed via `lparam`.
#[cfg(windows)]
unsafe extern "system" fn enum_steam_windows_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::BOOL {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
    };

    let pids = if lparam == 0 {
        return 1;
    } else {
        &*(lparam as *const Vec<u32>)
    };
    let mut process_id: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut process_id);
    if pids.contains(&process_id) {
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
    }
    1 // TRUE – continue enumeration
}

#[cfg(windows)]
fn find_steam_pids() -> Vec<u32> {
    use std::mem;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut pids = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return pids;
        }

        let mut entry: PROCESSENTRY32W = mem::zeroed();
        entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                // szExeFile is a null-terminated UTF-16 array
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if name.eq_ignore_ascii_case("steam.exe") {
                    pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    pids
}

#[cfg(windows)]
fn force_kill(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_TERMINATE,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle != 0 {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}