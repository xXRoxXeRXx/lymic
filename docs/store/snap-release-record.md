# Canonical Snap Store Public Release Record: Lymic

> Status: Entwurf. Die Checkliste in Abschnitt 4 wird nach Registrierung und Release-Freigabe abgehakt.

This document records the official public release parameters for Lymic on the Canonical Snap Store, documenting strict confinement, declared interface plugs, desktop integration, release workflow automation, and store credentials.

---

## 1. Release Identification

| Attribute | Details |
|---|---|
| **Application** | Lymic |
| **Snap Package Name** | `lymic` |
| **Base** | `core22` (Ubuntu 22.04 LTS runtime base) |
| **Confinement** | `strict` |
| **Grade** | `stable` |
| **Desktop Extension** | GNOME 42 (`extensions: [gnome]`) |
| **Store URL** | `https://snapcraft.io/lymic` |
| **Target Channel** | Candidate / Stable (`candidate` on tagged builds, promoted to `stable`) |
| **Pricing** | Free (Open Source, MIT License) |
| **Categories** | Graphics & Photography, Utilities |

---

## 2. Interface Plugs & Confinement Rationale

Lymic runs in `strict` confinement mode to provide application isolation while declaring the interface plugs necessary for local synchronization with Immich:

1. **`network`**:
   - Required to communicate with the user's Immich server via HTTPS (and loopback HTTP when explicitly opted in).
   - Auto-connected upon snap installation.

2. **`home`**:
   - Required to discover and watch local user directories configured as **Watched Folders** (e.g. `~/Pictures`, `~/Photos`).
   - Auto-connected upon snap installation.

3. **`removable-media`**:
   - Required to detect and watch media on external storage, USB drives, and SD cards configured as **Removable Watched Folders** (ADR-0002).
   - Auto-connection can be requested from the Snap Store or connected manually via `sudo snap connect lymic:removable-media`.

4. **`password-manager-service`**:
   - Required to access the Linux Secret Service daemon (GNOME Keyring / KSecretService) for secure credential storage of the Immich API key.

5. **`gnome` Extension**:
   - Automatically provides desktop integration plugs: `desktop`, `desktop-legacy`, `wayland`, `x11`, `opengl`, and `gsettings`.
   - Provisions WebKitGTK and GTK 3 desktop portal support for native file chooser dialogs.

---

## 3. Maintainer Setup: Snap Store Registration & CI Credentials

Registering the package name on Canonical Snap Store requires maintainer authentication:

### Step 1: Register Package Name
```bash
# Log in with Ubuntu One / Canonical credentials
snapcraft login

# Register the package name in the store
snapcraft register lymic
```

### Step 2: Export Store Credentials for GitHub Actions
```bash
# Generate the authentication macaroon token for CI/CD
snapcraft export-login --snaps lymic --channels edge,beta,candidate,stable snapcraft.login
```

### Step 3: Add Secret to GitHub Repository
1. In the GitHub repository, navigate to **Settings** → **Secrets and variables** → **Actions**.
2. Add a new repository secret named `SNAPCRAFT_STORE_CREDENTIALS`.
3. Paste the contents of the generated `snapcraft.login` file.

---

## 4. Release Checklist & Verification Sign-Off

- [ ] Package name `lymic` registered on `snapcraft.io` under the project owner account.
- [ ] Interface plugs (`network`, `home`, `removable-media`, `password-manager-service`) validated.
- [ ] `snap/snapcraft.yaml` and `snap/gui/lymic.desktop` validated via `scripts/validate-snapcraft.ps1`.
- [ ] WebKitGTK 4.1 layout bindings verified to prevent sub-process crashes under strict confinement.
- [ ] GitHub Actions release workflow configured to build `.snap` package and publish via `snapcore/action-publish`.
- [ ] `SNAPCRAFT_STORE_CREDENTIALS` secret configured in GitHub repository settings.

