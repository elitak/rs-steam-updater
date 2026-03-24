use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

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
            println!(["steam] Using Steam.exe at: {}", path.display());
            return Some(path.clone());
        }
    }

    println!(["steam] Not found in well-known locations; falling back to PATH search ...");
    let result = which_steam();
    match &result {
        Some(p) => println!("[steam] Found via PATH: {}", p.display()),
        None => println!("[steam] Steam.exe not found anywhere on PATH either."),
    }
    result
}

/// Attempt a graceful shutdown of Steam via `Steam.exe -shutdown`, wait 4 s,
/// then force-kill any remaining `steam.exe` processes.
pub fn shutdown_steam(steam_exe: Option<&PathBuf>) {
    if let Some(exe) = steam_exe {
        let _ = Command::new(exe).arg("-shutdown").spawn();
        thread::sleep(Duration::from_secs(4));
    }

    #[cfg(windows)]
    {
        for pid in find_steam_pids() {
            force_kill(pid);
        }
        thread::sleep(Duration::from_secs(2));
    }

    println!("[steam] Steam stopped.");
}

/// Launch Steam and log in as the given account.
/// Prints the full executable path, checks it exists on disk, reports the
/// spawn result (PID on success, OS error on failure), and confirms completion.
pub fn launch_steam(steam_exe: &PathBuf, login: &str, password: &str) {
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
            eprintln!("[steam]   Hint   : Check that the path exists and you have execute permission.");
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
        if !handle.is_null() {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}
