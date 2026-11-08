# ADR-0006: Album timelines use paginated metadata search

Album detail reuses the Photo Timeline presentation, but Immich does not expose
album-filtered timeline buckets. Lymic therefore retrieves an Album's Remote
Assets through paginated metadata search, groups their local capture times into
Timeline Buckets, and supplies that data to the shared timeline view.

## Consequences

- The full metadata result is retained in memory before the scrubber is shown,
  so a large Album is bounded by the available process memory and by Immich's
  response pagination. A failed later page deliberately fails the whole load;
  a partial chronological index would violate the scrubber contract.
- Pagination rejects a non-advancing next page to prevent an unbounded loop.
- Assets without a parseable local capture time are omitted from the
  chronological Album Timeline rather than placed in a misleading 1970 bucket.
