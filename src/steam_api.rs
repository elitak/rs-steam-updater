use regex::Regex;
use serde::Deserialize;

const STEAM_APP_LIST_URL: &str = "https://api.steampowered.com/ISteamApps/GetAppList/v2/";

#[derive(Deserialize)]
struct AppListResponse {
    applist: AppListInner,
}

#[derive(Deserialize)]
struct AppListInner {
    apps: Vec<App>,
}

#[derive(Deserialize)]
struct App {
    appid: u32,
    name: String,
}

/// Fetch the full Steam public app catalogue and return it.
/// This is called at most once; callers should cache the result.
pub fn fetch_app_list() -> Result<Vec<(u32, String)>, Box<dyn std::error::Error>> {
    println!("[api] Fetching Steam app catalogue ...");
    let response = reqwest::blocking::get(STEAM_APP_LIST_URL)?
        .error_for_status()?
        .json::<AppListResponse>()?;
    let apps: Vec<(u32, String)> = response
        .applist
        .apps
        .into_iter()
        .map(|a| (a.appid, a.name))
        .collect();
    println!("[api] Catalogue loaded ({} entries).", apps.len());
    Ok(apps)
}

/// For each regex pattern, collect all matching appIDs from `app_list`.
/// Prints a line for every match, and a warning for patterns with no matches.
pub fn resolve_app_res(
    patterns: &[String],
    app_list: &[(u32, String)],
) -> Vec<u32> {
    let mut resolved = Vec::new();
    for pattern in patterns {
        match Regex::new(pattern) {
            Ok(re) => {
                let mut matched = false;
                for (appid, name) in app_list {
                    if re.is_match(name) {
                        println!(
                            "  [api] Pattern '{}' -> AppID {}  ({})",
                            pattern, appid, name
                        );
                        resolved.push(*appid);
                        matched = true;
                    }
                }
                if !matched {
                    eprintln!("  [api] No apps matched pattern '{}'", pattern);
                }
            }
            Err(e) => {
                eprintln!("  [api] Invalid regex '{}': {}", pattern, e);
            }
        }
    }
    resolved
}
