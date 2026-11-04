# ADR-0005: Timeline media streaming protocol and local cache

## Status
Accepted

## Context
Displaying a photo timeline with ~300,000 assets requires efficient thumbnail rendering and video playback in the SvelteKit frontend without exhausting system memory or exposing authentication tokens. Immich assets require `x-api-key` and optional Cloudflare Access authentication, which are secured inside the native OS keyring. Passing raw credentials to the webview or transmitting images as base64 data URLs via Tauri IPC creates severe serialization latency and high memory overhead for thousands of visible or scrolled images. Uncontrolled parallel requests can saturate network and Immich server resources, while video playback requires partial content streaming (`Range` headers).

## Decision
1. Register a custom Tauri URI protocol (`lymic-asset://` and `lymic-thumb://`) in Rust. The Svelte webview renders images using `<img src="lymic-thumb://asset/<id>">` and plays video via `<video src="lymic-asset://video/<id>">`.
2. The protocol handlers intercept requests in Rust, inject stored authentication headers from the native keyring, and stream data directly from Immich. The video handler forwards HTTP 206 `Range: bytes=...` requests for video seeking.
3. Manage thumbnail downloads through an internal concurrency pool (maximum 12 concurrent requests) to avoid server saturation.
4. Introduce a local disk cache with LRU eviction and a configurable size limit (default 2 GB) managed by Rust for thumbnails. Incoming thumbnail requests serve cached files if available, fetching from Immich and populating the cache on cache miss.

## Consequences
- Credentials remain strictly within Rust and the native keyring; the webview never touches auth secrets.
- Images benefit from standard browser lazy loading, decode pipelining, and HTTP caching semantics without IPC base64 overhead.
- Cached thumbnails load instantly on subsequent viewings, significantly reducing server bandwidth and network latency.
- HTML5 video playback supports native seeking without downloading entire video files upfront.

