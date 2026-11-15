# ADR-0007: Place Timelines use paginated metadata search by city

The Explore Overview lists Places from Immich's one-asset-per-city endpoint and opens each Place as a Place Timeline that reuses the Photo Timeline presentation. Immich's timeline-bucket endpoints filter by person, album membership, favorites and similar, but not by city. Lymic therefore retrieves a Place's Remote Assets through Immich's paginated metadata search filtered by city, groups their local capture times into Timeline Buckets, and supplies that data to the shared timeline view, exactly as ADR-0006 does for Album Timelines.

## Considered Options

- **Build a Place grid separate from the timeline**: Rejected because it would duplicate timeline masonry, lightbox, thumbnail-cache, and accessibility behavior.
- **Link out to Immich's web search for the city**: Rejected because Lymic is a desktop client that keeps browsing inside the application.
- **Wait for city-filtered timeline endpoints**: Rejected because they are not part of Immich's current API.

## Consequences

- The consequences of ADR-0006 apply to a Place Timeline: all result pages load before the scrubber is shown, a failed later page fails the whole load, a non-advancing next page is rejected, and assets without a parseable local capture time are omitted.
- A Place is identified by the city name Immich returns, and the metadata search filters by city name only (`POST /api/search/metadata` with `city`, `withExif`, `order`, `page` and `size`).
- **Same-named cities are not distinguished, and that is consistent.** Verified against Immich's server source (no live server was available): `GET /api/search/cities` picks one asset per distinct `asset_exif.city` value (a recursive walk over distinct city names; country and state are not part of the key), so Springfield (US) and Springfield (another country) appear as one Place. The metadata search filters with an exact, case-sensitive equality on `asset_exif.city`; country and state are separate optional filters that Lymic does not send. A Place Timeline therefore shows the assets of every city with that exact name, which is precisely the set the single Place tile stands for. Lymic does not split such Places, because `/search/cities` gives no country to split by and adding one would need a different, per-country listing. Because matching is exact, Lymic sends the city name as Immich returned it and the Place Timeline is titled with that name.
- A Place Timeline can contain videos and other asset types that the one-asset-per-city listing (images only) does not choose as the Place thumbnail.
- Immich API keys require `asset.read` for Places.
