use std::path::PathBuf;
use std::process::Command;

const STEAMCMD_URL: &str =
    "https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip";

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
/// `library_root` must be the root of a Steam library that already has (or
/// will have) a `steamapps/` subdirectory — e.g.
/// `C:\Program Files (x86)\Steam` or `D:\SteamLibrary`.
/// SteamCMD will write:
///   `<library_root>/steamapps/appmanifest_<id>.acf`
///   `<library_root>/steamapps/common/<game name>/...`
///   `<library_root>/steamapps/libraryfolders.vdf`
pub fn update_app(
    login: &str,
    password: &str,
    app_id: u32,
    library_root: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  [update] AppID {}  (account: {})", app_id, login);

    // Ensure steamapps/ exists so SteamCMD treats this path as a library root.
    let steamapps_dir = PathBuf::from(library_root).join("steamapps");
    std::fs::create_dir_all(&steamapps_dir)?;

    let exe = steamcmd_exe();
    let status = Command::new(&exe)
        .args([
            "+force_install_dir",
            library_root,
            "+login",
            login,
            password,
            "+app_update",
            &app_id.to_string(),
            "validate",
            "+quit",
        ])
        .status()?;

    if !status.success() {
        eprintln!(
            "  [warning] SteamCMD exited with code {:?} for AppID {}",
            status.code(),
            app_id
        );
    } else {
        println!("  [done]   AppID {} updated successfully.", app_id);
    }
    Ok(())
}
