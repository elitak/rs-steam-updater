# rs-steam-updater

A native Windows executable that **bootstraps SteamCMD** and **downloads or
updates every Steam app** listed in `settings.yml` — a Rust port of the
[`elitak/steam-updater`](https://github.com/elitak/steam-updater) PowerShell
script.

---

## Features

* Reads `settings.yml` from the same directory as the executable.
* Detects whether Steam is running; if so shows a Win32 countdown dialog
  (10 s) with **Do It Now** / **Abort** buttons before shutting Steam down.
* Installs SteamCMD to `%ProgramData%\SteamCMD` on first run.
* Supports explicit `appIDs` and regex-based `appREs` (resolved via the Steam
  Web API) per account.
* Relaunches Steam (logged in as the first account) when done.

---

## Requirements

| | |
|---|---|
| OS | Windows 10 / 11 or Windows Server 2016+ |
| Rust | 1.70 or later (`rustup` recommended) |
| Target | `x86_64-pc-windows-msvc` (or `gnu`) |
| Internet | Required for SteamCMD install and all game downloads |
| Steam Guard | Must be **disabled** on every account (SteamCMD cannot handle 2-FA) |

---

## Build

```powershell
cargo build --release
# Produces: target\release\rs-steam-updater.exe
```

---

## Quick start

1. Copy `rs-steam-updater.exe` and `settings.yml` to the same folder.
2. Edit `settings.yml` with your accounts and app IDs.
3. Run `rs-steam-updater.exe` (Administrator recommended so SteamCMD can write
   to `C:\SteamLibrary`).

---

## settings.yml reference

```yaml
library_root: "C:\\SteamLibrary"   # Install path for all games (optional)

accounts:
  <steam_username>:                # map key IS the Steam login
    password: "<steam_password>"
    appIDs:                        # optional: explicit numeric App IDs
      - 730                        # Counter-Strike 2
      - 570                        # Dota 2
    appREs:                        # optional: regex patterns matched against
      - '^Portal$'                 # Steam app titles (Steam Web API lookup)
```

| Key | Required | Description |
|---|---|---|
| `library_root` | No | Where to install all games. Defaults to `C:\SteamLibrary`. |
| `accounts` | Yes | Ordered map of account entries. Key = Steam username. |
| `password` | Yes | Steam account password. |
| `appIDs` | No¹ | List of numeric Steam App IDs to download/update. |
| `appREs` | No¹ | List of regex patterns matched against Steam app titles. |

¹ At least one of `appIDs` or `appREs` must be present per account.

---

## Scheduling

Open **Task Scheduler** and create a task that runs:

```
C:\path\to\rs-steam-updater.exe
```

Start in the same directory so `settings.yml` is found next to the executable.

---

## Security note

`settings.yml` contains plaintext credentials. Do **not** commit a
filled-in `settings.yml` to a public repository.

---

## Project layout

```
Cargo.toml
src/
  main.rs           — entry point / orchestration
  settings.rs       — settings.yml parsing (serde + indexmap)
  steam_cmd.rs      — SteamCMD bootstrap and per-app update
  steam_api.rs      — Steam Web API app-list fetch + regex resolution
  steam_process.rs  — find / kill / relaunch Steam.exe (Win32 toolhelp32)
  dialog.rs         — Win32 modal countdown dialog
settings.yml        — example configuration
```