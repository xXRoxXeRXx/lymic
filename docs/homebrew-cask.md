# Homebrew Cask

Lymic's Apple Silicon Cask is prepared for the `xXRoxXeRXx/homebrew-tap` tap. Its Cask is generated from the GitHub release DMG, so the download URL and SHA-256 always describe the same published artifact.

## Install and uninstall

```bash
brew tap xXRoxXeRXx/tap
brew install --cask lymic
open -a Lymic
brew uninstall --cask lymic
```

Homebrew installs `Lymic.app` in `/Applications`. Since Lymic is not notarized with an Apple Developer certificate, macOS may require the user to Control-click the app and select **Open** on first launch.

## Release automation

The `Update Homebrew Cask` workflow runs only after a stable GitHub release is published. It downloads `Lymic_<version>_aarch64.dmg`, computes its SHA-256 using [`scripts/update-homebrew-cask.mjs`](../scripts/update-homebrew-cask.mjs), and commits `Casks/lymic.rb` to the tap.

Create `xXRoxXeRXx/homebrew-tap` with a `Casks/` directory, then add a `HOMEBREW_TAP_TOKEN` repository secret with contents-write permission to that tap. The job is deliberately skipped until that secret exists.

To run the update locally after publishing a release:

```bash
node scripts/update-homebrew-cask.mjs --version 0.23.0 --output /path/to/homebrew-tap/Casks/lymic.rb
brew audit --cask --strict /path/to/homebrew-tap/Casks/lymic.rb
brew install --cask /path/to/homebrew-tap/Casks/lymic.rb
brew uninstall --cask lymic
```
