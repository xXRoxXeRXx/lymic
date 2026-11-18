<p align="center">
  <img src="lymic.png" width="200" alt="Lymic Logo">
</p>

# Lymic — Desktop upload client for Immich

[![License](https://img.shields.io/github/license/xXRoxXeRXx/lymic?style=for-the-badge)](LICENSE)
[![Microsoft Store](https://img.shields.io/badge/Microsoft_Store-Lymic-%230078D4.svg?style=for-the-badge&logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCIgZmlsbD0id2hpdGUiPjxwYXRoIGQ9Ik0wIDBoMTF2MTFIMHpNMTMgMGgxMXYxMUgxM3pNMCAxM2gxMXYxMUgwek0xMyAxM2gxMXYxMUgxM3oiLz48L3N2Zz4=)](https://apps.microsoft.com/store/detail/9NLQWHTHR1FZ?cid=DevShareMCLPCS)
[![Snapcraft](https://img.shields.io/badge/Snapcraft-lymic-%23E95420.svg?style=for-the-badge&logo=snapcraft&logoColor=white)](https://snapcraft.io/lymic)
[![Built with Tauri](https://img.shields.io/badge/Built%20with-Tauri-%2324C8DB.svg?style=for-the-badge&logo=tauri&logoColor=white)](https://tauri.app/)
[![SvelteKit](https://img.shields.io/badge/Frontend-SvelteKit-%23FF3E00.svg?style=for-the-badge&logo=svelte&logoColor=white)](https://kit.svelte.dev/)

**Lymic** is an unofficial, cross-platform desktop client for uploading and synchronizing local media with [Immich](https://immich.app/). It watches selected local folders, detects supported media, avoids uploading assets that already exist on Immich, and streams missing files from disk.

> Lymic is an upload-oriented client, not a two-way filesystem mirror. It does not download remote assets automatically, mirror remote deletions or rename local files. Browsing your library is read-only; the only changes Lymic makes in Immich are uploads, target-album assignments, album creation, the Favorite flag and Trash restores.

<p align="center">
  <img src="docs/assets/login.png" alt="Lymic Login" width="100%">
</p>

<p align="center">
  <img src="docs/assets/sync.png" alt="Lymic Synchronization" width="49%">
  <img src="docs/assets/settings.png" alt="Lymic Settings" width="49%">
</p>

---

## Features

**Synchronization**

- **Folder watching** — Recursive, debounced, with a rescan after watcher overflow.
- **Efficient** — Cached SHA-1 hashes are reused while size and mtime are unchanged; Immich's bulk check skips assets it already has.
- **Large files** — Multipart uploads stream from disk without loading whole files into memory.
- **Durable queue** — Progress, pause state and failures survive restarts; failed items can be retried.
- **Media-aware** — Extension-based detection, live-photo/motion pairs and optional XMP sidecars.
- **Target albums** — Optionally add a folder's media, including files Immich already has, to an album.
- **Desktop integration** — Tray controls, notifications, optional autostart, light/dark themes, 10 languages. API keys are stored in the native keyring.

**Browsing your Immich library**

- **Photos and Albums** — Masonry timeline with date scrubber, thumbnail cache and Lightbox. In the Lightbox you can view details, set Favorite, download and add to an album.
- **Explore** — Recognized people, places (cities) and memories, each opening as a timeline.
- **People and Tags** — Filter the timeline by person; browse Tags as a hierarchy tree. Tags are never created or changed.
- **Map** — Geotagged photos and videos as clusters and thumbnails. Map tiles come from OpenFreeMap, not from your Immich server.
- **Archive** — View archived items and **Unarchive** them (single or multi-selection) so they return to your Photos. In Photos and Favorites you can **Archive** a multi-selection, which hides it there as in Immich's web app; Albums, People, Places, Memories, Tags and Search results offer Archive too and keep the assets in the list. Lymic only switches an asset between archived and not archived: never the locked folder, hidden assets or the trash.
- **Trash** — View, download and **Restore** trashed items (single or multi-selection). Lymic never deletes anything or empties the trash. A local file whose copy is in the Immich trash counts as synced and is not uploaded again.
- **Search** — Search bar on every view: by Context, Filename, Description, Folder Path or OCR, with filters for people, Tags, dates, location, media type, camera, rating, Favorites, Archived and Unfiled Assets. Requires **Immich 3.2 or later**; Context search needs Smart Search and OCR needs OCR enabled in Immich.

---

## Required permissions (scopes)

Create the API key in Immich under **Account Settings → API Keys**. The simplest option is to select all permissions. Otherwise grant only what you use; a missing permission affects just the features that need it.

**Upload and sync** (all you need if you only upload)

| Scope | Needed for |
|---|---|
| `asset.upload` | Uploading files and the check whether Immich already has them |
| `user.read` | Login (current user) |

**Albums**

| Scope | Needed for |
|---|---|
| `album.read` | Listing albums (target-album picker, Albums view, Lightbox) |
| `album.create` | Creating an album from the app |
| `albumAsset.create` | Adding media to an album (target albums, Lightbox) |

**Browsing**

| Scope | Needed for |
|---|---|
| `asset.read` | Every timeline (Photos, Favorites, Archive, Trash, Albums, Places, Tags), asset details, Search, and finding existing duplicates for target albums |
| `asset.view` | Thumbnails, previews and video playback |
| `asset.download` | Downloading originals |
| `asset.update` | Setting or clearing Favorite; Archive and Unarchive |
| `asset.delete` | Trash: Restore only. Lymic never deletes anything. |
| `person.read` | People, Explore people, Search: Recognized People filter |
| `memory.read` | Explore: memories |
| `tag.read` | Tags tree, Search: Tag filter |
| `map.read` | Map. Immich releases before 2.5 do not check it. |
| `userProfileImage.read` | Optional: your profile picture. Without it no avatar is shown. |

These permissions were checked against the Immich v3.4.1 source. Which behavior was and was not confirmed on a live server is recorded in the [application map](docs/agents/application-map.md).

---

## Getting started

### Download

Download the latest installer from the [Lymic releases](https://github.com/xXRoxXeRXx/lymic/releases) page, or install from a store or package manager:

**Windows (Microsoft Store)**

[![Download from Microsoft Store](https://get.microsoft.com/images/en-us%20dark.svg)](https://apps.microsoft.com/store/detail/9NLQWHTHR1FZ?cid=DevShareMCLPCS)

```powershell
winget install 9NLQWHTHR1FZ
```

**Linux (Snap)**

[![Get it from the Snap Store](https://snapcraft.io/static/images/badges/en/snap-store-black.svg)](https://snapcraft.io/lymic)

```bash
sudo snap install lymic
```

**Arch Linux (AUR)** — `paru -S lymic-bin` or `yay -S lymic-bin`

**macOS (Homebrew)** — `brew tap xXRoxXeRXx/tap && brew install --cask lymic` ([guide](docs/homebrew-cask.md))

> [!IMPORTANT]
> **macOS:** Lymic has no paid Apple Developer certificate, so macOS may block it as "unidentified" or "damaged". Drag it into **Applications**, then right-click it and choose **Open**. If it is still reported as damaged, run `xattr -cr /Applications/Lymic.app`.

### Building from source

Prerequisites: [Rust](https://www.rust-lang.org/tools/install) (latest stable), [Node.js](https://nodejs.org/) 18+ and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
git clone https://github.com/xXRoxXeRXx/lymic.git
cd lymic
npm install
npm run tauri dev     # development
npm run tauri build   # production build
```

---

## Connect to Immich

Enter the server URL and the API key (see [Required permissions](#required-permissions-scopes)) in Lymic.

The URL must use **HTTPS** and must not contain credentials, a query string or a fragment. Redirects are disabled, so your reverse proxy must expose the final HTTPS endpoint directly.

Under **Advanced login options**:

- **Allow HTTP for local server** — Off by default. Permits HTTP only for `localhost`, `127.0.0.0/8` and `[::1]`; LAN addresses and `.local` names stay HTTPS-only. HTTP exposes the API key to local network observers.
- **Cloudflare Access** — Enter a service-token client ID and secret ([docs](https://developers.cloudflare.com/cloudflare-one/identity/service-tokens/)). They are sent as `CF-Access-Client-Id` and `CF-Access-Client-Secret` only to your Immich endpoint.

The API key and these options are stored in the native keyring and kept out of logs.

---

## How synchronization works

1. **Discover** — Watched folders are scanned at startup and when added; filesystem events trigger incremental work.
2. **Classify and hash** — Supported media, live-photo/motion pairs and XMP sidecars are detected. Files with unchanged size and mtime reuse their cached SHA-1; others are streamed to hash them.
3. **Check** — Hashes go to Immich's bulk check in batches; existing assets are skipped.
4. **Queue and upload** — Missing assets enter the durable queue and are streamed to Immich in parallel, with the SHA-1 in the `x-immich-checksum` header.
5. **Record** — Results and failures are persisted and shown in the UI. Paused work resumes after a restart.

Duplicate and overlapping watched folders are rejected, and unavailable folders are retried. Closing the main window hides Lymic to the tray; use the tray menu to quit.

More on pairing rules, security, local data, architecture, releases and troubleshooting: **[full project documentation](docs/README.md)**.

---

## Technology

[Rust](https://www.rust-lang.org/) + [Tauri v2](https://tauri.app/) backend, [SvelteKit](https://kit.svelte.dev/) + [TypeScript](https://www.typescriptlang.org/) frontend, [Tailwind CSS v4](https://tailwindcss.com/), [Lucide](https://lucide.dev/) icons, [SQLite](https://sqlite.org/) via [SQLx](https://github.com/launchbadge/sqlx), and [Reqwest](https://github.com/seanmonstar/reqwest) for HTTP and multipart streaming.

---

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

---

## Disclaimer

**Lymic is an unofficial community project and is not affiliated with, maintained, or endorsed by the official Immich team.** Use it at your own risk. Always keep backups of your precious media.

---

## License

This project is licensed under the [MIT License](LICENSE).

---

<p align="center">
  Made with ❤️ for the Immich Community
</p>
