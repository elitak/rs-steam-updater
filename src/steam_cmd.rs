use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

const STEAMCMD_URL: &str =
    "https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip";

/// PID of the currently running SteamCMD child process, or 0 when idle.
static STEAMCMD_PID: AtomicU32 = AtomicU32::new(0);

/// Kill the currently running SteamCMD process, if any.
pub fn kill_current_steamcmd() {
    let pid = STEAMCMD_PID.load(Ordering::SeqCst);
    if pid != 0 {
        kill_pid(pid);
    }
}

#[cfg(windows)]
fn kill_pid(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle != 0 {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}

#[cfg(not(windows))]
fn kill_pid(_pid: u32) {
    // Non-Windows stub; this application targets Windows.
}

// ── ACF helpers ──────────────────────────────────────────────────────────────

/// Parse the `"installdir"` value from a SteamCMD-generated appmanifest ACF
/// or from `+app_info_print` output.  Returns `None` if not found.
fn parse_installdir(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("\"installdir\"") {
            let rest = t["\"installdir\"".len()..].trim();
            if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
                return Some(rest[1..rest.len() - 1].to_string());
            }
        }
    }
    None
}

/// Parse the `"installdir"` value from a SteamCMD-generated appmanifest ACF.
fn read_acf_installdir(acf_path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(acf_path).ok()?;
    parse_installdir(&text)
}



/// Returns `%ProgramData%\SteamCMD` (defaults to `C:\ProgramData\SteamCMD`).
pub fn steamcmd_dir() -> PathBuf {
    let base = std::env::var("ProgramData")
        .unwrap_or_else(|_| r"C:\ProgramData".to_string());
    PathBuf::from(base).join("SteamCMD")
}

/// Returns the path to `steamcmd.exe`.
pub fn steamcmd_exe() -> PathBuf {
    steamcmd_dir().join("steamcmd.exe")
}

/// Ensures `<steamcmd_dir>\steamapps` is a directory symlink pointing at
/// `<library_root>\steamapps`.
///
/// With this symlink in place SteamCMD writes game files directly into the
/// target library, so no post-update file movement is required.
///
/// If `<steamcmd_dir>\steamapps` already exists as a real directory (left over
/// from a previous run of the old move-based code), any `.acf` manifest files
/// inside it are copied to the target before the directory is removed and the
/// symlink is created.
pub fn setup_steamapps_symlink(library_root: &str) -> Result<(), Box<dyn std::error::Error>> {
    let link = steamcmd_dir().join("steamapps");
    let target = PathBuf::from(library_root).join("steamapps");

    // Ensure the target directory exists.
    std::fs::create_dir_all(&target)?;

    // Inspect whatever currently lives at the link path.
    match std::fs::symlink_metadata(&link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let current = std::fs::read_link(&link)?;
            if current == target {
                println!(
                    "[symlink] steamapps link already correct ({} -> {}).",
                    link.display(),
                    target.display()
                );
                return Ok(());
            }
            // Wrong target — remove and fall through to recreate.
            println!(
                "[symlink] Updating steamapps link (was {}).",
                current.display()
            );
            #[cfg(windows)]
            std::fs::remove_dir(&link)?;
            #[cfg(not(windows))]
            std::fs::remove_file(&link)?;
        }
        Ok(_) => {
            // Real directory from a previous run — migrate ACF files then remove.
            println!(
                "[symlink] Migrating existing steamapps directory to {} ...",
                target.display()
            );
            for entry in std::fs::read_dir(&link)? {
                let entry = entry?;
                let name = entry.file_name();
                if name.to_string_lossy().ends_with(".acf") {
                    let dest = target.join(&name);
                    if dest.exists() {
                        println!("[symlink] Keeping existing {}", dest.display());
                    } else {
                        std::fs::copy(entry.path(), &dest)?;
                        println!("[symlink] Migrated {}", dest.display());
                    }
                }
            }
            std::fs::remove_dir_all(&link)?;
        }
        Err(_) => {
            // Does not exist — nothing to clean up.
        }
    }

    println!(
        "[symlink] {} -> {}",
        link.display(),
        target.display()
    );
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&target, &link)?;
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&target, &link)?;

    println!("[symlink] steamapps symlink created successfully.");
    Ok(())
}

/// Download and install SteamCMD if it is not already present.
/// Runs `steamcmd.exe +quit` once for the initial self-update.
pub fn install_steam_cmd() -> Result<(), Box<dyn std::error::Error>> {
    let exe = steamcmd_exe();
    if exe.exists() {
        println!(
            "[bootstrap] SteamCMD already installed at {}",
            exe.display()
        );
        return Ok(());
    }

    println!(
        "[bootstrap] Downloading SteamCMD from {} ...",
        STEAMCMD_URL
    );
    let dir = steamcmd_dir();
    std::fs::create_dir_all(&dir)?;

    // Download to %TEMP%\steamcmd.zip
    let temp_dir = std::env::var("TEMP")
        .or_else(|_| std::env::var("TMP"))
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    let zip_path = PathBuf::from(&temp_dir).join("steamcmd.zip");

    let bytes = reqwest::blocking::get(STEAMCMD_URL)?
        .error_for_status()?
        .bytes()?;
    std::fs::write(&zip_path, &bytes)?;

    // Extract to steamcmd_dir
    println!("[bootstrap] Extracting to {} ...", dir.display());
    let file = std::fs::File::open(&zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let out_path = dir.join(entry.name());
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            // Defensively ensure the parent directory exists for archives that
            // omit explicit directory entries before their file contents.
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }
    std::fs::remove_file(&zip_path).ok();

    // Run initial self-update
    println!("[bootstrap] Running initial SteamCMD self-update ...");
    Command::new(&exe).arg("+quit").status()?;

    println!("[bootstrap] SteamCMD installed successfully.");
    Ok(())
}

/// Run `steamcmd.exe` to download/update a single app.
///
/// Because `<steamcmd_dir>\steamapps` is symlinked to `<library_root>\steamapps`
/// (set up by [`setup_steamapps_symlink`] before the first call), SteamCMD
/// writes game files directly into the library — no post-update file movement
/// is needed.
///
/// The spawned SteamCMD process PID is stored in [`STEAMCMD_PID`] so that a
/// Ctrl-C handler can terminate it promptly.
pub fn update_app(
    login: &str,
    password: &str,
    app_id: u32,
    library_root: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  [update] AppID {}  (account: {})", app_id, login);

    // ── Run the update ────────────────────────────────────────────────────────
    let exe = steamcmd_exe();
    let mut child = Command::new(&exe)
        .args([
            "+login",
            login,
            password,
            "+app_update",
            &app_id.to_string(),
            "validate",
            "+quit",
        ])
        .spawn()?;

    STEAMCMD_PID.store(child.id(), Ordering::SeqCst);
    let status = child.wait()?;
    STEAMCMD_PID.store(0, Ordering::SeqCst);

    if !status.success() {
        eprintln!(
            "  [warning] SteamCMD exited with code {:?} for AppID {}",
            status.code(),
            app_id
        );
    } else {
        println!("  [done]   AppID {} updated successfully.", app_id);
    }

    // ── Verify ────────────────────────────────────────────────────────────────
    let library_steamapps = PathBuf::from(library_root).join("steamapps");
    let acf = library_steamapps.join(format!("appmanifest_{}.acf", app_id));
    if let Some(installdir) = read_acf_installdir(&acf) {
        let game_dir = library_steamapps.join("common").join(&installdir);
        if game_dir.exists() {
            println!("  [files]  {} — OK", game_dir.display());
        } else {
            eprintln!(
                "  [files]  WARNING: expected game directory not found: {}",
                game_dir.display()
            );
        }
    }

    Ok(())
}
