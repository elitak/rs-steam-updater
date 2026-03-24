use std::path::PathBuf;
use std::process::Command;

const STEAMCMD_URL: &str =
    "https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip";

// ── ACF helpers ──────────────────────────────────────────────────────────────

/// Parse the `"installdir"` value from a SteamCMD-generated appmanifest ACF.
/// Returns `None` if the field cannot be found.
fn read_acf_installdir(acf_path: &PathBuf) -> Option<String> {
    let text = std::fs::read_to_string(acf_path).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("\"installdir\"") {
            // Typical line: \t"installdir"\t\t"Counter-Strike 2"
            let mut parts = trimmed.splitn(2, "\"installdir\"");
            let rest = parts.nth(1)?.trim();
            // rest is something like `"Counter-Strike 2"`
            if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
                return Some(rest[1..rest.len() - 1].to_string());
            }
        }
    }
    None
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

/// Run `steamcmd.exe` to download/update a single app.
///
/// SteamCMD honours `+force_install_dir` as the library root, creating:
///   `<library_root>\steamapps\common\<installdir>\`  ← game files
///   `<library_root>\steamapps\appmanifest_<id>.acf`  ← manifest
///
/// In some SteamCMD versions the manifest is written to SteamCMD's own
/// `steamapps\` directory instead.  We detect that and move it into the
/// library automatically so Steam.exe always finds everything in one place.
pub fn update_app(
    login: &str,
    password: &str,
    app_id: u32,
    library_root: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  [update] AppID {}  (account: {})", app_id, login);

    // Ensure <library_root>\steamapps\ exists before SteamCMD runs.
    let library_steamapps = PathBuf::from(library_root).join("steamapps");
    std::fs::create_dir_all(&library_steamapps)?;

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

    // ── Ensure the manifest (ACF) is in <library_root>\steamapps\ ────────────
    let expected_acf = library_steamapps.join(format!("appmanifest_{}.acf", app_id));

    if expected_acf.exists() {
        println!(
            "  [acf]    manifest present at {}",
            expected_acf.display()
        );
    } else {
        // SteamCMD may have written the ACF to its own steamapps directory.
        let steamcmd_acf = steamcmd_dir()
            .join("steamapps")
            .join(format!("appmanifest_{}.acf", app_id));

        if steamcmd_acf.exists() {
            println!(
                "  [acf]    manifest found in SteamCMD dir — copying to library …"
            );
            std::fs::copy(&steamcmd_acf, &expected_acf)?;
            println!("  [acf]    copied to {}", expected_acf.display());
        } else {
            eprintln!(
                "  [acf]    WARNING: appmanifest_{}.acf not found in library or \
                 SteamCMD dir.  Steam.exe may not see this app as installed.",
                app_id
            );
        }
    }

    // ── Verify game files landed in steamapps\common\ ────────────────────────
    if expected_acf.exists() {
        if let Some(install_dir) = read_acf_installdir(&expected_acf) {
            let common_path = library_steamapps
                .join("common")
                .join(&install_dir);
            if common_path.exists() {
                println!(
                    "  [files]  {} — OK",
                    common_path.display()
                );
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
