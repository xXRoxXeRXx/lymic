# Map profiling fixtures

`cmd /c "node scripts/map-fixtures/generate.mjs distributed > markers.json"` and
`cmd /c "node scripts/map-fixtures/generate.mjs dense > markers.json"` produce repeatable
Immich-shaped responses containing 300,000 valid coordinates. The dense fixture
uses identical coordinates to expose inseparable clusters. Serve each response
from a local Immich-compatible test server with the marker endpoint and use a
packaged build; do not use a real account's location data as a fixture.

Record the machine, RAM, GPU, OS, WebView version and viewport. Measure network
transfer separately from decode/index construction, first useful display, warm
reopen and refresh; capture process memory including Rust, WebView and workers,
including transient peaks, and frame rate while panning and zooming. Repeat with
thumbnails active. No real-WebView results have yet been recorded.
