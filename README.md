# 📸 Immich Desktop Sync

[![Rust](https://img.shields.io/badge/rust-%23E32F26.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-%2324C8DB.svg?style=for-the-badge&logo=tauri&logoColor=FFFFFF)](https://tauri.app/)
[![Svelte](https://img.shields.io/badge/Svelte-ff3e00?style=for-the-badge&logo=svelte&logoColor=white)](https://svelte.dev/)
[![Material Design](https://img.shields.io/badge/Material%20Design%203-%23757575.svg?style=for-the-badge&logo=material-design&logoColor=white)](https://m3.material.io/)

A high-performance, cross-platform desktop synchronization client for [Immich](https://immich.app/). Effortlessly keep your local media library in sync with your personal Immich server with a focus on speed, reliability, and a premium user experience.

---

## ✨ Features

- **🚀 High Performance**: Intelligent, metadata-based hash caching skips redundant processing for massive libraries.
- **⚡ Real-time Watcher**: Instantly detects new or modified files in your watched folders and starts background synchronization.
- **📦 Large File Support**: Optimized streaming uploads ensure that multi-gigabyte 4K videos are handled without high memory consumption.
- **🎨 Premium UI**: Modern interface built with Material Design 3, featuring real-time progress reporting and system tray integration.
- **🔒 Secure**: Credentials are safely stored in your system's native keyring (Keychain, Credential Manager, or Secret Service).
- **🔄 Smart Deduplication**: Leverages Immich's bulk-upload-check API to verify assets before transmitting data.
- **📂 Folder Management**: Easily add or remove multiple watched folders from your local filesystem.

---

## 🛠️ Tech Stack

- **Backend**: [Rust](https://www.rust-lang.org/) with [Tauri](https://tauri.app/)
- **Frontend**: [SvelteKit](https://kit.svelte.dev/) + [TypeScript](https://www.typescriptlang.org/)
- **Styling**: [Material Design 3](https://m3.material.io/) (M3) components
- **Database**: [SQLite](https://sqlite.org/) (via [SQLx](https://github.com/launchbadge/sqlx)) for local sync state and caching
- **Networking**: [Reqwest](https://github.com/seanmonstar/reqwest) for robust HTTP/multipart streaming

---

## 🚀 Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- [Node.js](https://nodejs.org/) (v18+)
- OS-specific dependencies (see [Tauri documentation](https://tauri.app/v1/guides/getting-started/prerequisites))

### Installation

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

## ⚙️ How it Works

1. **Scan**: The app scans your watched folders for media files (`jpg`, `mp4`, `heic`, etc.).
2. **Hash**: It calculates a SHA-1 hash for each file. This is cached in a local SQLite database alongside file metadata (mtime/size) to avoid re-hashing unless the file changes.
3. **Verify**: Before uploading, it checks the Immich server in chunks of 500 to see which assets already exist.
4. **Stream**: Missing assets are streamed directly from disk to the API, ensuring low memory usage even for large video files.

---

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request or open an issue for bugs and feature requests.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE) (or specify your license).

---

<p align="center">
  Made with ❤️ for the Immich Community
</p>
