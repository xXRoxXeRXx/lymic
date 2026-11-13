# Map profiling fixtures

Generate UTF-8 Immich marker responses with 300,000 valid, uniquely identified
locations. On Windows PowerShell use `cmd /c` for redirection: Windows
PowerShell's `>` writes UTF-16, which is not the HTTP JSON fixture format.

```powershell
cmd /c "node scripts/map-fixtures/generate.mjs distributed > distributed.json"
cmd /c "node scripts/map-fixtures/generate.mjs regional > regional.json"
cmd /c "node scripts/map-fixtures/generate.mjs identical > identical.json"
```

`distributed` spreads points globally and includes zero coordinates, both sides
of the antimeridian, and high latitudes. `regional` distributes all points in a
small area around Berlin. `identical` places all points at one coordinate;
`dense` remains an alias for the old identical fixture. The seed is fixed. Invalid
coordinates must be tested separately because these workloads each require
300,000 valid locations.

Serve each response at `/api/map/markers` from a local Immich-compatible test
server, with test image/video metadata and thumbnail/media endpoints, and use a
packaged build. The included local server supplies synthetic media; see below. Do not
use a real account's locations as a fixture. Record the server implementation,
fixture hashes and request logging configuration with each run.

For each workload record machine model, CPU, RAM, GPU, OS, WebView version,
viewport, build revision, fixture and test server. Record marker transfer time
and bytes separately from decode/index construction, first useful display, warm
reopening and background refresh. Capture peak and steady additional process
memory across the Rust process and WebView processes, including workers, during
initial build and replacement. Record sustained frame rate during repeatable
pan/zoom with thumbnails active; report the sequence and duration. Exercise fast
pan, repeated navigation and refresh, light/dark theme, the antimeridian and
high latitudes; log concurrent and total thumbnail requests and compare process
memory after repeated visits. Inspect identical-coordinate results through the
last item, including image/video Lightbox actions and navigation. Check packaged
worker/CSP and OpenFreeMap resource loading, camera preservation, and absence of
provider controls. Report failures and missing measurements, not just passes.

The provisional goals are 30 FPS and at most 500 MiB additional process memory
on a documented 16 GB integrated-graphics laptop. No cold-load deadline was
agreed. These fixtures and component tests do not establish either goal: a real
packaged desktop benchmark and its evidence are still required.

## Local server

Start the loopback-only test server with the generated file. Optionally supply a
small MP4 to make odd-numbered fixture IDs video assets; otherwise all assets are
images. Provide your own non-private MP4 for video smoke testing.

```powershell
node scripts/map-fixtures/serve.mjs "$env:TEMP\identical.json" "$env:TEMP\fixture.mp4"
```

In an isolated OS user profile, connect the packaged app to
`http://127.0.0.1:3001`, explicitly allow loopback HTTP, and use API key
`fixture-key`. The app stores its account and preferences in the OS profile;
using a separate profile prevents overwriting a real account. Select Map View.
The server handles current user, marker snapshots, asset details, thumbnails,
original media, and favorite changes; timeline and album lists are empty.
Favorite state is held only until the server exits. Unhandled endpoints return
404 and are logged. This is a narrow test double, not an Immich replacement.

The marker response is streamed from disk and logged at start and close. On
Ctrl+C the server prints total requests, thumbnail requests, and peak concurrent thumbnail responses.
These server-side counts do not reveal queued WebView work or requests served
from Lymic's thumbnail cache; capture client-side activity and clear or account
for that cache between cold runs. The included 1-pixel PNG is deterministic,
but unrealistically cheap to transfer/decode: use representative media and a
real or suitably extended test server for performance acceptance. The optional
MP4 is loaded in server memory and does not model production media streaming.
Use `LYMIC_FIXTURE_PORT` to choose a different port if needed.

