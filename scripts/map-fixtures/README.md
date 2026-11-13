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

The included loopback server supplies synthetic media for functional testing;
see below. Do not use a real account's locations as a fixture. These datasets
and component tests do not establish frame-rate or memory guarantees.

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
from Lymic's thumbnail cache. The included 1-pixel PNG is deterministic,
but unrealistically cheap to transfer and decode. The optional
MP4 is loaded in server memory and does not model production media streaming.
Use `LYMIC_FIXTURE_PORT` to choose a different port if needed.

