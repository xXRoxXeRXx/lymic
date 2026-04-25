<p align="center">
  <img src="lymic.png" width="200" alt="Lymic Logo">
</p>

# Lymic

[![License](https://img.shields.io/github/license/xXRoxXeRXx/immich-desktop-sync?style=for-the-badge)](LICENSE)
[![Built with Tauri](https://img.shields.io/badge/Built%20with-Tauri-%2324C8DB.svg?style=for-the-badge&logo=tauri&logoColor=white)](https://tauri.app/)
[![SvelteKit](https://img.shields.io/badge/Frontend-SvelteKit-%23FF3E00.svg?style=for-the-badge&logo=svelte&logoColor=white)](https://kit.svelte.dev/)

**Lymic** is a high-performance, cross-platform desktop synchronization client for [Immich](https://immich.app/). Effortlessly keep your local media library in sync with your personal Immich server — with a focus on speed, reliability, and a premium user experience.

![Lymic Screenshot](lymic_screen.png)

---

## ✨ Features

- **🚀 High Performance**: Intelligent, metadata-based hash caching skips redundant processing for massive libraries.
- **⚡ Real-time Watcher**: Instantly detects new or modified files in your watched folders and starts background synchronization.
- **📦 Large File Support**: Optimized streaming uploads ensure that multi-gigabyte 4K videos are handled without high memory consumption.
- **🎨 Premium UI**: Clean, modern interface with real-time progress reporting and full system tray integration.
- **🔒 Secure**: Credentials are safely stored in your system's native keyring (Keychain, Credential Manager, or Secret Service).
- **🔄 Smart Deduplication**: Leverages Immich's bulk-upload-check API to verify assets before transmitting data.
- **📂 Folder Management**: Easily add or remove multiple watched folders from your local filesystem.

---

## 🛠️ Tech Stack

| Layer | Technology |
|---|---|
| **Backend** | [Rust](https://www.rust-lang.org/) + [Tauri v2](https://tauri.app/) |
| **Frontend** | [SvelteKit](https://kit.svelte.dev/) + [TypeScript](https://www.typescriptlang.org/) |
| **Styling** | [Tailwind CSS v4](https://tailwindcss.com/) |
| **Icons** | [Lucide](https://lucide.dev/) |
| **Database** | [SQLite](https://sqlite.org/) via [SQLx](https://github.com/launchbadge/sqlx) |
| **Networking** | [Reqwest](https://github.com/seanmonstar/reqwest) (HTTP / multipart streaming) |

---

## 🚀 Getting Started

### Download

The easiest way to get started is to download the latest installer for your operating system from the [Releases](https://github.com/xXRoxXeRXx/immich-desktop-sync/releases) page.

### Building from Source

#### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- [Node.js](https://nodejs.org/) (v18+)
- OS-specific dependencies (see [Tauri prerequisites](https://tauri.app/start/prerequisites/))

#### Steps

1. **Clone the repository**
   ```bash
   git clone https://github.com/xXRoxXeRXx/immich-desktop-sync.git
   cd immich-desktop-sync
   ```

2. **Install dependencies**
   ```bash
   npm install
   ```

3. **Run in development mode**
   ```bash
   npm run tauri dev
   ```

4. **Build for production**
   ```bash
   npm run tauri build
   ```

---

## 🔑 Immich API Configuration

To use Lymic, generate an API Key in your Immich web interface under **Account Settings → API Keys**.

### Required Permissions (Scopes)

| Scope | Purpose |
|---|---|
| `asset.upload` | Transmit new media files to the server |
| `asset.read` | Verify whether a file already exists before uploading |
| `server_info.read` | Validate the connection and check server compatibility |

> [!TIP]
> Lymic adheres to the principle of least privilege. It currently **does not** require `asset.delete` or `album.write` permissions unless you are using specific experimental features.

---

## ⚙️ How it Works

1. **Scan** — The app scans your watched folders for media files (`jpg`, `mp4`, `heic`, etc.).
2. **Hash** — A SHA-1 hash is calculated for each file. Results are cached in a local SQLite database alongside file metadata (mtime/size) so unchanged files are never re-hashed.
3. **Verify** — Before uploading, Lymic checks the Immich server in chunks of 500 to determine which assets already exist.
4. **Stream** — Missing assets are streamed directly from disk to the API, keeping memory usage low even for large video files.

---

## 🤝 Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

---

## ⚠️ Disclaimer

**Lymic is an unofficial community project and is not affiliated with, maintained, or endorsed by the official Immich team.** Use it at your own risk. Always keep backups of your precious media.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).

---

<p align="center">
  Made with ❤️ for the Immich Community
</p>
