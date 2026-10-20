# SK Plugin Manager — Release Guide

How Mac (.dmg) and Windows (.exe) installers get built.

## What the workflow does

`.github/workflows/build.yml` runs on GitHub's servers (not your machine):

| Runner | Builds | Artifact |
|---|---|---|
| `macos-latest` | `.dmg` installer | `sk-plugin-manager-macos` |
| `windows-latest` | NSIS `-setup.exe` installer | `sk-plugin-manager-windows` |

It checks out the code, installs Node 20 + stable Rust, runs `npm ci`,
builds the app with `tauri-apps/tauri-action@v0`, and uploads the installers
as workflow artifacts.

**Triggers:** pushing a tag like `v1.0.0`, or the manual **Run workflow**
button in the repo's Actions tab.

## How to trigger a build (you do this)

1. Create the repo once: go to **github.com/new**, name it
   `sk-plugin-manager`, create it (empty, no README).
2. In this folder, point git at it and push:
   ```bash
   git remote add origin https://github.com/<your-username>/sk-plugin-manager.git
   git push -u origin main
   ```
   (Or with the GitHub CLI: `gh repo create sk-plugin-manager --public --source=. --push`)
3. Tag the release and push the tag — this starts the build:
   ```bash
   git tag v1.0.0
   git push origin v1.0.0
   ```
4. Open the repo → **Actions** tab → click the run → download the
   `sk-plugin-manager-macos` / `sk-plugin-manager-windows` artifacts.

## Signing caveat (read before sharing publicly)

These builds are **unsigned**:

- **macOS:** first open of the `.dmg` app shows *"cannot be opened because
  the developer cannot be verified"*. For personal testing: right-click the
  app → **Open** → **Open** once, and macOS remembers it afterwards.
- **Windows:** SmartScreen shows an *"Unknown publisher"* warning on the
  `-setup.exe`. Click **More info → Run anyway** for personal testing.

For a public release this must be fixed with paid certificates:

- **Apple Developer Program (~$99/year)** → Developer ID certificate +
  notarization (removes the Gatekeeper warning).
- **Windows code-signing certificate** (~$70–300/year depending on vendor) →
  removes the SmartScreen warning.

When those exist, uncomment the `env` block in
`.github/workflows/build.yml` and add the matching repository secrets
(`APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`,
`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`).

## Bundle identity

- App name: **SK Plugin Manager**
- Bundle identifier: `com.skmotion.pluginmanager`
- Version: from `package.json` / `src-tauri/tauri.conf.json` (keep both in sync)
