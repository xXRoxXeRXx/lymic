# Immich map API compatibility evidence

Lymic does not currently document a minimum supported Immich version. The
following version-tagged upstream sources establish the marker contract for the
versions examined; they do not prove compatibility with every release.

- [v2.4.1 map controller](https://github.com/immich-app/immich/blob/v2.4.1/server/src/controllers/map.controller.ts) exposes authenticated `GET /api/map/markers` without a dedicated permission. Its [marker DTO](https://github.com/immich-app/immich/blob/v2.4.1/server/src/dtos/map.dto.ts) contains an ID, numeric `lat` and `lon`, and optional place information.
- [v2.5.0 map controller](https://github.com/immich-app/immich/blob/v2.5.0/server/src/controllers/map.controller.ts) requires `map.read` for the same endpoint; the [v2.5.0 marker DTO](https://github.com/immich-app/immich/blob/v2.5.0/server/src/dtos/map.dto.ts) retains those fields. The [v2.5.0 release announcement](https://github.com/immich-app/immich/discussions/25577) identifies `map.read` as a new fine-grained permission. Marker results are returned as one list and neither version's marker query DTO exposes pagination or a viewport.
- In v2.5.0, [asset details](https://github.com/immich-app/immich/blob/v2.5.0/server/src/controllers/asset.controller.ts) require `asset.read`; [thumbnails and video playback](https://github.com/immich-app/immich/blob/v2.5.0/server/src/controllers/asset-media.controller.ts) require `asset.view`, while downloading originals requires `asset.download`. Other Lightbox actions require their respective permissions.

These are source-contract checks, not tests against live servers. Test the
packaged application against the release versions actually supported before
claiming a compatibility range.
