use std::path::PathBuf;
use std::process::Command;

const STEAMCMD_URL: &str =
    "https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip";

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

/// Ask SteamCMD for the `installdir` of `app_id` via `+app_info_print`.
/// Returns `None` on failure or if the field is absent from the output.
fn fetch_installdir(login: &str, password: &str, app_id: u32) -> Option<String> {
    let exe = steamcmd_exe();
    println!(
        "  [installdir] Querying SteamCMD app_info for AppID {} ...",
        app_id
    );
    let output = Command::new(&exe)
        .args([
            "+login",
            login,
            password,
            "+app_info_update",
            "1",
            "+app_info_print",
            &app_id.to_string(),
            "+quit",
        ])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    let dir = parse_installdir(&text);
    if let Some(ref d) = dir {
        println!("  [installdir] -> \"{}\"", d);
    } else {
        eprintln!("  [installdir] WARNING: could not determine installdir for AppID {} from app_info_print output.", app_id);
    }
    dir
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

/// Run `steamcmd.exe` to download/update a single app into a Steam-compatible
/// library layout:
///
///   `<library_root>\steamapps\common\<installdir>\`  ← game files
///   `<library_root>\steamapps\appmanifest_<id>.acf`  ← manifest
///
/// `+force_install_dir` sets the **direct game install path**, NOT a library
/// root.  To land files at the correct location we must first query the app's
/// `installdir` value via `+app_info_print`, then pass the full path:
///   `+force_install_dir <library_root>\steamapps\common\<installdir>`
///
/// The ACF is written by SteamCMD to its own `steamapps\` directory and is
/// copied into the library's `steamapps\` after the update.
pub fn update_app(
    login: &str,
    password: &str,
    app_id: u32,
    library_root: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  [update] AppID {}  (account: {})", app_id, login);

    let library_steamapps = PathBuf::from(library_root).join("steamapps");
    std::fs::create_dir_all(&library_steamapps)?;

    // ── Step 1: determine installdir ─────────────────────────────────────────
    // Check if we already have an ACF from a previous run (fast path).
    let expected_acf = library_steamapps.join(format!("appmanifest_{}.acf", app_id));
    let steamcmd_acf = steamcmd_dir()
        .join("steamapps")
        .join(format!("appmanifest_{}.acf", app_id));

    let installdir = read_acf_installdir(&expected_acf)
        .or_else(|| read_acf_installdir(&steamcmd_acf))
        .or_else(|| fetch_installdir(login, password, app_id));

    let game_install_dir = match &installdir {
        Some(d) => {
            let p = library_steamapps.join("common").join(d);
            println!("  [update] install dir: {}", p.display());
            p
        }
        None => {
            // Last resort: fall back to library root so the download still
            // proceeds, but warn loudly that layout will be wrong.
            eprintln!(
                "  [update] WARNING: could not determine installdir for AppID {}. \
                 Game files will land in the library root — layout may be incorrect.",
                app_id
            );
            PathBuf::from(library_root)
        }
    };

    std::fs::create_dir_all(&game_install_dir)?;
    let game_install_str = game_install_dir.to_string_lossy();

    // ── Step 2: run the actual update ────────────────────────────────────────
    let exe = steamcmd_exe();
    let status = Command::new(&exe)
        .args([
            "+force_install_dir",
            game_install_str.as_ref(),
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

    // ── Step 3: ensure ACF is in <library_root>\steamapps\ ───────────────────
    if expected_acf.exists() {
        println!("  [acf]    manifest present at {}", expected_acf.display());
    } else if steamcmd_acf.exists() {
        println!("  [acf]    manifest found in SteamCMD dir — copying to library ...");
        std::fs::copy(&steamcmd_acf, &expected_acf)?;
        println!("  [acf]    copied to {}", expected_acf.display());
    } else {
        eprintln!(
            "  [acf]    WARNING: appmanifest_{}.acf not found in library or \
             SteamCMD dir.  Steam.exe may not see this app as installed.",
            app_id
        );
    }

    // ── Step 4: verify game files are where we expect them ───────────────────
    if expected_acf.exists() {
        if let Some(d) = read_acf_installdir(&expected_acf) {
            let common_path = library_steamapps.join("common").join(&d);
            if common_path.exists() {
                println!("  [files]  {} — OK", common_path.display());
            } else {
                eprintln!(
                    "  [files]  WARNING: expected game directory not found: {}",
                    common_path.display()
                );
            }
        }
    }

    Ok(())
}
