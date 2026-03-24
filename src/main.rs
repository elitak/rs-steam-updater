mod dialog;
mod settings;
mod steam_api;
mod steam_cmd;
mod steam_process;

use std::collections::HashSet;
use std::path::PathBuf;

fn main() {
    if let Err(e) = run() {
        eprintln!("[error] {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // \u{2500}\u{2500} 1. Locate settings.yml next to the executable \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let settings_path = exe_dir.join("settings.yml");
    let settings = settings::Settings::load(&settings_path)?;
    let library_root = settings.library_root().to_string();

    // \u{2500}\u{2500} 2. First account \u{2014} used to relaunch Steam after updates \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}
    let (first_login, first_account) = settings
        .accounts
        .iter()
        .next()
        .ok_or("settings.yml has no accounts")?;
    let first_login = first_login.clone();
    let first_password = first_account
        .password
        .clone()
        .unwrap_or_default();

    // \u{2500}\u{2500} 3. Detect and optionally stop Steam \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}
    let steam_was_open = steam_process::is_steam_running();
    let steam_exe = steam_process::find_steam_exe();

    if steam_was_open {
        println!("[steam] Steam is currently running.");

        let proceed = dialog::show_countdown_dialog();
        if !proceed {
            println!("[abort] User aborted. Exiting.");
            return Ok(());
        }

        println!("[steam] Shutting down Steam ...");
        steam_process::shutdown_steam(steam_exe.as_ref());
    }

    // \u{2500}\u{2500} 4. Bootstrap SteamCMD \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}
    steam_cmd::install_steam_cmd()?;

    // \u{2500}\u{2500} 5. Update all apps \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}
    println!("\n[config] Library root : {}", library_root);
    std::fs::create_dir_all(&library_root)?;

    // Lazy-loaded Steam app catalogue (fetched at most once)
    let mut app_list_cache: Option<Vec<(u32, String)>> = None;

    for (account_name, account) in &settings.accounts {
        let password = match &account.password {
            Some(p) => p.as_str(),
            None => {
                eprintln!(
                    "[warn] Account '{}' has no 'password' -- skipping.",
                    account_name
                );
                continue;
            }
        };

        if account.app_ids.is_empty() && account.app_res.is_empty() {
            eprintln!(
                "[warn] Account '{}' has neither 'appIDs' nor 'appREs' -- skipping.",
                account_name
            );
            continue;
        }

        // Resolve appREs \u{2192} additional appIDs
        let mut resolved_ids: Vec<u32> = Vec::new();
        if !account.app_res.is_empty() {
            // Fetch the catalogue the first time it's needed
            if app_list_cache.is_none() {
                app_list_cache = Some(steam_api::fetch_app_list()?);
            }
            let catalogue = app_list_cache.as_ref().unwrap();
            resolved_ids = steam_api::resolve_app_res(&account.app_res, catalogue);
        }

        // Merge explicit IDs + resolved IDs, deduplicating while preserving order
        let mut seen: HashSet<u32> = HashSet::new();
        let all_ids: Vec<u32> = account
            .app_ids
            .iter()
            .copied()
            .chain(resolved_ids)
            .filter(|id| seen.insert(*id))
            .collect();

        println!(
            "\n[account] {}  ({} app(s))",
            account_name,
            all_ids.len()
        );

        for app_id in &all_ids {
            steam_cmd::update_app(account_name, password, *app_id, &library_root)?;
        }
    }

    println!("\n[all done] Steam library update complete.");

    // \u{2500}\u{2500} 6. Launch Steam \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}
    // Always launch Steam after updates, regardless of whether it was open before.
    println!("\n[steam] Preparing to launch Steam ...");
    match &steam_exe {
        Some(exe) => {
            steam_process::launch_steam(exe, &first_login, &first_password);
        }
        None => {
            eprintln!(
                "[warn] Could not find Steam.exe -- please start Steam manually."
            );
        }
    }

    Ok(())
}