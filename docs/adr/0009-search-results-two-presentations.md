# ADR-0009: Search Results have a chronological and a relevance presentation

Search Results are presented in one of two ways, decided by the Search, not by the user. A Search without a Context text (filters only, or a Filename, Description, Folder Path or OCR text) uses Immich's metadata search and is presented chronologically in the shared timeline view, grouped into Timeline Buckets like the Album Timeline and Place Timeline (ADR-0006, ADR-0007). A Search with a Context text uses Immich's smart search, which orders by relevance, and is presented as a flat grid in that order, without Timeline Buckets and without a Timeline Scrubber. Further pages of a Context Search load while the user scrolls.

The chronological presentation does not load an unbounded result. It loads at most the 5 000 newest matches, newest first, and tells the user that the Search was cut and should be narrowed. This deliberately departs from ADR-0006, which loads every page before showing the Scrubber: an Album or a Place is bounded by what it contains, whereas a Search such as "Favorites" or one year can match most of the Immich Library.

## Considered Options

- **Group Context results chronologically**: Rejected because it discards the ordering that makes Context useful: a best match from 2014 would no longer be recognisable as the best match.
- **Present Context results chronologically only after taking the best N**: Rejected because the user would still see a relevance cut-off as an unexplained gap, and the Scrubber would imply a complete index that the result is not.
- **Load every metadata result as ADR-0006 does**: Rejected because the full result set is held in memory and a broad Search could match tens of thousands of Remote Assets.
- **Let the user choose the sort**: Rejected as unnecessary: Context has no chronological option from Immich and the other Search Modes are inherently chronological.

## Consequences

- The Timeline Scrubber stays valid for the chronological presentation: the loaded part is gap-free, from the newest Remote Asset back to the cut-off date, so the Scrubber's complete-index promise still holds for what is shown.
- A chronological Search that reaches the 5 000 limit shows a notice that only the newest matches are displayed. The limit is a constant, not a setting.
- A failed page in a chronological Search fails the whole load, as in ADR-0006. A failed further page of a Context Search keeps the pages already shown and offers retry, as All Memories does.
- Context needs text: Immich rejects a smart search without `query` or `queryAssetId`, so a Search with Context selected but no text runs as a chronological filter-only Search.
- A Context Search combined with Search Filters relies on Immich applying the filters to the smart search; whether it filters before or after ranking has not been verified against a live server and is recorded as an open verification gap.
- Lightbox actions (Favorite, Download, Add to Album) come from the same Remote Asset action owner as every other timeline.
