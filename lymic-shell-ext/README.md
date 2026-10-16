# Lymic shell extension

This Windows-only C++17 project produces Lymic's in-process Explorer thumbnail
provider. It is deliberately a standalone COM DLL so loading it does not load
the Tauri application or its Rust runtime into Explorer.

The provider obtains a metadata-only handle, reads placeholder identity through
`CfGetPlaceholderInfo`, and talks to `\\.\pipe\lymic-thumbnail-cache-v1`.
The binary v1 request contains the asset id, asset version, and requested width
and height. A response is either a cache miss or image bytes. It does not open
the placeholder data stream, download from Immich, or issue Cloud Files fetch
operations.

Build with a Visual Studio developer shell:

```powershell
cmake -S lymic-shell-ext -B lymic-shell-ext/build
cmake --build lymic-shell-ext/build --config Release
```

Installation and COM registration are intentionally excluded; ticket #56 owns
that integration.
