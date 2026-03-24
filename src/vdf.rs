//! Minimal read/write support for Valve's `libraryfolders.vdf` KeyValues text
//! format, limited to what `rs-steam-updater` needs:
//!
//!  1. Ensure the custom `library_root` is registered in Steam's own
//!     `steamapps\libraryfolders.vdf` so that `Steam.exe` discovers the
//!     library on next launch.
//!
//!  2. Ensure each updated App ID appears in that library entry's `apps` map.
//!
//! The module does NOT attempt a full KV parser — it uses targeted string
//! search and well-defined insertion points that match the stable format Steam
//! and SteamCMD have written since 2021.

use std::path::Path;

// ── public API ────────────────────────────────────────────────────────────────

/// Ensure the custom library at `library_root` is registered in Steam's
/// primary `libraryfolders.vdf` and that every `app_id` in `app_ids` is
/// present in that library entry's `apps` block.
///
/// `steam_dir` is the directory that contains `Steam.exe`
/// (e.g. `C:\Program Files (x86)\Steam`).
pub fn ensure_library_registered(
    steam_dir: &Path,
    library_root: &str,
    app_ids: &[u32],
) -> Result<(), Box<dyn std::error::Error>> {
    let vdf_path = steam_dir.join("steamapps").join("libraryfolders.vdf");

    println!("[vdf] libraryfolders.vdf path: {}", vdf_path.display());

    // ── Read existing file, or start from a skeleton ─────────────────────────
    let original = if vdf_path.exists() {
        std::fs::read_to_string(&vdf_path)?
    } else {
        println!("[vdf] File not found — will create a new one.");
        // Ensure the directory exists.
        if let Some(parent) = vdf_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        skeleton_vdf()
    };

    let updated = apply_library(&original, library_root, app_ids)?;

    if updated == original {
        println!("[vdf] Already up to date — no changes needed.");
    } else {
        std::fs::write(&vdf_path, &updated)?;
        println!("[vdf] Updated successfully.");
    }

    Ok(())
}

// ── internal helpers ──────────────────────────────────────────────────────────

/// Returns a minimal skeleton `libraryfolders.vdf` (no library entries).
fn skeleton_vdf() -> String {
    "\"libraryfolders\"\n{\n}\n".to_string()
}

/// Escape a Windows path for embedding in a KV string value
/// (backslashes → double-backslash).
fn escape_path(path: &str) -> String {
    path.replace('\\', "\\\\")
}

/// Apply all changes to the VDF text and return the updated content.
fn apply_library(
    vdf: &str,
    library_root: &str,
    app_ids: &[u32],
) -> Result<String, Box<dyn std::error::Error>> {
    let escaped = escape_path(library_root);

    // Detect whether the library is already present — a line like:
    //   "path"   "C:\\SteamLibrary"
    let already_present = vdf
        .lines()
        .any(|l| l.contains("\"path\"") && l.contains(&format!("\"{}\"", escaped)));

    if already_present {
        println!("[vdf] Library path already registered: {}", library_root);
        // Ensure every app_id is present in the apps block for this entry.
        let result = ensure_apps_in_entry(vdf, &escaped, app_ids);
        return Ok(result);
    }

    // Not present — append a new numbered entry before the closing `}`.
    println!("[vdf] Registering new library path: {}", library_root);
    let next_index = next_library_index(vdf);
    let new_entry = build_library_entry(next_index, &escaped, app_ids);

    // Insert before the final closing `}` of the root block.
    if let Some(pos) = last_closing_brace(vdf) {
        let mut result = vdf[..pos].to_string();
        result.push_str(&new_entry);
        result.push_str(&vdf[pos..]);
        Ok(result)
    } else {
        // Fallback: just append.
        Ok(format!("{}\n{}", vdf.trim_end(), new_entry))
    }
}

/// Finds the next integer index to use for a new library entry by scanning
/// for existing quoted-integer keys at the top level.
fn next_library_index(vdf: &str) -> u32 {
    let mut max: u32 = 0;
    for line in vdf.lines() {
        let t = line.trim();
        // Matches lines like `"1"`, `"12"`, etc. (digit-only keys)
        if t.starts_with('"') && t.ends_with('"') && t.len() >= 3 {
            let inner = &t[1..t.len() - 1];
            if inner.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(n) = inner.parse::<u32>() {
                    if n > max {
                        max = n;
                    }
                }
            }
        }
    }
    max + 1
}

/// Returns the byte offset of the *last* `}` in the string (the root block
/// closer), or `None` if not found.
fn last_closing_brace(vdf: &str) -> Option<usize> {
    vdf.rfind('}')
}

/// Build a fully-formed library folder entry as a VDF string.
fn build_library_entry(index: u32, escaped_path: &str, app_ids: &[u32]) -> String {
    let mut apps_block = String::new();
    for id in app_ids {
        apps_block.push_str(&format!("\t\t\t\"{}\"\t\t\"0\"\n", id));
    }

    format!(
        "\t\"{}\"\n\t{{\n\
         \t\t\"path\"\t\t\"{}\"\n\
         \t\t\"label\"\t\t\"\"\n\
         \t\t\"contentid\"\t\t\"0\"\n\
         \t\t\"totalsize\"\t\t\"0\"\n\
         \t\t\"update_clean_bytes_tally\"\t\t\"0\"\n\
         \t\t\"time_last_update_corruption\"\t\t\"0\"\n\
         \t\t\"apps\"\n\
         \t\t{{\n\
         {}\
         \t\t}}\n\
         \t}}\n",
        index, escaped_path, apps_block
    )
}

/// For an already-registered library, ensure all `app_ids` appear in its
/// `apps` block.  Uses line-based insertion — safe for the fixed KV layout.
fn ensure_apps_in_entry(vdf: &str, escaped_path: &str, app_ids: &[u32]) -> String {
    // Locate the library entry that owns `escaped_path`.
    // Strategy: find the "path" line, then walk forward to the `apps` block
    // and insert missing IDs before the closing `}` of that block.

    let path_needle = format!("\"{}\"", escaped_path);
    let lines: Vec<&str> = vdf.lines().collect();

    // Find the line index of the "path" key for this library.
    let path_line_idx = match lines
        .iter()
        .position(|l| l.contains("\"path\"") && l.contains(&path_needle))
    {
        Some(i) => i,
        None => return vdf.to_string(), // shouldn't happen
    };

    // Find the `apps` block start: search forward for a line containing `"apps"`.
    let apps_block_idx = match lines[path_line_idx..]
        .iter()
        .position(|l| l.trim() == "\"apps\"")
    {
        Some(rel) => path_line_idx + rel,
        None => return vdf.to_string(),
    };

    // The apps block opening `{` is the next non-blank line.
    let apps_open_idx = match lines[apps_block_idx + 1..]
        .iter()
        .position(|l| l.trim() == "{")
    {
        Some(rel) => apps_block_idx + 1 + rel,
        None => return vdf.to_string(),
    };

    // The apps block closing `}` is the next `}` after the opening.
    let apps_close_idx = match lines[apps_open_idx + 1..]
        .iter()
        .position(|l| l.trim() == "}")
    {
        Some(rel) => apps_open_idx + 1 + rel,
        None => return vdf.to_string(),
    };

    // Collect which IDs are already in the block.
    let block_text: String = lines[apps_open_idx..=apps_close_idx].join("\n");

    let mut inserts: Vec<String> = Vec::new();
    for id in app_ids {
        let id_key = format!("\"{}\"", id);
        if !block_text.contains(&id_key) {
            inserts.push(format!("\t\t\t\"{}\"\t\t\"0\"", id));
            println!("[vdf]   adding AppID {} to apps block.", id);
        }
    }

    if inserts.is_empty() {
        println!("[vdf] All app IDs already present in apps block.");
        return vdf.to_string();
    }

    // Insert the new ID lines just before the closing `}` of the apps block.
    let mut result: Vec<String> = lines[..apps_close_idx]
        .iter()
        .map(|s| s.to_string())
        .collect();
    result.extend(inserts);
    result.extend(lines[apps_close_idx..].iter().map(|s| s.to_string()));
    result.join("\n")
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#""libraryfolders"
{
	"1"
	{
		"path"		"C:\\ExistingLib"
		"label"		""
		"contentid"		"0"
		"totalsize"		"0"
		"update_clean_bytes_tally"		"0"
		"time_last_update_corruption"		"0"
		"apps"
		{
			"730"		"0"
		}
	}
}"#;

    #[test]
    fn adds_new_library() {
        let out = apply_library(SAMPLE, r"C:\NewLib", &[440]).unwrap();
        assert!(out.contains("\"path\"\t\t\"C:\\\\NewLib\""));
        assert!(out.contains("\"440\""));
        assert!(out.contains("\"2\""));
    }

    #[test]
    fn no_duplicate_library() {
        let out = apply_library(SAMPLE, r"C:\ExistingLib", &[730]).unwrap();
        // path should appear exactly once
        assert_eq!(out.matches("C:\\\\ExistingLib").count(), 1);
    }

    #[test]
    fn adds_missing_app_to_existing_library() {
        let out = apply_library(SAMPLE, r"C:\ExistingLib", &[570]).unwrap();
        assert!(out.contains("\"570\""));
    }
}
