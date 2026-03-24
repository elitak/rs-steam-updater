# TODO

## Bugs

- [ ] **Steam API regex → AppID resolution is broken**  
  The `steam_api` module's logic for resolving `appREs` (regex patterns) to AppIDs via the Steam Web API does not work correctly. Needs investigation and a fix.

## Features

- [ ] **Steam Guard email code support**  
  When SteamCMD requires a Steam Guard code, automatically retrieve it from email and supply it during login. Email login credentials will be provided separately (do not hardcode). Design should allow credentials to be supplied via `settings.yml` or environment variables.