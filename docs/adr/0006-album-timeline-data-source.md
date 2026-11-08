# ADR-0006: Album timelines use paginated metadata search

Album detail reuses the Photo Timeline presentation, but Immich does not expose album-filtered timeline buckets. Lymic therefore retrieves the Album's Remote Assets through Immich's paginated metadata search, groups their local capture times into Timeline Buckets, and supplies that data to the shared timeline view. This preserves one media-viewing interaction model while keeping Album listing and detail access read-only; Target Album synchronization remains a separate concern governed by ADR-0001.

## Considered Options

- **Fetch the album detail endpoint and use its assets**: Rejected because Immich's album detail response is metadata-only and does not provide an asset collection.
- **Build a separate album grid**: Rejected because it would duplicate timeline masonry, lightbox, thumbnail-cache, and accessibility behavior.
- **Wait for album-filtered timeline endpoints**: Rejected because they are not part of Immich's current API. Complete metadata pagination before exposing the scrubber preserves its existing complete-index semantics.

## Consequences

- An Album Timeline initially loads all result pages before presenting the full chronological index; thumbnails still load lazily through the existing streaming protocol and cache.
- Album cards use Immich's Album metadata, including the cover asset and asset count, and respect the server's Album ordering.
