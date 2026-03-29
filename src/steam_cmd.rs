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

/// Run `steamcmd.exe` to download/update a single app, then move the result
/// into the target library.
///
/// SteamCMD is invoked WITHOUT `+force_install_dir`.  It installs/updates the
/// app inside its own working directory:
///
///   `<steamcmd_dir>\steamapps\common\<installdir>\`  ← game files
///   `<steamcmd_dir>\steamapps\appmanifest_<id>.acf`  ← manifest
///
/// After the update the game directory is moved into the target library:
///
///   `<library_root>\steamapps\common\<installdir>\`
///   `<library_root>\steamapps\appmanifest_<id>.acf`
///
/// To keep subsequent runs incremental (avoiding a full re-download each time),
/// before calling SteamCMD we move the game directory back from the library
/// into SteamCMD's own steamapps if the files are absent there.
pub fn update_app(
    login: &str,
    password: &str,
    app_id: u32,
    library_root: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  [update] AppID {}  (account: {})", app_id, login);

    let steamcmd_steamapps = steamcmd_dir().join("steamapps");
    let steamcmd_acf = steamcmd_steamapps
        .join(format!("appmanifest_{}.acf", app_id));

    let library_steamapps = PathBuf::from(library_root).join("steamapps");
    let library_acf = library_steamapps
        .join(format!("appmanifest_{}.acf", app_id));

    // ── Step 1: restore game files to SteamCMD dir for incremental update ────
    // If the game already lives in the library (from a previous run), move it
    // back to SteamCMD's steamapps\common\ so SteamCMD can update it in-place
    // rather than downloading everything from scratch.
    if let Some(installdir) = read_acf_installdir(&library_acf)
        .or_else(|| read_acf_installdir(&steamcmd_acf))
    {
        let lib_game = library_steamapps.join("common").join(&installdir);
        let cmd_game = steamcmd_steamapps.join("common").join(&installdir);
        if lib_game.exists() && !cmd_game.exists() {
            println!(
                "  [prep]   restoring {} to SteamCMD dir for incremental update ...",
                installdir
            );
            std::fs::create_dir_all(cmd_game.parent().unwrap())?;
            move_dir(&lib_game, &cmd_game)?;
            println!("  [prep]   done.");
        }
    }

    // ── Step 2: run the update ───────────────────────────────────────────────
    let exe = steamcmd_exe();
    let status = Command::new(&exe)
        .args([
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

    // ── Step 3: move game directory from SteamCMD to library ─────────────────
    let installdir = match read_acf_installdir(&steamcmd_acf) {
        Some(d) => d,
        None => {
            eprintln!(
                "  [error]  Could not read installdir from {:?} — \
                 game files remain in SteamCMD dir.",
                steamcmd_acf
            );
            return Ok(());
        }
    };

    let cmd_game = steamcmd_steamapps.join("common").join(&installdir);
    let lib_game = library_steamapps.join("common").join(&installdir);

    if cmd_game.exists() {
        std::fs::create_dir_all(lib_game.parent().unwrap())?;
        if lib_game.exists() {
            println!("  [move]   removing stale library copy ...");
            std::fs::remove_dir_all(&lib_game)?;
        }
        println!(
            "  [move]   {} → {}",
            cmd_game.display(),
            lib_game.display()
        );
        move_dir(&cmd_game, &lib_game)?;
        println!("  [move]   done.");
    } else {
        eprintln!(
            "  [warning] SteamCMD game dir not found at {} — \
             nothing to move.",
            cmd_game.display()
        );
    }

    // ── Step 4: copy ACF to library ───────────────────────────────────────────
    // SteamCMD's own ACF stays in place so the next run is incremental.
    // The library copy is what Steam.exe reads to discover the installation.
    if steamcmd_acf.exists() {
        std::fs::create_dir_all(&library_steamapps)?;
        std::fs::copy(&steamcmd_acf, &library_acf)?;
        println!("  [acf]    copied to {}", library_acf.display());
    } else {
        eprintln!(
            "  [acf]    WARNING: appmanifest_{}.acf not found in SteamCMD dir.",
            app_id
        );
    }

    // ── Step 5: verify ────────────────────────────────────────────────────────
    if lib_game.exists() {
        println!("  [files]  {} — OK", lib_game.display());
    } else {
        eprintln!(
            "  [files]  WARNING: expected game directory not found: {}",
            lib_game.display()
        );
    }

    Ok(())
}

/// Move `src` to `dst`.  Tries an atomic rename first; falls back to a
/// recursive copy + delete when rename fails (e.g. cross-device move).
fn move_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    // Cross-device or other rename failure — copy recursively then delete src.
    copy_dir_recursive(src, dst)?;
    std::fs::remove_dir_all(src)?;
    Ok(())
}

/// Recursively copy the contents of `src` into `dst` (creating `dst` if needed).
fn copy_dir_recursive(
    src: &std::path::Path,
    dst: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
