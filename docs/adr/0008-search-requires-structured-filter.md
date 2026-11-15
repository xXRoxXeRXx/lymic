# ADR-0008: Search requires Immich 3.2 and sends only the structured filter

Lymic's Search combines many Search Filters (Recognized Persons, dates, Location, Tags, media type, camera, Rating, Favorite, Archived Asset, Unfiled Asset) with an optional text. Immich 3.2 introduced a structured `filter` object on its metadata and smart search endpoints: filters combine at the top level, people and tags take `any`, `all` or `none`, and a Remote Asset's absence from every Album is `albumIds.none`. Immich marks the older flat fields (`personIds`, `city`, `takenAfter`, `rating`, `isNotInAlbum` and the rest) as deprecated, and a request that mixes flat fields with `filter`, `orderBy` or `cursor` is rejected. Lymic's Search therefore requires Immich 3.2 or later and sends only the structured `filter`. Before the first Search it reads the server's version and features; an older server shows the Search as unsupported, with the required version, and leaves every other view untouched.

## Considered Options

- **Support both request shapes**: Rejected because every Search Filter would be built, tested and verified twice, and the two shapes differ in meaning (for example "not in any Album" has no structured flat equivalent).
- **Send only the flat fields**: Rejected because Immich has deprecated them and has no structured replacement for several combinations Lymic needs (`all` on people and tags).
- **Probe by trying the structured request and falling back on a 400**: Rejected because a fallback would hide the real reason a request failed and send credentials-bearing requests twice.

## Consequences

- The Search is unavailable on Immich servers older than 3.2. No other view is affected, because Photo Timeline, Album Timeline, Place Timeline and Explore keep their current requests.
- Album Timelines and Place Timelines still send the flat `albumIds` and `city` fields (ADR-0006, ADR-0007). They keep working while Immich accepts the deprecated fields. Moving them to the structured `filter` is possible once the minimum version applies to them, but is not part of introducing Search.
- The 3.2 cut-off comes from Immich's source annotations ("v3.2+"), not from a running server, and should be re-checked against a live 3.2 server and the first later release.
- Context and OCR Search Modes also depend on the server features `smartSearch` and `ocr`, read from `GET /api/server/features`; a disabled feature disables only that Search Mode.
- Search needs the API key permission `asset.read`. Person and Tag Search Filters additionally need `person.read` and `tag.read`; a key lacking one disables only that filter.
- The Search never mutates a Remote Asset, so it requires no write permission.
