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

/// Run `steamcmd.exe` to download/update a single app.
///
/// `+force_install_dir <library_root>` is passed unchanged.  When SteamCMD
/// runs against a directory that does NOT yet have a pre-created `steamapps\`
/// folder it builds the full layout on its own:
///
///   `<library_root>\steamapps\common\<installdir>\`  ← game files
///   `<library_root>\steamapps\appmanifest_<id>.acf`  ← manifest
///
/// Critically: we must NOT pre-create `<library_root>\steamapps\` before
/// calling SteamCMD.  If that directory already exists SteamCMD treats the
/// path as a direct install target and dumps files straight into
/// `<library_root>\` instead of creating the steamapps layout.
///
/// SteamCMD also writes a duplicate ACF inside the game directory itself
/// (`<installdir>\steamapps\appmanifest_<id>.acf`).  After the update we
/// promote that ACF to the library root (if the library-level copy is absent)
/// and delete the now-empty `<installdir>\steamapps\` artefact.
pub fn update_app(
    login: &str,
    password: &str,
    app_id: u32,
    library_root: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  [update] AppID {}  (account: {})", app_id, login);

    // Do NOT create <library_root>\steamapps\ here — let SteamCMD do it.
    // Pre-creating it causes SteamCMD to install files directly into
    // library_root instead of creating the steamapps\common\<installdir>
    // subtree.
    let library_steamapps = PathBuf::from(library_root).join("steamapps");

    let expected_acf = library_steamapps.join(format!("appmanifest_{}.acf", app_id));
    let steamcmd_acf = steamcmd_dir()
        .join("steamapps")
        .join(format!("appmanifest_{}.acf", app_id));

    // ── Step 1: run the update ───────────────────────────────────────────────
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

    // ── Step 2: ensure ACF is in <library_root>\steamapps\ ───────────────────
    // SteamCMD may write the ACF to its own steamapps\ dir.  Some versions
    // also write a copy inside the game dir at
    //   <library_root>\steamapps\common\<installdir>\steamapps\appmanifest_<id>.acf
    // We prefer the library-level copy; if absent we pull from SteamCMD's dir.
    if expected_acf.exists() {
        println!("  [acf]    manifest present at {}", expected_acf.display());
    } else if steamcmd_acf.exists() {
        // Ensure destination directory exists before copying.
        std::fs::create_dir_all(&library_steamapps)?;
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

    // ── Step 3: clean up spurious <installdir>\steamapps\ artefact ───────────
    // SteamCMD creates a <game_dir>\steamapps\ subdirectory containing only a
    // duplicate ACF and a few empty tracking dirs.  It is not part of the game
    // and confuses users.  Promote the ACF (if we still need it) and delete
    // the folder if it contains nothing but SteamCMD bookkeeping.
    if expected_acf.exists() {
        if let Some(installdir) = read_acf_installdir(&expected_acf) {
            let game_dir = library_steamapps.join("common").join(&installdir);
            let inner_steamapps = game_dir.join("steamapps");
            let inner_acf = inner_steamapps.join(format!("appmanifest_{}.acf", app_id));

            if inner_acf.exists() {
                // Remove the duplicate inner ACF; the library-level one is canonical.
                let _ = std::fs::remove_file(&inner_acf);
            }

            // Delete the inner steamapps\ dir if it is now empty (or only has
            // empty subdirectories that SteamCMD creates for bookkeeping).
            if inner_steamapps.exists() {
                if dir_is_empty_or_only_empty_subdirs(&inner_steamapps) {
                    let _ = std::fs::remove_dir_all(&inner_steamapps);
                    println!("  [clean]  removed spurious steamapps artefact from game directory.");
                }
            }

            // ── Step 4: verify game files ────────────────────────────────────
            if game_dir.exists() {
                println!("  [files]  {} — OK", game_dir.display());
            } else {
                eprintln!(
                    "  [files]  WARNING: expected game directory not found: {}",
                    game_dir.display()
                );
            }
        }
    }

    Ok(())
}

/// Returns `true` if `dir` contains no files at any depth — only empty
/// directories (or itself is empty).  Used to decide whether the spurious
/// `<game>\steamapps\` folder left by SteamCMD is safe to delete.
fn dir_is_empty_or_only_empty_subdirs(dir: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            return false;
        }
        if path.is_dir() && !dir_is_empty_or_only_empty_subdirs(&path) {
            return false;
        }
    }
    true
}
